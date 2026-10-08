//! Matching: `match`, its arms and payloads, `?`, and projection.

use super::generic::quoted;
use super::unit::written_key;
use super::*;

impl Checker<'_> {
    /// Check `match scrutinee { … }`.
    ///
    /// The scrutinee is checked once, the arms are checked against the enum it named, and
    /// exhaustiveness is decided here rather than deferred: `docs/spec.md` gives the
    /// checker the language's meaning, and a `match` whose coverage were settled at
    /// lowering would have two answers to one question.
    pub(super) fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: Span,
        expected: Option<Type>,
    ) -> Completion {
        let sty = self.value_type(scrutinee, None);
        let Type::Enum(id) = sty else {
            let shown = self.show(sty);
            self.errors.push(
                Diagnostic::error(
                    codes::MATCH_ON_NON_ENUM,
                    scrutinee.span(),
                    format!("`match` needs an enum, but this is `{shown}`"),
                )
                .help(
                    "`match` selects one variant of an enum. There is no literal, record \
                     or range pattern",
                ),
            );
            return Completion::Value(expected.unwrap_or(Type::Int));
        };

        // One slot per variant, in declaration order: where the arm that covers it was
        // written. That is both the duplicate check and the exhaustiveness check, and
        // keeping them as one structure is what stops the two disagreeing.
        let count = self.res.enums().get(id).map_or(0, |d| d.variants.len());
        let mut covered: Vec<Option<Span>> = vec![None; count];
        // Whether some arm named a variant this enum does not have, or another enum's.
        // Coverage is not reported when one did: the checker does not know which variant
        // the author meant, so "does not cover `B`" would be a second diagnostic about the
        // one mistake already reported. `docs/spec.md`, *One mistake, one error*.
        let mut unresolved = false;
        let mut result: Option<Completion> = None;
        let mut first_value: Option<(Type, Span)> = None;

        for arm in arms {
            let Some(named) =
                self.resolve_enum(&written_key(arm.qualifier.as_ref(), &arm.ty), arm.ty_span)
            else {
                unresolved = true;
                continue;
            };
            // The enum a pattern names is a **definition**: `Maybe.Some(…)` against a
            // `Maybe[Int]` names the enum it is an instance of, and the scrutinee supplies
            // the arguments. A different definition is still another enum's variant.
            if self.res.types().definition_of(sty) != Some(TypeNode::Enum(named)) {
                let (want, got) = (self.show(sty), self.show(Type::Enum(named)));
                self.errors.push(
                    Diagnostic::error(
                        codes::WRONG_ENUM_IN_PATTERN,
                        arm.ty_span,
                        format!("this pattern is a `{got}`, but the value is a `{want}`"),
                    )
                    .secondary(scrutinee.span(), format!("this is a `{want}`"))
                    .help(
                        "a variant belongs to its enum, so a pattern names the scrutinee's \
                         own enum. There is no structural matching between two enums that \
                         happen to have a variant of the same name",
                    ),
                );
                unresolved = true;
                continue;
            }
            let Some(at) = self.find_variant(id, &arm.variant, arm.variant_span) else {
                unresolved = true;
                continue;
            };
            if let Some(Some(first)) = covered.get(at as usize).copied() {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_MATCH_ARM,
                        arm.variant_span,
                        format!("`{owner}` already has an arm"),
                    )
                    .secondary(first, "first covered here")
                    .help(
                        "the discriminant selects exactly one arm, so a second one for the \
                         same variant can never run",
                    ),
                );
                continue;
            }
            if let Some(slot) = covered.get_mut(at as usize) {
                *slot = Some(arm.variant_span);
            }
            self.res.record_variant(
                arm.variant_span,
                VariantRef {
                    enumeration: id,
                    variant: at,
                },
            );

            let completion = self.check_arm(id, at, arm, expected.or(first_value.map(|(t, _)| t)));
            if let Completion::Value(t) = completion {
                match first_value {
                    None => first_value = Some((t, arm.body.span())),
                    Some((want, at_span)) if want != t => {
                        let (swant, sgot) = (self.show(want), self.show(t));
                        self.errors.push(
                            Diagnostic::error(
                                codes::BRANCH_TYPES_DIFFER,
                                arm.body.span(),
                                format!("this arm is `{sgot}`, but an earlier one is `{swant}`"),
                            )
                            .secondary(at_span, format!("this one is `{swant}`"))
                            .help("every arm of a `match` must have the same type"),
                        );
                    }
                    Some(_) => {}
                }
            }
            result = Some(match (result, completion) {
                (None, c) => c,
                // One arm leaving means the others decide, exactly as one branch of an
                // `if` does.
                (Some(Completion::Diverges), c) | (Some(c), Completion::Diverges) => c,
                (Some(Completion::Unit), _) | (Some(_), Completion::Unit) => Completion::Unit,
                (Some(a), _) => a,
            });
        }

        let missing: Vec<String> = covered
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_none())
            .filter_map(|(i, _)| {
                self.res
                    .enums()
                    .get(id)
                    .and_then(|d| d.variants.get(i))
                    .map(|v| v.name.clone())
            })
            .collect();
        if !missing.is_empty() && !unresolved {
            let name = self.res.types().show(sty).to_owned();
            let list = quoted(&missing);
            let declared = self.res.enums().get(id).map(|d| d.name_span);
            let mut d = Diagnostic::error(
                codes::NON_EXHAUSTIVE_MATCH,
                span,
                format!("this `match` does not cover {list}"),
            )
            .help(format!(
                "an enum is closed, so every variant of `{name}` gets an arm. There is no \
                 `_` arm: adding a variant is meant to break the matches that believed \
                 they had handled all of them"
            ));
            if let Some(at) = declared {
                d = d.secondary(at, "declared here");
            }
            self.errors.push(d);
        }

        result.unwrap_or(Completion::Value(expected.unwrap_or(Type::Int)))
    }

    /// Whether `t` is an instance of the core prelude's `Result`, and if so its success
    /// and error types.
    ///
    /// By **definition identity**: the application's definition must be the prelude's
    /// `Result`. Nothing here reads a name or looks at variants, so a project enum called
    /// `Result`, or one shaped `Ok`/`Err`, is simply not one.
    fn result_parts(&self, t: Type) -> Option<(EnumId, Type, Type)> {
        let Type::Enum(id) = t else { return None };
        let def = self.result?;
        if self.res.types().definition_of(t) != Some(TypeNode::Enum(def)) {
            return None;
        }
        match self.res.types().type_args(t).as_slice() {
            [ok, err] => Some((id, *ok, *err)),
            _ => None,
        }
    }

    /// `operand?`.
    ///
    /// # The rule
    ///
    /// The operand is a `Result[T, E]` and the enclosing function returns `Result[U, E]` —
    /// the **same** `E`, compared as types, so two error types with identical fields are
    /// still two error types. The expression is a `T`. `U` is free: `?` yields the
    /// operand's success value where it is written, and what leaves is only the error.
    ///
    /// # Completion
    ///
    /// Exactly the operand's. The `Err` path leaves the function, which is a `return`, and
    /// a `return` inside an expression is already how a value position can transfer; the
    /// path that stays is `Value(T)`. So `?` needs no completion of its own, and
    /// `docs/spec.md` says why that is the point.
    pub(super) fn check_propagate(
        &mut self,
        operand: &Expr,
        at: Span,
        expected: Option<Type>,
    ) -> Completion {
        let completion = self.check_expr(operand, None);
        let ot = match completion {
            Completion::Value(t) => t,
            // An operand that never produces a value: nothing is taken apart, and the
            // expression transfers with it.
            Completion::Diverges => return Completion::Diverges,
            Completion::Unit => {
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        operand.span(),
                        "this produces no value",
                    )
                    .help("`?` takes apart a `Result`, and this is not a value at all"),
                );
                return Completion::Value(expected.unwrap_or(Type::Int));
            }
        };

        let Some((operand_id, ok, err)) = self.result_parts(ot) else {
            let shown = self.show(ot);
            self.errors.push(
                Diagnostic::error(
                    codes::PROPAGATE_NON_RESULT,
                    at,
                    format!("`?` needs a `Result`, but this is `{shown}`"),
                )
                .secondary(operand.span(), format!("this is `{shown}`"))
                .help(
                    "`?` propagates the core prelude's `Result` and nothing else — not an \
                     `Option`, and not an enum with the same variants. Take the value \
                     apart with `match` instead",
                ),
            );
            return Completion::Value(expected.unwrap_or(Type::Int));
        };

        let declared = self.return_type;
        let Some((into, _, into_err)) = declared.and_then(|t| self.result_parts(t)) else {
            let shown = declared.map_or_else(|| "nothing".to_owned(), |t| self.show(t));
            let mut d = Diagnostic::error(
                codes::PROPAGATE_OUTSIDE_RESULT,
                at,
                format!("`?` can only be used in a function that returns a `Result`, and this one returns `{shown}`"),
            )
            .help(
                "`?` leaves the function with the error, so the function has to return a \
                 `Result` with the same error type. Here, `match` on the value and decide \
                 what the error means instead",
            );
            if let Some(span) = self.return_span {
                d = d.secondary(span, format!("declared `{shown}` here"));
            }
            self.errors.push(d);
            return Completion::Value(ok);
        };

        if err != into_err {
            let (have, want) = (self.show(err), self.show(into_err));
            let mut d = Diagnostic::error(
                codes::PROPAGATE_ERROR_MISMATCH,
                at,
                format!(
                    "this `?` propagates a `{have}`, but the function's error type is `{want}`"
                ),
            )
            .secondary(operand.span(), format!("its error type is `{have}`"))
            .help(
                "there is no conversion between error types: `?` returns the error value \
                 as it is. Match on the value and build the error this function returns",
            );
            if let Some(span) = self.return_span {
                d = d.secondary(span, format!("the function returns `{want}` errors"));
            }
            self.errors.push(d);
            return Completion::Value(ok);
        }

        let variant = |c: &Self, id: EnumId, name: &str| {
            c.res
                .enums()
                .get(id)
                .and_then(|d| d.variant(name))
                .map(|(i, _)| i)
        };
        if let (Some(ok_at), Some(err_at), Some(into_at)) = (
            variant(self, operand_id, "Ok"),
            variant(self, operand_id, "Err"),
            variant(self, into, "Err"),
        ) {
            self.res.record_propagation(
                at,
                Propagation {
                    operand: operand_id,
                    ok: ok_at,
                    err: err_at,
                    into,
                    into_err: into_at,
                },
            );
        }
        Completion::Value(ok)
    }

    /// One arm: its payload bindings, then its body, in a scope of their own.
    fn check_arm(
        &mut self,
        id: EnumId,
        at: u32,
        arm: &MatchArm,
        expected: Option<Type>,
    ) -> Completion {
        self.open_scope(arm.span);
        let mut named: Vec<(String, Span)> = Vec::new();
        for f in &arm.fields {
            let declared = self
                .variant_def(id, at)
                .and_then(|v| v.field(&f.name).map(|(i, fd)| (i, fd.ty)));
            let Some((index, ty)) = declared else {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        format!("`{owner}` has no payload field `{}`", f.name),
                    )
                    .secondary(
                        self.variant_span(id, at).unwrap_or(arm.variant_span),
                        "declared here",
                    ),
                );
                continue;
            };
            if let Some((_, first)) = named.iter().find(|(g, _)| *g == f.name) {
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        f.name_span,
                        format!("this pattern names `{}` twice", f.name),
                    )
                    .secondary(*first, "first named here"),
                );
                continue;
            }
            named.push((f.name.clone(), f.name_span));
            self.res.record_payload(
                f.name_span,
                PayloadRef {
                    enumeration: id,
                    variant: at,
                    field: index,
                },
            );
            // `_` names nothing. The field is still resolved above, because it still
            // exists and is still released with the value — `docs/spec.md` is explicit
            // that a discard declines to name a payload rather than forgetting it.
            let Some(binding) = &f.binding else { continue };
            if self.scopes.defined_here(binding) {
                self.errors.push(
                    Diagnostic::error(
                        codes::ALREADY_DEFINED_IN_SCOPE,
                        f.binding_span,
                        format!("`{binding}` is bound twice by this pattern"),
                    )
                    .help("each payload field binds a name of its own, or is discarded with `_`"),
                );
                continue;
            }
            // Not mutable: a pattern binding is a borrowed, immutable view of the payload
            // the match's scrutinee holds. That is a statement about the *name* — a
            // binding that names a sequence still reaches shared mutable storage, which
            // `docs/spec.md` says in as many words.
            let slot = self.allocate(binding, f.binding_span, ty, false);
            self.record_var(f.binding_span, slot);
            self.define(
                binding,
                ty,
                false,
                Some(f.binding_span),
                slot,
                f.binding_span.end,
            );
        }

        let missing = self.missing_payload_fields(id, at, &named);
        if !missing.is_empty() {
            let owner = self.variant_path(id, at);
            let list = quoted(&missing);
            self.errors.push(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    arm.variant_span,
                    format!("this pattern does not name {list} of `{owner}`"),
                )
                .help(
                    "a pattern names every payload field, binding it or discarding it with \
                     `_`. There is no rest pattern, so adding a payload field breaks the \
                     patterns that did not know about it rather than ignoring it",
                ),
            );
        }

        let completion = self.check_expr(&arm.body, expected);
        self.close_scope();
        completion
    }

    /// The variant of this name, reported if the enum does not have one.
    pub(super) fn find_variant(&mut self, id: EnumId, name: &str, at: Span) -> Option<u32> {
        if let Some((i, _)) = self.res.enums().get(id).and_then(|d| d.variant(name)) {
            return Some(i);
        }
        // Which enum the name was looked for in, for a tool completing it (N21).
        self.res.record_unresolved_member(at, Type::Enum(id));
        let shown = self.res.types().show(Type::Enum(id)).to_owned();
        let known: Vec<String> = self
            .res
            .enums()
            .get(id)
            .map(|d| d.variants.iter().map(|v| v.name.clone()).collect())
            .unwrap_or_default();
        let declared = self.res.enums().get(id).map(|d| d.name_span);
        let mut d = Diagnostic::error(
            codes::UNKNOWN_VARIANT,
            at,
            format!("`{shown}` has no variant `{name}`"),
        );
        if let Some(span) = declared {
            d = d.secondary(span, "declared here");
        }
        if !known.is_empty() {
            d = d.help(format!("its variants are {}", quoted(&known)));
        }
        self.errors.push(d);
        None
    }

    pub(super) fn variant_def(&self, id: EnumId, at: u32) -> Option<&VariantDef> {
        self.res.enums().get(id)?.variants.get(at as usize)
    }

    pub(super) fn variant_span(&self, id: EnumId, at: u32) -> Option<Span> {
        self.variant_def(id, at).map(|v| v.name_span)
    }

    /// `Enum.Variant`, for a message. A spelling, not an identity.
    pub(super) fn variant_path(&self, id: EnumId, at: u32) -> String {
        let e = self.res.types().show(Type::Enum(id));
        match self.variant_def(id, at) {
            Some(v) => format!("{e}.{}", v.name),
            None => e.to_owned(),
        }
    }

    /// The payload fields of this variant that `given` does not mention.
    pub(super) fn missing_payload_fields(
        &self,
        id: EnumId,
        at: u32,
        given: &[(String, Span)],
    ) -> Vec<String> {
        self.variant_def(id, at).map_or_else(Vec::new, |v| {
            v.fields
                .iter()
                .filter(|f| !given.iter().any(|(g, _)| *g == f.name))
                .map(|f| f.name.clone())
                .collect()
        })
    }

    /// Check `base.name` and return the field's type.
    ///
    /// Field lookup begins at the base's **resolved type** and goes no further: there is
    /// no global field namespace, so a name that is a field of some other record is not a
    /// field of this one. `docs/spec.md`, *Field resolution starts from the type*.
    pub(super) fn check_projection(
        &mut self,
        base_ty: Type,
        name: &str,
        name_span: Span,
    ) -> Option<Type> {
        // `x.show` with no call and no field of that name, where a trait in scope has the
        // method (N78): methods are not values.
        let is_field = matches!(base_ty, Type::Record(id)
            if self.res.records().get(id).is_some_and(|d| d.field(name).is_some()));
        let a_method = self.trait_names.values().flatten().any(|t| {
            self.res
                .traits()
                .get(*t)
                .is_some_and(|d| d.method(name).is_some())
        });
        if !is_field && a_method {
            self.errors.push(
                Diagnostic::error(
                    codes::METHOD_NOT_A_VALUE,
                    name_span,
                    format!("`.{name}` is a method, and a method is called, not taken as a value"),
                )
                .help(format!(
                    "call it, `x.{name}()`, or make a value of the call with a closure: \
                     `fn() -> … {{ x.{name}() }}`"
                )),
            );
            return None;
        }
        let Type::Record(id) = base_ty else {
            let shown = self.show(base_ty);
            let help = if base_ty.is_enum() {
                // An enum's payload depends on its active variant, so there is nothing a
                // projection could mean. `docs/spec.md` gives `match` as the one
                // elimination form, and saying so is more use than "not a record".
                "which fields an enum value has depends on its active variant, so a \
                 payload is read by `match` and not by `.`"
            } else {
                "`.` reads a declared field out of a record value"
            };
            self.errors.push(
                Diagnostic::error(
                    codes::INVALID_PROJECTION,
                    name_span,
                    format!("`{shown}` is not a record, so `.{name}` has nothing to read"),
                )
                .help(help),
            );
            return None;
        };
        let found = self
            .res
            .records()
            .get(id)
            .and_then(|d| d.field(name).map(|(i, f)| (i, f.ty)));
        let Some((index, ty)) = found else {
            // What the name was looked for on, for a tool completing it (N21).
            self.res.record_unresolved_member(name_span, base_ty);
            let shown = self.show(base_ty);
            let at = self.res.records().get(id).map(|d| d.name_span);
            let mut d = Diagnostic::error(
                codes::UNKNOWN_FIELD,
                name_span,
                format!("`{shown}` has no field `{name}`"),
            );
            if let Some(at) = at {
                d = d.secondary(at, "declared here");
            }
            self.errors.push(d.help(
                "a field is looked for on the type of the value it is read from, and \
                 nowhere else",
            ));
            return None;
        };
        self.res.record_field(
            name_span,
            FieldRef {
                record: id,
                field: index,
            },
        );
        Some(ty)
    }
}
