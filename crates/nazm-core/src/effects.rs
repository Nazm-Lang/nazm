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
pub(crate) fn declared(
    decl: &EffectDecl,
    param: Option<&str>,
    errors: &mut Vec<Diagnostic>,
) -> Option<EffectSet> {
    let mut set = EffectSet::EMPTY;
    let mut known = true;
    for (i, (name, span)) in decl.names.iter().enumerate() {
        // The function's effect parameter, by the name it declared (N51), shadows nothing:
        // it cannot be spelled like a built-in effect's word without being refused there.
        let effect = if Some(name.as_str()) == param && Effect::by_name(name).is_none() {
            Some(Effect::Param)
        } else {
            Effect::by_name(name)
        };
        match effect {
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

/// One of the module's functions or closures, as the analysis sees it.
struct Node {
    id: FnId,
    /// How a message names it: `` `f` ``, or "a closure in `f`" (N50).
    label: String,
    name_span: Span,
    /// Whether its body parsed; one that did not requires nothing anyone can know.
    has_body: bool,
    foreign: bool,
    /// The span of its written effect set. A closure that writes none declares `! {}`.
    written: Option<Span>,
    /// Its effect parameter's name, for a message (N51): the function's own, or for a
    /// closure the enclosing function's.
    param: String,
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
    let mut nodes: Vec<Node> = Vec::new();
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
            label: format!("`{}`", f.name),
            name_span: f.name_span,
            has_body: f.body.is_some(),
            foreign: f.foreign.is_some(),
            written: f.effects.as_ref().map(|d| d.span),
            param: res
                .types()
                .effect_param_name(Some(nazm_sema::ParamOwner::Function(id))),
            declared: def.declared_effects,
            direct: Vec::new(),
            edges: Vec::new(),
        });
    }
    // Then the closures their bodies wrote (N50): each a function of its own, with a declared
    // set — `! {}` unwritten — that its body is checked against. Nothing calls a closure by
    // name, so none is an edge's target: a call through a value is its type's contract.
    let closures: Vec<Node> = res
        .functions()
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let info = d.closure.as_ref()?;
            (d.module == module).then(|| Node {
                id: FnId::from_index(i),
                label: format!(
                    "this closure in `{}`",
                    d.name.split('#').next().unwrap_or("")
                ),
                name_span: d.name_span,
                has_body: true,
                foreign: false,
                written: info.written_effects,
                param: res.types().effect_param_name(match info.ty {
                    nazm_sema::Type::Fn(f) => res.types().fn_signature(f).and_then(|s| s.row),
                    _ => None,
                }),
                declared: d.declared_effects,
                direct: Vec::new(),
                edges: Vec::new(),
            })
        })
        .collect();
    nodes.extend(closures);
    let index = |id: FnId, nodes: &[Node]| nodes.iter().position(|n| n.id == id);
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
                Callee::Indirect(t) => {
                    // What a function value may do is its type's (N50): a contract.
                    let effects = match t {
                        nazm_sema::Type::Fn(f) => res
                            .types()
                            .fn_signature(f)
                            .map_or(EffectSet::EMPTY, |s| s.effects),
                        _ => EffectSet::EMPTY,
                    };
                    direct.extend(effects.iter().map(|effect| EffectReason {
                        effect,
                        at,
                        source: EffectSource::Indirect(t),
                    }));
                }
                Callee::Function(g) => {
                    let local = res
                        .function(g)
                        .is_some_and(|d| d.module == module)
                        .then(|| index(g, &nodes))
                        .flatten();
                    // A callee's effect parameter, as this call bound it (N51): the empty set
                    // where it has none to bind.
                    let binding = res.effect_binding(at);
                    match local {
                        Some(j) => match nodes.get(j).and_then(|m| m.declared) {
                            Some(set) => {
                                direct.extend(set.bound(binding).iter().map(|effect| {
                                    EffectReason {
                                        effect,
                                        at,
                                        source: EffectSource::Declared(g),
                                    }
                                }));
                            }
                            // A body that did not parse requires nothing anyone can know;
                            // it is already an error, and guessing would add a second.
                            None if nodes.get(j).is_some_and(|m| !m.has_body) => {}
                            None => edges.push((j, at)),
                        },
                        None => match contract(g) {
                            Some(Some(set)) => {
                                direct.extend(set.bound(binding).iter().map(|effect| {
                                    EffectReason {
                                        effect,
                                        at,
                                        source: EffectSource::Declared(g),
                                    }
                                }));
                            }
                            // Imported and undeclared, or not found at all: every effect
                            // the bridge covers, and `foreign` if it can be handed the
                            // authority for it.
                            _ => direct.extend(assumed(g, res).iter().map(|effect| EffectReason {
                                effect,
                                at,
                                source: EffectSource::Undeclared(g),
                            })),
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
        if node.foreign {
            // A foreign function's body is C: what it requires is its contract, `foreign`.
            res.record_effects(node.id, EffectSet::of(Effect::Foreign), Vec::new());
        } else if node.has_body {
            res.record_effects(node.id, set, why.clone());
        }
    }
    // A named function used as a value promised its type's effects (N50): an undeclared one
    // of this module was taken to require none, and is held to it here.
    for value in res.fn_values() {
        let Some(n) = index(value.function, &nodes) else {
            continue;
        };
        let (Some(node), Some(set)) = (nodes.get(n), sets.get(n).copied()) else {
            continue;
        };
        if node.declared.is_some() || !node.has_body {
            continue;
        }
        let missing = set.without(value.assumed);
        if missing.is_empty() {
            continue;
        }
        let listed = missing
            .iter()
            .map(|e| format!("`{e}`"))
            .collect::<Vec<_>>()
            .join(" and ");
        let first = missing.iter().next().unwrap_or(Effect::Io);
        let mut d = Diagnostic::error(
            codes::UNDECLARED_EFFECT,
            value.at,
            format!(
                "{} is used as a value whose type requires no effect, but it requires {listed}",
                node.label
            ),
        );
        if let Some(r) = reasons
            .get(n)
            .and_then(|why| why.iter().find(|r| r.effect == first))
        {
            d = d.secondary(r.at, describe(*r, res));
        }
        errors.push(d.help(format!(
            "a function value's type carries its effects, and an undeclared function's are \
             taken to be none: declare `! {}` on {}",
            set_text(missing, "*"),
            node.label
        )));
    }
    for (n, node) in nodes.iter().enumerate() {
        let (Some(set), Some(why)) = (sets.get(n).copied(), reasons.get(n)) else {
            continue;
        };
        let Some(declared) = node.declared else {
            continue;
        };
        if !node.has_body {
            continue;
        }
        // Authority first, then effects: one deterministic order for a body wrong in both.
        authority(node, &nodes, &sets, module, &contract, res, errors);
        let missing = set.without(declared);
        if missing.is_empty() {
            continue;
        }
        errors.push(undeclared(node, declared, missing, why, res));
    }
    // A foreign call needs a `ForeignCap` held, in every function (N42): N37's bridge does not
    // reach it.
    for node in &nodes {
        if !node.has_body {
            continue;
        }
        let sites = res
            .function(node.id)
            .map(|d| d.call_sites.clone())
            .unwrap_or_default();
        for site in sites {
            let Callee::Function(g) = site.callee else {
                continue;
            };
            // This module's own declaration is read from the resolution; another module's is
            // what its interface publishes, as its effect contract is — the importer does not
            // read the declaration.
            let foreign = match res.function(g) {
                Some(d) if d.module == module => d.foreign.is_some(),
                _ => deps
                    .iter()
                    .flat_map(|d| d.exports())
                    .any(|e| e.def == g && e.foreign.is_some()),
            };
            if foreign && !site.held.contains(CapabilityKind::Foreign) {
                errors.push(missing_authority(
                    node,
                    CapabilityKind::Foreign,
                    site.span,
                    Needs::Foreign { callee: g },
                    res,
                ));
                break;
            }
        }
    }
}

/// What a caller must assume of `g`, a function in another module that declares no effect
/// set: every effect the bridge covers, and `foreign` too if one of its parameters can carry
/// a `ForeignCap` — the only way it could reach C.
pub fn assumed(g: FnId, res: &Resolution) -> EffectSet {
    let receives = res.function(g).is_some_and(|d| {
        d.params
            .iter()
            .any(|t| carries(*t, CapabilityKind::Foreign, res, 0))
    });
    if receives {
        EffectSet::bridged().with(Effect::Foreign)
    } else {
        EffectSet::bridged()
    }
}

/// Whether a value of `t` can carry a capability of this kind: it is one, or a record or enum
/// field it holds is, transitively.
fn carries(t: nazm_sema::Type, kind: CapabilityKind, res: &Resolution, depth: usize) -> bool {
    if depth > nazm_sema::MAX_TYPE_DEPTH * 4 {
        return true;
    }
    match t {
        nazm_sema::Type::Cap(k) => k == kind,
        nazm_sema::Type::Record(id) => res
            .records()
            .get(id)
            .is_some_and(|r| r.fields.iter().any(|f| carries(f.ty, kind, res, depth + 1))),
        nazm_sema::Type::Enum(id) => res.enums().get(id).is_some_and(|e| {
            e.variants
                .iter()
                .any(|v| v.fields.iter().any(|f| carries(f.ty, kind, res, depth + 1)))
        }),
        nazm_sema::Type::Vec(v) => res
            .types()
            .vec_element(v)
            .is_some_and(|e| carries(e, kind, res, depth + 1)),
        _ => false,
    }
}

/// The diagnostic for a body that needs more than its declaration allows: at the first
/// thing that needs the first missing effect, with the rest of its witness as labels.
fn undeclared(
    node: &Node,
    declared: EffectSet,
    missing: EffectSet,
    why: &[EffectReason],
    res: &Resolution,
) -> Diagnostic {
    let name = &node.label;
    let param = &node.param;
    let listed = missing
        .iter()
        .map(|e| format!("`{}`", effect_text(e, param)))
        .collect::<Vec<_>>()
        .join(" and ");
    let first = missing.iter().next().unwrap_or(Effect::Io);
    let reason = why.iter().find(|r| r.effect == first).copied();
    let at = reason.map_or(node.name_span, |r| r.at);
    let shown = set_text(declared, param);
    let mut d = Diagnostic::error(
        codes::UNDECLARED_EFFECT,
        at,
        format!("{name} declares `! {shown}`, but its body requires {listed}"),
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
    let wider = set_text(declared.union(missing), param);
    let d = match node.written {
        Some(written) => d
            .secondary(written, format!("declared `! {shown}` here"))
            .fix_with_review(
                written,
                format!("! {wider}"),
                format!("declare `! {wider}`"),
                format!(
                    "{name} is meant to exercise {listed}. Widening a contract can break a \
                     caller whose own declared set lacks {listed}"
                ),
            ),
        // A closure that writes no set declares `! {}` (N50).
        None => d
            .secondary(
                node.name_span,
                "a closure that writes no effect set declares `! {}`",
            )
            .help(format!(
                "write `! {wider}` after the closure's result type, and give it the authority \
                 as a parameter"
            )),
    };
    d.help(
        "a declared effect set is a contract: the body may require no effect outside it. \
             Declare the effect, or stop calling what requires it",
    )
}

/// An effect as source writes it: the effect parameter by its declared name (N51).
fn effect_text(e: Effect, param: &str) -> String {
    if e == Effect::Param {
        param.to_owned()
    } else {
        e.name().to_owned()
    }
}

/// A set as source writes it, `{}` or `{ io, E }`.
fn set_text(set: EffectSet, param: &str) -> String {
    if set.is_empty() {
        return "{}".to_owned();
    }
    let names: Vec<String> = set.iter().map(|e| effect_text(e, param)).collect();
    format!("{{ {} }}", names.join(", "))
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
        EffectSource::Indirect(t) => format!(
            "calls a function value of type `{}`, whose type includes `{e}`",
            res.types().show(t)
        ),
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
    /// A foreign function (N42), which needs a `ForeignCap` wherever it is called.
    Foreign { callee: FnId },
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
    node: &Node,
    nodes: &[Node],
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
            // A function value's type is a contract (N50): what it may do, it takes the
            // authority for as parameters, which type checking has made the caller supply.
            Callee::Indirect(_) => {}
            Callee::Function(g) => {
                let local = res.function(g).is_some_and(|d| d.module == module);
                let effects = if local {
                    nodes.iter().position(|m| m.id == g).and_then(|j| {
                        let m = nodes.get(j)?;
                        // Declared: the contract carries its own authority. Unparsed: already
                        // an error, and requiring nothing adds no second one.
                        if m.declared.is_some() || !m.has_body {
                            return None;
                        }
                        sets.get(j).copied()
                    })
                } else {
                    match contract(g) {
                        Some(Some(_)) => None,
                        _ => Some(assumed(g, res)),
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
    // A value of an undeclared function is made with the authority its type's effects need,
    // as a call to it would be (N51): calling a value needs none, so this is the one place
    // declared code could otherwise reach the bridge.
    for value in res.fn_values() {
        if value.within != Some(node.id) {
            continue;
        }
        let undeclared = res
            .function(value.function)
            .is_some_and(|d| d.declared_effects.is_none() && d.foreign.is_none());
        if !undeclared {
            continue;
        }
        let local = res
            .function(value.function)
            .is_some_and(|d| d.module == module);
        let required = CapabilitySet::for_effects(value.assumed);
        for kind in required.without(value.held).without(reported).iter() {
            reported = reported.with(kind);
            errors.push(missing_authority(
                node,
                kind,
                value.at,
                Needs::Ambient {
                    callee: value.function,
                    local,
                },
                res,
            ));
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
    node: &Node,
    kind: CapabilityKind,
    at: Span,
    why: Needs,
    res: &Resolution,
) -> Diagnostic {
    let name = &node.label;
    let name_of = |g: FnId| {
        res.function(g)
            .map_or_else(|| "a function".to_owned(), |d| format!("`{}`", d.name))
    };
    let what = match why {
        Needs::Builtin(b) => format!("the built-in `{}`", b.name()),
        Needs::Spawn => "starting a task".to_owned(),
        Needs::Ambient { callee, .. } | Needs::Foreign { callee } => {
            format!("calling {}", name_of(callee))
        }
    };
    let mut d = Diagnostic::error(
        codes::MISSING_AUTHORITY,
        at,
        format!("{what} requires `{kind}` authority, and {name} holds no `{kind}` here"),
    );
    d.primary.message = match why {
        Needs::Builtin(b) => format!(
            "`{}` is authorised by {} `{kind}`",
            b.name(),
            kind.article()
        ),
        Needs::Spawn => format!("`spawn` is authorised by {} `{kind}`", kind.article()),
        Needs::Foreign { callee } => format!(
            "{} is a foreign function: calling C is authorised by {} `{kind}`, with or \
             without an effect set",
            name_of(callee),
            kind.article()
        ),
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
    if let Some(written) = node.written {
        d = d.secondary(
            written,
            "an effect set says what a function does, not what allows it",
        );
    }
    d.help(format!(
        "authority is a value: give {name} {} `{kind}` parameter and have each caller pass \
         one. The only source of one is `main`'s parameters",
        kind.article()
    ))
}
