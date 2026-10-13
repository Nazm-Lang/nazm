//! Construction: building a record or an enum variant.

use super::generic::quoted;
use super::layout::opaque_refused;
use super::*;

impl Checker<'_> {
    /// Check a record construction and return the record it builds.
    ///
    /// Initialisers are evaluated **in the order written**, which `docs/spec.md` fixes
    /// and which this order of checking mirrors: a diagnostic about the second field
    /// comes after one about the first, and so does the side effect at runtime.
    pub(super) fn check_construction(
        &mut self,
        ty: &str,
        ty_span: Span,
        type_args: Option<&[TypeName]>,
        fields: &[FieldInit],
        span: Span,
    ) -> Option<Type> {
        let applied = self
            .resolve_record(ty, ty_span)
            .and_then(|def| self.applied(Type::Record(def), type_args, ty_span));
        let Some(Type::Record(id)) = applied else {
            // Still check the initialisers: one unknown type should not silence every
            // error inside it.
            for f in fields {
                self.check_expr(&f.value, None);
            }
            return None;
        };
        self.res.record_construction(ty_span, id);
        // Only C makes a handle (N85): it has no field a program could give a value.
        if self
            .res
            .records()
            .get(id)
            .is_some_and(|d| d.repr == CRepr::Opaque)
        {
            for f in fields {
                self.check_expr(&f.value, None);
            }
            self.errors.push(opaque_refused(ty_span, ty, "built"));
            return Some(Type::Record(id));
        }

        let mut given: Vec<(String, Span)> = Vec::new();
        for f in fields {
            let declared = self
                .res
                .records()
                .get(id)
                .and_then(|d| d.field(&f.name).map(|(i, fd)| (i, fd.ty, fd.name_span)));
            let Some((index, want, declared_at)) = declared else {
                let name = self.res.types().show(Type::Record(id)).to_owned();
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        format!("`{name}` has no field `{}`", f.name),
                    )
                    .secondary(
                        self.res.records().get(id).map_or(ty_span, |d| d.name_span),
                        "declared here",
                    ),
                );
                self.check_expr(&f.value, None);
                continue;
            };
            if let Some((_, first)) = given.iter().find(|(g, _)| *g == f.name) {
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        f.name_span,
                        format!("`{}` is given a value twice", f.name),
                    )
                    .secondary(*first, "first given here")
                    .help("every field is given exactly one value; there is no update form"),
                );
                self.check_expr(&f.value, None);
                continue;
            }
            given.push((f.name.clone(), f.name_span));
            // Which field this initialiser gives a value to — decided just above, and
            // recorded (N17) so that a field's uses include the constructions that set it,
            // not only the projections that read it.
            self.res.record_field(
                f.name_span,
                FieldRef {
                    record: id,
                    field: index,
                },
            );
            let got = self.value_type(&f.value, Some(want));
            if got != want {
                let (swant, sgot) = (self.show(want), self.show(got));
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        f.value.span(),
                        format!("`{}` is `{swant}`, but this is `{sgot}`", f.name),
                    )
                    .secondary(declared_at, format!("declared `{swant}` here")),
                );
            }
        }

        let missing: Vec<String> = self.res.records().get(id).map_or_else(Vec::new, |d| {
            d.fields
                .iter()
                .filter(|f| !given.iter().any(|(g, _)| *g == f.name))
                .map(|f| f.name.clone())
                .collect()
        });
        if !missing.is_empty() {
            let name = self.res.types().show(Type::Record(id)).to_owned();
            let list = missing
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ");
            self.errors.push(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    span,
                    format!("this `{name}` does not give a value for {list}"),
                )
                .secondary(
                    self.res.records().get(id).map_or(ty_span, |d| d.name_span),
                    "declared here",
                )
                .help(
                    "every field is given a value when a record is built. There are no \
                     default values and no partial records",
                ),
            );
        }
        Some(Type::Record(id))
    }

    /// Check `Enum.Variant(field: value, …)` and return the enum it builds.
    ///
    /// The same law as a record construction, applied to the fields of one variant: every
    /// payload field exactly once, by name, with no unknown field and no duplicate, and
    /// initialisers checked **in the order written** because that is the order they run in.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_variant(
        &mut self,
        ty: &str,
        ty_span: Span,
        type_args: Option<&[TypeName]>,
        variant: &str,
        variant_span: Span,
        fields: &[FieldInit],
        span: Span,
        expected: Option<Type>,
    ) -> Option<Type> {
        // A payload value checked while inferring the enum's arguments, so it is checked once.
        let mut pre: Vec<Option<Type>> = vec![None; fields.len()];
        let applied = if self.refused(ty) {
            None
        } else {
            self.resolve_enum(ty, ty_span).and_then(|def| {
                let inferred = if type_args.is_none() {
                    self.inferred_enum(def, variant, fields, expected, &mut pre)
                } else {
                    None
                };
                inferred.or_else(|| self.applied(Type::Enum(def), type_args, ty_span))
            })
        };
        let Some(Type::Enum(id)) = applied else {
            // Still check the initialisers: one unknown type should not silence every
            // error inside it.
            for (f, done) in fields.iter().zip(&pre) {
                if done.is_none() {
                    self.check_expr(&f.value, None);
                }
            }
            return None;
        };
        let Some(at) = self.find_variant(id, variant, variant_span) else {
            for (f, done) in fields.iter().zip(&pre) {
                if done.is_none() {
                    self.check_expr(&f.value, None);
                }
            }
            return Some(Type::Enum(id));
        };
        self.res.record_variant(
            variant_span,
            VariantRef {
                enumeration: id,
                variant: at,
            },
        );

        let mut given: Vec<(String, Span)> = Vec::new();
        for (f, done) in fields.iter().zip(pre) {
            let declared = self
                .variant_def(id, at)
                .and_then(|v| v.field(&f.name).map(|(i, fd)| (i, fd.ty, fd.name_span)));
            let Some((index, want, declared_at)) = declared else {
                let owner = self.variant_path(id, at);
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        f.name_span,
                        format!("`{owner}` has no payload field `{}`", f.name),
                    )
                    .secondary(
                        self.variant_span(id, at).unwrap_or(ty_span),
                        "declared here",
                    ),
                );
                if done.is_none() {
                    self.check_expr(&f.value, None);
                }
                continue;
            };
            if let Some((_, first)) = given.iter().find(|(g, _)| *g == f.name) {
                self.errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        f.name_span,
                        format!("`{}` is given a value twice", f.name),
                    )
                    .secondary(*first, "first given here")
                    .help("every payload field is given exactly one value"),
                );
                if done.is_none() {
                    self.check_expr(&f.value, None);
                }
                continue;
            }
            given.push((f.name.clone(), f.name_span));
            self.res.record_payload(
                f.name_span,
                PayloadRef {
                    enumeration: id,
                    variant: at,
                    field: index,
                },
            );
            let got = match done {
                Some(t) => t,
                None => self.value_type(&f.value, Some(want)),
            };
            if got != want {
                let (swant, sgot) = (self.show(want), self.show(got));
                self.errors.push(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        f.value.span(),
                        format!("`{}` is `{swant}`, but this is `{sgot}`", f.name),
                    )
                    .secondary(declared_at, format!("declared `{swant}` here")),
                );
            }
        }

        let missing = self.missing_payload_fields(id, at, &given);
        if !missing.is_empty() {
            let owner = self.variant_path(id, at);
            let list = quoted(&missing);
            self.errors.push(
                Diagnostic::error(
                    codes::MISSING_FIELD,
                    span,
                    format!("this `{owner}` does not give a value for {list}"),
                )
                .secondary(
                    self.variant_span(id, at).unwrap_or(ty_span),
                    "declared here",
                )
                .help(
                    "every payload field is given a value when a variant is built. There \
                     are no default values",
                ),
            );
        }
        Some(Type::Enum(id))
    }
}

impl Checker<'_> {
    /// A generic enum's arguments where none are written (R1, 5b.14): the type the
    /// construction is wanted as, when it is this enum; otherwise each payload field declared
    /// as one of its parameters says that parameter is its value's type. `None` where that
    /// leaves one undecided — then `N0352`, as before. A payload value checked here is recorded
    /// in `pre` and not checked again.
    fn inferred_enum(
        &mut self,
        def: EnumId,
        variant: &str,
        fields: &[FieldInit],
        expected: Option<Type>,
        pre: &mut [Option<Type>],
    ) -> Option<Type> {
        let node = nazm_sema::TypeNode::of(Type::Enum(def))?;
        let n = self.res.types().declared_params(node);
        if n == 0 {
            return None;
        }
        if let Some(e) = expected {
            if self.res.types().definition_of(e) == Some(node) {
                return Some(e);
            }
        }
        let at = self
            .res
            .enums()
            .get(def)?
            .variants
            .iter()
            .position(|v| v.name == variant)?;
        let at = u32::try_from(at).ok()?;
        let mut args: Vec<Option<Type>> = vec![None; n];
        for (f, slot) in fields.iter().zip(pre.iter_mut()) {
            let declared = self
                .variant_def(def, at)
                .and_then(|v| v.field(&f.name).map(|(_, fd)| fd.ty));
            if let Some(Type::Param(p)) = declared {
                if p.owner == nazm_sema::ParamOwner::Enum(def) {
                    let t = self.value_type(&f.value, None);
                    *slot = Some(t);
                    if let Some(a) = args.get_mut(p.index as usize) {
                        a.get_or_insert(t);
                    }
                }
            }
        }
        let args: Vec<Type> = args.into_iter().collect::<Option<_>>()?;
        let t = super::written::apply(self.res, node, args);
        self.mention(t);
        Some(t)
    }
}
