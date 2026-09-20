//! The v0.1 type system: three types, no inference across a function boundary.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    Str,
}

impl Type {
    /// Parse a written type name. `None` for anything else, so an unknown type is
    /// reported as such rather than silently treated as one of these.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Int" => Some(Self::Int),
            "Bool" => Some(Self::Bool),
            "Str" => Some(Self::Str),
            _ => None,
        }
    }

    #[must_use]
    pub fn all_names() -> &'static str {
        "`Int`, `Bool`, `Str`"
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Str => "Str",
        })
    }
}
