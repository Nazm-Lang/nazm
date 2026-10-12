//! Traits and impls: which traits and types are in scope, trait methods, and registering an
//! impl against its trait.

use super::generic::quoted;
use super::unit::{qualified_key, written_key};
use super::written::{resolve_written, type_environment, type_name_of, Collisions};
use super::*;

/// Every trait the module `unit` may name, by the name it writes (N78): its own, and the
/// exported traits of every module it imports — unqualified under a plain `use`, as
/// `alias::Name` under an alias. `None` for a name two imports give two traits, which a
/// use must then qualify. The module's own trait wins over an import's, as a module's own
/// record does.
pub(super) fn trait_scope(unit: &Unit<'_>, res: &Resolution) -> HashMap<String, Option<TraitId>> {
    let mut own: HashMap<String, Option<TraitId>> = HashMap::new();
    for (id, t) in res.traits().iter() {
        if t.module == unit.module {
            own.insert(t.name.clone(), Some(id));
        }
    }
    let mut out: HashMap<String, Option<TraitId>> = HashMap::new();
    for d in unit.imports {
        let reexported: Vec<TraitId> = res.traits().reexported_by(d.on).collect();
        for (id, t) in res.traits().iter() {
            // Its own exported traits, and those it re-exports (N103).
            let offered =
                (t.module == d.on && t.visibility.is_exported()) || reexported.contains(&id);
            if !offered {
                continue;
            }
            if d.plain {
                out.entry(t.name.clone())
                    .and_modify(|e| {
                        if *e != Some(id) {
                            *e = None;
                        }
                    })
                    .or_insert(Some(id));
            }
            for (alias, _) in &d.aliases {
                out.insert(qualified_key(alias, &t.name), Some(id));
            }
        }
    }
    out.extend(own);
    out
}

/// Every type an interface lets an importer name, by the name it writes: each nameable record
/// and enum by its own, and each type re-exported under another (N103).
pub(super) fn offered_types(iface: &UnitInterface, res: &Resolution) -> Vec<(String, Type)> {
    let mut out: Vec<(String, Type)> = iface
        .records()
        .iter()
        .filter(|e| e.nameable)
        .map(|e| Type::Record(e.def))
        .chain(
            iface
                .enums()
                .iter()
                .filter(|e| e.nameable)
                .map(|e| Type::Enum(e.def)),
        )
        .filter_map(|t| type_name_of(res, t).map(|n| (n, t)))
        .collect();
    out.extend(iface.renamed().iter().cloned());
    out
}

/// The modules whose impls `unit` may use (N78): itself and every module it imports
/// directly — and every module those re-export from (N103), whose impls their interfaces
/// carry. Coherence is over the whole compilation; *use* is limited to what the module's own
/// check key covers, so a cached check never depends on an impl it did not see.
pub(super) fn impl_scope(unit: &Unit<'_>, res: &Resolution) -> Vec<ModuleId> {
    let mut out = vec![unit.module];
    let mut work: Vec<ModuleId> = unit.imports.iter().map(|d| d.on).collect();
    while let Some(m) = work.pop() {
        if out.contains(&m) {
            continue;
        }
        out.push(m);
        work.extend(res.traits().reexport_sources(m));
    }
    out
}

/// Stage 0c (N78): every trait's method signatures, with `Self` the trait's own type
/// parameter. After every type is declared and settled, before any module's functions:
/// an impl may be in a module declared before its trait's.
pub fn declare_trait_methods(
    units: &[Unit<'_>],
    type_interfaces: &[UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    for unit in units {
        if unit.traits.is_empty() {
            continue;
        }
        let deps: Vec<&UnitInterface> = unit
            .imports
            .iter()
            .filter_map(|d| type_interfaces.get(d.on.index()))
            .collect();
        let types = type_environment(unit, &deps, res, errors, Collisions::Silent);
        for t in &unit.traits {
            let Some(id) = res
                .traits()
                .iter()
                .find(|(_, d)| d.module == unit.module && d.name_span == t.name_span)
                .map(|(id, _)| id)
            else {
                continue;
            };
            let me = Type::Param(id.self_param());
            res.types_mut()
                .name_params(ParamOwner::Trait(id), vec!["Self".to_owned()]);
            let scope = vec![("Self".to_owned(), me)];
            let mut methods: Vec<TraitMethodDef> = Vec::new();
            for m in &t.methods {
                if let Some(first) = methods.iter().find(|x| x.name == m.name) {
                    errors.push(
                        Diagnostic::error(
                            codes::DUPLICATE_FUNCTION,
                            m.name_span,
                            format!(
                                "`{}` is declared more than once in trait `{}`",
                                m.name, t.name
                            ),
                        )
                        .secondary(first.name_span, "first declared here"),
                    );
                    continue;
                }
                let resolved: Vec<Type> = m
                    .params
                    .iter()
                    .map(|p| resolve_written(&types, res, &p.ty, &scope, None, errors))
                    .collect();
                let self_ok = m.params.first().is_some_and(|p| p.name == "self")
                    && resolved.first() == Some(&me);
                if !self_ok {
                    errors.push(
                        Diagnostic::error(
                            codes::TRAIT_METHOD_SELF,
                            m.params.first().map_or(m.name_span, |p| p.span),
                            format!("`{}`'s first parameter must be `self: Self`", m.name),
                        )
                        .help(
                            "a trait method is called on a value of the implementing type, \
                             which it receives first: `fn show(self: Self) -> Str;`",
                        ),
                    );
                    continue;
                }
                let ret = resolve_written(&types, res, &m.return_type, &scope, None, errors);
                let effects = m
                    .effects
                    .as_ref()
                    .and_then(|d| crate::effects::declared(d, None, errors))
                    .unwrap_or_default();
                methods.push(TraitMethodDef {
                    name: m.name.clone(),
                    name_span: m.name_span,
                    params: resolved.into_iter().skip(1).collect(),
                    ret,
                    effects,
                    has_default: m.body.is_some(),
                });
            }
            if let Some(def) = res.traits_mut().get_mut(id) {
                def.methods = methods;
            }
        }
    }
}

/// The trait a written name in `unit` refers to (N78), or the diagnostic saying why none.
// One diagnostic, built only when the name does not resolve: not worth a box.
#[allow(clippy::result_large_err)]
pub(super) fn named_trait(
    written: &TypeName,
    scope: &HashMap<String, Option<TraitId>>,
    res: &Resolution,
    unit: &Unit<'_>,
) -> Result<TraitId, Diagnostic> {
    let key = written_key(written.qualifier.as_ref(), &written.name);
    match scope.get(&key) {
        Some(Some(id)) => Ok(*id),
        Some(None) => Err(Diagnostic::error(
            codes::AMBIGUOUS_IMPORT,
            written.name_span,
            format!(
                "`{}` is a trait in more than one imported module",
                written.name
            ),
        )
        .help("import one of them under a name and write `alias::Trait`")),
        None => {
            let is_type = res.records().declared(unit.module, &written.name).is_some()
                || res.enums().declared(unit.module, &written.name).is_some()
                || Type::from_name(&written.name).is_some();
            Err(if is_type {
                Diagnostic::error(
                    codes::NOT_A_TRAIT,
                    written.name_span,
                    format!("`{}` is a type, not a trait", written.name),
                )
                .help("a bound or an impl names a trait: `impl Show for Point`")
            } else {
                Diagnostic::error(
                    codes::UNKNOWN_TYPE,
                    written.name_span,
                    format!("no trait named `{}` is in scope", written.name),
                )
                .help("declare it, or import the module that exports it")
            })
        }
    }
}

/// Whether `ty` may have an impl (N78, `architecture.md` §7.79), and the module that
/// declares it — `None` for a built-in.
/// An impl for a generic type's header (R1b, L11b): `D[T1, …]`, a generic record or enum applied
/// to exactly the impl's parameters, in order. Its answer is the generic definition the impl is
/// filed under, as `Type::Record`/`Type::Enum` of its template.
pub(super) fn generic_impl_type(
    names: &HashMap<String, Type>,
    res: &Resolution,
    imp: &ImplDecl,
) -> Result<Type, String> {
    let written = &imp.self_ty;
    let key = super::unit::written_key(written.qualifier.as_ref(), &written.name);
    let template = names
        .get(&key)
        .copied()
        .ok_or_else(|| format!("`{}` is not a generic record or enum", written.name))?;
    let params = match template {
        Type::Record(id) => res
            .records()
            .get(id)
            .filter(|d| d.instance.is_none())
            .map(|d| d.params.len()),
        Type::Enum(id) => res
            .enums()
            .get(id)
            .filter(|d| d.instance.is_none())
            .map(|d| d.params.len()),
        _ => None,
    }
    .filter(|n| *n > 0)
    .ok_or_else(|| format!("`{}` is not a generic record or enum", written.name))?;
    let args = written.args.as_deref().unwrap_or_default();
    let exact = params == imp.type_params.len()
        && args.len() == params
        && args.iter().zip(&imp.type_params).all(|(a, p)| {
            a.qualifier.is_none() && a.args.is_none() && a.function.is_none() && a.name == p.name
        });
    if !exact {
        let wanted: Vec<&str> = imp.type_params.iter().map(|p| p.name.as_str()).collect();
        return Err(format!(
            "an impl with type parameters is for `{}[{}]`: the type applied to exactly its \
             parameters, in order",
            written.name,
            wanted.join(", ")
        ));
    }
    Ok(template)
}

/// The module a generic definition's impl may be in besides its trait's (R1b, L11b): the
/// definition's.
pub(super) fn template_home(res: &Resolution, ty: Type) -> Option<ModuleId> {
    match ty {
        Type::Record(id) => res.records().get(id).map(|d| d.module),
        Type::Enum(id) => res.enums().get(id).map(|d| d.module),
        _ => None,
    }
}

pub(super) fn implementable(res: &Resolution, ty: Type) -> Result<Option<ModuleId>, &'static str> {
    match ty {
        Type::Int | Type::Num(_) | Type::Bool | Type::Str => Ok(None),
        Type::Record(id) => {
            let def = res.records().get(id).ok_or("an unknown record")?;
            if !def.params.is_empty() || def.instance.is_some() {
                return Err("a generic record");
            }
            Ok(Some(def.module))
        }
        Type::Enum(id) => {
            let def = res.enums().get(id).ok_or("an unknown enum")?;
            if !def.params.is_empty() || def.instance.is_some() {
                return Err("a generic enum");
            }
            Ok(Some(def.module))
        }
        Type::Param(_) => Err("a type parameter"),
        _ => Err("a built-in type other than a number, `Bool` or `Str`"),
    }
}

/// The function name an impl method is declared under: `Trait/Type/m`, which no written
/// name can be (N78).
/// The definition a written function declared: by its name, or — for an impl's method,
/// which is declared under `Trait/Type/m` — by where its name is (N78).
pub(crate) fn defined(res: &Resolution, module: ModuleId, f: &Function) -> Option<FnId> {
    if f.method_of.is_none() {
        return res.definition(module, &f.name);
    }
    res.functions()
        .iter()
        .position(|d| d.module == module && d.name_span == f.name_span && d.closure.is_none())
        .map(FnId::from_index)
}

pub(super) fn impl_method_name(trait_name: &str, ty: &str, method: &str) -> String {
    format!("{trait_name}/{ty}/{method}")
}

/// File one impl whose header resolved (N78): its methods against its trait's, the orphan
/// rule, and the coherence table. Every rule is reported; the impl is filed only if its
/// module may hold it, and it is the table's answer only if it is the first of its kind.
pub(super) fn register_impl(
    unit: &Unit<'_>,
    imp: &ImplDecl,
    trait_id: TraitId,
    ty: Type,
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
) {
    let Some(tr) = res.traits().get(trait_id).cloned() else {
        return;
    };
    let ty_name = res.types().show(ty);
    // The orphan rule: the trait's module, or the type's.
    let generic = !imp.type_params.is_empty();
    let home = if generic {
        template_home(res, ty)
    } else {
        implementable(res, ty).ok().flatten()
    };
    if tr.module != unit.module && home != Some(unit.module) {
        errors.push(
            Diagnostic::error(
                codes::IMPL_ORPHAN,
                imp.impl_span,
                format!(
                    "an impl of `{}` for `{ty_name}` must be in the module declaring `{}` or \
                     the one declaring `{ty_name}`",
                    tr.name, tr.name
                ),
            )
            .secondary(tr.name_span, "the trait is declared here")
            .help(
                "one impl per trait and type, where everyone who can name both can find it: \
                 move it to the trait's module or the type's",
            ),
        );
        return;
    }
    let mut methods: Vec<Option<FnId>> = vec![None; tr.methods.len()];
    for (_, f) in unit.functions.iter().filter(|(_, f)| {
        f.method_of
            .as_ref()
            .is_some_and(|m| m.impl_span == imp.impl_span)
    }) {
        let Some(id) = res.definition(unit.module, &impl_method_name(&tr.name, &ty_name, &f.name))
        else {
            continue;
        };
        let Some(index) = tr.method(&f.name) else {
            errors.push(
                Diagnostic::error(
                    codes::IMPL_METHOD_MISMATCH,
                    f.name_span,
                    format!("`{}` is not a method of `{}`", f.name, tr.name),
                )
                .secondary(tr.name_span, "the trait is declared here")
                .help("an impl defines its trait's methods and nothing else"),
            );
            continue;
        };
        let Some(want) = tr.methods.get(index as usize) else {
            continue;
        };
        let Some(def) = res.function(id).cloned() else {
            continue;
        };
        // An impl of a generic type (L11b): `Self` is the definition applied to this method's
        // own copies of the impl's parameters, which the parser wrote first.
        let n = imp.type_params.len();
        let ty = if generic {
            let own: Vec<Type> = (0..n)
                .map(|i| {
                    Type::Param(nazm_sema::TypeParam {
                        owner: ParamOwner::Function(id),
                        index: u32::try_from(i).unwrap_or(u32::MAX),
                    })
                })
                .collect();
            match ty {
                Type::Record(r) => res.types_mut().apply_record(r, own),
                Type::Enum(e) => res.types_mut().apply_enum(e, own),
                other => other,
            }
        } else {
            ty
        };
        let types = res.types_mut();
        let owner = ParamOwner::Trait(trait_id);
        let params: Vec<Type> = std::iter::once(ty)
            .chain(
                want.params
                    .iter()
                    .map(|t| types.substitute(*t, owner, &[ty])),
            )
            .collect();
        let ret = types.substitute(want.ret, owner, &[ty]);
        let self_named = f.params.first().is_some_and(|p| p.name == "self");
        let effects_match = def.declared_effects.unwrap_or_default() == want.effects;
        if def.params != params
            || def.ret != ret
            || !self_named
            || !effects_match
            || def.type_params.len() != n
        {
            let wanted: Vec<String> = params.iter().map(|t| types.show(*t)).collect();
            errors.push(
                Diagnostic::error(
                    codes::IMPL_METHOD_MISMATCH,
                    f.name_span,
                    format!(
                        "`{}` must be `fn {}(self: {}) -> {}` with the trait's effects, as `{}` \
                         declares it with `Self` = `{ty_name}`",
                        f.name,
                        f.name,
                        wanted.join(", "),
                        types.show(ret),
                        tr.name
                    ),
                )
                .secondary(want.name_span, "the trait's method"),
            );
            continue;
        }
        if let Some(slot) = methods.get_mut(index as usize) {
            *slot = Some(id);
        }
    }
    let missing: Vec<&str> = tr
        .methods
        .iter()
        .zip(&methods)
        // A method with a default body (R1b, L11) may be left out.
        .filter(|(t, m)| m.is_none() && !t.has_default)
        .map(|(m, _)| m.name.as_str())
        .collect();
    if !missing.is_empty() {
        errors.push(
            Diagnostic::error(
                codes::IMPL_MISSING_METHOD,
                imp.impl_span,
                format!(
                    "this impl of `{}` for `{ty_name}` does not define {}",
                    tr.name,
                    quoted(&missing.iter().map(|m| (*m).to_owned()).collect::<Vec<_>>())
                ),
            )
            .secondary(tr.name_span, "the trait is declared here")
            .help("an impl defines every method of its trait that has no default body"),
        );
    }
    // What each parameter of an impl of a generic type requires (L11b), resolved as a
    // function's requirements are; an unknown name was reported on the methods.
    let names = trait_scope(unit, res);
    let requires = imp
        .type_params
        .iter()
        .map(|p| {
            let req = p.requires.as_ref()?;
            if req.name == super::declare::EQUALITY {
                Some(nazm_sema::traits::Require::Equality)
            } else {
                names
                    .get(&req.name)
                    .copied()
                    .flatten()
                    .map(nazm_sema::traits::Require::Trait)
            }
        })
        .collect();
    let filed = res.traits_mut().implement(ImplDef {
        trait_id,
        self_ty: ty,
        module: unit.module,
        impl_span: imp.impl_span,
        methods,
        requires,
    });
    if let Err(first) = filed {
        let at = res
            .traits()
            .impl_def(first)
            .map_or(imp.impl_span, |d| d.impl_span);
        errors.push(
            Diagnostic::error(
                codes::IMPL_CONFLICT,
                imp.impl_span,
                format!("`{ty_name}` already implements `{}`", tr.name),
            )
            .secondary(at, "the first impl")
            .help("a type implements a trait at most once in a program, so every call means one function"),
        );
    }
}
