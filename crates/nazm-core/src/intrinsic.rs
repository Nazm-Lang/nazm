//! The built-in operations, in one table both implementations read.
//!
//! # Why functions and not syntax
//!
//! A bootstrap compiler needs to measure, index, slice, join and format text. Every one
//! of those could be an operator, a method or a piece of syntax; each of those choices
//! would be a language decision made to serve an implementation, and `docs/spec.md` would
//! grow a section per operation. A **built-in function** adds no syntax at all: it is an
//! ordinary call, checked by the ordinary call rule, and the only new thing a reader has
//! to learn is a name.
//!
//! # One table, two implementations
//!
//! The signature list below is the single source of truth. The checker resolves calls
//! against it, the interpreter evaluates them, and `nazm-lir` lowers them — so the three
//! cannot disagree about arity or types, which is the failure a hand-written list in each
//! would eventually produce.
//!
//! Names live in the ordinary function namespace and **may not be redefined**: a program
//! declaring `fn str_len` is rejected rather than silently shadowing the built-in, because
//! a call whose meaning depends on whether a definition happens to be in scope is the
//! thing this language is trying not to have.

use crate::types::Type;

/// A built-in operation.
///
/// `Str` values are **immutable and byte-addressed**. See `docs/spec.md`, *Strings are
/// bytes, and what that commits to*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intrinsic {
    /// `str_len(s: Str) -> Int` — length in **bytes**, not characters.
    StrLen,
    /// `str_byte(s: Str, i: Int) -> Int` — the byte at `i`, as `0..=255`.
    ///
    /// Fails with `N0405` when `i` is outside `0..len`. A byte, not a character: see the
    /// specification for why a language with no character type indexes bytes.
    StrByte,
    /// `str_slice(s: Str, from: Int, to: Int) -> Str` — the bytes in `[from, to)`.
    ///
    /// Fails with `N0405` unless `0 <= from <= to <= len`. Makes no copy.
    StrSlice,
    /// `str_concat(a: Str, b: Str) -> Str` — a new string, `a` then `b`.
    StrConcat,
    /// `int_to_str(n: Int) -> Str` — decimal, with a leading `-` when negative.
    IntToStr,
}

impl Intrinsic {
    /// Every built-in, for the checker to register and for tests to enumerate.
    pub const ALL: [Self; 5] = [
        Self::StrLen,
        Self::StrByte,
        Self::StrSlice,
        Self::StrConcat,
        Self::IntToStr,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::StrLen => "str_len",
            Self::StrByte => "str_byte",
            Self::StrSlice => "str_slice",
            Self::StrConcat => "str_concat",
            Self::IntToStr => "int_to_str",
        }
    }

    #[must_use]
    pub fn params(self) -> &'static [Type] {
        match self {
            Self::StrLen => &[Type::Str],
            Self::StrByte => &[Type::Str, Type::Int],
            Self::StrSlice => &[Type::Str, Type::Int, Type::Int],
            Self::StrConcat => &[Type::Str, Type::Str],
            Self::IntToStr => &[Type::Int],
        }
    }

    #[must_use]
    pub fn ret(self) -> Type {
        match self {
            Self::StrLen | Self::StrByte => Type::Int,
            Self::StrSlice | Self::StrConcat | Self::IntToStr => Type::Str,
        }
    }

    /// The built-in of this name, if there is one.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.name() == name)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_intrinsic_is_reachable_by_its_own_name() {
        // `ALL` and `by_name` are two lists in disguise, and a built-in present in one
        // and missing from the other is a call that checks and then cannot be evaluated.
        for i in Intrinsic::ALL {
            assert_eq!(Intrinsic::by_name(i.name()), Some(i), "{}", i.name());
        }
        assert_eq!(Intrinsic::by_name("str_nonexistent"), None);
        assert_eq!(Intrinsic::by_name("main"), None);
    }

    #[test]
    fn names_are_distinct() {
        let mut names: Vec<&str> = Intrinsic::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "two built-ins share a name");
    }
}
