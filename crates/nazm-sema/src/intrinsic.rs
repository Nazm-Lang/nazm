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

use crate::types::{NumKind, Type};

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
    /// `Box[T]` (R1a).
    BoxT,
    /// `Chain[T]` (R1a; `List[T]` until R1.5, D7).
    ListT,
    /// `Cell[T]` (R1b).
    CellT,
    /// `Option[T]`, the core prelude's (R1b, L13): what a checked operation returns.
    OptionT,
    /// `Vec[Chan[T]]`: the channels a select waits on (N54).
    ChansT,
    /// `Float64`, `Float32`, `UInt64` and `UInt32` (Gate 2): the monomorphic numeric shapes the
    /// bit-level float conversions name.
    F64,
    F32,
    U64,
    U32,
    /// `UInt128` (R1b, L13b): what `widening_mul` returns.
    U128,
    /// `UInt8`, `Bytes` and `Text` (Gate 2, `docs/general-purpose.md` §5–§6).
    U8,
    Bytes,
    Text,
    /// `OsHandle` (Gate 2, `docs/general-purpose.md` §8).
    OsHandle,
    /// `Atomic` (Gate 3, `docs/spec.md`, *Atomics*).
    Atomic,
}

/// Which numeric types a generic numeric built-in's `T` ranges over (Gate 2). Checked where
/// the call settles `T`, as `Equality` is for a bounded parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NumClass {
    /// Every integer type, `Int` included.
    Integer,
    /// `Float32` and `Float64`.
    Float,
    /// Every numeric type.
    Numeric,
}

impl NumClass {
    /// Whether `t` is in the class.
    #[must_use]
    pub const fn admits(self, t: Type) -> bool {
        match self {
            Self::Integer => t.is_integer(),
            Self::Float => t.is_float(),
            Self::Numeric => t.num().is_some(),
        }
    }

    /// The class, as a diagnostic names it.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Integer => "an integer type",
            Self::Float => "`Float32` or `Float64`",
            Self::Numeric => "a numeric type",
        }
    }
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
            Self::F64 => Some(Type::Num(NumKind::F64)),
            Self::F32 => Some(Type::Num(NumKind::F32)),
            Self::U64 => Some(Type::Num(NumKind::U64)),
            Self::U128 => Some(Type::Num(NumKind::U128)),
            Self::U32 => Some(Type::Num(NumKind::U32)),
            Self::U8 => Some(Type::Num(NumKind::U8)),
            Self::Bytes => Some(Type::Bytes),
            Self::Text => Some(Type::Text),
            Self::OsHandle => Some(Type::OsHandle),
            Self::Atomic => Some(Type::Atomic),
            Self::T
            | Self::VecT
            | Self::ChanT
            | Self::ChansT
            | Self::BoxT
            | Self::ListT
            | Self::CellT
            | Self::OptionT => None,
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
            Self::BoxT => "Box[T]",
            Self::ListT => "Chain[T]",
            Self::CellT => "Cell[T]",
            Self::OptionT => "Option[T]",
            Self::F64 => "Float64",
            Self::F32 => "Float32",
            Self::U64 => "UInt64",
            Self::U128 => "UInt128",
            Self::U32 => "UInt32",
            Self::U8 => "UInt8",
            Self::Bytes => "Bytes",
            Self::Text => "Text",
            Self::OsHandle => "OsHandle",
            Self::Atomic => "Atomic",
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
    /// `time_now_ns() -> Int`, `time_wall_ns() -> Int`, `time_sleep_ms(ms) -> Int` (Gate 2,
    /// `docs/general-purpose.md` §11): a monotonic clock in nanoseconds, the wall clock in
    /// nanoseconds since the Unix epoch, and a wait that parks on the pool. Behind `TimeCap`.
    TimeNowNs,
    TimeWallNs,
    TimeSleepMs,
    /// `mmio_read32(addr: Int) -> Int` (N62) — one volatile 32-bit load from a device
    /// register, zero-extended. Needs an `MmioCap`, and exists only in a freestanding build.
    MmioRead32,
    /// `mmio_write32(addr: Int, value: Int) -> Int` (N62) — one volatile 32-bit store of
    /// `value`'s low half to a device register; `0`. Needs an `MmioCap`; freestanding only.
    MmioWrite32,
    /// `mmio_read8(addr: Int) -> Int` (N86) — one volatile 8-bit load, zero-extended. As
    /// `mmio_read32`: an `MmioCap`, freestanding only.
    MmioRead8,
    /// `mmio_write8(addr: Int, value: Int) -> Int` (N86) — one volatile 8-bit store of
    /// `value`'s low byte; `0`.
    MmioWrite8,
    /// `mmio_read16(addr: Int) -> Int` (N86) — one volatile 16-bit load, zero-extended.
    MmioRead16,
    /// `mmio_write16(addr: Int, value: Int) -> Int` (N86) — one volatile 16-bit store of
    /// `value`'s low half; `0`.
    MmioWrite16,
    /// `mmio_read64(addr: Int) -> Int` (Gate 3) — one volatile 64-bit load.
    MmioRead64,
    /// `mmio_write64(addr: Int, value: Int) -> Int` (Gate 3) — one volatile 64-bit store of
    /// `value`; `0`.
    MmioWrite64,
    /// `watchdog_feed() -> Int` (Gate 3) — re-arms the board's watchdog for its manifest's ticks;
    /// `0`. A device's authority, as `mmio_*`; a board's alone.
    WatchdogFeed,
    /// `ints_sum_from(start: Int, v: Ints) -> Int` (N66) — `start` plus every element of `v`, left to
    /// right, failing exactly where one of those running sums would overflow. Vectorised in
    /// native builds without changing that.
    IntsSum,
    /// `c_errno() -> Int` (N85) — the `errno` the most recent foreign call on this thread
    /// left, captured as it returned. Performs `foreign`, so it needs a `ForeignCap`; hosted
    /// targets only.
    CErrno,
    /// Internal (N85): a pointer C returned where a handle or a `Str` was promised, failing
    /// with `N0409` when it is null. Made by Core IR lowering; no program can name it.
    ForeignNonNull,
    /// Internal (N85): a copy of the NUL-terminated bytes a non-null `const char *` C returned
    /// points to, as a `Str` Nazm owns. Made by Core IR lowering; no program can name it.
    CStrCopy,
    /// Internal (N85): the address of an export's C entry, named by its symbol — a constant
    /// `Str` argument — passed to C as a function pointer. Made by Core IR lowering for a
    /// callback argument; no program can name it.
    CallbackAddress,
    /// Internal (N87): `contract_check(cond: Bool, which: Int) -> Int` — `0` when `cond` holds;
    /// otherwise the call fails, `N0410` for a precondition (`which` 0) and `N0411` for a
    /// postcondition. Made by Core IR lowering for each contract clause; no program can name it.
    ContractCheck,

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

    // --- numbers, Gate 2 (`docs/general-purpose.md` §1–§3) -------------------------------
    //
    // Generic over a class of numeric types ([`Intrinsic::requires`]), so one name serves
    // every width; `T` is inferred from the arguments.
    /// `wrapping_add[T: integer](a: T, b: T) -> T` — modulo `2^n`, never trapping.
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    /// `saturating_add[T: integer](a: T, b: T) -> T` — clamped to `T`'s range.
    SaturatingAdd,
    SaturatingSub,
    SaturatingMul,
    /// `count_ones[T: integer](x: T) -> Int` — the number of set bits.
    CountOnes,
    LeadingZeros,
    TrailingZeros,
    /// `rotate_left[T: integer](x: T, n: Int) -> T` — `n` taken modulo the width; a negative
    /// `n` fails with `N0408`.
    RotateLeft,
    RotateRight,
    /// `swap_bytes[T: integer](x: T) -> T` — the bytes in reverse order.
    SwapBytes,
    /// `to_big_endian[T: integer](x: T) -> T` — `x` with its bytes in big-endian order, as
    /// an integer; and the inverses and little-endian forms. On a little-endian host
    /// `to_big_endian` is `swap_bytes` and `to_little_endian` is the identity.
    ToBigEndian,
    FromBigEndian,
    ToLittleEndian,
    FromLittleEndian,
    /// `float_sqrt[T: float](x: T) -> T` — correctly rounded.
    FloatSqrt,
    FloatAbs,
    FloatFloor,
    FloatCeil,
    FloatTrunc,
    /// Half away from zero.
    FloatRound,
    /// The C library's; not promised bit-identical across hosts.
    FloatSin,
    FloatCos,
    FloatTan,
    FloatExp,
    FloatLn,
    FloatLog2,
    FloatLog10,
    FloatPow,
    FloatAtan2,
    /// `float_min[T: float](a: T, b: T) -> T` — a NaN loses to a number.
    FloatMin,
    FloatMax,
    FloatIsNan,
    FloatIsInfinite,
    FloatIsFinite,
    /// `num_to_str[T: numeric](x: T) -> Str` — decimal; for a float, the shortest `%g`-style
    /// text that reads back to the same value.
    NumToStr,
    /// `float_to_str_fixed[T: float](x: T, digits: Int) -> Str` (R1a, L4) — exactly `digits`
    /// decimals, correctly rounded, ties to even, as C's `%.*f`; `N0405` unless
    /// `0 <= digits <= 30`.
    FloatToStrFixed,
    /// `str_pad_start(s: Str, width: Int, fill: Str) -> Str` (R1a, L4) — copies of the one-byte
    /// `fill` before `s` until it is `width` bytes; `N0405` unless `fill` is one byte.
    StrPadStart,
    /// `str_pad_end(s: Str, width: Int, fill: Str) -> Str` (R1a, L4) — the same, after `s`.
    StrPadEnd,
    /// `float64_to_bits(x: Float64) -> UInt64`, and the inverse and `Float32` forms.
    Float64ToBits,
    /// `widening_mul(a: UInt64, b: UInt64) -> UInt128` (R1b, L13b): the exact product, which
    /// never traps.
    WideningMul,
    Float64FromBits,
    Float32ToBits,
    Float32FromBits,
    /// `float_parses(s: Str) -> Bool` — whether `s` is a decimal floating-point number, or
    /// `inf`, `-inf` or `nan`; `@std/num`'s `num_parse_float` is the `Option` form.
    FloatParses,
    /// `float_parse_raw(s: Str) -> Float64` — the value `s` writes, or NaN when it is not one.
    FloatParseRaw,

    // --- bytes and text, Gate 2 (`docs/general-purpose.md` §5–§6) ------------------------
    //
    // A `Bytes` is a counted view of a fixed-length buffer: its built-ins read and write
    // through it, and a slice shares it. A `Text` is a `Str` known to be UTF-8: made only by
    // `Text(s)`, `try_convert[Text](s)`, a literal, or these, each of which keeps it so.
    BytesNew,
    BytesFromStr,
    BytesLen,
    BytesGet,
    BytesSet,
    BytesSlice,
    BytesToStr,
    BytesCopy,
    BytesReadLe,
    BytesReadBe,
    BytesWriteLe,
    BytesWriteBe,
    Utf8Valid,
    TextAsStr,
    TextLen,
    TextConcat,
    TextIsBoundary,
    TextSlice,
    TextScalarAt,
    TextNext,
    TextScalarCount,
    ScalarValid,
    ScalarText,
    /// `Text(s)`: `s`, checked to be UTF-8 (`N0413`). Not callable by name.
    TextFromStr,
    /// `try_convert[Text](s)`'s `Some`, after `utf8_valid`: `s`, unchecked. Not callable by name.
    TextAssume,

    // --- files and handles, Gate 2 (`docs/general-purpose.md` §8; `docs/spec.md`, *Files and
    // handles*) --------------------------------------------------------------------------
    //
    // Every one that can fail answers a status, `-errno`, which `@std/ioerror` turns into an
    // `IoError`; none traps on the system's refusal.
    OsOpenRead,
    OsOpenWrite,
    OsDirOpen,
    OsStdin,
    OsStatus,
    OsRead,
    /// A line of standard input (R1d, P3): up to and including its newline.
    OsReadLine,
    OsWrite,
    OsSeek,
    OsSync,
    OsClose,
    OsDirNext,
    OsStat,
    OsMkdir,
    OsRmdir,
    OsUnlink,
    OsRename,
    OsFamily,
    OsErrorText,

    // --- networking, Gate 2 (`docs/general-purpose.md` §9–§10; `docs/spec.md`, *Networking*) ---
    OsTcpListen,
    OsTcpAccept,
    OsTcpConnect,
    OsUdpBind,
    OsUdpSendTo,
    OsUdpRecvFrom,
    OsSetTimeout,
    OsShutdown,
    OsLocalAddr,
    OsPeerAddr,
    OsResolve,

    // --- process and environment, Gate 2 (`docs/general-purpose.md` §12; `docs/spec.md`,
    // *Process and environment*) -----------------------------------------------------------
    OsEnvGet,
    OsEnvHas,
    OsEnvCount,
    OsEnvAt,
    OsSpawn,
    OsChildPipe,
    OsWait,
    OsKill,

    // --- the platform, further, R1d (`docs/spec.md`, *The platform, further*) --------------
    //
    // Each answers a status as Gate 2's do, and each is a prelude name (R1a's rule): a module
    // that declares a function of the same name keeps its own.
    /// `os_stdout() -> OsHandle` — standard output, the same handle every call.
    OsStdout,
    /// `os_stderr() -> OsHandle` — standard errors, likewise.
    OsStderr,
    /// `os_spawn_in(program, args, dir, env) -> OsHandle` — a child in `dir` with exactly `env`.
    OsSpawnIn,
    /// `os_spawn_with(program, args, dir, env, flags) -> OsHandle` — and a new process group
    /// (`1`), a null standard input (`2`).
    OsSpawnWith,
    /// `os_pid() -> Int` — this process's id.
    OsPid,
    /// `os_child_pid(h) -> Int` — a child's process id.
    OsChildPid,
    /// `os_kill_group(pgid, sig) -> Int` — a signal to a process group.
    OsKillGroup,
    /// `os_try_wait(h) -> Int` — `os_wait`'s answer, or `WouldBlock` while the child runs.
    OsTryWait,
    /// `os_wait_until(h, deadline) -> Int` — `os_wait`'s answer, or `TimedOut` at `deadline`.
    OsWaitUntil,
    /// `os_child_rusage(h) -> Int` — a waited child's peak resident memory in bytes.
    OsChildRusage,
    /// `os_lstat(path, out) -> Int` — six words about `path` itself, a link not followed.
    OsLstat,
    /// `os_chmod(path, mode) -> Int`.
    OsChmod,
    /// `os_realpath(path, out) -> Int` — the canonical path's length.
    OsRealpath,
    /// `os_mkdtemp(parent) -> Str` — a new private directory's path, or `""`.
    OsMkdtemp,
    /// `os_lock(h, exclusive) -> Int` — an advisory whole-file lock, waited for.
    OsLock,
    /// `os_try_lock(h, exclusive) -> Int` — the same, or `WouldBlock`.
    OsTryLock,
    /// `os_unlock(h) -> Int`.
    OsUnlock,
    /// `os_random_bytes(buf: Bytes) -> Int` (Gate 2, §21): `buf` filled from the operating system's
    /// entropy; its length. Behind `RandomCap`, with no effect.
    OsRandomBytes,

    // --- atomics, Gate 3 (`docs/spec.md`, *Atomics*) -----------------------------------------
    /// `atomic_new(v: Int) -> Atomic` — a cell holding `v`.
    AtomicNew,
    /// `atomic_load(a: Atomic) -> Int`.
    AtomicLoad,
    /// `atomic_store(a: Atomic, v: Int) -> Int` — `0`.
    AtomicStore,
    /// `atomic_add(a: Atomic, d: Int) -> Int` — the value before; a sum outside `Int` stops
    /// the program with `N0400` and leaves the cell as it was.
    AtomicAdd,
    /// `atomic_swap(a: Atomic, v: Int) -> Int` — the value before.
    AtomicSwap,
    /// `atomic_compare_swap(a: Atomic, expected: Int, new: Int) -> Bool` — `new` stored, and
    /// `true`, exactly when the cell held `expected`.
    AtomicCompareSwap,

    // --- immutable indirection, R1a (`docs/spec.md`, *Immutable indirection*) -------------
    //
    // Prelude built-ins ([`Intrinsic::is_prelude`]): a module's own definition of one of these
    // names hides it. Core IR lowering writes each as the `Vec[T]` operations it means, so no
    // layer below the checker sees one. R1.5 (D7) renamed R1a's `List[T]` and `list_*` to
    // `Chain[T]` and `chain_*`; the `List…` variants keep their Rust names.
    /// `box_new[T](v: T) -> Box[T]` — one `T`, which nothing can replace.
    BoxNew,
    /// `box_get[T](b: Box[T]) -> T` — a copy of it, by `T`'s law.
    BoxGet,
    /// `chain_from_vec[T](xs: Vec[T]) -> Chain[T]` — `xs`'s elements as they are now, copied.
    ListFromVec,
    /// `chain_empty[T]() -> Chain[T]` — no elements. `T` is written.
    ListEmpty,
    /// `chain_len[T](l: Chain[T]) -> Int`
    ListLen,
    /// `chain_get[T](l: Chain[T], i: Int) -> T` — fails with `N0405` unless `0 <= i < len`.
    ListGet,
    /// Internal (R1a): a `Box[T]` with nothing in it yet, which [`Self::BoxFill`] fills before
    /// anything else sees it. Made by Core IR lowering for `box_new`; no program can name it.
    BoxFresh,
    /// Internal (R1a): `box`'s one element, `v`, written while the box is still the lowering's
    /// alone. Made by Core IR lowering for `box_new`; no program can name it.
    BoxFill,
    /// Internal (R1a): `v` appended to a `Chain[T]` that is still the lowering's alone. Made by
    /// Core IR lowering for `chain_from_vec`; no program can name it.
    ListPush,

    // --- a mutable cell, R1b (`docs/spec.md`, *A mutable cell*) ------------------------------
    //
    // Prelude built-ins, as R1a's are. Core IR lowering writes each as the `Vec[T]` operation it
    // means on a vector of one element, so no layer below the checker sees one.
    /// `cell_new[T](v: T) -> Cell[T]` — one `T`, which `cell_set` replaces.
    CellNew,
    /// `cell_get[T](c: Cell[T]) -> T` — a copy of it, by `T`'s law.
    CellGet,
    /// `cell_set[T](c: Cell[T], v: T) -> Int` — `v` stored in place of what was there; 0.
    CellSet,

    // --- checked arithmetic, R1b (`docs/spec.md`, *Integer arithmetic*, L13) ---------------
    //
    // Prelude built-ins over every integer type. Core IR lowering writes each from the wrapping,
    // saturating and dividing operations it means, so no layer below the checker sees one.
    /// `checked_add[T](a: T, b: T) -> Option[T]` — `Some` of the sum, `None` where it does not fit.
    CheckedAdd,
    /// `checked_sub[T](a: T, b: T) -> Option[T]` — `Some` of the difference, or `None`.
    CheckedSub,
    /// `checked_mul[T](a: T, b: T) -> Option[T]` — `Some` of the product, or `None`.
    CheckedMul,
    /// Internal (R1b, D2): `s = str_concat(s, t)` on a `let mut` binding, as Core IR lowering
    /// writes it. The same bytes as `str_concat`; natively it appends in place when `s`'s storage
    /// is its own and has room, and otherwise copies into storage with room to grow, so a string
    /// built by repeated appends costs linear time. No program can name it.
    StrAppend,
    /// Internal (R1, 5b.9): `str_concat(s, num_to_str[T](n))` for an integer `T`, as an
    /// interpolation's lowering writes it. The same bytes; natively one allocation, the digits
    /// written straight after `s`'s, rather than a string for the number and then a copy of it.
    /// No program can name it.
    StrConcatNum,
}

impl Intrinsic {
    /// Every built-in, for the checker to register and for tests to enumerate.
    pub const ALL: [Self; 203] = [
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
        Self::TimeNowNs,
        Self::TimeWallNs,
        Self::TimeSleepMs,
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
        Self::CErrno,
        Self::MmioRead8,
        Self::MmioWrite8,
        Self::MmioRead16,
        Self::MmioWrite16,
        Self::MmioRead64,
        Self::MmioWrite64,
        Self::WatchdogFeed,
        Self::WrappingAdd,
        Self::WrappingSub,
        Self::WrappingMul,
        Self::SaturatingAdd,
        Self::SaturatingSub,
        Self::SaturatingMul,
        Self::CountOnes,
        Self::LeadingZeros,
        Self::TrailingZeros,
        Self::RotateLeft,
        Self::RotateRight,
        Self::SwapBytes,
        Self::ToBigEndian,
        Self::FromBigEndian,
        Self::ToLittleEndian,
        Self::FromLittleEndian,
        Self::FloatSqrt,
        Self::FloatAbs,
        Self::FloatFloor,
        Self::FloatCeil,
        Self::FloatTrunc,
        Self::FloatRound,
        Self::FloatSin,
        Self::FloatCos,
        Self::FloatTan,
        Self::FloatExp,
        Self::FloatLn,
        Self::FloatLog2,
        Self::FloatLog10,
        Self::FloatPow,
        Self::FloatAtan2,
        Self::FloatMin,
        Self::FloatMax,
        Self::FloatIsNan,
        Self::FloatIsInfinite,
        Self::FloatIsFinite,
        Self::NumToStr,
        Self::FloatToStrFixed,
        Self::StrPadStart,
        Self::StrPadEnd,
        Self::Float64ToBits,
        Self::WideningMul,
        Self::Float64FromBits,
        Self::Float32ToBits,
        Self::Float32FromBits,
        Self::FloatParses,
        Self::FloatParseRaw,
        Self::BytesNew,
        Self::BytesFromStr,
        Self::BytesLen,
        Self::BytesGet,
        Self::BytesSet,
        Self::BytesSlice,
        Self::BytesToStr,
        Self::BytesCopy,
        Self::BytesReadLe,
        Self::BytesReadBe,
        Self::BytesWriteLe,
        Self::BytesWriteBe,
        Self::Utf8Valid,
        Self::TextAsStr,
        Self::TextLen,
        Self::TextConcat,
        Self::TextIsBoundary,
        Self::TextSlice,
        Self::TextScalarAt,
        Self::TextNext,
        Self::TextScalarCount,
        Self::ScalarValid,
        Self::ScalarText,
        Self::OsOpenRead,
        Self::OsOpenWrite,
        Self::OsDirOpen,
        Self::OsStdin,
        Self::OsStatus,
        Self::OsRead,
        Self::OsReadLine,
        Self::OsWrite,
        Self::OsSeek,
        Self::OsSync,
        Self::OsClose,
        Self::OsDirNext,
        Self::OsStat,
        Self::OsMkdir,
        Self::OsRmdir,
        Self::OsUnlink,
        Self::OsRename,
        Self::OsFamily,
        Self::OsErrorText,
        Self::OsTcpListen,
        Self::OsTcpAccept,
        Self::OsTcpConnect,
        Self::OsUdpBind,
        Self::OsUdpSendTo,
        Self::OsUdpRecvFrom,
        Self::OsSetTimeout,
        Self::OsShutdown,
        Self::OsLocalAddr,
        Self::OsPeerAddr,
        Self::OsResolve,
        Self::OsEnvGet,
        Self::OsEnvHas,
        Self::OsEnvCount,
        Self::OsEnvAt,
        Self::OsSpawn,
        Self::OsChildPipe,
        Self::OsWait,
        Self::OsKill,
        Self::OsStdout,
        Self::OsStderr,
        Self::OsSpawnIn,
        Self::OsSpawnWith,
        Self::OsPid,
        Self::OsChildPid,
        Self::OsKillGroup,
        Self::OsTryWait,
        Self::OsWaitUntil,
        Self::OsChildRusage,
        Self::OsLstat,
        Self::OsChmod,
        Self::OsRealpath,
        Self::OsMkdtemp,
        Self::OsLock,
        Self::OsTryLock,
        Self::OsUnlock,
        Self::OsRandomBytes,
        Self::AtomicNew,
        Self::AtomicLoad,
        Self::AtomicStore,
        Self::AtomicAdd,
        Self::AtomicSwap,
        Self::AtomicCompareSwap,
        Self::BoxNew,
        Self::BoxGet,
        Self::ListFromVec,
        Self::ListEmpty,
        Self::ListLen,
        Self::ListGet,
        Self::CellNew,
        Self::CellGet,
        Self::CellSet,
        Self::CheckedAdd,
        Self::CheckedSub,
        Self::CheckedMul,
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
            Self::TimeNowNs => "time_now_ns",
            Self::TimeWallNs => "time_wall_ns",
            Self::TimeSleepMs => "time_sleep_ms",
            Self::MmioRead32 => "mmio_read32",
            Self::MmioWrite32 => "mmio_write32",
            Self::MmioRead8 => "mmio_read8",
            Self::MmioWrite8 => "mmio_write8",
            Self::MmioRead16 => "mmio_read16",
            Self::MmioWrite16 => "mmio_write16",
            Self::MmioRead64 => "mmio_read64",
            Self::MmioWrite64 => "mmio_write64",
            Self::WatchdogFeed => "watchdog_feed",
            Self::IntsSum => "ints_sum_from",
            Self::CErrno => "c_errno",
            Self::ForeignNonNull => "foreign_nonnull",
            Self::CStrCopy => "cstr_copy",
            Self::CallbackAddress => "callback_address",
            Self::ContractCheck => "contract_check",
            Self::WrappingAdd => "wrapping_add",
            Self::WrappingSub => "wrapping_sub",
            Self::WrappingMul => "wrapping_mul",
            Self::SaturatingAdd => "saturating_add",
            Self::SaturatingSub => "saturating_sub",
            Self::SaturatingMul => "saturating_mul",
            Self::CountOnes => "count_ones",
            Self::LeadingZeros => "leading_zeros",
            Self::TrailingZeros => "trailing_zeros",
            Self::RotateLeft => "rotate_left",
            Self::RotateRight => "rotate_right",
            Self::SwapBytes => "swap_bytes",
            Self::ToBigEndian => "to_big_endian",
            Self::FromBigEndian => "from_big_endian",
            Self::ToLittleEndian => "to_little_endian",
            Self::FromLittleEndian => "from_little_endian",
            Self::FloatSqrt => "float_sqrt",
            Self::FloatAbs => "float_abs",
            Self::FloatFloor => "float_floor",
            Self::FloatCeil => "float_ceil",
            Self::FloatTrunc => "float_trunc",
            Self::FloatRound => "float_round",
            Self::FloatSin => "float_sin",
            Self::FloatCos => "float_cos",
            Self::FloatTan => "float_tan",
            Self::FloatExp => "float_exp",
            Self::FloatLn => "float_ln",
            Self::FloatLog2 => "float_log2",
            Self::FloatLog10 => "float_log10",
            Self::FloatPow => "float_pow",
            Self::FloatAtan2 => "float_atan2",
            Self::FloatMin => "float_min",
            Self::FloatMax => "float_max",
            Self::FloatIsNan => "float_is_nan",
            Self::FloatIsInfinite => "float_is_infinite",
            Self::FloatIsFinite => "float_is_finite",
            Self::NumToStr => "num_to_str",
            Self::FloatToStrFixed => "float_to_str_fixed",
            Self::StrPadStart => "str_pad_start",
            Self::StrPadEnd => "str_pad_end",
            Self::Float64ToBits => "float64_to_bits",
            Self::WideningMul => "widening_mul",
            Self::Float64FromBits => "float64_from_bits",
            Self::Float32ToBits => "float32_to_bits",
            Self::Float32FromBits => "float32_from_bits",
            Self::FloatParses => "float_parses",
            Self::FloatParseRaw => "float_parse_raw",
            Self::BytesNew => "bytes_new",
            Self::BytesFromStr => "bytes_from_str",
            Self::BytesLen => "bytes_len",
            Self::BytesGet => "bytes_get",
            Self::BytesSet => "bytes_set",
            Self::BytesSlice => "bytes_slice",
            Self::BytesToStr => "bytes_to_str",
            Self::BytesCopy => "bytes_copy",
            Self::BytesReadLe => "bytes_read_le",
            Self::BytesReadBe => "bytes_read_be",
            Self::BytesWriteLe => "bytes_write_le",
            Self::BytesWriteBe => "bytes_write_be",
            Self::Utf8Valid => "utf8_valid",
            Self::TextAsStr => "text_as_str",
            Self::TextLen => "text_len",
            Self::TextConcat => "text_concat",
            Self::TextIsBoundary => "text_is_boundary",
            Self::TextSlice => "text_slice",
            Self::TextScalarAt => "text_scalar_at",
            Self::TextNext => "text_next",
            Self::TextScalarCount => "text_scalar_count",
            Self::ScalarValid => "scalar_valid",
            Self::ScalarText => "scalar_text",
            Self::OsOpenRead => "os_open_read",
            Self::OsOpenWrite => "os_open_write",
            Self::OsDirOpen => "os_dir_open",
            Self::OsStdin => "os_stdin",
            Self::OsStatus => "os_status",
            Self::OsRead => "os_read",
            Self::OsReadLine => "os_read_line",
            Self::OsWrite => "os_write",
            Self::OsSeek => "os_seek",
            Self::OsSync => "os_sync",
            Self::OsClose => "os_close",
            Self::OsDirNext => "os_dir_next",
            Self::OsStat => "os_stat",
            Self::OsMkdir => "os_mkdir",
            Self::OsRmdir => "os_rmdir",
            Self::OsUnlink => "os_unlink",
            Self::OsRename => "os_rename",
            Self::OsFamily => "os_family",
            Self::OsErrorText => "os_error_text",
            Self::OsTcpListen => "os_tcp_listen",
            Self::OsTcpAccept => "os_tcp_accept",
            Self::OsTcpConnect => "os_tcp_connect",
            Self::OsUdpBind => "os_udp_bind",
            Self::OsUdpSendTo => "os_udp_send_to",
            Self::OsUdpRecvFrom => "os_udp_recv_from",
            Self::OsSetTimeout => "os_set_timeout",
            Self::OsShutdown => "os_shutdown",
            Self::OsLocalAddr => "os_local_addr",
            Self::OsPeerAddr => "os_peer_addr",
            Self::OsResolve => "os_resolve",
            Self::OsEnvGet => "os_env_get",
            Self::OsEnvHas => "os_env_has",
            Self::OsEnvCount => "os_env_count",
            Self::OsEnvAt => "os_env_at",
            Self::OsSpawn => "os_spawn",
            Self::OsChildPipe => "os_child_pipe",
            Self::OsWait => "os_wait",
            Self::OsKill => "os_kill",
            Self::OsStdout => "os_stdout",
            Self::OsStderr => "os_stderr",
            Self::OsSpawnIn => "os_spawn_in",
            Self::OsSpawnWith => "os_spawn_with",
            Self::OsPid => "os_pid",
            Self::OsChildPid => "os_child_pid",
            Self::OsKillGroup => "os_kill_group",
            Self::OsTryWait => "os_try_wait",
            Self::OsWaitUntil => "os_wait_until",
            Self::OsChildRusage => "os_child_rusage",
            Self::OsLstat => "os_lstat",
            Self::OsChmod => "os_chmod",
            Self::OsRealpath => "os_realpath",
            Self::OsMkdtemp => "os_mkdtemp",
            Self::OsLock => "os_lock",
            Self::OsTryLock => "os_try_lock",
            Self::OsUnlock => "os_unlock",
            Self::OsRandomBytes => "os_random_bytes",
            Self::AtomicNew => "atomic_new",
            Self::AtomicLoad => "atomic_load",
            Self::AtomicStore => "atomic_store",
            Self::AtomicAdd => "atomic_add",
            Self::AtomicSwap => "atomic_swap",
            Self::AtomicCompareSwap => "atomic_compare_swap",
            Self::BoxNew => "box_new",
            Self::BoxGet => "box_get",
            Self::ListFromVec => "chain_from_vec",
            Self::ListEmpty => "chain_empty",
            Self::ListLen => "chain_len",
            Self::ListGet => "chain_get",
            Self::BoxFresh => "box_fresh",
            Self::BoxFill => "box_fill",
            Self::ListPush => "chain_push",
            Self::CellNew => "cell_new",
            Self::CellGet => "cell_get",
            Self::CellSet => "cell_set",
            Self::CheckedAdd => "checked_add",
            Self::CheckedSub => "checked_sub",
            Self::CheckedMul => "checked_mul",
            Self::StrAppend => "str_append",
            Self::StrConcatNum => "str_concat_num",
            Self::TextFromStr => "text_from_str",
            Self::TextAssume => "text_assume",
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
            | Self::ChanSelectUntil
            | Self::BoxNew
            | Self::BoxGet
            | Self::ListFromVec
            | Self::ListEmpty
            | Self::ListLen
            | Self::ListGet
            | Self::BoxFresh
            | Self::BoxFill
            | Self::ListPush
            | Self::CellNew
            | Self::CellGet
            | Self::CellSet => 1,
            other if other.requires().is_some() => 1,
            _ => 0,
        }
    }

    /// The class a generic numeric built-in's `T` must be in (Gate 2), or `None` for every
    /// other built-in.
    #[must_use]
    pub const fn requires(self) -> Option<NumClass> {
        match self {
            Self::WrappingAdd
            | Self::WrappingSub
            | Self::WrappingMul
            | Self::SaturatingAdd
            | Self::SaturatingSub
            | Self::SaturatingMul
            | Self::CountOnes
            | Self::LeadingZeros
            | Self::TrailingZeros
            | Self::RotateLeft
            | Self::RotateRight
            | Self::SwapBytes
            | Self::ToBigEndian
            | Self::FromBigEndian
            | Self::ToLittleEndian
            | Self::FromLittleEndian
            | Self::CheckedAdd
            | Self::CheckedSub
            | Self::CheckedMul => Some(NumClass::Integer),
            Self::FloatSqrt
            | Self::FloatAbs
            | Self::FloatFloor
            | Self::FloatCeil
            | Self::FloatTrunc
            | Self::FloatRound
            | Self::FloatSin
            | Self::FloatCos
            | Self::FloatTan
            | Self::FloatExp
            | Self::FloatLn
            | Self::FloatLog2
            | Self::FloatLog10
            | Self::FloatPow
            | Self::FloatAtan2
            | Self::FloatMin
            | Self::FloatMax
            | Self::FloatIsNan
            | Self::FloatIsInfinite
            | Self::FloatIsFinite
            | Self::FloatToStrFixed => Some(NumClass::Float),
            Self::NumToStr
            | Self::StrConcatNum
            | Self::BytesReadLe
            | Self::BytesReadBe
            | Self::BytesWriteLe
            | Self::BytesWriteBe => Some(NumClass::Numeric),
            _ => None,
        }
    }

    #[must_use]
    pub fn params(self) -> &'static [Shape] {
        match self {
            Self::StrLen | Self::StrVouch => &[Shape::Str],
            Self::StrByte => &[Shape::Str, Shape::Int],
            Self::StrSlice => &[Shape::Str, Shape::Int, Shape::Int],
            Self::StrConcat | Self::StrAppend => &[Shape::Str, Shape::Str],
            Self::StrPadStart | Self::StrPadEnd => &[Shape::Str, Shape::Int, Shape::Str],
            Self::FloatToStrFixed => &[Shape::T, Shape::Int],
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
            Self::TimeNowNs => &[],
            Self::TimeWallNs => &[],
            Self::TimeSleepMs => &[Shape::Int],
            Self::MmioRead32 | Self::MmioRead8 | Self::MmioRead16 | Self::MmioRead64 => {
                &[Shape::Int]
            }
            Self::MmioWrite32 | Self::MmioWrite8 | Self::MmioWrite16 | Self::MmioWrite64 => {
                &[Shape::Int, Shape::Int]
            }
            Self::WatchdogFeed => &[],
            Self::IntsSum => &[Shape::Int, Shape::Ints],
            Self::CErrno => &[],
            Self::ForeignNonNull | Self::CStrCopy => &[Shape::Int],
            Self::CallbackAddress => &[Shape::Str],
            Self::ContractCheck => &[Shape::Bool, Shape::Int],
            Self::WrappingAdd
            | Self::WrappingSub
            | Self::WrappingMul
            | Self::SaturatingAdd
            | Self::SaturatingSub
            | Self::SaturatingMul
            | Self::FloatPow
            | Self::FloatAtan2
            | Self::FloatMin
            | Self::FloatMax => &[Shape::T, Shape::T],
            Self::RotateLeft | Self::RotateRight => &[Shape::T, Shape::Int],
            Self::CountOnes
            | Self::LeadingZeros
            | Self::TrailingZeros
            | Self::SwapBytes
            | Self::ToBigEndian
            | Self::FromBigEndian
            | Self::ToLittleEndian
            | Self::FromLittleEndian
            | Self::FloatSqrt
            | Self::FloatAbs
            | Self::FloatFloor
            | Self::FloatCeil
            | Self::FloatTrunc
            | Self::FloatRound
            | Self::FloatSin
            | Self::FloatCos
            | Self::FloatTan
            | Self::FloatExp
            | Self::FloatLn
            | Self::FloatLog2
            | Self::FloatLog10
            | Self::FloatIsNan
            | Self::FloatIsInfinite
            | Self::FloatIsFinite
            | Self::NumToStr => &[Shape::T],
            Self::StrConcatNum => &[Shape::Str, Shape::T],
            Self::Float64ToBits => &[Shape::F64],
            Self::WideningMul => &[Shape::U64, Shape::U64],
            Self::Float64FromBits => &[Shape::U64],
            Self::Float32ToBits => &[Shape::F32],
            Self::Float32FromBits => &[Shape::U32],
            Self::FloatParses | Self::FloatParseRaw => &[Shape::Str],
            Self::BytesNew => &[Shape::Int],
            Self::BytesFromStr => &[Shape::Str],
            Self::BytesLen => &[Shape::Bytes],
            Self::BytesGet => &[Shape::Bytes, Shape::Int],
            Self::BytesSet => &[Shape::Bytes, Shape::Int, Shape::U8],
            Self::BytesSlice => &[Shape::Bytes, Shape::Int, Shape::Int],
            Self::BytesToStr => &[Shape::Bytes],
            Self::BytesCopy => &[Shape::Bytes, Shape::Bytes],
            Self::BytesReadLe => &[Shape::Bytes, Shape::Int],
            Self::BytesReadBe => &[Shape::Bytes, Shape::Int],
            Self::BytesWriteLe => &[Shape::Bytes, Shape::Int, Shape::T],
            Self::BytesWriteBe => &[Shape::Bytes, Shape::Int, Shape::T],
            Self::Utf8Valid => &[Shape::Str],
            Self::TextAsStr => &[Shape::Text],
            Self::TextLen => &[Shape::Text],
            Self::TextConcat => &[Shape::Text, Shape::Text],
            Self::TextIsBoundary => &[Shape::Text, Shape::Int],
            Self::TextSlice => &[Shape::Text, Shape::Int, Shape::Int],
            Self::TextScalarAt => &[Shape::Text, Shape::Int],
            Self::TextNext => &[Shape::Text, Shape::Int],
            Self::TextScalarCount => &[Shape::Text],
            Self::ScalarValid => &[Shape::Int],
            Self::ScalarText => &[Shape::Int],
            Self::OsOpenRead => &[Shape::Str],
            Self::OsOpenWrite => &[Shape::Str, Shape::Int],
            Self::OsDirOpen => &[Shape::Str],
            Self::OsStdin => &[],
            Self::OsStatus => &[Shape::OsHandle],
            Self::OsRead | Self::OsReadLine => &[Shape::OsHandle, Shape::Bytes],
            Self::OsWrite => &[Shape::OsHandle, Shape::Bytes],
            Self::OsSeek => &[Shape::OsHandle, Shape::Int, Shape::Int],
            Self::OsSync => &[Shape::OsHandle],
            Self::OsClose => &[Shape::OsHandle],
            Self::OsDirNext => &[Shape::OsHandle, Shape::Bytes],
            Self::OsStat => &[Shape::Str, Shape::Bytes],
            Self::OsMkdir => &[Shape::Str],
            Self::OsRmdir => &[Shape::Str],
            Self::OsUnlink => &[Shape::Str],
            Self::OsRename => &[Shape::Str, Shape::Str],
            Self::OsFamily => &[],
            Self::OsErrorText => &[Shape::Int],
            Self::OsTcpListen => &[Shape::Str],
            Self::OsTcpAccept => &[Shape::OsHandle],
            Self::OsTcpConnect => &[Shape::Str, Shape::Int],
            Self::OsUdpBind => &[Shape::Str],
            Self::OsUdpSendTo => &[Shape::OsHandle, Shape::Bytes, Shape::Str],
            Self::OsUdpRecvFrom => &[Shape::OsHandle, Shape::Bytes, Shape::Bytes],
            Self::OsSetTimeout => &[Shape::OsHandle, Shape::Int],
            Self::OsShutdown => &[Shape::OsHandle, Shape::Int],
            Self::OsLocalAddr => &[Shape::OsHandle],
            Self::OsPeerAddr => &[Shape::OsHandle],
            Self::OsResolve => &[Shape::Str, Shape::Int, Shape::Bytes],
            Self::OsEnvGet => &[Shape::Str],
            Self::OsEnvHas => &[Shape::Str],
            Self::OsEnvCount => &[],
            Self::OsEnvAt => &[Shape::Int],
            Self::OsSpawn => &[Shape::Str, Shape::Strs],
            Self::OsChildPipe => &[Shape::OsHandle, Shape::Int],
            Self::OsWait => &[Shape::OsHandle],
            Self::OsKill => &[Shape::OsHandle],
            Self::OsStdout | Self::OsStderr | Self::OsPid => &[],
            Self::OsSpawnIn => &[Shape::Str, Shape::Strs, Shape::Str, Shape::Strs],
            Self::OsSpawnWith => &[Shape::Str, Shape::Strs, Shape::Str, Shape::Strs, Shape::Int],
            Self::OsChildPid | Self::OsTryWait | Self::OsChildRusage | Self::OsUnlock => {
                &[Shape::OsHandle]
            }
            Self::OsKillGroup => &[Shape::Int, Shape::Int],
            Self::OsWaitUntil => &[Shape::OsHandle, Shape::Int],
            Self::OsLstat | Self::OsRealpath => &[Shape::Str, Shape::Bytes],
            Self::OsMkdtemp => &[Shape::Str],
            Self::OsChmod => &[Shape::Str, Shape::Int],
            Self::OsLock | Self::OsTryLock => &[Shape::OsHandle, Shape::Bool],
            Self::OsRandomBytes => &[Shape::Bytes],
            Self::AtomicNew => &[Shape::Int],
            Self::AtomicLoad => &[Shape::Atomic],
            Self::AtomicStore | Self::AtomicAdd | Self::AtomicSwap => &[Shape::Atomic, Shape::Int],
            Self::AtomicCompareSwap => &[Shape::Atomic, Shape::Int, Shape::Int],
            Self::BoxNew => &[Shape::T],
            Self::BoxGet => &[Shape::BoxT],
            Self::ListFromVec => &[Shape::VecT],
            Self::ListEmpty => &[],
            Self::ListLen => &[Shape::ListT],
            Self::ListGet => &[Shape::ListT, Shape::Int],
            Self::BoxFresh => &[],
            Self::BoxFill => &[Shape::BoxT, Shape::T],
            Self::ListPush => &[Shape::ListT, Shape::T],
            Self::CellNew => &[Shape::T],
            Self::CheckedAdd | Self::CheckedSub | Self::CheckedMul => &[Shape::T, Shape::T],
            Self::CellGet => &[Shape::CellT],
            Self::CellSet => &[Shape::CellT, Shape::T],
            Self::TextFromStr => &[Shape::Str],
            Self::TextAssume => &[Shape::Str],
        }
    }

    #[must_use]
    pub fn ret(self) -> Shape {
        match self {
            Self::StrLen | Self::StrByte => Shape::Int,
            Self::StrSlice
            | Self::StrConcat
            | Self::StrAppend
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
            | Self::TimeNowNs
            | Self::TimeWallNs
            | Self::TimeSleepMs
            | Self::MmioRead32
            | Self::MmioWrite32
            | Self::MmioRead8
            | Self::MmioWrite8
            | Self::MmioRead16
            | Self::MmioWrite16
            | Self::MmioRead64
            | Self::MmioWrite64
            | Self::WatchdogFeed
            | Self::IntsSum
            | Self::CErrno
            | Self::ForeignNonNull
            | Self::CallbackAddress
            | Self::ContractCheck => Shape::Int,
            Self::CStrCopy => Shape::Str,
            Self::WrappingAdd
            | Self::WrappingSub
            | Self::WrappingMul
            | Self::SaturatingAdd
            | Self::SaturatingSub
            | Self::SaturatingMul
            | Self::RotateLeft
            | Self::RotateRight
            | Self::SwapBytes
            | Self::ToBigEndian
            | Self::FromBigEndian
            | Self::ToLittleEndian
            | Self::FromLittleEndian
            | Self::FloatSqrt
            | Self::FloatAbs
            | Self::FloatFloor
            | Self::FloatCeil
            | Self::FloatTrunc
            | Self::FloatRound
            | Self::FloatSin
            | Self::FloatCos
            | Self::FloatTan
            | Self::FloatExp
            | Self::FloatLn
            | Self::FloatLog2
            | Self::FloatLog10
            | Self::FloatPow
            | Self::FloatAtan2
            | Self::FloatMin
            | Self::FloatMax => Shape::T,
            Self::CountOnes | Self::LeadingZeros | Self::TrailingZeros => Shape::Int,
            Self::FloatIsNan | Self::FloatIsInfinite | Self::FloatIsFinite | Self::FloatParses => {
                Shape::Bool
            }
            Self::NumToStr
            | Self::StrConcatNum
            | Self::FloatToStrFixed
            | Self::StrPadStart
            | Self::StrPadEnd => Shape::Str,
            Self::Float64ToBits => Shape::U64,
            Self::WideningMul => Shape::U128,
            Self::Float64FromBits | Self::FloatParseRaw => Shape::F64,
            Self::Float32ToBits => Shape::U32,
            Self::Float32FromBits => Shape::F32,
            Self::BytesNew => Shape::Bytes,
            Self::BytesFromStr => Shape::Bytes,
            Self::BytesLen => Shape::Int,
            Self::BytesGet => Shape::U8,
            Self::BytesSet => Shape::Int,
            Self::BytesSlice => Shape::Bytes,
            Self::BytesToStr => Shape::Str,
            Self::BytesCopy => Shape::Int,
            Self::BytesReadLe => Shape::T,
            Self::BytesReadBe => Shape::T,
            Self::BytesWriteLe => Shape::Int,
            Self::BytesWriteBe => Shape::Int,
            Self::Utf8Valid => Shape::Bool,
            Self::TextAsStr => Shape::Str,
            Self::TextLen => Shape::Int,
            Self::TextConcat => Shape::Text,
            Self::TextIsBoundary => Shape::Bool,
            Self::TextSlice => Shape::Text,
            Self::TextScalarAt => Shape::Int,
            Self::TextNext => Shape::Int,
            Self::TextScalarCount => Shape::Int,
            Self::ScalarValid => Shape::Bool,
            Self::ScalarText => Shape::Text,
            Self::OsOpenRead => Shape::OsHandle,
            Self::OsOpenWrite => Shape::OsHandle,
            Self::OsDirOpen => Shape::OsHandle,
            Self::OsStdin => Shape::OsHandle,
            Self::OsStatus => Shape::Int,
            Self::OsRead | Self::OsReadLine => Shape::Int,
            Self::OsWrite => Shape::Int,
            Self::OsSeek => Shape::Int,
            Self::OsSync => Shape::Int,
            Self::OsClose => Shape::Int,
            Self::OsDirNext => Shape::Int,
            Self::OsStat => Shape::Int,
            Self::OsMkdir => Shape::Int,
            Self::OsRmdir => Shape::Int,
            Self::OsUnlink => Shape::Int,
            Self::OsRename => Shape::Int,
            Self::OsFamily => Shape::Int,
            Self::OsErrorText => Shape::Str,
            Self::OsTcpListen => Shape::OsHandle,
            Self::OsTcpAccept => Shape::OsHandle,
            Self::OsTcpConnect => Shape::OsHandle,
            Self::OsUdpBind => Shape::OsHandle,
            Self::OsUdpSendTo => Shape::Int,
            Self::OsUdpRecvFrom => Shape::Int,
            Self::OsSetTimeout => Shape::Int,
            Self::OsShutdown => Shape::Int,
            Self::OsLocalAddr => Shape::Str,
            Self::OsPeerAddr => Shape::Str,
            Self::OsResolve => Shape::Int,
            Self::OsEnvGet => Shape::Str,
            Self::OsEnvHas => Shape::Bool,
            Self::OsEnvCount => Shape::Int,
            Self::OsEnvAt => Shape::Str,
            Self::OsSpawn => Shape::OsHandle,
            Self::OsChildPipe => Shape::OsHandle,
            Self::OsWait => Shape::Int,
            Self::OsKill => Shape::Int,
            Self::OsStdout | Self::OsStderr | Self::OsSpawnIn | Self::OsSpawnWith => {
                Shape::OsHandle
            }
            Self::OsPid
            | Self::OsChildPid
            | Self::OsKillGroup
            | Self::OsTryWait
            | Self::OsWaitUntil
            | Self::OsChildRusage
            | Self::OsLstat
            | Self::OsChmod
            | Self::OsRealpath
            | Self::OsLock
            | Self::OsTryLock
            | Self::OsUnlock => Shape::Int,
            Self::OsMkdtemp => Shape::Str,
            Self::OsRandomBytes => Shape::Int,
            Self::AtomicNew => Shape::Atomic,
            Self::AtomicLoad | Self::AtomicStore | Self::AtomicAdd | Self::AtomicSwap => Shape::Int,
            Self::AtomicCompareSwap => Shape::Bool,
            Self::BoxNew => Shape::BoxT,
            Self::BoxGet | Self::ListGet => Shape::T,
            Self::ListFromVec | Self::ListEmpty => Shape::ListT,
            Self::ListLen | Self::BoxFill | Self::ListPush | Self::CellSet => Shape::Int,
            Self::CellNew => Shape::CellT,
            Self::CheckedAdd | Self::CheckedSub | Self::CheckedMul => Shape::OptionT,
            Self::CellGet => Shape::T,
            Self::BoxFresh => Shape::BoxT,
            Self::TextFromStr => Shape::Text,
            Self::TextAssume => Shape::Text,
        }
    }

    /// Whether this built-in arrived after 1.0 as a **prelude** name (R1a): a module that
    /// defines a function of the same name keeps its own, and this one is hidden there, so no
    /// program valid under 1.0 changes meaning. The built-ins of 1.0 are never shadowed.
    #[must_use]
    pub const fn is_prelude(self) -> bool {
        matches!(
            self,
            Self::BoxNew
                | Self::BoxGet
                | Self::ListFromVec
                | Self::ListEmpty
                | Self::ListLen
                | Self::ListGet
                | Self::CellNew
                | Self::CellGet
                | Self::CellSet
                | Self::CheckedAdd
                | Self::CheckedSub
                | Self::CheckedMul
                | Self::FloatToStrFixed
                | Self::StrPadStart
                | Self::StrPadEnd
                // R1d: the platform, further.
                | Self::OsStdout
                | Self::OsStderr
                | Self::OsSpawnIn
                | Self::OsSpawnWith
                | Self::OsPid
                | Self::OsChildPid
                | Self::OsKillGroup
                | Self::OsTryWait
                | Self::OsWaitUntil
                | Self::OsChildRusage
                | Self::OsLstat
                | Self::OsChmod
                | Self::OsRealpath
                | Self::OsMkdtemp
                | Self::OsLock
                | Self::OsTryLock
                | Self::OsUnlock
        )
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
