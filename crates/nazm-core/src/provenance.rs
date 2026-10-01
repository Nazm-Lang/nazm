//! Provenance, N38: each module's bodies reduced to flows, the whole program's summaries
//! solved from them, and the one restricted flow v1 enforces.
//!
//! # Two halves, and why
//!
//! [`facts`] reads one module's checked bodies — through resolution, never through names —
//! and writes each function as flows over its own parameters and the calls it makes. That is
//! a function of the module's source and its imports' declarations, exactly what its cache
//! key already covers, so a reused module's facts can come from its cache entry. [`solve`]
//! then settles every function's [`Summary`] over the whole program from all modules' facts,
//! every run, and checks the restriction. Nothing it concludes is cached, so no conclusion
//! can be stale: a callee's changed body changes what its caller is told in the same run.
//!
//! # The flow rules
//!
//! Explicit data flow only. A value carries what it was computed from: a binding the join of
//! everything ever assigned to it (flow-insensitively, to a fixed point, so a loop is no
//! special case), an operator its operands', a call its callee's summary applied to its
//! arguments, a record or variant its fields', a field or a pattern's binding its whole
//! value's, `?` its operand's, an `if` or `match` its arms'. A condition contributes nothing:
//! implicit flow is not tracked. A value read out of a sequence or a channel is
//! [`Origin::Unknown`] — sequences and channels are shared handles, and a local analysis
//! cannot see every alias that wrote to one.
//!
//! # The fixed point
//!
//! Summaries start empty and grow: a worklist re-reads a function's flows whenever a callee's
//! summary grows, until nothing is queued. A summary is a set of origins and a set of
//! parameter positions, both finite, so the rounds end whatever the call graph — recursion,
//! mutual recursion and import cycles included — and the result is the least fixed point.

use std::collections::{BTreeSet, HashMap};

use nazm_diag::{codes, Diagnostic};
use nazm_sema::resolve::{Callee, FnId, Resolution};
use nazm_sema::{
    BuiltinFlow, CallFact, Flow, FunctionFacts, ModuleFacts, Origin, OriginSet, SinkFact, Summary,
    Type,
};
use nazm_span::Span;
use nazm_syntax::{BinOp, Block, Expr, Function, Stmt};

// ---------------------------------------------------------------------------------------
// One module's facts.
// ---------------------------------------------------------------------------------------

/// Reduce every function of the module `functions` belong to, as checked, to flows.
pub(crate) fn facts(
    functions: &[(u32, &Function)],
    module: nazm_sema::module::ModuleId,
    res: &Resolution,
) -> ModuleFacts {
    let mut out = ModuleFacts::default();
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
        let Some(body) = &f.body else { continue };
        let entry = res.entry() == Some(id);
        let mut slots = vec![Flow::default(); def.locals.len()];
        for (i, t) in def.params.iter().enumerate() {
            let Some(slot) = slots.get_mut(i) else {
                continue;
            };
            // `main`'s parameters have no caller: what they hold is the runtime's root
            // authority, and nothing else could be (N37).
            *slot = match (entry, t) {
                (true, Type::Cap(_)) => Flow::origin(
                    Origin::Authority,
                    f.params.get(i).map_or(f.name_span, |p| p.name_span),
                ),
                (true, _) => Flow::default(),
                (false, _) => Flow::param(u32::try_from(i).unwrap_or(u32::MAX)),
            };
        }
        let (facts, slots, closures) = walk(res, id, slots, body);
        out.functions.push(facts);
        // Each closure the body wrote is a function of its own (N50), walked after the body
        // that made it, whose final flows say what each capture holds.
        let mut pending: Vec<(&Expr, Vec<Flow>)> =
            closures.into_iter().map(|c| (c, slots.clone())).collect();
        let mut guard = 0usize;
        while let Some((c, outer)) = pending.pop() {
            guard += 1;
            if guard > 4096 {
                break;
            }
            let Expr::Closure {
                params, body, span, ..
            } = c
            else {
                continue;
            };
            let Some(cid) = res.closure_at(*span) else {
                continue;
            };
            let Some(cdef) = res.function(cid) else {
                continue;
            };
            let mut slots = vec![Flow::default(); cdef.locals.len()];
            // A closure is called through a value, by callers no analysis here can name, so
            // what a parameter is handed is unknown — and so is a capture of anything that
            // depended on its maker's parameters or calls.
            for (i, p) in params.iter().enumerate() {
                if let Some(slot) = slots.get_mut(i) {
                    *slot = Flow::origin(Origin::Unknown, p.name_span);
                }
            }
            if let Some(info) = &cdef.closure {
                for cap in &info.captures {
                    let from = outer.get(cap.outer as usize).cloned().unwrap_or_default();
                    let mut held = Flow {
                        origins: from.origins,
                        why: from.why.clone(),
                        ..Flow::default()
                    };
                    if !from.params.is_empty() || !from.calls.is_empty() {
                        held = held.joined(&Flow::origin(Origin::Unknown, info.at));
                    }
                    if let Some(slot) = slots.get_mut(cap.inner as usize) {
                        *slot = held;
                    }
                }
            }
            let (facts, slots, nested) = walk(res, cid, slots, body);
            out.functions.push(facts);
            pending.extend(nested.into_iter().map(|n| (n, slots.clone())));
        }
    }
    out
}

/// One body's facts, its bindings' final flows, and the closures it wrote.
fn walk<'b>(
    res: &Resolution,
    id: FnId,
    slots: Vec<Flow>,
    body: &'b Block,
) -> (FunctionFacts, Vec<Flow>, Vec<&'b Expr>) {
    let n = slots.len();
    let mut w = Walk {
        res,
        read: vec![false; n],
        stale: false,
        slots,
        calls: Vec::new(),
        call_at: HashMap::new(),
        sinks: Vec::new(),
        sink_at: HashMap::new(),
        result: Flow::default(),
        closures: Vec::new(),
    };
    // A walk reads each binding as it stands; one read before that binding grew later in
    // the same walk is the only thing another walk could change, so walk again only then.
    // Bounded by what a flow can hold, and capped regardless.
    for _ in 0..256 {
        w.stale = false;
        w.read.iter_mut().for_each(|r| *r = false);
        let tail = w.block(body);
        w.returned(&tail);
        if !w.stale {
            break;
        }
    }
    (
        FunctionFacts {
            function: id,
            calls: w.calls,
            result: w.result,
            sinks: w.sinks,
        },
        w.slots,
        w.closures,
    )
}

struct Walk<'a, 'b> {
    res: &'a Resolution,
    slots: Vec<Flow>,
    /// Which bindings this walk has read.
    read: Vec<bool>,
    /// Whether a binding grew after this walk read it.
    stale: bool,
    calls: Vec<CallFact>,
    call_at: HashMap<(u32, u32), usize>,
    sinks: Vec<SinkFact>,
    sink_at: HashMap<(u32, u32), usize>,
    result: Flow,
    /// The closure expressions this body wrote, once each (N50).
    closures: Vec<&'b Expr>,
}

impl<'b> Walk<'_, 'b> {
    fn returned(&mut self, v: &Flow) {
        let _ = self.result.join(v);
    }

    fn assign(&mut self, name_span: Span, v: &Flow) {
        let Some(s) = self.res.var(name_span).map(|s| s as usize) else {
            return;
        };
        if let Some(slot) = self.slots.get_mut(s) {
            if slot.join(v) && self.read.get(s) == Some(&true) {
                self.stale = true;
            }
        }
    }

    fn block(&mut self, b: &'b Block) -> Flow {
        for s in &b.statements {
            self.stmt(s);
        }
        b.tail.as_ref().map(|e| self.expr(e)).unwrap_or_default()
    }

    fn stmt(&mut self, s: &'b Stmt) {
        match s {
            Stmt::Let(l) => {
                let v = self.expr(&l.value);
                self.assign(l.name_span, &v);
            }
            Stmt::Assign(a) => {
                let v = self.expr(&a.value);
                self.assign(a.place.name_span, &v);
            }
            Stmt::While(w) => {
                self.expr(&w.cond);
                self.block(&w.body);
            }
            Stmt::Return(r) => {
                let v = self.expr(&r.value);
                self.returned(&v);
            }
            Stmt::Expr(e) => {
                self.expr(e);
            }
            Stmt::Scope(sc) => {
                self.block(&sc.body);
            }
            Stmt::Spawn(sp) => {
                let args: Vec<Flow> = sp.args.iter().map(|a| self.expr(a)).collect();
                if let Some(Callee::Function(g)) = self.res.callee(sp.callee_span) {
                    self.call(g, sp.callee_span, args, true);
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    fn call(&mut self, callee: FnId, at: Span, args: Vec<Flow>, spawned: bool) -> Flow {
        let key = (at.start, at.end);
        let index = if let Some(&i) = self.call_at.get(&key) {
            if let Some(c) = self.calls.get_mut(i) {
                for (into, from) in c.args.iter_mut().zip(&args) {
                    let _ = into.join(from);
                }
            }
            i
        } else {
            let i = self.calls.len();
            self.calls.push(CallFact {
                callee,
                at,
                args,
                spawned,
            });
            self.call_at.insert(key, i);
            i
        };
        Flow::call(u32::try_from(index).unwrap_or(u32::MAX))
    }

    fn sink(&mut self, at: Span, path: &Flow) {
        let key = (at.start, at.end);
        if let Some(s) = self.sink_at.get(&key).and_then(|&i| self.sinks.get_mut(i)) {
            let _ = s.path.join(path);
        } else {
            self.sink_at.insert(key, self.sinks.len());
            self.sinks.push(SinkFact {
                at,
                path: path.clone(),
            });
        }
    }

    fn expr_var(&mut self, span: Span) -> Flow {
        let Some(s) = self.res.var(span).map(|s| s as usize) else {
            return Flow::default();
        };
        if let Some(r) = self.read.get_mut(s) {
            *r = true;
        }
        self.slots.get(s).cloned().unwrap_or_default()
    }

    fn joined(&mut self, es: impl IntoIterator<Item = Flow>) -> Flow {
        es.into_iter().fold(Flow::default(), |a, b| a.joined(&b))
    }

    fn expr(&mut self, e: &'b Expr) -> Flow {
        match e {
            // A closure is walked as a function of its own; making one moves no data.
            Expr::Closure { span, .. } => {
                if !self
                    .closures
                    .iter()
                    .any(|c| matches!(c, Expr::Closure { span: s, .. } if s == span))
                {
                    self.closures.push(e);
                }
                Flow::default()
            }
            Expr::Int { .. } | Expr::Bool { .. } | Expr::Str { .. } => Flow::default(),
            Expr::Var { span, .. } => self.expr_var(*span),
            Expr::Neg { operand, .. } => self.expr(operand),
            // The logical operators are the `if`s they lower to (N49): the deciding operand is a
            // condition — walked, contributing no provenance — and the value is the other
            // operand's or a constant's.
            Expr::Not { operand, .. } => {
                self.expr(operand);
                Flow::default()
            }
            Expr::Binary {
                op: BinOp::And | BinOp::Or,
                lhs,
                rhs,
                ..
            } => {
                self.expr(lhs);
                self.expr(rhs)
            }
            Expr::Binary { lhs, rhs, .. } => {
                let l = self.expr(lhs);
                let r = self.expr(rhs);
                l.joined(&r)
            }
            Expr::Call {
                callee_span, args, ..
            } => {
                let flows: Vec<Flow> = args.iter().map(|a| self.expr(a)).collect();
                match self.res.callee(*callee_span) {
                    Some(Callee::Builtin(b)) => {
                        let mut all = Flow::default();
                        for f in &flows {
                            let _ = all.join(f);
                        }
                        match Origin::of_intrinsic(b) {
                            BuiltinFlow::Source(o) => Flow::origin(o, *callee_span).joined(&all),
                            BuiltinFlow::Derive => all,
                            BuiltinFlow::Read => {
                                Flow::origin(Origin::Unknown, *callee_span).joined(&all)
                            }
                            BuiltinFlow::Fresh => Flow::default(),
                            BuiltinFlow::WritePath => {
                                if let Some(path) = flows.first() {
                                    self.sink(*callee_span, path);
                                }
                                all
                            }
                        }
                    }
                    Some(Callee::Function(g)) => self.call(g, *callee_span, flows, false),
                    // Through a function value (N50): which function is not known here, so
                    // its result is unknown, and carries its arguments' and the value's own.
                    Some(Callee::Indirect(_)) => {
                        let callee = self.expr_var(*callee_span);
                        Flow::origin(Origin::Unknown, *callee_span)
                            .joined(&callee)
                            .joined(&self.joined(flows))
                    }
                    // A call that did not resolve is already an error.
                    None => self.joined(flows),
                }
            }
            Expr::Construct { fields, .. } | Expr::Variant { fields, .. } => {
                let flows: Vec<Flow> = fields.iter().map(|f| self.expr(&f.value)).collect();
                self.joined(flows)
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let s = self.expr(scrutinee);
                let mut out = Flow::default();
                for arm in arms {
                    for p in &arm.fields {
                        self.assign(p.binding_span, &s);
                    }
                    let v = self.expr(&arm.body);
                    let _ = out.join(&v);
                }
                out
            }
            Expr::Field { base, .. } => self.expr(base),
            Expr::Propagate { operand, .. } => self.expr(operand),
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                // The condition is walked for its calls and sinks, and contributes no
                // provenance to the value: implicit flow is not tracked.
                self.expr(cond);
                let t = self.block(then_block);
                let e = else_block
                    .as_ref()
                    .map(|b| self.block(b))
                    .unwrap_or_default();
                t.joined(&e)
            }
            Expr::Block(b) => self.block(b),
        }
    }
}

// ---------------------------------------------------------------------------------------
// The whole program.
// ---------------------------------------------------------------------------------------

/// A value, solved: origins, and the enclosing function's parameters it carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Val {
    origins: OriginSet,
    params: BTreeSet<u32>,
}

impl Val {
    fn join(&mut self, other: &Self) {
        self.origins = self.origins.union(other.origins);
        self.params.extend(other.params.iter().copied());
    }
}

struct Program<'a> {
    facts: Vec<&'a FunctionFacts>,
    /// By `FnId` index: which entry of `facts`, if any.
    at: Vec<Option<usize>>,
    summaries: Vec<Summary>,
}

impl Program<'_> {
    fn summary(&self, g: FnId) -> Option<&Summary> {
        self.summaries.get(g.index())
    }

    fn eval(flow: &Flow, calls: &[Val]) -> Val {
        let mut v = Val {
            origins: flow.origins,
            params: flow.params.iter().copied().collect(),
        };
        for &c in &flow.calls {
            if let Some(cv) = calls.get(c as usize) {
                v.join(cv);
            }
        }
        v
    }

    /// Every call's result in `f`, against the current summaries. A call's arguments may
    /// carry other calls' results — and, through a loop, its own — so this is a small fixed
    /// point of its own.
    fn calls_of(&self, f: &FunctionFacts) -> Vec<Val> {
        let mut vals = vec![Val::default(); f.calls.len()];
        for _ in 0..=f.calls.len() {
            let mut changed = false;
            for (i, c) in f.calls.iter().enumerate() {
                let mut v = Val::default();
                if let Some(s) = self.summary(c.callee) {
                    v.origins = s.origins;
                    for &p in &s.params {
                        if let Some(a) = c.args.get(p as usize) {
                            v.join(&Self::eval(a, &vals));
                        }
                    }
                }
                if let Some(slot) = vals.get_mut(i) {
                    if *slot != v {
                        *slot = v;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        vals
    }

    fn facts_of(&self, g: FnId) -> Option<&FunctionFacts> {
        self.at
            .get(g.index())
            .copied()
            .flatten()
            .and_then(|i| self.facts.get(i).copied())
    }
}

/// Settle every function's provenance summary from every module's facts, record it, and
/// check the restricted flow.
///
/// `facts` has one entry per module; `None` for a module whose facts are not available, whose
/// functions are then summarised as the worst case — every origin [`Origin::Unknown`], every
/// parameter passed on and reaching a sink — rather than as local.
pub(crate) fn solve(
    facts: &[(nazm_sema::module::ModuleId, Option<ModuleFacts>)],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let n = res.functions().len();
    let mut program = Program {
        facts: Vec::new(),
        at: vec![None; n],
        summaries: vec![Summary::default(); n],
    };
    for (module, f) in facts {
        match f {
            Some(mf) => {
                for ff in &mf.functions {
                    if let Some(slot) = program.at.get_mut(ff.function.index()) {
                        *slot = Some(program.facts.len());
                    }
                    program.facts.push(ff);
                }
            }
            None => {
                for (i, def) in res.functions().iter().enumerate() {
                    if def.module != *module {
                        continue;
                    }
                    let all: Vec<u32> = (0..def.params.len())
                        .map(|p| u32::try_from(p).unwrap_or(u32::MAX))
                        .collect();
                    if let Some(s) = program.summaries.get_mut(i) {
                        *s = Summary {
                            origins: OriginSet::of(Origin::Unknown),
                            params: all.clone(),
                            reaches: all,
                            receives: vec![OriginSet::of(Origin::Unknown); def.params.len()],
                        };
                    }
                }
            }
        }
    }

    // A foreign function (N42) has no body to reduce: its result is C's, which may be anything
    // — `Unknown`, the worst origin — and may be derived from any argument, so each argument's
    // origins pass through to it. What C does with an argument is not a Nazm sink: it is
    // authorised by the `ForeignCap` the call needed, and outside what a restriction can see.
    for (i, def) in res.functions().iter().enumerate() {
        if def.foreign.is_none() {
            continue;
        }
        if let Some(s) = program.summaries.get_mut(i) {
            *s = Summary {
                origins: OriginSet::of(Origin::Unknown),
                params: (0..def.params.len())
                    .map(|p| u32::try_from(p).unwrap_or(u32::MAX))
                    .collect(),
                reaches: Vec::new(),
                receives: vec![OriginSet::default(); def.params.len()],
            };
        }
    }

    // Who calls whom, by index into `facts`: a changed summary re-queues its callers only.
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); program.facts.len()];
    for (k, f) in program.facts.iter().enumerate() {
        for c in &f.calls {
            if let Some(Some(j)) = program.at.get(c.callee.index()) {
                if let Some(list) = callers.get_mut(*j) {
                    if !list.contains(&k) {
                        list.push(k);
                    }
                }
            }
        }
    }

    // Origins and pass-through parameters: a worklist to the least fixed point. Summaries
    // only grow, over a finite lattice, so it empties whatever the call graph; which order
    // functions are taken in changes how soon, never what is reached.
    let mut queue: std::collections::VecDeque<usize> = (0..program.facts.len()).rev().collect();
    let mut queued = vec![true; program.facts.len()];
    while let Some(k) = queue.pop_front() {
        if let Some(q) = queued.get_mut(k) {
            *q = false;
        }
        let Some(f) = program.facts.get(k).copied() else {
            continue;
        };
        let calls = program.calls_of(f);
        let r = Program::eval(&f.result, &calls);
        let params: Vec<u32> = r.params.iter().copied().collect();
        let Some(s) = program.summaries.get_mut(f.function.index()) else {
            continue;
        };
        if s.origins != r.origins || s.params != params {
            s.origins = r.origins;
            s.params = params;
            for &c in callers.get(k).map(Vec::as_slice).unwrap_or_default() {
                if queued.get(c) == Some(&false) {
                    queue.push_back(c);
                    if let Some(q) = queued.get_mut(c) {
                        *q = true;
                    }
                }
            }
        }
    }

    // Which parameters reach a restricted sink: a second worklist, over the settled values,
    // since reaching a sink changes no value.
    let values: Vec<Vec<Val>> = program.facts.iter().map(|f| program.calls_of(f)).collect();
    let mut queue: std::collections::VecDeque<usize> = (0..program.facts.len()).rev().collect();
    let mut queued = vec![true; program.facts.len()];
    while let Some(k) = queue.pop_front() {
        if let Some(q) = queued.get_mut(k) {
            *q = false;
        }
        let (Some(f), Some(calls)) = (program.facts.get(k).copied(), values.get(k)) else {
            continue;
        };
        let mut reach: BTreeSet<u32> = program
            .summary(f.function)
            .map(|s| s.reaches.iter().copied().collect())
            .unwrap_or_default();
        let before = reach.len();
        for s in &f.sinks {
            reach.extend(Program::eval(&s.path, calls).params);
        }
        for c in &f.calls {
            let theirs = program
                .summary(c.callee)
                .map(|s| s.reaches.clone())
                .unwrap_or_default();
            for i in theirs {
                if let Some(a) = c.args.get(i as usize) {
                    reach.extend(Program::eval(a, calls).params);
                }
            }
        }
        if reach.len() != before {
            if let Some(s) = program.summaries.get_mut(f.function.index()) {
                s.reaches = reach.into_iter().collect();
            }
            for &c in callers.get(k).map(Vec::as_slice).unwrap_or_default() {
                if queued.get(c) == Some(&false) {
                    queue.push_back(c);
                    if let Some(q) = queued.get_mut(c) {
                        *q = true;
                    }
                }
            }
        }
    }

    // What each parameter is handed, over every call and `spawn` in the program: the origins
    // an argument carries, and — for a parameter of the caller it passes on — whatever that
    // parameter is handed in turn. A third fixed point, over the settled values.
    let mut receives: Vec<Vec<OriginSet>> = program
        .facts
        .iter()
        .map(|f| vec![OriginSet::EMPTY; res.function(f.function).map_or(0, |d| d.params.len())])
        .collect();
    loop {
        let mut changed = false;
        for (k, f) in program.facts.iter().enumerate() {
            let Some(calls) = values.get(k) else { continue };
            for c in &f.calls {
                let Some(Some(j)) = program.at.get(c.callee.index()).copied() else {
                    continue;
                };
                for (i, a) in c.args.iter().enumerate() {
                    let v = Program::eval(a, calls);
                    let mut o = v.origins;
                    for p in &v.params {
                        if let Some(r) = receives.get(k).and_then(|r| r.get(*p as usize)) {
                            o = o.union(*r);
                        }
                    }
                    if let Some(slot) = receives.get_mut(j).and_then(|r| r.get_mut(i)) {
                        let grown = slot.union(o);
                        if grown != *slot {
                            *slot = grown;
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
    for (k, f) in program.facts.iter().enumerate() {
        if let (Some(s), Some(r)) = (
            program.summaries.get_mut(f.function.index()),
            receives.get_mut(k),
        ) {
            s.receives = std::mem::take(r);
        }
    }

    // The restriction, in functions that declared an effect set: the same functions N37
    // holds to their authority. One diagnostic per site, in module and body order.
    for (k, f) in program.facts.iter().enumerate() {
        let explicit = res
            .function(f.function)
            .is_some_and(|d| d.declared_effects.is_some());
        if !explicit {
            continue;
        }
        let Some(calls) = values.get(k) else { continue };
        let mut found: Vec<(Span, Diagnostic)> = Vec::new();
        for s in &f.sinks {
            let v = Program::eval(&s.path, calls);
            if let Some(o) = v.origins.intersection(OriginSet::RESTRICTED).iter().next() {
                found.push((s.at, direct(&program, res, f, s, o)));
            }
        }
        for c in &f.calls {
            let theirs = program
                .summary(c.callee)
                .map(|s| s.reaches.clone())
                .unwrap_or_default();
            for i in theirs {
                let Some(a) = c.args.get(i as usize) else {
                    continue;
                };
                let v = Program::eval(a, calls);
                if let Some(o) = v.origins.intersection(OriginSet::RESTRICTED).iter().next() {
                    found.push((c.at, through(&program, res, f, c, i, a, o)));
                    break;
                }
            }
        }
        found.sort_by_key(|(at, _)| (at.start, at.end));
        errors.extend(found.into_iter().map(|(_, d)| d));
    }

    for (i, s) in program.summaries.iter().enumerate() {
        let id = FnId::from_index(i);
        if program.at.get(i).copied().flatten().is_some() {
            res.record_provenance(id, s.clone());
        }
    }
}

const HELP: &str = "in a function that declares its effects, the path a program writes to may \
    come from the command line or be computed in the program, but not from the contents of a \
    file, nor from a sequence or channel the checker does not follow, nor from what a foreign \
    (C) function returns. v1 has no way to vouch for such a value";

fn name(res: &Resolution, g: FnId) -> String {
    res.function(g)
        .map_or_else(|| "a function".to_owned(), |d| format!("`{}`", d.name))
}

fn direct(
    p: &Program<'_>,
    res: &Resolution,
    f: &FunctionFacts,
    s: &SinkFact,
    o: Origin,
) -> Diagnostic {
    let mut d = Diagnostic::error(
        codes::RESTRICTED_FLOW,
        s.at,
        format!(
            "the path `write_file` writes to comes from {}",
            o.describe()
        ),
    );
    d.primary.message = format!("this path comes from {}", o.describe());
    let mut labels = Vec::new();
    witness(p, res, f, &s.path, o, &mut labels, 0);
    for (at, text) in labels {
        d = d.secondary(at, text);
    }
    d.help(HELP)
}

fn through(
    p: &Program<'_>,
    res: &Resolution,
    f: &FunctionFacts,
    c: &CallFact,
    i: u32,
    arg: &Flow,
    o: Origin,
) -> Diagnostic {
    let callee = name(res, c.callee);
    let param = res
        .function(c.callee)
        .and_then(|d| d.locals.get(i as usize))
        .map_or_else(|| format!("#{i}"), |l| format!("`{}`", l.name));
    let mut d = Diagnostic::error(
        codes::RESTRICTED_FLOW,
        c.at,
        format!(
            "{callee} writes to a path made from its parameter {param}, and this argument \
             comes from {}",
            o.describe()
        ),
    );
    d.primary.message = format!("{callee}'s parameter {param} reaches `write_file`'s path");
    let mut labels = Vec::new();
    reaching(p, res, c.callee, i, &mut labels, 0);
    witness(p, res, f, arg, o, &mut labels, 0);
    for (at, text) in labels {
        d = d.secondary(at, text);
    }
    d.help(HELP)
}

/// Where `o` entered `flow`, in `f`: one chain, deterministic, through calls to their source.
fn witness(
    p: &Program<'_>,
    res: &Resolution,
    f: &FunctionFacts,
    flow: &Flow,
    o: Origin,
    labels: &mut Vec<(Span, String)>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    if let Some((_, at)) = flow.why.iter().find(|(x, _)| *x == o) {
        labels.push((*at, o.enters().to_owned()));
        return;
    }
    let calls = p.calls_of(f);
    for &ci in &flow.calls {
        let Some(c) = f.calls.get(ci as usize) else {
            continue;
        };
        let Some(s) = p.summary(c.callee) else {
            continue;
        };
        if s.origins.contains(o) {
            let said = if res.function(c.callee).is_some_and(|d| d.foreign.is_some()) {
                format!(
                    "{} is a foreign function, and what C returns is not followed",
                    name(res, c.callee)
                )
            } else {
                format!("{} returns data from {}", name(res, c.callee), o.describe())
            };
            labels.push((c.at, said));
            if let Some(g) = p.facts_of(c.callee) {
                witness(p, res, g, &g.result, o, labels, depth + 1);
            }
            return;
        }
        for &param in &s.params {
            let Some(a) = c.args.get(param as usize) else {
                continue;
            };
            if Program::eval(a, &calls).origins.contains(o) {
                labels.push((
                    c.at,
                    format!("{} passes on what it is given", name(res, c.callee)),
                ));
                witness(p, res, f, a, o, labels, depth + 1);
                return;
            }
        }
    }
}

/// How parameter `i` of `g` reaches a restricted sink: one chain to the `write_file`.
fn reaching(
    p: &Program<'_>,
    res: &Resolution,
    g: FnId,
    i: u32,
    labels: &mut Vec<(Span, String)>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    let Some(f) = p.facts_of(g) else { return };
    let calls = p.calls_of(f);
    for s in &f.sinks {
        if Program::eval(&s.path, &calls).params.contains(&i) {
            labels.push((
                s.at,
                format!("in {}: it is the path written here", name(res, g)),
            ));
            return;
        }
    }
    for c in &f.calls {
        let theirs = p
            .summary(c.callee)
            .map(|s| s.reaches.clone())
            .unwrap_or_default();
        for j in theirs {
            let Some(a) = c.args.get(j as usize) else {
                continue;
            };
            if Program::eval(a, &calls).params.contains(&i) {
                labels.push((
                    c.at,
                    format!(
                        "in {}: it is passed to {}",
                        name(res, g),
                        name(res, c.callee)
                    ),
                ));
                reaching(p, res, c.callee, j, labels, depth + 1);
                return;
            }
        }
    }
}
