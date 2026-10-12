//! A compilation unit: declaring its interface, checking it, and the environment its module sees.

use super::clauses::{preconditions_of, CONVERSIONS};
use super::declare::{
    declare_type_params, declared_requirements, declared_type_params, param_scope,
};
use super::foreign::{check_export, check_foreign, declared_effect_param};
use super::generic::polymorphic;
use super::reexport::apply_reexports;
use super::traits::{
    impl_method_name, impl_scope, implementable, named_trait, register_impl, trait_scope,
};
use super::written::{record_module_names, resolve_written, type_environment, Collisions};
use super::*;

/// Every module's interface, as a compilation declares them: types, their fields and layouts,
/// traits' methods, each module's definitions, then what `pub use` re-exports (N103). For a
/// tool that publishes an interface without checking bodies — the same stages, in the same
/// order, as [`check`] runs before any body.
pub fn declared_interfaces(
    units: &[Unit<'_>],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Vec<UnitInterface> {
    res.types_mut().defer();
    let mut types = declare_types(units, res, errors);
    apply_reexports(units, &mut types, res, &mut Vec::new(), false);
    for u in units {
        let deps: Vec<&UnitInterface> = u
            .imports
            .iter()
            .filter_map(|d| types.get(d.on.index()))
            .collect();
        resolve_type_fields(u, &deps, res, errors);
    }
    res.types_mut().settle();
    validate_type_layouts(res, errors);
    declare_trait_methods(units, &types, res, errors);
    let mut interfaces: Vec<UnitInterface> = units
        .iter()
        .map(|u| {
            let deps: Vec<&UnitInterface> = u
                .imports
                .iter()
                .filter_map(|d| types.get(d.on.index()))
                .collect();
            declare_unit(u, &deps, res, errors)
        })
        .collect();
    apply_reexports(units, &mut interfaces, res, errors, true);
    declare_all_consts(units, &mut interfaces, res, errors);
    validate_new_arrays(res, errors);
    interfaces
}

/// Stage 1 for one module: declare what it defines, and say what it offers.
///
/// Reads this module's declarations and nothing else — no other module's source, no other
/// module's interface. That is what makes the stage orderable in any order, including one
/// that contains a cycle.
pub fn declare_unit(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> UnitInterface {
    let mut types = type_environment(unit, deps, res, errors, Collisions::Silent);
    // Its aggregate constants (L8), as far as they can be computed before the imports' values
    // are known: `declare_consts` computes and reports them, in dependency order.
    super::written::compute_aggregates(unit, deps, &mut types, res, errors, false);
    let mut exports = Vec::new();

    // Each impl's header, resolved once (N78): its trait and its type, or `None` when either
    // did not resolve — reported here, and its methods then declared as nothing at all.
    let trait_names = trait_scope(unit, res);
    let mut headers: Vec<(Span, Option<(TraitId, Type)>)> = Vec::new();
    for imp in &unit.impls {
        let trait_id = match named_trait(&imp.trait_name, &trait_names, res, unit) {
            Ok(t) => {
                res.traits_mut().record_mention(imp.trait_name.name_span, t);
                Some(t)
            }
            Err(d) => {
                errors.push(d);
                None
            }
        };
        // An impl of a generic type (R1b, L11b): filed under the generic definition.
        if !imp.type_params.is_empty() {
            let ty = match super::traits::generic_impl_type(&types.names, res, imp) {
                Ok(ty) => Some(ty),
                Err(why) => {
                    errors.push(
                        Diagnostic::error(codes::IMPL_TYPE_REFUSED, imp.self_ty.span, why).help(
                            "an impl with type parameters is written \
                             `impl[T: Show] Show for Pair[T]`, for a generic record or enum",
                        ),
                    );
                    None
                }
            };
            headers.push((imp.impl_span, trait_id.zip(ty)));
            continue;
        }
        let before = errors.len();
        let ty = resolve_written(&types, res, &imp.self_ty, &[], None, errors);
        let ty =
            (errors.len() == before)
                .then_some(ty)
                .and_then(|ty| match implementable(res, ty) {
                    Ok(_) => Some(ty),
                    Err(why) => {
                        errors.push(
                            Diagnostic::error(
                                codes::IMPL_TYPE_REFUSED,
                                imp.self_ty.span,
                                format!("`{}` cannot have an impl: it is {why}", imp.self_ty.name),
                            )
                            .help(
                                "an impl is for a non-generic record or enum, or for `Int`, \
                             `Bool` or `Str`",
                            ),
                        );
                        None
                    }
                });
        headers.push((imp.impl_span, trait_id.zip(ty)));
    }

    // Each foreign symbol's first declaration in this module, for `N0383`.
    let mut foreign_seen: std::collections::HashMap<String, (Vec<Type>, Type, Span)> =
        std::collections::HashMap::new();
    for (decl, f) in &unit.functions {
        // An impl's method (N78): declared under `Trait/Type/m`, with `Self` its impl's type.
        let method = match &f.method_of {
            Some(m) => match headers.iter().find(|(at, _)| *at == m.impl_span) {
                Some((_, Some(h))) => Some(*h),
                // Its impl's header did not resolve, which is reported: nothing to declare.
                _ => continue,
            },
            None => None,
        };
        let name = method.map_or_else(
            || f.name.clone(),
            |(t, ty)| {
                let trait_name = res.traits().get(t).map_or("?", |d| d.name.as_str());
                impl_method_name(trait_name, &res.types().show(ty), &f.name)
            },
        );
        // A generic function's parameter types mention its own parameters, and a parameter
        // names its owner — so the id comes first. Nothing else declares a function between
        // here and the `declare` below, so the id this one would get is known now.
        let id = res.next_function();
        let generics = declared_type_params(&f.type_params, errors);
        let mut scope = param_scope(&f.type_params, ParamOwner::Function(id));
        if let Some((_, ty)) = method {
            // An impl of a generic type (L11b): `Self` is its definition applied to this
            // method's own parameters, the impl's, which the parser wrote first.
            let own = |n: usize| -> Vec<Type> {
                (0..n)
                    .map(|i| {
                        Type::Param(nazm_sema::TypeParam {
                            owner: ParamOwner::Function(id),
                            index: u32::try_from(i).unwrap_or(u32::MAX),
                        })
                    })
                    .collect()
            };
            let me = match ty {
                Type::Record(r) => match res.records().get(r).map(|d| d.params.len()) {
                    Some(n) if n > 0 => res.types_mut().apply_record(r, own(n)),
                    _ => ty,
                },
                Type::Enum(e) => match res.enums().get(e).map(|d| d.params.len()) {
                    Some(n) if n > 0 => res.types_mut().apply_enum(e, own(n)),
                    _ => ty,
                },
                _ => ty,
            };
            scope.push(("Self".to_owned(), me));
        }
        let effect = declared_effect_param(f, errors).map(|n| (n, ParamOwner::Function(id)));
        let params: Vec<Type> = f
            .params
            .iter()
            .map(|p| resolve_written(&types, res, &p.ty, &scope, effect, errors))
            .collect();
        let ret = resolve_written(&types, res, &f.return_type, &scope, effect, errors);
        let mut declared_effects = f
            .effects
            .as_ref()
            .and_then(|d| crate::effects::declared(d, effect.map(|e| e.0), errors));
        // An impl method that writes no effect set has its trait method's (N78): a trait's
        // signature is a promise, and its impls keep it.
        if let (None, Some((t, _))) = (declared_effects, method) {
            declared_effects = res
                .traits()
                .get(t)
                .and_then(|d| d.method(&f.name).and_then(|i| d.methods.get(i as usize)))
                .map(|m| m.effects);
        }
        if let Some((name, owner)) = effect {
            res.types_mut().name_effect_param(owner, name.to_owned());
        }

        // A built-in has no source span to point at, so "first defined here" would aim
        // at byte zero. Said plainly instead. Built-ins are not a module's definitions,
        // so no visibility rule makes this one legal.
        // A prelude built-in (R1a) is hidden by a module's own function of its name instead.
        if method.is_none()
            && (Intrinsic::by_name(&f.name).is_some_and(|i| !i.is_prelude())
                || CONVERSIONS.contains(&f.name.as_str()))
        {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FUNCTION,
                    f.name_span,
                    format!("`{}` is a built-in and cannot be redefined", f.name),
                )
                .help(
                    "built-ins share the ordinary function namespace so that a call \
                     always means the same thing; choose another name",
                ),
            );
            // Nothing was declared, so the id this signature's parameters were resolved
            // under goes to the next function: what was recorded under it is not theirs.
            res.forget_type_params_of(ParamOwner::Function(id));
            continue;
        }

        // Within this module. Another module defining the same name is another module's
        // definition, and since 2026-09-22 that is not a collision.
        if let Some(existing) = res
            .definition(unit.module, &name)
            .and_then(|id| res.function(id))
        {
            errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FUNCTION,
                    f.name_span,
                    if method.is_some() {
                        format!("`{}` is defined more than once in this impl", f.name)
                    } else {
                        format!("`{}` is defined more than once in this module", f.name)
                    },
                )
                .secondary(existing.name_span, "first defined here"),
            );
            // Its signature is resolved again with its body, under the first definition,
            // which replaces what was recorded here under `id`.
            continue;
        }

        // A foreign declaration (N42): its shape, its ABI, its symbol and its types, each
        // refused by name. What it declares is a contract with no body — the effect
        // `foreign` and nothing else.
        let option = unit
            .prelude
            .and_then(|m| res.enums().declared(m, nazm_sema::prelude::OPTION));
        let foreign = f.foreign.as_ref().map(|d| {
            check_foreign(f, d, &params, ret, res, option, errors);
            match foreign_seen.get(d.symbol.as_str()) {
                Some((p, r, at)) if (p.as_slice(), *r) != (params.as_slice(), ret) => {
                    errors.push(
                        Diagnostic::error(
                            codes::FOREIGN_CONFLICT,
                            d.symbol_span,
                            format!(
                                "the C symbol `{}` is declared again with another signature",
                                d.symbol
                            ),
                        )
                        .secondary(*at, "declared differently here")
                        .help("a C function has one signature; declare it the same way"),
                    );
                }
                Some(_) => {}
                None => {
                    foreign_seen.insert(d.symbol.clone(), (params.clone(), ret, d.symbol_span));
                }
            }
            d.symbol.clone()
        });
        // An export (N55): a Nazm function C may call, under the rules §7.57 states.
        let export = f.export.as_ref().map(|d| {
            check_export(f, d, &params, ret, errors);
            d.symbol.clone()
        });
        let declared_effects = if foreign.is_some() {
            Some(nazm_sema::EffectSet::of(nazm_sema::Effect::Foreign))
        } else {
            declared_effects
        };

        // A method is never in the function namespace: it is reached through its trait.
        let visibility = if method.is_some() {
            Visibility::Private
        } else {
            Visibility::from_keyword(f.pub_span)
        };
        let declared = res.declare(FnDef {
            name: name.clone(),
            module: unit.module,
            visibility,
            params: params.clone(),
            param_names: f.params.iter().map(|p| p.name.clone()).collect(),
            ret,
            name_span: f.name_span,
            decl: *decl,
            locals: Vec::new(),
            type_params: generics.clone(),
            call_sites: Vec::new(),
            declared_effects,
            inferred_effects: None,
            effect_reasons: Vec::new(),
            provenance: None,
            mentions: Vec::new(),
            foreign: foreign.clone(),
            export,
            preconditions: preconditions_of(f),
            closure: None,
        });
        debug_assert_eq!(declared, id);
        declare_type_params(res, &f.type_params, ParamOwner::Function(id));
        res.types_mut()
            .name_params(ParamOwner::Function(id), generics.clone());
        declared_requirements(
            &f.type_params,
            ParamOwner::Function(id),
            &trait_names,
            res,
            errors,
        );

        if visibility.is_exported() {
            // A public signature naming a private record would publish a type the
            // importer cannot resolve. Checked at each public item on its *direct*
            // references, which is the recursion `docs/spec.md` asks for expressed
            // distributively: an intermediate record must itself be public to appear
            // here, and its own declaration is checked the same way — so one mistake
            // produces exactly one diagnostic, at the place the private name is written.
            for (t, at) in params
                .iter()
                .zip(f.params.iter().map(|p| p.ty.span))
                .chain(std::iter::once((&ret, f.return_type.span)))
            {
                report_private_type(*t, at, f.name_span, "function", &f.name, res, errors);
            }
            let bounds = (0..u32::try_from(generics.len()).unwrap_or(u32::MAX))
                .filter_map(|index| {
                    let t = res.traits().bound_of(TypeParam {
                        owner: ParamOwner::Function(id),
                        index,
                    })?;
                    let d = res.traits().get(t)?;
                    Some((index, d.name.clone(), d.module))
                })
                .collect();
            exports.push(Export {
                bounds,
                name: f.name.clone(),
                params,
                ret,
                name_span: f.name_span,
                def: id,
                type_params: generics,
                effects: declared_effects,
                foreign: foreign.clone(),
                preconditions: preconditions_of(f),
                origin: None,
            });
        }
    }

    // The same check for an exported type's own fields, and the interface's type set.
    for (imp, (_, header)) in unit.impls.iter().zip(&headers) {
        let Some((trait_id, ty)) = *header else {
            continue;
        };
        register_impl(unit, imp, trait_id, ty, res, errors);
    }

    let mut records: Vec<RecordExport> = Vec::new();
    let mut enums: Vec<EnumExport> = Vec::new();
    let mut roots: Vec<Type> = Vec::new();
    for (_, r) in &unit.records {
        let Some(id) = res.records().declared(unit.module, &r.name) else {
            continue;
        };
        let Some(def) = res.records().get(id) else {
            continue;
        };
        if !def.visibility.is_exported() {
            continue;
        }
        let name = def.name.clone();
        let name_span = def.name_span;
        let fields: Vec<(Type, Span)> = def.fields.iter().map(|f| (f.ty, f.ty_span)).collect();
        for (t, at) in fields {
            report_private_type(t, at, name_span, "record", &name, res, errors);
        }
        records.push(RecordExport {
            def: id,
            nameable: true,
        });
        roots.push(Type::Record(id));
    }
    for (_, e) in &unit.enums {
        let Some(id) = res.enums().declared(unit.module, &e.name) else {
            continue;
        };
        let Some(def) = res.enums().get(id) else {
            continue;
        };
        if !def.visibility.is_exported() {
            continue;
        }
        let name = def.name.clone();
        let name_span = def.name_span;
        // Every payload field of every variant: a public enum's variants and their
        // payloads are all part of the public type, because `docs/spec.md` gives it no
        // per-variant visibility and an importer must be able to match every arm.
        let payloads: Vec<(Type, Span)> = def
            .variants
            .iter()
            .flat_map(|v| v.fields.iter().map(|f| (f.ty, f.ty_span)))
            .collect();
        for (t, at) in payloads {
            report_private_type(t, at, name_span, "enum", &name, res, errors);
        }
        enums.push(EnumExport {
            def: id,
            nameable: true,
        });
        roots.push(Type::Enum(id));
    }
    // Its exported constants' types (L8), among the roots the closure starts from, since an
    // importer builds the value where it uses it. The values are `declare_consts`'s.
    for c in unit
        .consts
        .iter()
        .filter(|c| c.pub_span.is_some() && !c.is_static)
    {
        let ty = resolve_written(&types, res, &c.ty, &[], None, &mut Vec::new());
        report_private_type(ty, c.ty.span, c.name_span, "constant", &c.name, res, errors);
        roots.push(ty);
    }

    // Closed over what the public surface mentions: an importer that projects through an
    // exported record or matches an exported enum needs every shape it can reach, and
    // cannot go and read the module that declared one. `RecordExport` says why this is not
    // transitive visibility.
    roots.extend(exports.iter().flat_map(|e| {
        e.params
            .iter()
            .copied()
            .chain(std::iter::once(e.ret))
            .collect::<Vec<_>>()
    }));
    for node in reachable_types(res.types(), roots) {
        match node {
            TypeNode::Record(id) => {
                if !records.iter().any(|r| r.def == id) {
                    records.push(RecordExport {
                        def: id,
                        nameable: false,
                    });
                }
            }
            TypeNode::Enum(id) => {
                if !enums.iter().any(|e| e.def == id) {
                    enums.push(EnumExport {
                        def: id,
                        nameable: false,
                    });
                }
            }
        }
    }

    // What the module offers of traits (N78): its exported traits, and every impl it declares
    // — impls are global to a compilation, so all of them are part of what it offers.
    let traits = res
        .traits()
        .iter()
        .filter(|(_, t)| t.module == unit.module && t.visibility.is_exported())
        .map(|(_, t)| nazm_sema::interface::TraitExport {
            name: t.name.clone(),
            module: t.module,
            methods: t
                .methods
                .iter()
                .map(|m| (m.name.clone(), m.params.clone(), m.ret, m.effects))
                .collect(),
        })
        .collect();
    let impls = res
        .traits()
        .impls()
        .filter(|(_, d)| d.module == unit.module)
        .filter_map(|(_, d)| {
            let t = res.traits().get(d.trait_id)?;
            Some(nazm_sema::interface::ImplExport {
                module: d.module,
                trait_name: t.name.clone(),
                trait_module: t.module,
                self_ty: d.self_ty,
                methods: t.methods.iter().map(|m| m.name.clone()).collect(),
            })
        })
        .collect();
    UnitInterface::new(unit.module, exports, records, enums, unit.imports.to_vec())
        .with_traits(traits, impls)
}

/// Stage 1b for one module (R1a, L8): its constants, now that `deps` — its imports' full
/// interfaces — carry theirs, every refusal reported; and the ones it exports, by name.
pub(super) fn declare_consts(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Vec<nazm_sema::ConstExport> {
    let mut types = type_environment(unit, deps, res, errors, Collisions::Silent);
    super::written::compute_aggregates(unit, deps, &mut types, res, errors, true);
    let mut consts: Vec<nazm_sema::ConstExport> = unit
        .consts
        .iter()
        .filter(|c| c.pub_span.is_some() && !c.is_static)
        .filter_map(|c| {
            let (ty, value) = types.consts.values.get(&c.name)?;
            Some(nazm_sema::ConstExport {
                name: c.name.clone(),
                ty: *ty,
                value: value.clone(),
                name_span: c.name_span,
            })
        })
        .collect();
    consts.sort_by(|a, b| a.name.cmp(&b.name));
    consts
}

/// Report a public item that names a private user-defined type.
///
/// `kind` and `name` describe the **public** item, so the message says what is leaking
/// and the labels say from where to where. Records and enums both, because the closure
/// `docs/spec.md` requires runs through both and a check that knew one kind would let a
/// private enum out through a public record's field.
fn report_private_type(
    t: Type,
    at: Span,
    item_span: Span,
    kind: &str,
    name: &str,
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    // Anywhere in the written type, since N11: `Box[Secret]` exposes `Secret` exactly as a
    // field of type `Secret` would. The first private definition found is reported, once —
    // one written type is one mistake.
    let mut work = vec![t];
    let mut found: Option<(String, Span)> = None;
    while let Some(t) = work.pop() {
        work.extend(res.types().type_args(t));
        // An array's element (L8): `[Secret; 2]` exposes `Secret`.
        if let Type::Array(a) = t {
            work.extend(res.types().array_info(a).map(|(e, _)| e));
        }
        let private = match res.types().definition_of(t) {
            Some(TypeNode::Record(id)) => res
                .records()
                .get(id)
                .filter(|d| !d.visibility.is_exported())
                .map(|d| (d.name.clone(), d.name_span)),
            Some(TypeNode::Enum(id)) => res
                .enums()
                .get(id)
                .filter(|d| !d.visibility.is_exported())
                .map(|d| (d.name.clone(), d.name_span)),
            None => None,
        };
        if private.is_some() {
            found = private;
            break;
        }
    }
    let Some((private_name, declared_at)) = found else {
        return;
    };
    errors.push(
        Diagnostic::error(
            codes::PRIVATE_TYPE_EXPOSED,
            at,
            format!("this public {kind} exposes `{private_name}`, which is private to this module"),
        )
        .secondary(item_span, format!("`{name}` is public"))
        .secondary(declared_at, "declared without `pub` here")
        .help(
            "an importer would have to name this type to use the item and cannot. Export \
             the type with `pub`, or stop exporting the item",
        ),
    );
}

/// Stage 2 for one module: check its bodies against its environment.
///
/// `deps` are the interfaces of the modules this one **directly** imports. Their bodies
/// are not a parameter, are not reachable from one, and are not read. That sentence is the
/// whole claim `docs/spec.md` makes about compilation units, and this signature is where
/// it is either true or not.
pub fn check_unit(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let env = environment(unit, deps, res, errors);
    let mut types = type_environment(unit, deps, res, errors, Collisions::Silent);
    super::written::compute_aggregates(unit, deps, &mut types, res, errors, false);
    record_module_names(res, unit.module, &env, &types);
    // Built from `deps` rather than from a graph, so the one path that checks a module
    // against persisted interfaces resolves type names by exactly the rule the
    // whole-program path uses. Two builders would be two rules.
    let imports = unit.imports.iter().map(|d| d.on).collect();
    let result = unit
        .prelude
        .and_then(|m| res.enums().declared(m, nazm_sema::prelude::RESULT));
    let option = unit
        .prelude
        .and_then(|m| res.enums().declared(m, nazm_sema::prelude::OPTION));
    let impl_modules = impl_scope(unit, res);
    let mut c = Checker {
        res,
        module: unit.module,
        env,
        types,
        imports,
        current: None,
        scopes: Scopes::default(),
        errors: Vec::new(),
        return_type: None,
        return_span: None,
        result,
        option,
        scope_depth: 0,
        loop_depth: 0,
        type_scope: Vec::new(),
        closures: Vec::new(),
        closure_count: 0,
        effect_param: None,
        trait_names: HashMap::new(),
        impl_modules,
        preconditions: HashMap::new(),
    };
    for (_, f) in &unit.functions {
        let requires: Vec<Expr> = f
            .contracts
            .iter()
            .filter(|k| k.kind == nazm_syntax::ContractKind::Requires)
            .map(|k| k.expr.clone())
            .collect();
        if let (false, Some(id)) = (requires.is_empty(), defined(c.res, unit.module, f)) {
            let names = f.params.iter().map(|p| p.name.clone()).collect();
            c.preconditions.insert(id, (names, requires));
        }
    }
    c.trait_names = trait_scope(unit, c.res);
    for (_, f) in &unit.functions {
        c.check_function(f);
    }
    // Effects are a fact about the module's resolved calls, so they are settled once every
    // body has been checked and every call resolved (N36).
    crate::effects::analyse(&unit.functions, unit.module, deps, c.res, &mut c.errors);
    errors.append(&mut c.errors);
}

/// The names one module may write.
///
/// Built-ins, then its own definitions, then the exports of each direct import — in that
/// order, and every conflict between the last two is reported rather than resolved.
/// The key a qualified name is held under in a module's environment (N49): `alias::name`.
/// `::` is in no identifier, so no unqualified name can be the same string.
pub(super) fn qualified_key(alias: &str, name: &str) -> String {
    format!("{alias}::{name}")
}

/// The key a name written with an optional qualifier is looked up by.
pub(super) fn written_key(qualifier: Option<&nazm_syntax::Qualifier>, name: &str) -> String {
    match qualifier {
        Some(q) => qualified_key(&q.name, name),
        None => name.to_owned(),
    }
}

/// `NAME::item` that resolves to nothing (N49): `NAME` is no alias, or its module does not
/// export `item`.
pub(super) fn unknown_qualified(key: &str, aliases: &[&str], at: Span, what: &str) -> Diagnostic {
    let (alias, item) = key.split_once("::").unwrap_or(("", key));
    if aliases.contains(&alias) {
        Diagnostic::error(
            codes::UNKNOWN_QUALIFIED_NAME,
            at,
            format!("the module imported as `{alias}` exports no {what} `{item}`"),
        )
        .help(format!(
            "`{alias}::` names what that module marks `pub`; check the name, or export it there"
        ))
    } else {
        Diagnostic::error(
            codes::UNKNOWN_QUALIFIED_NAME,
            at,
            format!("`{alias}` is not the name of an imported module"),
        )
        .help(format!(
            "import one under that name: `use \"path.nz\" as {alias};`"
        ))
    }
}

fn environment(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Env {
    let mut names: HashMap<String, Visible> = HashMap::new();

    // Built-ins first, so a user definition of one collides with it rather than
    // shadowing it. They share the ordinary function namespace deliberately: a call is a
    // call, and `str_len(s)` should resolve by the same rule as any other. They belong to
    // no module, so every module has all of them.
    for i in Intrinsic::ALL {
        // The table's shapes become types here, once: `Vec[T]` is interned, so it cannot
        // be written in a `const`. `T` is the built-in's own parameter.
        let owner = ParamOwner::Builtin(i);
        let t = Type::Param(TypeParam { owner, index: 0 });
        let mut shape = |s: Shape| match s.monomorphic() {
            Some(m) => m,
            None if s == Shape::T => t,
            None if s == Shape::ChanT => res.types_mut().chan_of(t),
            None if s == Shape::ChansT => {
                let c = res.types_mut().chan_of(t);
                res.types_mut().vec_of(c)
            }
            None if s == Shape::BoxT => res.types_mut().box_of(t),
            None if s == Shape::ListT => res.types_mut().list_of(t),
            None if s == Shape::CellT => res.types_mut().cell_of(t),
            None => res.types_mut().vec_of(t),
        };
        let params: Vec<Type> = i.params().iter().map(|s| shape(*s)).collect();
        let ret = shape(i.ret());
        if i.generics() > 0 {
            res.types_mut().name_params(owner, vec!["T".to_owned()]);
        }
        names.insert(
            i.name().to_owned(),
            Visible {
                sig: Signature {
                    params,
                    ret,
                    name_span: Span::synthetic(),
                    generics: i.generics(),
                    owner: (i.generics() > 0).then_some(owner),
                    effect_param: false,
                    preconditions: 0,
                    pure: matches!(
                        i,
                        Intrinsic::StrLen
                            | Intrinsic::StrByte
                            | Intrinsic::IntsLen
                            | Intrinsic::IntsGet
                            | Intrinsic::StrsLen
                            | Intrinsic::VecLen
                            | Intrinsic::VecGet
                    ),
                },
                callee: Callee::Builtin(i),
            },
        );
    }

    // This module's own definitions, exported or not. A module sees all of its own. An
    // impl's methods are not among them: a method is reached through its trait (N78).
    for (_, f) in &unit.functions {
        if f.method_of.is_some() {
            continue;
        }
        let Some(id) = res.definition(unit.module, &f.name) else {
            continue;
        };
        let Some(def) = res.function(id) else {
            continue;
        };
        names.insert(
            def.name.clone(),
            Visible {
                sig: Signature {
                    params: def.params.clone(),
                    ret: def.ret,
                    name_span: def.name_span,
                    generics: u32::try_from(def.type_params.len()).unwrap_or(u32::MAX),
                    owner: (!def.type_params.is_empty() || polymorphic(def.declared_effects))
                        .then_some(ParamOwner::Function(id)),
                    effect_param: polymorphic(def.declared_effects),
                    preconditions: def.preconditions,
                    pure: def.foreign.is_none()
                        && def.declared_effects == Some(nazm_sema::EffectSet::EMPTY),
                },
                callee: Callee::Function(id),
            },
        );
    }

    // The exports of each direct import, and both ways that can go wrong. `brought_in`
    // remembers which `use` introduced each name, so the second collision points at the
    // first rather than at "somewhere else".
    let mut brought_in: HashMap<&str, Span> = HashMap::new();
    let mut refused: Vec<String> = Vec::new();
    let mut aliases: HashMap<String, ModuleId> = HashMap::new();
    let mut alias_at: HashMap<String, Span> = HashMap::new();
    for d in unit.imports {
        let Some(iface) = deps.iter().find(|i| i.module() == d.on) else {
            continue;
        };
        // Under a name (N49): every export as `alias::name`, colliding with nothing — but the
        // alias itself may not be a name this module already gives something else.
        for (alias, at) in &d.aliases {
            let clash = if let Some(first) = alias_at.get(alias) {
                Some((*first, "another import already has this name"))
            } else if names
                .get(alias)
                .is_some_and(|v| matches!(v.callee, Callee::Function(_)))
            {
                names
                    .get(alias)
                    .map(|v| (v.sig.name_span, "a function of this module"))
            } else if res.records().declared(unit.module, alias).is_some()
                || res.enums().declared(unit.module, alias).is_some()
            {
                Some((*at, "a type of this module"))
            } else {
                None
            };
            if let Some((other, what)) = clash {
                errors.push(
                    Diagnostic::error(
                        codes::ALIAS_COLLIDES,
                        *at,
                        format!("`{alias}` already names {what}"),
                    )
                    .secondary(other, "the other meaning")
                    .help(
                        "an import's name is written before `::` and must mean only that \
                         module here: choose another",
                    ),
                );
                continue;
            }
            alias_at.insert(alias.clone(), *at);
            aliases.insert(alias.clone(), d.on);
            for e in iface.exports() {
                names.insert(
                    qualified_key(alias, &e.name),
                    Visible {
                        sig: Signature {
                            params: e.params.clone(),
                            ret: e.ret,
                            name_span: e.name_span,
                            generics: u32::try_from(e.type_params.len()).unwrap_or(u32::MAX),
                            owner: (!e.type_params.is_empty() || polymorphic(e.effects))
                                .then_some(ParamOwner::Function(e.def)),
                            effect_param: polymorphic(e.effects),
                            preconditions: e.preconditions,
                            pure: e.foreign.is_none()
                                && e.effects == Some(nazm_sema::EffectSet::EMPTY),
                        },
                        callee: Callee::Function(e.def),
                    },
                );
            }
        }
        if !d.plain {
            continue;
        }
        for e in iface.exports() {
            if let Some(own) = res
                .definition(unit.module, &e.name)
                .and_then(|id| res.function(id))
            {
                errors.push(
                    Diagnostic::error(
                        codes::IMPORT_COLLIDES_WITH_DEFINITION,
                        d.at,
                        format!(
                            "this import brings in `{}`, which this module defines",
                            e.name
                        ),
                    )
                    .secondary(own.name_span, "defined here")
                    .secondary(e.name_span, "and exported here")
                    .help(
                        "an imported name and a local one are not ranked: rename one of \
                         them, or stop exporting the other",
                    ),
                );
                refused.push(e.name.clone());
                continue;
            }
            if let Some(first) = brought_in.get(e.name.as_str()) {
                errors.push(
                    Diagnostic::error(
                        codes::AMBIGUOUS_IMPORT,
                        d.at,
                        format!("`{}` is exported by two imported modules", e.name),
                    )
                    .secondary(*first, "also brought in here")
                    .secondary(e.name_span, "exported here")
                    .help(
                        "there is no way to write which one is meant, so the ambiguity \
                         is refused where it is created rather than where it is used",
                    ),
                );
                refused.push(e.name.clone());
                continue;
            }
            brought_in.insert(e.name.as_str(), d.at);
            names.insert(
                e.name.clone(),
                Visible {
                    sig: Signature {
                        params: e.params.clone(),
                        ret: e.ret,
                        name_span: e.name_span,
                        generics: u32::try_from(e.type_params.len()).unwrap_or(u32::MAX),
                        owner: (!e.type_params.is_empty() || polymorphic(e.effects))
                            .then_some(ParamOwner::Function(e.def)),
                        effect_param: polymorphic(e.effects),
                        preconditions: e.preconditions,
                        pure: e.foreign.is_none() && e.effects == Some(nazm_sema::EffectSet::EMPTY),
                    },
                    callee: Callee::Function(e.def),
                },
            );
        }
    }

    Env {
        names,
        refused,
        aliases,
    }
}

/// The units' positions with every module after the modules it imports (L8), where the imports
/// allow it: a cycle's members come in the order first reached. Iterative, as every walk of the
/// module graph is.
fn dependency_order(units: &[Unit<'_>]) -> Vec<usize> {
    let mut state = vec![0_u8; units.len()];
    let mut out = Vec::with_capacity(units.len());
    for start in 0..units.len() {
        if state.get(start).copied() != Some(0) {
            continue;
        }
        let mut stack = vec![(start, 0_usize)];
        if let Some(s) = state.get_mut(start) {
            *s = 1;
        }
        while let Some((n, next)) = stack.pop() {
            let dep = units
                .get(n)
                .and_then(|u| u.imports.get(next))
                .map(|d| d.on.index());
            match dep {
                Some(d) => {
                    stack.push((n, next + 1));
                    if state.get(d).copied() == Some(0) {
                        if let Some(s) = state.get_mut(d) {
                            *s = 1;
                        }
                        stack.push((d, 0));
                    }
                }
                None => {
                    if let Some(s) = state.get_mut(n) {
                        *s = 2;
                    }
                    out.push(n);
                }
            }
        }
    }
    out
}

/// Stage 1b (R1a, L8): every module's constants, in dependency order, so a constant may be
/// computed from one an import exports; each module's interface then carries its own exported
/// values. The positions of the units that reported a refusal.
pub(super) fn declare_all_consts(
    units: &[Unit<'_>],
    interfaces: &mut [UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) -> Vec<usize> {
    let mut unclean = Vec::new();
    for n in dependency_order(units) {
        let Some(u) = units.get(n) else {
            continue;
        };
        let deps: Vec<&UnitInterface> = u
            .imports
            .iter()
            .filter_map(|d| interfaces.get(d.on.index()))
            .collect();
        let before = errors.len();
        let consts = declare_consts(u, &deps, res, errors);
        if errors.len() != before {
            unclean.push(n);
        }
        if let Some(i) = interfaces.get_mut(n) {
            *i = i.clone().with_consts(consts);
        }
    }
    unclean
}
