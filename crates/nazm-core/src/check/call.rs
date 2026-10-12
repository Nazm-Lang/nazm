//! Calls: methods, indirect calls, closures, preconditions and callbacks, and recording each
//! checked call.

use super::clauses::{callee_fn, describe_clause, literal, Lit};
use super::unit::written_key;
use super::*;

impl Checker<'_> {
    /// A call through a binding of function type (N50).
    /// Whether `name.m()` names a receiver rather than an enum (N78): a value in scope, or —
    /// when no enum of that name is visible — a trait. An enum keeps every meaning it had.
    pub(super) fn names_a_receiver(
        &self,
        qualifier: Option<&nazm_syntax::Qualifier>,
        name: &str,
    ) -> bool {
        let key = written_key(qualifier, name);
        if qualifier.is_none() && self.scopes.lookup(name).is_some() {
            return true;
        }
        let enum_visible = matches!(self.types.names.get(&key), Some(Type::Enum(_)));
        !enum_visible && matches!(self.trait_names.get(&key), Some(Some(_)))
    }

    /// A method call (N78, `architecture.md` §7.79): `recv.m(args)`, or `Trait.m(recv, args)`
    /// when the receiver as written is a trait's name. Resolved here, completely: on a
    /// concrete type to the impl's function, recorded as an ordinary call; on a type
    /// parameter to its bound's method, recorded as a trait call and — for effects, authority
    /// and provenance — as an indirect call of the method's signature.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_method_call(
        &mut self,
        qualifier: Option<&nazm_syntax::Qualifier>,
        receiver: &Expr,
        method: &str,
        at: Span,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        self.check_method_call_on(qualifier, receiver, None, method, at, args, span, expected)
    }

    /// As [`Self::check_method_call`], with the receiver's type already known — an
    /// interpolation's hole (L4) — so the receiver is not checked a second time.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_method_call_on(
        &mut self,
        qualifier: Option<&nazm_syntax::Qualifier>,
        receiver: &Expr,
        known: Option<Type>,
        method: &str,
        at: Span,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        // What an unresolved call continues as: what was wanted, so one mistake is one
        // diagnostic.
        let fallback = expected.unwrap_or(Type::Int);
        // `Show.show(x)`: the trait is written, and the receiver is the first argument.
        let ufcs = match receiver {
            Expr::Var {
                name,
                span: name_at,
            } if qualifier.is_some() || self.scopes.lookup(name).is_none() => {
                let key = written_key(qualifier, name);
                match self.trait_names.get(&key) {
                    Some(Some(t)) => Some((*t, *name_at)),
                    _ if qualifier.is_some() => {
                        self.errors.push(Diagnostic::error(
                            codes::NOT_A_TRAIT,
                            *name_at,
                            format!("`{key}` is not a trait in scope"),
                        ));
                        for a in args {
                            self.check_expr(a, None);
                        }
                        return fallback;
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some((t, name_at)) = ufcs {
            self.res.traits_mut().record_mention(name_at, t);
        }
        let (recv, rest): (&Expr, &[Expr]) = match ufcs {
            Some((t, name_at)) => {
                let Some((first, rest)) = args.split_first() else {
                    let tname = self.res.traits().get(t).map_or("?", |d| d.name.as_str());
                    self.errors.push(
                        Diagnostic::error(
                            codes::WRONG_ARGUMENT_COUNT,
                            span,
                            format!("`{tname}.{method}` needs its receiver as its first argument"),
                        )
                        .secondary(name_at, "the trait"),
                    );
                    return fallback;
                };
                (first, rest)
            }
            None => (receiver, args),
        };
        let recv_ty = match known {
            Some(t) => t,
            None => self.value_type(recv, None),
        };

        // Which trait's method this is.
        let chosen: Result<(TraitId, u32), Diagnostic> = if let Some((t, _)) = ufcs {
            let def = self.res.traits().get(t);
            match def.and_then(|d| d.method(method)) {
                Some(i) => Ok((t, i)),
                None => Err(Diagnostic::error(
                    codes::NO_METHOD,
                    at,
                    format!(
                        "`{}` has no method `{method}`",
                        def.map_or("?", |d| d.name.as_str())
                    ),
                )),
            }
        } else {
            self.lookup_method(recv_ty, method, at)
        };
        let (trait_id, index) = match chosen {
            Ok(c) => c,
            Err(d) => {
                self.errors.push(d);
                for a in rest {
                    self.check_expr(a, None);
                }
                return fallback;
            }
        };
        let Some(want) = self
            .res
            .traits()
            .get(trait_id)
            .and_then(|d| d.methods.get(index as usize))
            .cloned()
        else {
            return fallback;
        };
        let tname = self
            .res
            .traits()
            .get(trait_id)
            .map_or_else(|| "?".to_owned(), |d| d.name.clone());
        let owner = ParamOwner::Trait(trait_id);
        let params: Vec<Type> = want
            .params
            .iter()
            .map(|t| self.res.types_mut().substitute(*t, owner, &[recv_ty]))
            .collect();
        let ret = self.res.types_mut().substitute(want.ret, owner, &[recv_ty]);

        // A derived `Hash` (L6) is dispatched as a call through a bound is: the type's own
        // walk, chosen where the type is known, by the interpreter and by MIR alike.
        let derived = !matches!(recv_ty, Type::Param(_))
            && self
                .res
                .traits()
                .derives(trait_id, recv_ty, self.res.types());
        let through_bound = matches!(recv_ty, Type::Param(_)) || derived;
        if through_bound {
            let fty = self.res.types_mut().fn_of(nazm_sema::FnSignature {
                params: std::iter::once(recv_ty)
                    .chain(params.iter().copied())
                    .collect(),
                ret,
                effects: want.effects,
                row: None,
            });
            self.res.traits_mut().record_call(
                at,
                TraitCall {
                    trait_id,
                    method: index,
                    self_ty: recv_ty,
                },
            );
            self.res.record_call(at, Callee::Indirect(fty));
        } else {
            let imp = self
                .res
                .traits()
                .method_fn(trait_id, recv_ty, index)
                .filter(|_| {
                    self.res
                        .traits()
                        .find(trait_id, recv_ty)
                        .and_then(|i| self.res.traits().impl_def(i))
                        .is_some_and(|d| self.impl_modules.contains(&d.module))
                });
            let Some(f) = imp else {
                self.errors.push(
                    Diagnostic::error(
                        codes::NO_METHOD,
                        at,
                        format!(
                            "`{}` does not implement `{tname}` in this module's scope",
                            self.show(recv_ty)
                        ),
                    )
                    .help(
                        "an impl is used where its module is this one or one this module imports",
                    ),
                );
                for a in rest {
                    self.check_expr(a, None);
                }
                return ret;
            };
            self.res.record_call(at, Callee::Function(f));
        }
        self.record_site(at, Vec::new(), false);

        if rest.len() != params.len() {
            self.errors.push(Diagnostic::error(
                codes::WRONG_ARGUMENT_COUNT,
                span,
                format!(
                    "`{tname}.{method}` takes {} argument{} after its receiver, but {} {} given",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    rest.len(),
                    if rest.len() == 1 { "was" } else { "were" }
                ),
            ));
        }
        for (i, a) in rest.iter().enumerate() {
            let want = params.get(i).copied();
            let got = self.value_type(a, want);
            if let Some(want) = want {
                if !self.assignable(got, want) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "argument {} of `{tname}.{method}` is `{}`, but this is `{}`",
                            i + 1,
                            self.show(want),
                            self.show(got)
                        ),
                    ));
                }
            }
        }
        if let Some(callee) = self.res.callee(at) {
            self.res.record_checked_call(
                at,
                CheckedCall {
                    callee,
                    span,
                    args: std::iter::once(recv.span())
                        .chain(rest.iter().map(Expr::span))
                        .collect(),
                    params: std::iter::once(recv_ty).chain(params).collect(),
                    ret,
                    instance: None,
                    spawned: false,
                    preconditions: self
                        .res
                        .function(callee_fn(callee))
                        .map_or(0, |d| d.preconditions),
                },
            );
        }
        ret
    }

    /// The one trait method `method` on a value of `ty` (N78): through a type parameter's
    /// bound, or among the traits in scope with an impl for `ty` this module may use.
    // One diagnostic, built only when the lookup fails: not worth a box.
    #[allow(clippy::result_large_err)]
    pub(super) fn lookup_method(
        &self,
        ty: Type,
        method: &str,
        at: Span,
    ) -> Result<(TraitId, u32), Diagnostic> {
        let shown = self.show(ty);
        if let Type::Param(p) = ty {
            let bound = self.res.traits().bound_of(p);
            return bound
                .and_then(|t| {
                    self.res
                        .traits()
                        .get(t)
                        .and_then(|d| d.method(method))
                        .map(|i| (t, i))
                })
                .ok_or_else(|| {
                    Diagnostic::error(
                        codes::NO_METHOD,
                        at,
                        format!("`{shown}` has no method `{method}`"),
                    )
                    .help(
                        "a type parameter has the methods of the trait it requires: \
                         `fn f[T: Show](x: T) -> Str { x.show() }`",
                    )
                });
        }
        let mut found: Vec<(TraitId, u32)> = Vec::new();
        let mut elsewhere: Vec<String> = Vec::new();
        for (t, def) in self.res.traits().iter() {
            let Some(i) = def.method(method) else {
                continue;
            };
            let in_scope = self.trait_names.values().any(|v| *v == Some(t));
            let Some(imp) = self.res.traits().find(t, ty) else {
                // A derived `Hash` (L6) has no impl to find, and no module it belongs to.
                if in_scope && self.res.traits().derives(t, ty, self.res.types()) {
                    found.push((t, i));
                }
                continue;
            };
            let usable = self
                .res
                .traits()
                .impl_def(imp)
                .is_some_and(|d| self.impl_modules.contains(&d.module));
            if in_scope && usable {
                found.push((t, i));
            } else {
                elsewhere.push(format!("`{}`", def.name));
            }
        }
        match found.as_slice() {
            [one] => Ok(*one),
            [] => {
                let d = Diagnostic::error(
                    codes::NO_METHOD,
                    at,
                    format!("no trait in scope gives `{shown}` a method `{method}`"),
                );
                Err(if elsewhere.is_empty() {
                    d
                } else {
                    d.help(format!(
                        "{} would: import the module that declares it",
                        elsewhere.join(", ")
                    ))
                })
            }
            many => {
                let names: Vec<String> = many
                    .iter()
                    .filter_map(|(t, _)| self.res.traits().get(*t).map(|d| format!("`{}`", d.name)))
                    .collect();
                Err(Diagnostic::error(
                    codes::AMBIGUOUS_METHOD,
                    at,
                    format!(
                        "`{method}` on `{shown}` is a method of {}",
                        names.join(" and ")
                    ),
                )
                .help("name the trait: `Show.show(x)`"))
            }
        }
    }

    /// Whether a value of `got` may be passed or returned where `want` is expected (N79): the
    /// same type; an `IoCap` where an `OutCap` is, which hands on less authority than it
    /// holds; or a function type with the same parameters and result whose effects are within
    /// the expected one's. An effect parameter is matched by binding, never by inclusion.
    pub(super) fn assignable(&self, got: Type, want: Type) -> bool {
        if got == want {
            return true;
        }
        match (got, want) {
            (Type::Cap(g), Type::Cap(w)) => nazm_sema::CapabilitySet::of(g).implied().contains(w),
            (Type::Fn(g), Type::Fn(w)) => {
                let types = self.res.types();
                let (Some(gs), Some(ws)) = (types.fn_signature(g), types.fn_signature(w)) else {
                    return false;
                };
                gs.params == ws.params
                    && gs.ret == ws.ret
                    && gs.row.is_none()
                    && ws.row.is_none()
                    && !gs.effects.has_param()
                    && !ws.effects.has_param()
                    && gs.effects.without(ws.effects).is_empty()
            }
            _ => false,
        }
    }

    pub(super) fn check_indirect_call(
        &mut self,
        name: &str,
        at: Span,
        args: &[Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let Some(b) = self.lookup_var(name, at) else {
            return expected.unwrap_or(Type::Int);
        };
        self.record_var(at, b.slot);
        let Type::Fn(f) = b.ty else {
            return expected.unwrap_or(Type::Int);
        };
        let Some(fsig) = self.res.types().fn_signature(f).cloned() else {
            return expected.unwrap_or(Type::Int);
        };
        if args.len() != fsig.params.len() {
            self.errors.push(Diagnostic::error(
                codes::WRONG_ARGUMENT_COUNT,
                span,
                format!(
                    "`{name}` takes {} argument{}, but {} {} given",
                    fsig.params.len(),
                    if fsig.params.len() == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                ),
            ));
        }
        self.res.record_call(at, Callee::Indirect(b.ty));
        self.record_site(at, Vec::new(), false);
        for (i, a) in args.iter().enumerate() {
            let want = fsig.params.get(i).copied();
            let got = self.value_type(a, want);
            if let Some(want) = want {
                if !self.assignable(got, want) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        a.span(),
                        format!(
                            "argument {} of `{name}` is `{}`, but this is `{}`",
                            i + 1,
                            self.show(want),
                            self.show(got)
                        ),
                    ));
                }
            }
        }
        let sig = Signature {
            params: fsig.params.clone(),
            ret: fsig.ret,
            name_span: at,
            generics: 0,
            owner: None,
            effect_param: false,
            preconditions: 0,
            pure: false,
        };
        self.record_checked_call(at, span, args, &sig, None, false);
        fsig.ret
    }

    /// A closure expression (N50): a definition of its own, checked as a function is, with
    /// what its body names from around it captured.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_closure(
        &mut self,
        params: &[nazm_syntax::Param],
        return_type: &TypeName,
        effects: Option<&nazm_syntax::EffectDecl>,
        body: &Block,
        fn_span: Span,
        span: Span,
        expected: Option<Type>,
    ) -> Type {
        let Some(enclosing) = self.current else {
            return expected.unwrap_or(Type::Int);
        };
        if !self.type_scope.is_empty() {
            self.errors.push(
                Diagnostic::error(
                    codes::NOT_A_FUNCTION_VALUE,
                    fn_span,
                    "a closure inside a generic function is not supported yet",
                )
                .help(
                    "a closure there would need an instance per type argument; write it in a \
                     function that is not generic, or pass a function value in",
                ),
            );
            return expected.unwrap_or(Type::Int);
        }
        let param_types: Vec<Type> = params
            .iter()
            .map(|p| self.resolve_written_type(&p.ty))
            .collect();
        let ret = self.resolve_written_type(return_type);
        let effect = self.effect_param.clone();
        let declared = match effects {
            Some(d) => {
                crate::effects::declared(d, effect.as_ref().map(|e| e.0.as_str()), &mut self.errors)
            }
            None => Some(nazm_sema::EffectSet::EMPTY),
        }
        .unwrap_or(nazm_sema::EffectSet::EMPTY);
        let (enclosing_name, decl) = self
            .res
            .function(enclosing)
            .map(|d| (d.name.clone(), d.decl))
            .unwrap_or_default();
        let base = enclosing_name.split('#').next().unwrap_or("").to_owned();
        let name = format!("{base}#{}", self.closure_count);
        self.closure_count += 1;
        let ty = self.res.types_mut().fn_of(nazm_sema::FnSignature {
            params: param_types.clone(),
            ret,
            effects: declared,
            row: effect.map(|e| e.1),
        });
        let id = self.res.declare_closure(FnDef {
            name,
            module: self.module,
            visibility: Visibility::Private,
            params: param_types.clone(),
            param_names: params.iter().map(|p| p.name.clone()).collect(),
            ret,
            name_span: fn_span,
            decl,
            locals: Vec::new(),
            type_params: Vec::new(),
            call_sites: Vec::new(),
            declared_effects: Some(declared),
            inferred_effects: None,
            effect_reasons: Vec::new(),
            provenance: None,
            mentions: Vec::new(),
            foreign: None,
            export: None,
            preconditions: 0,
            closure: Some(nazm_sema::ClosureInfo {
                enclosing,
                at: span,
                written_effects: effects.map(|d| d.span),
                ty,
                captures: Vec::new(),
            }),
        });

        // The closure's body is a function of its own: its own slots, returns, loops and
        // scopes, and none of the enclosing body's.
        let saved_current = self.current.replace(id);
        let saved_return = self.return_type.replace(ret);
        let saved_span = self.return_span.replace(return_type.span);
        let saved_scope_depth = std::mem::replace(&mut self.scope_depth, 0);
        let saved_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
        self.closures.push(ClosureFrame {
            id,
            boundary: self.scopes.depth(),
            captured: HashMap::new(),
        });
        self.open_scope(span);
        for (p, ty) in params.iter().zip(&param_types) {
            if self.scopes.defined_here(&p.name) {
                self.errors.push(Diagnostic::error(
                    codes::ALREADY_DEFINED_IN_SCOPE,
                    p.span,
                    format!("`{}` is listed twice in this parameter list", p.name),
                ));
            }
            let slot = self.allocate(&p.name, p.name_span, *ty, false);
            self.define(&p.name, *ty, false, None, slot, p.name_span.end);
        }
        let completion = self.check_block(body, Some(ret));
        match completion {
            Completion::Diverges => {}
            Completion::Value(actual) if actual == ret => {}
            Completion::Value(actual) => {
                let (actual, declared) = (self.show(actual), self.show(ret));
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.tail.as_ref().map_or(body.span, |t| t.span()),
                        format!("this closure returns `{actual}`, but its type says `{declared}`"),
                    )
                    .secondary(return_type.span, format!("declared `{declared}` here")),
                );
            }
            Completion::Unit => {
                let declared = self.show(ret);
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        body.span,
                        format!("this closure must produce a `{declared}`, but its body does not"),
                    )
                    .secondary(return_type.span, format!("declared `{declared}` here")),
                );
            }
        }
        self.close_scope();
        self.closures.pop();
        self.current = saved_current;
        self.return_type = saved_return;
        self.return_span = saved_span;
        self.scope_depth = saved_scope_depth;
        self.loop_depth = saved_loop_depth;

        self.res.record_mention(enclosing, ty);
        ty
    }

    /// Record a call as it was checked (N20): its resolved callee, its arguments and the
    /// signature they were checked against, for a tool to read rather than work out.
    /// A call whose arguments are all literals, of a function of this module stating a
    /// precondition (N87, §7.88): each clause evaluated with them. False is `N0615` — the call
    /// would fail every time — and every clause true makes the call's obligation proved, with
    /// the evaluated clauses as its witness. Any other call is not reasoned about.
    pub(super) fn prove_preconditions(&mut self, at: Span, args: &[Expr]) {
        let Some(Callee::Function(callee)) = self.res.callee(at) else {
            return;
        };
        let Some((names, clauses)) = self.preconditions.get(&callee) else {
            return;
        };
        let values: Option<Vec<Lit>> = args.iter().map(|a| literal(a, &HashMap::new())).collect();
        let Some(values) = values else {
            return;
        };
        if values.len() != names.len() {
            return;
        }
        let env: HashMap<&str, Lit> = names.iter().map(String::as_str).zip(values).collect();
        let mut witness: Vec<String> = Vec::new();
        for clause in clauses {
            match literal(clause, &env) {
                Some(Lit::Bool(true)) => witness.push(format!("{} holds", describe_clause(clause))),
                Some(Lit::Bool(false)) => {
                    self.errors.push(
                        Diagnostic::error(
                            codes::PRECONDITION_REFUTED,
                            at,
                            "this call breaks its callee's precondition",
                        )
                        .secondary(clause.span(), "false for these arguments")
                        .help("with these literal arguments the call fails every time it runs (`N0410`)"),
                    );
                    return;
                }
                _ => return,
            }
        }
        let shown: Vec<String> = names
            .iter()
            .map(|n| {
                format!(
                    "{n} = {}",
                    env.get(n.as_str()).map_or_else(String::new, Lit::show)
                )
            })
            .collect();
        self.res.record_proved_call(
            at,
            format!("{} with {}", witness.join(", "), shown.join(", ")),
        );
    }

    /// A call of a foreign function passes each callback as an export's name (N85, §7.86):
    /// only an export has a C entry that needs no environment and lives as long as the
    /// process. A closure, a function value held in a binding, or a function that is not an
    /// export is `N0612`.
    pub(super) fn check_callbacks(&mut self, at: Span, args: &[Expr], sig: &Signature) {
        let Some(Callee::Function(callee)) = self.res.callee(at) else {
            return;
        };
        if self
            .res
            .function(callee)
            .is_none_or(|d| d.foreign.is_none())
        {
            return;
        }
        for (a, want) in args.iter().zip(&sig.params) {
            if !matches!(want, Type::Fn(_)) {
                continue;
            }
            let export = self
                .res
                .fn_value(a.span())
                .and_then(|v| self.res.function(v.function))
                .is_some_and(|d| d.export.is_some());
            if !export {
                self.errors.push(
                    Diagnostic::error(
                        codes::CALLBACK_NOT_EXPORT,
                        a.span(),
                        "a callback C calls is the name of an export",
                    )
                    .primary_label("not an export")
                    .help(
                        "write the name of a `pub extern \"C\" fn … ! {} = \"symbol\" { … }`: \
                         its C entry needs no environment and lives as long as the process, \
                         which a closure or another function value does not",
                    ),
                );
            }
        }
    }

    pub(super) fn record_checked_call(
        &mut self,
        at: Span,
        span: Span,
        args: &[Expr],
        sig: &Signature,
        instance: Option<Instantiation>,
        spawned: bool,
    ) {
        let Some(callee) = self.res.callee(at) else {
            return;
        };
        self.res.record_checked_call(
            at,
            CheckedCall {
                callee,
                span,
                args: args.iter().map(Expr::span).collect(),
                params: sig.params.clone(),
                ret: sig.ret,
                instance,
                spawned,
                preconditions: sig.preconditions,
            },
        );
    }

    /// Record one call of the body being checked, for the native backend's planner.
    pub(super) fn record_site(&mut self, at: Span, type_args: Vec<Type>, spawned: bool) {
        let (Some(id), Some(callee)) = (self.current, self.res.callee(at)) else {
            return;
        };
        self.res.record_call_site(
            id,
            CallSite {
                span: at,
                callee,
                type_args,
                spawned,
                held: self.held(),
            },
        );
    }
}
