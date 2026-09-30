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
}

impl CapabilityKind {
    /// Every kind, in id order.
    pub const ALL: [Self; 3] = [Self::Io, Self::Spawn, Self::Foreign];

    /// The kind's stable identity. Never reused and never reordered.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::Io => 0,
            Self::Spawn => 1,
            Self::Foreign => 2,
        }
    }

    /// The built-in type a value of this authority has, as source writes it.
    #[must_use]
    pub const fn type_name(self) -> &'static str {
        match self {
            Self::Io => "IoCap",
            Self::Spawn => "SpawnCap",
            Self::Foreign => "ForeignCap",
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
            // v1 is coarse: one `IoCap` authorises the arguments, the files, the standard
            // streams and ending the process alike.
            Intrinsic::ArgsCount
            | Intrinsic::Arg
            | Intrinsic::ReadFile
            | Intrinsic::WriteFile
            | Intrinsic::FileExists
            | Intrinsic::Print
            | Intrinsic::EPrint
            | Intrinsic::ExitWith => CapabilitySet::of(Self::Io),
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
            | Intrinsic::ChanNew
            | Intrinsic::ChanSend
            | Intrinsic::ChanRecv
            | Intrinsic::ChanClose
            | Intrinsic::VecNew
            | Intrinsic::VecLen
            | Intrinsic::VecGet
            | Intrinsic::VecSet
            | Intrinsic::VecPush
            | Intrinsic::VecPop => CapabilitySet::EMPTY,
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
            Self::Spawn | Self::Foreign => "a",
        }
    }

    const fn bit(self) -> u8 {
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
pub struct CapabilitySet(u8);

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
        // Two tables, one question each; for v1 they agree built-in by built-in.
        for i in Intrinsic::ALL {
            let from_effect =
                Effect::of_intrinsic(i).map_or(CapabilitySet::EMPTY, CapabilityKind::for_effect);
            assert_eq!(CapabilityKind::of_intrinsic(i), from_effect, "{}", i.name());
        }
        assert_eq!(
            CapabilityKind::of_intrinsic(Intrinsic::Print),
            CapabilitySet::of(CapabilityKind::Io)
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
