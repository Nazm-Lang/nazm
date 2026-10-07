//! The invariants of `docs/architecture.md` §7.41, checked.
//!
//! Run on every lowering. A violation is a compiler defect — the checker accepted the program
//! and the lowering produced something no consumer may assume — so it is reported as one, by
//! function and by what broke, and nothing downstream runs.
//!
//! What is checked: identities in range, every `break` and `continue` naming a loop whose body
//! encloses it, value regions having a tail exactly when they fall through and statement
//! regions never having one, every region's completion agreeing with its statements, and the
//! types that must agree — a binding's value and its local, an assignment and its place, a
//! `return` and the signature, a condition and `Bool`, arithmetic and `Int`, a call's
//! arguments and a non-generic callee's parameters, an initialiser and its field. An
//! expression that never produces a value agrees with any type, as the checker says.

use nazm_sema::resolve::Resolution;
use nazm_sema::Type;

use crate::ir::{Block, Ends, Expr, ExprKind, Function, LoopId, Match, Program, Stmt};

/// Every violation in `program`, each naming its function. Empty for a well-formed program.
#[must_use]
pub fn verify(program: &Program, res: &Resolution) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, f) in program.functions.iter().enumerate() {
        if f.def.index() != i {
            problems.push(format!(
                "function {i} (`{}`) is at the index of another definition",
                f.name
            ));
        }
        let mut v = Verifier {
            f,
            res,
            program,
            loops: Vec::new(),
            problems: &mut problems,
        };
        v.function();
    }
    if let Some(e) = program.entry {
        if program.function(e).is_none() {
            problems.push("the entry names no function".to_owned());
        }
    }
    problems
}

fn fits(e: &Expr, want: Type) -> bool {
    e.ty.is_none_or(|t| t == want)
}

struct Verifier<'a> {
    f: &'a Function,
    res: &'a Resolution,
    program: &'a Program,
    /// The loops whose **bodies** enclose the point being checked, innermost last.
    loops: Vec<LoopId>,
    problems: &'a mut Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Region {
    Value,
    Statement,
}

impl Verifier<'_> {
    fn bad(&mut self, what: impl std::fmt::Display) {
        self.problems.push(format!("`{}`: {what}", self.f.name));
    }

    fn function(&mut self) {
        let f = self.f;
        if f.params as usize > f.locals.len() {
            self.bad("more parameters than locals");
        }
        if f.foreign.is_some() {
            // No body to walk: a foreign function's region is empty by construction (N42).
            if !f.body.stmts.is_empty() || f.body.tail.is_some() {
                self.bad("a foreign function with a body");
            }
            return;
        }
        self.region(&f.body, Region::Value);
        if let Some(t) = &f.body.tail {
            if !self.returns(t) {
                self.bad("the body's value is not the declared result type");
            }
        }
    }

    /// Whether a result fits the declared one up to effects, as an argument does (N79): the
    /// checker accepts a function value whose effects are a subset of the declared result's.
    fn returns(&self, e: &Expr) -> bool {
        e.ty.is_none_or(|t| self.res.types().same_shape(t, self.f.ret))
    }

    fn local_ty(&mut self, id: crate::ir::LocalId) -> Option<Type> {
        let t = self.f.local(id).map(|l| l.ty);
        if t.is_none() {
            self.bad(format!("local {} does not exist", id.0));
        }
        t
    }

    fn region(&mut self, b: &Block, kind: Region) {
        for l in &b.owns {
            let _ = self.local_ty(*l);
        }
        let mut ends = Ends::FallsThrough;
        for (i, s) in b.stmts.iter().enumerate() {
            if ends == Ends::Transfers {
                self.bad(format!("statement {i} of a region follows a transfer"));
            }
            self.stmt(s);
            if s.ends() == Ends::Transfers {
                ends = Ends::Transfers;
            }
        }
        if ends != b.ends {
            self.bad("a region's completion disagrees with its statements");
        }
        match (kind, &b.tail) {
            (Region::Value, Some(t)) => {
                if b.ends == Ends::Transfers {
                    self.bad("a value region that transfers has a tail");
                }
                self.expr(t);
            }
            (Region::Value, None) => {
                if b.ends == Ends::FallsThrough {
                    self.bad("a value region that falls through has no tail");
                }
            }
            (Region::Statement, Some(_)) => self.bad("a statement region has a tail"),
            (Region::Statement, None) => {}
        }
    }

    fn target(&mut self, t: LoopId, word: &str) {
        if t.0 >= self.f.loops {
            self.bad(format!("`{word}` names loop {}, which does not exist", t.0));
        } else if !self.loops.contains(&t) {
            self.bad(format!(
                "`{word}` names loop {}, which does not enclose it",
                t.0
            ));
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { local, value } => {
                self.expr(value);
                if let Some(t) = self.local_ty(*local) {
                    if !fits(value, t) {
                        self.bad(format!(
                            "`let` of local {} with a value of another type",
                            local.0
                        ));
                    }
                }
            }
            Stmt::Assign {
                local, path, value, ..
            } => {
                self.expr(value);
                let mut target = self.local_ty(*local);
                for step in path {
                    let step = match step {
                        crate::ir::PathStep::Field(s) => s,
                        crate::ir::PathStep::Index { index, len, .. } => {
                            self.expr(index);
                            let info = match target {
                                Some(Type::Array(a)) => self.res.types().array_info(a),
                                _ => None,
                            };
                            match info {
                                Some((e, n)) if n == *len && fits(index, Type::Int) => {
                                    target = Some(e);
                                }
                                _ => {
                                    self.bad("an assignment path indexes something that is not its array");
                                    target = None;
                                }
                            }
                            continue;
                        }
                    };
                    if target != Some(Type::Record(step.record)) {
                        self.bad("an assignment path steps into something that is not its record");
                    }
                    target = self
                        .res
                        .records()
                        .get(step.record)
                        .and_then(|d| d.fields.get(step.field as usize))
                        .map(|f| f.ty);
                    if target.is_none() {
                        self.bad("an assignment path names a field that does not exist");
                    }
                }
                if let Some(t) = target {
                    if !fits(value, t) {
                        self.bad("an assignment of a value of another type");
                    }
                }
            }
            Stmt::Eval(e) => self.expr(e),
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.cond(cond);
                self.region(then, Region::Statement);
                if let Some(o) = otherwise {
                    self.region(o, Region::Statement);
                }
            }
            Stmt::Match(m) => self.matching(m, Region::Statement),
            Stmt::Loop { id, cond, body, .. } => {
                if id.0 >= self.f.loops {
                    self.bad(format!("loop {} is outside the function's loops", id.0));
                }
                // The condition is outside this loop: a transfer there names an enclosing one.
                self.cond(cond);
                self.loops.push(*id);
                self.region(body, Region::Statement);
                self.loops.pop();
            }
            Stmt::Block(b) | Stmt::Scope { body: b, .. } => self.region(b, Region::Statement),
            Stmt::Spawn { callee, args, .. } => {
                for a in args {
                    self.expr(a);
                }
                self.arguments(*callee, &[], args);
            }
            Stmt::Return { value, .. } => {
                self.expr(value);
                if !self.returns(value) {
                    self.bad("a `return` of a value that is not the declared result type");
                }
            }
            Stmt::Break { target, .. } => self.target(*target, "break"),
            Stmt::Continue { target, .. } => self.target(*target, "continue"),
        }
    }

    fn cond(&mut self, c: &Expr) {
        self.expr(c);
        if !fits(c, Type::Bool) {
            self.bad("a condition that is not `Bool`");
        }
    }

    fn arguments(&mut self, callee: nazm_sema::FnId, type_args: &[Type], args: &[Expr]) {
        let Some(g) = self.program.function(callee) else {
            self.bad("a call to a function that does not exist");
            return;
        };
        if args.len() != g.params as usize {
            self.bad(format!(
                "a call to `{}` with the wrong number of arguments",
                g.name
            ));
            return;
        }
        // A generic callee's parameters mention its own type parameters; what they become here
        // is the checker's substitution, which this does not repeat.
        if g.type_params > 0 || !type_args.is_empty() {
            return;
        }
        // Up to effects (N51): a callee's effect parameter is bound per call and makes no
        // instance, so its `fn(…) ! { E }` parameter takes the caller's `fn(…) ! { io }`.
        for (a, p) in args.iter().zip(g.param_locals()) {
            if !a.ty.is_none_or(|t| self.res.types().same_shape(t, p.ty)) {
                self.bad(format!("an argument to `{}` of another type", g.name));
            }
        }
    }

    fn matching(&mut self, m: &Match, kind: Region) {
        self.expr(&m.scrutinee);
        if let Some(t) = m.scrutinee.ty {
            if t != Type::Enum(m.enumeration) {
                self.bad("a `match` whose scrutinee is not its enum");
            }
        }
        let arity = |v: u32| {
            self.res
                .enums()
                .get(m.enumeration)
                .and_then(|d| d.variants.get(v as usize))
                .map(|v| v.fields.len())
        };
        let variants = self
            .res
            .enums()
            .get(m.enumeration)
            .map_or(0, |d| d.variants.len());
        let mut seen = vec![false; variants];
        for a in &m.arms {
            match seen.get_mut(a.variant as usize) {
                Some(s) if !*s => *s = true,
                Some(_) => self.bad("two arms of one `match` name one variant"),
                None => self.bad("an arm names a variant its enum does not have"),
            }
            let n = arity(a.variant).unwrap_or(0);
            for b in &a.bindings {
                if b.field as usize >= n {
                    self.bad("an arm binds a payload field its variant does not have");
                }
                let _ = self.local_ty(b.local);
            }
            self.region(&a.body, kind);
        }
        if seen.iter().any(|s| !s) {
            self.bad("a `match` that does not cover every variant");
        }
    }

    fn inits(
        &mut self,
        arity: u32,
        fields: &[crate::ir::Init],
        ty_of: impl Fn(u32) -> Option<Type>,
    ) {
        let mut seen = vec![false; arity as usize];
        for i in fields {
            self.expr(&i.value);
            match seen.get_mut(i.field as usize) {
                Some(s) if !*s => *s = true,
                _ => self.bad("an initialiser names a field twice, or one that does not exist"),
            }
            if let Some(t) = ty_of(i.field) {
                if !fits(&i.value, t) {
                    self.bad("an initialiser of another type than its field");
                }
            }
        }
        if seen.iter().any(|s| !s) {
            self.bad("a construction that leaves a field uninitialised");
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Const(c) => match self.f.constant(*c) {
                Some(k) if e.ty == Some(k.ty()) => {}
                Some(_) => self.bad("a constant of another type than its node"),
                None => self.bad(format!("constant {} does not exist", c.0)),
            },
            ExprKind::Local(l) => {
                if let Some(t) = self.local_ty(*l) {
                    if e.ty != Some(t) {
                        self.bad(format!("a read of local {} typed as something else", l.0));
                    }
                }
            }
            ExprKind::Neg(x) => {
                self.expr(x);
                let numeric = e.ty.is_some_and(|t| t.num().is_some());
                if !numeric || e.ty.is_some_and(|t| !fits(x, t)) {
                    self.bad("negation of something that is not a number of its own type");
                }
            }
            ExprKind::Arith { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
                let numeric = e.ty.is_some_and(|t| t.num().is_some());
                if !numeric || e.ty.is_some_and(|t| !fits(lhs, t) || !fits(rhs, t)) {
                    self.bad("arithmetic on operands that are not numbers of its own type");
                }
            }
            ExprKind::Bits { op, lhs, rhs } => {
                self.expr(lhs);
                self.expr(rhs);
                let integer = e.ty.is_some_and(Type::is_integer);
                let amount = matches!(op, crate::ir::BitOp::Shl | crate::ir::BitOp::Shr);
                let rhs_ok = if amount {
                    fits(rhs, Type::Int)
                } else {
                    e.ty.is_some_and(|t| fits(rhs, t))
                };
                if !integer || e.ty.is_some_and(|t| !fits(lhs, t)) || !rhs_ok {
                    self.bad("a bit operation on operands that are not integers of its type");
                }
            }
            ExprKind::Complement(x) => {
                self.expr(x);
                if !e.ty.is_some_and(Type::is_integer) || e.ty.is_some_and(|t| !fits(x, t)) {
                    self.bad("a complement of something that is not an integer of its type");
                }
            }
            ExprKind::Convert { mode, operand } => {
                self.expr(operand);
                let from_numeric = operand.ty.is_none_or(|t| t.num().is_some());
                let to_numeric = e.ty.is_some_and(|t| t.num().is_some());
                let wrap_ok = *mode != nazm_sema::ConvertMode::Wrap
                    || (e.ty.is_some_and(Type::is_integer)
                        && operand.ty.is_none_or(Type::is_integer));
                if !from_numeric || !to_numeric || !wrap_ok || *mode == nazm_sema::ConvertMode::Try
                {
                    self.bad("a conversion between types it cannot convert");
                }
            }
            ExprKind::Array { elems } => {
                let ok = match e.ty {
                    Some(Type::Array(a)) => self.res.types().array_info(a).is_some_and(|(t, n)| {
                        n as usize == elems.len() && elems.iter().all(|x| fits(x, t))
                    }),
                    _ => false,
                };
                for x in elems {
                    self.expr(x);
                }
                if !ok {
                    self.bad("an array whose elements are not its element type and length");
                }
            }
            ExprKind::ArrayRepeat { value, count } => {
                self.expr(value);
                let ok = match e.ty {
                    Some(Type::Array(a)) => self
                        .res
                        .types()
                        .array_info(a)
                        .is_some_and(|(t, n)| n == *count && fits(value, t)),
                    _ => false,
                };
                if !ok {
                    self.bad("a repeated array of another element type or length");
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
                let elem = match base.ty {
                    Some(Type::Array(a)) => self.res.types().array_info(a).map(|(t, _)| t),
                    _ => None,
                };
                if elem.is_none() || e.ty != elem || !fits(index, Type::Int) {
                    self.bad("an index into something that is not an array, or by a non-`Int`");
                }
            }
            ExprKind::Fits { to, operand } => {
                self.expr(operand);
                if to.num().is_none()
                    || !operand.ty.is_none_or(|t| t.num().is_some())
                    || e.ty != Some(Type::Bool)
                {
                    self.bad("a conversion test between types it cannot convert");
                }
            }
            ExprKind::Compare {
                op,
                operand,
                law,
                lhs,
                rhs,
            } => {
                self.expr(lhs);
                self.expr(rhs);
                if !fits(lhs, *operand) || !fits(rhs, *operand) || e.ty != Some(Type::Bool) {
                    self.bad("a comparison whose operands are not its operand type");
                }
                if crate::ir::Law::of(*op, *operand) != Some(*law) {
                    self.bad("a comparison under a law its operand type does not have");
                }
            }
            ExprKind::Call {
                callee,
                type_args,
                args,
                ..
            } => {
                for a in args {
                    self.expr(a);
                }
                self.arguments(*callee, type_args, args);
                if e.ty.is_none() {
                    self.bad("a call with no result type");
                }
            }
            ExprKind::Builtin { op, args, .. } => {
                for a in args {
                    self.expr(a);
                }
                if args.len() != op.params().len() {
                    self.bad(format!(
                        "`{}` with the wrong number of arguments",
                        op.name()
                    ));
                }
                if e.ty.is_none() {
                    self.bad("a built-in with no result type");
                }
            }
            ExprKind::Record {
                record,
                arity,
                fields,
            } => {
                let def = self.res.records().get(*record);
                if def.map(|d| d.fields.len()) != Some(*arity as usize) {
                    self.bad("a record construction whose arity is not its record's");
                }
                let types: Vec<Type> = def
                    .map(|d| d.fields.iter().map(|f| f.ty).collect())
                    .unwrap_or_default();
                self.inits(*arity, fields, |i| types.get(i as usize).copied());
                if e.ty != Some(Type::Record(*record)) {
                    self.bad("a record construction typed as something else");
                }
            }
            ExprKind::Variant {
                enumeration,
                variant,
                arity,
                fields,
            } => {
                let def = self
                    .res
                    .enums()
                    .get(*enumeration)
                    .and_then(|d| d.variants.get(*variant as usize));
                if def.map(|d| d.fields.len()) != Some(*arity as usize) {
                    self.bad("a variant construction whose arity is not its variant's");
                }
                let types: Vec<Type> = def
                    .map(|d| d.fields.iter().map(|f| f.ty).collect())
                    .unwrap_or_default();
                self.inits(*arity, fields, |i| types.get(i as usize).copied());
                if e.ty != Some(Type::Enum(*enumeration)) {
                    self.bad("a variant construction typed as something else");
                }
            }
            ExprKind::Field {
                base,
                record,
                field,
            } => {
                self.expr(base);
                if !fits(base, Type::Record(*record)) {
                    self.bad("a projection from something that is not its record");
                }
                let t = self
                    .res
                    .records()
                    .get(*record)
                    .and_then(|d| d.fields.get(*field as usize))
                    .map(|f| f.ty);
                if t.is_none() || e.ty != t {
                    self.bad("a projection typed as something other than its field");
                }
            }
            ExprKind::Match(m) => {
                self.matching(m, Region::Value);
                let produced: Vec<Type> = m.arms.iter().filter_map(|a| a.body.ty()).collect();
                if produced.iter().any(|t| Some(*t) != e.ty)
                    || (produced.is_empty() && e.ty.is_some())
                {
                    self.bad("a value `match` typed as something its arms do not produce");
                }
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.cond(cond);
                self.region(then, Region::Value);
                self.region(otherwise, Region::Value);
                let produced: Vec<Type> =
                    [then.ty(), otherwise.ty()].into_iter().flatten().collect();
                if produced.iter().any(|t| Some(*t) != e.ty)
                    || (produced.is_empty() && e.ty.is_some())
                {
                    self.bad("a value `if` typed as something its branches do not produce");
                }
            }
            ExprKind::Block(b) => {
                self.region(b, Region::Value);
                if b.ty() != e.ty {
                    self.bad("a value block typed as something its tail does not produce");
                }
            }
            // N50: a closure's body is a closure definition, its captures are locals here,
            // and an indirect call's callee is a function value with as many parameters as
            // it is given arguments.
            ExprKind::Closure { func, captures } => {
                if self.res.function(*func).is_none_or(|d| d.closure.is_none()) {
                    self.bad("a closure whose body is not a closure definition");
                }
                for l in captures {
                    if self.local_ty(*l).is_none() {
                        self.bad(format!(
                            "a closure captures local {}, which does not exist",
                            l.0
                        ));
                    }
                }
                if !matches!(e.ty, Some(Type::Fn(_))) {
                    self.bad("a closure that is not a function value");
                }
            }
            ExprKind::FnValue(func) => {
                if self.res.function(*func).is_none() {
                    self.bad("a function value of a function that does not exist");
                }
                if !matches!(e.ty, Some(Type::Fn(_))) {
                    self.bad("a function value that is not one");
                }
            }
            ExprKind::CallTrait {
                trait_id,
                method,
                self_ty,
                args,
                ..
            } => {
                for a in args {
                    self.expr(a);
                }
                let arity = self
                    .res
                    .traits()
                    .get(*trait_id)
                    .and_then(|d| d.methods.get(*method as usize))
                    .map(|m| m.params.len() + 1);
                if arity != Some(args.len()) {
                    self.bad("a trait call with no such method, or the wrong number of arguments");
                }
                if args.first().and_then(|r| r.ty) != Some(*self_ty) {
                    self.bad("a trait call whose receiver is not of its `Self` type");
                }
                if !matches!(self_ty, Type::Param(_)) {
                    self.bad("a trait call on a concrete type, which is an ordinary call");
                }
            }
            ExprKind::CallIndirect { callee, args, .. } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
                match callee.ty {
                    Some(Type::Fn(f)) => {
                        let arity = self.res.types().fn_signature(f).map(|s| s.params.len());
                        if arity != Some(args.len()) {
                            self.bad("an indirect call with the wrong number of arguments");
                        }
                    }
                    _ => {
                        self.bad("an indirect call through something that is not a function value")
                    }
                }
            }
        }
    }
}
