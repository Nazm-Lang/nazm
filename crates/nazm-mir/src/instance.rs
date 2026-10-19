//! Which concrete generic function instances a native build needs, before any is lowered.
//!
//! # Parametric semantics, concrete native code
//!
//! `nazm check` and `nazm run` are parametric: a generic function is checked once and
//! interpreted as itself. The LLVM backend **monomorphises** — one native function per
//! concrete type-argument list — because its values have concrete layouts and its copy and
//! release code is type-specific. That is a decision about this backend, not about the
//! language (`docs/internal/architecture.md` §7.12), and this module is where it is made.
//!
//! # The plan is global, deterministic, and finite by construction
//!
//! A worklist, from the **roots** — every call a non-generic function makes to a generic
//! one, whose arguments are therefore concrete — through every call an instance's body
//! makes, under that instance's substitution, until nothing new appears. It reads the call
//! sites the checker recorded ([`nazm_sema::FnDef::call_sites`]) rather than the syntax
//! tree, so planning decides nothing the checker did not already decide.
//!
//! Order is fixed by the resolution, never by a map or a thread, and the finished list is
//! **sorted by identity** — definition, then the arguments' canonical keys — so the set of
//! instances, their names and their order are functions of the program alone.
//!
//! Moved here from `nazm-lir` by N40, unchanged in what it plans: MIR is post-instantiation
//! (`docs/internal/architecture.md` §7.42), so the decision of which instances exist belongs to the
//! level that creates them, and a backend reads the result.
//!
//! Finite because the only way an instance family grows without bound is a generic
//! function reaching itself with a *larger* argument — `f[T]` calling `f[Vec[T]]` — and
//! that is refused first, by the expanding-cycle rule below (`N0357`). A stated budget
//! (`N0358`) is the second line, not the first.

use std::collections::HashMap;

use nazm_diag::{codes, Diagnostic};
use nazm_sema::key::DefKind;
use nazm_sema::module::ModuleGraph;
use nazm_sema::resolve::{Callee, FnId, Resolution};
use nazm_sema::types::{ParamOwner, Type, TypeParam};
use nazm_span::Span;

use crate::ir::Identity;
use crate::names::{self, type_key};

/// How many concrete instances one program may need.
///
/// Far past anything written by hand, and stated so that a program that needs more gets a
/// diagnostic rather than a build that exhausts memory. It is a bound on what this
/// backend will generate, not on the language.
pub const MAX_INSTANCES: usize = 10_000;

/// How deeply an instance's type arguments may nest.
///
/// Twice the language's bound on a written type: an argument can be built by substituting
/// one written type into another, and this exists to stop a runaway, not to refuse what a
/// program could reasonably write.
pub const MAX_INSTANCE_NESTING: usize = 2 * nazm_sema::MAX_TYPE_DEPTH;

/// One concrete instance: a generic definition and its concrete type arguments.
///
/// The **generic instance key**. Its durable spelling is its [`Identity`] — the definition's
/// `DefKey` and the arguments' canonical keys, nothing session-local — which is also what a
/// backend derives its symbol from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceKey {
    pub def: FnId,
    pub args: Vec<Type>,
}

/// One planned instance: its key, its durable identity, and its signature under its
/// substitution.
#[derive(Debug, Clone)]
pub struct Planned {
    pub key: InstanceKey,
    pub identity: Identity,
    pub params: Vec<Type>,
    pub ret: Type,
}

/// Every instance a program needs, sorted by identity.
#[derive(Debug, Clone, Default)]
pub struct Instances {
    pub planned: Vec<Planned>,
    index: HashMap<InstanceKey, usize>,
}

impl Instances {
    /// The planned instance for this key, if the plan has it.
    #[must_use]
    pub fn get(&self, key: &InstanceKey) -> Option<&Planned> {
        self.index.get(key).and_then(|i| self.planned.get(*i))
    }

    /// Where an instance sits in [`Self::planned`].
    #[must_use]
    pub fn position(&self, key: &InstanceKey) -> Option<usize> {
        self.index.get(key).copied()
    }
}

/// Plan every generic function instance the program needs.
///
/// Interns every type an instance mentions under its substitution — its locals, what it
/// constructs, what it calls — so that lowering can substitute by lookup and never has to
/// create a type while writing code.
///
/// # Errors
///
/// `N0357` for polymorphic recursion and `N0358` past the stated bounds.
pub fn plan_instances(
    res: &mut Resolution,
    graph: &ModuleGraph,
) -> Result<Instances, Vec<Diagnostic>> {
    let errors = expanding_cycles(res);
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut work: std::collections::VecDeque<(InstanceKey, Span)> =
        std::collections::VecDeque::new();
    let mut seen: HashMap<InstanceKey, usize> = HashMap::new();
    let mut order: Vec<(InstanceKey, Span)> = Vec::new();

    // A generic function's closures (R1b, L10), each instantiated with it.
    let mut closures_of: HashMap<FnId, Vec<FnId>> = HashMap::new();
    for i in 0..res.functions().len() {
        let c = FnId::from_index(i);
        if let Some(root) = res.type_owner(c).filter(|r| *r != c) {
            closures_of.entry(root).or_default().push(c);
        }
    }

    // A trait's default body at every type whose impl leaves the method to it (R1b, L11): a
    // call through a bound reaches it at the instance's type, which nothing else records.
    let traits: Vec<(nazm_sema::traits::TraitId, Type, usize)> = res
        .traits()
        .impls()
        .map(|(_, imp)| (imp.trait_id, imp.self_ty, imp.methods.len()))
        .collect();
    for (trait_id, ty, n) in traits {
        // An impl of a generic type (L11b) is filed under its definition, which no value has:
        // its defaults are planned where a call reaches one of its instances.
        let generic = match ty {
            Type::Record(id) => res.records().get(id).is_some_and(|d| !d.params.is_empty()),
            Type::Enum(id) => res.enums().get(id).is_some_and(|d| !d.params.is_empty()),
            _ => false,
        };
        if generic {
            continue;
        }
        for m in 0..n {
            let Some((d, args)) = res.method_target(trait_id, ty, u32::try_from(m).unwrap_or(0))
            else {
                continue;
            };
            if args.is_empty() {
                continue;
            }
            if !res.types().is_concrete(ty) {
                continue;
            }
            let key = InstanceKey { def: d, args };
            if !seen.contains_key(&key) {
                let at = res.function(d).map_or(Span::synthetic(), |f| f.name_span);
                seen.insert(key.clone(), order.len());
                order.push((key.clone(), at));
                work.push_back((key, at));
            }
        }
    }

    // The roots: what the non-generic functions call, in definition order and then in the
    // order each body makes its calls.
    for (i, f) in res.functions().iter().enumerate() {
        if res.type_owner(FnId::from_index(i)).is_some() {
            continue;
        }
        for site in &f.call_sites {
            if let Callee::Function(g) = site.callee {
                if is_generic(res, g) {
                    let key = InstanceKey {
                        def: g,
                        args: site.type_args.clone(),
                    };
                    if !seen.contains_key(&key) {
                        seen.insert(key.clone(), order.len());
                        order.push((key.clone(), site.span));
                        work.push_back((key, site.span));
                    }
                }
            }
        }
    }

    while let Some((key, at)) = work.pop_front() {
        if order.len() > MAX_INSTANCES {
            return Err(vec![Diagnostic::error(
                codes::GENERIC_INSTANCE_LIMIT,
                at,
                format!("this program needs more than {MAX_INSTANCES} generic function instances"),
            )
            .help(
                "`nazm build` generates one native function per distinct list of type \
                 arguments, and bounds how many one program may need. `nazm run` has no \
                 such bound: it does not specialise",
            )]);
        }
        if key
            .args
            .iter()
            .any(|a| res.types().nesting(*a) > MAX_INSTANCE_NESTING)
        {
            return Err(vec![Diagnostic::error(
                codes::GENERIC_INSTANCE_LIMIT,
                at,
                format!(
                    "a generic instance here would have type arguments nested more than \
                     {MAX_INSTANCE_NESTING} deep"
                ),
            )
            .help(
                "the bound is on what `nazm build` generates; `nazm run` has none",
            )]);
        }
        let owner = ParamOwner::Function(res.type_owner(key.def).unwrap_or(key.def));
        // Its closures, under its substitution (L10).
        for c in closures_of.get(&key.def).cloned().unwrap_or_default() {
            let next = InstanceKey {
                def: c,
                args: key.args.clone(),
            };
            if !seen.contains_key(&next) {
                seen.insert(next.clone(), order.len());
                order.push((next.clone(), at));
                work.push_back((next, at));
            }
        }
        let (sites, mentions) = match res.function(key.def) {
            Some(d) => (d.call_sites.clone(), d.mentions.clone()),
            None => continue,
        };
        // Everything this body mentions, under its substitution, interned now.
        for t in mentions {
            let _ = res.types_mut().substitute(t, owner, &key.args);
        }
        for site in sites {
            let args: Vec<Type> = site
                .type_args
                .iter()
                .map(|t| res.types_mut().substitute(*t, owner, &key.args))
                .collect();
            // A call through a bound (N78), at this instance's type: an impl of a generic type's
            // method, or a default body, is an instance too (L11).
            if let Some(call) = res.traits().call(site.span) {
                let concrete = res.types_mut().substitute(call.self_ty, owner, &key.args);
                if let Some((g, targs)) = res.method_target(call.trait_id, concrete, call.method) {
                    let next = InstanceKey {
                        def: g,
                        args: targs,
                    };
                    if !next.args.is_empty() && !seen.contains_key(&next) {
                        seen.insert(next.clone(), order.len());
                        order.push((next.clone(), site.span));
                        work.push_back((next, site.span));
                    }
                }
                continue;
            }
            if let Callee::Function(g) = site.callee {
                if !is_generic(res, g) {
                    continue;
                }
                let next = InstanceKey { def: g, args };
                if !seen.contains_key(&next) {
                    seen.insert(next.clone(), order.len());
                    order.push((next.clone(), site.span));
                    work.push_back((next, site.span));
                }
            }
        }
    }

    // Each instance's signature, and its identity.
    let mut planned: Vec<Planned> = Vec::with_capacity(order.len());
    for (key, _) in order {
        let owner = ParamOwner::Function(res.type_owner(key.def).unwrap_or(key.def));
        let Some(def) = res.function(key.def).cloned() else {
            continue;
        };
        let params: Vec<Type> = def
            .params
            .iter()
            .map(|t| res.types_mut().substitute(*t, owner, &key.args))
            .collect();
        let ret = res.types_mut().substitute(def.ret, owner, &key.args);
        let keys: Vec<String> = key
            .args
            .iter()
            .map(|a| type_key(*a, res.types(), graph))
            .collect();
        planned.push(Planned {
            identity: Identity {
                owner: names::owner(graph, def.module, DefKind::Function, &def.name),
                args: keys,
            },
            key,
            params,
            ret,
        });
    }
    // By identity: the order the worklist happened to find them in is not part of what the
    // program needs.
    planned.sort_by(|a, b| a.identity.cmp(&b.identity));
    let index = planned
        .iter()
        .enumerate()
        .map(|(i, p)| (p.key.clone(), i))
        .collect();
    Ok(Instances { planned, index })
}

fn is_generic(res: &Resolution, f: FnId) -> bool {
    res.function(f).is_some_and(|d| !d.type_params.is_empty())
}

/// One node of the expanding-cycle graph: a function and one of its parameter positions.
type ParamNode = (FnId, u32);

/// The graph's edges out of each node: the node reached, whether the edge expands, and
/// the call that makes it.
type ParamEdges = HashMap<ParamNode, Vec<(FnId, u32, bool, Span)>>;

/// Refuse polymorphic recursion: a generic function that reaches itself with a type
/// argument **larger** than the parameter it came from.
///
/// The expanding-cycle rule. Nodes are (function, parameter position); a call site in `f`
/// passing a type argument `t` to `g`'s parameter `j` adds an edge from each of `f`'s
/// parameters `i` that `t` mentions to `(g, j)` — *expanding* when `t` is more than `f`'s
/// parameter itself. An instance family is infinite exactly when some cycle of this graph
/// contains an expanding edge, so that is what is refused, with the call that expands.
fn expanding_cycles(res: &Resolution) -> Vec<Diagnostic> {
    // Edges, and each expanding edge's call site.
    let mut edges: ParamEdges = HashMap::new();
    let mut nodes: Vec<(FnId, u32)> = Vec::new();
    for (i, f) in res.functions().iter().enumerate() {
        let me = FnId::from_index(i);
        for p in 0..f.type_params.len() {
            nodes.push((me, u32::try_from(p).unwrap_or(u32::MAX)));
        }
        // A closure's calls are its generic function's (L10): written over its parameters.
        let Some(from) = res.type_owner(me) else {
            continue;
        };
        let Some(params) = res.function(from).map(|d| d.type_params.len()) else {
            continue;
        };
        for site in &f.call_sites {
            let Callee::Function(to) = site.callee else {
                continue;
            };
            for (j, t) in site.type_args.iter().enumerate() {
                for p in 0..params {
                    let param = TypeParam {
                        owner: ParamOwner::Function(from),
                        index: u32::try_from(p).unwrap_or(u32::MAX),
                    };
                    if !mentions_param(res, *t, param) {
                        continue;
                    }
                    let expanding = *t != Type::Param(param);
                    edges.entry((from, param.index)).or_default().push((
                        to,
                        u32::try_from(j).unwrap_or(u32::MAX),
                        expanding,
                        site.span,
                    ));
                }
            }
        }
    }

    // Strongly connected components, iteratively (Tarjan with an explicit stack).
    let component = components(&nodes, &edges);
    let mut errors = Vec::new();
    let mut reported: Vec<usize> = Vec::new();
    // In node order and then edge order, so which call is named is a function of the
    // program rather than of a map's iteration.
    for from in &nodes {
        let Some(c) = component.get(from).copied() else {
            continue;
        };
        let Some(out) = edges.get(from) else {
            continue;
        };
        for (to, j, expanding, span) in out {
            if !*expanding || component.get(&(*to, *j)).copied() != Some(c) || reported.contains(&c)
            {
                continue;
            }
            reported.push(c);
            let name = res
                .function(from.0)
                .map_or("this function", |f| f.name.as_str());
            errors.push(
                Diagnostic::error(
                    codes::UNBOUNDED_SPECIALISATION,
                    *span,
                    format!(
                        "`{name}` reaches itself with a larger type argument here, so its \
                         native instances would never end"
                    ),
                )
                .help(
                    "`nazm build` generates one native function per list of type \
                     arguments, and this call makes each instance need a new one. \
                     `nazm run` runs it: it does not specialise",
                ),
            );
        }
    }
    // Deterministic, whatever order the map yielded them in.
    errors.sort_by_key(|d| (d.primary.span.file.index(), d.primary.span.start));
    errors
}

/// Whether `t` mentions this one parameter.
fn mentions_param(res: &Resolution, t: Type, p: TypeParam) -> bool {
    let mut work = vec![t];
    while let Some(t) = work.pop() {
        if t == Type::Param(p) {
            return true;
        }
        work.extend(res.types().type_args(t));
    }
    false
}

/// Tarjan's strongly connected components, with an explicit stack.
fn components(nodes: &[(FnId, u32)], edges: &ParamEdges) -> HashMap<(FnId, u32), usize> {
    let mut index: HashMap<(FnId, u32), usize> = HashMap::new();
    let mut low: HashMap<(FnId, u32), usize> = HashMap::new();
    let mut on: Vec<(FnId, u32)> = Vec::new();
    let mut out: HashMap<(FnId, u32), usize> = HashMap::new();
    let mut next = 0usize;
    let mut count = 0usize;
    let succ = |n: (FnId, u32)| -> Vec<(FnId, u32)> {
        edges
            .get(&n)
            .map(|e| e.iter().map(|(f, j, _, _)| (*f, *j)).collect())
            .unwrap_or_default()
    };
    for root in nodes {
        if index.contains_key(root) {
            continue;
        }
        let mut stack: Vec<((FnId, u32), usize)> = vec![(*root, 0)];
        index.insert(*root, next);
        low.insert(*root, next);
        next += 1;
        on.push(*root);
        while let Some((node, at)) = stack.last().copied() {
            let children = succ(node);
            if let Some(child) = children.get(at).copied() {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                if let std::collections::hash_map::Entry::Vacant(slot) = index.entry(child) {
                    slot.insert(next);
                    low.insert(child, next);
                    next += 1;
                    on.push(child);
                    stack.push((child, 0));
                } else if on.contains(&child) {
                    let l = low.get(&node).copied().unwrap_or(0);
                    let i = index.get(&child).copied().unwrap_or(0);
                    low.insert(node, l.min(i));
                }
                continue;
            }
            stack.pop();
            if let Some((parent, _)) = stack.last().copied() {
                let lp = low.get(&parent).copied().unwrap_or(0);
                let ln = low.get(&node).copied().unwrap_or(0);
                low.insert(parent, lp.min(ln));
            }
            if low.get(&node) == index.get(&node) {
                while let Some(m) = on.pop() {
                    out.insert(m, count);
                    if m == node {
                        break;
                    }
                }
                count += 1;
            }
        }
    }
    out
}
