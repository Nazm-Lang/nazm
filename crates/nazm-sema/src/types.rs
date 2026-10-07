//! The type system: a fixed, concrete set, with no inference across a function boundary.
//!
//! # Why `Ints` and `Strs` rather than `[Int]` and `[Str]`
//!
//! A bootstrap compiler needs growable sequences of integers and of strings, and nothing
//! else (`docs/bootstrap.md` §1). Naming them as **ordinary type names** buys two
//! things: the lexer needs no bracket tokens and the parser needs no type grammar, so
//! neither acquires a shape that a future generic `[T]` would have to be compatible
//! with. Concrete names commit to nothing about how generics will eventually look, which
//! a half-built `[T]` syntax very much would.

use crate::capability::CapabilityKind;
use crate::enums::EnumId;
use crate::intrinsic::Intrinsic;
use crate::record::RecordId;
use crate::resolve::FnId;

/// A `Vec[T]` of this compilation, by its session-local interned id.
///
/// Interned by element type, so two `Vec[Point]` written anywhere are one id and `==` on
/// [`Type`] is the language's type equality. Session-local like every id in this crate;
/// the durable spelling of a `Vec` is its element's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VecId(u32);

impl VecId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// A generic channel type of this compilation, `Chan[T]`, interned by element type (N53).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChanId(u32);

impl ChanId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// A function type of this compilation, by its session-local interned id (N50). Session-local
/// like [`VecId`]; the durable spelling of a function type is its parameters, result and effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FnTypeId(u32);

impl FnTypeId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// What a function type is: its parameters, its result and the effects a call performs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FnSignature {
    pub params: Vec<Type>,
    pub ret: Type,
    pub effects: crate::EffectSet,
    /// Whose effect parameter `effects` names, when it names one (N51); `None` otherwise,
    /// always — [`crate::UserTypes::fn_of`] keeps the two in step, so a type naming one
    /// function's parameter is never another's.
    pub row: Option<ParamOwner>,
}

impl FnSignature {
    /// A signature whose effects name no effect parameter.
    #[must_use]
    pub fn new(params: Vec<Type>, ret: Type, effects: crate::EffectSet) -> Self {
        Self {
            params,
            ret,
            effects,
            row: None,
        }
    }
}

/// Which declaration a type parameter belongs to.
///
/// A parameter is not a name: it is a **position in its owner's list**, and the owner is
/// part of it so that one function's `T` can never be mistaken for another's while one is
/// being inferred from the other. `docs/spec.md`, *Order is semantic; names are not*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamOwner {
    Function(FnId),
    Record(RecordId),
    Enum(EnumId),
    /// A generic built-in's own parameter, such as `vec_push`'s `T`.
    Builtin(Intrinsic),
    /// A trait's `Self`, position 0 (N78).
    Trait(crate::traits::TraitId),
}

/// One type parameter: its owner, and its position in the owner's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeParam {
    pub owner: ParamOwner,
    pub index: u32,
}

/// # Why a user-defined type is an id and not a shape
///
/// `Record(RecordId)` and `Enum(EnumId)` name definitions. Neither carries its fields or
/// its variants, and that is the nominality rule expressed as a representation: two
/// records with identical fields, or two enums with identical variants, are different
/// types, so a type that held its shape would compare equal where the language says
/// unequal. `docs/spec.md`, *Records are nominal* and *Enums*.
///
/// Everything a consumer wants to know about one — its fields, whether it needs cleanup,
/// whether it may cross into a task, how it is laid out — is a method on
/// [`UserTypes`](crate::UserTypes), because every one of those answers is a property of the
/// *definitions in this compilation* rather than of the type name. Since N10 it is
/// `UserTypes` rather than `Records` because those answers cross both kinds: a record field
/// may be an enum and an enum payload may be a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Type {
    Int,
    Bool,
    Str,
    /// A growable sequence of `Int`, **by reference**. See `docs/spec.md`, *Sequences
    /// are shared, mutable, and never freed*.
    Ints,
    /// A growable sequence of `Str`, by reference, with the same rules.
    Strs,
    /// A bounded channel of `Int`, by reference. The one type designed to be shared
    /// between tasks — see `docs/spec.md`, *Structured concurrency*.
    Chan,
    /// A user-defined record, by its session-local definition id.
    ///
    /// The first type whose meaning is not fixed by this enum. Its durable identity is a
    /// [`DefKey`](crate::DefKey) with [`DefKind::Record`](crate::DefKind), which is what
    /// a persisted interface writes; the id here is an index into one compilation and may
    /// never be serialised.
    Record(RecordId),
    /// A user-defined enum, by its session-local definition id.
    ///
    /// The same rules as [`Self::Record`], with [`DefKind::Enum`](crate::DefKind). It
    /// names a **definition**, never a set of variants: two enums with identical variants
    /// are different types, so a type that carried its variants would compare equal where
    /// the language says unequal.
    ///
    /// Which variant a *value* holds is not here and could not be. That is runtime state,
    /// and the whole of what `docs/spec.md` means by a sum type is that the fields which
    /// exist are decided then rather than now.
    ///
    /// Since N11 the id may also be an **applied** enum — `Maybe[Int]` — which the table
    /// interns as an entry of its own whose payloads are the definition's after
    /// substitution. Interning is what makes id equality type equality: one definition
    /// applied to one argument list is one id.
    Enum(EnumId),
    /// `Vec[T]`, interned by element type. N11. A mutable handle, by the sequence law.
    Vec(VecId),
    /// A type parameter, inside the declaration that owns it.
    Param(TypeParam),
    /// A capability: authority of one compiler-owned kind (N37). A value, freely copied,
    /// with no equality, that no expression can construct — the runtime hands the only
    /// ones to `main`. See [`crate::capability`].
    Cap(CapabilityKind),
    /// `fn(T…) -> R ! {e}` (N50), interned by parameters, result and effects, so `==` is the
    /// language's structural equality of function types. A handle to a closure object.
    Fn(FnTypeId),
    /// `Chan[T]` (N53), interned by element type: a bounded channel of `T`, by reference, for
    /// a `T` that may cross into a task. `Chan` alone is [`Self::Chan`], the `Int` channel.
    ChanOf(ChanId),
}

impl Type {
    /// Every **built-in** type, in the order a reader should meet them.
    ///
    /// The single list: [`Type::from_name`] and [`Type::all_names`] are both checked
    /// against it by a test. A type added to the enum and not to this list is a type the
    /// tooling cannot see.
    ///
    /// Records and enums are not here and must not be: this is the fixed vocabulary every
    /// compilation has, and a user type exists only in the compilation that declared it.
    /// `nazm capabilities` reports from this, which is why it lists no user type.
    pub const ALL: &'static [Self] = &[
        Self::Int,
        Self::Bool,
        Self::Str,
        Self::Ints,
        Self::Strs,
        Self::Chan,
        Self::Cap(CapabilityKind::Io),
        Self::Cap(CapabilityKind::Spawn),
    ];

    /// The written name of a **built-in** type, which is also how it is spelled in
    /// source.
    ///
    /// `None` for a user-defined type, whose name is in its definition rather than in this
    /// enum. Deliberately not a total function returning a placeholder: a caller that can
    /// be handed one has to say what it does about it, and
    /// [`UserTypes::show`](crate::UserTypes::show) is what most of them want.
    #[must_use]
    pub const fn builtin_name(self) -> Option<&'static str> {
        match self {
            Self::Int => Some("Int"),
            Self::Bool => Some("Bool"),
            Self::Str => Some("Str"),
            Self::Ints => Some("Ints"),
            Self::Strs => Some("Strs"),
            Self::Chan => Some("Chan"),
            Self::Cap(k) => Some(k.type_name()),
            Self::Record(_)
            | Self::Enum(_)
            | Self::Vec(_)
            | Self::Param(_)
            | Self::Fn(_)
            | Self::ChanOf(_) => None,
        }
    }

    /// Whether this is a user-defined record.
    #[must_use]
    pub const fn is_record(self) -> bool {
        matches!(self, Self::Record(_))
    }

    /// Whether this is a user-defined enum.
    #[must_use]
    pub const fn is_enum(self) -> bool {
        matches!(self, Self::Enum(_))
    }

    /// Whether this is a type some module declared, rather than one every module has.
    ///
    /// Until N13 this was the distinction `==` used to refuse every record and enum. Equality
    /// is derived now, from what a type contains — [`UserTypes::equality`](crate::UserTypes::equality)
    /// — and asking whether a type is user-defined is not the same question.
    #[must_use]
    pub const fn is_user_defined(self) -> bool {
        matches!(self, Self::Record(_) | Self::Enum(_))
    }

    /// Whether values of this type are handles to shared mutable storage.
    ///
    /// Load-bearing rather than descriptive: it is what stops a sequence crossing into a
    /// task. `==` is refused on one too, where it would have to mean either identity or
    /// contents — but that rule is [`UserTypes::equality`](crate::UserTypes::equality)'s,
    /// which lists the types with equality rather than the ones without.
    #[must_use]
    pub fn is_sequence(self) -> bool {
        matches!(self, Self::Ints | Self::Strs | Self::Vec(_))
    }

    /// Whether this is a capability type (N37).
    #[must_use]
    pub const fn is_capability(self) -> bool {
        matches!(self, Self::Cap(_))
    }

    /// Whether this is a type parameter, which supports only what every type supports.
    #[must_use]
    pub const fn is_param(self) -> bool {
        matches!(self, Self::Param(_))
    }

    /// Whether a value of this type may be passed to a task.
    ///
    /// `Int`, `Bool` and `Str` are values, so a task gets its own copy. `Chan` is the
    /// primitive designed to be shared and its operations are the synchronisation. A
    /// sequence is a handle to unsynchronised shared mutable storage, and letting one
    /// cross would be a data race the language had invited.
    ///
    /// A type parameter is refused too: nothing is known about it, and a generic function
    /// has no way to say it wants only types that may cross. `docs/spec.md`, *Generic
    /// definitions are checked once, parametrically*.
    #[must_use]
    pub fn may_cross_into_a_task(self) -> bool {
        // Nor a function value (N50): its type does not say what it captured.
        !self.is_sequence() && !self.is_param() && !matches!(self, Self::Fn(_))
    }

    /// Parse a written **built-in** type name. `None` for anything else, including every
    /// record and enum name, so a user type is resolved against the module's own
    /// declarations rather than silently treated as one of these.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Int" => Some(Self::Int),
            "Bool" => Some(Self::Bool),
            "Str" => Some(Self::Str),
            "Ints" => Some(Self::Ints),
            "Strs" => Some(Self::Strs),
            "Chan" => Some(Self::Chan),
            other => CapabilityKind::by_type_name(other).map(Self::Cap),
        }
    }

    /// Every built-in type a program can write, by name: [`Type::ALL`] in its order, then
    /// each capability type it does not hold.
    ///
    /// [`Type::ALL`] is the checker's fixed vocabulary — module scopes are built from it and
    /// the checker's identity hashes it — and has held two of the seven capability types
    /// since before the other five existed, while [`Type::from_name`] reads all seven. A
    /// person asking what they can write wants the seven, so the help text and
    /// `nazm capabilities` list this rather than [`Type::ALL`]: until 1.0 both said `IoCap`
    /// and `SpawnCap` and stopped.
    #[must_use]
    pub fn every_builtin_name() -> Vec<&'static str> {
        let mut names: Vec<&'static str> =
            Self::ALL.iter().filter_map(|t| t.builtin_name()).collect();
        for k in CapabilityKind::ALL {
            if !names.contains(&k.type_name()) {
                names.push(k.type_name());
            }
        }
        names
    }

    /// [`Type::every_builtin_name`], as help text writes a list of names.
    #[must_use]
    pub fn all_names() -> String {
        Self::every_builtin_name()
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::Type;

    #[test]
    fn every_type_is_in_all_and_parses_from_its_own_name() {
        // The list, the parser and the help text are three places one type has to be
        // mentioned. This is what stops a fourth type being added to two of them.
        for t in Type::ALL {
            let name = t.builtin_name().expect("ALL holds only built-in types");
            assert_eq!(
                Type::from_name(name),
                Some(*t),
                "`{name}` is in ALL but does not parse from its own name"
            );
            assert!(
                Type::all_names().contains(name),
                "`{name}` is in ALL but not in the help text"
            );
        }
        // Two backticks per name, so the help text names exactly as many types as
        // there are. Counted as pairs rather than divided, because the count being odd
        // would itself be a defect in the text and a division would hide it.
        assert_eq!(
            Type::all_names().matches('`').count(),
            Type::every_builtin_name().len() * 2,
            "the help text lists a different number of types than there are"
        );
    }

    #[test]
    fn every_capability_type_is_named_once_and_parses() {
        let names = Type::every_builtin_name();
        for k in crate::CapabilityKind::ALL {
            let name = k.type_name();
            assert_eq!(names.iter().filter(|n| **n == name).count(), 1, "{name}");
            assert_eq!(Type::from_name(name), Some(Type::Cap(k)), "{name}");
            assert!(Type::all_names().contains(&format!("`{name}`")), "{name}");
        }
        for name in &names {
            assert!(
                Type::from_name(name).is_some(),
                "`{name}` is named but does not parse"
            );
        }
    }
}
