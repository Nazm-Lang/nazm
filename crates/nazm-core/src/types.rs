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
            _ => None,
        }
    }

    #[must_use]
    pub fn all_names() -> &'static str {
        "`Int`, `Bool`, `Str`, `Ints`, `Strs`"
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
        })
    }
}
