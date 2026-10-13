//! Statements and expressions: what each Core IR form does when it runs.

use super::arith::{
    arith, as_bool, as_num, compare, derived_hash, from_num, index_outside, no_such_place,
    num_failure,
};
use super::intrinsic::bytes_text_intrinsic;
use super::value::{EnumVal, FuncVal, Rec, Str, Value};
use super::*;

impl Interpreter {
    /// A derived `Hash`'s method (L6): `hash` is the type's walk, `same` its `==`. Out of line,
    /// so the evaluator's frame — which every nested call pays for — does not grow by it.
    #[inline(never)]
    fn derived_hash_call(
        &self,
        method: u32,
        values: &[Value],
        at: Span,
    ) -> Result<Flow, RuntimeError> {
        let answer = match (self.core.derived_hash, values) {
            (Some(d), [v]) if method == d.hash => derived_hash(v, &self.core.orders).map(|h| {
                Value::Num(nazm_sema::numeric::NumVal::wrapped(
                    nazm_sema::NumKind::U64,
                    h,
                ))
            }),
            (Some(d), [a, b]) if method == d.same => {
                compare(cir::CmpOp::Eq, a, b, at, &self.core.orders).ok()
            }
            _ => None,
        };
        match answer {
            Some(v) => Ok(Flow::Fell(Some(v))),
            None => Err(fail(
                codes::COMPILER_DEFECT,
                at,
                "a derived hash of a value that has none",
            )),
        }
    }

    /// `match`, in either position.
    ///
    /// The scrutinee is evaluated **once**, into a value this expression owns, and the arm
    /// runs with the payload bound into locals its region owns. `docs/spec.md` specifies
    /// value evaluation rather than a borrow of the caller's storage, so `match x { … }`
    /// leaves `x` untouched and `match make() { … }` has a clear owner for the temporary.
    ///
    /// The bindings are **clones of the payload values**, which is the same operation a
    /// projection out of a record performs and the same rule: what comes out holds its own
    /// reference to whatever backs it, so an arm may return it and it survives the
    /// scrutinee. The scrutinee is dropped when this function returns, after the arm's
    /// value has been established — which is the ordering the whole milestone turns on.
    ///
    /// A `?` is one of these since N39 — its `Err` arm an ordinary `return` — so its error
    /// path travels out through loops, scopes and blocks exactly as a written `return` does,
    /// and a scope it leaves joins its tasks on the way.
    fn matching(
        &mut self,
        f: &cir::Function,
        m: &cir::Match,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let value = take!(self, f, &m.scrutinee, env);
        let Value::Enum(subject) = value else {
            return Err(fail(
                codes::MATCH_ON_NON_ENUM,
                m.scrutinee.at,
                "this is not an enum",
            ));
        };
        // The variant selects the arm. Checking has established that exactly one arm names
        // each variant, so the first match is the only match and there is no fallthrough to
        // express — and the arm names its variant by the same index the value carries.
        let Some(arm) = m.arms.iter().find(|a| a.variant == subject.variant) else {
            // Unreachable for a checked program: `N0343` refuses a `match` that leaves a
            // variant uncovered. Reported rather than assumed away, and deliberately not
            // as a language-visible trap — it says the compiler is wrong, not the program.
            return Err(RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(
                        codes::NON_EXHAUSTIVE_MATCH,
                        m.at,
                        "no arm covers this value's variant",
                    )
                    .help("this is a bug in the compiler: exhaustiveness is checked"),
                ),
            });
        };
        for b in &arm.bindings {
            let Some(v) = subject.field(b.field) else {
                return Err(fail(
                    codes::UNKNOWN_FIELD,
                    b.at,
                    "this variant has no such payload field",
                ));
            };
            if let Some(slot) = env.get_mut(b.local.index()) {
                *slot = Some(v.clone());
            }
        }
        // `subject` is dropped here, after the arm's value has been established — which is
        // what keeps a returned payload valid. A `_` field is in `subject` like any other
        // and goes with it; declining to name a value is not declining to own it.
        self.block(f, &arm.body, env)
    }

    pub(super) fn stmt(
        &mut self,
        f: &cir::Function,
        stmt: &Stmt,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        // What a statement's own value is: nothing. A flow still travels outward — an `if`
        // in statement position may still `return` or `break`.
        let discard = |flow: Flow| match flow {
            Flow::Fell(_) => Flow::Fell(None),
            other => other,
        };
        match stmt {
            Stmt::Let { local, value } => {
                let v = take!(self, f, value, env);
                if let Some(slot) = env.get_mut(local.index()) {
                    *slot = Some(v);
                }
                Ok(Flow::Fell(None))
            }
            Stmt::Assign {
                local,
                path,
                value,
                at,
            } => self.assign(f, *local, path, value, *at, env),
            Stmt::Eval(e) => Ok(discard(self.eval(f, e, env)?)),
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                let c = take!(self, f, cond, env);
                let flow = if as_bool(&c, cond)? {
                    self.block(f, then, env)?
                } else if let Some(b) = otherwise {
                    self.block(f, b, env)?
                } else {
                    Flow::Fell(None)
                };
                Ok(discard(flow))
            }
            Stmt::Match(m) => Ok(discard(self.matching(f, m, env)?)),
            // A loop consumes its own `break` and `continue`, but a `return` inside one
            // leaves the function and must travel through — as must a `break` naming an
            // enclosing loop.
            Stmt::Loop { id, cond, body, at } => self.while_loop(f, *id, cond, body, *at, env),
            Stmt::Block(b) => Ok(discard(self.block(f, b, env)?)),
            // Through `take!`: `return (if c { return 1; } else { 2 });` is legal, and
            // the inner `return` is the one that wins.
            Stmt::Return { value, .. } => Ok(Flow::Return(take!(self, f, value, env))),
            Stmt::Break { target, .. } => Ok(Flow::Break(*target)),
            Stmt::Continue { target, .. } => Ok(Flow::Continue(*target)),
            Stmt::Scope { body, .. } => {
                // Tasks run while the body does. The closing brace joins every one of
                // them, and joins them *before* propagating a failure of the body —
                // returning early would leave tasks running past the scope that promised
                // to contain them, which is the one guarantee this construct exists for.
                self.pending.push(Vec::new());
                let flow = self.block(f, body, env);
                let joined = Self::join_all(self.pending.pop().unwrap_or_default());
                // The body's failure wins: a task that failed because the body stopped
                // feeding it is a consequence, not the cause.
                let flow = flow?;
                joined?;
                // A `scope` produces nothing; a `break` or `return` inside one still
                // travels outward, after the join.
                Ok(discard(flow))
            }
            Stmt::Spawn {
                callee, args, at, ..
            } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(take!(self, f, a, env));
                }
                // Checked before starting anything: a task with no scope to join it is
                // the one case where starting it would be unrecoverable. Unreachable —
                // the checker reports `N0320`.
                if self.pending.is_empty() {
                    return Err(fail(
                        codes::SPAWN_OUTSIDE_SCOPE,
                        *at,
                        "`spawn` reached the interpreter with no scope to join it",
                    ));
                }
                let task = self.start_task(*callee, values, *at)?;
                if let Some(scope) = self.pending.last_mut() {
                    scope.push(task);
                }
                Ok(Flow::Fell(None))
            }
        }
    }

    /// `local = str_append(local, tail);` (R1b, D2): the tail, then the binding's string taken
    /// out of it, so that the binding's reference is the one the append weighs — as the
    /// compiled program's count of one is. A tail that shares the string's storage (`s = s + s`)
    /// is copied out first, because the compiled program reads it in place.
    fn append(
        &mut self,
        f: &cir::Function,
        local: cir::LocalId,
        tail: &cir::Expr,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let Value::Str(t) = take!(self, f, tail, env) else {
            return Err(no_value(tail));
        };
        let t = t.bytes().to_vec();
        match env.get_mut(local.index()).and_then(Option::take) {
            Some(Value::Str(s)) => {
                if let Some(slot) = env.get_mut(local.index()) {
                    *slot = Some(Value::Str(s.append(&t)));
                }
                Ok(Flow::Fell(None))
            }
            _ => Err(no_value(tail)),
        }
    }

    /// `local.path = value;`.
    ///
    /// **The new value first, always.** `docs/spec.md` requires a replacement to be secured
    /// before the old one is let go, so that `p.name = p.name` and a failing right-hand side
    /// are both safe. Evaluating here rather than after the walk is what makes that true: by
    /// the time anything is overwritten, `v` already holds its own reference to whatever
    /// backs it. The old value at the end of the path is dropped by the store, which is where
    /// a `Str` backing, a sequence or a channel loses the reference this record held — the
    /// same event, at the same moment, as the compiled program's release.
    ///
    /// Iterative: a path is a list, and walking it with a loop keeps the depth of a
    /// projection out of the host's stack.
    fn assign(
        &mut self,
        f: &cir::Function,
        local: cir::LocalId,
        path: &[cir::PathStep],
        value: &cir::Expr,
        at: Span,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        if path.is_empty() {
            if let Some(tail) = appended(local, value) {
                return self.append(f, local, tail, env);
            }
        }
        let v = take!(self, f, value, env);
        // The value, then each index, outermost first (Gate 2), before anything is written.
        let mut indexes = Vec::new();
        for step in path {
            if let cir::PathStep::Index { index, .. } = step {
                match take!(self, f, index, env) {
                    Value::Int(i) => indexes.push(i),
                    _ => return Err(no_such_place(at)),
                }
            }
        }
        let mut indexes = indexes.into_iter();
        let Some(Some(mut slot)) = env.get_mut(local.index()).map(Option::as_mut) else {
            return Err(fail(
                codes::UNDEFINED_VARIABLE,
                at,
                format!(
                    "`{}` is not defined",
                    f.local(local).map_or("a local", |l| l.name.as_str())
                ),
            ));
        };
        for step in path {
            // Unreachable if checking ran: the checker resolved this path and refused
            // anything it could not. A diagnostic rather than a panic, so a checker bug
            // surfaces as a report.
            let step = match step {
                cir::PathStep::Field(s) => s,
                cir::PathStep::Index { at: bracket, .. } => {
                    let i = indexes.next().unwrap_or(-1);
                    let Value::Array(items) = slot else {
                        return Err(no_such_place(at));
                    };
                    let n = items.len();
                    let Some(next) = usize::try_from(i).ok().and_then(|i| items.get_mut(i)) else {
                        return Err(index_outside(*bracket, i, n));
                    };
                    slot = next;
                    continue;
                }
            };
            let Value::Record(r) = slot else {
                return Err(no_such_place(at));
            };
            let Some(next) = r.field_mut(step.field) else {
                return Err(no_such_place(at));
            };
            slot = next;
        }
        *slot = v;
        Ok(Flow::Fell(None))
    }

    fn while_loop(
        &mut self,
        f: &cir::Function,
        id: cir::LoopId,
        cond: &cir::Expr,
        body: &Block,
        at: Span,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        loop {
            // `take!`, because a condition is an expression like any other: `while (if c
            // { return 1; } else { true })` leaves the function from the condition — and a
            // `break` there names the enclosing loop, which is where it travels.
            let c = take!(self, f, cond, env);
            if !as_bool(&c, cond)? {
                return Ok(Flow::Fell(None));
            }

            let Some(left) = self.iterations_left.checked_sub(1) else {
                return Err(RuntimeError {
                    diagnostic: Box::new(
                        Diagnostic::error(
                            codes::ITERATION_LIMIT,
                            at,
                            "this program exceeded the interpreter's iteration limit",
                        )
                        .help(
                            "Nazm does not bound iteration; `nazm run` does, so a \
                         non-terminating program reports instead of hanging. Raise it \
                         with `--max-iterations`, or check the loop's condition ever \
                         becomes false",
                        ),
                    ),
                });
            };
            self.iterations_left = left;

            match self.block(f, body, env)? {
                Flow::Fell(_) => {}
                Flow::Continue(t) if t == id => {}
                Flow::Break(t) if t == id => return Ok(Flow::Fell(None)),
                // A `return` inside a loop leaves the function, not just the loop; a
                // transfer naming an enclosing loop leaves this one on its way there.
                other => return Ok(other),
            }
        }
    }

    /// Evaluate an expression, keeping both its value and any control flow it produces.
    ///
    /// The two cannot be separated, which is why this returns a [`Flow`] rather than a
    /// [`Value`]. `if c { 1 } else { continue; }` in tail position both *is* the block's
    /// value on one path and leaves the loop on the other; `let x = if c { return 1; }
    /// else { 2 };` is the same shape one level in. Every sub-expression here takes its
    /// operands through `take!`, so a flow raised anywhere inside travels straight out
    /// instead of being mistaken for a value that never arrives.
    pub(super) fn eval(
        &mut self,
        f: &cir::Function,
        e: &cir::Expr,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let value = match &e.kind {
            ExprKind::Const(c) => match f.constant(*c) {
                Some(cir::Const::Int(v)) => Value::Int(*v),
                Some(cir::Const::Bool(v)) => Value::Bool(*v),
                // A literal is **immortal**: the compiled program puts it in a constant and
                // the interpreter must not pretend it allocated one, or the two would
                // disagree about what a program with no allocation did.
                Some(cir::Const::Str(v)) => Value::Str(Str::immortal(v.clone())),
                Some(cir::Const::Num { kind, bits }) => from_num(NumVal {
                    kind: *kind,
                    bits: *bits,
                }),
                // A static atomic (Gate 3): the run's one cell for it.
                Some(cir::Const::Cell { module, name }) => {
                    match self.cells.get(&(*module, name.clone())) {
                        Some(cell) => Value::Atomic(Arc::clone(cell)),
                        None => return unreachable_flow(e.at),
                    }
                }
                None => {
                    return Err(fail(
                        codes::TYPE_MISMATCH,
                        e.at,
                        "this literal is not in its function's constants",
                    ))
                }
            },

            ExprKind::Local(l) => match env.get(l.index()).and_then(Option::as_ref) {
                Some(v) => v.clone(),
                // Unreachable if checking ran. A diagnostic rather than a panic, so a
                // checker bug surfaces as a report.
                None => {
                    return Err(fail(
                        codes::UNDEFINED_VARIABLE,
                        e.at,
                        format!(
                            "`{}` is not defined",
                            f.local(*l).map_or("a local", |l| l.name.as_str())
                        ),
                    ))
                }
            },

            ExprKind::Neg(operand) => match take!(self, f, operand, env) {
                // `-i64::MIN` has no representable result, so negation overflows exactly
                // once — and it is the value people reach for when testing edges.
                Value::Int(v) => v.checked_neg().map(Value::Int).ok_or_else(|| {
                    fail(
                        codes::INTEGER_OVERFLOW,
                        e.at,
                        "this negation overflows `Int`",
                    )
                })?,
                Value::Num(v) => numeric::neg(v)
                    .map(from_num)
                    .map_err(|err| num_failure(err, e.at, "negation", v.kind))?,
                other => {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        format!("`-` applies to `Int`, but this is `{other}`"),
                    ))
                }
            },

            ExprKind::Arith { op, lhs, rhs } => {
                // Left to right, as the spec fixes — and `take!` preserves that: a flow
                // raised by the left operand leaves before the right one is evaluated.
                let l = take!(self, f, lhs, env);
                let r = take!(self, f, rhs, env);
                arith(*op, &l, &r, e.at)?
            }

            ExprKind::Compare { op, lhs, rhs, .. } => {
                let l = take!(self, f, lhs, env);
                let r = take!(self, f, rhs, env);
                compare(*op, &l, &r, e.at, &self.core.orders)?
            }

            // Gate 2's numeric forms, through the shared semantics.
            ExprKind::Bits { op, lhs, rhs } => {
                let l = take!(self, f, lhs, env);
                let r = take!(self, f, rhs, env);
                let (Some(a), Some(b)) = (as_num(&l), as_num(&r)) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a bit operation on a non-integer",
                    ));
                };
                numeric::bit(op.numeric(), a, b)
                    .map(from_num)
                    .map_err(|err| num_failure(err, e.at, "shift", a.kind))?
            }
            ExprKind::Complement(x) => {
                let v = take!(self, f, x, env);
                let Some(a) = as_num(&v) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a complement of a non-integer",
                    ));
                };
                from_num(numeric::complement(a))
            }
            ExprKind::Convert { mode, operand } => {
                let v = take!(self, f, operand, env);
                let (Some(a), Some(to)) = (as_num(&v), e.ty.and_then(nazm_sema::Type::num)) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a conversion of a non-number",
                    ));
                };
                numeric::convert(to, *mode, a)
                    .map(from_num)
                    .map_err(|err| num_failure(err, e.at, "conversion", to))?
            }
            // Arrays (Gate 2): built in order, copied, and read by a checked index.
            ExprKind::Array { elems } => {
                let mut out = Vec::with_capacity(elems.len());
                for x in elems {
                    out.push(take!(self, f, x, env));
                }
                Value::Array(out)
            }
            ExprKind::ArrayRepeat { value, count } => {
                let v = take!(self, f, value, env);
                Value::Array(vec![v; *count as usize])
            }
            ExprKind::Index { base, index } => {
                let b = take!(self, f, base, env);
                let i = take!(self, f, index, env);
                let (Value::Array(items), Value::Int(i)) = (&b, &i) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "an index into a non-array",
                    ));
                };
                match usize::try_from(*i).ok().and_then(|i| items.get(i)) {
                    Some(v) => v.clone(),
                    None => return Err(index_outside(e.at, *i, items.len())),
                }
            }
            ExprKind::Fits { to, operand } => {
                let v = take!(self, f, operand, env);
                let (Some(a), Some(to)) = (as_num(&v), to.num()) else {
                    return Err(fail(
                        codes::INVALID_OPERANDS,
                        e.at,
                        "a conversion of a non-number",
                    ));
                };
                Value::Bool(numeric::fits(to, a))
            }

            ExprKind::Record {
                arity,
                fields,
                record,
            } => {
                // **In the order written**, which `docs/spec.md` fixes as the evaluation
                // order and which physical layout has nothing to do with. A failure part
                // way through drops what has been built so far — the `Vec` goes with the
                // error — and that drop is what releases a string backing or a sequence
                // reference an earlier initialiser had taken. The counters observe it, so
                // the cleanup is evidence rather than an assumption about the host.
                let values = self.initialise(f, *arity, fields, e.at, env)?;
                match values {
                    Ok(values) => Value::Record(Rec::new(*record, values)),
                    Err(flow) => return Ok(flow),
                }
            }

            ExprKind::Variant {
                enumeration,
                variant,
                arity,
                fields,
            } => {
                // In the order written, like a record construction, and the same drop on
                // the way out. The discriminant is only written once every payload field
                // exists, so no partially built value is ever a value of a variant.
                let values = self.initialise(f, *arity, fields, e.at, env)?;
                match values {
                    Ok(values) => Value::Enum(EnumVal::new(*enumeration, *variant, values)),
                    Err(flow) => return Ok(flow),
                }
            }

            ExprKind::Match(m) => return self.matching(f, m, env),

            ExprKind::Field { base, field, .. } => {
                let base = take!(self, f, base, env);
                // The field's value is **cloned out**, which is the whole of rule 3 for a
                // projection: what comes out holds its own reference to whatever backs
                // it, so it survives the record it came from — including when that record
                // was a temporary that dies on this very line.
                let Value::Record(r) = &base else {
                    return Err(fail(
                        codes::INVALID_PROJECTION,
                        e.at,
                        "this is not a record",
                    ));
                };
                let Some(v) = r.field(*field) else {
                    return Err(fail(
                        codes::UNKNOWN_FIELD,
                        e.at,
                        "this record has no such field",
                    ));
                };
                v.clone()
            }

            ExprKind::Builtin { op, args, .. } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(take!(self, f, a, env));
                }
                match bytes_text_intrinsic(*op, &values, e.ty, e.at) {
                    Some(r) => r?,
                    None => match super::os::os_intrinsic(*op, &values, e.at) {
                        Some(r) => r?,
                        None => self.intrinsic(*op, &values, e.at)?,
                    },
                }
            }

            ExprKind::Closure {
                func,
                captures: taken,
            } => {
                let mut values = Vec::with_capacity(taken.len());
                for l in taken {
                    match env.get(l.index()).cloned().flatten() {
                        Some(v) => values.push(v),
                        None => {
                            return Err(fail(
                                codes::UNDEFINED_VARIABLE,
                                e.at,
                                "a closure captured a binding that holds nothing",
                            ))
                        }
                    }
                }
                Value::Func(FuncVal::new(*func, values))
            }
            ExprKind::FnValue(func) => Value::Func(FuncVal::new(*func, Vec::new())),

            ExprKind::Call { .. } | ExprKind::CallIndirect { .. } | ExprKind::CallTrait { .. } => {
                let (mut target, args) = match &e.kind {
                    ExprKind::Call { callee, args, .. } => (Ok(*callee), args),
                    // Decided below, from the receiver's value (N78).
                    ExprKind::CallTrait { args, .. } => {
                        (Ok(nazm_sema::FnId::from_index(usize::MAX)), args)
                    }
                    ExprKind::CallIndirect { callee, args, .. } => {
                        let v = take!(self, f, callee, env);
                        match v {
                            Value::Func(fv) => (Err(fv), args),
                            _ => {
                                return Err(fail(
                                    codes::TYPE_MISMATCH,
                                    e.at,
                                    "a call through something that is not a function value",
                                ))
                            }
                        }
                    }
                    _ => unreachable_flow(e.at)?,
                };
                let mut values = Vec::with_capacity(args.len() + 1);
                for a in args {
                    values.push(take!(self, f, a, env));
                }
                // A call through a bound (N78): this frame's type arguments are not kept, but the
                // receiver's value knows its type, and that is the type the bound was
                // instantiated at. Resolved in the one impl table, Core IR's.
                if let ExprKind::CallTrait {
                    trait_id, method, ..
                } = &e.kind
                {
                    let callee = values
                        .first()
                        .and_then(Value::runtime_type)
                        .and_then(|t| self.core.method_fn(*trait_id, t, *method));
                    // A derived `Hash` (L6): no impl, so the type's own walk.
                    if callee.is_none()
                        && self
                            .core
                            .derived_hash
                            .is_some_and(|d| d.trait_id == *trait_id)
                    {
                        return self.derived_hash_call(*method, &values, e.at);
                    }
                    match callee {
                        Some(c) => target = Ok(c),
                        None => {
                            return Err(fail(
                                codes::COMPILER_DEFECT,
                                e.at,
                                "a trait call whose receiver's type has no impl in the program",
                            ))
                        }
                    }
                }
                let callee = match &target {
                    Ok(c) => *c,
                    Err(fv) => fv.func,
                };
                let core = Arc::clone(&self.core);
                let Some(g) = core.function(callee) else {
                    return Err(fail(
                        codes::UNDEFINED_FUNCTION,
                        e.at,
                        "this call's function is not in the program",
                    ));
                };

                self.depth += 1;
                self.calls += 1;
                if self.depth > self.max_depth {
                    self.depth -= 1;
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::CALL_DEPTH_LIMIT,
                                e.at,
                                format!("`{}` recursed deeper than {}", g.name, self.max_depth),
                            )
                            .help(
                                "the interpreter bounds call depth so runaway recursion \
                             reports instead of exhausting the stack, which would crash \
                             without a diagnostic. Raise it with `--max-depth`, or check \
                             the recursion has a base case",
                            ),
                        ),
                    });
                }

                // A fresh frame: a function sees its parameters and nothing from the
                // caller. Lexical scope, not dynamic. A closure's body sees itself first, then
                // its parameters, and its captures where its body reads them (N50).
                let mut fr = match &target {
                    Err(fv) if g.closure => {
                        values.insert(0, Value::Func(Arc::clone(fv)));
                        frame(g, values)
                    }
                    _ => frame(g, values),
                };
                if let Err(fv) = &target {
                    let slots = &g.captures;
                    for (l, v) in slots.iter().zip(&fv.carried) {
                        if let Some(slot) = fr.get_mut(l.index()) {
                            *slot = Some(v.clone());
                        }
                    }
                }
                let out = self.call_body(g, fr);
                self.depth -= 1;
                // A call absorbs its callee's `return`: that is what makes it a call and
                // not a jump. Nothing propagates outward from here but a genuine error.
                out?
            }

            // Both branches are value regions; the one taken is the `if`'s flow.
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = take!(self, f, cond, env);
                return if as_bool(&c, cond)? {
                    self.block(f, then, env)
                } else {
                    self.block(f, otherwise, env)
                };
            }

            ExprKind::Block(b) => return self.block(f, b, env),
        };
        Ok(Flow::Fell(Some(value)))
    }
}

/// The tail of `local = str_append(local, tail)`, the shape `lower.rs` gives an append.
fn appended(local: cir::LocalId, value: &cir::Expr) -> Option<&cir::Expr> {
    let ExprKind::Builtin {
        op: nazm_sema::Intrinsic::StrAppend,
        args,
        ..
    } = &value.kind
    else {
        return None;
    };
    match args.as_slice() {
        [cir::Expr {
            kind: ExprKind::Local(l),
            ..
        }, tail]
            if *l == local =>
        {
            Some(tail)
        }
        _ => None,
    }
}
