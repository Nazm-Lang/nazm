//! Matching: `match`, its arms and payloads, `?`, and projection.

use super::generic::quoted;
use super::unit::written_key;
use super::usefulness;
use super::*;

impl Checker<'_> {
    /// Check `match scrutinee { … }` — written, or what `if let` or `let … else` means (R1a).
    ///
    /// The scrutinee is checked once, each arm's pattern against its type and its body with
    /// the pattern's bindings, and exhaustiveness and reachability are decided here rather
    /// than deferred: `docs/spec.md` gives the checker the language's meaning, and a `match`
    /// whose coverage were settled at lowering would have two answers to one question.
    pub(super) fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        source: nazm_syntax::MatchSource,
        span: Span,
        expected: Option<Type>,
    ) -> Completion {
        let sty = self.value_type(scrutinee, None);
        let errors_before = self.errors.len();
        // A variant pattern needs an enum; before R1a every `match` did.
        // An empty `match` of anything else matches nothing, and is refused as before R1a.
        let wants_enum = arms.is_empty() || arms.iter().any(|a| has_variant_pattern(&a.pattern));
        if wants_enum && !matches!(sty, Type::Enum(_)) {
            let shown = self.show(sty);
            self.errors.push(
                Diagnostic::error(
                    codes::MATCH_ON_NON_ENUM,
                    scrutinee.span(),
                    format!("`match` needs an enum, but this is `{shown}`"),
                )
                .help(
                    "a variant pattern selects one variant of an enum. A `Bool`, an integer or \
                     a string is matched by literals, and any value by `_` or a name",
                ),
            );
            return Completion::Value(expected.unwrap_or(Type::Int));
        }

        // Whether some pattern named a variant this enum does not have, or another enum's, or
        // was otherwise refused: coverage is not reported then — the checker does not know
        // what the author meant, so "does not cover `B`" would be a second diagnostic about
        // the one mistake already reported. `docs/spec.md`, *One mistake, one error*.
        let mut unresolved = false;
        let mut rows: Vec<Vec<usefulness::Pat>> = Vec::new();
        // Which arm each row is: a guarded arm adds none (R1, 5b.17).
        let mut row_arms: Vec<usize> = Vec::new();
        let mut result: Option<Completion> = None;
        let mut first_value: Option<(Type, Span)> = None;

        for (index, arm) in arms.iter().enumerate() {
            self.open_scope(arm.span);
            let mut binds: Vec<(String, Span, Type)> = Vec::new();
            let mut aliases: Vec<(String, Span)> = Vec::new();
            let before = self.errors.len();
            let pat = self.check_pattern(&arm.pattern, sty, scrutinee, &mut binds, &mut aliases);
            if self.errors.len() > before {
                // The pattern is the mistake: its arm's body would only repeat it, naming
                // bindings that were never made (`docs/spec.md`, *One mistake, one error*).
                unresolved = true;
                self.close_scope();
                continue;
            }
            // Bindings, each immutable: a borrowed view of what the scrutinee holds. That is
            // a statement about the *name* — a binding that names a sequence still reaches
            // shared mutable storage, which `docs/spec.md` says in as many words.
            let mut slots: Vec<(String, u32)> = Vec::new();
            for (name, at, ty) in &binds {
                if self.scopes.defined_here(name) {
                    self.errors.push(
                        Diagnostic::error(
                            codes::ALREADY_DEFINED_IN_SCOPE,
                            *at,
                            format!("`{name}` is bound twice by this pattern"),
                        )
                        .help("each part of a pattern binds a name of its own, or is `_`"),
                    );
                    continue;
                }
                let slot = self.allocate(name, *at, *ty, false);
                self.record_var(*at, slot);
                self.define(name, *ty, false, Some(*at), slot, at.end);
                slots.push((name.clone(), slot));
            }
            // An or-pattern's later alternatives bind the same names: the same bindings.
            for (name, at) in &aliases {
                if let Some((_, slot)) = slots.iter().find(|(n, _)| n == name) {
                    self.record_var(*at, *slot);
                }
            }

            // A guard (R1, 5b.17): a `Bool`, with the pattern's names bound.
            if let Some(g) = &arm.guard {
                let gt = self.value_type(g, Some(Type::Bool));
                if gt != Type::Bool {
                    self.errors.push(
                        Diagnostic::error(
                            codes::CONDITION_NOT_BOOL,
                            g.span(),
                            format!(
                                "a `match` arm's guard must be `Bool`, but this is `{}`",
                                self.show(gt)
                            ),
                        )
                        .help("Nazm has no truthiness: use a comparison such as `x != 0`"),
                    );
                }
            }

            // Reachable: something the arms above do not match.
            if !unresolved {
                let sig = Sigs(self);
                if !usefulness::useful(&rows, std::slice::from_ref(&pat), &[sty], &sig) {
                    self.unreachable_arm(arms, index, &rows, &row_arms, &pat);
                }
            }
            // A guarded arm covers nothing for exhaustiveness or the arms below it: its guard
            // may be false for any value its pattern matches.
            if arm.guard.is_none() {
                rows.push(vec![pat]);
                row_arms.push(index);
            }

            let completion = self.check_expr(&arm.body, expected.or(first_value.map(|(t, _)| t)));
            self.close_scope();
            // `let … else`: the second arm is the `else`, which must leave (R1a).
            if source == nazm_syntax::MatchSource::LetElse
                && index == 1
                && completion != Completion::Diverges
            {
                self.errors.push(
                    Diagnostic::error(
                        codes::LET_ELSE_COMPLETES,
                        arm.span,
                        "a `let … else`'s `else` must leave, and this one can finish",
                    )
                    .help(
                        "end it with `return`, `break` or `continue`: the names the pattern \
                         binds do not exist when it does not match",
                    ),
                );
                continue;
            }
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

        if !unresolved {
            self.report_uncovered(sty, &rows, span, source);
        }
        // A `_` that covers variants (L15), only where the `match` is otherwise right.
        if !unresolved
            && source == nazm_syntax::MatchSource::Written
            && self.errors.len() == errors_before
        {
            self.warn_covering(sty, arms);
        }
        match result {
            Some(c) => c,
            // Every arm's pattern was refused: what the `match` would produce is unknown, and
            // saying so again — a loop body with a value, a missing tail — would be a second
            // diagnostic about the one mistake.
            None if unresolved => Completion::Diverges,
            None => Completion::Value(expected.unwrap_or(Type::Int)),
        }
    }

    /// `N0632`, a warning (R1b, L15): a `match` on an enum whose `_`, or bare name, covers
    /// variants no arm names — so a variant added to the enum later is covered too, and the
    /// change that should have been reported at every `match` is reported at none. `docs/spec.md`,
    /// *Patterns*, which kept this voice when it admitted `_` (L3).
    fn warn_covering(&mut self, sty: Type, arms: &[MatchArm]) {
        let Type::Enum(id) = sty else { return };
        let Some(def) = self.res.enums().get(id) else {
            return;
        };
        let variants: Vec<String> = def.variants.iter().map(|v| v.name.clone()).collect();
        fn named(p: &nazm_syntax::Pattern, out: &mut Vec<String>) {
            match p {
                nazm_syntax::Pattern::Variant(v) => out.push(v.variant.clone()),
                nazm_syntax::Pattern::Or { alts, .. } => alts.iter().for_each(|a| named(a, out)),
                _ => {}
            }
        }
        let mut seen = Vec::new();
        for a in arms {
            named(&a.pattern, &mut seen);
        }
        let Some(catch_all) = arms.iter().find(|a| {
            a.guard.is_none()
                && matches!(
                    a.pattern,
                    nazm_syntax::Pattern::Wildcard { .. } | nazm_syntax::Pattern::Bind { .. }
                )
        }) else {
            return;
        };
        let covered: Vec<&String> = variants.iter().filter(|v| !seen.contains(v)).collect();
        if covered.is_empty() {
            return;
        }
        let shown = self.show(sty);
        let base = shown.split('[').next().unwrap_or(&shown).to_owned();
        let list = covered
            .iter()
            .map(|v| format!("`{base}.{v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let which = if matches!(catch_all.pattern, nazm_syntax::Pattern::Wildcard { .. }) {
            "`_`".to_owned()
        } else {
            "this name".to_owned()
        };
        // `codes::WILDCARD_COVERS_VARIANTS`'s text: resolution holds no `nazm-diag` type.
        self.res.warn(nazm_sema::resolve::Warning {
            code: "N0632",
            span: catch_all.pattern.span(),
            message: format!(
                "{which} covers {list}, which no arm names: a variant added to `{base}` later is \
                 covered too, without a word"
            ),
            help: Some(format!(
                "name them, so that adding a variant to `{base}` is reported at this `match`"
            )),
        });
    }

    /// `N0344` for an arm whose every value an earlier arm matches — said as before R1a when
    /// both are one variant's plain pattern.
    fn unreachable_arm(
        &mut self,
        arms: &[MatchArm],
        index: usize,
        rows: &[Vec<usefulness::Pat>],
        row_arms: &[usize],
        pat: &usefulness::Pat,
    ) {
        let Some(arm) = arms.get(index) else { return };
        let plain = |p: &usefulness::Pat| match p {
            usefulness::Pat::Ctor {
                enumeration,
                variant,
                args,
            } if args.iter().all(|a| *a == usefulness::Pat::Wild) => Some((*enumeration, *variant)),
            _ => None,
        };
        if let (Some((id, at)), nazm_syntax::Pattern::Variant(v)) = (plain(pat), &arm.pattern) {
            let first = rows
                .iter()
                .zip(row_arms.iter().filter_map(|i| arms.get(*i)))
                .find(|(r, _)| r.first().and_then(plain) == Some((id, at)))
                .and_then(|(_, a)| match &a.pattern {
                    nazm_syntax::Pattern::Variant(w) => Some(w.variant_span),
                    _ => None,
                });
            if let Some(first) = first {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_MATCH_ARM,
                        v.variant_span,
                        format!("`{owner}` already has an arm"),
                    )
                    .secondary(first, "first covered here")
                    .help(
                        "the discriminant selects exactly one arm, so a second one for the \
                         same variant can never run",
                    ),
                );
                return;
            }
        }
        self.errors.push(
            Diagnostic::error(
                codes::DUPLICATE_MATCH_ARM,
                arm.pattern.span(),
                "this arm can never run: the arms above match every value it does",
            )
            .help("arms are tried in order, and the first that matches runs"),
        );
    }

    /// `N0343` for a `match` some value of `sty` escapes — said as before R1a, listing the
    /// variants no arm names, when every arm is one variant's plain pattern.
    fn report_uncovered(
        &mut self,
        sty: Type,
        rows: &[Vec<usefulness::Pat>],
        span: Span,
        source: nazm_syntax::MatchSource,
    ) {
        let witness = {
            let sig = Sigs(self);
            usefulness::witness(rows, &[sty], &sig)
                .map(|w| w.first().map_or_else(|| "_".to_owned(), |x| x.show(&sig)))
        };
        let Some(shown_witness) = witness else { return };
        // `let P = e;` (L5) with a pattern that can fail.
        if source == nazm_syntax::MatchSource::Let {
            self.errors.push(
                Diagnostic::error(
                    codes::NON_EXHAUSTIVE_MATCH,
                    span,
                    format!("this `let` pattern does not match `{shown_witness}`"),
                )
                .help("a pattern that can fail needs an `else`: `let P = e else { return …; };`"),
            );
            return;
        }
        let all_plain = rows.iter().all(|r| {
            matches!(r.first(), Some(usefulness::Pat::Ctor { args, .. })
                if args.iter().all(|a| *a == usefulness::Pat::Wild))
        });
        if let (Type::Enum(id), true) = (sty, all_plain) {
            let covered: Vec<u32> = rows
                .iter()
                .filter_map(|r| match r.first() {
                    Some(usefulness::Pat::Ctor { variant, .. }) => Some(*variant),
                    _ => None,
                })
                .collect();
            let missing: Vec<String> = self
                .res
                .enums()
                .get(id)
                .map(|d| {
                    d.variants
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !covered.contains(&u32::try_from(*i).unwrap_or(u32::MAX)))
                        .map(|(_, v)| v.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            let name = self.res.types().show(sty).to_owned();
            let list = quoted(&missing);
            let declared = self.res.enums().get(id).map(|d| d.name_span);
            let mut d = Diagnostic::error(
                codes::NON_EXHAUSTIVE_MATCH,
                span,
                format!("this `match` does not cover {list}"),
            )
            .help(format!(
                "an enum is closed, so every variant of `{name}` gets an arm — or a `_` covers \
                 the rest, which gives up being told when a variant is added"
            ));
            if let Some(at) = declared {
                d = d.secondary(at, "declared here");
            }
            self.errors.push(d);
            return;
        }
        let help = if matches!(sty, Type::Enum(_) | Type::Bool) {
            "add an arm for it, or end with `_` or a name to cover the rest"
        } else {
            "an integer or a string has more values than any list of literals: end with `_` \
             or a name to cover the rest"
        };
        self.errors.push(
            Diagnostic::error(
                codes::NON_EXHAUSTIVE_MATCH,
                span,
                format!("this `match` does not cover `{shown_witness}`"),
            )
            .help(help),
        );
    }

    /// Check one pattern against a value of type `ty`, collecting what it binds — and, for an
    /// or-pattern's later alternatives, where they bind the same names — and returning what it
    /// matches, for exhaustiveness.
    fn check_pattern(
        &mut self,
        pattern: &nazm_syntax::Pattern,
        ty: Type,
        scrutinee: &Expr,
        binds: &mut Vec<(String, Span, Type)>,
        aliases: &mut Vec<(String, Span)>,
    ) -> usefulness::Pat {
        use nazm_syntax::Pattern as P;
        use usefulness::Pat;
        match pattern {
            P::Wildcard { .. } => Pat::Wild,
            P::Bind { name, span } => {
                binds.push((name.clone(), *span, ty));
                Pat::Wild
            }
            P::Literal(e) => self.check_literal_pattern(e, ty),
            P::Variant(v) => self.check_variant_pattern(v, ty, scrutinee, binds, aliases),
            // `(p, q, …)` (L5): a tuple of as many elements, each matching its pattern.
            P::Tuple { elems, span } => {
                let fields = self.res.types().tuple_elements(ty);
                match fields {
                    Some(fields) if fields.len() == elems.len() => {
                        let args = elems
                            .iter()
                            .zip(fields)
                            .map(|(p, t)| self.check_pattern(p, t, scrutinee, binds, aliases))
                            .collect();
                        Pat::Tuple(args)
                    }
                    _ => {
                        let shown = self.show(ty);
                        self.errors.push(Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            *span,
                            format!(
                                "this pattern is a tuple of {}, but the value is a `{shown}`",
                                elems.len()
                            ),
                        ));
                        Pat::Wild
                    }
                }
            }
            P::Or { alts, span } => {
                let mut out = Vec::new();
                let mut first: Option<Vec<(String, Span, Type)>> = None;
                for alt in alts {
                    let mut own = Vec::new();
                    out.push(self.check_pattern(alt, ty, scrutinee, &mut own, aliases));
                    match &first {
                        None => first = Some(own),
                        Some(f) => {
                            let names = |b: &[(String, Span, Type)]| {
                                let mut v: Vec<(String, Type)> =
                                    b.iter().map(|(n, _, t)| (n.clone(), *t)).collect();
                                v.sort_by(|a, b| a.0.cmp(&b.0));
                                v
                            };
                            if names(f) == names(&own) {
                                aliases.extend(own.into_iter().map(|(n, at, _)| (n, at)));
                            } else {
                                self.errors.push(
                                    Diagnostic::error(
                                        codes::OR_PATTERN_BINDINGS,
                                        alt.span(),
                                        "this alternative binds other names, or other types, \
                                         than the first",
                                    )
                                    .secondary(*span, "in this or-pattern")
                                    .help(
                                        "the arm's body sees the same names whichever \
                                         alternative matched, so each binds them all, with one \
                                         type each",
                                    ),
                                );
                            }
                        }
                    }
                }
                binds.extend(first.unwrap_or_default());
                Pat::Or(out)
            }
        }
    }

    /// A literal pattern (R1a): an integer (perhaps negated), a string, `true` or `false`, of
    /// the scrutinee's type.
    fn check_literal_pattern(&mut self, e: &Expr, ty: Type) -> usefulness::Pat {
        use usefulness::Pat;
        let key = match e {
            Expr::Int { value, .. } => Some(value.to_string()),
            Expr::WideInt { value, .. } => Some(value.to_string()),
            Expr::Neg { operand, .. } => match operand.as_ref() {
                Expr::Int { value, .. } => Some(format!("-{value}")),
                Expr::WideInt { value, .. } => Some(format!("-{value}")),
                _ => None,
            },
            Expr::Str { value, .. } => Some(format!("{value:?}")),
            Expr::Bool { value, .. } => Some(value.to_string()),
            _ => None,
        };
        let literal_type = ty.is_integer() || matches!(ty, Type::Bool | Type::Str | Type::Text);
        let Some(key) = key.filter(|_| literal_type) else {
            let shown = self.show(ty);
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    e.span(),
                    if literal_type {
                        "a literal pattern is an integer, a string, `true` or `false`".to_owned()
                    } else {
                        format!("a `{shown}` has no literal pattern")
                    },
                )
                .help(
                    "a `Bool`, an integer or a string is matched by literals; a float's \
                     equality is not what a literal reads as, so it has none",
                ),
            );
            return Pat::Wild;
        };
        let t = self.value_type(e, Some(ty));
        if t != ty {
            let (want, got) = (self.show(ty), self.show(t));
            self.errors.push(Diagnostic::error(
                codes::TYPE_MISMATCH,
                e.span(),
                format!("this pattern is a `{got}`, but the value is a `{want}`"),
            ));
            return Pat::Wild;
        }
        match e {
            Expr::Bool { value, .. } => Pat::Bool(*value),
            _ => Pat::Lit(key),
        }
    }

    /// `Enum.Variant(field: pattern, …)` against a value of type `ty`.
    fn check_variant_pattern(
        &mut self,
        v: &nazm_syntax::VariantPattern,
        ty: Type,
        scrutinee: &Expr,
        binds: &mut Vec<(String, Span, Type)>,
        aliases: &mut Vec<(String, Span)>,
    ) -> usefulness::Pat {
        use usefulness::Pat;
        let Type::Enum(id) = ty else {
            let shown = self.show(ty);
            self.errors.push(Diagnostic::error(
                codes::MATCH_ON_NON_ENUM,
                v.ty_span,
                format!("a variant pattern needs an enum, but this is a `{shown}`"),
            ));
            return Pat::Wild;
        };
        let Some(named) = self.resolve_enum(&written_key(v.qualifier.as_ref(), &v.ty), v.ty_span)
        else {
            return Pat::Wild;
        };
        // The enum a pattern names is a **definition**: `Maybe.Some(…)` against a
        // `Maybe[Int]` names the enum it is an instance of, and the value supplies the
        // arguments. A different definition is still another enum's variant.
        if self.res.types().definition_of(ty) != Some(TypeNode::Enum(named)) {
            let (want, got) = (self.show(ty), self.show(Type::Enum(named)));
            self.errors.push(
                Diagnostic::error(
                    codes::WRONG_ENUM_IN_PATTERN,
                    v.ty_span,
                    format!("this pattern is a `{got}`, but the value is a `{want}`"),
                )
                .secondary(scrutinee.span(), format!("this is a `{want}`"))
                .help(
                    "a variant belongs to its enum, so a pattern names the value's own enum. \
                     There is no structural matching between two enums that happen to have a \
                     variant of the same name",
                ),
            );
            return Pat::Wild;
        }
        let Some(at) = self.find_variant(id, &v.variant, v.variant_span) else {
            return Pat::Wild;
        };
        self.res.record_variant(
            v.variant_span,
            VariantRef {
                enumeration: id,
                variant: at,
            },
        );
        let arity = self.variant_def(id, at).map_or(0, |d| d.fields.len());
        let mut args = vec![Pat::Wild; arity];
        let mut named: Vec<(String, Span)> = Vec::new();
        for f in &v.fields {
            let declared = self
                .variant_def(id, at)
                .and_then(|d| d.field(&f.name).map(|(i, fd)| (i, fd.ty)));
            let Some((index, fty)) = declared else {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        format!("`{owner}` has no payload field `{}`", f.name),
                    )
                    .secondary(
                        self.variant_span(id, at).unwrap_or(v.variant_span),
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
            // `_` names nothing. The field is still resolved above, because it still exists
            // and is still released with the value — `docs/spec.md` is explicit that a
            // discard declines to name a payload rather than forgetting it.
            let sub = self.check_pattern(&f.pattern, fty, scrutinee, binds, aliases);
            if let Some(slot) = args.get_mut(index as usize) {
                *slot = sub;
            }
        }
        let missing = self.missing_payload_fields(id, at, &named);
        if !missing.is_empty() {
            let owner = self.variant_path(id, at);
            let list = quoted(&missing);
            self.errors.push(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    v.variant_span,
                    format!("this pattern does not name {list} of `{owner}`"),
                )
                .help(
                    "a pattern names every payload field, matching it or discarding it with \
                     `_`. There is no rest pattern, so adding a payload field breaks the \
                     patterns that did not know about it rather than ignoring it",
                ),
            );
        }
        Pat::Ctor {
            enumeration: id,
            variant: at,
            args,
        }
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

        // `?` on an `Option` (R1, 5b.17), in a function returning an `Option`: `Some`'s value
        // where it is written, and `None` returned.
        if let Some(t) = self.option_propagation(ot, at) {
            return Completion::Value(t);
        }
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
                    "`?` propagates the core prelude's `Result` and `Option` and nothing \
                     else — not an enum with the same variants. Take the value apart with \
                     `match` instead",
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

    /// `operand?` where the operand is an `Option[T]` (R1, 5b.17): `T`, and `None` leaving a
    /// function that returns an `Option[U]`. `None` where the operand is no `Option`; a function
    /// not returning one is `N0362`'s, as a `Result`'s would be.
    fn option_propagation(&mut self, ot: Type, at: Span) -> Option<Type> {
        let def = self.option?;
        if self.res.types().definition_of(ot) != Some(TypeNode::Enum(def)) {
            return None;
        }
        let Type::Enum(operand) = ot else { return None };
        let inner = *self.res.types().type_args(ot).first()?;
        let into = self
            .return_type
            .filter(|t| self.res.types().definition_of(*t) == Some(TypeNode::Enum(def)));
        let Some(Type::Enum(into)) = into else {
            let shown = self
                .return_type
                .map_or_else(|| "nothing".to_owned(), |t| self.show(t));
            let mut d = Diagnostic::error(
                codes::PROPAGATE_OUTSIDE_RESULT,
                at,
                format!(
                    "`?` on an `Option` can only be used in a function that returns an `Option`, \
                     and this one returns `{shown}`"
                ),
            )
            .help("`?` leaves the function with `None`. Here, `match` on the value instead");
            if let Some(span) = self.return_span {
                d = d.secondary(span, format!("declared `{shown}` here"));
            }
            self.errors.push(d);
            return Some(inner);
        };
        let variant = |c: &Self, id: EnumId, name: &str| {
            c.res
                .enums()
                .get(id)
                .and_then(|d| d.variant(name))
                .map(|(i, _)| i)
        };
        if let (Some(some), Some(none), Some(into_none)) = (
            variant(self, operand, "Some"),
            variant(self, operand, "None"),
            variant(self, into, "None"),
        ) {
            self.res.record_propagation(
                at,
                Propagation {
                    operand,
                    ok: some,
                    err: none,
                    into,
                    into_err: into_none,
                },
            );
        }
        Some(inner)
    }

    /// One arm: its payload bindings, then its body, in a scope of their own.
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

/// Whether a pattern names a variant anywhere at its top: what needs an enum scrutinee.
fn has_variant_pattern(p: &nazm_syntax::Pattern) -> bool {
    match p {
        nazm_syntax::Pattern::Variant(_) => true,
        nazm_syntax::Pattern::Or { alts, .. } => alts.iter().any(has_variant_pattern),
        _ => false,
    }
}

/// The checker's answer to what a type's constructors are, for [`usefulness`].
struct Sigs<'c, 'a>(&'c Checker<'a>);

impl usefulness::Signatures for Sigs<'_, '_> {
    fn variants(&self, t: Type) -> Option<(EnumId, Vec<Vec<Type>>)> {
        let Type::Enum(id) = t else { return None };
        let d = self.0.res.enums().get(id)?;
        Some((
            id,
            d.variants
                .iter()
                .map(|v| v.fields.iter().map(|f| f.ty).collect())
                .collect(),
        ))
    }

    fn tuple(&self, t: Type) -> Option<Vec<Type>> {
        self.0.res.types().tuple_elements(t)
    }

    fn spell(&self, enumeration: EnumId, variant: u32) -> (String, Vec<String>) {
        let name = self.0.variant_path(enumeration, variant);
        let fields = self
            .0
            .variant_def(enumeration, variant)
            .map(|v| v.fields.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_default();
        (name, fields)
    }
}
