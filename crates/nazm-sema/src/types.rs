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

use crate::enums::EnumId;
use crate::record::RecordId;

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
    Enum(EnumId),
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
            Self::Record(_) | Self::Enum(_) => None,
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
    /// The distinction `==` needs: `docs/spec.md` refuses equality on a record and on an
    /// enum for one reason — a field or payload may be a sequence, and equality would have
    /// to decide what that means.
    #[must_use]
    pub const fn is_user_defined(self) -> bool {
        matches!(self, Self::Record(_) | Self::Enum(_))
    }

    /// Whether values of this type are handles to shared mutable storage.
    ///
    /// Load-bearing rather than descriptive: it is what stops `==` being offered on a
    /// sequence, where it would have to mean either identity or contents and the
    /// language has not chosen.
    #[must_use]
    pub fn is_sequence(self) -> bool {
        matches!(self, Self::Ints | Self::Strs)
    }

    /// Whether a value of this type may be passed to a task.
    ///
    /// `Int`, `Bool` and `Str` are values, so a task gets its own copy. `Chan` is the
    /// primitive designed to be shared and its operations are the synchronisation. A
    /// sequence is a handle to unsynchronised shared mutable storage, and letting one
    /// cross would be a data race the language had invited.
    #[must_use]
    pub fn may_cross_into_a_task(self) -> bool {
        !self.is_sequence()
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
            _ => None,
        }
    }

    #[must_use]
    pub fn all_names() -> &'static str {
        "`Int`, `Bool`, `Str`, `Ints`, `Strs`, `Chan`"
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
            Type::ALL.len() * 2,
            "the help text lists a different number of types than ALL has"
        );
    }
}
