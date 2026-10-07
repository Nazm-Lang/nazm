//! Provenance, N38 and N80: each module's bodies reduced to flows, the whole program's
//! summaries solved from them, and the flow policy checked against them.
//!
//! # Two halves, and why
//!
//! [`facts`] reads one module's checked bodies — through resolution, never through names —
//! and writes each function as flows over its own parameters, the calls it makes, the
//! containers it reads and writes, and the function values it makes and calls. That is a
//! function of the module's source and its imports' declarations, exactly what its cache key
//! already covers, so a reused module's facts can come from its cache entry. [`solve`] then
//! settles every function's [`Summary`] over the whole program from all modules' facts, every
//! run, and checks the policy. Nothing it concludes is cached, so no conclusion can be stale:
//! a callee's changed body changes what its caller is told in the same run.
//!
//! # The flow rules
//!
//! Explicit data flow only. A value carries what it was computed from: a binding the join of
//! everything ever assigned to it (flow-insensitively, to a fixed point, so a loop is no
//! special case), an operator its operands', a call its callee's summary applied to its
//! arguments, `?` its operand's payload, an `if` or `match` its arms'. A condition contributes
//! nothing: implicit flow is not tracked.
//!
//! Within a body a record or variant keeps each field apart where the body built it (N80,
//! §7.81): a field read, a pattern's binding and `?` read that field's flow. At a call, a
//! result or a container, a value is whole again.
//!
//! A container is read and written through its **cell**, named by its type (N80): what a read
//! of a `Strs` carries is everything any `strs_push` or `strs_set` in the program wrote into
//! any `Strs`. Sound without alias analysis, because every write is a built-in and a handle of
//! one type only holds what was written into handles of that type; imprecise exactly where two
//! containers share a type. A call through a function value reaches every function value the
//! program makes of a matching type, and a call through a bound every impl of that method; a
//! closure's parameters are received from those calls, and its captures from its maker.
//!
//! # The fixed point
//!
//! Summaries start empty and grow: a worklist re-reads a function's flows whenever a callee's
//! summary grows, until nothing is queued. Cells and what each parameter is handed are solved
//! around it, again until nothing grows. Everything is a finite set of origins and positions,
//! so the rounds end whatever the call graph — recursion, mutual recursion, import cycles and
//! cycles through containers and function values included — and the result is the least
//! fixed point.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use nazm_diag::{codes, Diagnostic};
use nazm_sema::resolve::{Callee, FnId, Resolution};
use nazm_sema::{
    BuiltinFlow, CallFact, ContainerUse, Flow, FlowRule, FunctionFacts, IndirectFact, MakeFact,
    ModuleFacts, Origin, OriginSet, SinkClass, SinkFact, SinkReport, Step, StoreFact, Summary,
    Type, METHOD_TARGET, VALUE_TARGET, WILD_CELL,
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
        let Some(id) = crate::check::defined(res, module, f) else {
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
                (false, _) => Flow::param(position(i)),
            };
        }
        let params = def.params.len();
        let (facts, slots, closures) = walk(res, id, slots, params, body);
        // Each closure the body wrote is a function of its own (N50), walked after the body
        // that made it: its parameters are received from the calls that can reach it, and its
        // captures from its maker, at the point it is made (N80).
        let mut pending: Vec<&Expr> = Vec::new();
        out.functions
            .push(with_makes(res, facts, &slots, &closures, &mut pending));
        let mut guard = 0usize;
        while let Some(c) = pending.pop() {
            guard += 1;
            if guard > 4096 {
                break;
            }
            let Expr::Closure { body, span, .. } = c else {
                continue;
            };
            let Some(cid) = res.closure_at(*span) else {
                continue;
            };
            let Some(cdef) = res.function(cid) else {
                continue;
            };
            let n = cdef.params.len();
            let mut slots = vec![Flow::default(); cdef.locals.len()];
            for (i, slot) in slots.iter_mut().enumerate().take(n) {
                *slot = Flow::param(position(i));
            }
            if let Some(info) = &cdef.closure {
                for (j, cap) in info.captures.iter().enumerate() {
                    if let Some(slot) = slots.get_mut(cap.inner as usize) {
                        *slot = Flow::param(position(n + j));
                    }
                }
            }
            let (facts, slots, nested) = walk(res, cid, slots, n, body);
            out.functions
                .push(with_makes(res, facts, &slots, &nested, &mut pending));
        }
    }
    out
}

fn position(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// `facts`, with a [`MakeFact`] for every closure its body wrote, carrying what each capture
/// holds as the body finally left it; the closures are queued to be walked in turn.
fn with_makes<'b>(
    res: &Resolution,
    mut facts: FunctionFacts,
    slots: &[Flow],
    closures: &[&'b Expr],
    pending: &mut Vec<&'b Expr>,
) -> FunctionFacts {
    for c in closures {
        let Expr::Closure { span, .. } = c else {
            continue;
        };
        let Some(cid) = res.closure_at(*span) else {
            continue;
        };
        let Some(cdef) = res.function(cid) else {
            continue;
        };
        let captures = cdef
            .closure
            .as_ref()
            .map(|info| {
                info.captures
                    .iter()
                    .map(|cap| slots.get(cap.outer as usize).cloned().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        facts.makes.push(MakeFact {
            function: cid,
            target: value_target(res, &cdef.params, cdef.ret),
            at: *span,
            captures,
        });
        pending.push(c);
    }
    facts
}

/// The target a function value of this signature answers to: its type up to effects (N80).
fn value_target(res: &Resolution, params: &[Type], ret: Type) -> String {
    let types = res.types();
    if params.iter().any(|p| !types.is_concrete(*p)) || !types.is_concrete(ret) {
        return format!("{VALUE_TARGET}{WILD_CELL}");
    }
    let shown: Vec<String> = params.iter().map(|p| types.show(*p)).collect();
    format!(
        "{VALUE_TARGET}({}) -> {}",
        shown.join(", "),
        types.show(ret)
    )
}

/// The target of a function type, up to effects.
fn fn_type_target(res: &Resolution, t: Type) -> String {
    match t {
        Type::Fn(f) => match res.types().fn_signature(f) {
            Some(sig) => {
                let (params, ret) = (sig.params.clone(), sig.ret);
                value_target(res, &params, ret)
            }
            None => format!("{VALUE_TARGET}{WILD_CELL}"),
        },
        _ => format!("{VALUE_TARGET}{WILD_CELL}"),
    }
}

/// A value's flow, with each field kept apart where the body built it (N80): keyed by
/// variant (empty for a record) and field name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Shaped {
    flow: Flow,
    parts: Option<Parts>,
}

type Parts = BTreeMap<(String, String), Shaped>;

/// How deep a shape is kept: below this a field is whole, so a binding assigned a value built
/// from itself cannot grow without end.
const SHAPE_DEPTH: usize = 4;

impl Shaped {
    fn whole(flow: Flow) -> Self {
        Self { flow, parts: None }
    }

    /// A value that carries nothing and has no shape: a literal, or what a diverging branch
    /// leaves. It changes nothing it is joined with.
    fn neutral(&self) -> bool {
        self.parts.is_none() && self.flow.is_local()
    }

    /// Join `other` into this one, as an `if` or `match` joins its arms.
    fn join(&mut self, other: &Self) {
        if other.neutral() {
            return;
        }
        if self.neutral() {
            *self = other.clone();
            return;
        }
        let _ = self.flow.join(&other.flow);
        self.parts = match (self.parts.take(), &other.parts) {
            (Some(mut a), Some(b)) => {
                merge_parts(&mut a, b);
                Some(a)
            }
            _ => None,
        };
    }

    fn truncated(mut self, depth: usize) -> Self {
        if depth >= SHAPE_DEPTH {
            self.parts = None;
        } else if let Some(parts) = self.parts.take() {
            self.parts = Some(
                parts
                    .into_iter()
                    .map(|(k, v)| (k, v.truncated(depth + 1)))
                    .collect(),
            );
        }
        self
    }
}

/// Merge `from` into `into`, and say whether anything grew.
fn merge_parts(into: &mut Parts, from: &Parts) -> bool {
    let mut grew = false;
    for (k, v) in from {
        match into.get_mut(k) {
            Some(mine) => {
                let before = mine.clone();
                mine.join(v);
                grew |= *mine != before;
            }
            None => {
                into.insert(k.clone(), v.clone());
                grew = true;
            }
        }
    }
    grew
}

/// What a binding's fields are known to be (N80).
#[derive(Debug, Clone, PartialEq, Eq)]
enum SlotShape {
    /// Nothing with a shape has been assigned yet.
    Unset,
    /// Every value assigned so far was built here, field by field.
    Parts(Parts),
    /// Something without a shape was assigned: whole from then on.
    Whole,
}

/// One body's facts, its bindings' final flows, and the closures it wrote.
fn walk<'b>(
    res: &Resolution,
    id: FnId,
    slots: Vec<Flow>,
    params: usize,
    body: &'b Block,
) -> (FunctionFacts, Vec<Flow>, Vec<&'b Expr>) {
    let n = slots.len();
    let mut shapes = vec![SlotShape::Unset; n];
    // A parameter, or a capture, is whatever it is handed: whole.
    for (i, s) in shapes.iter_mut().enumerate() {
        if i < params || slots.get(i).is_some_and(|f| !f.params.is_empty()) {
            *s = SlotShape::Whole;
        }
    }
    let elems = res
        .function(id)
        .map(|d| {
            d.call_sites
                .iter()
                .filter_map(|s| Some(((s.span.start, s.span.end), *s.type_args.first()?)))
                .collect()
        })
        .unwrap_or_default();
    let mut w = Walk {
        res,
        id,
        elems,
        read: vec![false; n],
        stale: false,
        slots,
        shapes,
        calls: Vec::new(),
        call_at: HashMap::new(),
        sinks: Vec::new(),
        sink_at: HashMap::new(),
        stores: Vec::new(),
        store_at: HashMap::new(),
        indirect: Vec::new(),
        indirect_at: HashMap::new(),
        makes: Vec::new(),
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
        w.returned(&tail.flow);
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
            stores: w.stores,
            indirect: w.indirect,
            makes: w.makes,
        },
        w.slots,
        w.closures,
    )
}

struct Walk<'a, 'b> {
    res: &'a Resolution,
    id: FnId,
    /// Each call site's first type argument, by the callee's span: what a container's cell is
    /// named by. Indexed once, so a body with many container calls is not quadratic.
    elems: HashMap<(u32, u32), Type>,
    slots: Vec<Flow>,
    shapes: Vec<SlotShape>,
    /// Which bindings this walk has read.
    read: Vec<bool>,
    /// Whether a binding grew after this walk read it.
    stale: bool,
    calls: Vec<CallFact>,
    call_at: HashMap<(u32, u32), usize>,
    sinks: Vec<SinkFact>,
    sink_at: HashMap<(u32, u32, SinkClass), usize>,
    stores: Vec<StoreFact>,
    store_at: HashMap<(u32, u32, String), usize>,
    indirect: Vec<IndirectFact>,
    indirect_at: HashMap<(u32, u32), usize>,
    makes: Vec<MakeFact>,
    result: Flow,
    /// The closure expressions this body wrote, once each (N50).
    closures: Vec<&'b Expr>,
}

impl<'b> Walk<'_, 'b> {
    fn returned(&mut self, v: &Flow) {
        let _ = self.result.join(v);
    }

    fn assign(&mut self, name_span: Span, v: &Shaped) {
        let Some(s) = self.res.var(name_span).map(|s| s as usize) else {
            return;
        };
        let mut grew = false;
        if let Some(slot) = self.slots.get_mut(s) {
            grew |= slot.join(&v.flow);
        }
        if let Some(shape) = self.shapes.get_mut(s) {
            let next = match (&*shape, &v.parts) {
                (SlotShape::Whole, _) => None,
                (_, None) if v.neutral() => None,
                (_, None) => Some(SlotShape::Whole),
                (SlotShape::Unset, Some(p)) => Some(SlotShape::Parts(p.clone())),
                (SlotShape::Parts(mine), Some(p)) => {
                    let mut merged = mine.clone();
                    merge_parts(&mut merged, p).then_some(SlotShape::Parts(merged))
                }
            };
            if let Some(next) = next {
                if *shape != next {
                    *shape = next;
                    grew = true;
                }
            }
        }
        if grew && self.read.get(s) == Some(&true) {
            self.stale = true;
        }
    }

    /// A write to a field of a binding, `a.b = v`: the binding is whole from then on.
    fn assign_field(&mut self, name_span: Span, v: &Flow) {
        let Some(s) = self.res.var(name_span).map(|s| s as usize) else {
            return;
        };
        let mut grew = false;
        if let Some(slot) = self.slots.get_mut(s) {
            grew |= slot.join(v);
        }
        if let Some(shape) = self.shapes.get_mut(s) {
            if *shape != SlotShape::Whole {
                *shape = SlotShape::Whole;
                grew = true;
            }
        }
        if grew && self.read.get(s) == Some(&true) {
            self.stale = true;
        }
    }

    fn block(&mut self, b: &'b Block) -> Shaped {
        for s in &b.statements {
            self.stmt(s);
        }
        b.tail.as_ref().map(|e| self.shaped(e)).unwrap_or_default()
    }

    fn stmt(&mut self, s: &'b Stmt) {
        match s {
            Stmt::Let(l) => {
                let v = self.shaped(&l.value);
                self.assign(l.name_span, &v);
            }
            Stmt::Assign(a) => {
                let v = self.shaped(&a.value);
                if a.place.path.is_empty() {
                    self.assign(a.place.name_span, &v);
                } else {
                    self.assign_field(a.place.name_span, &v.flow);
                }
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
        Flow::call(position(index))
    }

    /// A call whose callee is not named: through a function value or a bound (N80).
    fn indirect(&mut self, target: String, at: Span, args: Vec<Flow>) -> Flow {
        let key = (at.start, at.end);
        let index = if let Some(&i) = self.indirect_at.get(&key) {
            if let Some(c) = self.indirect.get_mut(i) {
                for (into, from) in c.args.iter_mut().zip(&args) {
                    let _ = into.join(from);
                }
            }
            i
        } else {
            let i = self.indirect.len();
            self.indirect.push(IndirectFact { target, at, args });
            self.indirect_at.insert(key, i);
            i
        };
        Flow::indirect(position(index))
    }

    fn sink(&mut self, at: Span, class: SinkClass, path: &Flow) {
        let key = (at.start, at.end, class);
        if let Some(s) = self.sink_at.get(&key).and_then(|&i| self.sinks.get_mut(i)) {
            let _ = s.path.join(path);
        } else {
            self.sink_at.insert(key, self.sinks.len());
            self.sinks.push(SinkFact {
                at,
                path: path.clone(),
                class,
            });
        }
    }

    /// A value written into a container's cell (N80).
    fn store(&mut self, cell: String, at: Span, value: &Flow) {
        let key = (at.start, at.end, cell.clone());
        if let Some(s) = self
            .store_at
            .get(&key)
            .and_then(|&i| self.stores.get_mut(i))
        {
            let _ = s.value.join(value);
        } else {
            self.store_at.insert(key, self.stores.len());
            self.stores.push(StoreFact {
                cell,
                at,
                value: value.clone(),
            });
        }
    }

    /// The cell of a built-in's argument `arg`: its container's type (N80). A type the call
    /// site does not settle, or one that mentions a type parameter, is the wild cell.
    fn cell(&self, b: nazm_sema::Intrinsic, at: Span, arg: usize) -> String {
        use nazm_sema::intrinsic::Shape;
        let elem = || -> Option<String> {
            let t = *self.elems.get(&(at.start, at.end))?;
            self.res
                .types()
                .is_concrete(t)
                .then(|| self.res.types().show(t))
        };
        let name = match b.params().get(arg) {
            Some(Shape::Ints) => Some("Ints".to_owned()),
            Some(Shape::Strs) => Some("Strs".to_owned()),
            Some(Shape::Chan) => Some("Chan".to_owned()),
            Some(Shape::VecT) => elem().map(|t| format!("Vec[{t}]")),
            // A select's channels: the cell is their elements', `Chan[T]`.
            Some(Shape::ChanT | Shape::ChansT) => elem().map(|t| format!("Chan[{t}]")),
            _ => None,
        };
        name.unwrap_or_else(|| WILD_CELL.to_owned())
    }

    fn expr_var(&mut self, span: Span) -> Shaped {
        let Some(s) = self.res.var(span).map(|s| s as usize) else {
            // A named function written as a value (N80): which indirect calls may run it.
            if let Some(v) = self.res.fn_value(span) {
                let target = fn_type_target(self.res, v.ty);
                if !self
                    .makes
                    .iter()
                    .any(|m| m.at == span && m.function == v.function)
                {
                    self.makes.push(MakeFact {
                        function: v.function,
                        target,
                        at: span,
                        captures: Vec::new(),
                    });
                }
            }
            return Shaped::default();
        };
        if let Some(r) = self.read.get_mut(s) {
            *r = true;
        }
        let flow = self.slots.get(s).cloned().unwrap_or_default();
        let parts = match self.shapes.get(s) {
            Some(SlotShape::Parts(p)) => Some(p.clone()),
            _ => None,
        };
        Shaped { flow, parts }
    }

    /// A method call's flow (N78), receiver first: on a concrete type the impl's function
    /// is known and is a call like any other; through a bound, every impl's method of that
    /// trait method may run (N80).
    fn method(&mut self, at: Span, flows: Vec<Flow>) -> Flow {
        match self.res.callee(at) {
            Some(Callee::Function(g)) => self.call(g, at, flows, false),
            Some(Callee::Indirect(_)) => {
                let target = self.res.traits().call(at).and_then(|c| {
                    let t = self.res.traits().get(c.trait_id)?;
                    let m = t.methods.get(c.method as usize)?;
                    Some(format!("{METHOD_TARGET}{}.{}", t.name, m.name))
                });
                match target {
                    Some(target) => self.indirect(target, at, flows),
                    // No bound recorded: which function runs is not known.
                    None => Flow::origin(Origin::Unknown, at).joined(&joined(flows)),
                }
            }
            _ => joined(flows),
        }
    }

    fn expr(&mut self, e: &'b Expr) -> Flow {
        self.shaped(e).flow
    }

    fn shaped(&mut self, e: &'b Expr) -> Shaped {
        match e {
            // A closure is walked as a function of its own; making one moves no data, and
            // what it captures is its maker's [`MakeFact`].
            Expr::Closure { span, .. } => {
                if !self
                    .closures
                    .iter()
                    .any(|c| matches!(c, Expr::Closure { span: s, .. } if s == span))
                {
                    self.closures.push(e);
                }
                Shaped::default()
            }
            Expr::Int { .. }
            | Expr::WideInt { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Str { .. } => Shaped::default(),
            Expr::Var { span, .. } => self.expr_var(*span),
            Expr::Neg { operand, .. } | Expr::BitNot { operand, .. } => {
                Shaped::whole(self.expr(operand))
            }
            // An array carries what its elements carry; an element carries its array's and its
            // index's (Gate 2).
            Expr::Array { elems, .. } => {
                let mut flow: Option<Flow> = None;
                for x in elems {
                    let f = self.expr(x);
                    flow = Some(match flow {
                        Some(acc) => acc.joined(&f),
                        None => f,
                    });
                }
                flow.map_or_else(Shaped::default, Shaped::whole)
            }
            Expr::ArrayRepeat { value, count, .. } => {
                self.expr(count);
                Shaped::whole(self.expr(value))
            }
            Expr::Index { base, index, .. } => {
                let b = self.expr(base);
                let i = self.expr(index);
                Shaped::whole(b.joined(&i))
            }
            // The logical operators are the `if`s they lower to (N49): the deciding operand is a
            // condition — walked, contributing no provenance — and the value is the other
            // operand's or a constant's.
            Expr::Not { operand, .. } => {
                self.expr(operand);
                Shaped::default()
            }
            Expr::Binary {
                op: BinOp::And | BinOp::Or,
                lhs,
                rhs,
                ..
            } => {
                self.expr(lhs);
                Shaped::whole(self.expr(rhs))
            }
            Expr::Binary { lhs, rhs, .. } => {
                let l = self.expr(lhs);
                let r = self.expr(rhs);
                Shaped::whole(l.joined(&r))
            }
            Expr::Call {
                callee_span, args, ..
            } => Shaped::whole(self.call_expr(*callee_span, args)),
            // `x.m()` the checker read as a method call (N78).
            Expr::Variant {
                ty_span,
                variant_span,
                fields,
                ..
            } if fields.is_empty() && self.res.callee(*variant_span).is_some() => {
                let flows = if self.res.var(*ty_span).is_some() {
                    vec![self.expr_var(*ty_span).flow]
                } else {
                    Vec::new()
                };
                Shaped::whole(self.method(*variant_span, flows))
            }
            Expr::MethodCall {
                receiver,
                method_span,
                args,
                ..
            } => {
                // `Trait.m(recv, …)` names no value before the `.`; its receiver is an
                // argument.
                let mut flows = Vec::new();
                let ufcs =
                    matches!(&**receiver, Expr::Var { span, .. } if self.res.var(*span).is_none());
                if !ufcs {
                    flows.push(self.expr(receiver));
                }
                flows.extend(args.iter().map(|a| self.expr(a)));
                Shaped::whole(self.method(*method_span, flows))
            }
            Expr::Construct { fields, .. } => self.built("", fields),
            Expr::Variant {
                variant, fields, ..
            } => self.built(variant, fields),
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let s = self.shaped(scrutinee);
                let mut out = Shaped::default();
                for arm in arms {
                    for p in &arm.fields {
                        let v = match &s.parts {
                            Some(parts) => parts
                                .get(&(arm.variant.clone(), p.name.clone()))
                                .cloned()
                                .unwrap_or_default(),
                            None => Shaped::whole(s.flow.clone()),
                        };
                        self.assign(p.binding_span, &v);
                    }
                    let v = self.shaped(&arm.body);
                    out.join(&v);
                }
                out
            }
            Expr::Field { base, name, .. } => {
                let b = self.shaped(base);
                match &b.parts {
                    Some(parts) => {
                        let mut out = Shaped::default();
                        for ((_, field), v) in parts {
                            if field == name {
                                out.join(v);
                            }
                        }
                        out
                    }
                    None => Shaped::whole(b.flow),
                }
            }
            // `?` yields the payload; the error leaves the function.
            Expr::Propagate { operand, .. } => {
                let o = self.shaped(operand);
                match &o.parts {
                    Some(parts) => {
                        let mut out = Shaped::default();
                        for ((variant, _), v) in parts {
                            if variant != "Err" && variant != "None" {
                                out.join(v);
                            }
                        }
                        out
                    }
                    None => Shaped::whole(o.flow),
                }
            }
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                // The condition is walked for its calls and sinks, and contributes no
                // provenance to the value: implicit flow is not tracked.
                self.expr(cond);
                let mut t = self.block(then_block);
                let e = else_block
                    .as_ref()
                    .map(|b| self.block(b))
                    .unwrap_or_default();
                t.join(&e);
                t
            }
            Expr::Block(b) => self.block(b),
        }
    }

    /// A record or variant built here, each field kept apart (N80).
    fn built(&mut self, variant: &str, fields: &'b [nazm_syntax::FieldInit]) -> Shaped {
        let mut flow = Flow::default();
        let mut parts = Parts::new();
        for f in fields {
            let v = self.shaped(&f.value).truncated(1);
            let _ = flow.join(&v.flow);
            parts.insert((variant.to_owned(), f.name.clone()), v);
        }
        Shaped {
            flow,
            parts: Some(parts),
        }
    }

    fn call_expr(&mut self, callee_span: Span, args: &'b [Expr]) -> Flow {
        let flows: Vec<Flow> = args.iter().map(|a| self.expr(a)).collect();
        match self.res.callee(callee_span) {
            Some(Callee::Builtin(b)) => {
                let all = joined(flows.iter().cloned());
                // Every argument that takes data out of the program, by class (N52).
                for &(arg, class) in SinkClass::of_intrinsic(b) {
                    if let Some(flow) = flows.get(arg) {
                        self.sink(callee_span, class, flow);
                    }
                }
                // What it reads out of, or writes into, a container's cell (N80).
                let contained = match Origin::container(b) {
                    ContainerUse::None => None,
                    ContainerUse::Read { handle } => {
                        let cell = self.cell(b, callee_span, handle);
                        Some(Flow::cell(&cell, callee_span).joined(&all))
                    }
                    ContainerUse::Write { handle, value } => {
                        let cell = self.cell(b, callee_span, handle);
                        let v = flows.get(value).cloned().unwrap_or_default();
                        self.store(cell, callee_span, &v);
                        None
                    }
                    ContainerUse::Receive { from, into } => {
                        let source = self.cell(b, callee_span, from);
                        let target = self.cell(b, callee_span, into);
                        let moved = Flow::cell(&source, callee_span);
                        self.store(target, callee_span, &moved);
                        Some(moved.joined(&all))
                    }
                    ContainerUse::Select { from, into } => {
                        let source = self.cell(b, callee_span, from);
                        let target = self.cell(b, callee_span, into);
                        self.store(target, callee_span, &Flow::cell(&source, callee_span));
                        // Which channel was ready: control, not data.
                        Some(Flow::default())
                    }
                };
                match Origin::of_intrinsic(b) {
                    BuiltinFlow::Source(o) => Flow::origin(o, callee_span).joined(&all),
                    BuiltinFlow::Derive | BuiltinFlow::WritePath => all,
                    BuiltinFlow::Read => contained
                        .unwrap_or_else(|| Flow::origin(Origin::Unknown, callee_span).joined(&all)),
                    BuiltinFlow::Fresh => Flow::default(),
                    // Cleared, and recorded above as a `vouch` (N52).
                    BuiltinFlow::Vouch => Flow::default(),
                }
            }
            Some(Callee::Function(g)) => self.call(g, callee_span, flows, false),
            // Through a function value (N80): every function value of a matching type may
            // run, and the value itself carries what it captured.
            Some(Callee::Indirect(_)) => {
                let target = self
                    .res
                    .var(callee_span)
                    .and_then(|s| {
                        self.res
                            .function(self.id)
                            .and_then(|d| d.locals.get(s as usize))
                    })
                    .map(|l| l.ty);
                let target = match target {
                    Some(t) => fn_type_target(self.res, t),
                    None => format!("{VALUE_TARGET}{WILD_CELL}"),
                };
                let callee = self.expr_var(callee_span).flow;
                self.indirect(target, callee_span, flows).joined(&callee)
            }
            // A call that did not resolve is already an error.
            None => joined(flows),
        }
    }
}

fn joined(es: impl IntoIterator<Item = Flow>) -> Flow {
    es.into_iter().fold(Flow::default(), |a, b| a.joined(&b))
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

/// One function's call results, as settled: direct calls, and indirect ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Vals {
    calls: Vec<Val>,
    via: Vec<Val>,
}

/// Which parameters reach a sink of each class (N80: every class, not only the path).
type Reach = BTreeMap<SinkClass, BTreeSet<u32>>;

struct Program<'a> {
    facts: Vec<&'a FunctionFacts>,
    /// By `FnId` index: which entry of `facts`, if any.
    at: Vec<Option<usize>>,
    summaries: Vec<Summary>,
    /// By `FnId` index: how many parameters a caller passes (a closure's captures follow).
    arity: Vec<usize>,
    /// By `FnId` index: which sinks each parameter reaches.
    reach: Vec<Reach>,
    /// By `FnId` index: what each parameter, then each capture, is handed.
    receives: Vec<Vec<OriginSet>>,
    /// Each cell's origins (N80).
    cells: BTreeMap<String, OriginSet>,
    /// Which functions each indirect target may run (N80).
    targets: BTreeMap<String, Vec<FnId>>,
    /// [`Program::candidates`] of every target a fact names, settled once: targets do not
    /// change while the program is solved.
    resolved: HashMap<String, Vec<FnId>>,
    /// Whether some module's facts are missing: then nothing indirect is followed (N80).
    incomplete: bool,
}

impl Program<'_> {
    fn summary(&self, g: FnId) -> Option<&Summary> {
        self.summaries.get(g.index())
    }

    fn facts_of(&self, g: FnId) -> Option<&FunctionFacts> {
        self.at
            .get(g.index())
            .copied()
            .flatten()
            .and_then(|i| self.facts.get(i).copied())
    }

    fn arity(&self, g: FnId) -> usize {
        self.arity.get(g.index()).copied().unwrap_or(0)
    }

    fn received(&self, g: FnId, p: u32) -> OriginSet {
        self.receives
            .get(g.index())
            .and_then(|r| r.get(p as usize))
            .copied()
            .unwrap_or_default()
    }

    /// A cell's contents: its own and the wild cell's, or every cell's for the wild one.
    fn cell(&self, name: &str) -> OriginSet {
        let mut o = if name == WILD_CELL {
            self.cells
                .values()
                .fold(OriginSet::EMPTY, |a, b| a.union(*b))
        } else {
            self.cells
                .get(name)
                .copied()
                .unwrap_or_default()
                .union(self.cells.get(WILD_CELL).copied().unwrap_or_default())
        };
        if self.incomplete {
            o = o.with(Origin::Unknown);
        }
        o
    }

    /// Which functions an indirect target may run (N80): those of the target, or every
    /// function value for the wild one.
    fn candidates(&self, target: &str) -> &[FnId] {
        self.resolved.get(target).map_or(&[], Vec::as_slice)
    }

    fn resolve_candidates(&self, target: &str) -> Vec<FnId> {
        let wild = format!("{VALUE_TARGET}{WILD_CELL}");
        if target == wild {
            let mut all: Vec<FnId> = self
                .targets
                .iter()
                .filter(|(k, _)| k.starts_with(VALUE_TARGET))
                .flat_map(|(_, v)| v.iter().copied())
                .collect();
            all.sort_unstable();
            all.dedup();
            return all;
        }
        let mut out: Vec<FnId> = self.targets.get(target).cloned().unwrap_or_default();
        if target.starts_with(VALUE_TARGET) {
            // A value made in a generic body matches anything.
            out.extend(self.targets.get(&wild).into_iter().flatten().copied());
            out.sort_unstable();
            out.dedup();
        }
        out
    }

    fn eval(&self, flow: &Flow, vals: &Vals) -> Val {
        let mut v = Val {
            origins: flow.origins,
            params: flow.params.iter().copied().collect(),
        };
        for &c in &flow.calls {
            if let Some(cv) = vals.calls.get(c as usize) {
                v.join(cv);
            }
        }
        for &c in &flow.via {
            if let Some(cv) = vals.via.get(c as usize) {
                v.join(cv);
            }
        }
        for (name, _) in &flow.cells {
            v.origins = v.origins.union(self.cell(name));
        }
        v
    }

    /// What `flow` can carry, in `f`: its value's origins and whatever its parameters are
    /// handed.
    fn reaching(&self, f: &FunctionFacts, flow: &Flow, vals: &Vals) -> OriginSet {
        let v = self.eval(flow, vals);
        v.params
            .iter()
            .fold(v.origins, |o, p| o.union(self.received(f.function, *p)))
    }

    /// What a call of `g` with `args` returns, against the current summaries; a parameter
    /// past the arguments is a capture, and carries what it was handed.
    fn applied(&self, g: FnId, args: &[Flow], vals: &Vals) -> Val {
        let mut v = Val::default();
        let Some(s) = self.summary(g) else { return v };
        v.origins = s.origins;
        let n = self.arity(g);
        for &p in &s.params {
            if (p as usize) < n {
                if let Some(a) = args.get(p as usize) {
                    v.join(&self.eval(a, vals));
                }
            } else {
                v.origins = v.origins.union(self.received(g, p));
            }
        }
        v
    }

    /// Every call's result in `f`, against the current summaries. A call's arguments may
    /// carry other calls' results — and, through a loop, its own — so this is a small fixed
    /// point of its own.
    fn values(&self, f: &FunctionFacts) -> Vals {
        let mut vals = Vals {
            calls: vec![Val::default(); f.calls.len()],
            via: vec![Val::default(); f.indirect.len()],
        };
        for _ in 0..=(f.calls.len() + f.indirect.len()) {
            let mut changed = false;
            for (i, c) in f.calls.iter().enumerate() {
                let v = self.applied(c.callee, &c.args, &vals);
                if let Some(slot) = vals.calls.get_mut(i) {
                    if *slot != v {
                        *slot = v;
                        changed = true;
                    }
                }
            }
            for (i, c) in f.indirect.iter().enumerate() {
                let mut v = Val::default();
                for &g in self.candidates(&c.target) {
                    v.join(&self.applied(g, &c.args, &vals));
                }
                if self.incomplete {
                    v.origins = v.origins.with(Origin::Unknown);
                }
                if let Some(slot) = vals.via.get_mut(i) {
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

    /// The parameters of `f` that reach a sink of `class`, through `g`'s parameter `p`
    /// handed `arg`.
    fn reach_of(&self, g: FnId, class: SinkClass) -> BTreeSet<u32> {
        self.reach
            .get(g.index())
            .and_then(|r| r.get(&class))
            .cloned()
            .unwrap_or_default()
    }
}

/// Settle every function's provenance summary from every module's facts, record it and each
/// sink with its traces, and check the language's flow policy.
///
/// `facts` has one entry per module; `None` for a module whose facts are not available, whose
/// functions are then summarised as the worst case — every origin [`Origin::Unknown`], every
/// parameter passed on and reaching a sink — rather than as local, and whose containers and
/// function values make every cell and every indirect call [`Origin::Unknown`] too (N80).
#[allow(clippy::too_many_lines)]
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
        arity: res.functions().iter().map(|d| d.params.len()).collect(),
        reach: vec![Reach::new(); n],
        receives: res
            .functions()
            .iter()
            .map(|d| {
                let captures = d.closure.as_ref().map_or(0, |c| c.captures.len());
                vec![OriginSet::EMPTY; d.params.len() + captures]
            })
            .collect(),
        cells: BTreeMap::new(),
        targets: BTreeMap::new(),
        resolved: HashMap::new(),
        incomplete: false,
    };
    let mut worst: Vec<FnId> = Vec::new();
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
                program.incomplete = true;
                for (i, def) in res.functions().iter().enumerate() {
                    if def.module == *module {
                        worst.push(FnId::from_index(i));
                    }
                }
            }
        }
    }
    // Which functions each target may run: every value the program makes, by its type up to
    // effects, and every impl's method by its trait method (N80).
    for f in &program.facts {
        for m in &f.makes {
            program
                .targets
                .entry(m.target.clone())
                .or_default()
                .push(m.function);
        }
    }
    for (_, imp) in res.traits().impls() {
        let Some(t) = res.traits().get(imp.trait_id) else {
            continue;
        };
        for (m, g) in t.methods.iter().zip(&imp.methods) {
            if let Some(g) = g {
                program
                    .targets
                    .entry(format!("{METHOD_TARGET}{}.{}", t.name, m.name))
                    .or_default()
                    .push(*g);
            }
        }
    }
    for v in program.targets.values_mut() {
        v.sort_unstable();
        v.dedup();
    }
    let named: BTreeSet<String> = program
        .facts
        .iter()
        .flat_map(|f| f.indirect.iter().map(|c| c.target.clone()))
        .collect();
    program.resolved = named
        .into_iter()
        .map(|t| {
            let c = program.resolve_candidates(&t);
            (t, c)
        })
        .collect();

    // Who depends on whom, by index into `facts`: a changed summary or reach re-queues the
    // functions that call it, directly or through a target, and the makers of a closure.
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); program.facts.len()];
    for (k, f) in program.facts.iter().enumerate() {
        let mut on: Vec<FnId> = f.calls.iter().map(|c| c.callee).collect();
        for c in &f.indirect {
            on.extend(program.candidates(&c.target).iter().copied());
        }
        on.extend(f.makes.iter().map(|m| m.function));
        for g in on {
            if let Some(Some(j)) = program.at.get(g.index()) {
                if let Some(list) = callers.get_mut(*j) {
                    if !list.contains(&k) {
                        list.push(k);
                    }
                }
            }
        }
    }

    // Around everything, the cells and what parameters and captures are handed: each round
    // solves summaries, values and reaches against them, then reads them again, until
    // nothing grows. Monotone over finite sets, so it ends.
    //
    // Cells and receives add origins and never parameters, so which parameters reach a sink
    // is settled by the first round, and a later round re-solves only the functions that read
    // a cell that grew, or call through a value whose captures did.
    let mentions: Vec<BTreeSet<&str>> = program.facts.iter().map(|f| cells_named(f)).collect();
    let calls_through: Vec<usize> = (0..program.facts.len())
        .filter(|&k| program.facts.get(k).is_some_and(|f| !f.indirect.is_empty()))
        .collect();
    let mut values: Vec<Vals>;
    let mut seed: Option<Vec<usize>> = None;
    loop {
        let rounds_cells = program.cells.clone();
        let rounds_receives = program.receives.clone();
        settle(&mut program, &worst, res, &callers, seed.as_deref());
        values = program.facts.iter().map(|f| program.values(f)).collect();
        if seed.is_none() {
            settle_reach(&mut program, &values, &callers);
        }
        settle_receives(&mut program, &values, res);
        // What each container holds: every value written into its cell, with what the
        // writer's parameters are handed.
        let mut cells: BTreeMap<String, OriginSet> = BTreeMap::new();
        for (k, f) in program.facts.iter().enumerate() {
            let Some(vals) = values.get(k) else { continue };
            for s in &f.stores {
                let o = program.reaching(f, &s.value, vals);
                let slot = cells.entry(s.cell.clone()).or_default();
                *slot = slot.union(o);
            }
        }
        for (name, o) in rounds_cells.iter() {
            let slot = cells.entry(name.clone()).or_default();
            *slot = slot.union(*o);
        }
        let grown: BTreeSet<String> = cells
            .iter()
            .filter(|(k, v)| rounds_cells.get(*k) != Some(*v))
            .map(|(k, _)| k.clone())
            .collect();
        let captures_grew = program.receives != rounds_receives;
        program.cells = cells;
        if grown.is_empty() && !captures_grew {
            break;
        }
        let every = grown.contains(WILD_CELL);
        let mut dirty: Vec<usize> = mentions
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                !m.is_empty()
                    && (every || m.contains(WILD_CELL) || m.iter().any(|c| grown.contains(*c)))
            })
            .map(|(k, _)| k)
            .collect();
        if captures_grew {
            dirty.extend(calls_through.iter().copied());
        }
        dirty.sort_unstable();
        dirty.dedup();
        seed = Some(dirty);
    }

    // Every sink of every class, with what reaches it and how (N52, N80): its own flow's
    // origins, and what every caller — transitively — hands the parameters it depends on.
    let tracer = Tracer {
        p: &program,
        values: &values,
        res,
    };
    let mut reports = Vec::new();
    for (k, f) in program.facts.iter().enumerate() {
        let Some(vals) = values.get(k) else { continue };
        for s in &f.sinks {
            let origins = program.reaching(f, &s.path, vals);
            let trace = origins
                .iter()
                .map(|o| {
                    let mut steps = vec![Step {
                        at: s.at,
                        note: format!("it reaches {} here", sink_words(s.class)),
                    }];
                    tracer.flow(k, &s.path, o, &mut steps, &mut BTreeSet::new(), 0);
                    steps.reverse();
                    (o, steps)
                })
                .collect();
            reports.push(SinkReport {
                function: f.function,
                class: s.class,
                at: s.at,
                origins,
                trace,
            });
        }
    }

    // The language's policy, in functions that declared an effect set: the same functions
    // N37 holds to their authority. One diagnostic per site, in module and body order.
    let rule = FlowRule::LANGUAGE;
    for (k, f) in program.facts.iter().enumerate() {
        let explicit = res
            .function(f.function)
            .is_some_and(|d| d.declared_effects.is_some());
        if !explicit {
            continue;
        }
        let Some(vals) = values.get(k) else { continue };
        let refused = |flow: &Flow| -> Option<Origin> {
            program
                .eval(flow, vals)
                .origins
                .intersection(rule.forbidden)
                .iter()
                .next()
        };
        let mut found: Vec<(Span, Diagnostic)> = Vec::new();
        for s in f.sinks.iter().filter(|s| s.class == rule.class) {
            if let Some(o) = refused(&s.path) {
                found.push((s.at, tracer.direct(k, s, o)));
            }
        }
        for c in &f.calls {
            for i in program.reach_of(c.callee, rule.class) {
                let Some(a) = c.args.get(i as usize) else {
                    continue;
                };
                if let Some(o) = refused(a) {
                    found.push((c.at, tracer.through(k, c.callee, c.at, i, a, o, None)));
                    break;
                }
            }
        }
        for c in &f.indirect {
            'candidates: for &g in program.candidates(&c.target) {
                for i in program.reach_of(g, rule.class) {
                    if i as usize >= program.arity(g) {
                        continue;
                    }
                    let Some(a) = c.args.get(i as usize) else {
                        continue;
                    };
                    if let Some(o) = refused(a) {
                        found.push((c.at, tracer.through(k, g, c.at, i, a, o, Some("value"))));
                        break 'candidates;
                    }
                }
            }
        }
        for m in &f.makes {
            let base = program.arity(m.function);
            for i in program.reach_of(m.function, rule.class) {
                let Some(j) = (i as usize).checked_sub(base) else {
                    continue;
                };
                let Some(a) = m.captures.get(j) else { continue };
                if let Some(o) = refused(a) {
                    found.push((
                        m.at,
                        tracer.through(k, m.function, m.at, i, a, o, Some("capture")),
                    ));
                    break;
                }
            }
        }
        found.sort_by_key(|(at, _)| (at.start, at.end));
        errors.extend(found.into_iter().map(|(_, d)| d));
    }

    for r in reports {
        res.record_sink(r);
    }
    for (k, f) in program.facts.iter().enumerate() {
        let _ = k;
        let i = f.function.index();
        let mut s = program.summaries.get(i).cloned().unwrap_or_default();
        s.reaches = program
            .reach_of(f.function, SinkClass::Path)
            .into_iter()
            .filter(|p| (*p as usize) < program.arity(f.function))
            .collect();
        s.receives = program
            .receives
            .get(i)
            .map(|r| r.iter().take(program.arity(f.function)).copied().collect())
            .unwrap_or_default();
        res.record_provenance(f.function, s);
    }
}

/// Summaries' origins and pass-through parameters: a worklist to the least fixed point,
/// against the current cells and receives. It starts from the previous round's summaries,
/// which are below this round's fixed point because cells and receives only grow, so it
/// reaches the same least fixed point as a start from empty, with less work.
fn settle(
    program: &mut Program<'_>,
    worst: &[FnId],
    res: &Resolution,
    callers: &[Vec<usize>],
    seed: Option<&[usize]>,
) {
    for &g in worst {
        let all: Vec<u32> = (0..program.arity(g)).map(position).collect();
        if let Some(s) = program.summaries.get_mut(g.index()) {
            *s = Summary {
                origins: OriginSet::of(Origin::Unknown),
                params: all,
                ..Summary::default()
            };
        }
    }
    // A foreign function (N42) has no body to reduce: its result is C's, which may be
    // anything — `Unknown`, the worst origin — and may be derived from any argument. What C
    // does with an argument is not a Nazm sink: the `ForeignCap` the call needed authorised it.
    for (i, def) in res.functions().iter().enumerate() {
        if def.foreign.is_none() {
            continue;
        }
        if let Some(s) = program.summaries.get_mut(i) {
            *s = Summary {
                origins: OriginSet::of(Origin::Unknown),
                params: (0..def.params.len()).map(position).collect(),
                ..Summary::default()
            };
        }
    }
    let count = program.facts.len();
    let (mut queue, mut queued): (std::collections::VecDeque<usize>, Vec<bool>) = match seed {
        None => ((0..count).rev().collect(), vec![true; count]),
        Some(seed) => {
            let mut q = vec![false; count];
            for &k in seed {
                if let Some(x) = q.get_mut(k) {
                    *x = true;
                }
            }
            (seed.iter().copied().collect(), q)
        }
    };
    while let Some(k) = queue.pop_front() {
        if let Some(q) = queued.get_mut(k) {
            *q = false;
        }
        let Some(f) = program.facts.get(k).copied() else {
            continue;
        };
        let vals = program.values(f);
        let r = program.eval(&f.result, &vals);
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
}

/// Which parameters reach a sink of each class: a worklist over the settled values, since
/// reaching a sink changes no value.
fn settle_reach(program: &mut Program<'_>, values: &[Vals], callers: &[Vec<usize>]) {
    if program.incomplete {
        // An indirect call may run a function of a module whose facts are missing.
        for (g, def) in program.arity.clone().iter().enumerate() {
            let _ = def;
            if program.facts_of(FnId::from_index(g)).is_none() {
                if let Some(r) = program.reach.get_mut(g) {
                    for class in SinkClass::ALL {
                        r.insert(
                            class,
                            (0..program.arity.get(g).copied().unwrap_or(0))
                                .map(position)
                                .collect(),
                        );
                    }
                }
            }
        }
    }
    let count = program.facts.len();
    let mut queue: std::collections::VecDeque<usize> = (0..count).rev().collect();
    let mut queued = vec![true; count];
    while let Some(k) = queue.pop_front() {
        if let Some(q) = queued.get_mut(k) {
            *q = false;
        }
        let (Some(f), Some(vals)) = (program.facts.get(k).copied(), values.get(k)) else {
            continue;
        };
        let mut reach: Reach = program
            .reach
            .get(f.function.index())
            .cloned()
            .unwrap_or_default();
        let before = reach.clone();
        for s in &f.sinks {
            reach
                .entry(s.class)
                .or_default()
                .extend(program.eval(&s.path, vals).params);
        }
        for class in SinkClass::ALL {
            let mut add = BTreeSet::new();
            for c in &f.calls {
                for i in program.reach_of(c.callee, class) {
                    if let Some(a) = c.args.get(i as usize) {
                        add.extend(program.eval(a, vals).params);
                    }
                }
            }
            for c in &f.indirect {
                for &g in program.candidates(&c.target) {
                    for i in program.reach_of(g, class) {
                        if (i as usize) < program.arity(g) {
                            if let Some(a) = c.args.get(i as usize) {
                                add.extend(program.eval(a, vals).params);
                            }
                        }
                    }
                }
            }
            for m in &f.makes {
                let base = program.arity(m.function);
                for i in program.reach_of(m.function, class) {
                    if let Some(a) = (i as usize)
                        .checked_sub(base)
                        .and_then(|j| m.captures.get(j))
                    {
                        add.extend(program.eval(a, vals).params);
                    }
                }
            }
            if !add.is_empty() {
                reach.entry(class).or_default().extend(add);
            }
        }
        if reach != before {
            if let Some(r) = program.reach.get_mut(f.function.index()) {
                *r = reach;
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
}

/// What each parameter and capture is handed, over every call, indirect call, `spawn` and
/// closure the program makes, against the settled values. Grows only.
fn settle_receives(program: &mut Program<'_>, values: &[Vals], res: &Resolution) {
    // An export is called by C, with anything (N55); a function value's parameters, when
    // some module's facts are missing, by calls this run cannot see.
    for (i, def) in res.functions().iter().enumerate() {
        let open = def.export.is_some() || (program.incomplete && def.closure.is_some());
        if !open {
            continue;
        }
        if let Some(r) = program.receives.get_mut(i) {
            for o in r.iter_mut() {
                *o = o.with(Origin::Unknown);
            }
        }
    }
    // Every edge that hands a value to a parameter or capture, evaluated once: what it
    // carries itself, and which of the sender's parameters it passes on. Values are settled
    // for this round, so only the parameters' receives move below.
    let mut edges: Vec<(FnId, usize, OriginSet, FnId, Vec<u32>)> = Vec::new();
    for (k, f) in program.facts.iter().enumerate() {
        let Some(vals) = values.get(k) else { continue };
        let mut edge = |g: FnId, i: usize, a: &Flow| {
            let v = program.eval(a, vals);
            edges.push((g, i, v.origins, f.function, v.params.into_iter().collect()));
        };
        for c in &f.calls {
            for (i, a) in c.args.iter().enumerate() {
                edge(c.callee, i, a);
            }
        }
        for c in &f.indirect {
            for &g in program.candidates(&c.target) {
                for (i, a) in c.args.iter().enumerate().take(program.arity(g)) {
                    edge(g, i, a);
                }
            }
        }
        for m in &f.makes {
            let base = program.arity(m.function);
            for (j, a) in m.captures.iter().enumerate() {
                edge(m.function, base + j, a);
            }
        }
    }
    loop {
        let mut changed = false;
        for (g, i, own, from, params) in &edges {
            let o = params
                .iter()
                .fold(*own, |o, p| o.union(program.received(*from, *p)));
            if let Some(slot) = program
                .receives
                .get_mut(g.index())
                .and_then(|r| r.get_mut(*i))
            {
                let grown = slot.union(o);
                if grown != *slot {
                    *slot = grown;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Every cell a function's flows name (N80).
fn cells_named(f: &FunctionFacts) -> BTreeSet<&str> {
    std::iter::once(&f.result)
        .chain(f.calls.iter().flat_map(|c| c.args.iter()))
        .chain(f.indirect.iter().flat_map(|c| c.args.iter()))
        .chain(f.makes.iter().flat_map(|m| m.captures.iter()))
        .chain(f.sinks.iter().map(|s| &s.path))
        .chain(f.stores.iter().map(|s| &s.value))
        .flat_map(|flow| flow.cells.iter().map(|(c, _)| c.as_str()))
        .collect()
}

fn sink_words(class: SinkClass) -> &'static str {
    match class {
        SinkClass::Path => "the path `write_file` writes to",
        SinkClass::Output => "standard output or error",
        SinkClass::FileData => "what `write_file` writes",
        SinkClass::Vouch => "a vouch",
    }
}

const HELP: &str = "in a function that declares its effects, the path a program writes to may \
    come from the command line or be computed in the program, but not from the contents of a \
    file, nor from a device or foreign (C) function the checker does not follow. `str_vouch`, \
    under a `VouchCap`, declassifies a value on purpose";

fn name(res: &Resolution, g: FnId) -> String {
    res.function(g)
        .map_or_else(|| "a function".to_owned(), |d| format!("`{}`", d.name))
}

/// Builds witness chains (N80): for a diagnostic, and for every sink's trace.
struct Tracer<'a, 'p> {
    p: &'a Program<'p>,
    values: &'a [Vals],
    res: &'a Resolution,
}

/// Where a chain has been, so a cycle through calls or cells ends.
type Seen = BTreeSet<(usize, u32, u32)>;

impl Tracer<'_, '_> {
    fn vals(&self, k: usize) -> Vals {
        self.values.get(k).cloned().unwrap_or_default()
    }

    fn index(&self, g: FnId) -> Option<usize> {
        self.p.at.get(g.index()).copied().flatten()
    }

    fn direct(&self, k: usize, s: &SinkFact, o: Origin) -> Diagnostic {
        let mut d = Diagnostic::error(
            codes::RESTRICTED_FLOW,
            s.at,
            format!(
                "the path `write_file` writes to comes from {}",
                o.describe()
            ),
        );
        d.primary.message = format!("this path comes from {}", o.describe());
        let mut steps = Vec::new();
        self.flow(k, &s.path, o, &mut steps, &mut Seen::new(), 0);
        for st in steps {
            d = d.secondary(st.at, st.note);
        }
        d.help(HELP)
    }

    /// A call of `g` at `at` hands `arg`, carrying `o`, to its parameter `i`, which reaches
    /// the path. `via` says how `g` is reached: a call through a value, or a capture.
    #[allow(clippy::too_many_arguments)]
    fn through(
        &self,
        k: usize,
        g: FnId,
        at: Span,
        i: u32,
        arg: &Flow,
        o: Origin,
        via: Option<&str>,
    ) -> Diagnostic {
        let callee = name(self.res, g);
        let param = self
            .res
            .function(g)
            .and_then(|d| match &d.closure {
                Some(c) if i as usize >= d.params.len() => c
                    .captures
                    .get(i as usize - d.params.len())
                    .and_then(|cap| d.locals.get(cap.inner as usize)),
                _ => d.locals.get(i as usize),
            })
            .map_or_else(|| format!("#{i}"), |l| format!("`{}`", l.name));
        let message = match via {
            Some("capture") => format!(
                "this closure writes to a path made from what it captures, {param}, and that \
                 comes from {}",
                o.describe()
            ),
            Some(_) => format!(
                "a call through this value may run {callee}, which writes to a path made from \
                 its parameter {param}, and this argument comes from {}",
                o.describe()
            ),
            None => format!(
                "{callee} writes to a path made from its parameter {param}, and this argument \
                 comes from {}",
                o.describe()
            ),
        };
        let mut d = Diagnostic::error(codes::RESTRICTED_FLOW, at, message);
        d.primary.message = format!("{callee}'s parameter {param} reaches `write_file`'s path");
        let mut steps = Vec::new();
        self.reaching(g, i, SinkClass::Path, &mut steps, 0);
        self.flow(k, arg, o, &mut steps, &mut Seen::new(), 0);
        for st in steps {
            d = d.secondary(st.at, st.note);
        }
        d.help(HELP)
    }

    /// How parameter `i` of `g` reaches a sink of `class`: one chain to it.
    fn reaching(&self, g: FnId, i: u32, class: SinkClass, steps: &mut Vec<Step>, depth: usize) {
        if depth > 32 {
            return;
        }
        let Some(k) = self.index(g) else { return };
        let Some(f) = self.p.facts.get(k).copied() else {
            return;
        };
        let vals = self.vals(k);
        for s in f.sinks.iter().filter(|s| s.class == class) {
            if self.p.eval(&s.path, &vals).params.contains(&i) {
                steps.push(Step {
                    at: s.at,
                    note: format!("in {}: it is the path written here", name(self.res, g)),
                });
                return;
            }
        }
        for c in &f.calls {
            for j in self.p.reach_of(c.callee, class) {
                let Some(a) = c.args.get(j as usize) else {
                    continue;
                };
                if self.p.eval(a, &vals).params.contains(&i) {
                    steps.push(Step {
                        at: c.at,
                        note: format!(
                            "in {}: it is passed to {}",
                            name(self.res, g),
                            name(self.res, c.callee)
                        ),
                    });
                    self.reaching(c.callee, j, class, steps, depth + 1);
                    return;
                }
            }
        }
        for c in &f.indirect {
            for &h in self.p.candidates(&c.target) {
                for j in self.p.reach_of(h, class) {
                    let Some(a) = c.args.get(j as usize) else {
                        continue;
                    };
                    if self.p.eval(a, &vals).params.contains(&i) {
                        steps.push(Step {
                            at: c.at,
                            note: format!(
                                "in {}: it is passed through a value that may be {}",
                                name(self.res, g),
                                name(self.res, h)
                            ),
                        });
                        self.reaching(h, j, class, steps, depth + 1);
                        return;
                    }
                }
            }
        }
        for m in &f.makes {
            let base = self.p.arity(m.function);
            for j in self.p.reach_of(m.function, class) {
                let Some(a) = (j as usize)
                    .checked_sub(base)
                    .and_then(|x| m.captures.get(x))
                else {
                    continue;
                };
                if self.p.eval(a, &vals).params.contains(&i) {
                    steps.push(Step {
                        at: m.at,
                        note: format!("in {}: a closure captures it here", name(self.res, g)),
                    });
                    self.reaching(m.function, j, class, steps, depth + 1);
                    return;
                }
            }
        }
    }

    /// Where `o` entered `flow`, in the function at `k`: one chain, deterministic, through
    /// calls, cells and callers to its source. Each step is pushed nearest the sink first.
    fn flow(
        &self,
        k: usize,
        flow: &Flow,
        o: Origin,
        steps: &mut Vec<Step>,
        seen: &mut Seen,
        depth: usize,
    ) {
        if depth > 32 {
            return;
        }
        if let Some((_, at)) = flow.why.iter().find(|(x, _)| *x == o) {
            steps.push(Step {
                at: *at,
                note: o.enters().to_owned(),
            });
            return;
        }
        let Some(f) = self.p.facts.get(k).copied() else {
            return;
        };
        let vals = self.vals(k);
        for &ci in &flow.calls {
            let Some(c) = f.calls.get(ci as usize) else {
                continue;
            };
            let Some(s) = self.p.summary(c.callee) else {
                continue;
            };
            if s.origins.contains(o) {
                let said = if self
                    .res
                    .function(c.callee)
                    .is_some_and(|d| d.foreign.is_some())
                {
                    format!(
                        "{} is a foreign function, and what C returns is not followed",
                        name(self.res, c.callee)
                    )
                } else {
                    format!(
                        "{} returns data from {}",
                        name(self.res, c.callee),
                        o.describe()
                    )
                };
                steps.push(Step {
                    at: c.at,
                    note: said,
                });
                if let Some(j) = self.index(c.callee) {
                    if seen.insert((j, 0, u32::MAX)) {
                        if let Some(g) = self.p.facts.get(j).copied() {
                            self.flow(j, &g.result, o, steps, seen, depth + 1);
                        }
                    }
                }
                return;
            }
            for &param in &s.params {
                if (param as usize) >= self.p.arity(c.callee) {
                    if self.p.received(c.callee, param).contains(o) {
                        steps.push(Step {
                            at: c.at,
                            note: format!("{} returns what it captured", name(self.res, c.callee)),
                        });
                        self.param(c.callee, param, o, steps, seen, depth + 1);
                        return;
                    }
                    continue;
                }
                let Some(a) = c.args.get(param as usize) else {
                    continue;
                };
                if self.p.reaching(f, a, &vals).contains(o) {
                    steps.push(Step {
                        at: c.at,
                        note: format!("{} passes on what it is given", name(self.res, c.callee)),
                    });
                    self.flow(k, a, o, steps, seen, depth + 1);
                    return;
                }
            }
        }
        for &ci in &flow.via {
            let Some(c) = f.indirect.get(ci as usize) else {
                continue;
            };
            for &g in self.p.candidates(&c.target) {
                let Some(s) = self.p.summary(g) else { continue };
                if s.origins.contains(o) {
                    steps.push(Step {
                        at: c.at,
                        note: format!(
                            "a call through this value may run {}, which returns data from {}",
                            name(self.res, g),
                            o.describe()
                        ),
                    });
                    if let Some(j) = self.index(g) {
                        if seen.insert((j, 0, u32::MAX)) {
                            if let Some(h) = self.p.facts.get(j).copied() {
                                self.flow(j, &h.result, o, steps, seen, depth + 1);
                            }
                        }
                    }
                    return;
                }
                for &param in &s.params {
                    if (param as usize) >= self.p.arity(g) {
                        if self.p.received(g, param).contains(o) {
                            steps.push(Step {
                                at: c.at,
                                note: format!(
                                    "a call through this value may run {}, which returns what it \
                                     captured",
                                    name(self.res, g)
                                ),
                            });
                            self.param(g, param, o, steps, seen, depth + 1);
                            return;
                        }
                        continue;
                    }
                    let Some(a) = c.args.get(param as usize) else {
                        continue;
                    };
                    if self.p.reaching(f, a, &vals).contains(o) {
                        steps.push(Step {
                            at: c.at,
                            note: format!(
                                "a call through this value may run {}, which passes on what it is \
                                 given",
                                name(self.res, g)
                            ),
                        });
                        self.flow(k, a, o, steps, seen, depth + 1);
                        return;
                    }
                }
            }
            if self.p.incomplete && o == Origin::Unknown {
                steps.push(Step {
                    at: c.at,
                    note: "a call through this value may run a function whose facts are missing"
                        .to_owned(),
                });
                return;
            }
        }
        for (cell, at) in &flow.cells {
            if !self.p.cell(cell).contains(o) {
                continue;
            }
            steps.push(Step {
                at: *at,
                note: format!("this is read out of {}", cell_words(cell)),
            });
            if self.cell(cell, o, steps, seen, depth + 1) {
                return;
            }
        }
        for &p in &self.p.eval(flow, &vals).params {
            if self.p.received(f.function, p).contains(o) {
                self.param(f.function, p, o, steps, seen, depth + 1);
                return;
            }
        }
    }

    /// A write into `cell` — or into the wild cell, or any cell when it is the wild one —
    /// that carries `o`, and where that came from.
    fn cell(
        &self,
        cell: &str,
        o: Origin,
        steps: &mut Vec<Step>,
        seen: &mut Seen,
        depth: usize,
    ) -> bool {
        for (j, g) in self.p.facts.iter().enumerate() {
            let vals = self.vals(j);
            for (si, s) in g.stores.iter().enumerate() {
                let matches = cell == WILD_CELL || s.cell == cell || s.cell == WILD_CELL;
                if !matches || !self.p.reaching(g, &s.value, &vals).contains(o) {
                    continue;
                }
                if !seen.insert((j, 1, position(si))) {
                    continue;
                }
                steps.push(Step {
                    at: s.at,
                    note: format!(
                        "in {}: data from {} is written into {} here",
                        name(self.res, g.function),
                        o.describe(),
                        if s.cell == WILD_CELL {
                            "a container in generic code".to_owned()
                        } else {
                            format!("a `{}`", s.cell)
                        }
                    ),
                });
                self.flow(j, &s.value, o, steps, seen, depth);
                return true;
            }
        }
        if self.p.incomplete && o == Origin::Unknown {
            steps.push(Step {
                at: Span::synthetic(),
                note: "a module whose facts are missing may write into it".to_owned(),
            });
            return true;
        }
        false
    }

    /// Who hands parameter (or capture) `p` of `g` the origin `o`.
    fn param(
        &self,
        g: FnId,
        p: u32,
        o: Origin,
        steps: &mut Vec<Step>,
        seen: &mut Seen,
        depth: usize,
    ) {
        if depth > 32 || !seen.insert((g.index(), 2, p)) {
            return;
        }
        let pname = self
            .res
            .function(g)
            .and_then(|d| match &d.closure {
                Some(c) if p as usize >= d.params.len() => c
                    .captures
                    .get(p as usize - d.params.len())
                    .and_then(|cap| d.locals.get(cap.inner as usize)),
                _ => d.locals.get(p as usize),
            })
            .map_or_else(|| format!("#{p}"), |l| format!("`{}`", l.name));
        if self.res.function(g).is_some_and(|d| d.export.is_some()) && o == Origin::Unknown {
            if let Some(d) = self.res.function(g) {
                steps.push(Step {
                    at: d.name_span,
                    note: format!(
                        "{} is exported to C, which may call it with anything",
                        name(self.res, g)
                    ),
                });
            }
            return;
        }
        let base = self.p.arity(g);
        for (j, h) in self.p.facts.iter().enumerate() {
            let vals = self.vals(j);
            if (p as usize) < base {
                for c in h.calls.iter().filter(|c| c.callee == g) {
                    let Some(a) = c.args.get(p as usize) else {
                        continue;
                    };
                    if self.p.reaching(h, a, &vals).contains(o) {
                        steps.push(Step {
                            at: c.at,
                            note: format!(
                                "{} is called here, and its parameter {pname} is handed data from {}",
                                name(self.res, g),
                                o.describe()
                            ),
                        });
                        self.flow(j, a, o, steps, seen, depth + 1);
                        return;
                    }
                }
                for c in &h.indirect {
                    if !self.p.candidates(&c.target).contains(&g) {
                        continue;
                    }
                    let Some(a) = c.args.get(p as usize) else {
                        continue;
                    };
                    if self.p.reaching(h, a, &vals).contains(o) {
                        steps.push(Step {
                            at: c.at,
                            note: format!(
                                "a call through a value may run {} here, and hands its parameter \
                                 {pname} data from {}",
                                name(self.res, g),
                                o.describe()
                            ),
                        });
                        self.flow(j, a, o, steps, seen, depth + 1);
                        return;
                    }
                }
            } else {
                for m in h.makes.iter().filter(|m| m.function == g) {
                    let Some(a) = m.captures.get(p as usize - base) else {
                        continue;
                    };
                    if self.p.reaching(h, a, &vals).contains(o) {
                        steps.push(Step {
                            at: m.at,
                            note: format!(
                                "this closure captures {pname}, which holds data from {}",
                                o.describe()
                            ),
                        });
                        self.flow(j, a, o, steps, seen, depth + 1);
                        return;
                    }
                }
            }
        }
    }
}

fn cell_words(cell: &str) -> String {
    if cell == WILD_CELL {
        "a container in generic code, which may be any container".to_owned()
    } else {
        format!("a `{cell}`, which holds whatever is written into any `{cell}`")
    }
}
