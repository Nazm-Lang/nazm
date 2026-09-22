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

use crate::record::RecordId;

/// # Why a record is an id and not a shape
///
/// `Record(RecordId)` names a definition. It does **not** carry the field list, and that
/// is the nominality rule expressed as a representation: two records with identical
/// fields are different types, so a type that held its fields would compare equal where
/// the language says unequal. `docs/spec.md`, *Records are nominal*.
///
/// Everything a consumer wants to know about a record — its fields, whether it needs
/// cleanup, whether it may cross into a task, how it is laid out — is a method on
/// [`Records`](crate::record::Records), because every one of those answers is a property
/// of the *definitions in this compilation* rather than of the type name.
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
}

impl Type {
    /// Every **built-in** type, in the order a reader should meet them.
    ///
    /// The single list: [`Type::from_name`] and [`Type::all_names`] are both checked
    /// against it by a test. A type added to the enum and not to this list is a type the
    /// tooling cannot see.
    ///
    /// Records are not here and must not be: this is the fixed vocabulary every
    /// compilation has, and a record exists only in the compilation that declared it.
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
    /// `None` for a record, whose name is in its definition rather than in this enum.
    /// Deliberately not a total function returning a placeholder: a caller that can be
    /// handed a record has to say what it does about one, and
    /// [`Records::show`](crate::record::Records::show) is what most of them want.
    #[must_use]
    pub const fn builtin_name(self) -> Option<&'static str> {
        match self {
            Self::Int => Some("Int"),
            Self::Bool => Some("Bool"),
            Self::Str => Some("Str"),
            Self::Ints => Some("Ints"),
            Self::Strs => Some("Strs"),
            Self::Chan => Some("Chan"),
            Self::Record(_) => None,
        }
    }

    /// Whether this is a user-defined record.
    #[must_use]
    pub const fn is_record(self) -> bool {
        matches!(self, Self::Record(_))
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
    /// record name, so a user type is resolved against the module's own declarations
    /// rather than silently treated as one of these.
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
