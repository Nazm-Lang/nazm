//! Core IR to MIR: the one lowering, for `nazm build`.
//!
//! # What it reads
//!
//! Core IR, verified (`docs/architecture.md` §7.41), and the checker's semantic tables — the
//! user-type table, signatures, visibility, the call sites the instance planner walks. Never
//! the syntax tree, and never a position map: every identity it needs is already in Core IR.
//!
//! # What it decides
//!
//! What §7.42 says MIR adds, and nothing else: which concrete instances exist; the control-flow
//! graph; the temporaries a value passes through, in the language's evaluation order; the copy,
//! move and drop of every managed value; the join of every scope on every exit; and the failure
//! edge of every operation that can fail.
//!
//! # The cleanup stack
//!
//! One stack, in program order, of everything that must be undone when control leaves the
//! point it was created at: a managed local that owns a value (a binding, or a temporary still
//! waiting for the operation that consumes it), an open `scope`, and a marker where each loop's
//! body begins. Every exit is the same walk down it — `return` to the bottom, `break` and
//! `continue` to their loop's marker, a region's normal end to where the region began — so the
//! cleanup on an edge is decided in one place. A failure edge needs only the open scopes: the
//! shared unwind block drops every managed local, which is safe because a local that holds
//! nothing holds the representation's zero, and releasing zero is a no-op.

use std::collections::BTreeMap;

use nazm_cir as cir;
use nazm_diag::{codes, Diagnostic};
use nazm_sema::key::DefKind;
use nazm_sema::module::ModuleGraph;
use nazm_sema::resolve::{Callee, FnId, Resolution};
use nazm_sema::types::{ParamOwner, Type};
use nazm_sema::udt::TypeNode;
use nazm_sema::{EnumId, Intrinsic, RecordId};
use nazm_span::Span;

use crate::instance::{plan_instances, InstanceKey, Instances};
use crate::ir::{
    Block, BlockId, BlockOrigin, Const, EnumInfo, FieldInfo, FieldRef, FnInfo, FuncId, Function,
    Identity, LocalDecl, LocalId, LocalOrigin, LocalTy, Operand, Program, RecordInfo, Rvalue,
    Statement, StatementKind, Terminator, TypeIdentity, Types, VariantInfo, VecInfo,
};
use crate::names::{owner, type_key};

/// Lower a checked program's Core IR to MIR.
///
/// # Errors
///
/// The instance planner's refusals (`N0703`, `N0704`), and a compiler defect (`N0900`) for any
/// place the lowering and Core IR disagree.
pub fn lower(
    core: &cir::Program,
    res: &Resolution,
    graph: &ModuleGraph,
) -> Result<Program, Vec<Diagnostic>> {
    let mut res = res.clone();
    let instances = plan_instances(&mut res, graph)?;
    let res = res;

    // Which MIR function each definition and instance is. Non-generic definitions first, in
    // `FnId` order, then instances in the planner's canonical order.
    let mut plain: Vec<Option<FuncId>> = vec![None; res.functions().len()];
    let mut next = 0u32;
    for f in &core.functions {
        if f.type_params == 0 {
            if let Some(slot) = plain.get_mut(f.def.index()) {
                *slot = Some(FuncId(next));
                next += 1;
            }
        }
    }
    let first_instance = next;
    // A named function used as a value is called through a thunk (N50): code of a function
    // value's shape — the closure object first — that calls it. One per (module, function),
    // in the module that uses it, after every instance.
    let mut thunks: BTreeMap<(usize, usize), FuncId> = BTreeMap::new();
    let mut next_thunk = first_instance + u32::try_from(instances.planned.len()).unwrap_or(0);
    let mut thunk_plan: Vec<(nazm_sema::ModuleId, FnId, Span)> = Vec::new();
    for use_ in res.fn_values() {
        let Some(m) = graph.ids().find(|m| graph.file(*m) == Some(use_.at.file)) else {
            continue;
        };
        thunks
            .entry((m.index(), use_.function.index()))
            .or_insert_with(|| {
                thunk_plan.push((m, use_.function, use_.at));
                let id = FuncId(next_thunk);
                next_thunk += 1;
                id
            });
    }
    let map = FuncMap {
        plain,
        instances: &instances,
        first_instance,
        thunks,
    };

    // Functions a generic definition's body calls: an instance of that body is an artefact of
    // its own, so what it calls must be bindable across objects. Decided by the definitions.
    let mut from_generic = vec![false; res.functions().len()];
    for f in res.functions() {
        if f.type_params.is_empty() {
            continue;
        }
        for site in &f.call_sites {
            if let Callee::Function(g) = site.callee {
                if let Some(slot) = from_generic.get_mut(g.index()) {
                    *slot = true;
                }
            }
        }
    }

    // Which definitions are in a call cycle, and so begin with a stack check (N49).
    let facts = Facts {
        from_generic,
        recursive: in_call_cycles(&res),
    };

    let mut functions = Vec::new();
    let mut defects: Vec<Diagnostic> = Vec::new();
    for f in &core.functions {
        if f.type_params > 0 {
            continue;
        }
        let Some(id) = map.plain.get(f.def.index()).copied().flatten() else {
            continue;
        };
        let identity = Identity {
            owner: owner(graph, f.module, DefKind::Function, &f.name),
            args: Vec::new(),
        };
        match lower_function(&res, &map, f, None, id, identity, &facts) {
            Ok(m) => functions.push(m),
            Err(e) => defects.push(*e),
        }
    }
    for (at, planned) in instances.planned.iter().enumerate() {
        let Some(f) = core.function(planned.key.def) else {
            defects.push(defect(
                Span::synthetic(),
                "an instance names a definition with no Core IR",
            ));
            continue;
        };
        let id = FuncId(first_instance + u32::try_from(at).unwrap_or(u32::MAX));
        let subst = Some((
            ParamOwner::Function(planned.key.def),
            planned.key.args.clone(),
        ));
        match lower_function(&res, &map, f, subst, id, planned.identity.clone(), &facts) {
            Ok(m) => functions.push(m),
            Err(e) => defects.push(*e),
        }
    }
    for (module, g, at) in thunk_plan {
        let Some(id) = map.thunk(module, g) else {
            continue;
        };
        let Some(target) = core.function(g) else {
            defects.push(defect(
                at,
                "a function value names a function with no Core IR",
            ));
            continue;
        };
        let Some(ty) = res.fn_value(at).map(|u| u.ty) else {
            continue;
        };
        let thunk = thunk_of(target, module, ty, graph);
        let identity = Identity {
            owner: owner(graph, module, DefKind::Function, &thunk.name),
            args: Vec::new(),
        };
        match lower_function(&res, &map, &thunk, None, id, identity, &facts) {
            Ok(mut m) => {
                m.exported = false;
                functions.push(m);
            }
            Err(e) => defects.push(*e),
        }
    }
    if !defects.is_empty() {
        return Err(defects);
    }

    let types = collect_types(&res, graph, &functions);
    let mut modules: Vec<(nazm_sema::ModuleId, nazm_span::FileId)> = res
        .functions()
        .iter()
        .map(|d| (d.module, d.name_span.file))
        .collect();
    modules.sort_by_key(|(m, _)| m.index());
    modules.dedup_by_key(|(m, _)| *m);
    let entry = res
        .entry()
        .and_then(|e| map.plain.get(e.index()).copied().flatten());
    Ok(Program {
        functions,
        modules,
        types,
        entry,
    })
}

/// The Core IR of the thunk through which `module` calls `target` as a value (N50): the
/// closure object, then `target`'s parameters, and a body that is the call.
fn thunk_of(
    target: &cir::Function,
    module: nazm_sema::ModuleId,
    ty: Type,
    graph: &ModuleGraph,
) -> cir::Function {
    let at = target.name_span;
    let params = target.param_locals();
    let mut locals = vec![cir::Local {
        ty,
        name: "env".to_owned(),
        origin: cir::LocalOrigin::Param,
        at,
    }];
    locals.extend(params.iter().cloned());
    let args = params
        .iter()
        .enumerate()
        .map(|(i, l)| cir::Expr {
            kind: cir::ExprKind::Local(cir::LocalId(u32::try_from(i + 1).unwrap_or(u32::MAX))),
            ty: Some(l.ty),
            at,
        })
        .collect();
    // Named for where the target is, so two functions of one name are two thunks.
    let home = graph
        .key(target.module)
        .map_or_else(|| format!("{}", target.module.index()), |k| k.to_string());
    cir::Function {
        def: target.def,
        module,
        name: format!("{}#value@{home}", target.name),
        type_params: 0,
        params: target.params + 1,
        locals,
        ret: target.ret,
        consts: Vec::new(),
        loops: 0,
        effects: target.effects,
        body: cir::Block {
            stmts: Vec::new(),
            tail: Some(Box::new(cir::Expr {
                kind: cir::ExprKind::Call {
                    callee: target.def,
                    type_args: Vec::new(),
                    args,
                    callee_at: at,
                },
                ty: Some(target.ret),
                at,
            })),
            ends: cir::Ends::FallsThrough,
            owns: Vec::new(),
            at,
        },
        span: target.span,
        name_span: at,
        ret_span: target.ret_span,
        foreign: None,
        export: None,
        captures: Vec::new(),
        closure: true,
    }
}

fn defect(at: Span, what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        codes::COMPILER_DEFECT,
        at,
        format!(
            "the compiler could not lower Core IR to MIR: {}",
            what.into()
        ),
    )
    .help("this is a bug in the compiler, not in the program: it checked, and MIR lowering disagreed with Core IR")
}

struct FuncMap<'a> {
    plain: Vec<Option<FuncId>>,
    instances: &'a Instances,
    first_instance: u32,
    /// The thunk each module calls a named function through when it uses it as a value
    /// (N50), by (module, function).
    thunks: BTreeMap<(usize, usize), FuncId>,
}

impl FuncMap<'_> {
    fn thunk(&self, module: nazm_sema::ModuleId, g: FnId) -> Option<FuncId> {
        self.thunks.get(&(module.index(), g.index())).copied()
    }

    fn instance(&self, key: &InstanceKey) -> Option<FuncId> {
        self.instances
            .position(key)
            .and_then(|p| u32::try_from(p).ok())
            .map(|p| FuncId(self.first_instance + p))
    }
}

/// One entry of the cleanup stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    /// A managed local that owns a value until the entry is popped.
    Owned(LocalId),
    /// An open `scope`, by its task list.
    Scope(LocalId),
    /// Where a loop's body begins.
    Loop(cir::LoopId),
}

struct LoopFrame {
    id: cir::LoopId,
    head: BlockId,
    exit: BlockId,
    /// The cleanup stack's height where the body begins, marker included.
    depth: usize,
}

/// A block being built: its terminator arrives when it is sealed.
struct Building {
    stmts: Vec<Statement>,
    term: Option<(Terminator, Span)>,
    origin: BlockOrigin,
}

struct Fx<'a> {
    res: &'a Resolution,
    map: &'a FuncMap<'a>,
    f: &'a cir::Function,
    subst: Option<(ParamOwner, Vec<Type>)>,
    locals: Vec<LocalDecl>,
    blocks: Vec<Building>,
    /// Where statements are appended, or `None` when control cannot reach this point.
    cur: Option<BlockId>,
    stack: Vec<Entry>,
    /// The task lists of the `scope`s open at this point, innermost last: the [`Entry::Scope`]
    /// entries of [`Self::stack`], kept apart so that a failure edge — every fallible
    /// operation has one — finds them without walking every binding in the function.
    scopes: Vec<LocalId>,
    loops: Vec<LoopFrame>,
    /// The failure-cleanup block for each set of open scopes, innermost last.
    cleanups: BTreeMap<Vec<LocalId>, BlockId>,
    shared_unwind: Option<BlockId>,
    ret: Type,
    error: Option<(Span, String)>,
}

/// What the whole program says about each definition, by `FnId` index, before any body is
/// lowered.
struct Facts {
    /// A generic definition's body calls it, so its symbol must be bindable across objects.
    from_generic: Vec<bool>,
    /// It is in a call cycle, so it checks its stack first (N49).
    recursive: Vec<bool>,
}

/// Which definitions are in a call cycle — a cycle of calls between definitions, generic or
/// not, through any number of modules. `spawn` closes none: a task runs on its own stack, so a
/// function that spawns itself grows no stack (N49, `docs/spec.md` *Native recursion*).
///
/// Over definitions rather than instances, so an instance of a recursive generic definition
/// checks too; polymorphic recursion never reaches here (`N0357`). Tarjan's algorithm with an
/// explicit stack, so a deep call graph costs heap and not this compiler's stack.
fn in_call_cycles(res: &Resolution) -> Vec<bool> {
    let fns = res.functions();
    let n = fns.len();
    let edges: Vec<Vec<usize>> = fns
        .iter()
        .map(|f| {
            f.call_sites
                .iter()
                .filter(|site| !site.spawned)
                .filter_map(|site| match site.callee {
                    Callee::Function(g) => Some(g.index()),
                    _ => None,
                })
                .filter(|&g| g < n)
                .collect()
        })
        .collect();
    // A call through a function value may reach anything (N50): which function it calls is
    // decided at run time, so any cycle through one contains a function that makes one.
    let indirect: Vec<bool> = fns
        .iter()
        .map(|f| {
            f.call_sites
                .iter()
                .any(|s| !s.spawned && matches!(s.callee, Callee::Indirect(_)))
        })
        .collect();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];

    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut out = vec![false; n];
    let mut next = 0usize;
    for root in 0..n {
        if index.get(root).copied() != Some(usize::MAX) {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(v, i)) = work.last() {
            if i == 0 {
                if let (Some(x), Some(l)) = (index.get_mut(v), low.get_mut(v)) {
                    *x = next;
                    *l = next;
                }
                next += 1;
                stack.push(v);
                if let Some(o) = on_stack.get_mut(v) {
                    *o = true;
                }
            }
            if let Some(&w) = edges.get(v).and_then(|e| e.get(i)) {
                if let Some(top) = work.last_mut() {
                    top.1 = i + 1;
                }
                if index.get(w).copied() == Some(usize::MAX) {
                    work.push((w, 0));
                } else if on_stack.get(w).copied().unwrap_or(false) {
                    let iw = index.get(w).copied().unwrap_or(0);
                    if let Some(l) = low.get_mut(v) {
                        *l = (*l).min(iw);
                    }
                }
                continue;
            }
            work.pop();
            let lv = low.get(v).copied().unwrap_or(0);
            if let Some(&(parent, _)) = work.last() {
                if let Some(l) = low.get_mut(parent) {
                    *l = (*l).min(lv);
                }
            }
            if lv == index.get(v).copied().unwrap_or(0) {
                let mut members = Vec::new();
                while let Some(w) = stack.pop() {
                    if let Some(o) = on_stack.get_mut(w) {
                        *o = false;
                    }
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
                let cycle = members.len() > 1 || edges.get(v).is_some_and(|e| e.contains(&v));
                if cycle {
                    for m in members {
                        if let Some(o) = out.get_mut(m) {
                            *o = true;
                        }
                    }
                }
            }
        }
    }
    for (o, i) in out.iter_mut().zip(indirect) {
        *o |= i;
    }
    out
}

fn lower_function(
    res: &Resolution,
    map: &FuncMap<'_>,
    f: &cir::Function,
    subst: Option<(ParamOwner, Vec<Type>)>,
    id: FuncId,
    identity: Identity,
    facts: &Facts,
) -> Result<Function, Box<Diagnostic>> {
    if let Some(symbol) = &f.foreign {
        // A foreign function: its signature, as locals, and no body (N42).
        let locals = f
            .locals
            .iter()
            .take(f.params as usize)
            .map(|l| LocalDecl {
                ty: LocalTy::Value(l.ty),
                name: l.name.clone(),
                origin: LocalOrigin::Param,
                at: l.at,
            })
            .collect();
        return Ok(Function {
            id,
            def: f.def,
            type_args: Vec::new(),
            module: f.module,
            name: f.name.clone(),
            identity,
            exported: res
                .function(f.def)
                .is_some_and(|d| d.visibility.is_exported()),
            reached_from_generic: false,
            params: f.params,
            locals,
            ret: f.ret,
            blocks: Vec::new(),
            effects: f.effects.published(),
            span: f.span,
            ret_span: f.ret_span,
            foreign: Some(symbol.clone()),
            export: None,
            closure: None,
        });
    }
    let mut fx = Fx {
        res,
        map,
        f,
        subst,
        locals: Vec::with_capacity(f.locals.len() + 8),
        blocks: Vec::new(),
        cur: None,
        stack: Vec::new(),
        scopes: Vec::new(),
        loops: Vec::new(),
        cleanups: BTreeMap::new(),
        shared_unwind: None,
        ret: Type::Int,
        error: None,
    };
    // Core IR's locals keep their numbers: parameters, bindings, then its own temporaries.
    for l in &f.locals {
        let ty = fx.ty(l.ty, l.at);
        let origin = match l.origin {
            cir::LocalOrigin::Param => LocalOrigin::Param,
            cir::LocalOrigin::Binding { mutable } => LocalOrigin::Binding { mutable },
            cir::LocalOrigin::Temp(_) => LocalOrigin::Temp,
        };
        fx.locals.push(LocalDecl {
            ty: LocalTy::Value(ty),
            name: l.name.clone(),
            origin,
            at: l.at,
        });
    }
    fx.ret = fx.ty(f.ret, f.ret_span);
    let entry = fx.new_block(BlockOrigin::Source);
    fx.cur = Some(entry);
    // A function in a call cycle checks its thread's stack before anything else (N49): the
    // parameters are borrowed and nothing is owned yet, so its failure edge releases nothing.
    if facts.recursive.get(f.def.index()).copied().unwrap_or(false) {
        let unwind = fx.unwind();
        fx.push(StatementKind::StackCheck { unwind }, f.name_span);
    }

    // A function value's code reads what its closure object carries into its own locals,
    // which it then owns like bindings (N50).
    if f.closure {
        for (k, l) in f.captures.iter().enumerate() {
            let l = LocalId(l.0);
            fx.assign(
                l,
                Rvalue::Capture {
                    env: LocalId(0),
                    index: u32::try_from(k).unwrap_or(u32::MAX),
                },
                f.name_span,
            );
            fx.own(l);
        }
    }
    let result = fx.temp(fx.ret, f.body.at, "result");
    fx.block_into(&f.body, Some(result));
    if fx.cur.is_some() {
        // The body fell through with its value: drop what the function still owns — nothing
        // but the result, whose ownership leaves with it.
        let depth = 0;
        fx.cleanup_to(depth, f.body.at);
        fx.stack.clear();
        fx.seal(Terminator::Return(result), f.body.at);
    }
    fx.finish_unwind();
    let blocks = fx.finish_blocks();
    if let Some((at, what)) = fx.error.take() {
        return Err(Box::new(defect(at, format!("in `{}`: {what}", f.name))));
    }
    let def = res.function(f.def);
    let closure: Option<Vec<Type>> = f.closure.then(|| {
        f.captures
            .iter()
            .map(|l| {
                fx.locals
                    .get(l.index())
                    .and_then(|d| d.ty.value())
                    .unwrap_or(Type::Int)
            })
            .collect()
    });
    Ok(Function {
        id,
        def: f.def,
        type_args: fx
            .subst
            .as_ref()
            .map(|(_, a)| a.clone())
            .unwrap_or_default(),
        module: f.module,
        name: f.name.clone(),
        identity,
        exported: def.is_some_and(|d| d.visibility.is_exported()),
        reached_from_generic: facts
            .from_generic
            .get(f.def.index())
            .copied()
            .unwrap_or(false),
        params: f.params,
        locals: fx.locals,
        ret: fx.ret,
        blocks,
        effects: f.effects.published(),
        span: f.span,
        ret_span: f.ret_span,
        foreign: None,
        export: f.export.clone(),
        closure,
    })
}

impl Fx<'_> {
    fn fail(&mut self, at: Span, what: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some((at, what.into()));
        }
    }

    /// A Core IR type under this function's substitution — concrete, or a defect.
    fn ty(&mut self, t: Type, at: Span) -> Type {
        let concrete = match &self.subst {
            Some((owner, args)) => self.res.types().substituted(t, *owner, args),
            None => Some(t),
        };
        match concrete.filter(|c| self.res.types().is_concrete(*c)) {
            Some(c) => c,
            None => {
                let shown = self.res.types().show(t);
                self.fail(at, format!("a type with no concrete instance, `{shown}`"));
                Type::Int
            }
        }
    }

    fn managed(&self, t: Type) -> bool {
        managed_type(self.res, t, 0)
    }

    fn local_ty(&self, l: LocalId) -> Type {
        self.locals
            .get(l.index())
            .and_then(|d| d.ty.value())
            .unwrap_or(Type::Int)
    }

    fn temp(&mut self, ty: Type, at: Span, what: &str) -> LocalId {
        let id = LocalId(u32::try_from(self.locals.len()).unwrap_or(u32::MAX));
        self.locals.push(LocalDecl {
            ty: LocalTy::Value(ty),
            name: format!("{what}#{}", id.0),
            origin: LocalOrigin::Temp,
            at,
        });
        id
    }

    fn tasks_local(&mut self, at: Span) -> LocalId {
        let id = LocalId(u32::try_from(self.locals.len()).unwrap_or(u32::MAX));
        self.locals.push(LocalDecl {
            ty: LocalTy::Tasks,
            name: format!("tasks#{}", id.0),
            origin: LocalOrigin::Temp,
            at,
        });
        id
    }

    fn new_block(&mut self, origin: BlockOrigin) -> BlockId {
        let id = BlockId(u32::try_from(self.blocks.len()).unwrap_or(u32::MAX));
        self.blocks.push(Building {
            stmts: Vec::new(),
            term: None,
            origin,
        });
        id
    }

    fn push(&mut self, kind: StatementKind, at: Span) {
        if let Some(b) = self.cur {
            if let Some(block) = self.blocks.get_mut(b.index()) {
                block.stmts.push(Statement { kind, at });
            }
        }
    }

    /// End the current block with `term`. Control does not reach past it.
    fn seal(&mut self, term: Terminator, at: Span) {
        if let Some(b) = self.cur.take() {
            if let Some(block) = self.blocks.get_mut(b.index()) {
                block.term = Some((term, at));
            }
        }
    }

    fn goto(&mut self, target: BlockId, at: Span) {
        self.seal(Terminator::Goto(target), at);
    }

    /// The block a failure at this point goes to: through every open scope's join, innermost
    /// first, to the shared unwind block.
    fn unwind(&mut self) -> BlockId {
        let shared = match self.shared_unwind {
            Some(b) => b,
            None => {
                let b = self.new_block(BlockOrigin::Unwind);
                self.shared_unwind = Some(b);
                b
            }
        };
        if self.scopes.is_empty() {
            return shared;
        }
        if let Some(b) = self.cleanups.get(&self.scopes) {
            return *b;
        }
        let open = self.scopes.clone();
        let b = self.new_block(BlockOrigin::FailureCleanup);
        if let Some(block) = self.blocks.get_mut(b.index()) {
            for l in open.iter().rev() {
                block.stmts.push(Statement {
                    kind: StatementKind::ScopeJoin { list: *l },
                    at: Span::synthetic(),
                });
            }
            block.term = Some((Terminator::Goto(shared), Span::synthetic()));
        }
        self.cleanups.insert(open, b);
        b
    }

    /// Fill the shared unwind block, once every local is known: drop each managed local.
    fn finish_unwind(&mut self) {
        let Some(b) = self.shared_unwind else {
            return;
        };
        let drops: Vec<Statement> = self
            .locals
            .iter()
            .enumerate()
            .skip(self.f.params as usize)
            .filter(|(_, l)| l.ty.value().is_some_and(|t| self.managed(t)))
            .map(|(i, _)| Statement {
                kind: StatementKind::Drop(LocalId(u32::try_from(i).unwrap_or(u32::MAX))),
                at: Span::synthetic(),
            })
            .collect();
        if let Some(block) = self.blocks.get_mut(b.index()) {
            block.stmts = drops;
            block.term = Some((Terminator::Unwind, Span::synthetic()));
        }
    }

    /// Emit the cleanup for leaving everything above `depth` on the stack, innermost first,
    /// without popping it: the stack still describes the code that follows *textually*.
    fn cleanup_to(&mut self, depth: usize, at: Span) {
        let entries: Vec<Entry> = self.stack.get(depth..).unwrap_or(&[]).to_vec();
        for e in entries.iter().rev() {
            match e {
                Entry::Owned(l) => self.push(StatementKind::Drop(*l), at),
                Entry::Scope(l) => self.push(StatementKind::ScopeJoin { list: *l }, at),
                Entry::Loop(_) => {}
            }
        }
    }

    /// Pop the stack back to `depth`, dropping each owned local on the way — a region's
    /// normal end. Emits nothing if control cannot reach here.
    fn pop_to(&mut self, depth: usize, at: Span) {
        while self.stack.len() > depth {
            match self.stack.pop() {
                Some(Entry::Owned(l)) => self.push(StatementKind::Drop(l), at),
                Some(Entry::Scope(l)) => {
                    self.scopes.pop();
                    self.push(StatementKind::ScopeJoin { list: l }, at);
                }
                Some(Entry::Loop(_)) | None => {}
            }
        }
    }

    /// Forget a stack entry whose value was moved on (not dropped).
    fn forget(&mut self, l: LocalId) {
        if let Some(pos) = self.stack.iter().rposition(|e| *e == Entry::Owned(l)) {
            self.stack.remove(pos);
        }
    }

    fn own(&mut self, l: LocalId) {
        if self.cur.is_some() && self.managed(self.local_ty(l)) {
            self.stack.push(Entry::Owned(l));
        }
    }

    fn assign(&mut self, dst: LocalId, value: Rvalue, at: Span) {
        let unwind = if value.can_fail() && self.cur.is_some() {
            Some(self.unwind())
        } else {
            None
        };
        self.push(StatementKind::Assign { dst, value, unwind }, at);
    }

    fn constant(&mut self, id: cir::ConstId, at: Span) -> Const {
        match self.f.constant(id) {
            Some(cir::Const::Int(v)) => Const::Int(*v),
            Some(cir::Const::Bool(v)) => Const::Bool(*v),
            Some(cir::Const::Str(b)) => Const::Str(b.clone()),
            None => {
                self.fail(at, "a constant missing from its function's table");
                Const::Int(0)
            }
        }
    }

    /// The finished graph: only blocks reachable from the entry, renumbered in creation order.
    /// A block created for a join that nothing reached is dropped here rather than emitted with
    /// no predecessors. A reachable block with no terminator is a defect.
    fn finish_blocks(&mut self) -> Vec<Block> {
        let n = self.blocks.len();
        let mut reached = vec![false; n];
        let mut work = vec![0usize];
        while let Some(b) = work.pop() {
            if reached.get(b).copied().unwrap_or(true) {
                continue;
            }
            if let Some(r) = reached.get_mut(b) {
                *r = true;
            }
            let Some(block) = self.blocks.get(b) else {
                continue;
            };
            for s in &block.stmts {
                if let Some(u) = s.kind.unwind() {
                    work.push(u.index());
                }
            }
            if let Some((t, _)) = &block.term {
                work.extend(t.successors().iter().map(|x| x.index()));
            }
        }
        let mut renumber = vec![None; n];
        let mut next = 0u32;
        for (i, r) in reached.iter().enumerate() {
            if *r {
                if let Some(slot) = renumber.get_mut(i) {
                    *slot = Some(BlockId(next));
                }
                next += 1;
            }
        }
        let map = |b: BlockId| renumber.get(b.index()).copied().flatten().unwrap_or(b);
        let mut out = Vec::with_capacity(next as usize);
        let mut unsealed = false;
        for (i, b) in std::mem::take(&mut self.blocks).into_iter().enumerate() {
            if !reached.get(i).copied().unwrap_or(false) {
                continue;
            }
            let Some((term, term_at)) = b.term else {
                unsealed = true;
                continue;
            };
            let stmts = b
                .stmts
                .into_iter()
                .map(|mut s| {
                    s.kind = remap_statement(s.kind, &map);
                    s
                })
                .collect();
            out.push(Block {
                stmts,
                term: remap_terminator(term, &map),
                term_at,
                origin: b.origin,
            });
        }
        if unsealed {
            self.fail(self.f.span, "a reachable block with no terminator");
        }
        out
    }

    // --- regions ---------------------------------------------------------------------------

    /// A region: its statements, then its tail into `dst` if it has one, then the drop of what
    /// it bound.
    fn block_into(&mut self, b: &cir::Block, dst: Option<LocalId>) {
        let depth = self.stack.len();
        for s in &b.stmts {
            if self.cur.is_none() {
                break;
            }
            self.stmt(s);
        }
        if let Some(tail) = &b.tail {
            if self.cur.is_some() {
                self.expr_into(tail, dst);
            }
        }
        self.pop_to(depth, b.at);
    }

    fn stmt(&mut self, s: &cir::Stmt) {
        match s {
            cir::Stmt::Let { local, value } => {
                let l = LocalId(local.0);
                self.expr_into(value, Some(l));
                self.own(l);
            }
            cir::Stmt::Assign {
                local,
                path,
                value,
                at,
            } => self.assign_stmt(LocalId(local.0), path, value, *at),
            cir::Stmt::Eval(e) => match e.ty {
                Some(t) => {
                    let t = self.ty(t, e.at);
                    let tmp = self.temp(t, e.at, "discard");
                    self.expr_into(e, Some(tmp));
                    if self.cur.is_some() && self.managed(t) {
                        self.push(StatementKind::Drop(tmp), e.at);
                    }
                }
                None => self.expr_into(e, None),
            },
            cir::Stmt::If {
                cond,
                then,
                otherwise,
                at,
            } => self.statement_if(cond, then, otherwise.as_ref(), *at),
            cir::Stmt::Match(m) => self.lower_match(m, None),
            cir::Stmt::Loop { id, cond, body, at } => self.lower_loop(*id, cond, body, *at),
            cir::Stmt::Block(b) => self.block_into(b, None),
            cir::Stmt::Scope { body, at } => self.lower_scope(body, *at),
            cir::Stmt::Spawn {
                callee,
                args,
                at,
                callee_at,
            } => self.lower_spawn(*callee, args, *at, *callee_at),
            cir::Stmt::Return { value, at } => {
                let tmp = self.temp(self.ret, *at, "return");
                self.expr_into(value, Some(tmp));
                if self.cur.is_some() {
                    self.cleanup_to(0, *at);
                    self.seal(Terminator::Return(tmp), *at);
                }
            }
            cir::Stmt::Break { target, at } => self.transfer(*target, true, *at),
            cir::Stmt::Continue { target, at } => self.transfer(*target, false, *at),
        }
    }

    fn transfer(&mut self, target: cir::LoopId, leave: bool, at: Span) {
        let Some(frame) = self.loops.iter().rev().find(|l| l.id == target) else {
            self.fail(at, "a transfer to a loop that does not enclose it");
            self.cur = None;
            return;
        };
        let (depth, to) = (frame.depth, if leave { frame.exit } else { frame.head });
        self.cleanup_to(depth, at);
        self.goto(to, at);
    }

    fn assign_stmt(&mut self, local: LocalId, path: &[cir::Step], value: &cir::Expr, at: Span) {
        let root = self.local_ty(local);
        if path.is_empty() {
            if self.managed(root) {
                // The new value first, then the old one let go, then the move: `a = a` and a
                // right-hand side sharing the old value's storage are both safe.
                let tmp = self.temp(root, at, "assign");
                self.expr_into(value, Some(tmp));
                self.own(tmp);
                self.push(StatementKind::Drop(local), at);
                self.assign(local, Rvalue::Move(tmp), at);
                self.forget(tmp);
            } else {
                self.expr_into(value, Some(local));
            }
            return;
        }
        // A field path, each step on the concrete record it reaches.
        let mut refs = Vec::with_capacity(path.len());
        let mut current = root;
        for step in path {
            let Type::Record(rid) = current else {
                self.fail(step.at, "a field written on something that is not a record");
                return;
            };
            refs.push(FieldRef {
                record: rid,
                field: step.field,
            });
            current = self.record_field_ty(rid, step.field, step.at);
        }
        let tmp = self.temp(current, at, "field");
        self.expr_into(value, Some(tmp));
        self.own(tmp);
        self.push(
            StatementKind::SetField {
                base: local,
                path: refs,
                value: tmp,
            },
            at,
        );
        self.forget(tmp);
    }

    fn record_field_ty(&mut self, rid: RecordId, field: u32, at: Span) -> Type {
        match self
            .res
            .records()
            .get(rid)
            .and_then(|d| d.fields.get(field as usize))
        {
            Some(f) => f.ty,
            None => {
                self.fail(at, "a field its record does not declare");
                Type::Int
            }
        }
    }

    fn payload_ty(&mut self, eid: EnumId, variant: u32, field: u32, at: Span) -> Type {
        match self
            .res
            .enums()
            .get(eid)
            .and_then(|d| d.variants.get(variant as usize))
            .and_then(|v| v.fields.get(field as usize))
        {
            Some(f) => f.ty,
            None => {
                self.fail(at, "a payload field its variant does not declare");
                Type::Int
            }
        }
    }

    fn statement_if(
        &mut self,
        cond: &cir::Expr,
        then: &cir::Block,
        otherwise: Option<&cir::Block>,
        at: Span,
    ) {
        let Some((c, held)) = self.operand(cond, false) else {
            return;
        };
        self.release(held, at);
        let then_b = self.new_block(BlockOrigin::Source);
        let join = self.new_block(BlockOrigin::Source);
        let else_b = match otherwise {
            Some(_) => self.new_block(BlockOrigin::Source),
            None => join,
        };
        self.seal(
            Terminator::Branch {
                cond: c,
                then: then_b,
                otherwise: else_b,
            },
            at,
        );
        let mut arrives = otherwise.is_none();
        self.cur = Some(then_b);
        self.block_into(then, None);
        if self.cur.is_some() {
            arrives = true;
            self.goto(join, at);
        }
        if let Some(o) = otherwise {
            self.cur = Some(else_b);
            self.block_into(o, None);
            if self.cur.is_some() {
                arrives = true;
                self.goto(join, at);
            }
        }
        self.cur = arrives.then_some(join);
        if !arrives {
            // Nothing reaches the continuation; it stays an empty, unreachable block.
            self.seal_orphan(join);
        }
    }

    /// A block created for a join that nothing reached. Nothing jumps to it, so
    /// [`Self::finish_blocks`] drops it; it is sealed only so that no block is left open.
    fn seal_orphan(&mut self, b: BlockId) {
        if let Some(block) = self.blocks.get_mut(b.index()) {
            if block.term.is_none() {
                block.term = Some((Terminator::Unwind, Span::synthetic()));
            }
        }
    }

    fn lower_loop(&mut self, id: cir::LoopId, cond: &cir::Expr, body: &cir::Block, at: Span) {
        let head = self.new_block(BlockOrigin::Source);
        let body_b = self.new_block(BlockOrigin::Source);
        let exit = self.new_block(BlockOrigin::Source);
        self.goto(head, at);
        self.cur = Some(head);
        // The condition is outside the body: a `break` in it names the enclosing loop.
        let Some((c, held)) = self.operand(cond, false) else {
            self.seal_orphan(body_b);
            self.seal_orphan(exit);
            return;
        };
        self.release(held, at);
        self.seal(
            Terminator::Branch {
                cond: c,
                then: body_b,
                otherwise: exit,
            },
            at,
        );
        self.cur = Some(body_b);
        self.stack.push(Entry::Loop(id));
        self.loops.push(LoopFrame {
            id,
            head,
            exit,
            depth: self.stack.len(),
        });
        self.block_into(body, None);
        self.loops.pop();
        // The marker.
        self.stack.pop();
        if self.cur.is_some() {
            self.goto(head, at);
        }
        self.cur = Some(exit);
    }

    fn lower_scope(&mut self, body: &cir::Block, at: Span) {
        let list = self.tasks_local(at);
        let unwind = self.unwind();
        self.push(StatementKind::ScopeOpen { list, unwind }, at);
        self.stack.push(Entry::Scope(list));
        self.scopes.push(list);
        let depth = self.stack.len();
        self.block_into(body, None);
        self.pop_to(depth, body.at);
        // The closing brace: the join, then — outside the scope — the failure check.
        self.stack.pop();
        self.scopes.pop();
        if self.cur.is_some() {
            self.push(StatementKind::ScopeJoin { list }, body.at);
            let unwind = self.unwind();
            self.push(StatementKind::CheckFailed { unwind }, body.at);
        }
    }

    fn lower_spawn(&mut self, callee: FnId, args: &[cir::Expr], at: Span, callee_at: Span) {
        let Some(target) = self.callee(callee, &[], callee_at) else {
            return;
        };
        let Some((ops, held)) = self.operands(args) else {
            return;
        };
        let Some(list) = self.scopes.last().copied() else {
            self.fail(at, "a `spawn` outside any `scope`");
            return;
        };
        let unwind = self.unwind();
        self.push(
            StatementKind::Spawn {
                list,
                callee: target,
                args: ops,
                unwind,
                callee_at,
            },
            at,
        );
        self.release_all(held, at);
    }

    fn callee(&mut self, id: FnId, type_args: &[Type], at: Span) -> Option<FuncId> {
        let generic = self
            .res
            .function(id)
            .is_some_and(|d| !d.type_params.is_empty());
        let found = if generic {
            let args: Vec<Type> = type_args.iter().map(|t| self.ty(*t, at)).collect();
            self.map.instance(&InstanceKey { def: id, args })
        } else {
            self.map.plain.get(id.index()).copied().flatten()
        };
        if found.is_none() {
            self.fail(at, "a call to a function with no MIR instance");
        }
        found
    }

    // --- expressions -----------------------------------------------------------------------

    /// Whether evaluating `e` could change a local another operand has already read: only a
    /// region can hold a statement, so only an expression containing one can.
    fn hazardous(e: &cir::Expr) -> bool {
        match &e.kind {
            cir::ExprKind::Const(_)
            | cir::ExprKind::Local(_)
            | cir::ExprKind::Closure { .. }
            | cir::ExprKind::FnValue(_) => false,
            cir::ExprKind::CallIndirect { callee, args, .. } => {
                Self::hazardous(callee) || args.iter().any(Self::hazardous)
            }
            cir::ExprKind::Neg(x) => Self::hazardous(x),
            cir::ExprKind::Arith { lhs, rhs, .. } | cir::ExprKind::Compare { lhs, rhs, .. } => {
                Self::hazardous(lhs) || Self::hazardous(rhs)
            }
            cir::ExprKind::Call { args, .. } | cir::ExprKind::Builtin { args, .. } => {
                args.iter().any(Self::hazardous)
            }
            cir::ExprKind::Record { fields, .. } | cir::ExprKind::Variant { fields, .. } => {
                fields.iter().any(|i| Self::hazardous(&i.value))
            }
            cir::ExprKind::Field { base, .. } => Self::hazardous(base),
            cir::ExprKind::Match(_) | cir::ExprKind::If { .. } | cir::ExprKind::Block(_) => true,
        }
    }

    /// Evaluate `e` to an operand. A local is borrowed where it is — unless `snapshot`, when a
    /// later sibling could reassign it before the operand is used. Anything else goes through a
    /// temporary, which is returned so the caller gives it back once the operand is used.
    fn operand(&mut self, e: &cir::Expr, snapshot: bool) -> Option<(Operand, Option<LocalId>)> {
        match &e.kind {
            cir::ExprKind::Const(c) => {
                let c = self.constant(*c, e.at);
                Some((Operand::Const(c), None))
            }
            cir::ExprKind::Local(l) if !snapshot => Some((Operand::Local(LocalId(l.0)), None)),
            _ => {
                let t = match e.ty {
                    Some(t) => self.ty(t, e.at),
                    None => {
                        self.expr_into(e, None);
                        return None;
                    }
                };
                let tmp = self.temp(t, e.at, "arg");
                self.expr_into(e, Some(tmp));
                self.cur?;
                self.own(tmp);
                Some((Operand::Local(tmp), Some(tmp)))
            }
        }
    }

    /// Operands in written order, each snapshotted if a later one could reassign it.
    fn operands(&mut self, es: &[cir::Expr]) -> Option<(Vec<Operand>, Vec<LocalId>)> {
        let mut later_hazard = vec![false; es.len()];
        let mut any = false;
        for (i, e) in es.iter().enumerate().rev() {
            if let Some(slot) = later_hazard.get_mut(i) {
                *slot = any;
            }
            any = any || Self::hazardous(e);
        }
        let mut ops = Vec::with_capacity(es.len());
        let mut held = Vec::new();
        for (e, hazard) in es.iter().zip(later_hazard) {
            let Some((op, h)) = self.operand(e, hazard) else {
                // Control left part way through: what was already held is on the stack, and
                // the transfer that left dropped it. Forget it here.
                for l in held.into_iter().rev() {
                    self.forget(l);
                }
                return None;
            };
            ops.push(op);
            held.extend(h);
        }
        Some((ops, held))
    }

    /// Give back a temporary an operation only borrowed.
    fn release(&mut self, held: Option<LocalId>, at: Span) {
        if let Some(l) = held {
            if self.cur.is_some() && self.managed(self.local_ty(l)) {
                self.push(StatementKind::Drop(l), at);
            }
            self.forget(l);
        }
    }

    fn release_all(&mut self, held: Vec<LocalId>, at: Span) {
        for l in held.into_iter().rev() {
            self.release(Some(l), at);
        }
    }

    /// Evaluate `e` into `dst`, which is empty. `dst` is `None` only for an expression whose
    /// value nobody reads — one that never produces one.
    fn expr_into(&mut self, e: &cir::Expr, dst: Option<LocalId>) {
        if self.cur.is_none() {
            return;
        }
        let at = e.at;
        match &e.kind {
            cir::ExprKind::Const(c) => {
                let c = self.constant(*c, at);
                self.write(dst, Rvalue::Use(Operand::Const(c)), at);
            }
            cir::ExprKind::Local(l) => {
                let l = LocalId(l.0);
                let rv = if self.managed(self.local_ty(l)) {
                    Rvalue::Copy(l)
                } else {
                    Rvalue::Use(Operand::Local(l))
                };
                self.write(dst, rv, at);
            }
            cir::ExprKind::Neg(x) => {
                let Some((a, h)) = self.operand(x, false) else {
                    return;
                };
                self.write(dst, Rvalue::Neg(a), at);
                self.release(h, at);
            }
            cir::ExprKind::Arith { op, lhs, rhs } => {
                let Some((ops, held)) = self.operands(&[(**lhs).clone(), (**rhs).clone()]) else {
                    return;
                };
                let (a, b) = two(ops);
                self.write(dst, Rvalue::Arith(*op, a, b), at);
                self.release_all(held, at);
            }
            cir::ExprKind::Compare {
                op,
                operand,
                law,
                lhs,
                rhs,
            } => {
                let operand = self.ty(*operand, at);
                let law = if *law == cir::Law::Parametric {
                    cir::Law::of(*op, operand).unwrap_or(cir::Law::Structural)
                } else {
                    *law
                };
                let Some((ops, held)) = self.operands(&[(**lhs).clone(), (**rhs).clone()]) else {
                    return;
                };
                let (a, b) = two(ops);
                self.write(
                    dst,
                    Rvalue::Compare {
                        op: *op,
                        law,
                        operand,
                        lhs: a,
                        rhs: b,
                    },
                    at,
                );
                self.release_all(held, at);
            }
            cir::ExprKind::Call {
                callee,
                type_args,
                args,
                callee_at,
            } => {
                let Some(target) = self.callee(*callee, type_args, *callee_at) else {
                    return;
                };
                let Some((ops, held)) = self.operands(args) else {
                    return;
                };
                self.write(
                    dst,
                    Rvalue::Call {
                        callee: target,
                        args: ops,
                    },
                    *callee_at,
                );
                self.release_all(held, at);
            }
            cir::ExprKind::Builtin {
                op,
                type_args,
                args,
                ..
            } => self.builtin(*op, type_args, args, dst, at),
            cir::ExprKind::Closure { func, captures } => {
                let Some(target) = self.map.plain.get(func.index()).copied().flatten() else {
                    self.fail(at, "a closure with no MIR function");
                    return;
                };
                let captures = captures
                    .iter()
                    .map(|l| Operand::Local(LocalId(l.0)))
                    .collect();
                self.write(
                    dst,
                    Rvalue::Closure {
                        func: target,
                        captures,
                    },
                    at,
                );
            }
            cir::ExprKind::FnValue(g) => {
                let Some(target) = self.map.thunk(self.f.module, *g) else {
                    self.fail(at, "a function value with no thunk");
                    return;
                };
                self.write(
                    dst,
                    Rvalue::Closure {
                        func: target,
                        captures: Vec::new(),
                    },
                    at,
                );
            }
            cir::ExprKind::CallIndirect {
                callee,
                args,
                callee_at,
            } => {
                let mut all = Vec::with_capacity(args.len() + 1);
                all.push((**callee).clone());
                all.extend(args.iter().cloned());
                let Some((mut ops, held)) = self.operands(&all) else {
                    return;
                };
                let callee = ops.remove(0);
                self.write(dst, Rvalue::CallIndirect { callee, args: ops }, *callee_at);
                self.release_all(held, at);
            }
            cir::ExprKind::Record { fields, .. } => {
                let Some(Type::Record(rid)) = e.ty.map(|t| self.ty(t, at)) else {
                    self.fail(at, "a record construction whose type is not a record");
                    return;
                };
                let mut built = Vec::with_capacity(fields.len());
                for init in fields {
                    let fty = self.record_field_ty(rid, init.field, init.at);
                    let tmp = self.temp(fty, init.at, "init");
                    self.expr_into(&init.value, Some(tmp));
                    if self.cur.is_none() {
                        for (_, l) in built.iter().rev() {
                            self.forget(*l);
                        }
                        return;
                    }
                    self.own(tmp);
                    built.push((init.field, tmp));
                }
                for (_, l) in &built {
                    self.forget(*l);
                }
                self.write(
                    dst,
                    Rvalue::Record {
                        record: rid,
                        fields: built,
                    },
                    at,
                );
            }
            cir::ExprKind::Variant {
                variant, fields, ..
            } => {
                let Some(Type::Enum(eid)) = e.ty.map(|t| self.ty(t, at)) else {
                    self.fail(at, "a variant construction whose type is not an enum");
                    return;
                };
                let mut built = Vec::with_capacity(fields.len());
                for init in fields {
                    let fty = self.payload_ty(eid, *variant, init.field, init.at);
                    let tmp = self.temp(fty, init.at, "init");
                    self.expr_into(&init.value, Some(tmp));
                    if self.cur.is_none() {
                        for (_, l) in built.iter().rev() {
                            self.forget(*l);
                        }
                        return;
                    }
                    self.own(tmp);
                    built.push((init.field, tmp));
                }
                for (_, l) in &built {
                    self.forget(*l);
                }
                self.write(
                    dst,
                    Rvalue::Variant {
                        enumeration: eid,
                        variant: *variant,
                        fields: built,
                    },
                    at,
                );
            }
            cir::ExprKind::Field { base, field, .. } => {
                let Some(bt) = base.ty.map(|t| self.ty(t, base.at)) else {
                    self.expr_into(base, None);
                    return;
                };
                let Type::Record(rid) = bt else {
                    self.fail(at, "a field read from something that is not a record");
                    return;
                };
                let Some((b, h)) = self.operand(base, false) else {
                    return;
                };
                self.write(
                    dst,
                    Rvalue::Field {
                        base: b,
                        field: FieldRef {
                            record: rid,
                            field: *field,
                        },
                    },
                    at,
                );
                self.release(h, at);
            }
            cir::ExprKind::Match(m) => self.lower_match(m, dst),
            cir::ExprKind::If {
                cond,
                then,
                otherwise,
            } => self.value_if(cond, then, otherwise, dst, at),
            cir::ExprKind::Block(b) => self.block_into(b, dst),
        }
    }

    /// Write `value` into `dst`. With no destination the value is unread, which Core IR allows
    /// only for an expression that never produces one — so arriving here with none is a defect.
    fn write(&mut self, dst: Option<LocalId>, value: Rvalue, at: Span) {
        match dst {
            Some(d) => self.assign(d, value, at),
            None => self.fail(at, "a value produced where none is wanted"),
        }
    }

    fn builtin(
        &mut self,
        op: Intrinsic,
        type_args: &[Type],
        args: &[cir::Expr],
        dst: Option<LocalId>,
        at: Span,
    ) {
        let Some((ops, held)) = self.operands(args) else {
            return;
        };
        if op == Intrinsic::ExitWith {
            let code = ops
                .into_iter()
                .next()
                .unwrap_or(Operand::Const(Const::Int(0)));
            self.seal(Terminator::Exit(code), at);
            for l in held.into_iter().rev() {
                self.forget(l);
            }
            return;
        }
        let elem = type_args.first().copied().map(|t| self.ty(t, at));
        self.write(
            dst,
            Rvalue::Primitive {
                op,
                elem,
                args: ops,
            },
            at,
        );
        self.release_all(held, at);
    }

    fn value_if(
        &mut self,
        cond: &cir::Expr,
        then: &cir::Block,
        otherwise: &cir::Block,
        dst: Option<LocalId>,
        at: Span,
    ) {
        let Some((c, held)) = self.operand(cond, false) else {
            return;
        };
        self.release(held, at);
        let then_b = self.new_block(BlockOrigin::Source);
        let else_b = self.new_block(BlockOrigin::Source);
        let join = self.new_block(BlockOrigin::Source);
        self.seal(
            Terminator::Branch {
                cond: c,
                then: then_b,
                otherwise: else_b,
            },
            at,
        );
        let mut arrives = false;
        for (b, region) in [(then_b, then), (else_b, otherwise)] {
            self.cur = Some(b);
            self.block_into(region, dst);
            if self.cur.is_some() {
                arrives = true;
                self.goto(join, at);
            }
        }
        if arrives {
            self.cur = Some(join);
        } else {
            self.seal_orphan(join);
        }
    }

    /// A `match`, in value position (`dst`) or as a statement. The scrutinee is evaluated into
    /// a temporary the `match` owns until the arms have produced their value.
    fn lower_match(&mut self, m: &cir::Match, dst: Option<LocalId>) {
        let Some(st) = m.scrutinee.ty.map(|t| self.ty(t, m.scrutinee.at)) else {
            self.expr_into(&m.scrutinee, None);
            return;
        };
        let Type::Enum(eid) = st else {
            self.fail(m.at, "a `match` on something that is not an enum");
            return;
        };
        let name = match m.origin {
            cir::MatchOrigin::Written => "match",
            cir::MatchOrigin::Propagate => "propagate",
        };
        let s = self.temp(st, m.scrutinee.at, name);
        self.expr_into(&m.scrutinee, Some(s));
        if self.cur.is_none() {
            return;
        }
        self.own(s);
        let arms: Vec<(u32, BlockId)> = m
            .arms
            .iter()
            .map(|a| (a.variant, self.new_block(BlockOrigin::Source)))
            .collect();
        let join = self.new_block(BlockOrigin::Source);
        self.seal(
            Terminator::Switch {
                scrutinee: s,
                enumeration: eid,
                arms: arms.clone(),
            },
            m.at,
        );
        let mut arrives = false;
        for (arm, (_, b)) in m.arms.iter().zip(&arms) {
            self.cur = Some(*b);
            let depth = self.stack.len();
            for binding in &arm.bindings {
                let l = LocalId(binding.local.0);
                self.assign(
                    l,
                    Rvalue::Payload {
                        base: s,
                        enumeration: eid,
                        variant: arm.variant,
                        field: binding.field,
                    },
                    binding.at,
                );
                self.own(l);
            }
            self.block_into(&arm.body, dst);
            self.pop_to(depth, arm.at);
            if self.cur.is_some() {
                arrives = true;
                self.goto(join, arm.at);
            }
        }
        if arrives {
            self.cur = Some(join);
            // After the arms have established their value: let go of what was matched.
            if let Some(pos) = self.stack.iter().rposition(|e| *e == Entry::Owned(s)) {
                self.stack.truncate(pos);
                self.push(StatementKind::Drop(s), m.at);
            }
        } else {
            self.forget(s);
            self.seal_orphan(join);
        }
    }
}

fn remap_statement(k: StatementKind, map: &impl Fn(BlockId) -> BlockId) -> StatementKind {
    match k {
        StatementKind::Assign { dst, value, unwind } => StatementKind::Assign {
            dst,
            value,
            unwind: unwind.map(map),
        },
        StatementKind::ScopeOpen { list, unwind } => StatementKind::ScopeOpen {
            list,
            unwind: map(unwind),
        },
        StatementKind::Spawn {
            list,
            callee,
            args,
            unwind,
            callee_at,
        } => StatementKind::Spawn {
            list,
            callee,
            args,
            unwind: map(unwind),
            callee_at,
        },
        StatementKind::CheckFailed { unwind } => StatementKind::CheckFailed {
            unwind: map(unwind),
        },
        StatementKind::StackCheck { unwind } => StatementKind::StackCheck {
            unwind: map(unwind),
        },
        other => other,
    }
}

fn remap_terminator(t: Terminator, map: &impl Fn(BlockId) -> BlockId) -> Terminator {
    match t {
        Terminator::Goto(b) => Terminator::Goto(map(b)),
        Terminator::Branch {
            cond,
            then,
            otherwise,
        } => Terminator::Branch {
            cond,
            then: map(then),
            otherwise: map(otherwise),
        },
        Terminator::Switch {
            scrutinee,
            enumeration,
            arms,
        } => Terminator::Switch {
            scrutinee,
            enumeration,
            arms: arms.into_iter().map(|(v, b)| (v, map(b))).collect(),
        },
        other => other,
    }
}

/// Whether a value of `t` owns anything, by the classification [`Types::managed`] makes — the
/// same answer, from the checker's table, before MIR's own exists.
fn managed_type(res: &Resolution, t: Type, depth: usize) -> bool {
    if depth > nazm_sema::MAX_TYPE_DEPTH * 4 {
        return true;
    }
    match t {
        Type::Str
        | Type::Ints
        | Type::Strs
        | Type::Chan
        | Type::ChanOf(_)
        | Type::Vec(_)
        | Type::Fn(_) => true,
        Type::Int | Type::Bool | Type::Cap(_) | Type::Param(_) => false,
        Type::Record(id) => res
            .records()
            .get(id)
            .is_some_and(|r| r.fields.iter().any(|f| managed_type(res, f.ty, depth + 1))),
        Type::Enum(id) => res.enums().get(id).is_some_and(|e| {
            e.variants
                .iter()
                .any(|v| v.fields.iter().any(|f| managed_type(res, f.ty, depth + 1)))
        }),
    }
}

fn two(ops: Vec<Operand>) -> (Operand, Operand) {
    let mut it = ops.into_iter();
    let a = it.next().unwrap_or(Operand::Const(Const::Int(0)));
    let b = it.next().unwrap_or(Operand::Const(Const::Int(0)));
    (a, b)
}

// --- the type table --------------------------------------------------------------------------

fn collect_types(res: &Resolution, graph: &ModuleGraph, functions: &[Function]) -> Types {
    let mut work: Vec<Type> = Vec::new();
    for f in functions {
        work.push(f.ret);
        for l in &f.locals {
            if let Some(t) = l.ty.value() {
                work.push(t);
            }
        }
        for b in &f.blocks {
            for s in &b.stmts {
                if let StatementKind::Assign { value, .. } = &s.kind {
                    match value {
                        Rvalue::Record { record, .. } => work.push(Type::Record(*record)),
                        Rvalue::Variant { enumeration, .. }
                        | Rvalue::Payload { enumeration, .. } => {
                            work.push(Type::Enum(*enumeration));
                        }
                        Rvalue::Field { field, .. } => work.push(Type::Record(field.record)),
                        Rvalue::Primitive { elem: Some(t), .. } => {
                            work.push(*t);
                            if let Some(v) = res.types().find_vec(*t) {
                                work.push(v);
                            }
                            if let Some(c) = res.types().find_chan(*t) {
                                work.push(c);
                            }
                        }
                        Rvalue::Compare { operand, .. } => work.push(*operand),
                        Rvalue::Closure { func, .. } => {
                            if let Some(g) = functions.get(func.index()) {
                                work.extend(g.closure.iter().flatten().copied());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let mut types = Types::default();
    while let Some(t) = work.pop() {
        match t {
            Type::Record(id) => {
                let key = u32::try_from(id.index()).unwrap_or(u32::MAX);
                if types.records.contains_key(&key) {
                    continue;
                }
                let Some(def) = res.records().get(id) else {
                    continue;
                };
                let fields: Vec<FieldInfo> = def
                    .fields
                    .iter()
                    .map(|f| FieldInfo {
                        name: f.name.clone(),
                        ty: f.ty,
                    })
                    .collect();
                work.extend(fields.iter().map(|f| f.ty));
                types.records.insert(
                    key,
                    RecordInfo {
                        source: res.types().show(t),
                        identity: type_identity(res, graph, t),
                        key: type_key(t, res.types(), graph),
                        fields,
                        canonical: def.layout.clone(),
                    },
                );
            }
            Type::Enum(id) => {
                let key = u32::try_from(id.index()).unwrap_or(u32::MAX);
                if types.enums.contains_key(&key) {
                    continue;
                }
                let Some(def) = res.enums().get(id) else {
                    continue;
                };
                let variants: Vec<VariantInfo> = def
                    .variants
                    .iter()
                    .map(|v| VariantInfo {
                        name: v.name.clone(),
                        fields: v
                            .fields
                            .iter()
                            .map(|f| FieldInfo {
                                name: f.name.clone(),
                                ty: f.ty,
                            })
                            .collect(),
                        canonical: v.layout.clone(),
                    })
                    .collect();
                for v in &variants {
                    work.extend(v.fields.iter().map(|f| f.ty));
                }
                types.enums.insert(
                    key,
                    EnumInfo {
                        source: res.types().show(t),
                        identity: type_identity(res, graph, t),
                        key: type_key(t, res.types(), graph),
                        variants,
                        canonical: def.order.clone(),
                    },
                );
            }
            Type::Vec(v) => {
                let key = u32::try_from(v.index()).unwrap_or(u32::MAX);
                if types.vecs.contains_key(&key) {
                    continue;
                }
                let elem = res.types().vec_element(v).unwrap_or(Type::Int);
                work.push(elem);
                types.vecs.insert(
                    key,
                    VecInfo {
                        elem,
                        elem_key: type_key(elem, res.types(), graph),
                        source: res.types().show(t),
                    },
                );
            }
            Type::ChanOf(c) => {
                let key = u32::try_from(c.index()).unwrap_or(u32::MAX);
                if types.chans.contains_key(&key) {
                    continue;
                }
                let elem = res.types().chan_element(c).unwrap_or(Type::Int);
                work.push(elem);
                types.chans.insert(
                    key,
                    VecInfo {
                        elem,
                        elem_key: type_key(elem, res.types(), graph),
                        source: res.types().show(t),
                    },
                );
            }
            Type::Fn(f) => {
                let key = u32::try_from(f.index()).unwrap_or(u32::MAX);
                if types.fns.contains_key(&key) {
                    continue;
                }
                let Some(sig) = res.types().fn_signature(f).cloned() else {
                    continue;
                };
                work.extend(sig.params.iter().copied());
                work.push(sig.ret);
                types.fns.insert(
                    key,
                    FnInfo {
                        params: sig.params,
                        ret: sig.ret,
                        source: res.types().show(t),
                    },
                );
            }
            _ => {}
        }
    }
    types
}

fn type_identity(res: &Resolution, graph: &ModuleGraph, t: Type) -> TypeIdentity {
    let definition = res.types().definition_of(t);
    let (kind, module, name) = match definition {
        Some(TypeNode::Record(id)) => (
            DefKind::Record,
            res.records().get(id).map(|d| d.module),
            res.records().get(id).map(|d| d.name.clone()),
        ),
        Some(TypeNode::Enum(id)) => (
            DefKind::Enum,
            res.enums().get(id).map(|d| d.module),
            res.enums().get(id).map(|d| d.name.clone()),
        ),
        None => (DefKind::Record, None, None),
    };
    let owner = match (module, name) {
        (Some(m), Some(n)) => owner(graph, m, kind, &n),
        _ => crate::ir::Owner::Unkeyed {
            module: nazm_sema::module::ModuleId::ROOT,
            name: "?".to_owned(),
        },
    };
    let args = res
        .types()
        .type_args(t)
        .iter()
        .map(|a| type_key(*a, res.types(), graph))
        .collect();
    TypeIdentity { owner, args }
}
