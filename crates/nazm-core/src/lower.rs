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
use nazm_sema::Type;
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
    // method defined. One the checker refused is not here, and nothing calls through it.
    let impls = res
        .traits()
        .impls()
        .filter(|(id, d)| res.traits().find(d.trait_id, d.self_ty) == Some(*id))
        .filter_map(|(_, d)| {
            let methods: Option<Vec<FnId>> = d.methods.iter().copied().collect();
            methods.map(|methods| cir::ImplRow {
                trait_id: d.trait_id,
                self_ty: d.self_ty,
                methods,
            })
        })
        .collect();
    let core = cir::Program {
        functions,
        entry: res.entry(),
        impls,
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
            .chain(def.locals.iter().enumerate().map(|(i, l)| Local {
                ty: l.ty,
                name: l.name.clone(),
                origin: if i < params {
                    LocalOrigin::Param
                } else {
                    LocalOrigin::Binding { mutable: l.mutable }
                },
                at: l.span,
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
        Ok(cir::Function {
            def: self.id,
            module: def.module,
            name: def.name.clone(),
            type_params: 0,
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
        let body = self.value_block(body, Vec::new())?;
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
            export: def.export.clone(),
            captures: Vec::new(),
            closure: false,
        })
    }

    // --- identities --------------------------------------------------------------------

    fn local_at(&self, span: Span) -> Lowered<LocalId> {
        match self.res.var(span) {
            Some(slot) if ((slot + self.shift) as usize) < self.locals.len() => {
                Ok(LocalId(slot + self.shift))
            }
            _ => defect(span, "a name the checker bound to no slot"),
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
        let lowered = self.statements(&b.statements).and_then(|(stmts, ends)| {
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
            Ok((stmts, ends, tail))
        });
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
            });
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
                let value = self.expr(&l.value)?;
                let local = self.local_at(l.name_span)?;
                self.bind(local);
                Stmt::Let { local, value }
            }
            AstStmt::Assign(a) => {
                let value = self.expr(&a.value)?;
                let local = self.local_at(a.place.name_span)?;
                let mut path = Vec::with_capacity(a.place.path.len());
                for step in &a.place.path {
                    let Some(to) = self.res.field(step.name_span) else {
                        return defect(step.name_span, "a field the checker did not resolve");
                    };
                    path.push(Step {
                        record: to.record,
                        field: to.field,
                        at: step.name_span,
                    });
                }
                Stmt::Assign {
                    local,
                    path,
                    value,
                    at: a.span,
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
            AstStmt::Return(r) => Stmt::Return {
                value: self.expr(&r.value)?,
                at: r.span,
            },
            AstStmt::Break { span } => Stmt::Break {
                target: self.target("break", *span)?,
                at: *span,
            },
            AstStmt::Continue { span } => Stmt::Continue {
                target: self.target("continue", *span)?,
                at: *span,
            },
            AstStmt::Expr(e) => self.effect(e)?,
        })
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
            } => Stmt::Match(Box::new(self.matching(scrutinee, arms, *span, false)?)),
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
            AstExpr::Int { value, span } => node(
                ExprKind::Const(self.constant(Const::Int(*value))),
                Some(Type::Int),
                *span,
            ),
            AstExpr::Bool { value, span } => node(
                ExprKind::Const(self.constant(Const::Bool(*value))),
                Some(Type::Bool),
                *span,
            ),
            AstExpr::Str { value, span } => node(
                ExprKind::Const(self.constant(Const::Str(value.clone().into_bytes()))),
                Some(Type::Str),
                *span,
            ),
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
            AstExpr::Neg {
                operand, op_span, ..
            } => node(
                ExprKind::Neg(Box::new(self.expr(operand)?)),
                Some(Type::Int),
                *op_span,
            ),
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
                match arithmetic(*op) {
                    Some(op) => node(
                        ExprKind::Arith {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        Some(Type::Int),
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
            } => {
                let m = self.matching(scrutinee, arms, *span, true)?;
                // The type its arms produce; checking made them agree, and one that transfers
                // produces none.
                let ty = m.arms.iter().rev().find_map(|a| a.body.ty());
                node(ExprKind::Match(Box::new(m)), ty, *span)
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
                type_args: Vec::new(),
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
            let Some(to) = self.res.variant(arm.variant_span) else {
                return defect(
                    arm.variant_span,
                    "an arm whose variant the checker did not resolve",
                );
            };
            enumeration.get_or_insert(to.enumeration);
            let mut bindings = Vec::new();
            for f in &arm.fields {
                // A discard binds nothing; the field is still the scrutinee's, and goes with it.
                if f.binding.is_none() {
                    continue;
                }
                let Some(p) = self.res.payload(f.name_span) else {
                    return defect(f.name_span, "a pattern field the checker did not resolve");
                };
                bindings.push(Binding {
                    field: p.field,
                    local: self.local_at(f.binding_span)?,
                    at: f.binding_span,
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
        let leave = Stmt::Return { value: rebuilt, at };
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
        | BinOp::Or => None,
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
        | BinOp::Or => cir::CmpOp::GtEq,
    }
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
