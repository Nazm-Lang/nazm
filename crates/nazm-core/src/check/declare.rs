//! Declaring a module's types: names, type parameters and their requirements, and fields.

use super::layout::{foreign_struct, resolve_fields};
use super::written::{record_type_scope, type_environment, Collisions};
use super::*;

/// Stage 0a for the whole compilation: declare every user-defined type's **name**.
///
/// Names before field types, for every module, because a field type may name a type
/// declared later in the file or in another module — `docs/user/spec.md` makes declaration
/// order irrelevant, and that is only true if every name exists before any type is
/// resolved. It is N2's declare-then-check, one stage earlier, and the returned interfaces
/// are the *type* half of what a dependent needs.
///
/// Records and enums are declared in one pass over one namespace. A module that declares
/// `struct Item` and `enum Item` is refused here rather than producing two types nothing
/// could tell apart.
///
/// The interfaces returned carry types and imports and no exports: at this point no type
/// has fields, so there is nothing to close over and no signature to publish.
/// [`declare_unit`] returns the whole thing once both are known.
pub fn declare_types(
    units: &[Unit<'_>],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Vec<UnitInterface> {
    let mut interfaces = Vec::with_capacity(units.len());
    for unit in units {
        let mut records = Vec::new();
        let mut enums = Vec::new();
        for (decl, r) in &unit.records {
            if refuse_type_name(unit.module, &r.name, r.name_span, res, errors) {
                continue;
            }
            let visibility = Visibility::from_keyword(r.pub_span);
            let params = declared_type_params(&r.type_params, errors);
            let id = res.records_mut().declare(RecordDef {
                name: r.name.clone(),
                module: unit.module,
                visibility,
                name_span: r.name_span,
                fields: Vec::new(),
                layout: Vec::new(),
                decl: *decl,
                params: params.clone(),
                instance: None,
                repr: match &r.foreign {
                    None => CRepr::Nazm,
                    Some(f) if f.opaque => CRepr::Opaque,
                    Some(_) => CRepr::Layout,
                },
            });
            res.types_mut().name_params(ParamOwner::Record(id), params);
            declared_requirements(
                &r.type_params,
                ParamOwner::Record(id),
                &HashMap::new(),
                res,
                errors,
            );
            if visibility.is_exported() {
                records.push(RecordExport {
                    def: id,
                    nameable: true,
                });
            }
        }
        for (decl, e) in &unit.enums {
            if refuse_type_name(unit.module, &e.name, e.name_span, res, errors) {
                continue;
            }
            let visibility = Visibility::from_keyword(e.pub_span);
            let params = declared_type_params(&e.type_params, errors);
            let id = res.enums_mut().declare(EnumDef {
                name: e.name.clone(),
                module: unit.module,
                visibility,
                name_span: e.name_span,
                variants: Vec::new(),
                order: Vec::new(),
                decl: *decl,
                params: params.clone(),
                instance: None,
            });
            res.types_mut().name_params(ParamOwner::Enum(id), params);
            declared_requirements(
                &e.type_params,
                ParamOwner::Enum(id),
                &HashMap::new(),
                res,
                errors,
            );
            if visibility.is_exported() {
                enums.push(EnumExport {
                    def: id,
                    nameable: true,
                });
            }
        }
        // Traits share the type namespace (N78); their methods' signatures are resolved
        // once every type is, by `declare_trait_methods`.
        for t in &unit.traits {
            if refuse_type_name(unit.module, &t.name, t.name_span, res, errors) {
                continue;
            }
            res.traits_mut().declare(TraitDef {
                name: t.name.clone(),
                module: unit.module,
                visibility: Visibility::from_keyword(t.pub_span),
                name_span: t.name_span,
                methods: Vec::new(),
            });
        }
        interfaces.push(UnitInterface::new(
            unit.module,
            Vec::new(),
            records,
            enums,
            unit.imports.to_vec(),
        ));
    }
    interfaces
}

/// Whether this module may not declare a type of this name, having said why.
///
/// One function for both kinds, because the namespace is one: a built-in's name is nobody's
/// to take, and a second declaration under a name this module already has is a duplicate
/// whether or not the two are the same kind.
fn refuse_type_name(
    module: ModuleId,
    name: &str,
    at: Span,
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) -> bool {
    // A built-in's name is not a module's to define. Unlike a duplicate, there is no
    // declaration to point at, so the reason is said rather than shown. `Vec` is one: a
    // type written `Vec` is always the built-in, so a record declared under that name
    // could be built and never named.
    if Type::from_name(name).is_some() || name == VEC {
        errors.push(
            Diagnostic::error(
                codes::BUILTIN_TYPE_REDEFINED,
                at,
                format!("`{name}` is a built-in type and cannot be redefined"),
            )
            .help(
                "built-in type names mean the same thing in every module of every \
                 program, so none of them may be shadowed; choose another name",
            ),
        );
        return true;
    }
    let first = res
        .records()
        .declared(module, name)
        .and_then(|id| res.records().get(id))
        .map(|d| d.name_span)
        .or_else(|| {
            res.enums()
                .declared(module, name)
                .and_then(|id| res.enums().get(id))
                .map(|d| d.name_span)
        })
        .or_else(|| {
            res.traits()
                .iter()
                .find(|(_, t)| t.module == module && t.name == name)
                .map(|(_, t)| t.name_span)
        });
    if let Some(first) = first {
        errors.push(
            Diagnostic::error(
                codes::DUPLICATE_TYPE,
                at,
                format!("`{name}` is declared more than once in this module"),
            )
            .secondary(first, "first declared here")
            .help(
                "records and enums share one type namespace, so a `struct` and an `enum` \
                 of the same name collide exactly as two of either would",
            ),
        );
        return true;
    }
    false
}

/// A declaration's type parameters, checked once: unique within the list (`N0350`), and
/// never a built-in type's name or `Vec` (`N0331`). Returns the names in order, for
/// messages — a parameter's identity is its position, not what it is called.
pub(super) fn declared_type_params(
    params: &[WrittenParam],
    errors: &mut Vec<Diagnostic>,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (i, p) in params.iter().enumerate() {
        if Type::from_name(&p.name).is_some() || p.name == VEC {
            errors.push(
                Diagnostic::error(
                    codes::BUILTIN_TYPE_REDEFINED,
                    p.span,
                    format!(
                        "`{}` is a built-in type and cannot be a type parameter",
                        p.name
                    ),
                )
                .help(
                    "a built-in type's name means the same thing everywhere, including \
                     inside a generic definition; choose another name, such as `T`",
                ),
            );
        } else if let Some(first) = params.iter().take(i).find(|q| q.name == p.name) {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_TYPE_PARAMETER,
                    p.span,
                    format!("`{}` is declared twice in this parameter list", p.name),
                )
                .secondary(first.span, "first declared here")
                .help("a type parameter is one position; two cannot share a name"),
            );
        }
        names.push(p.name.clone());
    }
    names
}

/// The one capability a type parameter may require. N14.
///
/// Not a type and not a declaration: a name that means something only after the `:` in a
/// type parameter list, so no record, enum or import can shadow it and it shadows nothing.
pub const EQUALITY: &str = "Equality";

/// What a declaration's type parameters require, recorded against each parameter's
/// identity — its owner and position. N14.
///
/// Only a function's parameters may carry a requirement. One on a record's or an enum's is
/// refused (`N0101`) rather than parsed and ignored: every instance of such a type would
/// have to be checked wherever it is written, and nothing needs that yet. A name that is
/// not a capability is `N0365`.
pub(super) fn declared_requirements(
    params: &[WrittenParam],
    owner: ParamOwner,
    traits: &HashMap<String, Option<TraitId>>,
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    for (i, p) in params.iter().enumerate() {
        let Some(req) = &p.requires else {
            continue;
        };
        if !matches!(owner, ParamOwner::Function(_)) {
            errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    req.span,
                    "a requirement on a record's or an enum's type parameter is not supported yet",
                )
                .help(
                    "require the capability where it is used: a function can say \
                     `fn same[T: Equality](a: Box[T], b: Box[T]) -> Bool`",
                ),
            );
            continue;
        }
        // A trait (N78): the parameter's methods are the trait's.
        if req.name != EQUALITY {
            if let Some(Some(t)) = traits.get(&req.name) {
                res.traits_mut().record_mention(req.span, *t);
                res.traits_mut().bound(
                    TypeParam {
                        owner,
                        index: u32::try_from(i).unwrap_or(u32::MAX),
                    },
                    *t,
                );
                continue;
            }
            errors.push(
                Diagnostic::error(
                    codes::UNKNOWN_REQUIREMENT,
                    req.span,
                    format!(
                        "`{}` is not a capability a type parameter can require",
                        req.name
                    ),
                )
                .help(
                    "a type parameter requires `Equality` or a trait in scope: \
                     `fn same[T: Equality](a: T, b: T) -> Bool`, `fn f[T: Show](x: T) -> Str`",
                ),
            );
            continue;
        }
        res.types_mut().require_equality(TypeParam {
            owner,
            index: u32::try_from(i).unwrap_or(u32::MAX),
        });
    }
}

/// The written name of the one generic built-in type.
///
/// Not in [`Type::from_name`], which answers for a complete type, and `Vec` alone is not
/// one: it is always applied to its element.
pub(super) const VEC: &str = "Vec";

/// A declaration's type parameters as the types they are inside it, by name.
/// Record each of `params` as the declaration of its parameter of `owner` (N23), at the
/// name its `[…]` list writes — the identity [`param_scope`] gives the same position.
pub(super) fn declare_type_params(
    res: &mut Resolution,
    params: &[WrittenParam],
    owner: ParamOwner,
) {
    for (i, p) in params.iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        res.record_written_type(
            p.span,
            WrittenType::ParamDeclaration(TypeParam { owner, index }),
        );
    }
}

pub(super) fn param_scope(params: &[WrittenParam], owner: ParamOwner) -> Vec<(String, Type)> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                p.name.clone(),
                Type::Param(TypeParam {
                    owner,
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                }),
            )
        })
        .collect()
}

/// Stage 0b for one module: resolve its types' field types.
///
/// `deps` are the type-carrying interfaces of the modules this one directly imports —
/// what [`declare_types`] returned, selected by the caller. Their *bodies* are not a
/// parameter and never will be, which is the same claim [`check_unit`] makes one stage
/// later.
pub fn resolve_type_fields(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let types = type_environment(unit, deps, res, errors, Collisions::Report);
    // Only the first declaration of a name gets fields: a duplicate was already reported
    // and must not overwrite the shape the winner has.
    let mut completed: Vec<&str> = Vec::new();
    for (_, r) in &unit.records {
        let Some(id) = res.records().declared(unit.module, &r.name) else {
            continue;
        };
        if completed.contains(&r.name.as_str()) {
            continue;
        }
        completed.push(&r.name);
        declare_type_params(res, &r.type_params, ParamOwner::Record(id));
        let scope = param_scope(&r.type_params, ParamOwner::Record(id));
        record_type_scope(res, r.span, &scope, &types);
        let mut fields = resolve_fields(&types, res, &r.name, &r.fields, &scope, errors);
        // A record C sees (N85, §7.86): the C ABI, not generic, and — with fields — only
        // fields C has a layout for. An opaque handle's one field is a pointer no program
        // can name.
        if let Some(f) = &r.foreign {
            foreign_struct(r, f, &fields, res, errors);
            if f.opaque {
                fields = vec![FieldDef {
                    name: OPAQUE_FIELD.to_owned(),
                    ty: Type::Int,
                    name_span: r.name_span,
                    ty_span: r.name_span,
                }];
            }
        }
        // A record with no fields. Refused rather than allowed, and the reason is the
        // grammar: `Marker()` is exactly what a call to a function named `Marker` looks
        // like, and types and functions are separate namespaces, so the two forms would
        // be genuinely ambiguous. `docs/user/spec.md` records the decision and what would make
        // it reversible.
        if fields.is_empty() && r.fields.is_empty() {
            errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    r.name_span,
                    format!("`{}` has no fields, which is not supported yet", r.name),
                )
                .help(
                    "a record with no fields would be built by writing `Name()`, which is \
                     exactly how a call to a function of that name is written — and types \
                     and functions are separate namespaces, so neither reading could be \
                     preferred. Give it a field, or use `Bool`",
                ),
            );
        }
        res.records_mut().complete(id, fields);
    }

    let mut done: Vec<&str> = Vec::new();
    for (_, e) in &unit.enums {
        let Some(id) = res.enums().declared(unit.module, &e.name) else {
            continue;
        };
        if done.contains(&e.name.as_str()) {
            continue;
        }
        done.push(&e.name);
        // An enum with no variants is a type with no values, which raises `Never`
        // semantics the language has not settled. Refused deliberately rather than
        // allowed as a side effect. `docs/user/spec.md` says what a future milestone would
        // have to decide.
        if e.variants.is_empty() {
            errors.push(
                Diagnostic::error(
                    codes::EMPTY_ENUM,
                    e.name_span,
                    format!("`{}` has no variants, which is not supported yet", e.name),
                )
                .help(
                    "a type with no values decides what a function returning one means \
                     and whether a `match` on one is exhaustive, and neither has been \
                     settled. Give it a variant",
                ),
            );
        }
        let mut variants: Vec<VariantDef> = Vec::new();
        declare_type_params(res, &e.type_params, ParamOwner::Enum(id));
        let scope = param_scope(&e.type_params, ParamOwner::Enum(id));
        record_type_scope(res, e.span, &scope, &types);
        for v in &e.variants {
            if let Some(first) = variants.iter().find(|p| p.name == v.name) {
                errors.push(
                    Diagnostic::error(
                        codes::DUPLICATE_VARIANT,
                        v.name_span,
                        format!("`{}` has two variants called `{}`", e.name, v.name),
                    )
                    .secondary(first.name_span, "first declared here")
                    .help("a variant is identified by its name, so two cannot share one"),
                );
                continue;
            }
            let fields = resolve_fields(&types, res, &v.name, &v.fields, &scope, errors);
            variants.push(VariantDef {
                name: v.name.clone(),
                name_span: v.name_span,
                fields,
                layout: Vec::new(),
            });
        }
        res.enums_mut().complete(id, variants);
    }
}
