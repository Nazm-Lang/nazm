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
        }
    }

    /// How source and every persisted form write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Spawn => "spawn",
            Self::Foreign => "foreign",
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
            Intrinsic::ArgsCount
            | Intrinsic::Arg
            | Intrinsic::ReadFile
            | Intrinsic::WriteFile
            | Intrinsic::FileExists
            | Intrinsic::Print
            | Intrinsic::EPrint
            | Intrinsic::ExitWith => Some(Self::Io),
            // Strings and sequences compute over values the caller holds; channels are
            // values too, and the power to run anything alongside is `spawn`'s.
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
            | Intrinsic::VecPop => None,
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

    /// In id order.
    pub fn iter(self) -> impl Iterator<Item = Effect> {
        Effect::ALL.into_iter().filter(move |e| self.contains(*e))
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
                "exit_with"
            ]
        );
        assert!(Intrinsic::ALL
            .into_iter()
            .all(|i| Effect::of_intrinsic(i) != Some(Effect::Spawn)));
    }
}
