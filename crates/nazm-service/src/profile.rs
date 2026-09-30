//! Restriction profiles (N47): rules over facts the checker already settled.
//!
//! `docs/architecture.md` §7.49. A profile is a named set of [`Rule`]s. Each rule reads a
//! semantic fact — the effect set `main` requires (transitively, through every call, standard
//! wrappers included), a function's declared contract, whether the build was held to its
//! lockfile — and either passes or fails with witnesses. No rule reads a name: effects are
//! [`Effect`]s, never spellings. A profile only refuses: it never changes what an accepted
//! program means, and every program a profile accepts, `general` accepts too.

use std::fmt::Write as _;

use nazm_core::sema::{Effect, EffectSource, FnId, ModuleGraph, Resolution};
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
}

impl Rule {
    /// Every rule, in identity order.
    pub const ALL: [Self; 5] = [
        Self::DeclaredEffects,
        Self::NoIo,
        Self::NoSpawn,
        Self::NoForeign,
        Self::LockedBuild,
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
        }
    }

    /// The effect a rule forbids, if it is an effect rule.
    const fn effect(self) -> Option<Effect> {
        match self {
            Self::NoIo => Some(Effect::Io),
            Self::NoSpawn => Some(Effect::Spawn),
            Self::NoForeign => Some(Effect::Foreign),
            Self::DeclaredEffects | Self::LockedBuild => None,
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
}

impl Profile {
    pub const ALL: [Self; 4] = [Self::General, Self::Embedded, Self::Critical, Self::Cyber];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Embedded => "embedded",
            Self::Critical => "critical",
            Self::Cyber => "cyber",
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
            Self::Embedded => &[Rule::NoIo, Rule::NoSpawn],
            Self::Critical => &[
                Rule::DeclaredEffects,
                Rule::NoSpawn,
                Rule::NoForeign,
                Rule::LockedBuild,
            ],
            Self::Cyber => &[Rule::DeclaredEffects, Rule::NoForeign, Rule::LockedBuild],
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
    pub witnesses: Vec<Witness>,
}

/// Every verdict for a build under `profiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// The profiles in force, sorted and each once.
    pub profiles: Vec<Profile>,
    /// One verdict per rule any of them has, in rule order.
    pub verdicts: Vec<Verdict>,
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
        }
    }
    out
}

/// Evaluate `profiles` over a checked program. `locked` is whether this build is a package
/// build whose lockfile was verified.
#[must_use]
pub fn evaluate(
    profiles: &[Profile],
    graph: &ModuleGraph,
    res: &Resolution,
    locked: bool,
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
    let verdicts = rules
        .into_iter()
        .map(|rule| {
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
                witnesses,
            }
        })
        .collect();
    Evaluation { profiles, verdicts }
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
            let mut d = Diagnostic::error(
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
            if v.pass { "pass" } else { "fail" }
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
    s.push_str("],\"note\":\"compiler-enforced restriction profile: evidence from the checker's facts, not a certification\"}");
    s
}
