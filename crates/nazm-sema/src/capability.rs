//! Capabilities, N37: what **authority** lets a function exercise an effect.
//!
//! # An effect is behaviour; a capability is authority
//!
//! N36's [`Effect`] answers *what can this function do* — `{ io }` says it may reach the
//! outside world. It grants nothing. A **capability** answers *what allows it to*: a value
//! of a compiler-owned type — `IoCap`, `SpawnCap` — whose presence in a function's scope is
//! the authority to exercise the corresponding effect. The two are checked separately and
//! reported separately: a body that exercises `io` without declaring it is an effect error,
//! and one that declares it without holding an `IoCap` is an authority error.
//!
//! # Identity
//!
//! A capability kind is identified by its [`CapabilityKind::id`], owned by the compiler and
//! stable, never by how its type is spelled. Its type is [`Type::Cap`](crate::Type::Cap),
//! a built-in type like `Chan`: a module cannot declare a type of that name, and nothing
//! a program writes can construct one. The only values are the ones the runtime hands to
//! `main`, which is the root of authority, and copies of those.
//!
//! # Kind, not resource
//!
//! v1 capabilities are **coarse**: an `IoCap` authorises every `io` built-in — standard
//! streams, files, arguments and exit alike. What a capability authorises is its kind and
//! nothing narrower; a resource-specific capability (one file, one directory) would be a
//! kind with a type argument, and no v1 kind has one. [`CapabilityKind::for_effect`] and
//! [`CapabilityKind::of_intrinsic`] return a [`CapabilitySet`] rather than a kind so that a
//! finer mapping later changes a table, not every caller.
//!
//! # Sets
//!
//! A [`CapabilitySet`] is a bit per id, as an [`EffectSet`] is: canonical by construction,
//! iterated and shown in id order, so no map's order reaches a diagnostic.

use std::fmt;

use crate::effect::{Effect, EffectSet};
use crate::intrinsic::Intrinsic;

/// One compiler-owned kind of authority. The discriminant is its [`id`](Self::id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityKind {
    /// Authority over the outside world: what the `io` effect exercises.
    Io,
    /// Authority to start a task: what the `spawn` effect exercises.
    Spawn,
    /// Authority to call a foreign (C) function: what the `foreign` effect exercises (N42).
    Foreign,
    /// Authority to declassify (N52): what `str_vouch` needs. No effect is authorised by it —
    /// clearing an origin does nothing outside the program.
    Vouch,
    /// Authority to read a clock (N54): what `time_now_ms` and a select's deadline need. No
    /// effect is authorised by it; a profile refuses it to keep a program reproducible.
    Time,
    /// Authority to touch a device register (N62): what `mmio_read32` and `mmio_write32`
    /// need. No effect is authorised by it; only a freestanding build has registers at all.
    Mmio,
    /// The standard streams and nothing else (N79): `print` and `eprint`. An attenuation of
    /// `IoCap`, which implies it; nothing implies `IoCap`. Never constructed: an `IoCap` is
    /// accepted where an `OutCap` is expected, and `main` may take one as a root.
    Out,
    /// Sockets and name resolution (Gate 2): what naming an address needs. Separate from `Io`
    /// — nothing implies it, and it implies nothing — so a program can be given files without
    /// the network or the network without files. `main` may take one as a root.
    Net,
    /// The operating system's entropy (Gate 2): what `os_random_bytes` needs, as a clock is a
    /// `TimeCap`'s. No effect is authorised by it; a seeded generator needs none.
    Random,
    /// Running another program (Gate 2): every authority that program has, so granted
    /// explicitly. Implies nothing, and nothing implies it. `main` may take one as a root.
    Process,
}

impl CapabilityKind {
    /// Every kind, in id order.
    pub const ALL: [Self; 10] = [
        Self::Io,
        Self::Spawn,
        Self::Foreign,
        Self::Vouch,
        Self::Time,
        Self::Mmio,
        Self::Out,
        Self::Net,
        Self::Random,
        Self::Process,
    ];

    /// The kind's stable identity. Never reused and never reordered.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::Io => 0,
            Self::Spawn => 1,
            Self::Foreign => 2,
            Self::Vouch => 3,
            Self::Time => 4,
            Self::Mmio => 5,
            Self::Out => 6,
            Self::Net => 7,
            Self::Random => 8,
            Self::Process => 9,
        }
    }

    /// The built-in type a value of this authority has, as source writes it.
    #[must_use]
    pub const fn type_name(self) -> &'static str {
        match self {
            Self::Io => "IoCap",
            Self::Spawn => "SpawnCap",
            Self::Foreign => "ForeignCap",
            Self::Vouch => "VouchCap",
            Self::Time => "TimeCap",
            Self::Mmio => "MmioCap",
            Self::Out => "OutCap",
            Self::Net => "NetCap",
            Self::Random => "RandomCap",
            Self::Process => "ProcessCap",
        }
    }

    /// The kind whose type is spelled `name`, in the type namespace only.
    #[must_use]
    pub fn by_type_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.type_name() == name)
    }

    /// The authority exercising `effect` requires.
    ///
    /// Exhaustive on purpose: an effect added without saying what authorises it does not
    /// compile. A set, not a kind, so a later finer mapping is a change here alone.
    #[must_use]
    pub const fn for_effect(effect: Effect) -> CapabilitySet {
        match effect {
            Effect::Io => CapabilitySet::of(Self::Io),
            Effect::Spawn => CapabilitySet::of(Self::Spawn),
            Effect::Foreign => CapabilitySet::of(Self::Foreign),
            // What a bound parameter performs is authorised by the value that performs it.
            Effect::Param => CapabilitySet::EMPTY,
        }
    }

    /// The authority calling a built-in requires, by the built-in's identity.
    ///
    /// Exhaustive and written out rather than derived from
    /// [`Effect::of_intrinsic`](crate::Effect::of_intrinsic): what a built-in may *do* and
    /// what *allows* it are two questions, and a test holds the two tables consistent
    /// rather than one being the other. A program's own function named `print` is not
    /// this `print`; resolution already knows which one a call reached.
    #[must_use]
    pub const fn of_intrinsic(i: Intrinsic) -> CapabilitySet {
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
            | Intrinsic::FloatParseRaw => CapabilitySet::EMPTY,
            // Bytes and text (Gate 2): computed from their arguments and the buffers they hold.
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
            | Intrinsic::TextAssume => CapabilitySet::EMPTY,
            // v1 is coarse: one `IoCap` authorises the arguments, the files, the standard
            // streams and ending the process alike.
            Intrinsic::ArgsCount
            | Intrinsic::Arg
            | Intrinsic::ReadFile
            | Intrinsic::WriteFile
            | Intrinsic::FileExists
            | Intrinsic::ExitWith => CapabilitySet::of(Self::Io),
            // Files and handles (Gate 2): naming a path is a file's authority, as `read_file`'s
            // is; using a handle already opened needs only the handle, which reaches nothing but
            // what it names — so a function can be given one file and nothing else.
            Intrinsic::OsOpenRead
            | Intrinsic::OsOpenWrite
            | Intrinsic::OsDirOpen
            | Intrinsic::OsStdin
            | Intrinsic::OsStat
            | Intrinsic::OsMkdir
            | Intrinsic::OsRmdir
            | Intrinsic::OsUnlink
            | Intrinsic::OsRename => CapabilitySet::of(Self::Io),
            Intrinsic::OsRead
            | Intrinsic::OsWrite
            | Intrinsic::OsSeek
            | Intrinsic::OsSync
            | Intrinsic::OsClose
            | Intrinsic::OsDirNext
            | Intrinsic::OsStatus
            | Intrinsic::OsFamily
            | Intrinsic::OsErrorText => CapabilitySet::EMPTY,
            // Networking (Gate 2): naming an address — to listen, connect, bind, send to or
            // resolve — is the network's authority; a socket already open is its handle's.
            Intrinsic::OsTcpListen
            | Intrinsic::OsTcpConnect
            | Intrinsic::OsUdpBind
            | Intrinsic::OsUdpSendTo
            | Intrinsic::OsResolve => CapabilitySet::of(Self::Net),
            Intrinsic::OsTcpAccept
            | Intrinsic::OsUdpRecvFrom
            | Intrinsic::OsSetTimeout
            | Intrinsic::OsShutdown
            | Intrinsic::OsLocalAddr
            | Intrinsic::OsPeerAddr => CapabilitySet::EMPTY,
            // The environment is the outside world's, as the arguments are (Gate 2).
            Intrinsic::OsEnvGet
            | Intrinsic::OsEnvHas
            | Intrinsic::OsEnvCount
            | Intrinsic::OsEnvAt => CapabilitySet::of(Self::Io),
            // Running another program is every authority it has (Gate 2); a child already
            // started is its handle's.
            Intrinsic::OsSpawn => CapabilitySet::of(Self::Process),
            Intrinsic::OsChildPipe | Intrinsic::OsWait | Intrinsic::OsKill => CapabilitySet::EMPTY,
            // Entropy is authority of its own (Gate 2, §21).
            Intrinsic::OsRandomBytes => CapabilitySet::of(Self::Random),
            // The standard streams need only their attenuation (N79), which `IoCap` implies.
            Intrinsic::Print | Intrinsic::EPrint => CapabilitySet::of(Self::Out),
            // Declassification is authority of its own (N52).
            Intrinsic::StrVouch => CapabilitySet::of(Self::Vouch),
            // A clock is authority of its own too (N54).
            Intrinsic::TimeNowMs
            | Intrinsic::TimeNowNs
            | Intrinsic::TimeWallNs
            | Intrinsic::TimeSleepMs
            | Intrinsic::ChanSelectUntil => CapabilitySet::of(Self::Time),
            // And a device register (N62).
            Intrinsic::MmioRead32
            | Intrinsic::MmioWrite32
            | Intrinsic::MmioRead8
            | Intrinsic::MmioWrite8
            | Intrinsic::MmioRead16
            | Intrinsic::MmioWrite16
            | Intrinsic::MmioRead64
            | Intrinsic::MmioWrite64
            | Intrinsic::WatchdogFeed => CapabilitySet::of(Self::Mmio),
            // Reading `errno` is part of calling C (N85).
            Intrinsic::CErrno => CapabilitySet::of(Self::Foreign),
            Intrinsic::StrLen
            | Intrinsic::StrByte
            | Intrinsic::StrSlice
            | Intrinsic::StrConcat
            | Intrinsic::IntToStr
            | Intrinsic::StrFromByte
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
            | Intrinsic::IntsSum
            | Intrinsic::ChanNew
            | Intrinsic::ChanSend
            | Intrinsic::ChanRecv
            | Intrinsic::ChanClose
            | Intrinsic::AtomicNew
            | Intrinsic::AtomicLoad
            | Intrinsic::AtomicStore
            | Intrinsic::AtomicAdd
            | Intrinsic::AtomicSwap
            | Intrinsic::AtomicCompareSwap
            | Intrinsic::ChanNewOf
            | Intrinsic::ChanSendOf
            | Intrinsic::ChanRecvOf
            | Intrinsic::ChanCloseOf
            | Intrinsic::ChanSelectOf
            | Intrinsic::VecNew
            | Intrinsic::VecLen
            | Intrinsic::VecGet
            | Intrinsic::VecSet
            | Intrinsic::VecPush
            | Intrinsic::VecPop
            | Intrinsic::BoxNew
            | Intrinsic::BoxGet
            | Intrinsic::ListFromVec
            | Intrinsic::ListEmpty
            | Intrinsic::ListLen
            | Intrinsic::ListGet
            | Intrinsic::BoxFresh
            | Intrinsic::BoxFill
            | Intrinsic::ListPush
            | Intrinsic::CellNew
            | Intrinsic::CellGet
            | Intrinsic::CellSet
            | Intrinsic::ForeignNonNull
            | Intrinsic::CStrCopy
            | Intrinsic::CallbackAddress
            | Intrinsic::ContractCheck => CapabilitySet::EMPTY,
        }
    }

    /// The authority a built-in needs **held where it is called, in every function**, apart
    /// from any effect: a declassification (N52) and a clock (N54). Neither performs an
    /// effect, so no declared set says it, and N37's bridge never supplies it.
    #[must_use]
    pub const fn held_for(i: Intrinsic) -> Option<Self> {
        match i {
            Intrinsic::StrVouch => Some(Self::Vouch),
            Intrinsic::TimeNowMs
            | Intrinsic::TimeNowNs
            | Intrinsic::TimeWallNs
            | Intrinsic::TimeSleepMs
            | Intrinsic::ChanSelectUntil => Some(Self::Time),
            Intrinsic::MmioRead32
            | Intrinsic::MmioWrite32
            | Intrinsic::MmioRead8
            | Intrinsic::MmioWrite8
            | Intrinsic::MmioRead16
            | Intrinsic::MmioWrite16
            | Intrinsic::MmioRead64
            | Intrinsic::MmioWrite64
            | Intrinsic::WatchdogFeed => Some(Self::Mmio),
            Intrinsic::OsRandomBytes => Some(Self::Random),
            _ => None,
        }
    }

    /// The authority a `spawn` statement requires to **create** a task — not what the task
    /// then does, which is its own function's business.
    #[must_use]
    pub const fn to_start_a_task() -> CapabilitySet {
        CapabilitySet::of(Self::Spawn)
    }

    /// `a` or `an`, for prose that names the type: "an `IoCap`", "a `SpawnCap`".
    #[must_use]
    pub const fn article(self) -> &'static str {
        match self {
            Self::Io => "an",
            Self::Spawn
            | Self::Foreign
            | Self::Vouch
            | Self::Time
            | Self::Net
            | Self::Random
            | Self::Process => "a",
            Self::Mmio | Self::Out => "an",
        }
    }

    const fn bit(self) -> u16 {
        1 << self.id()
    }
}

impl fmt::Display for CapabilityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.type_name())
    }
}

/// A set of capability kinds, canonical by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CapabilitySet(u16);

impl CapabilitySet {
    pub const EMPTY: Self = Self(0);

    #[must_use]
    pub const fn of(k: CapabilityKind) -> Self {
        Self(k.bit())
    }

    #[must_use]
    pub const fn with(self, k: CapabilityKind) -> Self {
        Self(self.0 | k.bit())
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
    pub const fn contains(self, k: CapabilityKind) -> bool {
        self.0 & k.bit() != 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The authority this set gives (N79): itself, and every kind a held kind implies —
    /// `IoCap` implies `OutCap`. Monotone: a narrower kind implies nothing broader.
    #[must_use]
    pub const fn implied(self) -> Self {
        if self.contains(CapabilityKind::Io) {
            self.with(CapabilityKind::Out)
        } else {
            self
        }
    }

    /// The authority exercising every effect of `effects` requires.
    #[must_use]
    pub fn for_effects(effects: EffectSet) -> Self {
        effects
            .iter()
            .fold(Self::EMPTY, |s, e| s.union(CapabilityKind::for_effect(e)))
    }

    /// In id order.
    pub fn iter(self) -> impl Iterator<Item = CapabilityKind> {
        CapabilityKind::ALL
            .into_iter()
            .filter(move |k| self.contains(*k))
    }
}

impl FromIterator<CapabilityKind> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = CapabilityKind>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::with)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_are_stable_and_types_resolve_by_their_own_name() {
        assert_eq!(CapabilityKind::Io.id(), 0);
        assert_eq!(CapabilityKind::Spawn.id(), 1);
        assert_eq!(
            CapabilityKind::by_type_name("IoCap"),
            Some(CapabilityKind::Io)
        );
        assert_eq!(
            CapabilityKind::by_type_name("SpawnCap"),
            Some(CapabilityKind::Spawn)
        );
        assert_eq!(CapabilityKind::by_type_name("io"), None);
        assert_eq!(CapabilityKind::by_type_name("Io"), None);
    }

    #[test]
    fn every_built_in_needs_exactly_the_authority_for_its_effect() {
        // Two tables, one question each; they agree built-in by built-in, with four stated
        // exceptions: declassification needs authority and performs no effect (N52), the
        // standard streams need only an `OutCap`, which the effect's `IoCap` implies (N79), and
        // a handle already open needs only itself, and naming an address needs a `NetCap`
        // (Gate 2).
        for i in Intrinsic::ALL {
            let from_effect =
                Effect::of_intrinsic(i).map_or(CapabilitySet::EMPTY, CapabilityKind::for_effect);
            if let Some(kind) = CapabilityKind::held_for(i) {
                assert_eq!(from_effect, CapabilitySet::EMPTY);
                assert_eq!(CapabilityKind::of_intrinsic(i), CapabilitySet::of(kind));
                continue;
            }
            // A handle already open is its own authority (Gate 2): using one performs `io` and
            // needs nothing held but the handle.
            if matches!(
                i,
                Intrinsic::OsRead
                    | Intrinsic::OsWrite
                    | Intrinsic::OsSeek
                    | Intrinsic::OsSync
                    | Intrinsic::OsClose
                    | Intrinsic::OsDirNext
                    | Intrinsic::OsTcpAccept
                    | Intrinsic::OsUdpRecvFrom
                    | Intrinsic::OsSetTimeout
                    | Intrinsic::OsShutdown
                    | Intrinsic::OsLocalAddr
                    | Intrinsic::OsPeerAddr
                    | Intrinsic::OsChildPipe
                    | Intrinsic::OsWait
                    | Intrinsic::OsKill
            ) {
                assert_eq!(from_effect, CapabilitySet::of(CapabilityKind::Io));
                assert!(CapabilityKind::of_intrinsic(i).is_empty(), "{}", i.name());
                continue;
            }
            // Naming an address is the network's authority (Gate 2), not the file system's:
            // `io` performed, a `NetCap` held.
            if matches!(
                i,
                Intrinsic::OsTcpListen
                    | Intrinsic::OsTcpConnect
                    | Intrinsic::OsUdpBind
                    | Intrinsic::OsUdpSendTo
                    | Intrinsic::OsResolve
            ) {
                assert_eq!(from_effect, CapabilitySet::of(CapabilityKind::Io));
                assert_eq!(
                    CapabilityKind::of_intrinsic(i),
                    CapabilitySet::of(CapabilityKind::Net)
                );
                continue;
            }
            // Running a program is `io` performed and a `ProcessCap` held (Gate 2).
            if i == Intrinsic::OsSpawn {
                assert_eq!(from_effect, CapabilitySet::of(CapabilityKind::Io));
                assert_eq!(
                    CapabilityKind::of_intrinsic(i),
                    CapabilitySet::of(CapabilityKind::Process)
                );
                continue;
            }
            if matches!(i, Intrinsic::Print | Intrinsic::EPrint) {
                assert_eq!(
                    CapabilityKind::of_intrinsic(i),
                    CapabilitySet::of(CapabilityKind::Out)
                );
                assert!(from_effect.implied().contains(CapabilityKind::Out));
                continue;
            }
            assert_eq!(CapabilityKind::of_intrinsic(i), from_effect, "{}", i.name());
        }
        assert_eq!(
            CapabilityKind::of_intrinsic(Intrinsic::ReadFile),
            CapabilitySet::of(CapabilityKind::Io)
        );
        // Monotone: nothing narrower implies anything broader.
        assert_eq!(
            CapabilitySet::of(CapabilityKind::Out).implied(),
            CapabilitySet::of(CapabilityKind::Out)
        );
        assert!(CapabilityKind::of_intrinsic(Intrinsic::ChanSend).is_empty());
    }

    #[test]
    fn a_set_is_canonical_and_covers_what_its_effects_need() {
        let a: CapabilitySet = [
            CapabilityKind::Spawn,
            CapabilityKind::Io,
            CapabilityKind::Io,
        ]
        .into_iter()
        .collect();
        assert_eq!(
            a.iter().collect::<Vec<_>>(),
            [CapabilityKind::Io, CapabilityKind::Spawn]
        );
        assert_eq!(CapabilitySet::for_effects(EffectSet::bridged()), a);
        assert_eq!(
            CapabilitySet::for_effects(EffectSet::all()),
            a.union(CapabilitySet::of(CapabilityKind::Foreign))
        );
        assert!(CapabilitySet::for_effects(EffectSet::EMPTY).is_empty());
        assert_eq!(
            a.without(CapabilitySet::of(CapabilityKind::Io)),
            CapabilitySet::of(CapabilityKind::Spawn)
        );
        assert_eq!(
            CapabilityKind::to_start_a_task(),
            CapabilitySet::of(CapabilityKind::Spawn)
        );
    }
}
