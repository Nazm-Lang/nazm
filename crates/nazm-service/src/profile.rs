//! Restriction profiles (N47): rules over facts the checker already settled.
//!
//! `docs/architecture.md` §7.49. A profile is a named set of [`Rule`]s. Each rule reads a
//! semantic fact — the effect set `main` requires (transitively, through every call, standard
//! wrappers included), a function's declared contract, whether the build was held to its
//! lockfile — and either passes or fails with witnesses. No rule reads a name: effects are
//! [`Effect`]s, never spellings. A profile only refuses: it never changes what an accepted
//! program means, and every program a profile accepts, `general` accepts too.

use std::fmt::Write as _;

use nazm_core::sema::{Effect, EffectSource, FnId, ModuleGraph, Resolution, SinkClass};
use nazm_diag::{codes, Diagnostic};
use nazm_span::{SourceMap, Span};

/// One rule, by its stable identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// Every function of the program's own modules declares its effect set: no ambient
    /// authority bridge (N37).
    DeclaredEffects,
    /// The program requires no `io`.
    NoIo,
    /// The program requires no `spawn`: no tasks, so no scheduler.
    NoSpawn,
    /// The program requires no `foreign`: no C.
    NoForeign,
    /// The build is a package build held to its lockfile (`--locked`, N46).
    LockedBuild,
    /// No allocation site whose count per call is unknown (N52): what an embedded program
    /// allocates is bounded by its call structure.
    BoundedAllocation,
    /// No file contents, and nothing of unknown origin, reach standard output or error (N52).
    ContentsStayInFiles,
    /// No declassification: no `str_vouch` anywhere (N52).
    NoDeclassification,
    /// No clock (N54): nothing that needs a `TimeCap` — a program whose behaviour depends on
    /// time is not reproducible.
    NoAmbientTime,
    /// No call cycle (N60): a stack bound the compiler can state. `unknown` where a call goes
    /// through a function value, whose target the compiler does not know.
    NoRecursion,
    /// No select (N60): its result depends on which channels are ready when it looks.
    NoSelect,
    /// No channel send, receive or select (N63): their latency is another task's or the host
    /// scheduler's, not the program's.
    NoBlocking,
    /// Every `while` has a trip bound the compiler establishes (N63, §7.65); `unknown` otherwise.
    BoundedLoops,
    /// Every contract entrypoint that writes the state decides on its `caller` (N72, §7.74): a
    /// write no signer constraint guards is refused by name.
    SignedWrites,
    /// No function of the program's own modules exercises authority it does not hold (N79,
    /// §7.80): its recorded inherited authority is empty, so `main`'s parameters are the only
    /// root.
    ExplicitAuthority,
    /// No path `write_file` writes to, in any function, comes from a file's contents or what
    /// the checker does not follow (N80, §7.81): the language's rule over every function.
    CheckedPaths,
    /// No obligation whose status is unknown (N87, §7.88): no call through a function value,
    /// whose callee and so whose contract the compiler does not know — and, with `no-foreign`,
    /// no call into C. Every obligation is then proved or checked as the program runs.
    NoUnknownCalls,
    /// No allocation site at all (Gate 3, `docs/spec.md`, *Boards*): what the program holds is
    /// its statics, its stack and its devices, whatever the board's manifest gives it.
    NoHeap,
}

impl Rule {
    /// Every rule, in identity order.
    pub const ALL: [Self; 18] = [
        Self::DeclaredEffects,
        Self::NoIo,
        Self::NoSpawn,
        Self::NoForeign,
        Self::LockedBuild,
        Self::BoundedAllocation,
        Self::ContentsStayInFiles,
        Self::NoDeclassification,
        Self::NoAmbientTime,
        Self::NoRecursion,
        Self::NoSelect,
        Self::NoBlocking,
        Self::BoundedLoops,
        Self::SignedWrites,
        Self::ExplicitAuthority,
        Self::CheckedPaths,
        Self::NoUnknownCalls,
        Self::NoHeap,
    ];

    /// Its stable identity. Never reused and never renamed.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::DeclaredEffects => "declared-effects",
            Self::NoIo => "no-io",
            Self::NoSpawn => "no-spawn",
            Self::NoForeign => "no-foreign",
            Self::LockedBuild => "locked-build",
            Self::BoundedAllocation => "bounded-allocation",
            Self::ContentsStayInFiles => "contents-stay-in-files",
            Self::NoDeclassification => "no-declassification",
            Self::NoAmbientTime => "no-ambient-time",
            Self::NoRecursion => "no-recursion",
            Self::NoSelect => "no-select",
            Self::NoBlocking => "no-blocking",
            Self::BoundedLoops => "bounded-loops",
            Self::SignedWrites => "signed-writes",
            Self::ExplicitAuthority => "explicit-authority",
            Self::CheckedPaths => "checked-paths",
            Self::NoUnknownCalls => "no-unknown-calls",
            Self::NoHeap => "no-heap",
        }
    }

    /// The effect a rule forbids, if it is an effect rule.
    const fn effect(self) -> Option<Effect> {
        match self {
            Self::NoIo => Some(Effect::Io),
            Self::NoSpawn => Some(Effect::Spawn),
            Self::NoForeign => Some(Effect::Foreign),
            Self::DeclaredEffects
            | Self::LockedBuild
            | Self::BoundedAllocation
            | Self::ContentsStayInFiles
            | Self::NoDeclassification
            | Self::NoAmbientTime
            | Self::NoRecursion
            | Self::NoSelect
            | Self::NoBlocking
            | Self::BoundedLoops
            | Self::SignedWrites
            | Self::ExplicitAuthority
            | Self::CheckedPaths
            | Self::NoUnknownCalls
            | Self::NoHeap => None,
        }
    }
}

/// A profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Profile {
    General,
    Embedded,
    Critical,
    Cyber,
    /// An analysable subset with stated bounds (N63, §7.65): `embedded`'s rules, no blocking, no
    /// clock, and bounded loops.
    Realtime,
    /// Deterministic contracts (N69, §7.71): every contract declared, no outside world, no tasks,
    /// channels, clock or C, and no call cycle.
    Web3,
    /// Account-oriented contracts (N72, §7.74): `web3`, and no state write without a decision on
    /// the signer.
    Accounts,
    /// Authority only by holding it (N79, §7.80): no function inherits its caller's.
    Authority,
    /// A kernel-style image (Gate 3, `docs/spec.md`, *Boards*): no tasks, no heap, no C, nothing
    /// whose latency is another task's.
    Kernel,
}

impl Profile {
    pub const ALL: [Self; 9] = [
        Self::General,
        Self::Embedded,
        Self::Critical,
        Self::Cyber,
        Self::Realtime,
        Self::Web3,
        Self::Accounts,
        Self::Authority,
        Self::Kernel,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Embedded => "embedded",
            Self::Critical => "critical",
            Self::Cyber => "cyber",
            Self::Realtime => "realtime",
            Self::Web3 => "web3",
            Self::Accounts => "accounts",
            Self::Authority => "authority",
            Self::Kernel => "kernel",
        }
    }

    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Its rules, in identity order.
    #[must_use]
    pub const fn rules(self) -> &'static [Rule] {
        match self {
            Self::General => &[],
            Self::Embedded => &[
                Rule::NoIo,
                Rule::NoSpawn,
                Rule::BoundedAllocation,
                Rule::NoRecursion,
            ],
            Self::Critical => &[
                Rule::DeclaredEffects,
                Rule::NoSpawn,
                Rule::NoForeign,
                Rule::LockedBuild,
                Rule::NoDeclassification,
                Rule::NoAmbientTime,
                Rule::NoSelect,
                Rule::NoUnknownCalls,
            ],
            Self::Cyber => &[
                Rule::DeclaredEffects,
                Rule::NoForeign,
                Rule::LockedBuild,
                Rule::ContentsStayInFiles,
                Rule::CheckedPaths,
                Rule::NoUnknownCalls,
            ],
            Self::Realtime => &[
                Rule::NoIo,
                Rule::NoSpawn,
                Rule::BoundedAllocation,
                Rule::NoAmbientTime,
                Rule::NoRecursion,
                Rule::NoBlocking,
                Rule::BoundedLoops,
            ],
            Self::Web3 => &[
                Rule::DeclaredEffects,
                Rule::NoIo,
                Rule::NoSpawn,
                Rule::NoForeign,
                Rule::NoAmbientTime,
                Rule::NoRecursion,
                Rule::NoBlocking,
            ],
            Self::Accounts => &[
                Rule::DeclaredEffects,
                Rule::NoIo,
                Rule::NoSpawn,
                Rule::NoForeign,
                Rule::NoAmbientTime,
                Rule::NoRecursion,
                Rule::NoBlocking,
                Rule::SignedWrites,
            ],
            Self::Authority => &[Rule::ExplicitAuthority],
            Self::Kernel => &[
                Rule::NoSpawn,
                Rule::NoForeign,
                Rule::NoBlocking,
                Rule::NoHeap,
            ],
        }
    }
}

/// Where a rule's verdict comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Witness {
    pub span: Span,
    pub note: String,
}

/// One rule's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub rule: Rule,
    pub pass: bool,
    /// The facts the rule needs were not established (N60): not a pass, and not a proof of a
    /// violation either. A profile refuses it.
    pub unknown: bool,
    pub witnesses: Vec<Witness>,
}

/// What the profile rules read off MIR (N52, N60), computed by the caller when a rule in force
/// needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirFacts {
    /// Allocation sites whose count per call is unknown.
    pub unbounded: Vec<Witness>,
    /// Functions in a call cycle: those MIR gives a stack check.
    pub recursive: Vec<Witness>,
    /// Calls through a function value, whose target the compiler does not know.
    pub indirect: Vec<Witness>,
    /// What the bounds report states (N63, §7.65).
    pub bounds: Bounds,
    /// Contract entrypoints that write the state without deciding on their caller (N72).
    pub unsigned_writes: Vec<Witness>,
    /// Every allocation site, bounded or not (Gate 3): what `no-heap` refuses.
    pub allocations: Vec<Witness>,
}

/// The static bounds a program's facts establish (N63, §7.65): counts and bounds, never a
/// measurement, and `None` where the compiler does not know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bounds {
    /// Every loop, with its trip bound per entry.
    pub loops: Vec<crate::bounds::LoopBound>,
    /// The longest chain of direct calls from `main`, counting `main`: `None` with a cycle or a
    /// call through a function value.
    pub call_depth: Option<usize>,
    /// Allocation sites in the program's MIR — sites, not executions or bytes.
    pub allocation_sites: usize,
    /// `spawn` sites.
    pub task_sites: usize,
    /// Channel creation sites.
    pub queue_sites: usize,
}

/// Every verdict for a build under `profiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// The profiles in force, sorted and each once.
    pub profiles: Vec<Profile>,
    /// One verdict per rule any of them has, in rule order.
    pub verdicts: Vec<Verdict>,
    /// The bounds the facts establish, where they were computed (N63).
    pub bounds: Option<Bounds>,
}

impl Evaluation {
    /// Whether every rule passed.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.verdicts.iter().all(|v| v.pass)
    }
}

/// Whether a function belongs to the toolchain — the prelude or a standard module — rather
/// than to the program. The toolchain's functions all declare their effects; a rule about the
/// program's own contracts is about the others.
fn toolchain(graph: &ModuleGraph, res: &Resolution, g: FnId) -> bool {
    res.function(g)
        .and_then(|f| graph.key(f.module))
        .is_some_and(|k| k.is_prelude() || k.is_std())
}

/// The chain of calls from `main` to what first requires `effect`: each call, and the
/// built-in, `spawn` or contract it ends at.
fn chain(res: &Resolution, main: FnId, effect: Effect) -> Vec<Witness> {
    let mut out = Vec::new();
    let mut at = main;
    // Bounded by the number of functions: an inferred chain never revisits one.
    for _ in 0..=res.functions().len() {
        let Some(f) = res.function(at) else { break };
        let Some(r) = f.effect_reasons.iter().find(|r| r.effect == effect) else {
            break;
        };
        let name = |g: FnId| res.function(g).map_or_else(String::new, |d| d.name.clone());
        match r.source {
            EffectSource::Builtin(b) => {
                out.push(Witness {
                    span: r.at,
                    note: format!("the built-in `{}` exercises `{}`", b.name(), effect.name()),
                });
                break;
            }
            EffectSource::Spawn => {
                out.push(Witness {
                    span: r.at,
                    note: "a `spawn` starts a task".to_owned(),
                });
                break;
            }
            EffectSource::Inferred(g) => {
                out.push(Witness {
                    span: r.at,
                    note: format!("`{}` requires `{}`", name(g), effect.name()),
                });
                at = g;
            }
            EffectSource::Declared(g) => {
                out.push(Witness {
                    span: r.at,
                    note: format!("`{}` declares `{}`", name(g), effect.name()),
                });
                break;
            }
            EffectSource::Undeclared(g) => {
                out.push(Witness {
                    span: r.at,
                    note: format!(
                        "`{}` declares no effect set, so `{}` is assumed of it",
                        name(g),
                        effect.name()
                    ),
                });
                break;
            }
            EffectSource::Indirect(t) => {
                out.push(Witness {
                    span: r.at,
                    note: format!(
                        "a call through a function value of type `{}`, which requires `{}`",
                        res.types().show(t),
                        effect.name()
                    ),
                });
                break;
            }
        }
    }
    out
}

/// Evaluate `profiles` over a checked program. `locked` is whether this build is a package
/// build whose lockfile was verified.
///
/// `facts` is what MIR established, read off it by the caller (N52, N60); `None` where it was
/// not computed, which a rule needing it reports as `unknown` rather than passing vacuously.
#[must_use]
pub fn evaluate(
    profiles: &[Profile],
    graph: &ModuleGraph,
    res: &Resolution,
    locked: bool,
    facts: Option<&MirFacts>,
) -> Evaluation {
    let mut profiles: Vec<Profile> = profiles.to_vec();
    profiles.sort_unstable();
    profiles.dedup();
    let mut rules: Vec<Rule> = profiles
        .iter()
        .flat_map(|p| p.rules().iter().copied())
        .collect();
    rules.sort_unstable();
    rules.dedup();
    let main = res.entry();
    let required = main
        .and_then(|m| res.function(m))
        .and_then(|f| f.inferred_effects.or(f.declared_effects))
        .unwrap_or_default();
    let at_main = || {
        main.and_then(|m| res.function(m))
            .map_or(Span::synthetic(), |f| f.name_span)
    };
    let verdicts = rules
        .into_iter()
        .map(|rule| {
            let mut unknown = false;
            let witnesses = if let Some(effect) = rule.effect() {
                match main {
                    Some(m) if required.contains(effect) => {
                        let mut w = chain(res, m, effect);
                        if w.is_empty() {
                            w.push(Witness {
                                span: res.function(m).map_or(Span::synthetic(), |f| f.name_span),
                                note: format!("`main` requires `{}`", effect.name()),
                            });
                        }
                        w
                    }
                    _ => Vec::new(),
                }
            } else if rule == Rule::BoundedAllocation {
                if let Some(f) = facts {
                    f.unbounded.clone()
                } else {
                    unknown = true;
                    vec![Witness {
                        span: at_main(),
                        note: "the program's resource facts were not computed".to_owned(),
                    }]
                }
            } else if rule == Rule::NoHeap {
                if let Some(f) = facts {
                    f.allocations.clone()
                } else {
                    unknown = true;
                    vec![Witness {
                        span: at_main(),
                        note: "the program's resource facts were not computed".to_owned(),
                    }]
                }
            } else if rule == Rule::NoUnknownCalls {
                match facts {
                    None => {
                        unknown = true;
                        vec![Witness {
                            span: at_main(),
                            note: "the program's calls were not computed".to_owned(),
                        }]
                    }
                    Some(f) => f.indirect.clone(),
                }
            } else if rule == Rule::NoRecursion {
                match facts {
                    None => {
                        unknown = true;
                        vec![Witness {
                            span: at_main(),
                            note: "the program's call graph was not computed".to_owned(),
                        }]
                    }
                    Some(f) if !f.recursive.is_empty() => f.recursive.clone(),
                    Some(f) => {
                        unknown = !f.indirect.is_empty();
                        f.indirect.clone()
                    }
                }
            } else if rule == Rule::NoSelect {
                res.functions()
                    .iter()
                    .flat_map(|f| f.call_sites.iter())
                    .filter(|s| {
                        matches!(
                            s.callee,
                            nazm_core::sema::Callee::Builtin(
                                nazm_core::sema::Intrinsic::ChanSelectOf
                                    | nazm_core::sema::Intrinsic::ChanSelectUntil
                            )
                        )
                    })
                    .map(|s| Witness {
                        span: s.span,
                        note: "this select's result depends on which channel is ready first"
                            .to_owned(),
                    })
                    .collect()
            } else if rule == Rule::NoBlocking {
                res.functions()
                    .iter()
                    .flat_map(|f| f.call_sites.iter())
                    .filter(|s| {
                        matches!(
                            s.callee,
                            nazm_core::sema::Callee::Builtin(i) if blocks(i)
                        )
                    })
                    .map(|s| Witness {
                        span: s.span,
                        note: "this waits for another task or the scheduler".to_owned(),
                    })
                    .collect()
            } else if rule == Rule::BoundedLoops {
                match facts {
                    None => {
                        unknown = true;
                        vec![Witness {
                            span: at_main(),
                            note: "the program's loops were not analysed".to_owned(),
                        }]
                    }
                    Some(f) => {
                        unknown = true;
                        f.bounds
                            .loops
                            .iter()
                            .filter(|l| l.bound.is_none())
                            .map(|l| Witness {
                                span: l.at,
                                note: format!(
                                    "this loop of `{}` has no bound the compiler establishes: {}",
                                    l.function, l.reason
                                ),
                            })
                            .collect()
                    }
                }
            } else if rule == Rule::SignedWrites {
                if let Some(f) = facts {
                    f.unsigned_writes.clone()
                } else {
                    unknown = true;
                    vec![Witness {
                        span: at_main(),
                        note: "the program's contract facts were not computed".to_owned(),
                    }]
                }
            } else if let Some((policy, what)) = flow_rule(rule) {
                // One engine for every flow rule (N80, §7.81): the sink as solved, and the
                // chain that brought the origin to it.
                res.sinks()
                    .iter()
                    .filter_map(|s| {
                        let o = policy.refuses(s)?;
                        let mut w = vec![Witness {
                            span: s.at,
                            note: format!("{what} {}", o.describe()),
                        }];
                        if let Some((_, steps)) = s.trace.iter().find(|(x, _)| *x == o) {
                            w.extend(steps.iter().filter(|st| st.at != s.at).map(|st| Witness {
                                span: st.at,
                                note: st.note.clone(),
                            }));
                        }
                        Some(w)
                    })
                    .flatten()
                    .collect()
            } else if rule == Rule::NoDeclassification {
                res.sinks()
                    .iter()
                    .filter(|s| s.class == SinkClass::Vouch)
                    .map(|s| Witness {
                        span: s.at,
                        note: if s.origins.is_empty() {
                            "a vouch, clearing nothing".to_owned()
                        } else {
                            format!("a vouch, clearing {}", s.origins.names().join(", "))
                        },
                    })
                    .collect()
            } else if rule == Rule::NoAmbientTime {
                res.functions()
                    .iter()
                    .flat_map(|f| f.call_sites.iter())
                    .filter(|s| {
                        matches!(
                            s.callee,
                            nazm_core::sema::Callee::Builtin(i)
                                if nazm_core::sema::CapabilityKind::held_for(i)
                                    == Some(nazm_core::sema::CapabilityKind::Time)
                        )
                    })
                    .map(|s| Witness {
                        span: s.span,
                        note: "this reads a clock".to_owned(),
                    })
                    .collect()
            } else if rule == Rule::ExplicitAuthority {
                // The language's own rule since N104 (§7.105): no function inherits authority,
                // so nothing is left for the profile to refuse. Kept by name, so a manifest
                // that lists it still means what it meant.
                Vec::new()
            } else if rule == Rule::DeclaredEffects {
                res.functions()
                    .iter()
                    .enumerate()
                    .filter(|(i, f)| {
                        f.declared_effects.is_none()
                            && f.foreign.is_none()
                            && !toolchain(graph, res, FnId::from_index(*i))
                    })
                    .map(|(_, f)| Witness {
                        span: f.name_span,
                        note: format!("`{}` declares no effect set", f.name),
                    })
                    .collect()
            } else if locked {
                Vec::new()
            } else {
                // About the build rather than a line of it: shown at `main`, the program's root.
                vec![Witness {
                    span: main
                        .and_then(|m| res.function(m))
                        .map_or(Span::synthetic(), |f| f.name_span),
                    note: "this build is not a package build held to its lockfile".to_owned(),
                }]
            };
            Verdict {
                rule,
                pass: witnesses.is_empty(),
                unknown: unknown && !witnesses.is_empty(),
                witnesses,
            }
        })
        .collect();
    Evaluation {
        profiles,
        verdicts,
        bounds: facts.map(|f| f.bounds.clone()),
    }
}

/// The flow policy a rule is, and how its witness begins (N80).
fn flow_rule(rule: Rule) -> Option<(nazm_core::sema::FlowRule, &'static str)> {
    use nazm_core::sema::FlowRule;
    match rule {
        Rule::ContentsStayInFiles => Some((
            FlowRule::CONTENTS_STAY_IN_FILES,
            "this writes to standard output what comes from",
        )),
        Rule::CheckedPaths => Some((FlowRule::CHECKED_PATHS, "this path comes from")),
        _ => None,
    }
}

/// Whether a built-in waits for another task or the scheduler (N63): every channel send,
/// receive and select. A close does not wait.
const fn blocks(i: nazm_core::sema::Intrinsic) -> bool {
    use nazm_core::sema::Intrinsic as I;
    matches!(
        i,
        I::ChanSend
            | I::ChanRecv
            | I::ChanSendOf
            | I::ChanRecvOf
            | I::ChanSelectOf
            | I::ChanSelectUntil
    )
}

/// One diagnostic per failed rule: the profiles in force, the rule, and its witnesses.
#[must_use]
pub fn diagnostics(e: &Evaluation) -> Vec<Diagnostic> {
    let names: Vec<&str> = e.profiles.iter().map(|p| p.name()).collect();
    e.verdicts
        .iter()
        .filter(|v| !v.pass)
        .map(|v| {
            let first = v.witnesses.first();
            let d = Diagnostic::error(
                codes::PROFILE_VIOLATION,
                first.map_or(Span::synthetic(), |w| w.span),
                format!(
                    "the profile {} refuses this program: rule `{}`",
                    names
                        .iter()
                        .map(|n| format!("`{n}`"))
                        .collect::<Vec<_>>()
                        .join(" and "),
                    v.rule.id()
                ),
            );
            let d = if v.unknown {
                // Not a proof of a violation: the facts the rule needs are missing (N60).
                d.secondary(
                    first.map_or(Span::synthetic(), |w| w.span),
                    "unknown: the compiler could not establish what this rule needs, so it is not met",
                )
            } else {
                d
            };
            let mut d = d;
            if let Some(w) = first {
                d = d.primary_label(&w.note);
            }
            for w in v.witnesses.iter().skip(1).take(16) {
                d = d.secondary(w.span, &w.note);
            }
            d.help(match v.rule {
                Rule::DeclaredEffects => "write `! { … }` on each function named: a profile that forbids the ambient bridge reads every contract".to_owned(),
                Rule::LockedBuild => "build the package's directory with `--locked`, after `nazm lock`".to_owned(),
                Rule::NoIo | Rule::NoSpawn | Rule::NoForeign => {
                    "the chain shows where the effect enters; a profile refuses, it never rewrites".to_owned()
                }
                Rule::BoundedAllocation => "each site allocates inside a loop, a number of times the compiler does not know; allocate before the loop, or reuse what was allocated (`nazm explain-cost` lists every site)".to_owned(),
                Rule::CheckedPaths => "in every function, the path a program writes to may come from the command line or be computed in it, never from a file's contents or what the checker does not follow; `nazm explain-flow` shows the chain".to_owned(),
                Rule::ContentsStayInFiles => "a file's contents may be written to a file, never shown; `nazm explain-flow` lists every sink and what reaches it".to_owned(),
                Rule::NoDeclassification => "this profile admits no declassification: the value keeps its origin".to_owned(),
                Rule::NoAmbientTime => "this profile admits no clock: a deadline or a time read makes the program's behaviour depend on when it runs".to_owned(),
                Rule::NoRecursion => "a call cycle has no stack bound the compiler can state, and a call through a function value has a target it does not know; call each function directly, without a cycle".to_owned(),
                Rule::NoSelect => "this profile admits no select: receive from one channel, so the order is the program's".to_owned(),
                Rule::NoBlocking => "this profile admits nothing whose latency is another task's or the scheduler's: no channel send, receive or select".to_owned(),
                Rule::ExplicitAuthority => "take the capability as a parameter and pass it from `main`: under this profile a function holds only what its scope holds (`nazm inspect` shows each function's `inherits`)".to_owned(),
                Rule::SignedWrites => "compare `caller` with the state's owner, or another signer it names, before writing; an unguarded write lets anyone change the state".to_owned(),
                Rule::NoUnknownCalls => "call the function by its name: a call through a function value has a callee, and so a contract, the compiler does not know, and this profile admits no obligation of unknown status (`nazm obligations` lists them)".to_owned(),
                Rule::NoHeap => "each site allocates; under this profile a program holds its statics, its stack and its devices, so make the value a static, a fixed array or a local (`nazm explain-cost` lists every site)".to_owned(),
                Rule::BoundedLoops => "write the loop as a counter from a literal, compared with a literal and stepped towards it once per turn by a positive literal (§7.65); `nazm check --profile-report` lists every loop's bound".to_owned(),
            })
        })
        .collect()
}

/// `nazm.profile-report/1`: one line of canonical JSON — the same bytes for the same
/// program, profiles and build.
#[must_use]
pub fn report(e: &Evaluation, map: &SourceMap) -> String {
    let esc = |s: &str| {
        let mut o = String::new();
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                c if (c as u32) < 0x20 => {
                    let _ = write!(o, "\\u{:04x}", c as u32);
                }
                c => o.push(c),
            }
        }
        o
    };
    let mut s = String::from("{\"schema\":\"nazm.profile-report/1\",\"profiles\":[");
    let names: Vec<String> = e
        .profiles
        .iter()
        .map(|p| format!("\"{}\"", p.name()))
        .collect();
    s.push_str(&names.join(","));
    let _ = write!(
        s,
        "],\"outcome\":\"{}\",\"rules\":[",
        if e.accepted() { "accepted" } else { "refused" }
    );
    for (i, v) in e.verdicts.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(
            s,
            "{{\"id\":\"{}\",\"verdict\":\"{}\",\"witnesses\":[",
            v.rule.id(),
            if v.pass {
                "pass"
            } else if v.unknown {
                "unknown"
            } else {
                "fail"
            }
        );
        for (j, w) in v.witnesses.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            let file = if w.span == Span::synthetic() {
                "null".to_owned()
            } else {
                format!("\"{}\"", esc(map.name(w.span.file)))
            };
            let _ = write!(
                s,
                "{{\"file\":{file},\"start\":{},\"end\":{},\"note\":\"{}\"}}",
                w.span.start,
                w.span.end,
                esc(&w.note)
            );
        }
        s.push_str("]}");
    }
    s.push(']');
    if let Some(b) = &e.bounds {
        let _ = write!(s, ",\"bounds\":{{\"loops\":[");
        for (i, l) in b.loops.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let file = if l.at == Span::synthetic() {
                "null".to_owned()
            } else {
                format!("\"{}\"", esc(map.name(l.at.file)))
            };
            let bound = l.bound.map_or_else(|| "null".to_owned(), |n| n.to_string());
            let _ = write!(
                s,
                "{{\"function\":\"{}\",\"file\":{file},\"start\":{},\"end\":{},\"bound\":{bound},\"reason\":{}}}",
                esc(&l.function),
                l.at.start,
                l.at.end,
                if l.reason.is_empty() {
                    "null".to_owned()
                } else {
                    format!("\"{}\"", esc(l.reason))
                }
            );
        }
        let _ = write!(
            s,
            "],\"call_depth\":{},\"allocation_sites\":{},\"task_sites\":{},\"queue_sites\":{},\"stack_bytes\":null,\"wcet\":null,\"kinds\":{{\"loops\":\"static bound per entry\",\"call_depth\":\"static bound\",\"sites\":\"static count of sites, not executions\",\"stack_bytes\":\"unknown here: a board build states it (bounds.json)\",\"wcet\":\"unknown: not computed\"}}}}",
            b.call_depth.map_or_else(|| "null".to_owned(), |n| n.to_string()),
            b.allocation_sites,
            b.task_sites,
            b.queue_sites
        );
    }
    s.push_str(",\"note\":\"compiler-enforced restriction profile: evidence from the checker's facts, not a certification\"}");
    s
}
