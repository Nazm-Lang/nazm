//! Effect checking, N36: what a module's functions require, and whether each declared set
//! allows it.
//!
//! # Where the truth comes from
//!
//! From resolution, never from text. Each function's recorded call sites say, per call,
//! what it resolved to — a built-in, one of this module's functions, or an imported one —
//! and whether it is a `spawn`. A built-in's effect is its identity's
//! ([`Effect::of_intrinsic`]); a `spawn` exercises `spawn` and everything its task does.
//!
//! # The rule for a call
//!
//! A callee with a declared set contributes that set: its contract, which is what every
//! caller reads, in this module or another. A callee of this module with no declaration
//! contributes what its body requires. A callee of **another** module with no declaration
//! contributes every effect: a caller must not need another module's body to know its
//! effects, and the interface it checks against publishes contracts, not bodies. There is
//! no discharge — no handler removes an effect — so a caller requires the union.
//!
//! # Recursion
//!
//! The effects of a module's undeclared functions are the least fixed point of "direct
//! effects, plus every callee's": computed in rounds, each reading only the previous
//! round's sets, until nothing changes. Monotone over a finite lattice, so it terminates
//! in at most `functions × effects` rounds whatever the call graph — cycles included —
//! and source order does not matter. A declared callee is a constant in it.
//!
//! # The witness
//!
//! When an effect is first added to a function, the call that added it is recorded, and
//! the callee had the effect a round earlier. So a chain of reasons from any function
//! reaches a built-in, a `spawn` or a contract without revisiting anything, and — rounds
//! being breadth-first — it is a shortest one; ties go to the earlier call in the body.

use nazm_diag::{codes, Diagnostic};
use nazm_sema::interface::UnitInterface;
use nazm_sema::resolve::{Callee, FnId, Resolution};
use nazm_sema::{CapabilityKind, CapabilitySet, Effect, EffectReason, EffectSet, EffectSource};
use nazm_span::Span;
use nazm_syntax::{EffectDecl, Function};

/// The set a declaration writes, reporting every unknown and duplicate name.
///
/// `None` when a name is not an effect: a set with a word in it nobody can interpret is
/// not a contract, and checking the body against what remains would report the same
/// mistake a second time. Treated as unwritten, it is reported once.
pub(crate) fn declared(decl: &EffectDecl, errors: &mut Vec<Diagnostic>) -> Option<EffectSet> {
    let mut set = EffectSet::EMPTY;
    let mut known = true;
    for (i, (name, span)) in decl.names.iter().enumerate() {
        match Effect::by_name(name) {
            None => {
                known = false;
                errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_EFFECT,
                        *span,
                        format!("`{name}` is not an effect"),
                    )
                    .help(format!(
                        "the effects are {}. There are no user-defined effects",
                        Effect::ALL
                            .iter()
                            .map(|e| format!("`{e}`"))
                            .collect::<Vec<_>>()
                            .join(" and ")
                    )),
                );
            }
            Some(e) if set.contains(e) => {
                let mut d = Diagnostic::error(
                    codes::DUPLICATE_EFFECT,
                    *span,
                    format!("`{e}` is written twice in this effect set"),
                );
                // From the end of the name before it: `{ io, io }` becomes `{ io }`.
                if let Some((_, before)) = i.checked_sub(1).and_then(|p| decl.names.get(p)) {
                    d = d.fix_automatically(
                        Span::new(span.file, before.end, span.end),
                        "",
                        format!("remove the second `{e}`"),
                        "a set holds an effect once, so the second one means nothing",
                    );
                }
                errors.push(d);
            }
            Some(e) => set = set.with(e),
        }
    }
    known.then_some(set)
}

/// One of the module's functions, as the analysis sees it.
struct Node<'a> {
    id: FnId,
    f: &'a Function,
    declared: Option<EffectSet>,
    /// Effects its body requires directly, each with where, in call order.
    direct: Vec<EffectReason>,
    /// Calls to this module's undeclared functions: which node, and where.
    edges: Vec<(usize, Span)>,
}

/// Settle the effects of every function of the module `functions` belong to, record them,
/// and check each declared set.
pub(crate) fn analyse(
    functions: &[(u32, &Function)],
    module: nazm_sema::module::ModuleId,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    // This module's functions, once each: a second definition of a name is not declared.
    let mut nodes: Vec<Node<'_>> = Vec::new();
    for (decl, f) in functions {
        let Some(id) = res.definition(module, &f.name) else {
            continue;
        };
        let Some(def) = res.function(id) else {
            continue;
        };
        if def.decl != *decl {
            continue;
        }
        nodes.push(Node {
            id,
            f,
            declared: def.declared_effects,
            direct: Vec::new(),
            edges: Vec::new(),
        });
    }
    let index = |id: FnId, nodes: &[Node<'_>]| nodes.iter().position(|n| n.id == id);
    // What another module publishes for a function: its contract, or nothing.
    let contract = |id: FnId| -> Option<Option<EffectSet>> {
        deps.iter()
            .flat_map(|d| d.exports())
            .find(|e| e.def == id)
            .map(|e| e.effects)
    };

    for n in 0..nodes.len() {
        let Some(node) = nodes.get(n) else { continue };
        let sites = res
            .function(node.id)
            .map(|d| d.call_sites.clone())
            .unwrap_or_default();
        let mut direct = Vec::new();
        let mut edges = Vec::new();
        for site in sites {
            let at = site.span;
            if site.spawned {
                direct.push(EffectReason {
                    effect: Effect::Spawn,
                    at,
                    source: EffectSource::Spawn,
                });
            }
            match site.callee {
                Callee::Builtin(b) => {
                    if let Some(effect) = Effect::of_intrinsic(b) {
                        direct.push(EffectReason {
                            effect,
                            at,
                            source: EffectSource::Builtin(b),
                        });
                    }
                }
                Callee::Function(g) => {
                    let local = res
                        .function(g)
                        .is_some_and(|d| d.module == module)
                        .then(|| index(g, &nodes))
                        .flatten();
                    match local {
                        Some(j) => match nodes.get(j).and_then(|m| m.declared) {
                            Some(set) => direct.extend(set.iter().map(|effect| EffectReason {
                                effect,
                                at,
                                source: EffectSource::Declared(g),
                            })),
                            // A body that did not parse requires nothing anyone can know;
                            // it is already an error, and guessing would add a second.
                            None if nodes.get(j).is_some_and(|m| m.f.body.is_none()) => {}
                            None => edges.push((j, at)),
                        },
                        None => match contract(g) {
                            Some(Some(set)) => {
                                direct.extend(set.iter().map(|effect| EffectReason {
                                    effect,
                                    at,
                                    source: EffectSource::Declared(g),
                                }));
                            }
                            // Imported and undeclared, or not found at all: every effect.
                            _ => {
                                direct.extend(EffectSet::all().iter().map(|effect| EffectReason {
                                    effect,
                                    at,
                                    source: EffectSource::Undeclared(g),
                                }))
                            }
                        },
                    }
                }
            }
        }
        if let Some(node) = nodes.get_mut(n) {
            node.direct = direct;
            node.edges = edges;
        }
    }

    // Round zero: each body's own effects, the first reason for each.
    let mut sets: Vec<EffectSet> = Vec::with_capacity(nodes.len());
    let mut reasons: Vec<Vec<EffectReason>> = Vec::with_capacity(nodes.len());
    for node in &nodes {
        let mut set = EffectSet::EMPTY;
        let mut why = Vec::new();
        for r in &node.direct {
            if !set.contains(r.effect) {
                set = set.with(r.effect);
                why.push(*r);
            }
        }
        sets.push(set);
        reasons.push(why);
    }
    // Then rounds: each reads the previous one's sets only, so the order functions are
    // visited in cannot change a result.
    loop {
        let before = sets.clone();
        let mut changed = false;
        for (n, node) in nodes.iter().enumerate() {
            for &(j, at) in &node.edges {
                let Some(theirs) = before.get(j).copied() else {
                    continue;
                };
                let Some(callee) = nodes.get(j).map(|m| m.id) else {
                    continue;
                };
                for effect in theirs.iter() {
                    if let (Some(set), Some(why)) = (sets.get_mut(n), reasons.get_mut(n)) {
                        if !set.contains(effect) {
                            *set = set.with(effect);
                            why.push(EffectReason {
                                effect,
                                at,
                                source: EffectSource::Inferred(callee),
                            });
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Every function's effects first, so a witness can follow any of them.
    for (n, node) in nodes.iter().enumerate() {
        let (Some(set), Some(why)) = (sets.get(n).copied(), reasons.get_mut(n)) else {
            continue;
        };
        why.sort_by_key(|r| r.effect.id());
        if node.f.body.is_some() {
            res.record_effects(node.id, set, why.clone());
        }
    }
    for (n, node) in nodes.iter().enumerate() {
        let (Some(set), Some(why)) = (sets.get(n).copied(), reasons.get(n)) else {
            continue;
        };
        let (Some(declared), Some(decl)) = (node.declared, node.f.effects.as_ref()) else {
            continue;
        };
        if node.f.body.is_none() {
            continue;
        }
        // Authority first, then effects: one deterministic order for a body wrong in both.
        authority(node, &nodes, &sets, module, &contract, res, errors);
        let missing = set.without(declared);
        if missing.is_empty() {
            continue;
        }
        errors.push(undeclared(node, declared, decl, missing, why, res));
    }
}

/// The diagnostic for a body that needs more than its declaration allows: at the first
/// thing that needs the first missing effect, with the rest of its witness as labels.
fn undeclared(
    node: &Node<'_>,
    declared: EffectSet,
    decl: &EffectDecl,
    missing: EffectSet,
    why: &[EffectReason],
    res: &Resolution,
) -> Diagnostic {
    let name = &node.f.name;
    let listed = missing
        .iter()
        .map(|e| format!("`{e}`"))
        .collect::<Vec<_>>()
        .join(" and ");
    let first = missing.iter().next().unwrap_or(Effect::Io);
    let reason = why.iter().find(|r| r.effect == first).copied();
    let at = reason.map_or(node.f.name_span, |r| r.at);
    let mut d = Diagnostic::error(
        codes::UNDECLARED_EFFECT,
        at,
        format!("`{name}` declares `! {declared}`, but its body requires {listed}"),
    );
    if let Some(r) = reason {
        d.primary.message = describe(r, res);
        // Follow the chain through this module's inferred functions to where it ends.
        let mut hop = r;
        let mut seen = 0;
        while let EffectSource::Inferred(g) = hop.source {
            seen += 1;
            let Some(next) = res
                .function(g)
                .and_then(|def| def.effect_reasons.iter().find(|x| x.effect == first))
                .copied()
            else {
                break;
            };
            if seen > 64 {
                break;
            }
            let owner = res.function(g).map_or("it", |def| def.name.as_str());
            d = d.secondary(next.at, format!("in `{owner}`: {}", describe(next, res)));
            hop = next;
        }
    }
    let wider = declared.union(missing);
    d.secondary(decl.span, format!("declared `! {declared}` here"))
        .fix_with_review(
            decl.span,
            format!("! {wider}"),
            format!("declare `! {wider}`"),
            format!(
                "`{name}` is meant to exercise {listed}. Widening a contract can break a caller \
                 whose own declared set lacks {listed}"
            ),
        )
        .help(
            "a declared effect set is a contract: the body may require no effect outside it. \
             Declare the effect, or stop calling what requires it",
        )
}

/// One hop of a witness, in words.
fn describe(r: EffectReason, res: &Resolution) -> String {
    let name = |g: FnId| {
        res.function(g)
            .map_or_else(|| "a function".to_owned(), |d| format!("`{}`", d.name))
    };
    let e = r.effect;
    match r.source {
        EffectSource::Builtin(b) => format!("the built-in `{}` requires `{e}`", b.name()),
        EffectSource::Spawn => "`spawn` starts a task, which requires `spawn`".to_owned(),
        EffectSource::Inferred(g) => format!("calls {}, which requires `{e}`", name(g)),
        EffectSource::Declared(g) => {
            format!("calls {}, whose declared set includes `{e}`", name(g))
        }
        EffectSource::Undeclared(g) => format!(
            "calls {}, which is in another module and declares no effect set, so every \
             effect is assumed of it",
            name(g)
        ),
    }
}

/// What, at one call, requires a capability (N37).
#[derive(Clone, Copy)]
enum Needs {
    /// A built-in whose authority is its identity's.
    Builtin(nazm_sema::Intrinsic),
    /// A `spawn`, which needs the authority to start a task.
    Spawn,
    /// A function with no declared effect set, which exercises its **caller's** authority —
    /// N37's compatibility bridge. The caller must hold what the callee's effects need.
    Ambient { callee: FnId, local: bool },
}

/// Check that a function with a declared effect set holds, at every call, the authority
/// what it calls requires (N37).
///
/// Authority is lexical: the capability-typed bindings a name reaches at the call, which
/// the checker recorded on the call site ([`nazm_sema::CallSite::held`]). A built-in needs
/// its identity's authority and a `spawn` the authority to start a task. A callee with a
/// declared set needs nothing here — its contract takes its authority as parameters, which
/// ordinary type checking has already made the caller supply. A callee with **no** declared
/// set exercises its caller's authority, so the caller must hold what its effects need: its
/// inferred set in this module, and every effect across one. That is what keeps the bridge
/// from being a backdoor — code that states its contract cannot reach ambient authority
/// through code that does not.
///
/// One diagnostic per missing kind, at the first call that needs it.
fn authority(
    node: &Node<'_>,
    nodes: &[Node<'_>],
    sets: &[EffectSet],
    module: nazm_sema::module::ModuleId,
    contract: &dyn Fn(FnId) -> Option<Option<EffectSet>>,
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let sites = res
        .function(node.id)
        .map(|d| d.call_sites.clone())
        .unwrap_or_default();
    let mut reported = CapabilitySet::EMPTY;
    for site in sites {
        let mut needs: Vec<(CapabilitySet, Needs)> = Vec::new();
        if site.spawned {
            needs.push((CapabilityKind::to_start_a_task(), Needs::Spawn));
        }
        match site.callee {
            Callee::Builtin(b) => needs.push((CapabilityKind::of_intrinsic(b), Needs::Builtin(b))),
            Callee::Function(g) => {
                let local = res.function(g).is_some_and(|d| d.module == module);
                let effects = if local {
                    nodes.iter().position(|m| m.id == g).and_then(|j| {
                        let m = nodes.get(j)?;
                        // Declared: the contract carries its own authority. Unparsed: already
                        // an error, and requiring nothing adds no second one.
                        if m.declared.is_some() || m.f.body.is_none() {
                            return None;
                        }
                        sets.get(j).copied()
                    })
                } else {
                    match contract(g) {
                        Some(Some(_)) => None,
                        _ => Some(EffectSet::all()),
                    }
                };
                if let Some(effects) = effects {
                    needs.push((
                        CapabilitySet::for_effects(effects),
                        Needs::Ambient { callee: g, local },
                    ));
                }
            }
        }
        for (required, why) in needs {
            for kind in required.without(site.held).without(reported).iter() {
                reported = reported.with(kind);
                errors.push(missing_authority(node, kind, site.span, why, res));
            }
        }
    }
}

/// The effect a capability kind authorises, for a witness to follow.
fn authorised(kind: CapabilityKind) -> Option<Effect> {
    Effect::ALL
        .into_iter()
        .find(|e| CapabilityKind::for_effect(*e).contains(kind))
}

fn missing_authority(
    node: &Node<'_>,
    kind: CapabilityKind,
    at: Span,
    why: Needs,
    res: &Resolution,
) -> Diagnostic {
    let name = &node.f.name;
    let name_of = |g: FnId| {
        res.function(g)
            .map_or_else(|| "a function".to_owned(), |d| format!("`{}`", d.name))
    };
    let what = match why {
        Needs::Builtin(b) => format!("the built-in `{}`", b.name()),
        Needs::Spawn => "starting a task".to_owned(),
        Needs::Ambient { callee, .. } => format!("calling {}", name_of(callee)),
    };
    let mut d = Diagnostic::error(
        codes::MISSING_AUTHORITY,
        at,
        format!("{what} requires `{kind}` authority, and `{name}` holds no `{kind}` here"),
    );
    d.primary.message = match why {
        Needs::Builtin(b) => format!(
            "`{}` is authorised by {} `{kind}`",
            b.name(),
            kind.article()
        ),
        Needs::Spawn => format!("`spawn` is authorised by {} `{kind}`", kind.article()),
        Needs::Ambient {
            callee,
            local: true,
        } => format!(
            "{} declares no effect set, so it exercises its caller's authority, and it \
             requires `{}`",
            name_of(callee),
            authorised(kind).map_or("an effect", Effect::name)
        ),
        Needs::Ambient {
            callee,
            local: false,
        } => format!(
            "{} is in another module and declares no effect set, so it may exercise every \
             effect with its caller's authority",
            name_of(callee)
        ),
    };
    // Through this module's undeclared functions to what needs the authority.
    if let (
        Needs::Ambient {
            callee,
            local: true,
        },
        Some(effect),
    ) = (why, authorised(kind))
    {
        let mut g = callee;
        for _ in 0..64 {
            let Some(def) = res.function(g) else { break };
            let Some(next) = def.effect_reasons.iter().find(|r| r.effect == effect) else {
                break;
            };
            d = d.secondary(
                next.at,
                format!("in `{}`: {}", def.name, describe(*next, res)),
            );
            match next.source {
                EffectSource::Inferred(h) => g = h,
                _ => break,
            }
        }
    }
    if let Some(decl) = node.f.effects.as_ref() {
        d = d.secondary(
            decl.span,
            "an effect set says what a function does, not what allows it",
        );
    }
    d.help(format!(
        "authority is a value: give `{name}` {} `{kind}` parameter and have each caller pass \
         one. The only source of one is `main`'s parameters",
        kind.article()
    ))
}
