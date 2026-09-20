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
    /// `str_from_byte(b: Int) -> Str` — a one-byte `Str`.
    ///
    /// Fails with `N0405` unless `0 <= b <= 255`. This is how a program writes a byte it
    /// cannot type: v0.1 string literals have **no escapes** (a deliberate deferral in
    /// the lexer — an escape table is a decision about text encoding), so without this
    /// there is no way to produce a newline, and a compiler that cannot emit a newline
    /// cannot emit LLVM IR.
    StrFromByte,

    // --- sequences ---------------------------------------------------------------
    /// `ints_new() -> Ints` — an empty sequence.
    IntsNew,
    /// `ints_len(a: Ints) -> Int`
    IntsLen,
    /// `ints_get(a: Ints, i: Int) -> Int` — fails with `N0405` outside `0..len`.
    IntsGet,
    /// `ints_set(a: Ints, i: Int, v: Int) -> Int` — fails with `N0405` outside
    /// `0..len`. Returns the index written, because the language has no unit type and
    /// every function returns something; the index is the one answer that is always
    /// meaningful.
    IntsSet,
    /// `ints_push(a: Ints, v: Int) -> Int` — appends; returns the **new length**.
    IntsPush,
    /// `strs_new() -> Strs`
    StrsNew,
    /// `strs_len(a: Strs) -> Int`
    StrsLen,
    /// `strs_get(a: Strs, i: Int) -> Str`
    StrsGet,
    /// `strs_set(a: Strs, i: Int, v: Str) -> Int` — the index written, as `ints_set`.
    StrsSet,
    /// `strs_push(a: Strs, v: Str) -> Int` — appends; returns the new length.
    StrsPush,
    /// `ints_pop(a: Ints) -> Int` — removes and returns the last item.
    ///
    /// Fails with `N0405` when the sequence is empty. Without it a sequence cannot be
    /// used as a stack, and an explicit work stack is how this language's compiler
    /// traverses a tree without recursion — so this is not a convenience.
    IntsPop,
    /// `strs_pop(a: Strs) -> Str` — the same, for `Strs`.
    StrsPop,
    /// `str_join(parts: Strs, sep: Str) -> Str` — the parts in order, separated.
    ///
    /// Not a convenience. Building a large string with `str_concat` in a loop is
    /// quadratic in both time and memory, and nothing is ever freed — the compiler
    /// written in Nazm ran out of memory assembling its own output before this existed.
    /// One pass measures, one allocation holds the result.
    StrJoin,

    // --- the outside world -------------------------------------------------------
    //
    // **Ambient authority, and labelled as such.** These are callable from anywhere,
    // and nothing in the language restricts which functions may reach the file system
    // or the process arguments. That is a real limitation, not a design: see
    // `docs/spec.md`, *What the outside-world built-ins do and do not promise*. The
    // growth path is a capability parameter, and the built-ins are shaped so that
    // adding one changes what may call them rather than what they mean.
    /// `args_count() -> Int` — how many arguments the program was given, **excluding**
    /// the program's own name.
    ArgsCount,
    /// `arg(i: Int) -> Str` — argument `i`, counting from zero, excluding the program
    /// name. Fails with `N0405` outside `0..args_count()`.
    Arg,
    /// `read_file(path: Str) -> Str` — the file's bytes. Fails with `N0407`.
    ReadFile,
    /// `write_file(path: Str, data: Str) -> Int` — writes, truncating; returns the byte
    /// count. Fails with `N0407`.
    WriteFile,
    /// `file_exists(path: Str) -> Bool` — whether the path can be opened for reading.
    FileExists,
    /// `print(s: Str) -> Int` — bytes to standard output; returns the count.
    Print,
    /// `eprint(s: Str) -> Int` — bytes to standard error; returns the count.
    EPrint,
    /// `exit_with(code: Int) -> Int` — ends the process. Never returns, which the type
    /// system cannot say, so it is written `Int` and the value never arrives.
    ExitWith,
}

impl Intrinsic {
    /// Every built-in, for the checker to register and for tests to enumerate.
    pub const ALL: [Self; 27] = [
        Self::StrLen,
        Self::StrByte,
        Self::StrSlice,
        Self::StrConcat,
        Self::IntToStr,
        Self::StrFromByte,
        Self::IntsNew,
        Self::IntsLen,
        Self::IntsGet,
        Self::IntsSet,
        Self::IntsPush,
        Self::StrsNew,
        Self::StrsLen,
        Self::StrsGet,
        Self::StrsSet,
        Self::StrsPush,
        Self::IntsPop,
        Self::StrsPop,
        Self::StrJoin,
        Self::ArgsCount,
        Self::Arg,
        Self::ReadFile,
        Self::WriteFile,
        Self::FileExists,
        Self::Print,
        Self::EPrint,
        Self::ExitWith,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::StrLen => "str_len",
            Self::StrByte => "str_byte",
            Self::StrSlice => "str_slice",
            Self::StrConcat => "str_concat",
            Self::IntToStr => "int_to_str",
            Self::StrFromByte => "str_from_byte",
            Self::IntsNew => "ints_new",
            Self::IntsLen => "ints_len",
            Self::IntsGet => "ints_get",
            Self::IntsSet => "ints_set",
            Self::IntsPush => "ints_push",
            Self::StrsNew => "strs_new",
            Self::StrsLen => "strs_len",
            Self::StrsGet => "strs_get",
            Self::StrsSet => "strs_set",
            Self::StrsPush => "strs_push",
            Self::IntsPop => "ints_pop",
            Self::StrsPop => "strs_pop",
            Self::StrJoin => "str_join",
            Self::ArgsCount => "args_count",
            Self::Arg => "arg",
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::FileExists => "file_exists",
            Self::Print => "print",
            Self::EPrint => "eprint",
            Self::ExitWith => "exit_with",
        }
    }

    #[must_use]
    pub fn params(self) -> &'static [Type] {
        match self {
            Self::StrLen => &[Type::Str],
            Self::StrByte => &[Type::Str, Type::Int],
            Self::StrSlice => &[Type::Str, Type::Int, Type::Int],
            Self::StrConcat => &[Type::Str, Type::Str],
            Self::IntToStr | Self::StrFromByte => &[Type::Int],
            Self::IntsNew | Self::StrsNew => &[],
            Self::IntsLen | Self::IntsPop => &[Type::Ints],
            Self::StrsLen | Self::StrsPop => &[Type::Strs],
            Self::IntsGet => &[Type::Ints, Type::Int],
            Self::StrsGet => &[Type::Strs, Type::Int],
            Self::IntsSet => &[Type::Ints, Type::Int, Type::Int],
            Self::StrsSet => &[Type::Strs, Type::Int, Type::Str],
            Self::IntsPush => &[Type::Ints, Type::Int],
            Self::StrsPush => &[Type::Strs, Type::Str],
            Self::StrJoin => &[Type::Strs, Type::Str],
            Self::ArgsCount => &[],
            Self::Arg | Self::ExitWith => &[Type::Int],
            Self::ReadFile | Self::FileExists | Self::Print | Self::EPrint => &[Type::Str],
            Self::WriteFile => &[Type::Str, Type::Str],
        }
    }

    #[must_use]
    pub fn ret(self) -> Type {
        match self {
            Self::StrLen | Self::StrByte => Type::Int,
            Self::StrSlice | Self::StrConcat | Self::IntToStr | Self::StrFromByte => Type::Str,
            Self::IntsLen
            | Self::StrsLen
            | Self::IntsGet
            | Self::IntsSet
            | Self::IntsPush
            | Self::IntsPop
            | Self::StrsSet
            | Self::StrsPush => Type::Int,
            Self::StrsGet | Self::StrsPop | Self::StrJoin | Self::Arg | Self::ReadFile => Type::Str,
            Self::ArgsCount | Self::WriteFile | Self::Print | Self::EPrint | Self::ExitWith => {
                Type::Int
            }
            Self::FileExists => Type::Bool,
            Self::IntsNew => Type::Ints,
            Self::StrsNew => Type::Strs,
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
