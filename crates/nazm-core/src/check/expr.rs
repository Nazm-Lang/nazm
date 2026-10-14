//! Expressions: the dispatch over every expression form.

use super::layout::opaque_refused;
use super::unit::{unknown_qualified, written_key};
use super::written::not_generic;
use super::*;

impl Checker<'_> {
    /// Check an expression, returning how it completes.
    ///
    /// `expected` is used only for recovery: when an error makes the real type
    /// unknowable, it is returned so the caller does not then report a second, invented
    /// mismatch.
    pub(super) fn check_expr(&mut self, e: &Expr, expected: Option<Type>) -> Completion {
        let unwanted = std::mem::take(&mut self.unwanted);
        match e {
            Expr::Int { value, span } => {
                Completion::Value(self.integer_literal((*value).into(), *span, expected))
            }
            Expr::WideInt { value, span } => {
                Completion::Value(self.integer_literal((*value).into(), *span, expected))
            }
            // `Float32` where one is expected, `Float64` everywhere else (Gate 2).
            Expr::Float { span, .. } => Completion::Value(self.float_literal(*span, expected)),
            Expr::Bool { .. } => Completion::Value(Type::Bool),
            // A literal is UTF-8 by construction, so it is a `Text` where one is expected (Gate 2).
            Expr::Str { span, .. } if expected == Some(Type::Text) => {
                self.res
                    .record_numeric(*span, NumericFact::Literal(Type::Text));
                Completion::Value(Type::Text)
            }
            Expr::Str { .. } => Completion::Value(Type::Str),

            Expr::Var { name, span } => match self.lookup_var(name, *span) {
                Some(b) => {
                    // Which binding this mention refers to, recorded where the question
                    // was answered. The backend reads this instead of walking scopes again.
                    self.record_var(*span, b.slot);
                    Completion::Value(b.ty)
                }
                // A constant (Gate 2): its type, and its value for Core IR to write.
                None if self.types.consts.values.contains_key(name) => {
                    Completion::Value(self.const_value(name, *span))
                }
                // A static `Atomic` (Gate 3): its one cell.
                None if self.types.consts.cells.contains_key(name) => {
                    let module = self.module;
                    self.res.record_static_cell(*span, module, name.clone());
                    if let Some(decl) = self.types.consts.decls.get(name) {
                        self.res.record_const_ref(*span, *decl);
                    }
                    Completion::Value(Type::Atomic)
                }
                None if self.env.contains(name) => {
                    Completion::Value(self.function_value(name, *span, expected))
                }
                // `Dot` (R1.5, D4): a variant with no payload, written without its enum —
                // the one enum in scope that has it.
                None if name.starts_with(|c: char| c.is_ascii_uppercase())
                    && !self.variant_owners(name).is_empty() =>
                {
                    match self.check_construction(name, *span, None, &[], *span, expected) {
                        Some(t) => Completion::Value(t),
                        None => Completion::Value(expected.unwrap_or(Type::Int)),
                    }
                }
                None => {
                    self.errors.push(Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not defined"),
                    ));
                    Completion::Value(expected.unwrap_or(Type::Int))
                }
            },

            Expr::Closure {
                params,
                return_type,
                effects,
                body,
                fn_span,
                span,
            } => Completion::Value(self.check_closure(
                params,
                return_type,
                effects.as_ref(),
                body,
                *fn_span,
                *span,
                expected,
            )),

            Expr::Neg {
                operand, op_span, ..
            } => Completion::Value(self.check_negation(operand, *op_span, expected)),

            // `~x` (Gate 2): the complement of an integer of any width.
            Expr::BitNot {
                operand, op_span, ..
            } => Completion::Value(self.check_complement(operand, *op_span, expected)),

            // Arrays (Gate 2).
            Expr::Array { elems, span } => {
                Completion::Value(self.check_array_literal(elems, *span, expected))
            }
            // `(a, b, …)` (L5): a tuple of the elements' types, each checked against the one
            // expected where a tuple of that many is expected.
            Expr::Tuple { elems, .. } => {
                let wanted = expected
                    .and_then(|t| self.res.types().tuple_elements(t))
                    .filter(|w| w.len() == elems.len());
                let mut tys = Vec::with_capacity(elems.len());
                for (i, e) in elems.iter().enumerate() {
                    let want = wanted.as_ref().and_then(|w| w.get(i).copied());
                    tys.push(self.value_type(e, want));
                }
                Completion::Value(self.res.types_mut().tuple_of(tys))
            }
            Expr::ArrayRepeat { value, count, span } => {
                Completion::Value(self.check_array_repeat(value, count, *span, expected))
            }
            Expr::Index {
                base,
                index,
                bracket_span,
                ..
            } => Completion::Value(self.check_index(base, index, *bracket_span, expected)),

            Expr::Not {
                operand, op_span, ..
            } => {
                let t = self.value_type(operand, Some(Type::Bool));
                if t != Type::Bool {
                    self.errors.push(Diagnostic::error(
                        codes::INVALID_OPERANDS,
                        *op_span,
                        format!("`!` applies to `Bool`, but this is `{}`", self.show(t)),
                    ));
                }
                Completion::Value(Type::Bool)
            }

            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => Completion::Value(self.check_binary_expr(*op, lhs, rhs, *op_span, expected)),

            Expr::Call {
                qualifier,
                callee,
                callee_span,
                type_args,
                args,
                span,
            } => {
                // A binding of function type of that name hides a function of it (N50):
                // the call is through the value.
                if qualifier.is_none()
                    && type_args.is_none()
                    && matches!(self.scopes.lookup(callee).map(|b| b.ty), Some(Type::Fn(_)))
                {
                    return Completion::Value(self.check_indirect_call(
                        callee,
                        *callee_span,
                        args,
                        *span,
                        expected,
                    ));
                }
                // `UInt8(x)`, `try_convert[T](x)` and its kin (Gate 2): a numeric conversion,
                // where no function of the name is in scope.
                if qualifier.is_none() {
                    if let Some(t) = self.maybe_conversion(
                        callee,
                        *callee_span,
                        type_args.as_deref(),
                        args,
                        *span,
                    ) {
                        return Completion::Value(t);
                    }
                }
                // `Point(1, 2)`, `Circle(2)` (R1.5, D4): a capitalised name no function in scope
                // has, but a record or a variant does, is that construction, its arguments its
                // fields by position.
                if qualifier.is_none()
                    && !args.is_empty()
                    && callee.starts_with(|c: char| c.is_ascii_uppercase())
                    && !self.env.contains(callee)
                    && (matches!(self.quiet_type(callee, *callee_span), Type::Record(_))
                        || !self.variant_owners(callee).is_empty())
                {
                    let fields = positional_fields(args);
                    return match self.check_construction(
                        callee,
                        *callee_span,
                        type_args.as_deref(),
                        &fields,
                        *span,
                        expected,
                    ) {
                        Some(t) => Completion::Value(t),
                        None => Completion::Value(expected.unwrap_or(Type::Int)),
                    };
                }
                // `m::f` is looked up as one key (N49); everything below names it as written.
                let key = written_key(qualifier.as_ref(), callee);
                let callee = &key;
                let resolved = self.resolve_callee(callee, *callee_span);
                // A function that returns nothing (R1, 5b.2) has no value to use.
                if let Some(sig) = resolved.as_ref().filter(|s| s.no_result && !unwanted) {
                    let _ = sig;
                    self.errors.push(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            *span,
                            format!("`{callee}` returns nothing, so this call has no value to use"),
                        )
                        .help(format!(
                            "call it as a statement of its own, or give `{callee}` a result type"
                        )),
                    );
                }
                let Some(sig) = resolved else {
                    let qualified = qualifier.is_some().then(|| {
                        unknown_qualified(callee, &self.alias_names(), *callee_span, "function")
                    });
                    let d = qualified
                        .or_else(|| self.why_invisible(callee, *callee_span))
                        .unwrap_or_else(|| {
                            // `IoCap()`: the question is where authority comes from (N37).
                            if let Some(Type::Cap(k)) = Type::from_name(callee) {
                                return forged(k, *callee_span);
                            }
                            Diagnostic::error(
                                codes::UNDEFINED_FUNCTION,
                                *callee_span,
                                format!("`{callee}` is not a function"),
                            )
                        });
                    self.errors.push(d);
                    // A completion probe's `Name(` with no function `Name` (N26): which
                    // record the module's types name by it, read and applied exactly as a
                    // construction's head is — so a generic record with no type arguments
                    // written names no instance, and nothing is recorded. Never in an
                    // ordinary program, whose `completion_call` is `None`.
                    if self.res.completion_call() == Some(*callee_span) {
                        self.called_record(callee, *callee_span, type_args.as_deref());
                    }
                    for a in args {
                        self.check_expr(a, None);
                    }
                    return Completion::Value(expected.unwrap_or(Type::Int));
                };

                if args.len() != sig.params.len() {
                    self.errors.push(
                        Diagnostic::error(
                            codes::WRONG_ARGUMENT_COUNT,
                            *span,
                            format!(
                                "`{callee}` takes {} argument{}, but {} {} given",
                                sig.params.len(),
                                if sig.params.len() == 1 { "" } else { "s" },
                                args.len(),
                                if args.len() == 1 { "was" } else { "were" }
                            ),
                        )
                        .secondary(sig.name_span, "defined here"),
                    );
                }

                if sig.generics > 0 || sig.effect_param {
                    let ret = self.check_generic_call(
                        callee,
                        *callee_span,
                        type_args.as_deref(),
                        args,
                        *span,
                        &sig,
                        expected,
                    );
                    return Completion::Value(ret);
                }
                if type_args.is_some() {
                    self.errors
                        .push(not_generic(callee, "not generic", *callee_span));
                }
                self.record_site(*callee_span, Vec::new(), false);

                // Left to right, matching the evaluation order the spec fixes.
                for (i, a) in args.iter().enumerate() {
                    let want = sig.params.get(i).copied();
                    let got = self.value_type(a, want);
                    if let Some(want) = want {
                        if !self.assignable(got, want) {
                            self.errors.push(Diagnostic::error(
                                codes::TYPE_MISMATCH,
                                a.span(),
                                format!(
                                    "argument {} of `{callee}` is `{}`, but this is `{}`",
                                    i + 1,
                                    self.show(want),
                                    self.show(got)
                                ),
                            ));
                        }
                    }
                }
                self.record_checked_call(*callee_span, *span, args, &sig, None, false);
                self.check_callbacks(*callee_span, args, &sig);
                self.prove_preconditions(*callee_span, args);
                Completion::Value(sig.ret)
            }

            Expr::Construct {
                qualifier,
                ty,
                ty_span,
                type_args,
                fields,
                span,
            } => match self.check_construction(
                &written_key(qualifier.as_ref(), ty),
                *ty_span,
                type_args.as_deref(),
                fields,
                *span,
                expected,
            ) {
                Some(t) => Completion::Value(t),
                // The type did not resolve, and that is already reported. `Int` is what
                // every unresolved type continues as, so no second error is raised here.
                None => Completion::Value(Type::Int),
            },

            // `x.m()` and `Show.m()` with nothing to tell them from a variant by (N78): a
            // call when the name before the `.` is a value or a trait rather than an enum.
            Expr::Variant {
                qualifier,
                ty,
                ty_span,
                type_args: None,
                variant,
                variant_span,
                fields,
                span,
            } if fields.is_empty() && self.names_a_receiver(qualifier.as_ref(), ty) => {
                let receiver = Expr::Var {
                    name: ty.clone(),
                    span: *ty_span,
                };
                Completion::Value(self.check_method_call(
                    qualifier.as_ref(),
                    &receiver,
                    variant,
                    *variant_span,
                    &[],
                    *span,
                    expected,
                ))
            }

            // `f"…"` (L4): a `Str`, each hole a value interpolation writes.
            Expr::Interp { parts, .. } => {
                for p in parts {
                    if let nazm_syntax::InterpPart::Hole { expr, spec, span } = p {
                        self.check_hole(expr, *spec, *span);
                    }
                }
                Completion::Value(Type::Str)
            }

            Expr::MethodCall {
                qualifier,
                receiver,
                method,
                method_span,
                args,
                span,
            } => Completion::Value(self.check_method_call(
                qualifier.as_ref(),
                receiver,
                method,
                *method_span,
                args,
                *span,
                expected,
            )),

            Expr::Variant {
                qualifier,
                ty,
                ty_span,
                type_args,
                variant,
                variant_span,
                fields,
                span,
            } => match self.check_variant(
                &written_key(qualifier.as_ref(), ty),
                *ty_span,
                type_args.as_deref(),
                variant,
                *variant_span,
                fields,
                *span,
                expected,
            ) {
                Some(t) => Completion::Value(t),
                None => Completion::Value(expected.unwrap_or(Type::Int)),
            },

            Expr::Match {
                scrutinee,
                arms,
                source,
                span,
            } => self.check_match(scrutinee, arms, *source, *span, expected),

            Expr::Field {
                base,
                name,
                name_span,
                ..
            } => {
                // `State.Rea` while `State.Ready(…)` is being written: `State` is no value,
                // so this is a projection that fails — but the module's types name `State`
                // as an enum, and a tool completing the name after `.` needs which (N21).
                // Recorded, not acted on: the diagnostics below are unchanged. A generic
                // enum is not recorded, since which instance is meant is not written.
                if let Expr::Var {
                    name: qualifier, ..
                } = &**base
                {
                    let is_value = self.scopes.lookup(qualifier).is_some();
                    let is_param = self.type_scope.iter().any(|(n, _)| n == qualifier);
                    if let (false, false, false, Some(Type::Enum(id))) = (
                        is_value,
                        is_param,
                        self.refused(qualifier),
                        self.types.names.get(qualifier).copied(),
                    ) {
                        if self.res.types().arity(ParamOwner::Enum(id)) == 0 {
                            self.res
                                .record_unresolved_member(*name_span, Type::Enum(id));
                        }
                    }
                }
                // The base is checked with no expectation: what a projection wants of its
                // base is "a record with this field", which is not a `Type` and so cannot
                // be passed as one.
                let base_ty = self.value_type(base, None);
                if let Type::Record(id) = base_ty {
                    if self
                        .res
                        .records()
                        .get(id)
                        .is_some_and(|d| d.repr == CRepr::Opaque)
                    {
                        let shown = self.show(base_ty);
                        self.errors
                            .push(opaque_refused(*name_span, &shown, "taken apart"));
                        return Completion::Value(Type::Int);
                    }
                }
                match self.check_projection(base_ty, name, *name_span) {
                    Some(t) => Completion::Value(t),
                    None => Completion::Value(Type::Int),
                }
            }

            Expr::Propagate {
                operand, op_span, ..
            } => {
                self.refuse_leaving_defer("`?`", *op_span);
                self.check_propagate(operand, *op_span, expected)
            }

            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let ct = self.value_type(cond, Some(Type::Bool));
                if ct != Type::Bool {
                    self.errors.push(
                        Diagnostic::error(
                            codes::CONDITION_NOT_BOOL,
                            cond.span(),
                            format!(
                                "an `if` condition must be `Bool`, but this is `{}`",
                                self.show(ct)
                            ),
                        )
                        .help("Nazm has no truthiness: use a comparison such as `x != 0`"),
                    );
                }

                self.discard_tail = unwanted;
                let then_c = self.check_block(then_block, expected);
                let Some(else_block) = else_block else {
                    // No `else`. Unit whatever the branch does — even when it returns on
                    // every path, the condition may be false, so the `if` itself always
                    // completes. Usable as a statement; the caller reports if it wanted a
                    // value, which is where the context to say so lives.
                    return Completion::Unit;
                };
                self.discard_tail = unwanted;
                let else_c = if unwanted {
                    self.check_block(else_block, None)
                } else {
                    self.check_block(else_block, expected.or(then_c.ty()))
                };

                match (then_c, else_c) {
                    // One branch leaving means the other decides. Without this,
                    // `if c { return 0; } else { 1 }` would be an error for having
                    // "different" branches, which is exactly the shape `return` is for.
                    (Completion::Diverges, other) | (other, Completion::Diverges) => other,
                    (Completion::Value(a), Completion::Value(b)) if a == b => Completion::Value(a),
                    // Where nobody wants the `if`'s value (R1, 5b.19), its branches may end
                    // differently — in values of two types, or one in none — and it produces
                    // nothing, as an `if` without `else` does. So `if a { print(x) } else if b {
                    // name(y) }` is a statement however its branches end.
                    (Completion::Value(a), Completion::Value(b)) if unwanted && a != b => {
                        Completion::Unit
                    }
                    (Completion::Value(_), Completion::Unit)
                    | (Completion::Unit, Completion::Value(_))
                        if unwanted =>
                    {
                        Completion::Unit
                    }
                    (Completion::Value(a), Completion::Value(b)) => {
                        let (sa, sb) = (self.show(a), self.show(b));
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                else_block
                                    .tail
                                    .as_ref()
                                    .map_or(else_block.span, |t| t.span()),
                                format!("this branch is `{sb}`, but the other is `{sa}`"),
                            )
                            .secondary(
                                then_block
                                    .tail
                                    .as_ref()
                                    .map_or(then_block.span, |t| t.span()),
                                format!("this one is `{sa}`"),
                            )
                            .help("both branches of an `if` must have the same type"),
                        );
                        Completion::Value(a)
                    }
                    // One branch produces a value and the other does not.
                    (Completion::Value(a), Completion::Unit)
                    | (Completion::Unit, Completion::Value(a)) => {
                        let sa = self.show(a);
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                *e.span_ref(),
                                format!("one branch produces `{sa}` and the other produces nothing"),
                            )
                            .help("both branches of an `if` must agree, or neither may produce a value"),
                        );
                        Completion::Value(a)
                    }
                    (Completion::Unit, Completion::Unit) => Completion::Unit,
                }
            }

            Expr::Block(b) => self.check_block(b, expected),
        }
    }

    /// An interpolation's hole (R1a, L4, `docs/spec.md`, *String interpolation*): a `Str`, an
    /// integer, a float, a `Bool`, or a value whose `Show` is in scope — its `show()` checked as
    /// the method call at the hole's braces it lowers to. A precision is a float's.
    fn check_hole(&mut self, expr: &Expr, spec: nazm_syntax::FormatSpec, at: Span) {
        let ty = self.value_type(expr, None);
        let float = matches!(ty, Type::Num(k) if k.is_float());
        if spec.precision.is_some() && !float {
            self.errors.push(
                Diagnostic::error(
                    codes::INTERPOLATION,
                    at,
                    format!("a precision is a float's, and this is `{}`", self.show(ty)),
                )
                .help("write the hole without `.precision`, or convert the value to a float"),
            );
        }
        let primitive = matches!(ty, Type::Str | Type::Int | Type::Bool | Type::Num(_));
        if primitive {
            return;
        }
        // Anything else is written by its `Show`, if it has one in scope.
        if self.lookup_method(ty, "show", at).is_err() {
            let shown = self.show(ty);
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    expr.span(),
                    format!("a value printed or interpolated is a `Str`, a number, a `Bool` or a `Show`, and this is `{shown}`"),
                )
                .help(format!(
                    "implement `Show` (`@std/show`) for `{shown}`, or write the value's text yourself"
                )),
            );
            return;
        }
        let ret = self.check_method_call_on(None, expr, Some(ty), "show", at, &[], at, None);
        if ret != Type::Str {
            self.errors.push(Diagnostic::error(
                codes::TYPE_MISMATCH,
                at,
                format!(
                    "this value's `show` gives `{}`, not a `Str`",
                    self.show(ret)
                ),
            ));
        }
    }
}

/// A call's arguments as fields given by position (R1.5, D4): each with no name, at a span of no
/// width where its value begins — the spans the checker records them at and Core IR reads.
pub(crate) fn positional_fields(args: &[Expr]) -> Vec<nazm_syntax::FieldInit> {
    args.iter()
        .map(|a| {
            let s = a.span();
            nazm_syntax::FieldInit {
                name: String::new(),
                name_span: Span::new(s.file, s.start, s.start),
                span: s,
                value: a.clone(),
            }
        })
        .collect()
}
