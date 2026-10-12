//! Generic calls: inferring type arguments by unification and checking the call they make.

use super::written::wrong_arity;
use super::*;

/// Where a type argument came from, when it was written rather than inferred.
const WRITTEN: usize = usize::MAX;

/// Why an argument's type could not be matched against its declared type.
/// Whether a declared effect set names its function's effect parameter (N51).
pub(super) fn polymorphic(effects: Option<nazm_sema::EffectSet>) -> bool {
    effects.is_some_and(nazm_sema::EffectSet::has_param)
}

#[derive(Debug, Clone, Copy)]
enum Unify {
    /// The callee's effect parameter already bound to another set (N51).
    EffectConflict {
        earlier: nazm_sema::EffectSet,
        from: usize,
    },
    /// A parameter already settled to another type.
    Conflict {
        param: TypeParam,
        earlier: Type,
        from: usize,
    },
    /// The shapes differ: a `Vec` where the signature wants a record, say.
    Mismatch,
}

/// The arguments settled so far, with each unsettled parameter left as itself — so that
/// substituting them into a declared type says what is known and nothing more.
fn settled_args(solved: &[Option<(Type, usize)>], owner: ParamOwner) -> Vec<Type> {
    solved
        .iter()
        .enumerate()
        .map(|(i, s)| {
            s.map_or(
                Type::Param(TypeParam {
                    owner,
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                }),
                |(t, _)| t,
            )
        })
        .collect()
}

/// Mark every parameter of `owner` that `formal` mentions as already accounted for.
fn excuse(types: &nazm_sema::UserTypes, formal: Type, owner: ParamOwner, excused: &mut [bool]) {
    let mut work = vec![formal];
    while let Some(t) = work.pop() {
        if let Type::Param(p) = t {
            if p.owner == owner {
                if let Some(slot) = excused.get_mut(p.index as usize) {
                    *slot = true;
                }
            }
        }
        work.extend(types.type_args(t));
    }
}

/// What `actual` says `param` is, by matching it against `formal` — for the message that
/// reports a conflict, which should name the type the argument implies rather than the
/// argument's whole type.
fn inferred_from(
    types: &nazm_sema::UserTypes,
    formal: Type,
    actual: Type,
    param: TypeParam,
) -> Option<Type> {
    match formal {
        Type::Param(p) if p == param => Some(actual),
        Type::Vec(_)
        | Type::Box(_)
        | Type::List(_)
        | Type::Cell(_)
        | Type::ChanOf(_)
        | Type::Record(_)
        | Type::Enum(_) => types
            .type_args(formal)
            .into_iter()
            .zip(types.type_args(actual))
            .find_map(|(f, a)| inferred_from(types, f, a, param)),
        _ => None,
    }
}

/// `N0390` for a `Chan[elem]` whose element may not cross into a task, or `None` (N53): the
/// rule a `spawn` argument follows, so a channel carries exactly what a task may be given.
pub(super) fn channel_element(
    types: &nazm_sema::UserTypes,
    elem: Type,
    at: Span,
) -> Option<Diagnostic> {
    let why = types.task_safety(elem).err()?;
    let shown = types.show(elem);
    let message = if why.path.is_empty() {
        format!("a channel cannot carry `{shown}`: it may not cross into a task")
    } else {
        format!(
            "a channel cannot carry `{shown}`: its `{}` is a `{}`, which may not cross into a task",
            why.path.join("."),
            types.show(why.ty)
        )
    };
    Some(Diagnostic::error(codes::CHANNEL_ELEMENT, at, message).help(
        "a channel's values reach another task, so they follow the rule a `spawn` argument \
         does: `Int`, `Bool`, `Str`, channels, and records and enums of those",
    ))
}

/// A list of names, quoted, for a diagnostic that has to name several.
pub(super) fn quoted(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Checker<'_> {
    /// Whether `ty` satisfies `T: tr` here (N78, R1b): an impl in this module's scope for `ty`,
    /// or for the generic definition it instantiates whose parameters' requirements its
    /// arguments meet (L11b); a type parameter bound by `tr`; or a derived `Hash` (L6).
    pub(super) fn satisfies(&self, tr: TraitId, ty: Type) -> bool {
        self.satisfies_within(tr, ty, 0)
    }

    fn satisfies_within(&self, tr: TraitId, ty: Type, depth: usize) -> bool {
        if depth > nazm_sema::MAX_TYPE_DEPTH {
            return false;
        }
        if let Type::Param(q) = ty {
            return self.res.traits().bound_of(q) == Some(tr);
        }
        if let Some((imp, args)) = self.res.traits().applying(tr, ty, self.res.types()) {
            let Some(d) = self.res.traits().impl_def(imp) else {
                return false;
            };
            return self.impl_modules.contains(&d.module)
                && d.requires.iter().zip(&args).all(|(req, a)| match req {
                    None => true,
                    Some(nazm_sema::traits::Require::Equality) => {
                        self.res.types().equality(*a).is_ok()
                    }
                    Some(nazm_sema::traits::Require::Trait(t)) => {
                        self.satisfies_within(*t, *a, depth + 1)
                    }
                });
        }
        self.res.traits().derives(tr, ty, self.res.types())
    }

    /// A call to a generic function: settle its type arguments, check its arguments
    /// against them, and return what it produces.
    ///
    /// The arguments are **written** or **inferred from the arguments**, and nothing
    /// else — the expected result type never takes part, which is what keeps a call's
    /// meaning visible in the call (`docs/spec.md`, *Calling a generic function*). Left to
    /// right: an argument whose declared type is already fully known is checked against
    /// it, exactly as a monomorphic call's is, and one whose declared type still mentions
    /// an unsettled parameter is unified with it.
    pub(super) fn check_generic_call(
        &mut self,
        name: &str,
        at: Span,
        written: Option<&[TypeName]>,
        args: &[Expr],
        span: Span,
        sig: &Signature,
    ) -> Type {
        let Some(owner) = sig.owner else {
            return sig.ret;
        };
        let n = sig.generics as usize;
        // Each parameter's argument, and where it came from: an argument's index, or
        // `WRITTEN` for an explicit one.
        let mut solved: Vec<Option<(Type, usize)>> = vec![None; n];
        // The callee's effect parameter, once an argument binds it (N51).
        let mut bound: Option<(nazm_sema::EffectSet, usize)> = None;
        let caller = self.effect_param.as_ref().map(|e| e.1);
        let mut explicit = false;
        // Parameters an argument already failed on. One mistake is one error: an argument
        // of the wrong shape is reported as a mismatch, and the parameters its declared
        // type mentions are not reported again as uninferable.
        let mut excused: Vec<bool> = vec![false; n];
        if let Some(written) = written {
            let resolved: Vec<Type> = written
                .iter()
                .map(|w| self.resolve_written_type(w))
                .collect();
            if resolved.len() == n {
                for (slot, t) in solved.iter_mut().zip(resolved) {
                    *slot = Some((t, WRITTEN));
                }
                explicit = true;
            } else {
                self.errors.push(wrong_arity(name, n, resolved.len(), at));
            }
        }

        for (i, a) in args.iter().enumerate() {
            let Some(formal) = sig.params.get(i).copied() else {
                self.check_expr(a, None);
                continue;
            };
            // Checked against its declared type — as a monomorphic call's argument is — when
            // that type is fully known: it mentions no parameter, or every parameter was
            // **written**. A parameter settled by an earlier argument is not "written": the
            // two arguments are unified, so that `same(1, "x")` is a conflict between them
            // rather than the second one read against what the first one happened to say.
            let open = self.res.types().mentions(formal, owner)
                || (sig.effect_param && self.res.types().names_effect_param(formal, owner));
            if (explicit && !sig.effect_param) || !open {
                let now = settled_args(&solved, owner);
                let want = self.res.types_mut().substitute(formal, owner, &now);
                let got = self.value_type(a, Some(want));
                // As a monomorphic call's argument is (N79, N104): an attenuation is accepted.
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
                continue;
            }
            // What is already known is the expectation — for recovery only: it is what an
            // argument that never produces a value stands in for, and it never settles a
            // parameter.
            let expected = solved.iter().all(Option::is_some).then(|| {
                let now = settled_args(&solved, owner);
                self.res.types_mut().substitute(formal, owner, &now)
            });
            match self.check_expr(a, expected) {
                Completion::Value(got) => {
                    match self.unify(formal, got, owner, &mut solved, &mut bound, caller, i) {
                        Ok(()) => {}
                        Err(Unify::EffectConflict { earlier, from }) => {
                            let pname = self.res.types().effect_param_name(Some(owner));
                            let shown = |set: nazm_sema::EffectSet, me: &Self| {
                                if set.is_empty() {
                                    "none".to_owned()
                                } else {
                                    format!("`{}`", me.show_effects(set))
                                }
                            };
                            let earlier = shown(earlier, self);
                            self.errors.push(
                                Diagnostic::error(
                                    codes::CONFLICTING_EFFECT_ARGUMENT,
                                    a.span(),
                                    format!(
                                        "`{pname}` is {earlier} from argument {}, and argument {} \
                                     binds it to something else",
                                        from + 1,
                                        i + 1
                                    ),
                                )
                                .secondary(sig.name_span, "declared here")
                                .help(format!(
                                    "every use of `{pname}` in `{name}`'s signature is one set of \
                                 effects; give the function values the same effects"
                                )),
                            );
                        }
                        Err(Unify::Conflict {
                            param,
                            earlier,
                            from,
                        }) => {
                            let p = self.res.types().param_name(param);
                            let earlier = self.show(earlier);
                            let source = if from == WRITTEN {
                                "the written type arguments".to_owned()
                            } else {
                                format!("argument {}", from + 1)
                            };
                            let implied =
                                inferred_from(self.res.types(), formal, got, param).unwrap_or(got);
                            let implied = self.show(implied);
                            self.errors.push(
                                Diagnostic::error(
                                    codes::CONFLICTING_TYPE_ARGUMENTS,
                                    a.span(),
                                    format!(
                                    "`{p}` is `{earlier}` from {source}, but argument {} makes \
                                     it `{implied}`",
                                    i + 1
                                ),
                                )
                                .help(format!(
                                    "every use of `{p}` in `{name}`'s signature is one type. \
                                 Inference does not choose the first and convert the rest"
                                )),
                            );
                        }
                        Err(Unify::Mismatch) => {
                            excuse(self.res.types(), formal, owner, &mut excused);
                            let partial = settled_args(&solved, owner);
                            let want = self.res.types_mut().substitute(formal, owner, &partial);
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
                Completion::Unit => {
                    excuse(self.res.types(), formal, owner, &mut excused);
                    self.errors.push(
                        Diagnostic::error(codes::TYPE_MISMATCH, a.span(), "this produces no value")
                            .help(
                                "an `if` used as a value needs an `else`, and a block used as \
                                 one needs a final expression with no `;` after it",
                            ),
                    );
                }
                // An argument that never produces a value says nothing about a type.
                Completion::Diverges => {}
            }
        }

        let missing: Vec<String> = solved
            .iter()
            .enumerate()
            .filter(|(i, s)| s.is_none() && !excused.get(*i).copied().unwrap_or(false))
            .map(|(i, _)| {
                self.res.types().param_name(TypeParam {
                    owner,
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                })
            })
            .collect();
        if !missing.is_empty() {
            self.errors.push(
                Diagnostic::error(
                    codes::CANNOT_INFER_TYPE_ARGUMENT,
                    at,
                    format!(
                        "nothing in this call to `{name}` says what {} {}",
                        quoted(&missing),
                        if missing.len() == 1 { "is" } else { "are" }
                    ),
                )
                .secondary(sig.name_span, "declared here")
                .help(format!(
                    "a type argument is inferred from the arguments and never from what the \
                     result is used for. Write it: `{name}[…](…)`"
                )),
            );
        }

        // What each parameter's declaration requires, of the argument it was given (N14).
        // Asked of the one derivation `==` asks, so a concrete argument answers by N13's rule
        // and a caller's own parameter answers by its declaration: forwarding a `T` to a
        // callee that requires equality works exactly when the caller's `T` requires it too.
        // Written or inferred makes no difference to the rule, only to where it is reported.
        for (i, s) in solved.iter().enumerate() {
            let Some((t, from)) = *s else {
                continue;
            };
            let param = TypeParam {
                owner,
                index: u32::try_from(i).unwrap_or(u32::MAX),
            };
            // A trait bound (N78): the argument has a usable impl, or is a caller's own
            // parameter bound by the same trait.
            if let Some(tr) = self.res.traits().bound_of(param) {
                // Or an impl of the generic definition `t` instantiates (L11b), or derived (L6).
                let satisfied = self.satisfies(tr, t);
                if !satisfied {
                    let p = self.res.types().param_name(param);
                    let shown = self.show(t);
                    let tname = self
                        .res
                        .traits()
                        .get(tr)
                        .map_or_else(|| "?".to_owned(), |d| d.name.clone());
                    let place = if from == WRITTEN {
                        at
                    } else {
                        args.get(from).map_or(at, Expr::span)
                    };
                    // Where `Hash` is derived (L6), why this type does not derive it: the
                    // component responsible, as a refused `==` names it.
                    let underived = if self.res.traits().derived_hash() == Some(tr)
                        && !matches!(t, Type::Param(_))
                    {
                        self.res
                            .types()
                            .hashing(t, |c| self.res.traits().find(tr, c).is_some())
                            .err()
                            .filter(|w| !w.path.is_empty())
                            .map(|w| {
                                format!(
                                    ", and does not derive one: its `{}` is a `{}`",
                                    w.path.join("."),
                                    self.show(w.ty)
                                )
                            })
                    } else {
                        None
                    };
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNSATISFIED_REQUIREMENT,
                            place,
                            format!(
                                "`{name}` requires `{p}: {tname}`, and `{p}` is `{shown}` here, \
                                 which has no impl of `{tname}` in this module's scope{}",
                                underived.unwrap_or_default()
                            ),
                        )
                        .secondary(sig.name_span, "declared here")
                        .help(if let Type::Param(_) = t {
                            format!("declare `{shown}: {tname}` on this definition")
                        } else {
                            format!(
                                "implement it, `impl {tname} for {shown}`, in the module of \
                                 `{tname}` or of `{shown}`, and import that module here"
                            )
                        }),
                    );
                }
                continue;
            }
            if !self.res.types().requires_equality(param) {
                continue;
            }
            let Err(why) = self.res.types().equality(t) else {
                continue;
            };
            let p = self.res.types().param_name(param);
            let shown = self.show(t);
            let detail = if let Type::Param(_) = t {
                format!("this `{shown}` is a type parameter whose declaration does not require it")
            } else if why.path.is_empty() {
                format!("`{shown}` has no equality")
            } else {
                format!(
                    "its `{}` is a `{}`, which has no equality",
                    why.path.join("."),
                    self.show(why.ty)
                )
            };
            // A written argument is reported at the callee, where `N0353` reports the list;
            // an inferred one at the argument it was inferred from.
            let place = if from == WRITTEN {
                at
            } else {
                args.get(from).map_or(at, Expr::span)
            };
            self.errors.push(
                Diagnostic::error(
                    codes::UNSATISFIED_REQUIREMENT,
                    place,
                    if let Type::Param(_) = t {
                        format!("`{name}` requires `{p}: {EQUALITY}`, and {detail}")
                    } else {
                        format!(
                            "`{name}` requires `{p}: {EQUALITY}`, and `{p}` is `{shown}` here: \
                             {detail}"
                        )
                    },
                )
                .secondary(sig.name_span, "declared here")
                .help(if let Type::Param(_) = t {
                    format!(
                        "a generic definition may pass on only what it requires itself: \
                         declare `{shown}: {EQUALITY}`"
                    )
                } else {
                    "equality is derived from what a type contains; a sequence, a `Vec` or a \
                     `Chan` anywhere inside a type gives it none"
                        .to_owned()
                }),
            );
        }

        let settled: Vec<Type> = solved
            .iter()
            .map(|s| s.map_or(Type::Int, |(t, _)| t))
            .collect();
        for t in &settled {
            self.mention(*t);
        }
        // A generic numeric built-in's `T` is in its class (Gate 2): `count_ones` takes an
        // integer, `float_sqrt` a float.
        if let ParamOwner::Builtin(i) = owner {
            if let (Some(class), Some(t)) = (i.requires(), settled.first().copied()) {
                if !class.admits(t) {
                    self.errors.push(Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        at,
                        format!(
                            "`{name}` applies to {}, and this is `{}`",
                            class.describe(),
                            self.show(t)
                        ),
                    ));
                }
            }
        }
        // The one generic built-in that makes a `Chan[T]` from a written `T` (N53).
        if owner == ParamOwner::Builtin(Intrinsic::ChanNewOf) {
            if let Some(d) = settled
                .first()
                .and_then(|t| channel_element(self.res.types(), *t, at))
            {
                self.errors.push(d);
            }
        }
        self.res.record_type_args(at, settled.clone());
        self.record_site(at, settled.clone(), false);
        // What this call settled, for tooling: unsettled parameters stay themselves, and
        // the substitution only reads what already exists, so recording it interns nothing.
        let partial = settled_args(&solved, owner);
        let instance = Instantiation {
            owner,
            written: explicit,
            args: solved.iter().map(|s| s.map(|(t, _)| t)).collect(),
            params: sig
                .params
                .iter()
                .map(|p| self.res.types().substituted(*p, owner, &partial))
                .collect(),
            ret: self.res.types().substituted(sig.ret, owner, &partial),
        };
        let mut instance = instance;
        let mut ret = self.res.types_mut().substitute(sig.ret, owner, &settled);
        // The effect parameter, as this call bound it (N51): what the call performs, and what
        // a function type in its result says — which is the result the call has.
        if sig.effect_param {
            let binding = bound.map_or(nazm_sema::EffectSet::EMPTY, |(b, _)| b);
            self.res.record_effect_binding(at, binding);
            ret = self
                .res
                .types_mut()
                .bind_effects(ret, owner, binding, caller);
            instance.ret = Some(ret);
        }
        self.record_checked_call(at, span, args, sig, Some(instance), false);
        self.mention(ret);
        if self.res.types().nesting(ret) > MAX_TYPE_DEPTH {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_NESTING_TOO_DEEP,
                    span,
                    format!("this call produces a type nested more than {MAX_TYPE_DEPTH} deep"),
                )
                .help("types may nest 32 deep, and the result of a generic call is a type too"),
            );
            return Type::Int;
        }
        ret
    }

    /// Match a declared parameter type against an argument's type, settling the callee's
    /// parameters it mentions. Structural through applications, `Vec` and function types;
    /// nothing else is.
    #[allow(clippy::too_many_arguments)]
    fn unify(
        &self,
        formal: Type,
        actual: Type,
        owner: ParamOwner,
        solved: &mut [Option<(Type, usize)>],
        bound: &mut Option<(nazm_sema::EffectSet, usize)>,
        caller: Option<ParamOwner>,
        arg: usize,
    ) -> Result<(), Unify> {
        let types = self.res.types();
        if !types.mentions(formal, owner) && !types.names_effect_param(formal, owner) {
            return if formal == actual {
                Ok(())
            } else {
                Err(Unify::Mismatch)
            };
        }
        match formal {
            Type::Param(p) if p.owner == owner => {
                let Some(slot) = solved.get_mut(p.index as usize) else {
                    return Err(Unify::Mismatch);
                };
                match *slot {
                    None => {
                        *slot = Some((actual, arg));
                        Ok(())
                    }
                    Some((t, _)) if t == actual => Ok(()),
                    Some((t, from)) => Err(Unify::Conflict {
                        param: p,
                        earlier: t,
                        from,
                    }),
                }
            }
            // `Box` and `List` (R1a) and `Cell` (R1b) as a `Vec` is, each only with its own kind.
            Type::Vec(_) | Type::Box(_) | Type::List(_) | Type::Cell(_) => {
                let (Some(fe), Some(ae)) = (
                    types.type_args(formal).first().copied(),
                    types.type_args(actual).first().copied(),
                ) else {
                    return Err(Unify::Mismatch);
                };
                if std::mem::discriminant(&formal) != std::mem::discriminant(&actual) {
                    return Err(Unify::Mismatch);
                }
                self.unify(fe, ae, owner, solved, bound, caller, arg)
            }
            // A generic channel, as a `Vec` is: through its element (N53).
            Type::ChanOf(_) => {
                let (Type::ChanOf(_), Some(fe), Some(ae)) = (
                    actual,
                    types.type_args(formal).first().copied(),
                    types.type_args(actual).first().copied(),
                ) else {
                    return Err(Unify::Mismatch);
                };
                self.unify(fe, ae, owner, solved, bound, caller, arg)
            }
            Type::Record(_) | Type::Enum(_) => {
                if types.definition_of(formal) != types.definition_of(actual)
                    || std::mem::discriminant(&formal) != std::mem::discriminant(&actual)
                {
                    return Err(Unify::Mismatch);
                }
                let (fa, aa) = (types.type_args(formal), types.type_args(actual));
                if fa.len() != aa.len() {
                    return Err(Unify::Mismatch);
                }
                for (f, a) in fa.into_iter().zip(aa) {
                    self.unify(f, a, owner, solved, bound, caller, arg)?;
                }
                Ok(())
            }
            // Structural through a function type's parameters and result (N50). Its effects
            // are the same set — or, where it names the callee's effect parameter, bind it to
            // what the argument has beyond the named ones (N51).
            Type::Fn(ff) => {
                let Type::Fn(af) = actual else {
                    return Err(Unify::Mismatch);
                };
                let (Some(fs), Some(asig)) = (types.fn_signature(ff), types.fn_signature(af))
                else {
                    return Err(Unify::Mismatch);
                };
                if fs.params.len() != asig.params.len() {
                    return Err(Unify::Mismatch);
                }
                if fs.row == Some(owner) {
                    let named = fs.effects.named();
                    // A parameter in the argument's effects can only be the caller's own.
                    if !named.is_within(asig.effects)
                        || (asig.effects.has_param() && asig.row != caller)
                    {
                        return Err(Unify::Mismatch);
                    }
                    let binding = asig.effects.without(named);
                    match *bound {
                        None => *bound = Some((binding, arg)),
                        Some((b, _)) if b == binding => {}
                        Some((earlier, from)) => {
                            return Err(Unify::EffectConflict { earlier, from });
                        }
                    }
                } else if fs.effects != asig.effects || fs.row != asig.row {
                    return Err(Unify::Mismatch);
                }
                let pairs: Vec<(Type, Type)> = fs
                    .params
                    .iter()
                    .copied()
                    .zip(asig.params.iter().copied())
                    .chain(std::iter::once((fs.ret, asig.ret)))
                    .collect();
                for (f, a) in pairs {
                    self.unify(f, a, owner, solved, bound, caller, arg)?;
                }
                Ok(())
            }
            _ => Err(Unify::Mismatch),
        }
    }
}
