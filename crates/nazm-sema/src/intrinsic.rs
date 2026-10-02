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

/// How one position of a built-in's signature is typed, in the table's own vocabulary.
///
/// A vocabulary rather than [`Type`] since N11, because a generic built-in's signature
/// names `Vec[T]`, and a `Vec` is an interned id that exists only inside one compilation —
/// so it cannot be written in a `const` table. `T` is the built-in's **own** parameter, and
/// the checker turns a shape into a [`Type`] once, when it builds a module's environment.
/// One inventory still: the checker, the interpreter and the backend all read this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    Int,
    Bool,
    Str,
    Ints,
    Strs,
    Chan,
    /// The built-in's type parameter.
    T,
    /// `Vec[T]`.
    VecT,
    /// `Chan[T]` (N53).
    ChanT,
    /// `Vec[Chan[T]]`: the channels a select waits on (N54).
    ChansT,
}

impl Shape {
    /// The monomorphic type this shape is, or `None` for one that mentions `T`.
    #[must_use]
    pub const fn monomorphic(self) -> Option<Type> {
        match self {
            Self::Int => Some(Type::Int),
            Self::Bool => Some(Type::Bool),
            Self::Str => Some(Type::Str),
            Self::Ints => Some(Type::Ints),
            Self::Strs => Some(Type::Strs),
            Self::Chan => Some(Type::Chan),
            Self::T | Self::VecT | Self::ChanT | Self::ChansT => None,
        }
    }

    /// How the shape is spelled in source, for tooling and for the checker's identity.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Str => "Str",
            Self::Ints => "Ints",
            Self::Strs => "Strs",
            Self::Chan => "Chan",
            Self::T => "T",
            Self::VecT => "Vec[T]",
            Self::ChanT => "Chan[T]",
            Self::ChansT => "Vec[Chan[T]]",
        }
    }
}

/// A built-in operation.
///
/// `Str` values are **immutable and byte-addressed**. See `docs/spec.md`, *Strings are
/// bytes, and what that commits to*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// `str_vouch(s: Str) -> Str` — its argument, with no origin (N52). Declassification:
    /// needs a `VouchCap` held where it is written, in every function, and every use is
    /// recorded with the origins it cleared. Makes no copy.
    StrVouch,

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

    // --- channels -----------------------------------------------------------------
    /// `chan_new(capacity: Int) -> Chan` — a channel holding at most `capacity` values.
    /// Fails with `N0405` for a capacity below 1.
    ChanNew,
    /// `chan_send(c: Chan, v: Int) -> Bool` — blocks while full; `false` when closed, in
    /// which case **the value was not sent**.
    ChanSend,
    /// `chan_recv(c: Chan, out: Ints) -> Bool` — blocks while empty; pushes one value
    /// onto `out` and returns `true`, or `false` when closed **and** drained.
    ChanRecv,
    /// `chan_close(c: Chan) -> Int` — idempotent; wakes every blocked sender and
    /// receiver. Closing is the cancellation mechanism.
    ChanClose,
    /// `chan_new_of[T](capacity: Int) -> Chan[T]` (N53) — `chan_new` for a `Chan[T]`.
    ChanNewOf,
    /// `chan_send_of[T](c: Chan[T], v: T) -> Bool` — a copy of `v` queued, or `false` and
    /// nothing queued when closed.
    ChanSendOf,
    /// `chan_recv_of[T](c: Chan[T], out: Vec[T]) -> Bool` — one value moved onto `out`, or
    /// `false` when closed and drained.
    ChanRecvOf,
    /// `chan_close_of[T](c: Chan[T]) -> Int` — `chan_close` for a `Chan[T]`.
    ChanCloseOf,
    /// `chan_select_of[T](cs: Vec[Chan[T]], out: Vec[T]) -> Int` (N54) — waits for the
    /// lowest-indexed ready channel: a value moved onto `out`, or a closed and drained one
    /// giving nothing. `-1` for no channels.
    ChanSelectOf,
    /// `chan_select_until[T](cs: Vec[Chan[T]], out: Vec[T], ms: Int) -> Int` (N54) —
    /// `chan_select_of`, or `-2` once at least `ms` milliseconds pass. Needs a `TimeCap`.
    ChanSelectUntil,
    /// `time_now_ms() -> Int` (N54) — a monotonic clock in milliseconds. Needs a `TimeCap`.
    TimeNowMs,
    /// `mmio_read32(addr: Int) -> Int` (N62) — one volatile 32-bit load from a device
    /// register, zero-extended. Needs an `MmioCap`, and exists only in a freestanding build.
    MmioRead32,
    /// `mmio_write32(addr: Int, value: Int) -> Int` (N62) — one volatile 32-bit store of
    /// `value`'s low half to a device register; `0`. Needs an `MmioCap`; freestanding only.
    MmioWrite32,
    /// `ints_sum_from(start: Int, v: Ints) -> Int` (N66) — `start` plus every element of `v`, left to
    /// right, failing exactly where one of those running sums would overflow. Vectorised in
    /// native builds without changing that.
    IntsSum,

    // --- the outside world -------------------------------------------------------
    //
    // **Authority, since N37.** A function that declares an effect set may call these
    // only while it holds an `IoCap` (`crate::capability`, `docs/spec.md`, *Capabilities*);
    // one that declares none is still ambient, exercising its caller's authority — a
    // compatibility bridge, labelled as such, and bounded so that code with a contract
    // cannot reach through it. The signatures did not change: the capability changes
    // what may call them rather than what they mean, which is how they were shaped.
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

    // --- `Vec[T]`, N11 --------------------------------------------------------------
    //
    // The first generic built-ins. Each has one type parameter, `T`, and follows the
    // sequence law of `Ints` and `Strs` with `T`'s own copy law for the elements:
    // `docs/spec.md`, *`Vec[T]` — a mutable handle, by the sequence law*.
    /// `vec_new[T]() -> Vec[T]` — an empty vector. `T` is written, because nothing else
    /// could say what it is.
    VecNew,
    /// `vec_len[T](xs: Vec[T]) -> Int`
    VecLen,
    /// `vec_get[T](xs: Vec[T], i: Int) -> T` — a copy of element `i`, by `T`'s law.
    /// Fails with `N0405` unless `0 <= i < len`.
    VecGet,
    /// `vec_set[T](xs: Vec[T], i: Int, v: T) -> Int` — replaces element `i` with a copy
    /// of `v` and returns `i`. The new value is taken before the old one is released.
    VecSet,
    /// `vec_push[T](xs: Vec[T], v: T) -> Int` — appends a copy of `v` and returns the new
    /// length. A failed growth leaves the vector as it was.
    VecPush,
    /// `vec_pop[T](xs: Vec[T]) -> T` — removes the last element and transfers it out.
    /// Fails with `N0405` on an empty vector.
    VecPop,
}

impl Intrinsic {
    /// Every built-in, for the checker to register and for tests to enumerate.
    pub const ALL: [Self; 48] = [
        Self::StrLen,
        Self::StrByte,
        Self::StrSlice,
        Self::StrConcat,
        Self::IntToStr,
        Self::StrFromByte,
        Self::StrVouch,
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
        Self::ChanNew,
        Self::ChanSend,
        Self::ChanRecv,
        Self::ChanClose,
        Self::ChanNewOf,
        Self::ChanSendOf,
        Self::ChanRecvOf,
        Self::ChanCloseOf,
        Self::ChanSelectOf,
        Self::ChanSelectUntil,
        Self::TimeNowMs,
        Self::MmioRead32,
        Self::MmioWrite32,
        Self::IntsSum,
        Self::ArgsCount,
        Self::Arg,
        Self::ReadFile,
        Self::WriteFile,
        Self::FileExists,
        Self::Print,
        Self::EPrint,
        Self::ExitWith,
        Self::VecNew,
        Self::VecLen,
        Self::VecGet,
        Self::VecSet,
        Self::VecPush,
        Self::VecPop,
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
            Self::StrVouch => "str_vouch",
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
            Self::ChanNew => "chan_new",
            Self::ChanSend => "chan_send",
            Self::ChanRecv => "chan_recv",
            Self::ChanClose => "chan_close",
            Self::ArgsCount => "args_count",
            Self::Arg => "arg",
            Self::ReadFile => "read_file",
            Self::WriteFile => "write_file",
            Self::FileExists => "file_exists",
            Self::Print => "print",
            Self::EPrint => "eprint",
            Self::ExitWith => "exit_with",
            Self::VecNew => "vec_new",
            Self::VecLen => "vec_len",
            Self::VecGet => "vec_get",
            Self::VecSet => "vec_set",
            Self::VecPush => "vec_push",
            Self::VecPop => "vec_pop",
            Self::ChanNewOf => "chan_new_of",
            Self::ChanSendOf => "chan_send_of",
            Self::ChanRecvOf => "chan_recv_of",
            Self::ChanCloseOf => "chan_close_of",
            Self::ChanSelectOf => "chan_select_of",
            Self::ChanSelectUntil => "chan_select_until",
            Self::TimeNowMs => "time_now_ms",
            Self::MmioRead32 => "mmio_read32",
            Self::MmioWrite32 => "mmio_write32",
            Self::IntsSum => "ints_sum_from",
        }
    }

    /// How many type parameters this built-in has: one for the `Vec` operations, none for
    /// everything else.
    #[must_use]
    pub const fn generics(self) -> u32 {
        match self {
            Self::VecNew
            | Self::VecLen
            | Self::VecGet
            | Self::VecSet
            | Self::VecPush
            | Self::VecPop
            | Self::ChanNewOf
            | Self::ChanSendOf
            | Self::ChanRecvOf
            | Self::ChanCloseOf
            | Self::ChanSelectOf
            | Self::ChanSelectUntil => 1,
            _ => 0,
        }
    }

    #[must_use]
    pub fn params(self) -> &'static [Shape] {
        match self {
            Self::StrLen | Self::StrVouch => &[Shape::Str],
            Self::StrByte => &[Shape::Str, Shape::Int],
            Self::StrSlice => &[Shape::Str, Shape::Int, Shape::Int],
            Self::StrConcat => &[Shape::Str, Shape::Str],
            Self::IntToStr | Self::StrFromByte => &[Shape::Int],
            Self::IntsNew | Self::StrsNew => &[],
            Self::IntsLen | Self::IntsPop => &[Shape::Ints],
            Self::StrsLen | Self::StrsPop => &[Shape::Strs],
            Self::IntsGet => &[Shape::Ints, Shape::Int],
            Self::StrsGet => &[Shape::Strs, Shape::Int],
            Self::IntsSet => &[Shape::Ints, Shape::Int, Shape::Int],
            Self::StrsSet => &[Shape::Strs, Shape::Int, Shape::Str],
            Self::IntsPush => &[Shape::Ints, Shape::Int],
            Self::StrsPush => &[Shape::Strs, Shape::Str],
            Self::StrJoin => &[Shape::Strs, Shape::Str],
            Self::ChanNew => &[Shape::Int],
            Self::ChanSend => &[Shape::Chan, Shape::Int],
            Self::ChanRecv => &[Shape::Chan, Shape::Ints],
            Self::ChanClose => &[Shape::Chan],
            Self::ArgsCount => &[],
            Self::Arg | Self::ExitWith => &[Shape::Int],
            Self::ReadFile | Self::FileExists | Self::Print | Self::EPrint => &[Shape::Str],
            Self::WriteFile => &[Shape::Str, Shape::Str],
            Self::VecNew => &[],
            Self::VecLen | Self::VecPop => &[Shape::VecT],
            Self::VecGet => &[Shape::VecT, Shape::Int],
            Self::VecSet => &[Shape::VecT, Shape::Int, Shape::T],
            Self::VecPush => &[Shape::VecT, Shape::T],
            Self::ChanNewOf => &[Shape::Int],
            Self::ChanSendOf => &[Shape::ChanT, Shape::T],
            Self::ChanRecvOf => &[Shape::ChanT, Shape::VecT],
            Self::ChanCloseOf => &[Shape::ChanT],
            Self::ChanSelectOf => &[Shape::ChansT, Shape::VecT],
            Self::ChanSelectUntil => &[Shape::ChansT, Shape::VecT, Shape::Int],
            Self::TimeNowMs => &[],
            Self::MmioRead32 => &[Shape::Int],
            Self::MmioWrite32 => &[Shape::Int, Shape::Int],
            Self::IntsSum => &[Shape::Int, Shape::Ints],
        }
    }

    #[must_use]
    pub fn ret(self) -> Shape {
        match self {
            Self::StrLen | Self::StrByte => Shape::Int,
            Self::StrSlice
            | Self::StrConcat
            | Self::IntToStr
            | Self::StrFromByte
            | Self::StrVouch => Shape::Str,
            Self::IntsLen
            | Self::StrsLen
            | Self::IntsGet
            | Self::IntsSet
            | Self::IntsPush
            | Self::IntsPop
            | Self::StrsSet
            | Self::StrsPush => Shape::Int,
            Self::StrsGet | Self::StrsPop | Self::StrJoin | Self::Arg | Self::ReadFile => {
                Shape::Str
            }
            Self::ArgsCount | Self::WriteFile | Self::Print | Self::EPrint | Self::ExitWith => {
                Shape::Int
            }
            Self::FileExists | Self::ChanSend | Self::ChanRecv => Shape::Bool,
            Self::ChanNew => Shape::Chan,
            Self::ChanClose => Shape::Int,
            Self::IntsNew => Shape::Ints,
            Self::StrsNew => Shape::Strs,
            Self::VecNew => Shape::VecT,
            Self::VecLen | Self::VecSet | Self::VecPush => Shape::Int,
            Self::VecGet | Self::VecPop => Shape::T,
            Self::ChanNewOf => Shape::ChanT,
            Self::ChanSendOf | Self::ChanRecvOf => Shape::Bool,
            Self::ChanCloseOf
            | Self::ChanSelectOf
            | Self::ChanSelectUntil
            | Self::TimeNowMs
            | Self::MmioRead32
            | Self::MmioWrite32
            | Self::IntsSum => Shape::Int,
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
