//! Effects, N36: what externally observable computational power a function can exercise.
//!
//! # What an effect is
//!
//! An effect is a power a function exercises beyond computing its result from its
//! arguments, that someone outside the call could observe or would want to restrict. v1
//! has exactly the two the implementation can justify:
//!
//! - [`Effect::Io`] — the outside world: the process's arguments, files, standard output
//!   and error, and ending the process. The eight built-ins `docs/spec.md` lists under
//!   *What the outside-world built-ins do and do not promise*, and nothing else.
//! - [`Effect::Spawn`] — starting a task: the `spawn` statement.
//!
//! Allocation, mutation of a sequence, channel operations, a trap and a `Result` are not
//! effects here; `docs/spec.md`, *Effects*, says why each is not.
//!
//! # Identity
//!
//! An effect is identified by its [`Effect::id`], owned by the compiler and stable across
//! releases: never by how a name is spelled in source, and never by where. Source names
//! resolve to an effect in their own namespace ([`Effect::by_name`]) — `io` is not a
//! variable, a type or a function. A built-in's effect is a property of the built-in
//! ([`Effect::of_intrinsic`]), decided by an exhaustive `match`, so a built-in added
//! without classifying it does not compile.
//!
//! # Sets
//!
//! An [`EffectSet`] is a bit per id: order-independent and duplicate-free by
//! construction, and iterated, printed and persisted in id order, so no map's iteration
//! order can reach a result. The empty set is purity; there is no separate flag.

use std::fmt;

use crate::intrinsic::Intrinsic;

/// One built-in effect. The discriminant is its [`id`](Effect::id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    /// The outside world: arguments, files, standard streams, process exit.
    Io,
    /// Starting a task.
    Spawn,
    /// Calling a foreign (C) function (N42): whatever that function does, which Nazm cannot
    /// see.
    Foreign,
    /// The effect parameter (N51): whatever set a caller binds it to. In a function's declared
    /// set it is that function's; in a function type, its `row` owner's. Never a built-in's,
    /// never assumed, and needing no capability — the function value that performs it
    /// carries its own authority. Not in [`Effect::ALL`], which lists the effects a program
    /// can name by a fixed word.
    Param,
}

impl Effect {
    /// Every effect, in id order.
    pub const ALL: [Self; 3] = [Self::Io, Self::Spawn, Self::Foreign];

    /// The effect's stable identity. Never reused and never reordered: a persisted
    /// interface writes names, and a set's order is this order.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::Io => 0,
            Self::Spawn => 1,
            Self::Foreign => 2,
            Self::Param => 3,
        }
    }

    /// How source and every persisted form write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Spawn => "spawn",
            Self::Foreign => "foreign",
            // Persisted and shown as `*`: source writes the parameter's declared name, which a
            // set does not hold.
            Self::Param => "*",
        }
    }

    /// The effect a name in an effect set denotes, in the effect namespace only.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name)
    }

    /// The effect calling a built-in exercises, or `None` for one with none.
    ///
    /// Exhaustive on purpose. A new built-in does not compile until someone has said
    /// whether it reaches the outside world, which is the one question it must not be
    /// allowed to skip. Decided by the built-in's identity: a program's own function named
    /// `print` is not this `print`, and resolution already knows which it called.
    #[must_use]
    pub const fn of_intrinsic(i: Intrinsic) -> Option<Self> {
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
            | Intrinsic::Float64ToBits
            | Intrinsic::Float64FromBits
            | Intrinsic::Float32ToBits
            | Intrinsic::Float32FromBits
            | Intrinsic::FloatParses
            | Intrinsic::FloatParseRaw => None,
            // Bytes and text (Gate 2): memory the program holds, and nothing outside it.
            Intrinsic::BytesNew
            | Intrinsic::BytesFromStr
            | Intrinsic::BytesLen
            | Intrinsic::BytesGet
            | Intrinsic::BytesSet
            | Intrinsic::BytesSlice
            | Intrinsic::BytesToStr
            | Intrinsic::BytesCopy
            | Intrinsic::BytesReadLe
            | Intrinsic::BytesReadBe
            | Intrinsic::BytesWriteLe
            | Intrinsic::BytesWriteBe
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
            | Intrinsic::TextAssume => None,
            Intrinsic::ArgsCount
            | Intrinsic::Arg
            | Intrinsic::ReadFile
            | Intrinsic::WriteFile
            | Intrinsic::FileExists
            | Intrinsic::Print
            | Intrinsic::EPrint
            | Intrinsic::ExitWith
            // Files and handles (Gate 2): the operating system's.
            | Intrinsic::OsOpenRead
            | Intrinsic::OsOpenWrite
            | Intrinsic::OsDirOpen
            | Intrinsic::OsStdin
            | Intrinsic::OsRead
            | Intrinsic::OsWrite
            | Intrinsic::OsSeek
            | Intrinsic::OsSync
            | Intrinsic::OsClose
            | Intrinsic::OsDirNext
            | Intrinsic::OsStat
            | Intrinsic::OsMkdir
            | Intrinsic::OsRmdir
            | Intrinsic::OsUnlink
            | Intrinsic::OsRename
            // Networking (Gate 2): the operating system's too.
            | Intrinsic::OsTcpListen
            | Intrinsic::OsTcpAccept
            | Intrinsic::OsTcpConnect
            | Intrinsic::OsUdpBind
            | Intrinsic::OsUdpSendTo
            | Intrinsic::OsUdpRecvFrom
            | Intrinsic::OsSetTimeout
            | Intrinsic::OsShutdown
            | Intrinsic::OsLocalAddr
            | Intrinsic::OsPeerAddr
            | Intrinsic::OsResolve
            | Intrinsic::OsEnvGet
            | Intrinsic::OsEnvHas
            | Intrinsic::OsEnvCount
            | Intrinsic::OsEnvAt
            | Intrinsic::OsSpawn
            | Intrinsic::OsChildPipe
            | Intrinsic::OsWait
            | Intrinsic::OsKill => Some(Self::Io),
            // A handle's own state and a status's meaning are values the program holds.
            Intrinsic::OsStatus | Intrinsic::OsFamily | Intrinsic::OsErrorText => None,
            // Entropy changes nothing outside the program; its authority is a `RandomCap`, held.
            Intrinsic::OsRandomBytes => None,
            // Reading what C left is part of calling C (N85).
            Intrinsic::CErrno => Some(Self::Foreign),
            // Strings and sequences compute over values the caller holds; channels are
            // values too, and the power to run anything alongside is `spawn`'s.
            Intrinsic::StrLen
            | Intrinsic::StrByte
            | Intrinsic::StrSlice
            | Intrinsic::StrConcat
            | Intrinsic::IntToStr
            | Intrinsic::StrFromByte
            // Clearing an origin does nothing outside the program (N52).
            | Intrinsic::StrVouch
            // Reading a clock changes nothing outside the program; its authority is a
            // `TimeCap`, held (N54). A select waits as a receive does.
            | Intrinsic::TimeNowMs
            | Intrinsic::TimeNowNs
            | Intrinsic::TimeWallNs
            | Intrinsic::TimeSleepMs
            // A device register is the board's, not the operating system's — there is none
            // (N62). Its authority is an `MmioCap`, held, as a clock's is a `TimeCap`.
            | Intrinsic::MmioRead32
            | Intrinsic::MmioWrite32
            | Intrinsic::MmioRead8
            | Intrinsic::MmioWrite8
            | Intrinsic::MmioRead16
            | Intrinsic::MmioWrite16
            // A sum of what the caller holds (N66).
            | Intrinsic::IntsSum
            | Intrinsic::ChanSelectOf
            | Intrinsic::ChanSelectUntil
            | Intrinsic::IntsNew
            | Intrinsic::IntsLen
            | Intrinsic::IntsGet
            | Intrinsic::IntsSet
            | Intrinsic::IntsPush
            | Intrinsic::StrsNew
            | Intrinsic::StrsLen
            | Intrinsic::StrsGet
            | Intrinsic::StrsSet
            | Intrinsic::StrsPush
            | Intrinsic::IntsPop
            | Intrinsic::StrsPop
            | Intrinsic::StrJoin
            | Intrinsic::ChanNew
            | Intrinsic::ChanSend
            | Intrinsic::ChanRecv
            | Intrinsic::ChanClose
            | Intrinsic::ChanNewOf
            | Intrinsic::ChanSendOf
            | Intrinsic::ChanRecvOf
            | Intrinsic::ChanCloseOf
            | Intrinsic::VecNew
            | Intrinsic::VecLen
            | Intrinsic::VecGet
            | Intrinsic::VecSet
            | Intrinsic::VecPush
            | Intrinsic::VecPop
            // The call these belong to performs `foreign` already (N85).
            | Intrinsic::ForeignNonNull
            | Intrinsic::CStrCopy
            | Intrinsic::CallbackAddress
            | Intrinsic::ContractCheck => None,
        }
    }

    const fn bit(self) -> u8 {
        1 << self.id()
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of effects, canonical by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct EffectSet(u8);

impl EffectSet {
    /// No effect: a pure function's set.
    pub const EMPTY: Self = Self(0);

    /// Every effect: what a caller in another module must assume of a function that
    /// declares none, since it may not read that function's body.
    #[must_use]
    pub const fn all() -> Self {
        // A test holds this equal to `Effect::ALL` collected, so a new effect left out of it
        // fails there.
        Self(Effect::Io.bit() | Effect::Spawn.bit() | Effect::Foreign.bit())
    }

    /// What N37's compatibility bridge assumes of a function in another module that declares
    /// no effect set: `io` and `spawn`. Not `foreign` (N42): a foreign call needs a
    /// `ForeignCap` held wherever it is written, declared set or not, so an undeclared
    /// function can reach C only through a capability its caller passes it — which the caller
    /// then already holds.
    #[must_use]
    pub const fn bridged() -> Self {
        Self(Effect::Io.bit() | Effect::Spawn.bit())
    }

    #[must_use]
    pub const fn of(e: Effect) -> Self {
        Self(e.bit())
    }

    #[must_use]
    pub const fn with(self, e: Effect) -> Self {
        Self(self.0 | e.bit())
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// What `self` has and `other` does not.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    #[must_use]
    pub const fn contains(self, e: Effect) -> bool {
        self.0 & e.bit() != 0
    }

    /// Whether every effect of `self` is in `other`: the only compatibility relation
    /// effects have, and not a general subtyping.
    #[must_use]
    pub const fn is_within(self, other: Self) -> bool {
        self.0 & !other.0 == 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// In id order, the effect parameter last.
    pub fn iter(self) -> impl Iterator<Item = Effect> {
        Effect::ALL
            .into_iter()
            .chain([Effect::Param])
            .filter(move |e| self.contains(*e))
    }

    /// Whether the effect parameter is in it (N51).
    #[must_use]
    pub const fn has_param(self) -> bool {
        self.contains(Effect::Param)
    }

    /// Its named effects: without the effect parameter.
    #[must_use]
    pub const fn named(self) -> Self {
        Self(self.0 & !Effect::Param.bit())
    }

    /// `self` with the effect parameter replaced by `binding` (N51); `self` when it has none.
    #[must_use]
    pub const fn bound(self, binding: Self) -> Self {
        if self.has_param() {
            self.named().union(binding)
        } else {
            self
        }
    }

    /// The set a persisted name list means: built-in names, and `*` for the parameter.
    #[must_use]
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let mut out = Self::EMPTY;
        for n in names {
            let e = if n == Effect::Param.name() {
                Effect::Param
            } else {
                Effect::by_name(n)?
            };
            out = out.with(e);
        }
        Some(out)
    }

    /// The names, in id order: how a set is persisted and shown.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        self.iter().map(Effect::name).collect()
    }
}

impl FromIterator<Effect> for EffectSet {
    fn from_iter<I: IntoIterator<Item = Effect>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::with)
    }
}

/// `{}` or `{ io, spawn }` — the formatter's canonical spelling.
impl fmt::Display for EffectSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("{}");
        }
        write!(f, "{{ {} }}", self.names().join(", "))
    }
}

/// Why a function has one of its effects: the first thing in its body, in the order the
/// analysis met it, that requires it. A chain of these from a function to a built-in or a
/// `spawn` is the witness a diagnostic shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectReason {
    pub effect: Effect,
    /// The callee's name at the call, or the `spawn`'s callee — in this function's body.
    pub at: nazm_span::Span,
    pub source: EffectSource,
}

/// What, at [`EffectReason::at`], requires the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectSource {
    /// A built-in that exercises it. Compiler-owned: it has no source declaration.
    Builtin(Intrinsic),
    /// A `spawn`, which starts a task.
    Spawn,
    /// A function of this module whose effects were inferred and include it: the witness
    /// continues in that function.
    Inferred(crate::resolve::FnId),
    /// A function whose declared effect set includes it: the witness ends at its contract.
    Declared(crate::resolve::FnId),
    /// A function of another module that declares no effect set, so every effect is
    /// assumed of it: the witness ends there.
    Undeclared(crate::resolve::FnId),
    /// A call through a function value whose type includes it (N50): the witness ends at
    /// the type.
    Indirect(crate::types::Type),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_the_same_whatever_order_or_repetition_built_it() {
        let a: EffectSet = [Effect::Spawn, Effect::Io, Effect::Spawn]
            .into_iter()
            .collect();
        let b: EffectSet = [Effect::Io, Effect::Spawn].into_iter().collect();
        assert_eq!(a, b);
        assert_eq!(a.names(), ["io", "spawn"]);
        assert_eq!(a.to_string(), "{ io, spawn }");
        assert_eq!(EffectSet::EMPTY.to_string(), "{}");
        // The compatibility bridge covers the two N36 effects; `foreign` (N42) is never assumed.
        assert_eq!(EffectSet::bridged(), b);
        assert_eq!(EffectSet::all(), b.with(Effect::Foreign));
        assert_eq!(EffectSet::all().names(), ["io", "spawn", "foreign"]);
    }

    #[test]
    fn union_within_and_without_are_set_operations() {
        let io = EffectSet::of(Effect::Io);
        let spawn = EffectSet::of(Effect::Spawn);
        let both = io.union(spawn);
        assert!(io.is_within(both) && spawn.is_within(both) && !both.is_within(io));
        assert!(EffectSet::EMPTY.is_within(io));
        assert_eq!(both.without(io), spawn);
        assert!(both.contains(Effect::Io) && both.contains(Effect::Spawn));
        assert!(EffectSet::EMPTY.is_empty() && !io.is_empty());
    }

    #[test]
    fn identities_are_stable_and_names_resolve_only_in_their_own_namespace() {
        assert_eq!(Effect::Io.id(), 0);
        assert_eq!(Effect::Spawn.id(), 1);
        assert_eq!(Effect::by_name("io"), Some(Effect::Io));
        assert_eq!(Effect::by_name("spawn"), Some(Effect::Spawn));
        assert_eq!(Effect::by_name("print"), None);
        assert_eq!(Effect::by_name("IO"), None);
    }

    #[test]
    fn exactly_the_outside_world_built_ins_are_io() {
        let io: Vec<&str> = Intrinsic::ALL
            .into_iter()
            .filter(|i| Effect::of_intrinsic(*i) == Some(Effect::Io))
            .map(Intrinsic::name)
            .collect();
        assert_eq!(
            io,
            [
                "args_count",
                "arg",
                "read_file",
                "write_file",
                "file_exists",
                "print",
                "eprint",
                "exit_with",
                "os_open_read",
                "os_open_write",
                "os_dir_open",
                "os_stdin",
                "os_read",
                "os_write",
                "os_seek",
                "os_sync",
                "os_close",
                "os_dir_next",
                "os_stat",
                "os_mkdir",
                "os_rmdir",
                "os_unlink",
                "os_rename",
                "os_tcp_listen",
                "os_tcp_accept",
                "os_tcp_connect",
                "os_udp_bind",
                "os_udp_send_to",
                "os_udp_recv_from",
                "os_set_timeout",
                "os_shutdown",
                "os_local_addr",
                "os_peer_addr",
                "os_resolve",
                "os_env_get",
                "os_env_has",
                "os_env_count",
                "os_env_at",
                "os_spawn",
                "os_child_pipe",
                "os_wait",
                "os_kill",
            ]
        );
        assert!(Intrinsic::ALL
            .into_iter()
            .all(|i| Effect::of_intrinsic(i) != Some(Effect::Spawn)));
    }
}
