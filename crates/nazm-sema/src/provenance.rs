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
    /// Not followed: a value read out of shared mutable storage — a sequence or a channel —
    /// whose writers a local analysis cannot see, or a function whose facts are missing. It
    /// may be any of the others, and a restriction treats it as the worst of them.
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
            Self::Unknown => "shared storage or foreign code the checker does not follow",
        }
    }

    /// A witness label for where this origin enters a body.
    #[must_use]
    pub const fn enters(self) -> &'static str {
        match self {
            Self::Argument => "the command line is read here",
            Self::File => "a file's contents are read here",
            Self::Authority => "the runtime's root authority is handed over here",
            Self::Unknown => {
                "this is read out of a sequence or channel, which the checker does not follow"
            }
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
            Intrinsic::ArgsCount | Intrinsic::Arg => BuiltinFlow::Source(Self::Argument),
            Intrinsic::ReadFile | Intrinsic::FileExists => BuiltinFlow::Source(Self::File),
            // Where a file is written is the one restricted sink v1 has.
            Intrinsic::WriteFile => BuiltinFlow::WritePath,
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
            | Intrinsic::ChanCloseOf => BuiltinFlow::Derive,
            // Read out of a shared handle: what is there is whatever any alias wrote.
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
            | Intrinsic::ChanRecvOf
            | Intrinsic::ChanSelectOf
            | Intrinsic::ChanSelectUntil
            | Intrinsic::ChanRecv => BuiltinFlow::Read,
            // A new, empty handle, or a call that never returns.
            Intrinsic::IntsNew
            | Intrinsic::StrsNew
            | Intrinsic::VecNew
            | Intrinsic::ChanNew
            | Intrinsic::ChanNewOf
            | Intrinsic::TimeNowMs
            | Intrinsic::ExitWith => BuiltinFlow::Fresh,
            // Declassification (N52): no origin, and the clearing recorded.
            Intrinsic::StrVouch => BuiltinFlow::Vouch,
        }
    }

    const fn bit(self) -> u8 {
        1 << self.id()
    }
}

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

    /// Whether this carries nothing at all.
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.origins.is_empty() && self.params.is_empty() && self.calls.is_empty()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkReport {
    pub function: crate::resolve::FnId,
    pub class: SinkClass,
    pub at: Span,
    pub origins: OriginSet,
}

/// One function's body, reduced to flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionFacts {
    pub function: FnId,
    pub calls: Vec<CallFact>,
    pub result: Flow,
    pub sinks: Vec<SinkFact>,
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
                ("args_count", Origin::Argument),
                ("arg", Origin::Argument),
                ("read_file", Origin::File),
                ("file_exists", Origin::File)
            ]
        );
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
