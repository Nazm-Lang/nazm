//! A function's body and its contracts.

use super::declare::param_scope;
use super::unit::written_key;
use super::written::record_type_scope;
use super::*;

impl Checker<'_> {
    pub(super) fn check_function(&mut self, f: &Function) {
        // Slot allocation starts fresh per function: a caller's local and a callee's
        // local with the same spelling are different storage, not the same.
        //
        // Looked up in **this module's** table. Before N2 the name alone was enough,
        // which is the same thing as saying a name meant one function in the program.
        self.current = defined(self.res, self.module, f);
        self.closure_count = 0;
        self.effect_param = self.current.and_then(|id| {
            f.effect_params
                .first()
                .map(|p| (p.name.clone(), ParamOwner::Function(id)))
        });
        // The function's own type parameters are in scope for every type written inside
        // it, and in no other function.
        self.type_scope = self
            .current
            .map(|id| param_scope(&f.type_params, ParamOwner::Function(id)))
            .unwrap_or_default();
        // An impl's method may write its impl's type as `Self` (N78): its first parameter's.
        if f.method_of.is_some() {
            if let Some(ty) = self
                .current
                .and_then(|id| self.res.function(id))
                .and_then(|d| d.params.first().copied())
            {
                self.type_scope.push(("Self".to_owned(), ty));
            }
        }
        record_type_scope(self.res, f.span, &self.type_scope, &self.types);
        self.open_scope(f.span);
        for p in &f.params {
            let ty = self.signature_type(&p.ty);
            if self.scopes.defined_here(&p.name) {
                self.errors.push(Diagnostic::error(
                    codes::ALREADY_DEFINED_IN_SCOPE,
                    p.span,
                    format!("`{}` is listed twice in this parameter list", p.name),
                ));
            }
            // Parameters are never mutable. A mutable parameter is a local copy, which
            // reads as an out-parameter to anyone who has not checked the calling
            // convention; `let mut` in the body says the same thing unambiguously.
            // No declaration span: `mut` on a parameter is `N0311`, so there is no
            // edit at the declaration that would make this assignment legal.
            let slot = self.allocate(&p.name, p.name_span, ty, false);
            self.define(&p.name, ty, false, None, slot, p.name_span.end);
        }

        let declared = self.signature_type(&f.return_type);
        // Contracts (N87, §7.88): `Bool`, over the parameters and, after `ensures`, `result`.
        self.check_contracts(f, declared);
        if f.foreign.is_some() {
            // A foreign declaration has no body, and that is not a parse failure (N42).
            self.close_scope();
            return;
        }
        let Some(body) = &f.body else {
            // The body failed to parse; the parse error is already reported. Anything
            // said about statements that were never built would be about the parser's
            // guess, not the program.
            self.res.scopes_mut().incomplete();
            self.close_scope();
            return;
        };
        let previous = self.return_type.replace(declared);
        let previous_span = self.return_span.replace(f.return_type.span);
        let completion = self.check_block(body, Some(declared));
        self.return_type = previous;
        self.return_span = previous_span;

        match completion {
            // Every path returned. No tail is needed, and demanding one would force a
            // dummy expression after code that cannot reach it.
            Completion::Diverges => {}
            Completion::Value(actual) if self.assignable(actual, declared) => {}
            Completion::Value(actual) => {
                let (actual, declared) = (self.show(actual), self.show(declared));
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.tail.as_ref().map_or(body.span, |t| t.span()),
                        format!(
                            "`{}` returns `{actual}`, but its signature says `{declared}`",
                            f.name
                        ),
                    )
                    .secondary(f.return_type.span, format!("declared `{declared}` here")),
                );
            }
            Completion::Unit => {
                let declared = self.show(declared);
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.span,
                        format!(
                            "`{}` must produce a `{declared}`, but this body does not",
                            f.name
                        ),
                    )
                    .secondary(f.return_type.span, format!("declared `{declared}` here"))
                    .help(
                        "a function's value is its final expression — one with no `;` \
                         after it — or it returns on every path",
                    ),
                );
            }
        }
        self.close_scope();
    }

    /// A function's contract clauses (N87, §7.88): each a `Bool`; a `requires` over the
    /// parameters, an `ensures` over them and `result`, the value being returned, in a slot of
    /// the function's own; and each made only of what cannot fail for want of authority or
    /// memory — literals, names, operators, projections and calls of pure functions (`N0614`).
    fn check_contracts(&mut self, f: &Function, declared: Type) {
        for c in &f.contracts {
            let ensures = c.kind == nazm_syntax::ContractKind::Ensures;
            if ensures {
                self.open_scope(c.span);
                let slot = match self.current.and_then(|id| self.res.contract_result(id)) {
                    Some(slot) => slot,
                    None => {
                        let slot = self.allocate("result", c.keyword_span, declared, false);
                        if let Some(id) = self.current {
                            self.res.record_contract_result(id, slot);
                        }
                        slot
                    }
                };
                self.define("result", declared, false, None, slot, c.keyword_span.end);
            }
            let got = self.value_type(&c.expr, Some(Type::Bool));
            if got != Type::Bool {
                let shown = self.show(got);
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        c.expr.span(),
                        format!("a contract clause is a `Bool`, but this is `{shown}`"),
                    )
                    .help("a clause states a condition: compare, or combine with `&&`, `||`, `!`"),
                );
            }
            if let Some((at, what)) = self.impure_clause(&c.expr) {
                self.errors.push(
                    Diagnostic::error(
                        codes::CONTRACT_SHAPE,
                        at,
                        format!("a contract clause cannot {what}"),
                    )
                    .primary_label("not a contract's")
                    .help(
                        "a clause is evaluated every time the function is entered or returns, \
                             and must not fail for want of authority or memory: it is literals, \
                             names, operators, field reads and calls of functions declaring `! {}`",
                    ),
                );
            }
            if ensures {
                self.close_scope();
            }
        }
    }

    /// The first part of a clause a contract may not contain (N87), with what it is.
    fn impure_clause(&self, e: &Expr) -> Option<(Span, &'static str)> {
        match e {
            Expr::Int { .. }
            | Expr::WideInt { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Str { .. }
            | Expr::Var { .. } => None,
            Expr::Neg { operand, .. }
            | Expr::Not { operand, .. }
            | Expr::BitNot { operand, .. } => self.impure_clause(operand),
            Expr::Array { span, .. }
            | Expr::ArrayRepeat { span, .. }
            | Expr::Tuple { span, .. }
            | Expr::Interp { span, .. } => Some((*span, "build a value")),
            Expr::Index { base, index, .. } => self
                .impure_clause(base)
                .or_else(|| self.impure_clause(index)),
            Expr::Binary { lhs, rhs, .. } => {
                self.impure_clause(lhs).or_else(|| self.impure_clause(rhs))
            }
            Expr::Field { base, .. } => self.impure_clause(base),
            Expr::Call {
                qualifier,
                callee,
                callee_span,
                args,
                ..
            } => {
                let pure = self
                    .env
                    .get(&written_key(qualifier.as_ref(), callee))
                    .is_some_and(|v| v.sig.pure);
                if pure {
                    args.iter().find_map(|a| self.impure_clause(a))
                } else {
                    Some((*callee_span, "call a function that is not pure"))
                }
            }
            Expr::Closure { span, .. } => Some((*span, "make a closure")),
            Expr::Construct { span, .. } | Expr::Variant { span, .. } => {
                Some((*span, "build a value"))
            }
            Expr::Match { span, .. } | Expr::If { span, .. } => {
                Some((*span, "branch; write the condition with `&&` and `||`"))
            }
            Expr::Block(b) => Some((b.span, "contain a block")),
            Expr::Propagate { op_span, .. } => Some((*op_span, "propagate a failure")),
            Expr::MethodCall { span, .. } => Some((*span, "call a method")),
        }
    }
}
