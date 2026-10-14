//! A checked program to Core IR — the one lowering (`docs/architecture.md` §7.41).
//!
//! # Only after checking
//!
//! Private to this crate and reached only through [`crate::on_lowered_map`] and the run
//! pipeline, both of which call it only when parsing and checking produced no diagnostic. So
//! every question it asks the resolution has an answer: a missing one is the checker and this
//! file disagreeing, which is a compiler defect ([`codes::COMPILER_DEFECT`]) and stops the
//! compilation. There is no fallback to another lowering, because there is no other.
//!
//! # What it reads, and what it decides
//!
//! Every identity is **read** from the resolution the checker produced: which slot a name is,
//! which function or built-in a call is, which field, variant and payload field an
//! initialiser, projection or pattern names, what a `?` takes apart and builds. Every type is
//! read too — a binding's, a checked call's result, a field's or payload's declared type —
//! and only composed here: an `if`'s is its branches', a block's its tail's.
//!
//! What it decides is exactly what used to be decided twice, once by the interpreter walking
//! the tree and once by `nazm-lir` lowering it: where a value is wanted and where it is not,
//! how a statement sequence finishes, which loop a `break` leaves, and what `?` means. Those
//! rules are the checker's own (`check_block_inner`, `check_while`, `check_expr`), stated
//! once more here and nowhere after.
//!
//! # Completion, as the checker has it
//!
//! A `return`, `break` or `continue` transfers; a statement `if` transfers only with an `else`
//! and both branches transferring; a statement `match` when every arm does; a `scope` or block
//! as its body does. A statement after a transfer is `N0313`, an error, so none reaches here —
//! and a region stops at its first transfer rather than lowering text that cannot run.

use std::collections::HashMap;

use nazm_cir as cir;
use nazm_diag::{codes, Diagnostic};
use nazm_sema::resolve::{Callee, FnDef, FnId, Resolution};
use nazm_sema::{ConvertMode, NumKind, NumericFact, Type};
use nazm_span::Span;
use nazm_syntax::{
    BinOp, Block as AstBlock, Expr as AstExpr, Function as AstFn, MatchArm as AstArm, Program,
    Stmt as AstStmt,
};

use cir::{
    Arm, Binding, Block, Const, ConstId, Ends, Expr, ExprKind, Init, Law, Local, LocalId,
    LocalOrigin, LoopId, Match, MatchOrigin, Step, Stmt, Temp,
};

/// Where the lowering and the checker disagreed. Never a user's error.
struct Defect {
    at: Span,
    what: String,
}

type Lowered<T> = Result<T, Defect>;

fn defect<T>(at: Span, what: impl Into<String>) -> Lowered<T> {
    Err(Defect {
        at,
        what: what.into(),
    })
}

fn report(d: Defect) -> Diagnostic {
    Diagnostic::error(
        codes::COMPILER_DEFECT,
        d.at,
        format!("the compiler could not lower checked code to Core IR: {}", d.what),
    )
    .help("this is a bug in the compiler, not in the program: it checked, and the lowering disagreed with the checker")
}

/// Lower every function of a checked program.
///
/// # Errors
///
/// A defect for each function the lowering and the checker disagree about, and one for each
/// invariant [`cir::verify`] finds broken. Either means the compiler is wrong.
pub(crate) fn lower(program: &Program, res: &Resolution) -> Result<cir::Program, Vec<Diagnostic>> {
    let mut functions = Vec::with_capacity(res.functions().len());
    let mut errors = Vec::new();
    for (i, def) in res.functions().iter().enumerate() {
        let id = FnId::from_index(i);
        let lowered = program.functions.get(def.decl as usize).map_or_else(
            || defect(def.name_span, format!("`{}` has no declaration", def.name)),
            |f| match &def.closure {
                // A closure's declaration is its maker's; its syntax is found by its span.
                Some(info) => match f.body.as_ref().and_then(|b| b.closure_at(info.at)) {
                    Some(c) => Lower::new(res, id, def).closure(c, info),
                    None => defect(info.at, "a closure whose expression cannot be found"),
                },
                None => Lower::new(res, id, def).function(f),
            },
        );
        match lowered {
            Ok(f) => functions.push(f),
            Err(d) => errors.push(report(d)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    // The impl table (N78): each impl that is the coherence table's answer, with every
    // method defined or defaulted. One the checker refused is not here, and nothing calls through it.
    let impls = res
        .traits()
        .impls()
        .filter(|(id, d)| res.traits().find(d.trait_id, d.self_ty) == Some(*id))
        .filter_map(|(_, d)| {
            // A method the impl leaves to its trait's default body (R1b, L11) is that body: the
            // interpreter calls it at the receiver's type, which needs no type argument.
            let methods: Option<Vec<FnId>> = (0..d.methods.len())
                .map(|m| {
                    let m = u32::try_from(m).unwrap_or(u32::MAX);
                    res.method_target(d.trait_id, d.self_ty, m).map(|(f, _)| f)
                })
                .collect();
            methods.map(|methods| cir::ImplRow {
                trait_id: d.trait_id,
                self_ty: d.self_ty,
                methods,
            })
        })
        .collect();
    let orders = cir::Orders {
        records: (0..res.records().len())
            .map(|i| {
                res.records()
                    .get(nazm_sema::RecordId::from_index(i))
                    .map(|d| d.layout.clone())
                    .unwrap_or_default()
            })
            .collect(),
        enums: (0..res.enums().len())
            .map(|i| {
                res.enums()
                    .get(nazm_sema::EnumId::from_index(i))
                    .map(|d| cir::EnumOrder {
                        variants: d.order.clone(),
                        payloads: d.variants.iter().map(|v| v.layout.clone()).collect(),
                        names: d.variants.iter().map(|v| v.name.clone()).collect(),
                    })
                    .unwrap_or_default()
            })
            .collect(),
    };
    let derived_hash = res.traits().derived_hash().and_then(|t| {
        let d = res.traits().get(t)?;
        Some(cir::DerivedHash {
            trait_id: t,
            hash: d.method("hash")?,
            same: d.method("same")?,
        })
    });
    // Every generic instance's definition (L11b), for the impl table's fallback.
    let mut templates: Vec<(Type, Type)> = (0..res.records().len())
        .filter_map(|i| {
            let id = nazm_sema::RecordId::from_index(i);
            let inst = res.records().get(id)?.instance.as_ref()?;
            Some((Type::Record(id), Type::Record(inst.template)))
        })
        .chain((0..res.enums().len()).filter_map(|i| {
            let id = nazm_sema::EnumId::from_index(i);
            let inst = res.enums().get(id)?.instance.as_ref()?;
            Some((Type::Enum(id), Type::Enum(inst.template)))
        }))
        .collect();
    templates.sort();
    let core = cir::Program {
        functions,
        entry: res.entry(),
        impls,
        templates,
        statics: res.statics().to_vec(),
        orders,
        derived_hash,
    };
    let problems = cir::verify(&core, res);
    if problems.is_empty() {
        Ok(core)
    } else {
        Err(problems
            .into_iter()
            .map(|p| {
                report(Defect {
                    at: Span::synthetic(),
                    what: format!("an invariant does not hold — {p}"),
                })
            })
            .collect())
    }
}

/// One function's lowering.
struct Lower<'a> {
    res: &'a Resolution,
    /// The definition being lowered — by position in the resolution, never by its name.
    id: FnId,
    def: &'a FnDef,
    locals: Vec<Local>,
    consts: Vec<Const>,
    by_value: HashMap<Const, ConstId>,
    loops: u32,
    /// The loops whose **bodies** enclose the point being lowered, innermost last. A loop's
    /// condition is outside it: raised around the body alone, as the checker raises
    /// `loop_depth`.
    targets: Vec<LoopId>,
    /// For each open region, the locals bound directly in it.
    owned: Vec<Vec<LocalId>>,
    /// How far the resolution's slots are from Core IR's locals: 1 in a closure, whose
    /// local 0 is the closure itself (N50), and 0 everywhere else.
    shift: u32,
    /// The function's `ensures` clauses (N87), checked wherever it returns.
    ensures: Vec<(AstExpr, Span)>,
    /// The slot its clauses read as `result`, and the local a return site holds it in.
    result_slot: Option<u32>,
    result_here: Option<LocalId>,
    /// A shared binding's field assignment in progress (R1b, L10): its cell, the copy being
    /// changed, and the read that fills the copy.
    pending_cell: Option<(LocalId, LocalId, Expr)>,
    /// The `defer` bodies registered in the blocks being lowered (R1b, L12), innermost last,
    /// each with how many loops enclosed it: emitted, last first, wherever their block is left.
    defers: Vec<(AstBlock, usize)>,
}

impl<'a> Lower<'a> {
    fn new(res: &'a Resolution, id: FnId, def: &'a FnDef) -> Self {
        let params = def.params.len();
        // A closure's body is handed the closure first (N50): what its captures are read from.
        let env = def.closure.as_ref().map(|info| Local {
            ty: info.ty,
            name: "env".to_owned(),
            origin: LocalOrigin::Param,
            at: info.at,
        });
        let shift = u32::from(env.is_some());
        let locals = env
            .into_iter()
            .chain(def.locals.iter().enumerate().map(|(i, l)| {
                Local {
                    // A `let mut` a closure shares lives in a cell (R1b, L10).
                    ty: u32::try_from(i)
                        .ok()
                        .and_then(|s| res.shared(id, s))
                        .unwrap_or(l.ty),
                    name: l.name.clone(),
                    origin: if i < params {
                        LocalOrigin::Param
                    } else {
                        LocalOrigin::Binding { mutable: l.mutable }
                    },
                    at: l.span,
                }
            }))
            .collect();
        Self {
            res,
            id,
            def,
            locals,
            consts: Vec::new(),
            by_value: HashMap::new(),
            loops: 0,
            targets: Vec::new(),
            owned: Vec::new(),
            shift,
            ensures: Vec::new(),
            result_slot: None,
            result_here: None,
            pending_cell: None,
            defers: Vec::new(),
        }
    }

    /// A closure's body, as a function of its own (N50): the closure, then its parameters,
    /// then its captures — each filled from the closure as a call begins, and owned by the
    /// body like any binding.
    fn closure(mut self, e: &AstExpr, info: &nazm_sema::ClosureInfo) -> Lowered<cir::Function> {
        let AstExpr::Closure {
            body,
            return_type,
            fn_span,
            span,
            ..
        } = e
        else {
            return defect(info.at, "a closure that is not a closure expression");
        };
        let Some(required) = self.def.inferred_effects else {
            return defect(*fn_span, "a closure with no inferred effects");
        };
        let captures: Vec<LocalId> = info
            .captures
            .iter()
            .map(|c| LocalId(c.inner + self.shift))
            .collect();
        let body = self.value_block(body, captures.clone())?;
        let def = self.def;
        // A closure in a generic function is generic over that function's parameters (L10).
        let type_params = self
            .res
            .type_owner(self.id)
            .and_then(|g| self.res.function(g))
            .map_or(0, |g| {
                u32::try_from(g.type_params.len()).unwrap_or(u32::MAX)
            });
        Ok(cir::Function {
            def: self.id,
            module: def.module,
            name: def.name.clone(),
            type_params,
            params: u32::try_from(def.params.len() + 1).unwrap_or(u32::MAX),
            locals: self.locals,
            ret: def.ret,
            consts: self.consts,
            loops: self.loops,
            effects: cir::Effects {
                declared: def.declared_effects,
                required,
            },
            body,
            span: *span,
            name_span: *fn_span,
            ret_span: return_type.span,
            foreign: None,
            section: None,
            interrupt: None,
            on_failure: false,
            task: None,
            export: None,
            captures,
            closure: true,
        })
    }

    fn function(mut self, f: &AstFn) -> Lowered<cir::Function> {
        if let Some(symbol) = &self.def.foreign {
            let def = self.def;
            // The signature C has (N85): a handle, a callback, a returned string and a
            // nullable result are each a pointer-sized `Int` here, and the call site makes
            // them what the declaration says.
            let (params, ret) = raw_foreign(self.res, def);
            for (l, t) in self.locals.iter_mut().zip(params) {
                l.ty = t;
            }
            return Ok(cir::Function {
                def: self.id,
                module: def.module,
                name: def.name.clone(),
                type_params: 0,
                params: u32::try_from(def.params.len()).unwrap_or(u32::MAX),
                locals: self.locals,
                ret,
                consts: Vec::new(),
                loops: 0,
                effects: cir::Effects {
                    declared: def.declared_effects,
                    required: def.inferred_effects.unwrap_or_default(),
                },
                body: cir::Block {
                    stmts: Vec::new(),
                    tail: None,
                    ends: cir::Ends::Transfers,
                    owns: Vec::new(),
                    at: f.span,
                },
                span: f.span,
                name_span: f.name_span,
                ret_span: f.return_type.span,
                foreign: Some(symbol.clone()),
                section: None,
                interrupt: None,
                on_failure: false,
                task: None,
                export: None,
                captures: Vec::new(),
                closure: false,
            });
        }
        let Some(body) = &f.body else {
            return defect(f.name_span, format!("`{}` has no body", f.name));
        };
        let Some(required) = self.def.inferred_effects else {
            return defect(f.name_span, format!("`{}` has no inferred effects", f.name));
        };
        // Contracts (N87, §7.88): every `requires` first, then the body; every `ensures` where
        // it returns — its tail, here, and each `return` and `?`, by `leave`.
        self.ensures = f
            .contracts
            .iter()
            .filter(|c| c.kind == nazm_syntax::ContractKind::Ensures)
            .map(|c| (c.expr.clone(), c.span))
            .collect();
        self.result_slot = self.res.contract_result(self.id);
        let mut checks = Vec::new();
        for c in f
            .contracts
            .iter()
            .filter(|c| c.kind == nazm_syntax::ContractKind::Requires)
        {
            let cond = self.expr(&c.expr)?;
            let check = self.contract_check(cond, 0, c.span);
            checks.push(Stmt::Eval(check));
        }
        let mut body = self.value_block(body, Vec::new())?;
        if !checks.is_empty() {
            checks.append(&mut body.stmts);
            body.stmts = checks;
        }
        if let Some(tail) = body.tail.take() {
            body.tail = Some(Box::new(self.ensured(*tail)?));
        }
        let def = self.def;
        Ok(cir::Function {
            def: self.id,
            module: def.module,
            name: def.name.clone(),
            type_params: u32::try_from(def.type_params.len()).unwrap_or(u32::MAX),
            params: u32::try_from(def.params.len()).unwrap_or(u32::MAX),
            locals: self.locals,
            ret: def.ret,
            consts: self.consts,
            loops: self.loops,
            effects: cir::Effects {
                declared: def.declared_effects,
                required,
            },
            body,
            span: f.span,
            name_span: f.name_span,
            ret_span: f.return_type.span,
            foreign: None,
            section: crate::constants::section_of(&f.attrs),
            interrupt: crate::constants::interrupt_of(&f.attrs),
            on_failure: f.attrs.iter().any(|a| a.name == "on_failure"),
            task: crate::constants::task_of(&f.attrs),
            export: def.export.clone(),
            captures: Vec::new(),
            closure: false,
        })
    }

    // --- identities --------------------------------------------------------------------

    fn local_at(&self, span: Span) -> Lowered<LocalId> {
        // `result` in an `ensures` clause is the value this return site holds (N87).
        if let (Some(slot), Some(here)) = (self.result_slot, self.result_here) {
            if self.res.var(span) == Some(slot) {
                return Ok(here);
            }
        }
        match self.res.var(span) {
            Some(slot) if ((slot + self.shift) as usize) < self.locals.len() => {
                Ok(LocalId(slot + self.shift))
            }
            _ => defect(span, "a name the checker bound to no slot"),
        }
    }

    /// Whether `value` is `str_concat(x, t)` with `x` the binding `local` (R1b, D2): an assignment
    /// of it to `local` appends to `local`'s own string.
    fn appends_to(&self, local: LocalId, value: &AstExpr) -> bool {
        let AstExpr::Call {
            callee_span, args, ..
        } = value
        else {
            return false;
        };
        if self.res.callee(*callee_span) != Some(Callee::Builtin(nazm_sema::Intrinsic::StrConcat)) {
            return false;
        }
        matches!(args.first(), Some(AstExpr::Var { span, .. })
            if self.res.var(*span).is_some() && self.local_at(*span).ok() == Some(local))
    }

    /// The type a shared binding holds, if local `id` is one a closure shares and so a cell
    /// (R1b, L10); `None` for every other local.
    fn shared(&self, id: LocalId) -> Option<Type> {
        let slot = id.0.checked_sub(self.shift)?;
        self.res.shared(self.id, slot)?;
        self.def.locals.get(slot as usize).map(|l| l.ty)
    }

    /// `vec_get(cell, 0)` or `vec_set(cell, 0, value)` on shared local `cell` holding `elem`.
    fn through_cell(&mut self, cell: LocalId, elem: Type, value: Option<Expr>, at: Span) -> Expr {
        let ty = self.locals.get(cell.index()).map_or(elem, |l| l.ty);
        let zero = Expr {
            kind: ExprKind::Const(self.constant(Const::Int(0))),
            ty: Some(Type::Int),
            at,
        };
        let read = Expr {
            kind: ExprKind::Local(cell),
            ty: Some(ty),
            at,
        };
        let (op, result) = match value {
            Some(_) => (nazm_sema::Intrinsic::VecSet, Type::Int),
            None => (nazm_sema::Intrinsic::VecGet, elem),
        };
        Expr {
            kind: ExprKind::Builtin {
                op,
                type_args: vec![elem],
                args: [read, zero].into_iter().chain(value).collect(),
                callee_at: at,
            },
            ty: Some(result),
            at,
        }
    }

    fn local_ty(&self, id: LocalId) -> Lowered<Type> {
        match self.locals.get(id.index()) {
            Some(l) => Ok(l.ty),
            None => defect(Span::synthetic(), format!("local {} does not exist", id.0)),
        }
    }

    fn temp(&mut self, t: Temp, ty: Type, at: Span) -> LocalId {
        let id = LocalId(u32::try_from(self.locals.len()).unwrap_or(u32::MAX));
        self.locals.push(Local {
            ty,
            name: t.name().to_owned(),
            origin: LocalOrigin::Temp(t),
            at,
        });
        id
    }

    fn constant(&mut self, c: Const) -> ConstId {
        if let Some(id) = self.by_value.get(&c) {
            return *id;
        }
        let id = ConstId(u32::try_from(self.consts.len()).unwrap_or(u32::MAX));
        self.consts.push(c.clone());
        self.by_value.insert(c, id);
        id
    }

    fn open(&mut self, seed: Vec<LocalId>) {
        self.owned.push(seed);
    }

    fn close(&mut self) -> Vec<LocalId> {
        self.owned.pop().unwrap_or_default()
    }

    fn bind(&mut self, l: LocalId) {
        if let Some(scope) = self.owned.last_mut() {
            scope.push(l);
        }
    }

    // --- regions -----------------------------------------------------------------------

    /// A region whose value is wanted: statements, then a tail — unless the statements
    /// transfer, in which case no tail is lowered because none can run.
    fn value_block(&mut self, b: &AstBlock, seed: Vec<LocalId>) -> Lowered<Block> {
        self.open(seed);
        let mark = self.defers.len();
        let lowered = self
            .statements(&b.statements)
            .and_then(|(mut stmts, ends)| {
                let tail = match (ends, &b.tail) {
                    (Ends::Transfers, _) => None,
                    (Ends::FallsThrough, Some(t)) => Some(Box::new(self.expr(t)?)),
                    (Ends::FallsThrough, None) => {
                        return defect(
                            b.span,
                            "a block whose value is wanted has no final expression",
                        )
                    }
                };
                // What this block deferred runs after its value is computed (L12).
                let tail = match tail {
                    Some(t) if self.defers.len() > mark => {
                        let ty = t.ty.unwrap_or(Type::Int);
                        let held = self.temp(Temp::Result, ty, b.span);
                        stmts.push(Stmt::Let {
                            local: held,
                            value: *t,
                        });
                        stmts.extend(self.deferred_since(mark)?);
                        Some(Box::new(Expr {
                            kind: ExprKind::Local(held),
                            ty: Some(ty),
                            at: b.span,
                        }))
                    }
                    other => other,
                };
                Ok((stmts, ends, tail))
            });
        self.defers.truncate(mark);
        let owns = self.close();
        let (stmts, ends, tail) = lowered?;
        Ok(Block {
            stmts,
            tail,
            ends,
            owns,
            at: b.span,
        })
    }

    /// A region whose value is not wanted. Its tail, if written, is a statement: an `if`, a
    /// `match` or a block in statement form, or anything else evaluated and discarded.
    fn statement_block(&mut self, b: &AstBlock, seed: Vec<LocalId>) -> Lowered<Block> {
        self.open(seed);
        let mark = self.defers.len();
        let lowered = self
            .statements(&b.statements)
            .and_then(|(mut stmts, ends)| match (ends, &b.tail) {
                (Ends::FallsThrough, Some(t)) => {
                    let s = self.effect(t)?;
                    let ends = s.ends();
                    stmts.push(s);
                    Ok((stmts, ends))
                }
                _ => Ok((stmts, ends)),
            })
            // What this block deferred runs as it falls through (L12).
            .and_then(|(mut stmts, ends)| {
                if ends == Ends::FallsThrough {
                    stmts.extend(self.deferred_since(mark)?);
                }
                Ok((stmts, ends))
            });
        self.defers.truncate(mark);
        let owns = self.close();
        let (stmts, ends) = lowered?;
        Ok(Block {
            stmts,
            tail: None,
            ends,
            owns,
            at: b.span,
        })
    }

    /// A region holding one expression: an arm written without braces.
    fn expr_region(&mut self, e: &AstExpr, seed: Vec<LocalId>, value: bool) -> Lowered<Block> {
        self.open(seed);
        let lowered = if value {
            self.expr(e)
                .map(|x| (Vec::new(), Some(Box::new(x)), Ends::FallsThrough))
        } else {
            self.effect(e).map(|s| {
                let ends = s.ends();
                (vec![s], None, ends)
            })
        };
        let owns = self.close();
        let (stmts, tail, ends) = lowered?;
        Ok(Block {
            stmts,
            tail,
            ends,
            owns,
            at: e.span(),
        })
    }

    fn statements(&mut self, statements: &[AstStmt]) -> Lowered<(Vec<Stmt>, Ends)> {
        let mut out = Vec::with_capacity(statements.len());
        for s in statements {
            let lowered = self.stmt(s)?;
            let ends = lowered.ends();
            out.push(lowered);
            if ends == Ends::Transfers {
                return Ok((out, Ends::Transfers));
            }
        }
        Ok((out, Ends::FallsThrough))
    }

    fn target(&self, word: &str, at: Span) -> Lowered<LoopId> {
        match self.targets.last() {
            Some(t) => Ok(*t),
            None => defect(at, format!("a `{word}` with no enclosing loop body")),
        }
    }

    fn stmt(&mut self, s: &AstStmt) -> Lowered<Stmt> {
        Ok(match s {
            AstStmt::Let(l) => {
                // The value first: `let x = x;` reads the outer `x`, as the checker says.
                let mut value = self.expr(&l.value)?;
                let local = self.local_at(l.name_span)?;
                // Shared with a closure (R1b, L10): a cell made holding the value.
                if let Some(elem) = self.shared(local) {
                    let cell = self.local_ty(local)?;
                    let (fresh, fill) =
                        (nazm_sema::Intrinsic::VecNew, nazm_sema::Intrinsic::VecPush);
                    value = self.made_holding(
                        fresh,
                        fill,
                        vec![elem],
                        vec![value],
                        cell,
                        l.span,
                        l.span,
                    )?;
                }
                self.bind(local);
                Stmt::Let { local, value }
            }
            AstStmt::Assign(a)
                if a.place.path.is_empty() && {
                    let local = self.local_at(a.place.name_span)?;
                    self.shared(local).is_some()
                } =>
            {
                // `x = v` on a shared binding (R1b, L10): the cell's one value replaced.
                let value = self.expr(&a.value)?;
                let local = self.local_at(a.place.name_span)?;
                let elem = self.shared(local).unwrap_or(Type::Int);
                Stmt::Eval(self.through_cell(local, elem, Some(value), a.span))
            }
            // `xs[i] = v` on a sequence (R1, 5b.15): the call the checker read it as.
            AstStmt::Assign(a) if matches!(a.place.path.as_slice(), [nazm_syntax::PlaceStep::Index { span, .. }] if self.res.named_method(*span)) =>
            {
                let Some(nazm_syntax::PlaceStep::Index { index, span }) = a.place.path.first()
                else {
                    return defect(a.span, "an element write that is no longer one");
                };
                let args = [
                    AstExpr::Var {
                        name: a.place.name.clone(),
                        span: a.place.name_span,
                    },
                    (**index).clone(),
                    a.value.clone(),
                ];
                Stmt::Eval(self.call(*span, &args, *span)?)
            }
            AstStmt::Assign(a) => {
                let mut value = self.expr(&a.value)?;
                let mut local = self.local_at(a.place.name_span)?;
                // `s = str_concat(s, t)` (R1b, D2): an append, which may reuse `s`'s storage.
                if a.place.path.is_empty() && self.appends_to(local, &a.value) {
                    if let ExprKind::Builtin { op, .. } = &mut value.kind {
                        *op = nazm_sema::Intrinsic::StrAppend;
                    }
                }
                // `x.f = v` on a shared binding (R1b, L10): its value copied out, changed, and
                // set back.
                let shared = self.shared(local).map(|elem| (local, elem));
                if let Some((cell, elem)) = shared {
                    let copy = self.temp(Temp::Made, elem, a.span);
                    let read = self.through_cell(cell, elem, None, a.span);
                    self.pending_cell = Some((cell, copy, read));
                    local = copy;
                }
                let mut path = Vec::with_capacity(a.place.path.len());
                let mut target = self.local_ty(local)?;
                for step in &a.place.path {
                    let step = match step {
                        nazm_syntax::PlaceStep::Field(f) => f,
                        // An element of an array (Gate 2), whose length the type says.
                        nazm_syntax::PlaceStep::Index { index, span } => {
                            let Some((elem, len)) = (match target {
                                Type::Array(arr) => self.res.types().array_info(arr),
                                _ => None,
                            }) else {
                                return defect(*span, "an index into something not an array");
                            };
                            path.push(cir::PathStep::Index {
                                index: self.expr(index)?,
                                len,
                                at: *span,
                            });
                            target = elem;
                            continue;
                        }
                    };
                    let Some(to) = self.res.field(step.name_span) else {
                        return defect(step.name_span, "a field the checker did not resolve");
                    };
                    target = self
                        .res
                        .records()
                        .get(to.record)
                        .and_then(|d| d.fields.get(to.field as usize))
                        .map_or(Type::Int, |f| f.ty);
                    path.push(cir::PathStep::Field(Step {
                        record: to.record,
                        field: to.field,
                        at: step.name_span,
                    }));
                }
                let assign = Stmt::Assign {
                    local,
                    path,
                    value,
                    at: a.span,
                };
                match self.pending_cell.take() {
                    None => assign,
                    Some((cell, copy, read)) => {
                        let back = Expr {
                            kind: ExprKind::Local(copy),
                            ty: self.local_ty(copy).ok(),
                            at: a.span,
                        };
                        let elem = self.local_ty(copy)?;
                        let set = self.through_cell(cell, elem, Some(back), a.span);
                        Stmt::Block(Block {
                            stmts: vec![
                                Stmt::Let {
                                    local: copy,
                                    value: read,
                                },
                                assign,
                                Stmt::Eval(set),
                            ],
                            tail: None,
                            ends: Ends::FallsThrough,
                            owns: vec![copy],
                            at: a.span,
                        })
                    }
                }
            }
            AstStmt::While(w) => {
                // Numbered where the `while` is, so loops are in source order; the condition
                // is lowered outside the loop's own target.
                let id = LoopId(self.loops);
                self.loops += 1;
                let cond = self.expr(&w.cond)?;
                self.targets.push(id);
                let body = self.statement_block(&w.body, Vec::new());
                self.targets.pop();
                Stmt::Loop {
                    id,
                    cond,
                    body: body?,
                    at: w.span,
                }
            }
            AstStmt::For(f) => {
                // Numbered where the `for` is, as a `while` is.
                let id = LoopId(self.loops);
                self.loops += 1;
                self.open(Vec::new());
                let stmts = self.for_loop(f, id);
                let owns = self.close();
                Stmt::Block(Block {
                    stmts: stmts?,
                    tail: None,
                    ends: Ends::FallsThrough,
                    owns,
                    at: f.span,
                })
            }
            AstStmt::Scope(sc) => Stmt::Scope {
                body: self.statement_block(&sc.body, Vec::new())?,
                at: sc.span,
            },
            AstStmt::Spawn(sp) => {
                let args = self.exprs(&sp.args)?;
                let Some(Callee::Function(callee)) = self.res.callee(sp.callee_span) else {
                    return defect(
                        sp.callee_span,
                        "a task whose function the checker did not resolve",
                    );
                };
                Stmt::Spawn {
                    callee,
                    args,
                    at: sp.span,
                    callee_at: sp.callee_span,
                }
            }
            AstStmt::Return(r) => {
                let value = self.expr(&r.value)?;
                self.leave(value, r.span)?
            }
            AstStmt::Break { span } => {
                let leave = Stmt::Break {
                    target: self.target("break", *span)?,
                    at: *span,
                };
                self.through_defers(self.targets.len(), leave, *span)?
            }
            AstStmt::Continue { span } => {
                let leave = Stmt::Continue {
                    target: self.target("continue", *span)?,
                    at: *span,
                };
                self.through_defers(self.targets.len(), leave, *span)?
            }
            // Registered, and run where its block is left (L12): nothing happens here.
            AstStmt::Defer(d) => {
                self.defers.push((d.body.clone(), self.targets.len()));
                Stmt::Block(Block {
                    stmts: Vec::new(),
                    tail: None,
                    ends: Ends::FallsThrough,
                    owns: Vec::new(),
                    at: d.span,
                })
            }
            AstStmt::Expr(e) => self.effect(e)?,
        })
    }

    /// A `for` loop as the `while` loop `docs/spec.md` (*`for` loops*) says it means (R1a):
    /// `let seq = e; let mut i = 0; while i < len(seq) { let x = get(seq, i); i = i + 1; body }`,
    /// or `let mut i = a; let end = b; while i < end { let x = i; i = i + 1; body }`. The
    /// step is counted before the body, so `continue` moves on; the hidden locals are this
    /// region's, released when the loop is done.
    fn for_loop(&mut self, f: &nazm_syntax::For, id: LoopId) -> Lowered<Vec<Stmt>> {
        let at = f.name_span;
        let int = |kind: ExprKind| Expr {
            kind,
            ty: Some(Type::Int),
            at,
        };
        let mut stmts = Vec::new();
        let index = self.temp(Temp::ForIndex, Type::Int, at);
        // What the condition compares the step with, and how each step's element is read.
        let (bound, element): (Expr, Box<dyn Fn(Expr) -> Expr>) = match &f.iter {
            nazm_syntax::ForIter::Each(e) => {
                let seq_value = self.expr(e)?;
                let Some(seq_ty) = seq_value.ty else {
                    return defect(e.span(), "a `for` sequence the checker gave no type");
                };
                let (len, get, elem, type_args) = match seq_ty {
                    Type::Ints => (
                        nazm_sema::Intrinsic::IntsLen,
                        nazm_sema::Intrinsic::IntsGet,
                        Type::Int,
                        Vec::new(),
                    ),
                    Type::Strs => (
                        nazm_sema::Intrinsic::StrsLen,
                        nazm_sema::Intrinsic::StrsGet,
                        Type::Str,
                        Vec::new(),
                    ),
                    Type::Bytes => (
                        nazm_sema::Intrinsic::BytesLen,
                        nazm_sema::Intrinsic::BytesGet,
                        Type::of_num(NumKind::U8),
                        Vec::new(),
                    ),
                    Type::Vec(v) => {
                        let Some(elem) = self.res.types().vec_element(v) else {
                            return defect(e.span(), "a `Vec` with no element type");
                        };
                        (
                            nazm_sema::Intrinsic::VecLen,
                            nazm_sema::Intrinsic::VecGet,
                            elem,
                            vec![elem],
                        )
                    }
                    // A `List` (R1a), read as `list_len` and `list_get` read it.
                    Type::List(v) => {
                        let Some(elem) = self.res.types().vec_element(v) else {
                            return defect(e.span(), "a `List` with no element type");
                        };
                        (
                            nazm_sema::Intrinsic::ListLen,
                            nazm_sema::Intrinsic::ListGet,
                            elem,
                            vec![elem],
                        )
                    }
                    _ => return defect(e.span(), "a `for` over a type the checker refuses"),
                };
                let seq = self.temp(Temp::ForSeq, seq_ty, at);
                stmts.push(Stmt::Let {
                    local: seq,
                    value: seq_value,
                });
                self.bind(seq);
                let zero = self.constant(Const::Int(0));
                stmts.push(Stmt::Let {
                    local: index,
                    value: int(ExprKind::Const(zero)),
                });
                self.bind(index);
                let read_seq = Expr {
                    kind: ExprKind::Local(seq),
                    ty: Some(seq_ty),
                    at,
                };
                let bound = int(ExprKind::Builtin {
                    op: len,
                    type_args: type_args.clone(),
                    args: vec![read_seq.clone()],
                    callee_at: at,
                });
                let element = Box::new(move |i: Expr| Expr {
                    kind: ExprKind::Builtin {
                        op: get,
                        type_args: type_args.clone(),
                        args: vec![read_seq.clone(), i],
                        callee_at: at,
                    },
                    ty: Some(elem),
                    at,
                });
                (bound, element as Box<dyn Fn(Expr) -> Expr>)
            }
            nazm_syntax::ForIter::Range { start, end } => {
                let start = self.expr(start)?;
                let end = self.expr(end)?;
                stmts.push(Stmt::Let {
                    local: index,
                    value: start,
                });
                self.bind(index);
                let last = self.temp(Temp::ForEnd, Type::Int, at);
                stmts.push(Stmt::Let {
                    local: last,
                    value: end,
                });
                self.bind(last);
                (
                    int(ExprKind::Local(last)),
                    Box::new(|i: Expr| i) as Box<dyn Fn(Expr) -> Expr>,
                )
            }
        };
        let read_index = int(ExprKind::Local(index));
        let cond = Expr {
            kind: ExprKind::Compare {
                op: cir::CmpOp::Lt,
                operand: Type::Int,
                law: Law::Int,
                lhs: Box::new(read_index.clone()),
                rhs: Box::new(bound),
            },
            ty: Some(Type::Bool),
            at,
        };
        let name = self.local_at(f.name_span)?;
        let one = self.constant(Const::Int(1));
        let step = vec![
            Stmt::Let {
                local: name,
                value: element(read_index.clone()),
            },
            Stmt::Assign {
                local: index,
                path: Vec::new(),
                value: int(ExprKind::Arith {
                    op: cir::ArithOp::Add,
                    lhs: Box::new(read_index),
                    rhs: Box::new(int(ExprKind::Const(one))),
                }),
                at,
            },
        ];
        self.targets.push(id);
        let body = self.statement_block(&f.body, vec![name]);
        self.targets.pop();
        let mut body = body?;
        body.stmts.splice(0..0, step);
        stmts.push(Stmt::Loop {
            id,
            cond,
            body,
            at: f.span,
        });
        Ok(stmts)
    }

    /// An expression where no value is wanted.
    fn effect(&mut self, e: &AstExpr) -> Lowered<Stmt> {
        Ok(match e {
            AstExpr::If {
                cond,
                then_block,
                else_block,
                span,
            } => {
                let cond = self.expr(cond)?;
                let then = self.statement_block(then_block, Vec::new())?;
                let otherwise = match else_block {
                    Some(b) => Some(self.statement_block(b, Vec::new())?),
                    None => None,
                };
                Stmt::If {
                    cond,
                    then,
                    otherwise,
                    at: *span,
                }
            }
            AstExpr::Match {
                scrutinee,
                arms,
                span,
                ..
            } => {
                if arms.iter().all(plain_arm) {
                    Stmt::Match(Box::new(self.matching(scrutinee, arms, *span, false)?))
                } else {
                    Stmt::Block(self.matching_general(scrutinee, arms, *span, false)?)
                }
            }
            AstExpr::Block(b) => Stmt::Block(self.statement_block(b, Vec::new())?),
            other => Stmt::Eval(self.expr(other)?),
        })
    }

    // --- values ------------------------------------------------------------------------

    fn exprs(&mut self, es: &[AstExpr]) -> Lowered<Vec<Expr>> {
        es.iter().map(|e| self.expr(e)).collect()
    }

    fn expr(&mut self, e: &AstExpr) -> Lowered<Expr> {
        let node = |kind: ExprKind, ty: Option<Type>, at: Span| Expr { kind, ty, at };
        Ok(match e {
            AstExpr::Int { value, span } => self.integer((*value).into(), *span)?,
            AstExpr::WideInt { value, span } => self.integer((*value).into(), *span)?,
            // Rounded once, from the decimal text, in the literal's own type (Gate 2).
            AstExpr::Float { text, span } => {
                let kind = match self.res.numeric(*span) {
                    Some(NumericFact::Literal(t)) => t.num().unwrap_or(NumKind::F64),
                    _ => NumKind::F64,
                };
                let v = nazm_sema::numeric::literal(kind, text);
                node(
                    ExprKind::Const(self.constant(Const::Num { kind, bits: v.bits })),
                    Some(Type::of_num(kind)),
                    *span,
                )
            }
            AstExpr::Bool { value, span } => node(
                ExprKind::Const(self.constant(Const::Bool(*value))),
                Some(Type::Bool),
                *span,
            ),
            // A literal where a `Text` is expected (Gate 2): the literal's bytes, which are UTF-8,
            // as the `Text` they are.
            AstExpr::Str { value, span }
                if matches!(
                    self.res.numeric(*span),
                    Some(NumericFact::Literal(Type::Text))
                ) =>
            {
                let s = Expr {
                    kind: ExprKind::Const(self.constant(Const::Str(value.clone().into_bytes()))),
                    ty: Some(Type::Str),
                    at: *span,
                };
                self.internal(nazm_sema::Intrinsic::TextAssume, s, Type::Text)
            }
            AstExpr::Str { value, span } => node(
                ExprKind::Const(self.constant(Const::Str(value.clone().into_bytes()))),
                Some(Type::Str),
                *span,
            ),
            // A static `Atomic` (Gate 3): its one cell.
            AstExpr::Var { span, .. } if self.res.static_cell(*span).is_some() => {
                let Some((module, name)) = self.res.static_cell(*span).cloned() else {
                    return defect(*span, "a static the checker did not record");
                };
                node(
                    ExprKind::Const(self.constant(Const::Cell { module, name })),
                    Some(Type::Atomic),
                    *span,
                )
            }
            // A constant (Gate 2): the value the checker computed, written as a literal.
            AstExpr::Var { span, .. } if self.res.const_use(*span).is_some() => {
                let Some(value) = self.res.const_use(*span).cloned() else {
                    return defect(*span, "a constant the checker did not compute");
                };
                self.const_value(&value, *span)?
            }
            // A named function as a value (N50): the checker recorded which, and its type.
            AstExpr::Var { span, .. } if self.res.var(*span).is_none() => {
                match self.res.fn_value(*span) {
                    Some(v) => node(ExprKind::FnValue(v.function), Some(v.ty), *span),
                    None => {
                        return defect(*span, "a name that is neither a binding nor a function")
                    }
                }
            }
            AstExpr::Var { span, .. } => {
                let l = self.local_at(*span)?;
                // A shared binding is read out of its cell (R1b, L10).
                if let Some(elem) = self.shared(l) {
                    return Ok(self.through_cell(l, elem, None, *span));
                }
                node(ExprKind::Local(l), Some(self.local_ty(l)?), *span)
            }
            AstExpr::Closure { span, .. } => {
                let (Some(func), Some(info)) = (
                    self.res.closure_at(*span),
                    self.res
                        .closure_at(*span)
                        .and_then(|f| self.res.function(f))
                        .and_then(|d| d.closure.as_ref()),
                ) else {
                    return defect(*span, "a closure the checker did not declare");
                };
                let captures = info
                    .captures
                    .iter()
                    .map(|c| LocalId(c.outer + self.shift))
                    .collect();
                node(ExprKind::Closure { func, captures }, Some(info.ty), *span)
            }
            // `-128` written as an `Int8` is one literal (Gate 2): the checker judged it whole.
            AstExpr::Neg { operand, .. }
                if matches!(&**operand, AstExpr::Int { span, .. }
                    if matches!(self.res.numeric(*span), Some(NumericFact::Literal(_)))) =>
            {
                let AstExpr::Int { value, span } = &**operand else {
                    return defect(operand.span(), "a negated literal that is not one");
                };
                self.integer((-i128::from(*value)).into(), *span)?
            }
            // And past `Int`'s digits (R1b, L13b).
            AstExpr::Neg { operand, .. }
                if matches!(&**operand, AstExpr::WideInt { span, .. }
                    if matches!(self.res.numeric(*span), Some(NumericFact::Literal(_)))) =>
            {
                let AstExpr::WideInt { value, span } = &**operand else {
                    return defect(operand.span(), "a negated literal that is not one");
                };
                self.integer(nazm_sema::numeric::Wide::from_u128(*value).negate(), *span)?
            }
            AstExpr::Neg {
                operand, op_span, ..
            } => {
                let operand = self.expr(operand)?;
                let ty = operand.ty.or(Some(Type::Int));
                node(ExprKind::Neg(Box::new(operand)), ty, *op_span)
            }
            // Arrays (Gate 2). The type is the checker's, read back from the elements' and the
            // count's: `[x; N]`'s `N` is a literal or a constant, both already computed.
            AstExpr::Array { elems, span } => {
                let mut lowered = Vec::with_capacity(elems.len());
                for x in elems {
                    lowered.push(self.expr(x)?);
                }
                let elem = lowered.first().and_then(|x| x.ty).unwrap_or(Type::Int);
                let n = u32::try_from(lowered.len()).unwrap_or(u32::MAX);
                let Some(ty) = self.res.types().find_array(elem, n) else {
                    return defect(*span, "an array type the checker did not form");
                };
                node(ExprKind::Array { elems: lowered }, Some(ty), *span)
            }
            AstExpr::ArrayRepeat { value, count, span } => {
                let v = self.expr(value)?;
                let n = match count.as_ref() {
                    AstExpr::Int { value, .. } => u32::try_from(*value).ok(),
                    AstExpr::Var { span, .. } => self
                        .res
                        .const_use(*span)
                        .and_then(nazm_sema::numeric::ConstValue::as_int)
                        .and_then(|n| u32::try_from(n).ok()),
                    _ => None,
                };
                let elem = v.ty.unwrap_or(Type::Int);
                let Some(ty) = n.and_then(|n| self.res.types().find_array(elem, n)) else {
                    return defect(*span, "a repeated array the checker did not form");
                };
                node(
                    ExprKind::ArrayRepeat {
                        value: Box::new(v),
                        count: n.unwrap_or(0),
                    },
                    Some(ty),
                    *span,
                )
            }
            // On a sequence (R1, 5b.15), the call the checker read it as.
            AstExpr::Index {
                base,
                index,
                bracket_span,
                ..
            } if self.res.named_method(*bracket_span) => self.call(
                *bracket_span,
                &[(**base).clone(), (**index).clone()],
                *bracket_span,
            )?,
            AstExpr::Index {
                base,
                index,
                bracket_span,
                ..
            } => {
                let b = self.expr(base)?;
                let i = self.expr(index)?;
                let elem = match b.ty {
                    Some(Type::Array(a)) => self.res.types().array_info(a).map(|(t, _)| t),
                    _ => None,
                };
                node(
                    ExprKind::Index {
                        base: Box::new(b),
                        index: Box::new(i),
                    },
                    elem,
                    *bracket_span,
                )
            }
            AstExpr::BitNot {
                operand, op_span, ..
            } => {
                let operand = self.expr(operand)?;
                let ty = operand.ty.or(Some(Type::Int));
                node(ExprKind::Complement(Box::new(operand)), ty, *op_span)
            }
            // `!e`, `a && b` and `a || b` are `if`s (`docs/spec.md`, *Logical operators*, N49):
            // lowered here, so short-circuiting is control flow every later layer already has.
            AstExpr::Not {
                operand, op_span, ..
            } => {
                let operand = self.expr(operand)?;
                let (f, t) = (self.boolean(false, *op_span), self.boolean(true, *op_span));
                self.conditional(operand, f, t, *op_span)
            }
            AstExpr::Binary {
                op: op @ (BinOp::And | BinOp::Or),
                lhs,
                rhs,
                op_span,
                ..
            } => {
                let (lhs, rhs) = (self.expr(lhs)?, self.expr(rhs)?);
                if *op == BinOp::And {
                    let f = self.boolean(false, *op_span);
                    self.conditional(lhs, rhs, f, *op_span)
                } else {
                    let t = self.boolean(true, *op_span);
                    self.conditional(lhs, t, rhs, *op_span)
                }
            }
            AstExpr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => {
                let (lhs, rhs) = (self.expr(lhs)?, self.expr(rhs)?);
                // An operator's result is its operands' type (Gate 2); checking made them one,
                // and for a shift the left operand's.
                let operand_ty = lhs.ty.or(rhs.ty).or(Some(Type::Int));
                if let Some(bop) = bitwise(*op) {
                    return Ok(node(
                        ExprKind::Bits {
                            op: bop,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        operand_ty,
                        *op_span,
                    ));
                }
                match arithmetic(*op) {
                    Some(op) => node(
                        ExprKind::Arith {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        operand_ty,
                        *op_span,
                    ),
                    None => {
                        let op = comparison(*op);
                        // Whichever side has a type; checking made them agree. Neither has
                        // one only when both always transfer, and then nothing compares.
                        let operand = lhs.ty.or(rhs.ty).unwrap_or(Type::Int);
                        let Some(law) = Law::of(op, operand) else {
                            return defect(*op_span, "a comparison of a type with no such law");
                        };
                        node(
                            ExprKind::Compare {
                                op,
                                operand,
                                law,
                                lhs: Box::new(lhs),
                                rhs: Box::new(rhs),
                            },
                            Some(Type::Bool),
                            *op_span,
                        )
                    }
                }
            }
            AstExpr::Call {
                callee_span,
                args,
                span,
                ..
            } => self.call(*callee_span, args, *span)?,
            // A method call (N78), and `x.m()` the checker read as one.
            // `f"…"` (L4): the text and each hole's, joined by `str_concat` left to right.
            AstExpr::Interp { parts, span } => self.interpolation(parts, *span)?,
            AstExpr::MethodCall {
                receiver,
                method_span,
                args,
                span,
                ..
            } => {
                let ufcs = matches!(&**receiver, AstExpr::Var { span, .. } if self.res.var(*span).is_none());
                self.method_call((!ufcs).then_some(&**receiver), *method_span, args, *span)?
            }
            AstExpr::Variant {
                ty,
                ty_span,
                variant_span,
                fields,
                span,
                ..
            } if fields.is_empty() && self.res.callee(*variant_span).is_some() => {
                let receiver = AstExpr::Var {
                    name: ty.clone(),
                    span: *ty_span,
                };
                let ufcs = self.res.var(*ty_span).is_none();
                self.method_call((!ufcs).then_some(&receiver), *variant_span, &[], *span)?
            }
            AstExpr::Construct {
                ty_span, fields, ..
            } => {
                let Some(record) = self.res.construction(*ty_span) else {
                    return defect(*ty_span, "a construction the checker did not resolve");
                };
                let arity = self.res.records().get(record).map_or(0, |d| d.fields.len());
                let mut inits = Vec::with_capacity(fields.len());
                for f in fields {
                    let Some(to) = self.res.field(f.name_span) else {
                        return defect(f.name_span, "an initialiser the checker did not resolve");
                    };
                    inits.push(Init {
                        field: to.field,
                        value: self.expr(&f.value)?,
                        at: f.name_span,
                    });
                }
                node(
                    ExprKind::Record {
                        record,
                        arity: u32::try_from(arity).unwrap_or(u32::MAX),
                        fields: inits,
                    },
                    Some(Type::Record(record)),
                    *ty_span,
                )
            }
            AstExpr::Variant {
                variant_span,
                fields,
                ty_span,
                ..
            } => {
                let Some(to) = self.res.variant(*variant_span) else {
                    return defect(*variant_span, "a variant the checker did not resolve");
                };
                let arity = self
                    .res
                    .enums()
                    .get(to.enumeration)
                    .and_then(|d| d.variants.get(to.variant as usize))
                    .map_or(0, |v| v.fields.len());
                let mut inits = Vec::with_capacity(fields.len());
                for f in fields {
                    let Some(p) = self.res.payload(f.name_span) else {
                        return defect(f.name_span, "a payload field the checker did not resolve");
                    };
                    inits.push(Init {
                        field: p.field,
                        value: self.expr(&f.value)?,
                        at: f.name_span,
                    });
                }
                node(
                    ExprKind::Variant {
                        enumeration: to.enumeration,
                        variant: to.variant,
                        arity: u32::try_from(arity).unwrap_or(u32::MAX),
                        fields: inits,
                    },
                    Some(Type::Enum(to.enumeration)),
                    *ty_span,
                )
            }
            AstExpr::Match {
                scrutinee,
                arms,
                span,
                ..
            } if !arms.iter().all(plain_arm) => {
                let b = self.matching_general(scrutinee, arms, *span, true)?;
                let ty = b.ty();
                node(ExprKind::Block(b), ty, *span)
            }
            AstExpr::Match {
                scrutinee,
                arms,
                span,
                ..
            } => {
                let m = self.matching(scrutinee, arms, *span, true)?;
                // The type its arms produce; checking made them agree, and one that transfers
                // produces none.
                let ty = m.arms.iter().rev().find_map(|a| a.body.ty());
                node(ExprKind::Match(Box::new(m)), ty, *span)
            }
            // `(a, b, …)` (L5): the tuple record of its elements' types, built in order.
            AstExpr::Tuple { elems, span } => {
                let values = self.exprs(elems)?;
                let tys: Option<Vec<Type>> = values.iter().map(|v| v.ty).collect();
                let Some(Type::Record(record)) = tys.and_then(|t| self.res.types().find_tuple(&t))
                else {
                    return defect(*span, "a tuple the checker did not intern");
                };
                let arity = u32::try_from(values.len()).unwrap_or(u32::MAX);
                let fields = values
                    .into_iter()
                    .enumerate()
                    .map(|(i, value)| Init {
                        field: u32::try_from(i).unwrap_or(u32::MAX),
                        at: value.at,
                        value,
                    })
                    .collect();
                node(
                    ExprKind::Record {
                        record,
                        arity,
                        fields,
                    },
                    Some(Type::Record(record)),
                    *span,
                )
            }
            AstExpr::Field {
                base, name_span, ..
            } => {
                let Some(to) = self.res.field(*name_span) else {
                    return defect(*name_span, "a projection the checker did not resolve");
                };
                let Some(ty) = self
                    .res
                    .records()
                    .get(to.record)
                    .and_then(|d| d.fields.get(to.field as usize))
                    .map(|f| f.ty)
                else {
                    return defect(
                        *name_span,
                        "a projection of a field its record does not have",
                    );
                };
                node(
                    ExprKind::Field {
                        base: Box::new(self.expr(base)?),
                        record: to.record,
                        field: to.field,
                    },
                    Some(ty),
                    *name_span,
                )
            }
            AstExpr::Propagate {
                operand, op_span, ..
            } => self.propagate(operand, *op_span)?,
            AstExpr::If {
                cond,
                then_block,
                else_block,
                span,
            } => {
                let Some(else_block) = else_block else {
                    return defect(*span, "an `if` with no `else` where a value is wanted");
                };
                let cond = self.expr(cond)?;
                let then = self.value_block(then_block, Vec::new())?;
                let otherwise = self.value_block(else_block, Vec::new())?;
                // A branch that transfers contributes no type, and the checker says the same:
                // `(Diverges, other) => other`.
                let ty = then.ty().or(otherwise.ty());
                node(
                    ExprKind::If {
                        cond: Box::new(cond),
                        then,
                        otherwise,
                    },
                    ty,
                    *span,
                )
            }
            AstExpr::Block(b) => {
                let b = self.value_block(b, Vec::new())?;
                let ty = b.ty();
                let at = b.at;
                node(ExprKind::Block(b), ty, at)
            }
        })
    }

    /// The constant `value`, as a `Bool` expression at `at`.
    fn boolean(&mut self, value: bool, at: Span) -> Expr {
        Expr {
            kind: ExprKind::Const(self.constant(Const::Bool(value))),
            ty: Some(Type::Bool),
            at,
        }
    }

    /// `if cond { then } else { otherwise }`, each branch a block holding only its expression.
    /// What a logical operator is: the branch not taken is never evaluated.
    fn conditional(&mut self, cond: Expr, then: Expr, otherwise: Expr, at: Span) -> Expr {
        let branch = |e: Expr| Block {
            stmts: Vec::new(),
            at: e.at,
            tail: Some(Box::new(e)),
            ends: Ends::FallsThrough,
            owns: Vec::new(),
        };
        let (then, otherwise) = (branch(then), branch(otherwise));
        let ty = then.ty().or(otherwise.ty());
        Expr {
            kind: ExprKind::If {
                cond: Box::new(cond),
                then,
                otherwise,
            },
            ty,
            at,
        }
    }

    /// A call: to a built-in or to a function of this compilation, as the checker resolved it,
    /// with the result type the checker settled for this call.
    fn call(&mut self, callee_at: Span, args: &[AstExpr], at: Span) -> Lowered<Expr> {
        if let Some(NumericFact::Convert {
            to, mode, result, ..
        }) = self.res.numeric(callee_at)
        {
            return self.conversion(to, mode, result, args, at);
        }
        let args = self.exprs(args)?;
        let Some(checked) = self.res.checked_call(callee_at) else {
            return defect(callee_at, "a call the checker did not record");
        };
        // A generic call's result is the checker's substitution of the callee's declared
        // result by the arguments it settled — recorded on the call where it had already
        // been formed, and otherwise formed by the checker just after recording, so it is
        // read back here by lookup. Nothing is interned and nothing is inferred.
        let ty = match &checked.instance {
            Some(i) => i.ret.or_else(|| {
                self.res
                    .type_args(callee_at)
                    .and_then(|args| self.res.types().substituted(checked.ret, i.owner, args))
            }),
            None => Some(checked.ret),
        };
        let Some(ty) = ty else {
            return defect(
                callee_at,
                "a call whose result type the checker never formed",
            );
        };
        let type_args = |generic: bool| -> Lowered<Vec<Type>> {
            if !generic {
                return Ok(Vec::new());
            }
            match self.res.type_args(callee_at) {
                Some(a) => Ok(a.to_vec()),
                None => defect(
                    callee_at,
                    "a generic call whose type arguments were not settled",
                ),
            }
        };
        let kind = match checked.callee {
            // Made out of the `Box` and `List` operations a program cannot name (R1a).
            Callee::Builtin(nazm_sema::Intrinsic::BoxNew) => {
                let (fresh, fill) = (
                    nazm_sema::Intrinsic::BoxFresh,
                    nazm_sema::Intrinsic::BoxFill,
                );
                return self.made_holding(fresh, fill, type_args(true)?, args, ty, callee_at, at);
            }
            // A `Cell[T]` is a `Vec[T]` of one element (R1b): made with it, read and replaced at 0.
            Callee::Builtin(nazm_sema::Intrinsic::CellNew) => {
                let (fresh, fill) = (nazm_sema::Intrinsic::VecNew, nazm_sema::Intrinsic::VecPush);
                return self.made_holding(fresh, fill, type_args(true)?, args, ty, callee_at, at);
            }
            Callee::Builtin(
                op @ (nazm_sema::Intrinsic::CellGet | nazm_sema::Intrinsic::CellSet),
            ) => {
                let zero = Expr {
                    kind: ExprKind::Const(self.constant(Const::Int(0))),
                    ty: Some(Type::Int),
                    at: callee_at,
                };
                let mut args = args.into_iter();
                let cell: Vec<Expr> = args.next().into_iter().collect();
                ExprKind::Builtin {
                    op: if op == nazm_sema::Intrinsic::CellGet {
                        nazm_sema::Intrinsic::VecGet
                    } else {
                        nazm_sema::Intrinsic::VecSet
                    },
                    type_args: type_args(true)?,
                    args: cell.into_iter().chain([zero]).chain(args).collect(),
                    callee_at,
                }
            }
            // Checked arithmetic (L13): written from wrapping, saturating and dividing operations.
            Callee::Builtin(
                op @ (nazm_sema::Intrinsic::CheckedAdd
                | nazm_sema::Intrinsic::CheckedSub
                | nazm_sema::Intrinsic::CheckedMul),
            ) => {
                return self.checked(op, type_args(true)?, args, ty, callee_at, at);
            }
            Callee::Builtin(nazm_sema::Intrinsic::ListFromVec) => {
                return self.list_from_vec(type_args(true)?, args, ty, callee_at, at);
            }
            Callee::Builtin(op) => ExprKind::Builtin {
                op,
                type_args: type_args(op.generics() > 0)?,
                args,
                callee_at,
            },
            Callee::Function(callee) => {
                // A foreign call's crossing, written as ordinary Core IR (N85, §7.86).
                if self
                    .res
                    .function(callee)
                    .is_some_and(|d| d.foreign.is_some())
                {
                    return self.foreign_call(callee, args, ty, callee_at, at);
                }
                let generic = self
                    .res
                    .function(callee)
                    .is_some_and(|d| !d.type_params.is_empty());
                ExprKind::Call {
                    callee,
                    type_args: type_args(generic)?,
                    args,
                    callee_at,
                }
            }
            // Through a binding of function type (N50).
            Callee::Indirect(t) => {
                let l = self.local_at(callee_at)?;
                ExprKind::CallIndirect {
                    callee: Box::new(Expr {
                        kind: ExprKind::Local(l),
                        ty: Some(t),
                        at: callee_at,
                    }),
                    args,
                    callee_at,
                }
            }
        };
        Ok(Expr {
            kind,
            ty: Some(ty),
            at,
        })
    }

    /// `checked_add(a, b)`, `checked_sub(a, b)` or `checked_mul(a, b)` (R1b, L13), as the
    /// operations it means. An addition or subtraction overflows exactly when its wrapping and
    /// saturating results differ — the wrapped value is never the bound the saturating one stops
    /// at. A product overflows exactly when, `a` being neither `0` nor (signed) `-1`, dividing the
    /// wrapped product by `a` does not give `b` back; `-1 * MIN` is the one signed product that
    /// division cannot test, and is tested by name.
    #[allow(clippy::too_many_lines)]
    fn checked(
        &mut self,
        op: nazm_sema::Intrinsic,
        type_args: Vec<Type>,
        mut args: Vec<Expr>,
        ty: Type,
        callee_at: Span,
        at: Span,
    ) -> Lowered<Expr> {
        use nazm_sema::Intrinsic as I;
        let (Some(b), Some(a)) = (args.pop(), args.pop()) else {
            return defect(callee_at, "a checked operation without two operands");
        };
        let elem = type_args.first().copied().unwrap_or(Type::Int);
        let (Some(kind), Type::Enum(option)) = (elem.num(), ty) else {
            return defect(
                callee_at,
                "a checked operation not over an integer, or not to an `Option`",
            );
        };
        let Some(law) = Law::of(cir::CmpOp::Eq, elem) else {
            return defect(callee_at, "an integer with no equality");
        };
        let x = self.temp(Temp::Convert, elem, at);
        let y = self.temp(Temp::Convert, elem, at);
        let w = self.temp(Temp::Convert, elem, at);
        let local = |l: LocalId| Expr {
            kind: ExprKind::Local(l),
            ty: Some(elem),
            at,
        };
        let konst = |me: &mut Self, v: i128| -> Expr {
            let c = if kind == NumKind::I64 {
                Const::Int(i64::try_from(v).unwrap_or(0))
            } else {
                Const::Num {
                    kind,
                    bits: nazm_sema::numeric::NumVal::int(kind, v).map_or(0, |n| n.bits),
                }
            };
            Expr {
                kind: ExprKind::Const(me.constant(c)),
                ty: Some(elem),
                at,
            }
        };
        let builtin = |op: I, args: Vec<Expr>| Expr {
            kind: ExprKind::Builtin {
                op,
                type_args: vec![elem],
                args,
                callee_at,
            },
            ty: Some(elem),
            at,
        };
        let eq = |l: Expr, r: Expr| Expr {
            kind: ExprKind::Compare {
                op: cir::CmpOp::Eq,
                operand: elem,
                law,
                lhs: Box::new(l),
                rhs: Box::new(r),
            },
            ty: Some(Type::Bool),
            at,
        };
        let region = |e: Expr| Block {
            stmts: Vec::new(),
            tail: Some(Box::new(e)),
            ends: Ends::FallsThrough,
            owns: Vec::new(),
            at,
        };
        let choose = |c: Expr, then: Expr, otherwise: Expr| Expr {
            kind: ExprKind::If {
                cond: Box::new(c),
                then: region(then),
                otherwise: region(otherwise),
            },
            ty: Some(ty),
            at,
        };
        let some = |e: Expr| Expr {
            kind: ExprKind::Variant {
                enumeration: option,
                variant: 1,
                arity: 1,
                fields: vec![Init {
                    field: 0,
                    value: e,
                    at,
                }],
            },
            ty: Some(ty),
            at,
        };
        let none = || Expr {
            kind: ExprKind::Variant {
                enumeration: option,
                variant: 0,
                arity: 0,
                fields: Vec::new(),
            },
            ty: Some(ty),
            at,
        };
        let mut stmts = vec![
            Stmt::Let { local: x, value: a },
            Stmt::Let { local: y, value: b },
        ];
        let mut owns = vec![x, y, w];
        let tail = if op == I::CheckedMul {
            stmts.push(Stmt::Let {
                local: w,
                value: builtin(I::WrappingMul, vec![local(x), local(y)]),
            });
            let quotient = Expr {
                kind: ExprKind::Arith {
                    op: cir::ArithOp::Div,
                    lhs: Box::new(local(w)),
                    rhs: Box::new(local(x)),
                },
                ty: Some(elem),
                at,
            };
            let divided = choose(eq(quotient, local(y)), some(local(w)), none());
            let past_minus_one = if kind.is_signed() {
                // The kind's minimum; `Int128`'s is `i128::MIN` itself (R1b, L13b).
                let min = kind
                    .int_range()
                    .and_then(|(lo, _)| lo.to_i128())
                    .unwrap_or(0);
                let minus_one = konst(self, -1);
                let min = konst(self, min);
                choose(
                    eq(local(x), minus_one),
                    choose(eq(local(y), min), none(), some(local(w))),
                    divided,
                )
            } else {
                divided
            };
            let zero = konst(self, 0);
            choose(eq(local(x), zero), some(local(w)), past_minus_one)
        } else {
            let (wrapping, saturating) = if op == I::CheckedAdd {
                (I::WrappingAdd, I::SaturatingAdd)
            } else {
                (I::WrappingSub, I::SaturatingSub)
            };
            let s = self.temp(Temp::Convert, elem, at);
            owns.push(s);
            stmts.push(Stmt::Let {
                local: w,
                value: builtin(wrapping, vec![local(x), local(y)]),
            });
            stmts.push(Stmt::Let {
                local: s,
                value: builtin(saturating, vec![local(x), local(y)]),
            });
            choose(eq(local(w), local(s)), some(local(w)), none())
        };
        Ok(Expr {
            kind: ExprKind::Block(Block {
                stmts,
                tail: Some(Box::new(tail)),
                ends: Ends::FallsThrough,
                owns,
                at,
            }),
            ty: Some(ty),
            at,
        })
    }

    /// `box_new(v)` (R1a), as `{ let made = <a fresh box>; <fill made with v>; made }`: the box
    /// is the lowering's alone until it is the result, which is what lets it be written once.
    /// `cell_new(v)` (R1b) likewise, with `vec_new` and `vec_push`.
    #[allow(clippy::too_many_arguments)]
    fn made_holding(
        &mut self,
        fresh: nazm_sema::Intrinsic,
        fill: nazm_sema::Intrinsic,
        type_args: Vec<Type>,
        mut args: Vec<Expr>,
        ty: Type,
        callee_at: Span,
        at: Span,
    ) -> Lowered<Expr> {
        let Some(value) = args.pop() else {
            return defect(callee_at, "a `box_new` or `cell_new` with no value");
        };
        let made = self.temp(Temp::Made, ty, at);
        let read = Expr {
            kind: ExprKind::Local(made),
            ty: Some(ty),
            at,
        };
        let stmts = vec![
            Stmt::Let {
                local: made,
                value: Expr {
                    kind: ExprKind::Builtin {
                        op: fresh,
                        type_args: type_args.clone(),
                        args: Vec::new(),
                        callee_at,
                    },
                    ty: Some(ty),
                    at,
                },
            },
            Stmt::Eval(Expr {
                kind: ExprKind::Builtin {
                    op: fill,
                    type_args,
                    args: vec![read.clone(), value],
                    callee_at,
                },
                ty: Some(Type::Int),
                at,
            }),
        ];
        Ok(Expr {
            kind: ExprKind::Block(Block {
                stmts,
                tail: Some(Box::new(read)),
                ends: Ends::FallsThrough,
                owns: vec![made],
                at,
            }),
            ty: Some(ty),
            at,
        })
    }

    /// `list_from_vec(xs)` (R1a), as `{ let seq = xs; let made = list_empty(); let mut i = 0;
    /// while i < vec_len(seq) { list_push(made, vec_get(seq, i)); i = i + 1; } made }`: the
    /// elements as they are now, copied by their own law.
    fn list_from_vec(
        &mut self,
        type_args: Vec<Type>,
        mut args: Vec<Expr>,
        ty: Type,
        callee_at: Span,
        at: Span,
    ) -> Lowered<Expr> {
        use nazm_sema::Intrinsic as I;
        let Some(source) = args.pop() else {
            return defect(callee_at, "a `list_from_vec` with no vector");
        };
        let Some(seq_ty) = source.ty else {
            return defect(
                callee_at,
                "a `list_from_vec` vector the checker gave no type",
            );
        };
        let elem = type_args.first().copied().unwrap_or(Type::Int);
        let seq = self.temp(Temp::ForSeq, seq_ty, at);
        let made = self.temp(Temp::Made, ty, at);
        let index = self.temp(Temp::ForIndex, Type::Int, at);
        let local = |l: LocalId, t: Type| Expr {
            kind: ExprKind::Local(l),
            ty: Some(t),
            at,
        };
        let builtin = |op: I, args: Vec<Expr>, t: Type| Expr {
            kind: ExprKind::Builtin {
                op,
                type_args: type_args.clone(),
                args,
                callee_at,
            },
            ty: Some(t),
            at,
        };
        let zero = self.constant(Const::Int(0));
        let one = self.constant(Const::Int(1));
        let int = |kind: ExprKind| Expr {
            kind,
            ty: Some(Type::Int),
            at,
        };
        let id = LoopId(self.loops);
        self.loops += 1;
        let cond = Expr {
            kind: ExprKind::Compare {
                op: cir::CmpOp::Lt,
                operand: Type::Int,
                law: Law::Int,
                lhs: Box::new(local(index, Type::Int)),
                rhs: Box::new(builtin(I::VecLen, vec![local(seq, seq_ty)], Type::Int)),
            },
            ty: Some(Type::Bool),
            at,
        };
        let body = Block {
            stmts: vec![
                Stmt::Eval(builtin(
                    I::ListPush,
                    vec![
                        local(made, ty),
                        builtin(
                            I::VecGet,
                            vec![local(seq, seq_ty), local(index, Type::Int)],
                            elem,
                        ),
                    ],
                    Type::Int,
                )),
                Stmt::Assign {
                    local: index,
                    path: Vec::new(),
                    value: int(ExprKind::Arith {
                        op: cir::ArithOp::Add,
                        lhs: Box::new(local(index, Type::Int)),
                        rhs: Box::new(int(ExprKind::Const(one))),
                    }),
                    at,
                },
            ],
            tail: None,
            ends: Ends::FallsThrough,
            owns: Vec::new(),
            at,
        };
        let stmts = vec![
            Stmt::Let {
                local: seq,
                value: source,
            },
            Stmt::Let {
                local: made,
                value: builtin(I::ListEmpty, Vec::new(), ty),
            },
            Stmt::Let {
                local: index,
                value: int(ExprKind::Const(zero)),
            },
            Stmt::Loop { id, cond, body, at },
        ];
        Ok(Expr {
            kind: ExprKind::Block(Block {
                stmts,
                tail: Some(Box::new(local(made, ty))),
                ends: Ends::FallsThrough,
                owns: vec![seq, made, index],
                at,
            }),
            ty: Some(ty),
            at,
        })
    }

    /// A call of a foreign function (N85, §7.86), as the C function sees it: each handle
    /// argument its pointer, each callback the address of an export's C entry, and the result
    /// — a pointer for a handle or a string — checked against null or tested for it, then
    /// made a handle, a copied `Str` or an `Option` of either. The callee's own signature is
    /// the raw one ([`raw_foreign`]), so MIR and both backends see only `Int`, `Bool`, a
    /// borrowed `Str` and a C-layout record cross.
    fn foreign_call(
        &mut self,
        callee: FnId,
        args: Vec<Expr>,
        ty: Type,
        callee_at: Span,
        at: Span,
    ) -> Lowered<Expr> {
        use crate::check::{foreign_param, foreign_result, CrossIn, CrossOut};
        let Some(def) = self.res.function(callee) else {
            return defect(callee_at, "a foreign call of no function");
        };
        let params = def.params.clone();
        let mut raw = Vec::with_capacity(args.len());
        for (a, t) in args.into_iter().zip(params) {
            let a_at = a.at;
            raw.push(match foreign_param(self.res, t) {
                Some(CrossIn::Handle(record)) => Expr {
                    kind: ExprKind::Field {
                        base: Box::new(a),
                        record,
                        field: 0,
                    },
                    ty: Some(Type::Int),
                    at: a_at,
                },
                Some(CrossIn::Callback) => {
                    let ExprKind::FnValue(g) = a.kind else {
                        return defect(a_at, "a callback that is not an export's name");
                    };
                    let Some(symbol) = self.res.function(g).and_then(|d| d.export.clone()) else {
                        return defect(a_at, "a callback that is not an export");
                    };
                    let name = Expr {
                        kind: ExprKind::Const(self.constant(Const::Str(symbol.into_bytes()))),
                        ty: Some(Type::Str),
                        at: a_at,
                    };
                    self.internal(nazm_sema::Intrinsic::CallbackAddress, name, Type::Int)
                }
                _ => a,
            });
        }
        let out = foreign_result(self.res, checked_option(self.res, ty), ty);
        let raw_ty = match out {
            Some(CrossOut::Scalar) | None => ty,
            Some(_) => Type::Int,
        };
        let call = Expr {
            kind: ExprKind::Call {
                callee,
                type_args: Vec::new(),
                args: raw,
                callee_at,
            },
            ty: Some(raw_ty),
            at,
        };
        let handle = |record, value: Expr| Expr {
            kind: ExprKind::Record {
                record,
                arity: 1,
                fields: vec![Init {
                    field: 0,
                    value,
                    at,
                }],
            },
            ty: Some(Type::Record(record)),
            at,
        };
        Ok(match out {
            Some(CrossOut::Scalar) | None => call,
            Some(CrossOut::Handle {
                record,
                nullable: false,
            }) => handle(
                record,
                self.internal(nazm_sema::Intrinsic::ForeignNonNull, call, Type::Int),
            ),
            Some(CrossOut::CString { nullable: false }) => {
                let checked = self.internal(nazm_sema::Intrinsic::ForeignNonNull, call, Type::Int);
                self.internal(nazm_sema::Intrinsic::CStrCopy, checked, Type::Str)
            }
            Some(
                CrossOut::Handle { nullable: true, .. } | CrossOut::CString { nullable: true },
            ) => {
                let Type::Enum(e) = ty else {
                    return defect(at, "a nullable C result that is not an `Option`");
                };
                let variant = |name: &str| {
                    self.res
                        .enums()
                        .get(e)
                        .and_then(|d| d.variants.iter().position(|v| v.name == name))
                        .and_then(|i| u32::try_from(i).ok())
                };
                let (Some(none), Some(some)) = (variant("None"), variant("Some")) else {
                    return defect(at, "an `Option` without `None` and `Some`");
                };
                let local = self.temp(Temp::Foreign, Type::Int, at);
                let read = Expr {
                    kind: ExprKind::Local(local),
                    ty: Some(Type::Int),
                    at,
                };
                let zero = Expr {
                    kind: ExprKind::Const(self.constant(Const::Int(0))),
                    ty: Some(Type::Int),
                    at,
                };
                let is_null = Expr {
                    kind: ExprKind::Compare {
                        op: cir::CmpOp::Eq,
                        operand: Type::Int,
                        law: Law::Int,
                        lhs: Box::new(read.clone()),
                        rhs: Box::new(zero),
                    },
                    ty: Some(Type::Bool),
                    at,
                };
                let value = match out {
                    Some(CrossOut::Handle { record, .. }) => handle(record, read),
                    _ => self.internal(nazm_sema::Intrinsic::CStrCopy, read, Type::Str),
                };
                let variant_of = |variant: u32, fields: Vec<Init>| Expr {
                    kind: ExprKind::Variant {
                        enumeration: e,
                        variant,
                        arity: u32::try_from(fields.len()).unwrap_or(u32::MAX),
                        fields,
                    },
                    ty: Some(ty),
                    at,
                };
                let region = |tail: Expr| Block {
                    stmts: Vec::new(),
                    tail: Some(Box::new(tail)),
                    ends: Ends::FallsThrough,
                    owns: Vec::new(),
                    at,
                };
                let some_value = variant_of(
                    some,
                    vec![Init {
                        field: 0,
                        value,
                        at,
                    }],
                );
                let none_value = variant_of(none, Vec::new());
                Expr {
                    kind: ExprKind::Block(Block {
                        stmts: vec![Stmt::Let { local, value: call }],
                        tail: Some(Box::new(Expr {
                            kind: ExprKind::If {
                                cond: Box::new(is_null),
                                then: region(none_value),
                                otherwise: region(some_value),
                            },
                            ty: Some(ty),
                            at,
                        })),
                        ends: Ends::FallsThrough,
                        owns: vec![local],
                        at,
                    }),
                    ty: Some(ty),
                    at,
                }
            }
        })
    }

    /// An integer literal of the type the checker gave it (Gate 2): an `Int` constant, or a
    /// numeric one by its canonical bits.
    fn integer(&mut self, v: nazm_sema::numeric::Wide, span: Span) -> Lowered<Expr> {
        let ty = match self.res.numeric(span) {
            Some(NumericFact::Literal(t)) => t,
            _ => Type::Int,
        };
        let kind = ty.num().unwrap_or(NumKind::I64);
        let Some(value) = nazm_sema::numeric::NumVal::int(kind, v) else {
            return defect(
                span,
                "a literal outside the range the checker accepted it in",
            );
        };
        let c = if kind == NumKind::I64 {
            #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
            Const::Int(value.bits as i64)
        } else {
            Const::Num {
                kind,
                bits: value.bits,
            }
        };
        Ok(Expr {
            kind: ExprKind::Const(self.constant(c)),
            ty: Some(ty),
            at: span,
        })
    }

    /// A numeric conversion (Gate 2). `try_convert[T](x)` is `{ let c = x; if fits(c) {
    /// Some(T(c)) } else { None } }`, so every later stage sees only exact conversions.
    fn conversion(
        &mut self,
        to: Type,
        mode: ConvertMode,
        result: Type,
        args: &[AstExpr],
        at: Span,
    ) -> Lowered<Expr> {
        let Some(arg) = args.first() else {
            return defect(at, "a conversion with no argument");
        };
        let operand = self.expr(arg)?;
        // `Text(s)` checks; `try_convert[Text](s)` asks first (Gate 2).
        if to == Type::Text && mode != ConvertMode::Try {
            return Ok(self.internal(nazm_sema::Intrinsic::TextFromStr, operand, Type::Text));
        }
        if mode != ConvertMode::Try {
            return Ok(Expr {
                kind: ExprKind::Convert {
                    mode,
                    operand: Box::new(operand),
                },
                ty: Some(to),
                at,
            });
        }
        let Some(from) = operand.ty else {
            return Ok(operand);
        };
        let Type::Enum(e) = result else {
            return defect(at, "a `try_convert` the checker gave no `Option`");
        };
        let ty = Type::Enum(e);
        let variant = |name: &str| {
            self.res
                .enums()
                .get(e)
                .and_then(|d| d.variants.iter().position(|v| v.name == name))
                .and_then(|i| u32::try_from(i).ok())
        };
        let (Some(none), Some(some)) = (variant("None"), variant("Some")) else {
            return defect(at, "an `Option` without `None` and `Some`");
        };
        let local = self.temp(Temp::Convert, from, at);
        let read = Expr {
            kind: ExprKind::Local(local),
            ty: Some(from),
            at,
        };
        let (test, converted) = if to == Type::Text {
            (
                self.internal(nazm_sema::Intrinsic::Utf8Valid, read.clone(), Type::Bool),
                self.internal(nazm_sema::Intrinsic::TextAssume, read, Type::Text),
            )
        } else {
            (
                Expr {
                    kind: ExprKind::Fits {
                        to,
                        operand: Box::new(read.clone()),
                    },
                    ty: Some(Type::Bool),
                    at,
                },
                Expr {
                    kind: ExprKind::Convert {
                        mode: ConvertMode::Exact,
                        operand: Box::new(read),
                    },
                    ty: Some(to),
                    at,
                },
            )
        };
        let variant_of = |variant: u32, fields: Vec<Init>| Expr {
            kind: ExprKind::Variant {
                enumeration: e,
                variant,
                arity: u32::try_from(fields.len()).unwrap_or(u32::MAX),
                fields,
            },
            ty: Some(ty),
            at,
        };
        let region = |tail: Expr| Block {
            stmts: Vec::new(),
            tail: Some(Box::new(tail)),
            ends: Ends::FallsThrough,
            owns: Vec::new(),
            at,
        };
        let some_value = variant_of(
            some,
            vec![Init {
                field: 0,
                value: converted,
                at,
            }],
        );
        let none_value = variant_of(none, Vec::new());
        Ok(Expr {
            kind: ExprKind::Block(Block {
                stmts: vec![Stmt::Let {
                    local,
                    value: operand,
                }],
                tail: Some(Box::new(Expr {
                    kind: ExprKind::If {
                        cond: Box::new(test),
                        then: region(some_value),
                        otherwise: region(none_value),
                    },
                    ty: Some(ty),
                    at,
                })),
                ends: Ends::FallsThrough,
                owns: vec![local],
                at,
            }),
            ty: Some(ty),
            at,
        })
    }

    /// The `defer` bodies registered since `mark` (R1b, L12), last first, each lowered where it
    /// runs: a block of statements.
    fn deferred_since(&mut self, mark: usize) -> Lowered<Vec<Stmt>> {
        let bodies: Vec<AstBlock> = self
            .defers
            .get(mark..)
            .unwrap_or_default()
            .iter()
            .rev()
            .map(|(b, _)| b.clone())
            .collect();
        // A body runs without the defers registered at or after its own, which have run.
        let saved = std::mem::take(&mut self.defers);
        let mut out = Vec::with_capacity(bodies.len());
        let mut failed = None;
        for b in &bodies {
            match self.statement_block(b, Vec::new()) {
                Ok(block) => out.push(Stmt::Block(block)),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        self.defers = saved;
        match failed {
            Some(e) => Err(e),
            None => Ok(out),
        }
    }

    /// `leave` — a `break` or `continue` — after the defers registered inside the loops it
    /// leaves (L12): those registered while `loops` loops, or more, enclosed them.
    fn through_defers(&mut self, loops: usize, leave: Stmt, at: Span) -> Lowered<Stmt> {
        let mark = self
            .defers
            .iter()
            .position(|(_, n)| *n >= loops)
            .unwrap_or(self.defers.len());
        if mark == self.defers.len() {
            return Ok(leave);
        }
        let mut stmts = self.deferred_since(mark)?;
        stmts.push(leave);
        Ok(Stmt::Block(Block {
            stmts,
            tail: None,
            ends: Ends::Transfers,
            owns: Vec::new(),
            at,
        }))
    }

    /// Leave the function with `value` (N87): a `return`, after every `ensures` clause holds of
    /// it. With none, a `return` as it always was.
    fn leave(&mut self, value: Expr, at: Span) -> Lowered<Stmt> {
        if self.ensures.is_empty() && self.defers.is_empty() {
            return Ok(Stmt::Return { value, at });
        }
        let ty = self.def.ret;
        let held = self.temp(Temp::Result, ty, at);
        let mut stmts = vec![Stmt::Let { local: held, value }];
        // Every `defer` pending in the function runs once the value is held (L12).
        stmts.extend(self.deferred_since(0)?);
        stmts.extend(self.postconditions(held)?);
        stmts.push(Stmt::Return {
            value: Expr {
                kind: ExprKind::Local(held),
                ty: Some(ty),
                at,
            },
            at,
        });
        Ok(Stmt::Block(Block {
            stmts,
            tail: None,
            ends: Ends::Transfers,
            owns: vec![held],
            at,
        }))
    }

    /// A body's tail, once every `ensures` clause holds of it (N87).
    fn ensured(&mut self, tail: Expr) -> Lowered<Expr> {
        if self.ensures.is_empty() {
            return Ok(tail);
        }
        let ty = self.def.ret;
        let at = tail.at;
        let held = self.temp(Temp::Result, ty, at);
        let mut stmts = vec![Stmt::Let {
            local: held,
            value: tail,
        }];
        stmts.extend(self.postconditions(held)?);
        Ok(Expr {
            kind: ExprKind::Block(Block {
                stmts,
                tail: Some(Box::new(Expr {
                    kind: ExprKind::Local(held),
                    ty: Some(ty),
                    at,
                })),
                ends: Ends::FallsThrough,
                owns: vec![held],
                at,
            }),
            ty: Some(ty),
            at,
        })
    }

    /// Each `ensures` clause, checked with `result` the value `held` holds.
    fn postconditions(&mut self, held: LocalId) -> Lowered<Vec<Stmt>> {
        let clauses = self.ensures.clone();
        let saved = self.result_here.replace(held);
        let mut out = Vec::with_capacity(clauses.len());
        for (e, span) in &clauses {
            let cond = self.expr(e);
            match cond {
                Ok(cond) => {
                    let check = self.contract_check(cond, 1, *span);
                    out.push(Stmt::Eval(check));
                }
                Err(d) => {
                    self.result_here = saved;
                    return Err(d);
                }
            }
        }
        self.result_here = saved;
        Ok(out)
    }

    /// A contract clause's check (N87): `contract_check(cond, which)`, failing with `N0410` for
    /// a precondition (`which` 0) or `N0411` for a postcondition (1) when `cond` is false, at
    /// the clause. Its result is `0` and is discarded.
    fn contract_check(&mut self, cond: Expr, which: i64, at: Span) -> Expr {
        let which = Expr {
            kind: ExprKind::Const(self.constant(Const::Int(which))),
            ty: Some(Type::Int),
            at,
        };
        Expr {
            kind: ExprKind::Builtin {
                op: nazm_sema::Intrinsic::ContractCheck,
                type_args: Vec::new(),
                args: vec![cond, which],
                callee_at: at,
            },
            ty: Some(Type::Int),
            at,
        }
    }

    /// One of the built-ins only this lowering makes (N85), applied to one argument.
    fn internal(&mut self, op: nazm_sema::Intrinsic, arg: Expr, ty: Type) -> Expr {
        let at = arg.at;
        Expr {
            kind: ExprKind::Builtin {
                op,
                type_args: Vec::new(),
                args: vec![arg],
                callee_at: at,
            },
            ty: Some(ty),
            at,
        }
    }

    /// A method call (N78): the receiver — `None` when it is written as the first argument,
    /// `Trait.m(recv, …)` — then the arguments, as the checker resolved them. A call on a
    /// concrete type is a call of the impl's function; one through a bound is a trait call.
    fn method_call(
        &mut self,
        receiver: Option<&AstExpr>,
        at: Span,
        args: &[AstExpr],
        span: Span,
    ) -> Lowered<Expr> {
        // A method call by name (R1, 5b.15) is the ordinary call the checker read it as.
        if self.res.named_method(at) {
            let mut all: Vec<AstExpr> = receiver.into_iter().cloned().collect();
            all.extend(args.iter().cloned());
            return self.call(at, &all, span);
        }
        let mut lowered = Vec::with_capacity(args.len() + 1);
        if let Some(r) = receiver {
            lowered.push(self.expr(r)?);
        }
        lowered.extend(self.exprs(args)?);
        let Some(checked) = self.res.checked_call(at) else {
            return defect(at, "a method call the checker did not record");
        };
        let ty = checked.ret;
        let kind = if let Some(tc) = self.res.traits().call(at) {
            ExprKind::CallTrait {
                trait_id: tc.trait_id,
                method: tc.method,
                self_ty: tc.self_ty,
                args: lowered,
                callee_at: at,
            }
        } else if let Callee::Function(callee) = checked.callee {
            ExprKind::Call {
                callee,
                // A trait's default body is called at the receiver's type (L11).
                type_args: self
                    .res
                    .type_args(at)
                    .map(<[Type]>::to_vec)
                    .unwrap_or_default(),
                args: lowered,
                callee_at: at,
            }
        } else {
            return defect(
                at,
                "a method call that is neither a function's nor a trait's",
            );
        };
        Ok(Expr {
            kind,
            ty: Some(ty),
            at: span,
        })
    }

    /// `match`, in either position. The arms keep their written order; each binds the payload
    /// fields it names into locals its body region owns.
    fn matching(
        &mut self,
        scrutinee: &AstExpr,
        arms: &[AstArm],
        at: Span,
        value: bool,
    ) -> Lowered<Match> {
        let scrutinee = self.expr(scrutinee)?;
        let mut lowered = Vec::with_capacity(arms.len());
        let mut enumeration = match scrutinee.ty {
            Some(Type::Enum(id)) => Some(id),
            _ => None,
        };
        for arm in arms {
            let nazm_syntax::Pattern::Variant(vp) = &arm.pattern else {
                return defect(
                    arm.span,
                    "a plain `match` arm that is not a variant pattern",
                );
            };
            let Some(to) = self.res.variant(vp.variant_span) else {
                return defect(
                    vp.variant_span,
                    "an arm whose variant the checker did not resolve",
                );
            };
            enumeration.get_or_insert(to.enumeration);
            let mut bindings = Vec::new();
            for f in &vp.fields {
                // A discard binds nothing; the field is still the scrutinee's, and goes with it.
                let nazm_syntax::Pattern::Bind { span: bound, .. } = &f.pattern else {
                    continue;
                };
                let Some(p) = self.res.payload(f.name_span) else {
                    return defect(f.name_span, "a pattern field the checker did not resolve");
                };
                bindings.push(Binding {
                    field: p.field,
                    local: self.local_at(*bound)?,
                    at: *bound,
                });
            }
            let seed: Vec<LocalId> = bindings.iter().map(|b| b.local).collect();
            let body = match (&arm.body, value) {
                (AstExpr::Block(b), true) => self.value_block(b, seed)?,
                (AstExpr::Block(b), false) => self.statement_block(b, seed)?,
                (e, value) => self.expr_region(e, seed, value)?,
            };
            lowered.push(Arm {
                variant: to.variant,
                bindings,
                body,
                at: arm.span,
            });
        }
        let Some(enumeration) = enumeration else {
            return defect(at, "a `match` on something that is not an enum");
        };
        Ok(Match {
            scrutinee,
            enumeration,
            arms: lowered,
            origin: MatchOrigin::Written,
            at,
        })
    }

    /// A `match` with any pattern (R1a, `docs/spec.md`, *Patterns*), as
    /// `{ let scrutinee = e; let arm = <the first arm whose pattern matches>; <arm `arm`> }`:
    /// each pattern's test is a `Bool` of the plain matches and comparisons it means, and each
    /// arm, once chosen, binds its names out of the scrutinee and runs its body — written once.
    fn matching_general(
        &mut self,
        scrutinee: &AstExpr,
        arms: &[AstArm],
        at: Span,
        value: bool,
    ) -> Lowered<Block> {
        let sval = self.expr(scrutinee)?;
        let Some(sty) = sval.ty else {
            return defect(at, "a `match` scrutinee the checker gave no type");
        };
        self.open(Vec::new());
        let lowered = self.matching_general_in(sval, sty, arms, at, value);
        let owns = self.close();
        let (stmts, tail) = lowered?;
        let ends = if tail.as_ref().is_some_and(|t| t.ty.is_none())
            || stmts.last().is_some_and(|s| s.ends() == Ends::Transfers)
        {
            Ends::Transfers
        } else {
            Ends::FallsThrough
        };
        Ok(Block {
            stmts,
            tail: tail.map(Box::new),
            ends,
            owns,
            at,
        })
    }

    #[allow(clippy::type_complexity)]
    fn matching_general_in(
        &mut self,
        sval: Expr,
        sty: Type,
        arms: &[AstArm],
        at: Span,
        value: bool,
    ) -> Lowered<(Vec<Stmt>, Option<Expr>)> {
        let s = self.temp(Temp::Scrutinee, sty, at);
        self.bind(s);
        let mut stmts = vec![Stmt::Let {
            local: s,
            value: sval,
        }];
        let read = Expr {
            kind: ExprKind::Local(s),
            ty: Some(sty),
            at,
        };
        // Which arm: the first whose test holds; the last needs none — exhaustiveness.
        let k = self.temp(Temp::Arm, Type::Int, at);
        self.bind(k);
        let n = arms.len();
        let mut choose = self.int_const(i64::try_from(n.saturating_sub(1)).unwrap_or(0), at);
        for (i, arm) in arms.iter().enumerate().rev().skip(1) {
            let test = self.pattern_test(&arm.pattern, &read)?;
            let here = self.int_const(i64::try_from(i).unwrap_or(0), at);
            choose = self.value_if(test, here, choose, Type::Int, at);
        }
        stmts.push(Stmt::Let {
            local: k,
            value: choose,
        });
        // Each arm, chosen by `arm`: its bindings, then its body.
        let mut regions = Vec::with_capacity(n);
        for arm in arms {
            regions.push(self.arm_region(arm, &read, value)?);
        }
        let read_k = Expr {
            kind: ExprKind::Local(k),
            ty: Some(Type::Int),
            at,
        };
        let mut chosen = regions.pop().ok_or(Defect {
            at,
            what: "a `match` with no arms".to_owned(),
        })?;
        for (i, region) in regions.into_iter().enumerate().rev() {
            let here = self.int_const(i64::try_from(i).unwrap_or(0), at);
            let cond = Expr {
                kind: ExprKind::Compare {
                    op: cir::CmpOp::Eq,
                    operand: Type::Int,
                    law: Law::Int,
                    lhs: Box::new(read_k.clone()),
                    rhs: Box::new(here),
                },
                ty: Some(Type::Bool),
                at,
            };
            chosen = if value {
                let ty = region.ty().or(chosen.ty());
                Block {
                    stmts: Vec::new(),
                    tail: Some(Box::new(Expr {
                        kind: ExprKind::If {
                            cond: Box::new(cond),
                            then: region,
                            otherwise: chosen,
                        },
                        ty,
                        at,
                    })),
                    ends: Ends::FallsThrough,
                    owns: Vec::new(),
                    at,
                }
            } else {
                let stmt = Stmt::If {
                    cond,
                    then: region,
                    otherwise: Some(chosen),
                    at,
                };
                let ends = stmt.ends();
                Block {
                    stmts: vec![stmt],
                    tail: None,
                    ends,
                    owns: Vec::new(),
                    at,
                }
            };
        }
        if value {
            let ty = chosen.ty();
            let tail = Expr {
                kind: ExprKind::Block(chosen),
                ty,
                at,
            };
            Ok((stmts, Some(tail)))
        } else {
            stmts.push(Stmt::Block(chosen));
            Ok((stmts, None))
        }
    }

    /// A string literal's value as an expression.
    fn str_const(&mut self, v: &str, at: Span) -> Expr {
        Expr {
            kind: ExprKind::Const(self.constant(Const::Str(v.as_bytes().to_vec()))),
            ty: Some(Type::Str),
            at,
        }
    }

    /// A built-in returning a `Str`, at `at`.
    fn str_builtin(
        op: nazm_sema::Intrinsic,
        type_args: Vec<Type>,
        args: Vec<Expr>,
        at: Span,
    ) -> Expr {
        Expr {
            kind: ExprKind::Builtin {
                op,
                type_args,
                args,
                callee_at: at,
            },
            ty: Some(Type::Str),
            at,
        }
    }

    /// An interpolation (R1a, L4, `docs/spec.md`, *String interpolation*): each piece's text,
    /// joined left to right by `str_concat`, so each hole is evaluated once and in order.
    fn interpolation(&mut self, parts: &[nazm_syntax::InterpPart], at: Span) -> Lowered<Expr> {
        let mut acc: Option<Expr> = None;
        for p in parts {
            let piece = match p {
                nazm_syntax::InterpPart::Text(t) => self.str_const(t, at),
                nazm_syntax::InterpPart::Hole { expr, spec, span } => {
                    self.hole_text(expr, *spec, *span)?
                }
            };
            acc = Some(match (acc, Self::integer_text(&piece)) {
                (None, _) => piece,
                // An integer's digits written straight after the text before them (R1, 5b.9):
                // one string, not one for the number and another for the two together.
                (Some(a), Some((t, v))) => {
                    Self::str_builtin(nazm_sema::Intrinsic::StrConcatNum, vec![t], vec![a, v], at)
                }
                (Some(a), None) => Self::str_builtin(
                    nazm_sema::Intrinsic::StrConcat,
                    Vec::new(),
                    vec![a, piece],
                    at,
                ),
            });
        }
        Ok(acc.unwrap_or_else(|| self.str_const("", at)))
    }

    /// `num_to_str[T](v)` for an integer `T` of at most 64 bits: its type and its operand.
    fn integer_text(piece: &Expr) -> Option<(Type, Expr)> {
        let ExprKind::Builtin {
            op: nazm_sema::Intrinsic::NumToStr,
            type_args,
            args,
            ..
        } = &piece.kind
        else {
            return None;
        };
        let t = *type_args.first()?;
        let narrow = match t {
            Type::Int => true,
            Type::Num(k) => !k.is_float() && k.bits() <= 64,
            _ => false,
        };
        match (narrow, args.as_slice()) {
            (true, [v]) => Some((t, v.clone())),
            _ => None,
        }
    }

    /// One hole's text (L4): its `show()` where the checker resolved one at its braces, and
    /// otherwise the value's own text; then its padding.
    fn hole_text(
        &mut self,
        expr: &AstExpr,
        spec: nazm_syntax::FormatSpec,
        at: Span,
    ) -> Lowered<Expr> {
        use nazm_sema::Intrinsic;
        let (text, numeric) = if self.res.callee(at).is_some() {
            (self.method_call(Some(expr), at, &[], at)?, false)
        } else {
            let v = self.expr(expr)?;
            match v.ty {
                Some(Type::Str) => (v, false),
                Some(Type::Bool) => {
                    let yes = self.str_const("true", at);
                    let no = self.str_const("false", at);
                    (self.value_if(v, yes, no, Type::Str, at), false)
                }
                Some(t @ Type::Num(k)) if k.is_float() && spec.precision.is_some() => {
                    let digits = self.int_const(i64::from(spec.precision.unwrap_or(0)), at);
                    (
                        Self::str_builtin(Intrinsic::FloatToStrFixed, vec![t], vec![v, digits], at),
                        true,
                    )
                }
                Some(t @ (Type::Int | Type::Num(_))) => (
                    Self::str_builtin(Intrinsic::NumToStr, vec![t], vec![v], at),
                    true,
                ),
                _ => return defect(at, "a hole the checker gave no text"),
            }
        };
        let Some(width) = spec.width else {
            return Ok(text);
        };
        let start = match spec.align {
            Some(nazm_syntax::Align::Right) => true,
            Some(nazm_syntax::Align::Left) => false,
            None => numeric,
        };
        let width = self.int_const(i64::from(width), at);
        let fill = self.str_const(" ", at);
        let op = if start {
            Intrinsic::StrPadStart
        } else {
            Intrinsic::StrPadEnd
        };
        Ok(Self::str_builtin(
            op,
            Vec::new(),
            vec![text, width, fill],
            at,
        ))
    }

    fn int_const(&mut self, v: i64, at: Span) -> Expr {
        Expr {
            kind: ExprKind::Const(self.constant(Const::Int(v))),
            ty: Some(Type::Int),
            at,
        }
    }

    fn bool_const(&mut self, v: bool, at: Span) -> Expr {
        Expr {
            kind: ExprKind::Const(self.constant(Const::Bool(v))),
            ty: Some(Type::Bool),
            at,
        }
    }

    /// `if cond { then } else { otherwise }` as a value of `ty`.
    fn value_if(&mut self, cond: Expr, then: Expr, otherwise: Expr, ty: Type, at: Span) -> Expr {
        let region = |e: Expr| Block {
            stmts: Vec::new(),
            ends: Ends::FallsThrough,
            tail: Some(Box::new(e)),
            owns: Vec::new(),
            at,
        };
        Expr {
            kind: ExprKind::If {
                cond: Box::new(cond),
                then: region(then),
                otherwise: region(otherwise),
            },
            ty: Some(ty),
            at,
        }
    }

    /// Whether `v` matches `p`: a `Bool` of plain matches and comparisons.
    fn pattern_test(&mut self, p: &nazm_syntax::Pattern, v: &Expr) -> Lowered<Expr> {
        use nazm_syntax::Pattern as P;
        let at = p.span();
        match p {
            P::Wildcard { .. } | P::Bind { .. } => Ok(self.bool_const(true, at)),
            // A tuple (L5): every element matches, read out of the record by position.
            P::Tuple { elems, .. } => {
                let mut acc: Option<Expr> = None;
                for (i, sub) in elems.iter().enumerate().rev() {
                    let element = self.tuple_element(v, i, at)?;
                    let test = self.pattern_test(sub, &element)?;
                    acc = Some(match acc {
                        None => test,
                        Some(rest) => {
                            let no = self.bool_const(false, at);
                            self.value_if(test, rest, no, Type::Bool, at)
                        }
                    });
                }
                Ok(acc.unwrap_or_else(|| self.bool_const(true, at)))
            }
            P::Literal(e) => {
                let lit = self.expr(e)?;
                let Some(ty) = v.ty else {
                    return defect(at, "a literal pattern against a value of no type");
                };
                let Some(law) = Law::of(cir::CmpOp::Eq, ty) else {
                    return defect(at, "a literal pattern against a type with no equality");
                };
                Ok(Expr {
                    kind: ExprKind::Compare {
                        op: cir::CmpOp::Eq,
                        operand: ty,
                        law,
                        lhs: Box::new(v.clone()),
                        rhs: Box::new(lit),
                    },
                    ty: Some(Type::Bool),
                    at,
                })
            }
            P::Or { alts, .. } => {
                let mut tests = Vec::with_capacity(alts.len());
                for alt in alts {
                    tests.push(self.pattern_test(alt, v)?);
                }
                let mut acc = tests.pop().ok_or(Defect {
                    at,
                    what: "an or-pattern with no alternatives".to_owned(),
                })?;
                while let Some(t) = tests.pop() {
                    let yes = self.bool_const(true, at);
                    acc = self.value_if(t, yes, acc, Type::Bool, at);
                }
                Ok(acc)
            }
            P::Variant(vp) => {
                let (id, chosen, n) = self.variant_of(vp, v)?;
                let mut arms_out = Vec::with_capacity(n);
                for w in 0..n {
                    let wv = u32::try_from(w).unwrap_or(u32::MAX);
                    self.open(Vec::new());
                    let mut bindings = Vec::new();
                    let mut test: Option<Expr> = None;
                    if wv == chosen {
                        // Each field whose pattern tests something, read into a local.
                        let mut parts = Vec::new();
                        for f in &vp.fields {
                            if matches!(f.pattern, P::Wildcard { .. } | P::Bind { .. }) {
                                continue;
                            }
                            let (field, local, ty) = self.payload_local(id, chosen, f)?;
                            bindings.push(Binding {
                                field,
                                local,
                                at: f.name_span,
                            });
                            let read = Expr {
                                kind: ExprKind::Local(local),
                                ty: Some(ty),
                                at: f.name_span,
                            };
                            parts.push(self.pattern_test(&f.pattern, &read)?);
                        }
                        while let Some(part) = parts.pop() {
                            test = Some(match test {
                                None => part,
                                Some(rest) => {
                                    let no = self.bool_const(false, at);
                                    self.value_if(part, rest, no, Type::Bool, at)
                                }
                            });
                        }
                    }
                    let _ = self.close();
                    let tail = match test {
                        Some(t) => t,
                        None => self.bool_const(wv == chosen, at),
                    };
                    arms_out.push(Arm {
                        variant: wv,
                        body: Block {
                            stmts: Vec::new(),
                            tail: Some(Box::new(tail)),
                            ends: Ends::FallsThrough,
                            owns: bindings.iter().map(|b| b.local).collect(),
                            at,
                        },
                        bindings,
                        at,
                    });
                }
                Ok(Expr {
                    kind: ExprKind::Match(Box::new(Match {
                        scrutinee: v.clone(),
                        enumeration: id,
                        arms: arms_out,
                        origin: MatchOrigin::Written,
                        at,
                    })),
                    ty: Some(Type::Bool),
                    at,
                })
            }
        }
    }

    /// A constant's value (Gate 2), written where it is used: a scalar as a literal, and a
    /// record, enum value, tuple or array (L8) as its construction, built here, of literals.
    fn const_value(&mut self, value: &nazm_sema::numeric::ConstValue, at: Span) -> Lowered<Expr> {
        use nazm_sema::numeric::ConstValue as V;
        let ty = value.ty();
        let inits = |this: &mut Self, fields: &[V]| -> Lowered<Vec<Init>> {
            fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    Ok(Init {
                        field: u32::try_from(i).unwrap_or(u32::MAX),
                        at,
                        value: this.const_value(f, at)?,
                    })
                })
                .collect()
        };
        let kind = match value {
            V::Num(n) if n.kind == NumKind::I64 =>
            {
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                ExprKind::Const(self.constant(Const::Int(n.bits as i64)))
            }
            V::Num(n) => ExprKind::Const(self.constant(Const::Num {
                kind: n.kind,
                bits: n.bits,
            })),
            V::Bool(b) => ExprKind::Const(self.constant(Const::Bool(*b))),
            V::Str(s) => ExprKind::Const(self.constant(Const::Str(s.clone()))),
            V::Record { ty, fields } => {
                let Type::Record(record) = *ty else {
                    return defect(at, "a record constant of another type");
                };
                ExprKind::Record {
                    record,
                    arity: u32::try_from(fields.len()).unwrap_or(u32::MAX),
                    fields: inits(self, fields)?,
                }
            }
            V::Variant {
                ty,
                variant,
                fields,
            } => {
                let Type::Enum(enumeration) = *ty else {
                    return defect(at, "an enum constant of another type");
                };
                ExprKind::Variant {
                    enumeration,
                    variant: *variant,
                    arity: u32::try_from(fields.len()).unwrap_or(u32::MAX),
                    fields: inits(self, fields)?,
                }
            }
            V::Array { elems, .. } => ExprKind::Array {
                elems: elems
                    .iter()
                    .map(|e| self.const_value(e, at))
                    .collect::<Lowered<Vec<_>>>()?,
            },
        };
        Ok(Expr {
            kind,
            ty: Some(ty),
            at,
        })
    }

    /// Element `i` of the tuple `v` (L5): a projection of its record.
    fn tuple_element(&self, v: &Expr, i: usize, at: Span) -> Lowered<Expr> {
        let Some(t @ Type::Record(record)) = v.ty else {
            return defect(at, "a tuple pattern against a value that is not a tuple");
        };
        let Some(ty) = self
            .res
            .types()
            .tuple_elements(t)
            .and_then(|e| e.get(i).copied())
        else {
            return defect(at, "a tuple pattern with more elements than its tuple");
        };
        Ok(Expr {
            kind: ExprKind::Field {
                base: Box::new(v.clone()),
                record,
                field: u32::try_from(i).unwrap_or(u32::MAX),
            },
            ty: Some(ty),
            at,
        })
    }

    /// The enum a variant pattern matches `v` of, the variant it names, and how many the
    /// enum has.
    fn variant_of(
        &mut self,
        vp: &nazm_syntax::VariantPattern,
        v: &Expr,
    ) -> Lowered<(nazm_sema::EnumId, u32, usize)> {
        let Some(Type::Enum(id)) = v.ty else {
            return defect(
                vp.ty_span,
                "a variant pattern against a value that is not an enum",
            );
        };
        let Some(to) = self.res.variant(vp.variant_span) else {
            return defect(
                vp.variant_span,
                "a variant pattern the checker did not resolve",
            );
        };
        let n = self.res.enums().get(id).map_or(0, |d| d.variants.len());
        Ok((id, to.variant, n))
    }

    /// A payload field of `variant` read into a local of its own, for a test or an extraction.
    fn payload_local(
        &mut self,
        id: nazm_sema::EnumId,
        variant: u32,
        f: &nazm_syntax::PatternField,
    ) -> Lowered<(u32, LocalId, Type)> {
        let Some(p) = self.res.payload(f.name_span) else {
            return defect(f.name_span, "a pattern field the checker did not resolve");
        };
        let Some(ty) = self
            .res
            .enums()
            .get(id)
            .and_then(|d| d.variants.get(variant as usize))
            .and_then(|v| v.fields.get(p.field as usize))
            .map(|fd| fd.ty)
        else {
            return defect(f.name_span, "a payload field with no type");
        };
        let local = self.temp(Temp::Payload, ty, f.name_span);
        Ok((p.field, local, ty))
    }

    /// The value `name` (bound at `bound`) takes when `v` matches `p`, out of `p`'s structure;
    /// `None` when `p` does not bind it. A branch exhaustiveness rules out ends the process
    /// without a value, so it needs none.
    fn extract(&mut self, p: &nazm_syntax::Pattern, v: &Expr, name: &str) -> Lowered<Option<Expr>> {
        use nazm_syntax::Pattern as P;
        let at = p.span();
        Ok(match p {
            P::Bind { name: n, .. } if n == name => Some(v.clone()),
            P::Bind { .. } | P::Wildcard { .. } | P::Literal(_) => None,
            P::Tuple { elems, .. } => {
                let Some((i, sub)) = elems
                    .iter()
                    .enumerate()
                    .find(|(_, e)| e.bindings().iter().any(|(n, _)| *n == name))
                else {
                    return Ok(None);
                };
                let element = self.tuple_element(v, i, at)?;
                self.extract(sub, &element, name)?
            }
            P::Or { alts, .. } => {
                let mut options = Vec::new();
                for alt in alts {
                    if let Some(x) = self.extract(alt, v, name)? {
                        options.push((alt, x));
                    }
                }
                let Some((_, mut acc)) = options.pop() else {
                    return Ok(None);
                };
                let ty = acc.ty;
                while let Some((alt, x)) = options.pop() {
                    let test = self.pattern_test(alt, v)?;
                    let region = |e: Expr| Block {
                        stmts: Vec::new(),
                        ends: Ends::FallsThrough,
                        tail: Some(Box::new(e)),
                        owns: Vec::new(),
                        at,
                    };
                    acc = Expr {
                        kind: ExprKind::If {
                            cond: Box::new(test),
                            then: region(x),
                            otherwise: region(acc),
                        },
                        ty,
                        at,
                    };
                }
                Some(acc)
            }
            P::Variant(vp) => {
                let Some(f) = vp
                    .fields
                    .iter()
                    .find(|f| f.pattern.bindings().iter().any(|(n, _)| *n == name))
                else {
                    return Ok(None);
                };
                let (id, chosen, n) = self.variant_of(vp, v)?;
                let mut arms_out = Vec::with_capacity(n);
                let mut ty = None;
                for w in 0..n {
                    let wv = u32::try_from(w).unwrap_or(u32::MAX);
                    if wv == chosen {
                        let (field, local, fty) = self.payload_local(id, chosen, f)?;
                        let read = Expr {
                            kind: ExprKind::Local(local),
                            ty: Some(fty),
                            at: f.name_span,
                        };
                        let Some(x) = self.extract(&f.pattern, &read, name)? else {
                            return defect(f.name_span, "a binding its field's pattern lost");
                        };
                        ty = x.ty;
                        arms_out.push(Arm {
                            variant: wv,
                            bindings: vec![Binding {
                                field,
                                local,
                                at: f.name_span,
                            }],
                            body: Block {
                                stmts: Vec::new(),
                                tail: Some(Box::new(x)),
                                ends: Ends::FallsThrough,
                                owns: vec![local],
                                at,
                            },
                            at,
                        });
                    } else {
                        let code = self.int_const(70, at);
                        arms_out.push(Arm {
                            variant: wv,
                            bindings: Vec::new(),
                            body: Block {
                                stmts: vec![Stmt::Eval(Expr {
                                    kind: ExprKind::Builtin {
                                        op: nazm_sema::Intrinsic::ExitWith,
                                        type_args: Vec::new(),
                                        args: vec![code],
                                        callee_at: at,
                                    },
                                    ty: Some(Type::Int),
                                    at,
                                })],
                                tail: None,
                                ends: Ends::Transfers,
                                owns: Vec::new(),
                                at,
                            },
                            at,
                        });
                    }
                }
                Some(Expr {
                    kind: ExprKind::Match(Box::new(Match {
                        scrutinee: v.clone(),
                        enumeration: id,
                        arms: arms_out,
                        origin: MatchOrigin::Written,
                        at,
                    })),
                    ty,
                    at,
                })
            }
        })
    }

    /// One arm of a general `match`: its bindings, each extracted from the scrutinee, then its
    /// body — a value region or a statement region.
    fn arm_region(&mut self, arm: &AstArm, read: &Expr, value: bool) -> Lowered<Block> {
        let mut lets = Vec::new();
        let mut seed = Vec::new();
        for (name, bound) in arm.pattern.bindings() {
            let local = self.local_at(bound)?;
            let Some(x) = self.extract(&arm.pattern, read, name)? else {
                return defect(bound, "a binding its pattern does not bind");
            };
            lets.push(Stmt::Let { local, value: x });
            seed.push(local);
        }
        let mut body = match (&arm.body, value) {
            (AstExpr::Block(b), true) => self.value_block(b, seed)?,
            (AstExpr::Block(b), false) => self.statement_block(b, seed)?,
            (e, value) => self.expr_region(e, seed, value)?,
        };
        body.stmts.splice(0..0, lets);
        Ok(body)
    }

    /// `operand?`, as the `match` it means:
    ///
    /// ```text
    /// match operand {
    ///     Result.Ok(value: ok)   => ok,
    ///     Result.Err(error: err) => return <the function's Result>.Err(error: err),
    /// }
    /// ```
    ///
    /// Which enum the operand is, which variant is which and which `Result` the function
    /// returns are the checker's [`Propagation`](nazm_sema::Propagation), and nothing here
    /// asks whether a type is called `Result`. The two bindings are this IR's temporaries,
    /// and the error path is an ordinary `return` — so no backend has a `?` to implement.
    fn propagate(&mut self, operand: &AstExpr, at: Span) -> Lowered<Expr> {
        let Some(p) = self.res.propagation(at) else {
            return defect(at, "a `?` the checker did not resolve");
        };
        let scrutinee = self.expr(operand)?;
        let payload = |e: nazm_sema::EnumId, v: u32| {
            self.res
                .enums()
                .get(e)
                .and_then(|d| d.variants.get(v as usize))
                .and_then(|v| v.fields.first())
                .map(|f| f.ty)
        };
        let (Some(ok_ty), Some(err_ty)) = (payload(p.operand, p.ok), payload(p.operand, p.err))
        else {
            return defect(at, "a `?` on a `Result` whose variants carry no payload");
        };
        let into_arity = self
            .res
            .enums()
            .get(p.into)
            .and_then(|d| d.variants.get(p.into_err as usize))
            .map_or(0, |v| v.fields.len());
        let ok = self.temp(Temp::Ok, ok_ty, at);
        let err = self.temp(Temp::Err, err_ty, at);
        let local = |l: LocalId, ty: Type| Expr {
            kind: ExprKind::Local(l),
            ty: Some(ty),
            at,
        };
        // The error, in the enclosing function's `Result` — a different instance from the
        // operand's whenever the success types differ — and the `return` that leaves with it.
        let rebuilt = Expr {
            kind: ExprKind::Variant {
                enumeration: p.into,
                variant: p.into_err,
                arity: u32::try_from(into_arity).unwrap_or(u32::MAX),
                fields: vec![Init {
                    field: 0,
                    value: local(err, err_ty),
                    at,
                }],
            },
            ty: Some(Type::Enum(p.into)),
            at,
        };
        let leave = self.leave(rebuilt, at)?;
        let arm = |variant: u32, local: LocalId, stmts: Vec<Stmt>, tail: Option<Box<Expr>>| {
            let ends = if stmts.iter().any(|s| s.ends() == Ends::Transfers) {
                Ends::Transfers
            } else {
                Ends::FallsThrough
            };
            Arm {
                variant,
                bindings: vec![Binding {
                    field: 0,
                    local,
                    at,
                }],
                body: Block {
                    stmts,
                    tail,
                    ends,
                    owns: vec![local],
                    at,
                },
                at,
            }
        };
        let ok_arm = arm(p.ok, ok, Vec::new(), Some(Box::new(local(ok, ok_ty))));
        let err_arm = arm(p.err, err, vec![leave], None);
        Ok(Expr {
            kind: ExprKind::Match(Box::new(Match {
                scrutinee,
                enumeration: p.operand,
                arms: vec![ok_arm, err_arm],
                origin: MatchOrigin::Propagate,
                at,
            })),
            ty: Some(ok_ty),
            at,
        })
    }
}

/// A foreign function's parameters and result as the C function has them (N85): what crosses
/// as a pointer is an `Int`, everything else as declared.
fn raw_foreign(res: &Resolution, def: &FnDef) -> (Vec<Type>, Type) {
    use crate::check::{foreign_param, foreign_result, CrossIn, CrossOut};
    let params = def
        .params
        .iter()
        .map(|t| match foreign_param(res, *t) {
            Some(CrossIn::Handle(_) | CrossIn::Callback) => Type::Int,
            _ => *t,
        })
        .collect();
    let ret = match foreign_result(res, checked_option(res, def.ret), def.ret) {
        Some(CrossOut::Scalar) | None => def.ret,
        Some(_) => Type::Int,
    };
    (params, ret)
}

/// The enum definition a foreign result's type applies, taken as the prelude's `Option`: the
/// checker accepted the declaration only if it is (`check_foreign`), so lowering trusts it.
fn checked_option(res: &Resolution, t: Type) -> Option<nazm_sema::EnumId> {
    match res.types().definition_of(t) {
        Some(nazm_sema::TypeNode::Enum(e)) => Some(e),
        _ => None,
    }
}

fn arithmetic(op: BinOp) -> Option<cir::ArithOp> {
    match op {
        BinOp::Add => Some(cir::ArithOp::Add),
        BinOp::Sub => Some(cir::ArithOp::Sub),
        BinOp::Mul => Some(cir::ArithOp::Mul),
        BinOp::Div => Some(cir::ArithOp::Div),
        BinOp::Rem => Some(cir::ArithOp::Rem),
        BinOp::Eq
        | BinOp::NotEq
        | BinOp::Lt
        | BinOp::LtEq
        | BinOp::Gt
        | BinOp::GtEq
        | BinOp::And
        | BinOp::Or
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::BitXor
        | BinOp::Shl
        | BinOp::Shr => None,
    }
}

/// The bit operator a binary operator is (Gate 2), if it is one.
fn bitwise(op: BinOp) -> Option<cir::BitOp> {
    match op {
        BinOp::BitAnd => Some(cir::BitOp::And),
        BinOp::BitOr => Some(cir::BitOp::Or),
        BinOp::BitXor => Some(cir::BitOp::Xor),
        BinOp::Shl => Some(cir::BitOp::Shl),
        BinOp::Shr => Some(cir::BitOp::Shr),
        _ => None,
    }
}

fn comparison(op: BinOp) -> cir::CmpOp {
    match op {
        BinOp::Eq => cir::CmpOp::Eq,
        BinOp::NotEq => cir::CmpOp::NotEq,
        BinOp::Lt => cir::CmpOp::Lt,
        BinOp::LtEq => cir::CmpOp::LtEq,
        BinOp::Gt => cir::CmpOp::Gt,
        // Unreachable for an arithmetic operator, which [`arithmetic`] took first.
        // Unreachable for a logical operator, which the expression lowering took first.
        BinOp::GtEq
        | BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Rem
        | BinOp::And
        | BinOp::Or
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::BitXor
        | BinOp::Shl
        | BinOp::Shr => cir::CmpOp::GtEq,
    }
}

/// Whether an arm is a plain variant pattern — each payload field a name or `_` — which the
/// `match` before R1a took, and which lowers to one Core IR `match` arm.
fn plain_arm(arm: &AstArm) -> bool {
    matches!(&arm.pattern, nazm_syntax::Pattern::Variant(v)
    if v.fields.iter().all(|f| matches!(
        f.pattern,
        nazm_syntax::Pattern::Bind { .. } | nazm_syntax::Pattern::Wildcard { .. }
    )))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn checked(src: &str) -> (Program, Resolution) {
        let (program, errors) = nazm_syntax::parse_source(src);
        assert!(errors.is_empty(), "{errors:?}");
        let (res, errors) = crate::check(&program, &nazm_sema::ModuleGraph::single());
        assert!(errors.is_empty(), "{errors:?}");
        (program, res)
    }

    #[test]
    fn a_lowering_that_disagrees_with_the_checker_is_a_compiler_defect() {
        // The one way to reach N0900 from outside is for the compiler to be wrong. Lowering
        // one program's tree against another program's resolution makes it so on purpose:
        // the names in the tree were never resolved.
        let (_, res) = checked("fn main() -> Int { let x = 1; x }");
        let (other, _) = checked("fn main() -> Int {\n    let y = 2;\n    let z = y;\n    z\n}");
        let defects = lower(&other, &res).expect_err("a defect");
        assert!(!defects.is_empty());
        assert!(defects.iter().all(|d| d.code == codes::COMPILER_DEFECT));
        assert!(defects.iter().all(|d| d
            .help
            .as_deref()
            .is_some_and(|h| h.contains("bug in the compiler"))));
    }

    #[test]
    fn a_checked_program_lowers_and_verifies() {
        let (program, res) = checked(
            "fn f(n: Int) -> Int { if n > 0 { n } else { 0 - n } }\nfn main() -> Int { f(3) }",
        );
        let core = lower(&program, &res).expect("Core IR");
        assert_eq!(core.functions.len(), 2);
        assert!(cir::verify(&core, &res).is_empty());
    }
}
