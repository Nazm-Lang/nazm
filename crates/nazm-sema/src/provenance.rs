//! Provenance, N38: where a value came from, and where it may flow.
//!
//! # What provenance is
//!
//! A value's provenance is the set of **origins** its contents were derived from, by explicit
//! data flow: what was assigned, returned, passed, built into it or computed from it. It is
//! not an effect (reading a file is `io`; the text read is not), not a capability (an `IoCap`
//! is authority; knowing a value came from one grants nothing), not a type and not a runtime
//! tag. It is a checker fact, computed from resolved bodies and erased before anything runs.
//!
//! # The origins
//!
//! Four, and the compiler's ([`Origin`]). The absence of every origin is **local**: a literal,
//! or something computed only from literals. v1 is coarse on purpose — an origin is a *kind*
//! of source, not a particular argument or file — and [`Flow::why`] keeps the first span each
//! origin was met at, which is where a finer identity would attach.
//!
//! # Summaries
//!
//! A function's provenance is summarised as the origins its result always carries, the
//! parameters whose provenance it passes on, and the parameters that reach a restricted sink
//! ([`Summary`]). A function that returns a constant passes on nothing, whatever it is given.
//! Summaries are solved over the whole program from each module's [`ModuleFacts`] — its bodies
//! reduced to flows in terms of parameters and calls — which a cache can keep beside a module's
//! other results, so a caller needs another module's facts, never its body.

use std::fmt;

use nazm_span::Span;

use crate::intrinsic::Intrinsic;
use crate::resolve::FnId;

/// One kind of origin. The discriminant is its [`id`](Origin::id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// The process's command line: `arg` and `args_count`. Supplied by whoever ran the
    /// program, who already holds every authority the process has.
    Argument,
    /// The file system: `read_file`'s contents and `file_exists`'s answer. Supplied by
    /// whoever could write that file, who need not be the person running the program.
    File,
    /// The runtime's root authority: a capability `main` was handed (N37).
    Authority,
    /// Not followed: a device register, what a foreign (C) function returns, or a function
    /// whose facts are missing. It may be any of the others, and a restriction treats it as
    /// the worst of them. Since N80 a container's contents are followed, by cell (§7.81).
    Unknown,
}

impl Origin {
    /// Every origin, in id order.
    pub const ALL: [Self; 4] = [Self::Argument, Self::File, Self::Authority, Self::Unknown];

    /// The origin's stable identity. Never reused and never reordered.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::Argument => 0,
            Self::File => 1,
            Self::Authority => 2,
            Self::Unknown => 3,
        }
    }

    /// How every persisted and machine-readable form writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Argument => "argument",
            Self::File => "file",
            Self::Authority => "authority",
            Self::Unknown => "unknown",
        }
    }

    /// The origin written `name`, by its persisted spelling.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.name() == name)
    }

    /// In words, for a diagnostic.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Argument => "the command line",
            Self::File => "the contents of a file",
            Self::Authority => "the runtime's root authority",
            Self::Unknown => "a device or foreign code the checker does not follow",
        }
    }

    /// A witness label for where this origin enters a body.
    #[must_use]
    pub const fn enters(self) -> &'static str {
        match self {
            Self::Argument => "the command line is read here",
            Self::File => "a file's contents are read here",
            Self::Authority => "the runtime's root authority is handed over here",
            Self::Unknown => "a device register is read here, which the checker does not follow",
        }
    }

    /// How a built-in's result relates to its arguments, by the built-in's identity.
    ///
    /// Exhaustive on purpose, like [`crate::Effect::of_intrinsic`]: a built-in added without
    /// saying where its result comes from does not compile. A program's own function cannot
    /// be named `read_file`, so a call spelled `read_file` is this one.
    #[must_use]
    pub const fn of_intrinsic(i: Intrinsic) -> BuiltinFlow {
        match i {
            // Numbers (Gate 2): computed from their arguments, and from nothing else.
            Intrinsic::WrappingAdd
            | Intrinsic::WrappingSub
            | Intrinsic::WrappingMul
            | Intrinsic::SaturatingAdd
            | Intrinsic::SaturatingSub
            | Intrinsic::SaturatingMul
            | Intrinsic::CountOnes
            | Intrinsic::LeadingZeros
            | Intrinsic::TrailingZeros
            | Intrinsic::RotateLeft
            | Intrinsic::RotateRight
            | Intrinsic::SwapBytes
            | Intrinsic::ToBigEndian
            | Intrinsic::FromBigEndian
            | Intrinsic::ToLittleEndian
            | Intrinsic::FromLittleEndian
            | Intrinsic::FloatSqrt
            | Intrinsic::FloatAbs
            | Intrinsic::FloatFloor
            | Intrinsic::FloatCeil
            | Intrinsic::FloatTrunc
            | Intrinsic::FloatRound
            | Intrinsic::FloatSin
            | Intrinsic::FloatCos
            | Intrinsic::FloatTan
            | Intrinsic::FloatExp
            | Intrinsic::FloatLn
            | Intrinsic::FloatLog2
            | Intrinsic::FloatLog10
            | Intrinsic::FloatPow
            | Intrinsic::FloatAtan2
            | Intrinsic::FloatMin
            | Intrinsic::FloatMax
            | Intrinsic::FloatIsNan
            | Intrinsic::FloatIsInfinite
            | Intrinsic::FloatIsFinite
            | Intrinsic::NumToStr
            | Intrinsic::FloatToStrFixed
            | Intrinsic::StrPadStart
            | Intrinsic::StrPadEnd
            | Intrinsic::Float64ToBits
            | Intrinsic::Float64FromBits
            | Intrinsic::Float32ToBits
            | Intrinsic::Float32FromBits
            | Intrinsic::FloatParses
            | Intrinsic::FloatParseRaw => BuiltinFlow::Derive,
            // Bytes (Gate 2): a buffer is a container, its cell named `Bytes`, so a view and
            // the buffer it was cut from share it. A new zeroed one carries nothing; one made
            // from a `Str` carries the string's origin on its handle, which every read through
            // it, or through a view of it, joins in.
            Intrinsic::BytesNew => BuiltinFlow::Fresh,
            Intrinsic::BytesLen
            | Intrinsic::BytesGet
            | Intrinsic::BytesSlice
            | Intrinsic::BytesToStr
            | Intrinsic::BytesReadLe
            | Intrinsic::BytesReadBe => BuiltinFlow::Read,
            Intrinsic::BytesSet
            | Intrinsic::BytesWriteLe
            | Intrinsic::BytesWriteBe
            | Intrinsic::BytesCopy => BuiltinFlow::Derive,
            // Text (Gate 2): a value, like `Str`, computed from its arguments.
            Intrinsic::BytesFromStr
            | Intrinsic::Utf8Valid
            | Intrinsic::TextAsStr
            | Intrinsic::TextLen
            | Intrinsic::TextConcat
            | Intrinsic::TextIsBoundary
            | Intrinsic::TextSlice
            | Intrinsic::TextScalarAt
            | Intrinsic::TextNext
            | Intrinsic::TextScalarCount
            | Intrinsic::ScalarValid
            | Intrinsic::ScalarText
            | Intrinsic::TextFromStr
            | Intrinsic::TextAssume => BuiltinFlow::Derive,
            // Immutable indirection (R1a): nothing can write into one after it is made, so what
            // it holds is carried on the value itself, and read back out of it.
            Intrinsic::BoxNew | Intrinsic::BoxGet | Intrinsic::ListLen | Intrinsic::ListGet => {
                BuiltinFlow::Derive
            }
            Intrinsic::ListEmpty | Intrinsic::BoxFresh => BuiltinFlow::Fresh,
            // Made by lowering only, after this analysis has run on the program as written.
            Intrinsic::BoxFill | Intrinsic::ListPush => BuiltinFlow::Derive,
            Intrinsic::ArgsCount | Intrinsic::Arg => BuiltinFlow::Source(Self::Argument),
            Intrinsic::ReadFile | Intrinsic::FileExists => BuiltinFlow::Source(Self::File),
            // Where a file is written is the one restricted sink v1 has.
            Intrinsic::WriteFile => BuiltinFlow::WritePath,
            // Files and handles (Gate 2): a handle opened for reading, and what is read through
            // one, is the file system's; where one writes, and what it removes or renames, is
            // the restricted sink `write_file`'s path is.
            Intrinsic::OsOpenRead
            | Intrinsic::OsDirOpen
            | Intrinsic::OsStdin
            | Intrinsic::OsRead
            | Intrinsic::OsDirNext
            | Intrinsic::OsStat
            // A socket is a descriptor: what arrives through one is the outside world's, as
            // standard input's is, and so is the peer's address and a name's addresses.
            | Intrinsic::OsUdpRecvFrom
            | Intrinsic::OsPeerAddr
            | Intrinsic::OsResolve => BuiltinFlow::Source(Self::File),
            Intrinsic::OsTcpListen
            | Intrinsic::OsTcpAccept
            | Intrinsic::OsTcpConnect
            | Intrinsic::OsUdpBind
            | Intrinsic::OsUdpSendTo
            | Intrinsic::OsSetTimeout
            | Intrinsic::OsShutdown
            | Intrinsic::OsLocalAddr => BuiltinFlow::Derive,
            // The environment is the program's surroundings, as its arguments are (Gate 2).
            Intrinsic::OsEnvGet
            | Intrinsic::OsEnvHas
            | Intrinsic::OsEnvCount
            | Intrinsic::OsEnvAt => BuiltinFlow::Source(Self::Argument),
            // Which program runs is the restricted sink a written file's path is.
            Intrinsic::OsSpawn => BuiltinFlow::WritePath,
            Intrinsic::OsChildPipe | Intrinsic::OsWait | Intrinsic::OsKill => BuiltinFlow::Derive,
            Intrinsic::OsRandomBytes => BuiltinFlow::Derive,
            // An atomic is a container of one `Int` (Gate 3): made from its value, which the
            // handle carries; written; and read, alone or as an exchange.
            Intrinsic::AtomicNew | Intrinsic::AtomicStore => BuiltinFlow::Derive,
            Intrinsic::AtomicLoad
            | Intrinsic::AtomicAdd
            | Intrinsic::AtomicSwap
            | Intrinsic::AtomicCompareSwap => BuiltinFlow::Read,
            Intrinsic::OsOpenWrite
            | Intrinsic::OsMkdir
            | Intrinsic::OsRmdir
            | Intrinsic::OsUnlink
            | Intrinsic::OsRename => BuiltinFlow::WritePath,
            Intrinsic::OsStatus
            | Intrinsic::OsWrite
            | Intrinsic::OsSeek
            | Intrinsic::OsSync
            | Intrinsic::OsClose
            | Intrinsic::OsFamily
            | Intrinsic::OsErrorText => BuiltinFlow::Derive,
            // Computed from their arguments, and from nothing else.
            Intrinsic::StrLen
            | Intrinsic::StrByte
            | Intrinsic::StrSlice
            | Intrinsic::StrConcat
            | Intrinsic::IntToStr
            | Intrinsic::StrFromByte
            | Intrinsic::Print
            | Intrinsic::EPrint
            | Intrinsic::IntsPush
            | Intrinsic::IntsSet
            | Intrinsic::StrsPush
            | Intrinsic::StrsSet
            | Intrinsic::VecPush
            | Intrinsic::VecSet
            | Intrinsic::ChanSend
            | Intrinsic::ChanClose
            | Intrinsic::ChanSendOf
            | Intrinsic::ChanCloseOf
            // A store's result is a constant; what reaches the device is the caller's (N62).
            | Intrinsic::MmioWrite32
            | Intrinsic::MmioWrite8
            | Intrinsic::MmioWrite16
            | Intrinsic::MmioWrite64 => BuiltinFlow::Derive,
            // Read out of a handle: what is there is whatever was written into any container
            // of its type — its cell (N80, §7.81). Which cell is [`Origin::container`]'s.
            Intrinsic::IntsLen
            | Intrinsic::IntsGet
            | Intrinsic::IntsPop
            | Intrinsic::StrsLen
            | Intrinsic::StrsGet
            | Intrinsic::StrsPop
            | Intrinsic::StrJoin
            | Intrinsic::VecLen
            | Intrinsic::VecGet
            | Intrinsic::VecPop
            // A copy of a `Vec`'s elements, read out of its cell (R1a).
            | Intrinsic::ListFromVec
            | Intrinsic::ChanRecvOf
            | Intrinsic::ChanSelectOf
            | Intrinsic::ChanSelectUntil
            | Intrinsic::ChanRecv
            // Read out of a handle, as `ints_get` is (N66).
            | Intrinsic::IntsSum => BuiltinFlow::Read,
            // A device register holds whatever the device put there (N62): not followed.
            Intrinsic::MmioRead32
            | Intrinsic::MmioRead8
            | Intrinsic::MmioRead16
            | Intrinsic::MmioRead64 => {
                BuiltinFlow::Source(Self::Unknown)
            }
            // What C left in `errno` is C's (N85), as everything C returns is (N42).
            Intrinsic::CErrno => BuiltinFlow::Source(Self::Unknown),
            // A check and a copy of what C returned: the call's own origin, carried (N85).
            Intrinsic::ForeignNonNull | Intrinsic::CStrCopy => BuiltinFlow::Derive,
            // A code address: no data flows through it.
            Intrinsic::CallbackAddress => BuiltinFlow::Fresh,
            // A check's result is the constant `0` (N87), as a feed's is (Gate 3).
            Intrinsic::ContractCheck | Intrinsic::WatchdogFeed => BuiltinFlow::Fresh,
            // A new, empty handle, or a call that never returns.
            Intrinsic::IntsNew
            | Intrinsic::StrsNew
            | Intrinsic::VecNew
            | Intrinsic::ChanNew
            | Intrinsic::ChanNewOf
            | Intrinsic::TimeNowMs
            | Intrinsic::TimeNowNs
            | Intrinsic::TimeWallNs
            | Intrinsic::TimeSleepMs
            | Intrinsic::ExitWith => BuiltinFlow::Fresh,
            // Declassification (N52): no origin, and the clearing recorded.
            Intrinsic::StrVouch => BuiltinFlow::Vouch,
        }
    }

    /// How a built-in touches a container's cell (N80, §7.81), by argument position.
    /// Exhaustive on purpose: a built-in that reads or writes a container says so here, and
    /// one that does neither says so too.
    #[must_use]
    pub const fn container(i: Intrinsic) -> ContainerUse {
        match i {
            // Files and handles (Gate 2): what is read lands in the buffer handed in.
            Intrinsic::OsRead | Intrinsic::OsDirNext | Intrinsic::OsStat => {
                ContainerUse::Fill { into: &[1] }
            }
            // Networking (Gate 2): a datagram and its sender's address, and a name's addresses.
            Intrinsic::OsUdpRecvFrom => ContainerUse::Fill { into: &[1, 2] },
            Intrinsic::OsEnvGet
            | Intrinsic::OsEnvHas
            | Intrinsic::OsEnvCount
            | Intrinsic::OsEnvAt
            | Intrinsic::OsSpawn
            | Intrinsic::OsChildPipe
            | Intrinsic::OsWait
            | Intrinsic::OsKill
            | Intrinsic::OsRandomBytes => ContainerUse::None,
            Intrinsic::OsResolve => ContainerUse::Fill { into: &[2] },
            Intrinsic::OsTcpListen
            | Intrinsic::OsTcpAccept
            | Intrinsic::OsTcpConnect
            | Intrinsic::OsUdpBind
            | Intrinsic::OsUdpSendTo
            | Intrinsic::OsSetTimeout
            | Intrinsic::OsShutdown
            | Intrinsic::OsLocalAddr
            | Intrinsic::OsPeerAddr => ContainerUse::None,
            Intrinsic::OsOpenRead
            | Intrinsic::OsOpenWrite
            | Intrinsic::OsDirOpen
            | Intrinsic::OsStdin
            | Intrinsic::OsStatus
            | Intrinsic::OsWrite
            | Intrinsic::OsSeek
            | Intrinsic::OsSync
            | Intrinsic::OsClose
            | Intrinsic::OsMkdir
            | Intrinsic::OsRmdir
            | Intrinsic::OsUnlink
            | Intrinsic::OsRename
            | Intrinsic::OsFamily
            | Intrinsic::OsErrorText => ContainerUse::None,
            // Numbers (Gate 2): computed from their arguments, and from nothing else.
            Intrinsic::WrappingAdd
            | Intrinsic::WrappingSub
            | Intrinsic::WrappingMul
            | Intrinsic::SaturatingAdd
            | Intrinsic::SaturatingSub
            | Intrinsic::SaturatingMul
            | Intrinsic::CountOnes
            | Intrinsic::LeadingZeros
            | Intrinsic::TrailingZeros
            | Intrinsic::RotateLeft
            | Intrinsic::RotateRight
            | Intrinsic::SwapBytes
            | Intrinsic::ToBigEndian
            | Intrinsic::FromBigEndian
            | Intrinsic::ToLittleEndian
            | Intrinsic::FromLittleEndian
            | Intrinsic::FloatSqrt
            | Intrinsic::FloatAbs
            | Intrinsic::FloatFloor
            | Intrinsic::FloatCeil
            | Intrinsic::FloatTrunc
            | Intrinsic::FloatRound
            | Intrinsic::FloatSin
            | Intrinsic::FloatCos
            | Intrinsic::FloatTan
            | Intrinsic::FloatExp
            | Intrinsic::FloatLn
            | Intrinsic::FloatLog2
            | Intrinsic::FloatLog10
            | Intrinsic::FloatPow
            | Intrinsic::FloatAtan2
            | Intrinsic::FloatMin
            | Intrinsic::FloatMax
            | Intrinsic::FloatIsNan
            | Intrinsic::FloatIsInfinite
            | Intrinsic::FloatIsFinite
            | Intrinsic::NumToStr
            | Intrinsic::FloatToStrFixed
            | Intrinsic::StrPadStart
            | Intrinsic::StrPadEnd
            | Intrinsic::Float64ToBits
            | Intrinsic::Float64FromBits
            | Intrinsic::Float32ToBits
            | Intrinsic::Float32FromBits
            | Intrinsic::FloatParses
            | Intrinsic::FloatParseRaw => ContainerUse::None,
            Intrinsic::BytesLen
            | Intrinsic::BytesGet
            | Intrinsic::BytesSlice
            | Intrinsic::BytesToStr
            | Intrinsic::BytesReadLe
            | Intrinsic::BytesReadBe => ContainerUse::Read { handle: 0 },
            Intrinsic::BytesSet | Intrinsic::BytesWriteLe | Intrinsic::BytesWriteBe => {
                ContainerUse::Write {
                    handle: 0,
                    value: 2,
                }
            }
            // What `src` holds is written into `dst`'s cell.
            Intrinsic::BytesCopy => ContainerUse::Write {
                handle: 0,
                value: 1,
            },
            Intrinsic::BytesNew
            | Intrinsic::BytesFromStr
            | Intrinsic::Utf8Valid
            | Intrinsic::TextAsStr
            | Intrinsic::TextLen
            | Intrinsic::TextConcat
            | Intrinsic::TextIsBoundary
            | Intrinsic::TextSlice
            | Intrinsic::TextScalarAt
            | Intrinsic::TextNext
            | Intrinsic::TextScalarCount
            | Intrinsic::ScalarValid
            | Intrinsic::ScalarText
            | Intrinsic::TextFromStr
            | Intrinsic::TextAssume => ContainerUse::None,
            Intrinsic::IntsLen
            | Intrinsic::IntsGet
            | Intrinsic::IntsPop
            | Intrinsic::StrsLen
            | Intrinsic::StrsGet
            | Intrinsic::StrsPop
            | Intrinsic::StrJoin
            | Intrinsic::VecLen
            | Intrinsic::VecGet
            | Intrinsic::VecPop
            | Intrinsic::ListFromVec => ContainerUse::Read { handle: 0 },
            // Immutable (R1a): no cell; what one holds rides on the value.
            Intrinsic::BoxNew
            | Intrinsic::BoxGet
            | Intrinsic::ListEmpty
            | Intrinsic::ListLen
            | Intrinsic::ListGet
            | Intrinsic::BoxFresh
            | Intrinsic::BoxFill
            | Intrinsic::ListPush => ContainerUse::None,
            Intrinsic::IntsSum => ContainerUse::Read { handle: 1 },
            Intrinsic::AtomicNew => ContainerUse::None,
            Intrinsic::AtomicLoad => ContainerUse::Read { handle: 0 },
            Intrinsic::AtomicStore => ContainerUse::Write {
                handle: 0,
                value: 1,
            },
            Intrinsic::AtomicAdd | Intrinsic::AtomicSwap => ContainerUse::Exchange {
                handle: 0,
                value: 1,
            },
            // Whether it stored depends on what the cell held.
            Intrinsic::AtomicCompareSwap => ContainerUse::Exchange {
                handle: 0,
                value: 2,
            },
            Intrinsic::IntsPush
            | Intrinsic::StrsPush
            | Intrinsic::VecPush
            | Intrinsic::ChanSend
            | Intrinsic::ChanSendOf => ContainerUse::Write {
                handle: 0,
                value: 1,
            },
            Intrinsic::IntsSet | Intrinsic::StrsSet | Intrinsic::VecSet => ContainerUse::Write {
                handle: 0,
                value: 2,
            },
            // A receive moves a value from the channel onto `out`, and says whether it did.
            Intrinsic::ChanRecv | Intrinsic::ChanRecvOf => {
                ContainerUse::Receive { from: 0, into: 1 }
            }
            // A select moves a value from one of the channels onto `out`; its index is which
            // channel was ready — control, not data, as a condition is.
            Intrinsic::ChanSelectOf | Intrinsic::ChanSelectUntil => {
                ContainerUse::Select { from: 0, into: 1 }
            }
            Intrinsic::StrLen
            | Intrinsic::StrByte
            | Intrinsic::StrSlice
            | Intrinsic::StrConcat
            | Intrinsic::IntToStr
            | Intrinsic::StrFromByte
            | Intrinsic::Print
            | Intrinsic::EPrint
            | Intrinsic::ChanClose
            | Intrinsic::ChanCloseOf
            | Intrinsic::MmioWrite32
            | Intrinsic::MmioRead32
            | Intrinsic::MmioWrite8
            | Intrinsic::MmioRead8
            | Intrinsic::MmioWrite16
            | Intrinsic::MmioRead16
            | Intrinsic::MmioWrite64
            | Intrinsic::MmioRead64
            | Intrinsic::WatchdogFeed
            | Intrinsic::IntsNew
            | Intrinsic::StrsNew
            | Intrinsic::VecNew
            | Intrinsic::ChanNew
            | Intrinsic::ChanNewOf
            | Intrinsic::TimeNowMs
            | Intrinsic::TimeNowNs
            | Intrinsic::TimeWallNs
            | Intrinsic::TimeSleepMs
            | Intrinsic::ExitWith
            | Intrinsic::StrVouch
            | Intrinsic::ArgsCount
            | Intrinsic::Arg
            | Intrinsic::ReadFile
            | Intrinsic::FileExists
            | Intrinsic::WriteFile
            | Intrinsic::CErrno
            | Intrinsic::ForeignNonNull
            | Intrinsic::CStrCopy
            | Intrinsic::CallbackAddress
            | Intrinsic::ContractCheck => ContainerUse::None,
        }
    }

    const fn bit(self) -> u8 {
        1 << self.id()
    }
}

/// How a built-in touches a container's cell (N80): which argument is the handle, and
/// which is the value written. A cell is named by the handle's type (§7.81).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerUse {
    None,
    /// Its result carries the cell of argument `handle`'s container.
    Read {
        handle: usize,
    },
    /// Argument `value` is written into the cell of argument `handle`'s container.
    Write {
        handle: usize,
        value: usize,
    },
    /// Argument `value` is written into the cell of argument `handle`'s container, and the
    /// result carries the cell, which it read in the same step (Gate 3: an atomic's add, swap
    /// and compare-and-swap).
    Exchange {
        handle: usize,
        value: usize,
    },
    /// A value moves from `from`'s cell into `into`'s; the result carries `from`'s.
    Receive {
        from: usize,
        into: usize,
    },
    /// A value moves from the channels of `from` — a `Vec[Chan[T]]`, so its elements' cell —
    /// into `into`'s; the result is an index and carries nothing.
    Select {
        from: usize,
        into: usize,
    },
    /// The built-in's own source origin is written into the cell of each argument in `into`:
    /// what `os_read` puts into a buffer came from outside (Gate 2).
    Fill {
        into: &'static [usize],
    },
}

/// The cell every container whose type mentions a type parameter shares (N80): a write to it
/// reaches every cell, and a read of it reads every cell.
pub const WILD_CELL: &str = "*";

/// The prefix of an indirect target named by a function type (N80).
pub const VALUE_TARGET: &str = "fn ";
/// The prefix of an indirect target named by a trait method: `trait Name.method`.
pub const METHOD_TARGET: &str = "trait ";

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a built-in's result is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFlow {
    /// This origin, and whatever its arguments carry.
    Source(Origin),
    /// Its arguments' provenance, joined.
    Derive,
    /// [`Origin::Unknown`], and its arguments'.
    Read,
    /// Nothing: a fresh handle, or a call that does not return.
    Fresh,
    /// Its arguments', joined — and its first argument, the path, is a restricted sink.
    WritePath,
    /// No origin: its argument's are cleared, and the clearing is recorded (N52).
    Vouch,
}

/// Where data leaves the program, by class (N52): the sink registry. A sink is an argument
/// of a built-in; every one is reduced to flows and solved as N38's path sink is, and what a
/// class may receive beyond the language's one restriction is a profile's policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SinkClass {
    /// The path `write_file` writes to: the restricted sink (`N0372`).
    Path,
    /// What `print` and `eprint` write.
    Output,
    /// What `write_file` writes.
    FileData,
    /// Not a sink: where `str_vouch` cleared these origins, recorded beside the sinks so the
    /// evidence of every declassification is in one place.
    Vouch,
}

impl SinkClass {
    pub const ALL: [Self; 4] = [Self::Path, Self::Output, Self::FileData, Self::Vouch];

    /// How every persisted and printed form writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Output => "output",
            Self::FileData => "file-data",
            Self::Vouch => "vouch",
        }
    }

    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The sinks a built-in's arguments are, by argument position. Exhaustive on purpose:
    /// a built-in that takes data out of the program says so here.
    #[must_use]
    pub const fn of_intrinsic(i: Intrinsic) -> &'static [(usize, Self)] {
        match i {
            Intrinsic::WriteFile => &[(0, Self::Path), (1, Self::FileData)],
            Intrinsic::OsOpenWrite
            | Intrinsic::OsMkdir
            | Intrinsic::OsRmdir
            | Intrinsic::OsUnlink => &[(0, Self::Path)],
            Intrinsic::OsRename => &[(0, Self::Path), (1, Self::Path)],
            Intrinsic::OsWrite => &[(1, Self::FileData)],
            Intrinsic::OsUdpSendTo => &[(1, Self::FileData)],
            Intrinsic::OsSpawn => &[(0, Self::Path)],
            Intrinsic::Print | Intrinsic::EPrint => &[(0, Self::Output)],
            Intrinsic::StrVouch => &[(0, Self::Vouch)],
            _ => &[],
        }
    }
}

/// A set of origins, canonical by construction: a bit per id, iterated in id order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct OriginSet(u8);

impl OriginSet {
    /// No origin: local.
    pub const EMPTY: Self = Self(0);

    /// What a restricted sink refuses: file contents, and what the checker cannot follow.
    pub const RESTRICTED: Self = Self(Origin::File.bit() | Origin::Unknown.bit());

    #[must_use]
    pub const fn of(o: Origin) -> Self {
        Self(o.bit())
    }

    #[must_use]
    pub const fn with(self, o: Origin) -> Self {
        Self(self.0 | o.bit())
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[must_use]
    pub const fn contains(self, o: Origin) -> bool {
        self.0 & o.bit() != 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// In id order.
    pub fn iter(self) -> impl Iterator<Item = Origin> {
        Origin::ALL.into_iter().filter(move |o| self.contains(*o))
    }

    /// The names, in id order.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        self.iter().map(Origin::name).collect()
    }
}

impl FromIterator<Origin> for OriginSet {
    fn from_iter<I: IntoIterator<Item = Origin>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::with)
    }
}

/// What one value inside a body is made of, before the program is solved: origins met in
/// this body, the function's own parameters, and the results of calls it makes.
///
/// A join of any number of these is one of these, so a value's flow never grows past the
/// function's parameters and call sites: the representation is bounded by the body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Flow {
    pub origins: OriginSet,
    /// For each origin met in this body, where it was first met, in origin-id order.
    pub why: Vec<(Origin, Span)>,
    /// The function's parameters whose provenance this carries, sorted.
    pub params: Vec<u32>,
    /// The calls whose results this carries, by index into [`FunctionFacts::calls`], sorted.
    pub calls: Vec<u32>,
    /// The container cells this was read out of, by name, sorted, each with where it was first
    /// read (N80).
    pub cells: Vec<(String, Span)>,
    /// The indirect calls whose results this carries, by index into
    /// [`FunctionFacts::indirect`], sorted (N80).
    pub via: Vec<u32>,
}

impl Flow {
    /// A value of this origin, first met at `at`.
    #[must_use]
    pub fn origin(o: Origin, at: Span) -> Self {
        Self {
            origins: OriginSet::of(o),
            why: vec![(o, at)],
            ..Self::default()
        }
    }

    /// A parameter's own provenance.
    #[must_use]
    pub fn param(i: u32) -> Self {
        Self {
            params: vec![i],
            ..Self::default()
        }
    }

    /// A call's result.
    #[must_use]
    pub fn call(i: u32) -> Self {
        Self {
            calls: vec![i],
            ..Self::default()
        }
    }

    /// The contents of the cell `name`, first read at `at` (N80).
    #[must_use]
    pub fn cell(name: &str, at: Span) -> Self {
        Self {
            cells: vec![(name.to_owned(), at)],
            ..Self::default()
        }
    }

    /// An indirect call's result (N80).
    #[must_use]
    pub fn indirect(i: u32) -> Self {
        Self {
            via: vec![i],
            ..Self::default()
        }
    }

    /// Whether this carries nothing at all.
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.origins.is_empty()
            && self.params.is_empty()
            && self.calls.is_empty()
            && self.cells.is_empty()
            && self.via.is_empty()
    }

    /// Join `other` into this one, and say whether anything was added. Commutative,
    /// associative and idempotent: the result is a set, and a first reason already held is
    /// kept, so the order joins happen in changes only which span is shown first for an
    /// origin, never what the value carries.
    pub fn join(&mut self, other: &Self) -> bool {
        let mut grew = false;
        for &(o, at) in &other.why {
            if !self.origins.contains(o) {
                self.origins = self.origins.with(o);
                self.why.push((o, at));
                grew = true;
            }
        }
        if grew {
            self.why.sort_by_key(|(o, _)| o.id());
        }
        // An origin with no reason recorded still joins.
        let origins = self.origins.union(other.origins);
        grew |= origins != self.origins;
        self.origins = origins;
        grew |= merge(&mut self.params, &other.params);
        grew |= merge(&mut self.calls, &other.calls);
        grew |= merge(&mut self.via, &other.via);
        for (name, at) in &other.cells {
            if let Err(i) = self
                .cells
                .binary_search_by(|(n, _)| n.as_str().cmp(name.as_str()))
            {
                self.cells.insert(i, (name.clone(), *at));
                grew = true;
            }
        }
        grew
    }

    /// `self` joined with `other`.
    #[must_use]
    pub fn joined(mut self, other: &Self) -> Self {
        let _ = self.join(other);
        self
    }
}

fn merge(into: &mut Vec<u32>, from: &[u32]) -> bool {
    let mut grew = false;
    for &x in from {
        if let Err(at) = into.binary_search(&x) {
            into.insert(at, x);
            grew = true;
        }
    }
    grew
}

/// One call a body makes to a function of the program, or one `spawn` of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallFact {
    pub callee: FnId,
    /// The callee's name at the call: where a diagnostic about it points.
    pub at: Span,
    /// Each argument's flow, in the callee's parameter order.
    pub args: Vec<Flow>,
    pub spawned: bool,
}

/// A restricted sink in a body: the path argument of `write_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkFact {
    /// The built-in's name at the call.
    pub at: Span,
    /// The flow into the sink (the path, for a `path` sink).
    pub path: Flow,
    /// Which sink (N52).
    pub class: SinkClass,
}

/// A sink as solved (N52): which function, which class, where, and the origins that reach
/// it. What profiles and `nazm explain-flow` read; never cached, like every solved fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkReport {
    pub function: crate::resolve::FnId,
    pub class: SinkClass,
    pub at: Span,
    pub origins: OriginSet,
    /// For each origin that reaches it, in id order, one chain from where it entered to here
    /// (N80): each step a place and what happened there.
    pub trace: Vec<(Origin, Vec<Step>)>,
}

/// One step of a trace (N80).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub at: Span,
    pub note: String,
}

/// A value written into a container's cell (N80).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreFact {
    /// The cell's name: the container's type, or [`WILD_CELL`].
    pub cell: String,
    /// The built-in's name at the call.
    pub at: Span,
    pub value: Flow,
}

/// A call whose callee is not named (N80): through a function value or a trait bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndirectFact {
    /// Which functions it may run: [`VALUE_TARGET`] and the function type up to effects, or
    /// [`METHOD_TARGET`] and `Trait.method`.
    pub target: String,
    pub at: Span,
    /// Each argument's flow, receiver first for a method.
    pub args: Vec<Flow>,
}

/// A function value made (N80): a named function written as a value, or a closure, with what
/// a closure captures, in its capture order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakeFact {
    pub function: FnId,
    /// [`VALUE_TARGET`] and its type up to effects: which indirect calls may run it.
    pub target: String,
    pub at: Span,
    pub captures: Vec<Flow>,
}

/// One function's body, reduced to flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionFacts {
    pub function: FnId,
    pub calls: Vec<CallFact>,
    pub result: Flow,
    pub sinks: Vec<SinkFact>,
    /// What it writes into containers (N80).
    pub stores: Vec<StoreFact>,
    /// Its calls through values and bounds (N80).
    pub indirect: Vec<IndirectFact>,
    /// The function values it makes (N80).
    pub makes: Vec<MakeFact>,
}

/// Where a policy rule applies (N80).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleScope {
    /// In a function that declares an effect set, at its own sinks and at every call that
    /// hands a parameter reaching one: the language's rule, N37's functions.
    Declared,
    /// At every sink of the program, with everything that reaches it.
    Every,
}

/// One rule of a flow policy (N80, §7.81): a sink class may not receive these origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowRule {
    pub class: SinkClass,
    pub forbidden: OriginSet,
    pub scope: RuleScope,
}

impl FlowRule {
    /// The language's policy: one rule, `N0372`.
    pub const LANGUAGE: Self = Self {
        class: SinkClass::Path,
        forbidden: OriginSet::RESTRICTED,
        scope: RuleScope::Declared,
    };

    /// `cyber`'s `contents-stay-in-files` (N52): nothing restricted is shown.
    pub const CONTENTS_STAY_IN_FILES: Self = Self {
        class: SinkClass::Output,
        forbidden: OriginSet::RESTRICTED,
        scope: RuleScope::Every,
    };

    /// `cyber`'s `checked-paths` (N80): the language's rule, in every function.
    pub const CHECKED_PATHS: Self = Self {
        class: SinkClass::Path,
        forbidden: OriginSet::RESTRICTED,
        scope: RuleScope::Every,
    };

    /// The origin of `report` this rule refuses first, if any: for a rule over every sink.
    #[must_use]
    pub fn refuses(&self, report: &SinkReport) -> Option<Origin> {
        if report.class != self.class {
            return None;
        }
        report.origins.intersection(self.forbidden).iter().next()
    }
}

/// Every function of one module, reduced to flows. A function of the module the facts, and
/// its imports' declarations, fully determine — which is what lets a cache keep it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleFacts {
    pub functions: Vec<FunctionFacts>,
}

/// A function's solved provenance.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    /// Origins its result carries whatever it is given.
    pub origins: OriginSet,
    /// Parameters whose provenance its result carries, sorted.
    pub params: Vec<u32>,
    /// Parameters whose value may reach a restricted sink, sorted.
    pub reaches: Vec<u32>,
    /// For each parameter, in order, the origins the program's calls and `spawn`s hand it:
    /// where a parameter's value may come from, over every call site the program has.
    pub receives: Vec<OriginSet>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(n: u32) -> Span {
        Span::new(nazm_span::FileId::from_index(0), n, n + 1)
    }

    #[test]
    fn identities_are_stable_and_names_round_trip() {
        for (i, o) in Origin::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(o.id()), i);
            assert_eq!(Origin::by_name(o.name()), Some(o));
        }
        assert_eq!(Origin::by_name("File"), None);
    }

    #[test]
    fn a_join_is_a_set_union_whatever_the_order_and_keeps_one_reason_per_origin() {
        let a = Flow::origin(Origin::File, at(1)).joined(&Flow::param(2));
        let b = Flow::origin(Origin::Argument, at(5))
            .joined(&Flow::call(0))
            .joined(&Flow::param(0));
        let ab = a.clone().joined(&b);
        let ba = b.clone().joined(&a);
        assert_eq!(ab.origins, ba.origins);
        assert_eq!(ab.params, [0, 2]);
        assert_eq!(ab.calls, [0]);
        assert_eq!(ab.clone().joined(&ab), ab, "idempotent");
        assert_eq!(ab.why.len(), 2);
        assert_eq!(ab.why.first().map(|w| w.0), Some(Origin::Argument));
        assert!(Flow::default().is_local());
    }

    #[test]
    fn every_built_in_says_where_its_result_comes_from() {
        let sources: Vec<(&str, Origin)> = Intrinsic::ALL
            .into_iter()
            .filter_map(|i| match Origin::of_intrinsic(i) {
                BuiltinFlow::Source(o) => Some((i.name(), o)),
                _ => None,
            })
            .collect();
        assert_eq!(
            sources,
            [
                // A device register is not followed (N62); with `c_errno` (N85), the only
                // built-ins whose result is `unknown`.
                ("mmio_read32", Origin::Unknown),
                ("args_count", Origin::Argument),
                ("arg", Origin::Argument),
                ("read_file", Origin::File),
                ("file_exists", Origin::File),
                // What C left in `errno` is C's, as anything C returns is (N85).
                ("c_errno", Origin::Unknown),
                // And a device register of another width (N86), as the 32-bit one.
                ("mmio_read8", Origin::Unknown),
                ("mmio_read16", Origin::Unknown),
                ("mmio_read64", Origin::Unknown),
                // Files and handles (Gate 2): a handle opened for reading, and what is read
                // through one, are the file system's.
                ("os_open_read", Origin::File),
                ("os_dir_open", Origin::File),
                ("os_stdin", Origin::File),
                ("os_read", Origin::File),
                ("os_dir_next", Origin::File),
                ("os_stat", Origin::File),
                // A socket's data, its peer and a name's addresses (Gate 2).
                ("os_udp_recv_from", Origin::File),
                ("os_peer_addr", Origin::File),
                ("os_resolve", Origin::File),
                // The environment (Gate 2), as the arguments.
                ("os_env_get", Origin::Argument),
                ("os_env_has", Origin::Argument),
                ("os_env_count", Origin::Argument),
                ("os_env_at", Origin::Argument)
            ]
        );
        // Every container built-in names its handle, and no other built-in claims one (N80).
        for i in Intrinsic::ALL {
            let reads = matches!(Origin::of_intrinsic(i), BuiltinFlow::Read);
            let touches = !matches!(Origin::container(i), ContainerUse::None);
            assert!(
                !reads || touches,
                "{} reads a handle it does not name",
                i.name()
            );
        }
        // Output is not input: `print` is not a source.
        assert_eq!(Origin::of_intrinsic(Intrinsic::Print), BuiltinFlow::Derive);
        assert_eq!(
            Origin::of_intrinsic(Intrinsic::WriteFile),
            BuiltinFlow::WritePath
        );
        assert!(OriginSet::RESTRICTED.contains(Origin::File));
        assert!(OriginSet::RESTRICTED.contains(Origin::Unknown));
        assert!(!OriginSet::RESTRICTED.contains(Origin::Argument));
        assert!(!OriginSet::RESTRICTED.contains(Origin::Authority));
    }
}
