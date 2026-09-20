//! The type system: a fixed, concrete set, with no inference across a function boundary.
//!
//! # Why `Ints` and `Strs` rather than `[Int]` and `[Str]`
//!
//! A bootstrap compiler needs growable sequences of integers and of strings, and nothing
//! else (`docs/bootstrap-design.md`). Naming them as **ordinary type names** buys two
//! things: the lexer needs no bracket tokens and the parser needs no type grammar, so
//! neither acquires a shape that a future generic `[T]` would have to be compatible
//! with. Concrete names commit to nothing about how generics will eventually look, which
//! a half-built `[T]` syntax very much would.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Type {
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

    /// Parse a written type name. `None` for anything else, so an unknown type is
    /// reported as such rather than silently treated as one of these.
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

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Str => "Str",
            Self::Ints => "Ints",
            Self::Strs => "Strs",
            Self::Chan => "Chan",
        })
    }
}
