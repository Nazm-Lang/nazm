//! Written types: the type environment a module sees, and resolving a type as written to the
//! type it names.

use super::declare::VEC;
use super::generic::channel_element;
use super::layout::array_length;
use super::traits::offered_types;
use super::unit::{qualified_key, unknown_qualified, written_key};
use super::*;

/// Whether a table entry is a definition as written, rather than an application of one.
pub(super) fn is_declaration(res: &Resolution, node: TypeNode) -> bool {
    match node {
        TypeNode::Record(id) => res.records().get(id).is_some_and(|d| d.instance.is_none()),
        TypeNode::Enum(id) => res.enums().get(id).is_some_and(|d| d.instance.is_none()),
    }
}

/// One hop of an ownership path, as a reader would find it: `Node.children`, or
/// `Tree.Branch.kids` for a payload field.
pub(super) fn hop_name(res: &Resolution, via: &nazm_sema::CycleEdge) -> String {
    match (via.from, via.variant) {
        (TypeNode::Record(id), _) => res
            .records()
            .get(id)
            .and_then(|d| {
                d.fields
                    .get(via.field as usize)
                    .map(|f| format!("{}.{}", d.name, f.name))
            })
            .unwrap_or_default(),
        (TypeNode::Enum(id), Some(v)) => res
            .enums()
            .get(id)
            .and_then(|d| {
                let vd = d.variants.get(v as usize)?;
                vd.fields
                    .get(via.field as usize)
                    .map(|f| format!("{}.{}.{}", d.name, vd.name, f.name))
            })
            .unwrap_or_default(),
        (TypeNode::Enum(_), None) => String::new(),
    }
}

/// Where the field that closes a cycle is written.
pub(super) fn closing_field(res: &Resolution, via: &nazm_sema::CycleEdge) -> Option<Span> {
    match (via.from, via.variant) {
        (TypeNode::Record(id), _) => res
            .records()
            .get(id)?
            .fields
            .get(via.field as usize)
            .map(|f| f.name_span),
        (TypeNode::Enum(id), Some(v)) => res
            .enums()
            .get(id)?
            .variants
            .get(v as usize)?
            .fields
            .get(via.field as usize)
            .map(|f| f.name_span),
        (TypeNode::Enum(_), None) => None,
    }
}

/// Where a definition's name is written.
pub(super) fn node_span(res: &Resolution, node: TypeNode) -> Option<Span> {
    match node {
        TypeNode::Record(id) => res.records().get(id).map(|d| d.name_span),
        TypeNode::Enum(id) => res.enums().get(id).map(|d| d.name_span),
    }
}

/// The type names one module may write.
///
/// Its own records — public and private, since a module sees all of its own — then the
/// records each direct import makes nameable. Both ways a name can collide are reported
/// here, with the same wording and the same philosophy as the function namespace's, and
/// for the same reason: an ambiguity is refused where it is created.
/// Whether a call to [`type_environment`] is the one that reports name collisions.
///
/// Three stages build the same environment — field resolution, signature declaration and
/// body checking — and a collision is one mistake, so exactly one of them says so.
/// `docs/spec.md` refuses an ambiguity where it is created, and the earliest stage that
/// can see it is where it is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Collisions {
    Report,
    Silent,
}

pub(super) fn type_environment(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    res: &Resolution,
    errors: &mut Vec<Diagnostic>,
    collisions: Collisions,
) -> Types {
    let mut names: HashMap<String, Type> = HashMap::new();
    for (_, r) in &unit.records {
        if let Some(id) = res.records().declared(unit.module, &r.name) {
            names.insert(r.name.clone(), Type::Record(id));
        }
    }
    for (_, e) in &unit.enums {
        if let Some(id) = res.enums().declared(unit.module, &e.name) {
            names.insert(e.name.clone(), Type::Enum(id));
        }
    }

    let mut brought_in: HashMap<String, Span> = HashMap::new();
    let mut refused: Vec<String> = Vec::new();
    let mut aliases: Vec<String> = Vec::new();
    for d in unit.imports {
        let Some(iface) = deps.iter().find(|i| i.module() == d.on) else {
            continue;
        };
        // Under a name (N49): every nameable type as `alias::Name`, colliding with nothing.
        for (alias, _) in &d.aliases {
            aliases.push(alias.clone());
            for (name, offer) in offered_types(iface, res) {
                names.insert(qualified_key(alias, &name), offer);
            }
        }
        if !d.plain {
            continue;
        }
        // Records then enums, each the ones this interface makes nameable. One loop body
        // for both: which kind a name refers to changes the `Type` that goes in the map
        // and nothing else about the rule.
        for (name, offer) in offered_types(iface, res) {
            let Some(declared_at) = user_type_span(res, offer) else {
                continue;
            };
            // The prelude's standard modules (R1.5, D9) yield to the module's own types and to
            // a written import's.
            if d.implicit
                && (res.records().declared(unit.module, &name).is_some()
                    || res.enums().declared(unit.module, &name).is_some()
                    || brought_in.contains_key(&name))
            {
                continue;
            }
            if let Some(own) = names
                .get(&name)
                .copied()
                .and_then(|t| user_type_span(res, t))
                .filter(|_| {
                    res.records().declared(unit.module, &name).is_some()
                        || res.enums().declared(unit.module, &name).is_some()
                })
            {
                // The prelude's names are reserved rather than colliding at an import: no
                // `use` was written, so there is no import line to point at, and the rule
                // is about the declaration. A project `Result` would otherwise be the one
                // ordinary lookup finds while `?` still meant the core one.
                if Some(d.on) == unit.prelude {
                    if collisions == Collisions::Report {
                        errors.push(
                            Diagnostic::error(
                                codes::PRELUDE_TYPE_REDECLARED,
                                own,
                                format!("`{name}` is a core prelude type"),
                            )
                            .secondary(declared_at, "declared by the core prelude here")
                            .help(
                                "every module sees the prelude's `Result` and `Option` \
                                 without importing them, and there is no way to shadow \
                                 one. Give this type another name",
                            ),
                        );
                    }
                    refused.push(name);
                    continue;
                }
                if collisions == Collisions::Report {
                    errors.push(
                        Diagnostic::error(
                            codes::IMPORT_COLLIDES_WITH_DEFINITION,
                            d.at,
                            format!(
                                "this import brings in the type `{name}`, which this \
                                 module declares"
                            ),
                        )
                        .secondary(own, "declared here")
                        .secondary(declared_at, "and exported here")
                        .help(
                            "an imported type and a local one are not ranked: rename one \
                             of them, or stop exporting the other",
                        ),
                    );
                }
                refused.push(name);
                continue;
            }
            if let Some(first) = brought_in.get(&name) {
                // Only reachable when another import exports a prelude name, which that
                // module's own declaration was already refused for. One mistake, one
                // error: the prelude's meaning stands and nothing more is said here.
                if Some(d.on) == unit.prelude {
                    names.insert(name, offer);
                    continue;
                }
                if collisions == Collisions::Report {
                    errors.push(
                        Diagnostic::error(
                            codes::AMBIGUOUS_IMPORT,
                            d.at,
                            format!("the type `{name}` is exported by two imported modules"),
                        )
                        .secondary(*first, "also brought in here")
                        .secondary(declared_at, "exported here")
                        .help(
                            "there is no way to write which one is meant, so the \
                             ambiguity is refused where it is created rather than where \
                             it is used",
                        ),
                    );
                }
                refused.push(name);
                continue;
            }
            brought_in.insert(name.clone(), d.at);
            names.insert(name, offer);
        }
    }
    // Constants are computed here, where every type position can read them; their refusals
    // are reported once, by stage 1 (`compute_aggregates`), which is the first pass that has
    // every import's constants (L8).
    let consts =
        crate::constants::evaluate(&unit.consts, imported_consts(unit, deps), &mut Vec::new());
    Types {
        names,
        refused,
        aliases,
        consts,
    }
}

/// The constants a module's imports export (L8), under the name it writes for each: `NAME` from
/// a plain import, `m::NAME` from one under a name.
fn imported_consts(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
) -> Vec<(String, (Type, crate::constants::ConstValue))> {
    let mut imported = Vec::new();
    for d in unit.imports {
        let Some(iface) = deps.iter().find(|i| i.module() == d.on) else {
            continue;
        };
        for c in iface.consts() {
            for (alias, _) in &d.aliases {
                imported.push((qualified_key(alias, &c.name), (c.ty, c.value.clone())));
            }
            if d.plain {
                imported.push((c.name.clone(), (c.ty, c.value.clone())));
            }
        }
    }
    imported
}

/// Each constant of a record, enum, tuple or array type (R1a, L8), computed where the module's
/// types are known; every constant's refusals reported only where `report`, the one pass that
/// does — stage 1, which runs for every module and has every import's constants.
pub(super) fn compute_aggregates(
    unit: &Unit<'_>,
    deps: &[&UnitInterface],
    types: &mut Types,
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
    report: bool,
) {
    let aggregates: Vec<&nazm_syntax::Const> = unit
        .consts
        .iter()
        .copied()
        .filter(|c| crate::constants::is_aggregate(c))
        .collect();
    let mut scratch = Vec::new();
    let sink = if report { errors } else { &mut scratch };
    if report {
        // The scalars' refusals, once (the environment computed them silently).
        let _ = crate::constants::evaluate(&unit.consts, imported_consts(unit, deps), sink);
    }
    if aggregates.is_empty() {
        return;
    }
    let mut tys = Vec::new();
    for c in aggregates {
        let before = sink.len();
        let t = resolve_written(types, res, &c.ty, &[], None, sink);
        // A type that did not resolve is one error, not a second about the value.
        if sink.len() == before {
            tys.push((c.name.clone(), t));
        }
    }
    crate::constants::evaluate_aggregates(&mut types.consts, &unit.consts, &tys, res.types(), sink);
}

/// Record what a module may name, as its checker is about to use it (N19): every function
/// and built-in in its environment, every type in its type namespace and every built-in
/// type — less the names the environments refused, which mean nothing unambiguously.
pub(super) fn record_module_names(
    res: &mut Resolution,
    module: ModuleId,
    env: &Env,
    types: &Types,
) {
    // Its constants, and every constant named in another's value (Gate 2), for the index.
    let mut decls: Vec<Span> = types.consts.decls.values().copied().collect();
    decls.sort_by_key(|s| (s.file, s.start));
    for d in decls {
        res.record_const_decl(d);
    }
    for (at, name) in &types.consts.refs {
        if let Some(decl) = types.consts.decls.get(name) {
            res.record_const_ref(*at, *decl);
        }
    }
    // Its statics (Gate 3), in declaration order: the storage its unit defines.
    let mut statics: Vec<(&String, Span)> = types
        .consts
        .statics
        .iter()
        .filter_map(|n| types.consts.decls.get(n).map(|d| (n, *d)))
        .collect();
    statics.sort_by_key(|(_, s)| (s.file, s.start));
    for (name, at) in statics {
        let value = match (types.consts.cells.get(name), types.consts.values.get(name)) {
            (Some(init), _) => nazm_sema::StaticValue::Cell(*init),
            (None, Some((_, v))) => nazm_sema::StaticValue::Value(v.clone()),
            (None, None) => continue,
        };
        res.record_static(nazm_sema::StaticDef {
            module,
            name: name.clone(),
            value,
            at,
            section: types.consts.sections.get(name).cloned(),
        });
    }
    use nazm_sema::scope::{defined, Candidate, Named};
    let callables = env
        .names
        .iter()
        .filter(|(n, _)| !env.refused.contains(n))
        .map(|(name, v)| Named {
            name: name.clone(),
            candidate: match v.callee {
                Callee::Builtin(i) => Candidate::Intrinsic(i),
                Callee::Function(id) => Candidate::Entity(nazm_sema::ReferenceTarget::Function(id)),
                // An environment holds named functions only; an indirect callee is a value.
                Callee::Indirect(_) => Candidate::Intrinsic(nazm_sema::Intrinsic::StrLen),
            },
        })
        .collect::<Vec<Named>>();
    // `println` and `eprintln` are names of `print` and `eprint` (R1, 5b.4, 5b.13): a call of
    // one resolves to that built-in, so a scope offers it under the name that was written.
    let printed: Vec<Named> = callables
        .iter()
        .filter_map(|n| {
            match n.candidate {
                Candidate::Intrinsic(nazm_sema::Intrinsic::Print) if n.name == "print" => {
                    Some("println")
                }
                Candidate::Intrinsic(nazm_sema::Intrinsic::EPrint) if n.name == "eprint" => {
                    Some("eprintln")
                }
                _ => None,
            }
            .map(|name| Named {
                name: name.to_owned(),
                candidate: n.candidate,
            })
        })
        .collect();
    let mut callables = callables;
    callables.extend(printed);
    let mut named: Vec<Named> = types
        .names
        .iter()
        .filter(|(n, _)| !types.refused.contains(n))
        .filter_map(|(name, t)| {
            TypeNode::of(*t).map(|node| Named {
                name: name.clone(),
                candidate: defined(node),
            })
        })
        .collect();
    for t in Type::ALL {
        if let Some(name) = t.builtin_name() {
            named.push(Named {
                name: name.to_owned(),
                candidate: Candidate::BuiltinType(*t),
            });
        }
    }
    named.push(Named {
        name: VEC.to_owned(),
        candidate: Candidate::BuiltinVec,
    });
    res.scopes_mut().module(module, callables, named);
}

/// Record a definition's type parameters over its declaration (N19), each with the type
/// name it hides — found here by the same lookup `resolve_written_inner` makes after it.
pub(super) fn record_type_scope(
    res: &mut Resolution,
    span: Span,
    scope: &[(String, Type)],
    types: &Types,
) {
    use nazm_sema::scope::{defined, Candidate};
    let params = scope
        .iter()
        .filter_map(|(name, t)| {
            let Type::Param(p) = t else { return None };
            let hides = if name == VEC {
                Some(Candidate::BuiltinVec)
            } else if let Some(b) = Type::from_name(name) {
                Some(Candidate::BuiltinType(b))
            } else {
                types
                    .names
                    .get(name)
                    .and_then(|t| TypeNode::of(*t))
                    .map(defined)
            };
            Some((name.clone(), *p, hides))
        })
        .collect();
    res.scopes_mut().type_scope(span, params);
}

/// A user-defined type's declared name, or `None` for a built-in.
pub(super) fn type_name_of(res: &Resolution, t: Type) -> Option<String> {
    match t {
        Type::Record(id) => res.records().get(id).map(|d| d.name.clone()),
        Type::Enum(id) => res.enums().get(id).map(|d| d.name.clone()),
        _ => None,
    }
}

/// Where a user-defined type's name is written.
fn user_type_span(res: &Resolution, t: Type) -> Option<Span> {
    TypeNode::of(t).and_then(|n| node_span(res, n))
}

/// A type name, against the built-ins and then this module's visible types.
///
/// Built-ins first and unshadowable, which is why they are matched before the map is
/// consulted rather than being put in it. A name alone: whether a generic definition may be
/// named without its arguments is the caller's question, because a construction's head and a
/// pattern may and a field's type may not.
fn resolve_name(
    types: &Types,
    res: &Resolution,
    name: &str,
    span: Span,
    errors: &mut Vec<Diagnostic>,
) -> Type {
    if let Some(t) = Type::from_name(name) {
        // One list type (R1, 5b.18): `Ints` is `Vec[Int]` and `Strs` is `Vec[Str]`, interned
        // with the built-ins.
        return match t {
            Type::Ints => res.types().find_vec(Type::Int).unwrap_or(t),
            Type::Strs => res.types().find_vec(Type::Str).unwrap_or(t),
            t => t,
        };
    }
    if let Some(t) = types.names.get(name) {
        return *t;
    }
    // `Map[K, V]` (R1.5, D5): the standard hash map under a shorter name, where it is in scope.
    if name == "Map" {
        if let Some(t) = types.names.get("HashMap") {
            return *t;
        }
        errors.push(
            Diagnostic::error(
                codes::UNKNOWN_TYPE,
                span,
                "`Map` is `@std/hashmap`'s `HashMap`",
            )
            .help("import it: `use \"@std/hashmap\";`"),
        );
        return Type::Int;
    }
    if name.contains("::") {
        let aliases: Vec<&str> = types.aliases.iter().map(String::as_str).collect();
        errors.push(unknown_qualified(name, &aliases, span, "type"));
        return Type::Int;
    }
    // A trait where a type is expected (N78): there are no trait objects.
    let bare = name.rsplit("::").next().unwrap_or(name);
    if let Some((_, t)) = res.traits().iter().find(|(_, t)| t.name == bare) {
        errors.push(
            Diagnostic::error(
                codes::TRAIT_NOT_A_TYPE,
                span,
                format!("`{name}` is a trait, not a type"),
            )
            .secondary(t.name_span, "the trait")
            .help(format!(
                "a value has one concrete type; to accept any type with `{bare}`, take a type \
                 parameter that requires it: `fn f[T: {bare}](x: T)`"
            )),
        );
        return Type::Int;
    }
    let mut d = Diagnostic::error(codes::UNKNOWN_TYPE, span, format!("`{name}` is not a type"))
        .help(format!(
            "the built-in types are {}, and `struct` and `enum` declare others",
            Type::all_names()
        ));
    // The same courtesy the function namespace extends: a type that exists somewhere is
    // more usefully reported as unreachable than as non-existent.
    let elsewhere = res
        .records()
        .declared_anywhere(name)
        .next()
        .and_then(|id| res.records().get(id))
        .map(|d| d.name_span)
        .or_else(|| {
            res.enums()
                .declared_anywhere(name)
                .next()
                .and_then(|id| res.enums().get(id))
                .map(|d| d.name_span)
        });
    if let Some(at) = elsewhere {
        d = d.secondary(at, "a type of this name is declared here");
    }
    errors.push(d);
    // Continue as `Int` rather than abandoning: one unknown type should not suppress
    // every other error around it.
    Type::Int
}

/// A written type, resolved: its name, its arguments, and the application they make.
///
/// Where a type is written in full — a field, a parameter, a return, a type argument. A
/// type parameter in `scope` shadows every other name; `Vec` is the one generic built-in;
/// and a generic definition must be written with exactly as many arguments as it declares.
/// The bound on how deeply a written type may nest is checked once, at the top.
pub(super) fn resolve_written(
    types: &Types,
    res: &mut Resolution,
    written: &TypeName,
    scope: &[(String, Type)],
    effect: Option<(&str, ParamOwner)>,
    errors: &mut Vec<Diagnostic>,
) -> Type {
    let t = resolve_written_inner(types, res, written, scope, effect, errors);
    if res.types().nesting(t) > MAX_TYPE_DEPTH {
        errors.push(
            Diagnostic::error(
                codes::TYPE_NESTING_TOO_DEEP,
                written.span,
                format!("types may nest {MAX_TYPE_DEPTH} deep"),
            )
            .help(
                "each level multiplies the copy and cleanup code generated for every value \
                 of the outer type, so the bound is on the emitted program rather than on \
                 the compiler. It is stated rather than discovered",
            ),
        );
        return Type::Int;
    }
    t
}

fn resolve_written_inner(
    types: &Types,
    res: &mut Resolution,
    written: &TypeName,
    scope: &[(String, Type)],
    effect: Option<(&str, ParamOwner)>,
    errors: &mut Vec<Diagnostic>,
) -> Type {
    let args: Option<Vec<Type>> = written.args.as_ref().map(|a| {
        a.iter()
            .map(|w| resolve_written_inner(types, res, w, scope, effect, errors))
            .collect()
    });
    // An effect parameter is named only in function types (N51): inside a type's
    // arguments it would make two instances of one type that differ only in effects.
    if let (Some((name, owner)), Some(args)) = (effect, &args) {
        if args
            .iter()
            .any(|a| res.types().names_effect_param(*a, owner))
        {
            errors.push(
                Diagnostic::error(
                    codes::EFFECT_PARAMETER,
                    written.span,
                    format!("the effect parameter `{name}` is named inside this type's arguments"),
                )
                .help(
                    "an effect parameter may be named in a function type that is a \
                     parameter's or the result's, or one nested in those: write the function \
                     type there",
                ),
            );
        }
    }
    // `(A, B, …)` (L5): a tuple of the elements written.
    if written.name == TypeName::TUPLE && written.qualifier.is_none() {
        let elems = args.clone().unwrap_or_default();
        let t = res.types_mut().tuple_of(elems);
        res.record_written_type(written.name_span, WrittenType::Builtin(t));
        return t;
    }
    // `[T; N]` (Gate 2): an array of the element written, its length a constant.
    if let (Some(len), true) = (&written.length, written.name == TypeName::ARRAY) {
        let elem = args
            .as_ref()
            .and_then(|a| a.first().copied())
            .unwrap_or(Type::Int);
        if let Expr::Var { name, span } = len.as_ref() {
            if let Some(decl) = types.consts.decls.get(name) {
                res.record_const_ref(*span, *decl);
            }
        }
        let n = array_length(types, len, errors).unwrap_or(1);
        let t = res.types_mut().array_of(elem, n);
        res.record_array_written(t, written.span);
        return t;
    }
    // `fn(T…) -> R ! {e}` (N50): its parts resolved as any type is, interned.
    if let Some(f) = &written.function {
        let params: Vec<Type> = f
            .params
            .iter()
            .map(|p| resolve_written_inner(types, res, p, scope, effect, errors))
            .collect();
        let ret = resolve_written_inner(types, res, &f.ret, scope, effect, errors);
        let effects = f
            .effects
            .as_ref()
            .and_then(|d| crate::effects::declared(d, effect.map(|e| e.0), errors))
            .unwrap_or(nazm_sema::EffectSet::EMPTY);
        return res.types_mut().fn_of(nazm_sema::FnSignature {
            params,
            ret,
            effects,
            row: effect.map(|e| e.1),
        });
    }
    // A qualified type names a module's definition (N49): never a type parameter, never `Vec`
    // or a built-in, which belong to no module — so those branches are for plain names only.
    let qualified = written.qualifier.is_some();
    if let Some((_, t)) = scope.iter().find(|(n, _)| !qualified && *n == written.name) {
        if args.is_some() {
            errors.push(not_generic(&written.name, "a type parameter", written.span));
        }
        // Which parameter this name is, recorded where it was decided (N23).
        if let Type::Param(p) = *t {
            res.record_written_type(written.name_span, WrittenType::Param(p));
        }
        return *t;
    }
    // `Chan[T]` (N53): `Chan` with an argument is a generic channel, for a `T` that may cross
    // into a task; `Chan` alone is the `Int` channel, below.
    if !qualified && written.name == "Chan" {
        if let Some(args) = args {
            if args.len() != 1 {
                errors.push(wrong_arity("Chan", 1, args.len(), written.span));
            }
            let elem = args.first().copied().unwrap_or(Type::Int);
            if let Some(d) = channel_element(res.types(), elem, written.span) {
                errors.push(d);
            }
            let chan = res.types_mut().chan_of(elem);
            res.record_written_type(written.name_span, WrittenType::Builtin(chan));
            return chan;
        }
    }
    // `Box[T]` and `Chain[T]` (R1a; `Chain` was `List` until R1.5, D7) and `Cell[T]` (R1b), and
    // `List[T]` (R1.5, D7), which is `Vec[T]`: prelude names, so a module's own type of the name
    // keeps it.
    if !qualified
        && matches!(written.name.as_str(), "Box" | "Chain" | "Cell" | "List")
        && !types.names.contains_key(written.name.as_str())
    {
        let Some(args) = args else {
            errors.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_ARGUMENTS,
                    written.name_span,
                    format!("`{}` is written with its element type", written.name),
                )
                .help(format!(
                    "a {} of `Int` is `{}[Int]`; `{}` alone is not a type",
                    match written.name.as_str() {
                        "Box" => "box",
                        "Cell" => "cell",
                        "Chain" => "chain",
                        _ => "list",
                    },
                    written.name,
                    written.name
                )),
            );
            return Type::Int;
        };
        if args.len() != 1 {
            errors.push(wrong_arity(&written.name, 1, args.len(), written.span));
        }
        let elem = args.first().copied().unwrap_or(Type::Int);
        let t = match written.name.as_str() {
            "Box" => res.types_mut().box_of(elem),
            "Cell" => res.types_mut().cell_of(elem),
            "Chain" => res.types_mut().list_of(elem),
            _ => res.types_mut().vec_of(elem),
        };
        res.record_written_type(written.name_span, WrittenType::Builtin(t));
        return t;
    }
    if !qualified && written.name == VEC {
        let Some(args) = args else {
            errors.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_ARGUMENTS,
                    written.name_span,
                    "`Vec` is written with its element type",
                )
                .help("a vector of `Int` is `Vec[Int]`; `Vec` alone is not a type"),
            );
            return Type::Int;
        };
        if args.len() != 1 {
            errors.push(wrong_arity("Vec", 1, args.len(), written.span));
        }
        let elem = args.first().copied().unwrap_or(Type::Int);
        let vec = res.types_mut().vec_of(elem);
        // The one generic built-in, as the vector type it was resolved to (N23).
        res.record_written_type(written.name_span, WrittenType::Builtin(vec));
        return vec;
    }
    let key = written_key(written.qualifier.as_ref(), &written.name);
    let t = resolve_name(types, res, &key, written.name_span, errors);
    // Which definition this name is, recorded where it was decided (N18). A built-in is
    // not one, and neither is a type parameter, which returned above.
    if let Some(node) = TypeNode::of(t) {
        res.record_type_name(written.name_span, node);
    }
    if !qualified && Type::from_name(&written.name).is_some() {
        if args.is_some() {
            errors.push(not_generic(&written.name, "a built-in type", written.span));
        }
        // A built-in, as the type resolution decided it is (N23).
        res.record_written_type(written.name_span, WrittenType::Builtin(t));
        return t;
    }
    let Some(node) = TypeNode::of(t) else {
        return t;
    };
    let arity = res.types().declared_params(node);
    match (args, arity) {
        (None, 0) => t,
        (None, n) => {
            let shown = res.types().show(t);
            errors.push(
                Diagnostic::error(
                    codes::MISSING_TYPE_ARGUMENTS,
                    written.name_span,
                    format!(
                        "`{shown}` is generic, so it is written with its {n} type argument{}",
                        if n == 1 { "" } else { "s" }
                    ),
                )
                .secondary(
                    user_type_span(res, t).unwrap_or(written.name_span),
                    "declared here",
                )
                .help(format!(
                    "write `{shown}[…]` with {n} type{}",
                    if n == 1 { "" } else { "s" }
                )),
            );
            Type::Int
        }
        (Some(_), 0) => {
            let shown = res.types().show(t);
            errors.push(not_generic(&shown, "not generic", written.span));
            t
        }
        (Some(mut args), n) => {
            if args.len() != n {
                errors.push(wrong_arity(
                    &res.types().show(t),
                    n,
                    args.len(),
                    written.span,
                ));
                args.resize(n, Type::Int);
            }
            apply(res, node, args)
        }
    }
}

/// A generic definition applied to its arguments, interned.
pub(super) fn apply(res: &mut Resolution, node: TypeNode, args: Vec<Type>) -> Type {
    match node {
        TypeNode::Record(id) => res.types_mut().apply_record(id, args),
        TypeNode::Enum(id) => res.types_mut().apply_enum(id, args),
    }
}

/// `N0351`: type arguments written on something that takes none.
pub(super) fn not_generic(name: &str, what: &str, at: Span) -> Diagnostic {
    let why = if what == "not generic" {
        format!("`{name}` is not generic, so it takes no type arguments")
    } else {
        format!("`{name}` is {what}, so it takes no type arguments")
    };
    Diagnostic::error(codes::TYPE_ARGUMENTS_ON_NON_GENERIC, at, why)
        .help("only a definition that declares type parameters is applied to type arguments")
}

/// `N0353`: the wrong number of type arguments.
pub(super) fn wrong_arity(name: &str, want: usize, got: usize, at: Span) -> Diagnostic {
    Diagnostic::error(
        codes::WRONG_TYPE_ARGUMENT_COUNT,
        at,
        format!(
            "`{name}` takes {want} type argument{}, but {got} {} given",
            if want == 1 { "" } else { "s" },
            if got == 1 { "was" } else { "were" }
        ),
    )
    .help("a generic definition is applied to exactly as many types as it declares")
}

impl Checker<'_> {
    /// The aliases this module imports modules under (N49), for a diagnostic.
    pub(super) fn alias_names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.env.aliases.keys().map(String::as_str).collect();
        out.sort_unstable();
        out
    }

    pub(super) fn resolve_callee(&mut self, name: &str, at: Span) -> Option<Signature> {
        let visible = self.env.get(name)?.clone();
        self.res.record_call(at, visible.callee);
        Some(visible.sig)
    }

    /// The record a completion probe's hole call names, recorded as a construction's head
    /// would be (N26).
    pub(super) fn called_record(&mut self, name: &str, at: Span, written: Option<&[TypeName]>) {
        if self.refused(name) {
            return;
        }
        let Some(def) = self.resolve_record(name, at) else {
            return;
        };
        if let Some(Type::Record(id)) = self.applied(Type::Record(def), written, at) {
            self.res.record_construction(at, id);
        }
    }

    /// Whether a completion probe must not anchor through the type name `name` (N26): the
    /// module's types refused it as an ambiguity or a collision, so it means nothing
    /// unambiguously here, whatever the table goes on to answer. Always `false` outside a
    /// probe, so nothing an ordinary compilation records depends on it.
    pub(super) fn refused(&self, name: &str) -> bool {
        self.res.probing() && self.types.refused.iter().any(|n| n == name)
    }

    /// Why this module cannot write `name`, when something in the compilation has it.
    ///
    /// Returns the diagnostic to raise instead of a plain "not defined". Both cases point
    /// at the definition, because in both the reader is looking at a file that has it.
    ///
    /// Reading every module's definitions is legitimate **here and nowhere else**: this
    /// runs only after resolution has already failed, and it decides what to say rather
    /// than what a name means. A resolver that consulted this would be the global
    /// namespace again, which is why it lives behind a name that says so.
    pub(super) fn why_invisible(&self, name: &str, at: Span) -> Option<Diagnostic> {
        // R1a's sequence built-ins, renamed with their type by R1.5 (D7): `List[T]` is `Vec[T]`
        // now, and the sequence is `Chain[T]`. Only after resolution failed, so a module's own
        // function of one of these names keeps it, as a prelude name's would.
        if let Some(new) = match name {
            "list_from_vec" => Some("chain_from_vec"),
            "list_empty" => Some("chain_empty"),
            "list_len" => Some("chain_len"),
            "list_get" => Some("chain_get"),
            _ => None,
        } {
            return Some(
                Diagnostic::error(
                    codes::BUILTIN_RENAMED,
                    at,
                    format!("`{name}` is now `{new}`"),
                )
                .help(format!(
                    "R1a's immutable sequence is `Chain[T]` and its built-ins `chain_`; write \
                     `{new}`. `List[T]` now names the growable list, `Vec[T]`"
                )),
            );
        }
        let mut elsewhere = None;
        for id in self.res.defined_anywhere(name) {
            let Some(def) = self.res.function(id) else {
                continue;
            };
            if self.imports.contains(&def.module) && !def.visibility.is_exported() {
                return Some(
                    Diagnostic::error(
                        codes::PRIVATE_DEFINITION,
                        at,
                        format!("`{name}` is private to the module that defines it"),
                    )
                    .secondary(def.name_span, "defined privately here")
                    .help(format!(
                        "write `pub fn {name}` there to export it, if it is meant to be \
                         part of that module's interface"
                    )),
                );
            }
            if !self.imports.contains(&def.module)
                && def.module != self.module
                && def.visibility.is_exported()
            {
                elsewhere = Some(def.name_span);
            }
        }
        // Exported by a module this one does not import. Almost always a missing `use`,
        // and almost always because the name used to be reachable through someone else's
        // import — which is exactly what stopped being true.
        elsewhere.map(|span| {
            Diagnostic::error(
                codes::UNDEFINED_FUNCTION,
                at,
                format!("`{name}` is not defined in this module"),
            )
            .secondary(span, "exported here, by a module this one does not import")
            .help("imports are not transitive: add a `use` for the module that exports it")
        })
    }

    /// How a type is spelled in a message.
    ///
    /// A `String` rather than a `Display` adapter because the table it reads is behind the
    /// same `&mut self` most of these sites already hold. A built-in spells itself; a
    /// record spells its declared name, which is not its identity — two modules may each
    /// declare `Thing`, and a message that says `Thing` is right about what was written.
    pub(super) fn show(&self, t: Type) -> String {
        self.res.types().show(t)
    }

    /// An effect set as source writes it, the effect parameter by its name (N51).
    pub(super) fn show_effects(&self, set: nazm_sema::EffectSet) -> String {
        let mut names: Vec<String> = set.named().names().into_iter().map(str::to_owned).collect();
        if set.has_param() {
            names.push(
                self.res
                    .types()
                    .effect_param_name(self.effect_param.as_ref().map(|e| e.1)),
            );
        }
        format!("{{ {} }}", names.join(", "))
    }

    /// The definition a name written as a construction's head or a pattern's enum refers
    /// to — the **definition**, not a type: `Maybe` in `Maybe.Some(value: x)` names the
    /// generic enum whose instance the scrutinee already is, and `Pair` in
    /// `Pair[Int, Str](…)` is applied by its arguments afterwards.
    /// The type `name` means here, asked without reporting or recording anything (R1.5, D4):
    /// what a construction's positional fields are named by, before it is checked.
    pub(super) fn quiet_type(&self, name: &str, span: Span) -> Type {
        if let Some((_, p)) = self.type_scope.iter().find(|(n, _)| n == name) {
            return *p;
        }
        let mut discarded = Vec::new();
        resolve_name(&self.types, self.res, name, span, &mut discarded)
    }

    fn resolve_type(&mut self, name: &str, span: Span) -> Type {
        // Split borrow: the environment and the error list are both behind `self`.
        let mut errors = std::mem::take(&mut self.errors);
        let t = if let Some((_, p)) = self.type_scope.iter().find(|(n, _)| n == name) {
            *p
        } else {
            let t = resolve_name(&self.types, self.res, name, span, &mut errors);
            // The head of a construction and the enum before a variant's `.`: which
            // definition the name is, recorded where it was decided (N18).
            if let Some(node) = TypeNode::of(t) {
                self.res.record_type_name(span, node);
            }
            t
        };
        self.errors = errors;
        t
    }

    /// A type in the current function's signature, resolved again to the same answer
    /// stage 1 reached — **silently**, because stage 1 already reported whatever was wrong
    /// with it, and one mistake is one error. Before N11 an unknown parameter type was
    /// reported twice, once here and once there.
    pub(super) fn signature_type(&mut self, written: &TypeName) -> Type {
        let mut discarded = Vec::new();
        let scope = std::mem::take(&mut self.type_scope);
        let effect = self.effect_param.clone();
        let t = resolve_written(
            &self.types,
            self.res,
            written,
            &scope,
            effect.as_ref().map(|(n, o)| (n.as_str(), *o)),
            &mut discarded,
        );
        self.type_scope = scope;
        t
    }

    /// A type written in full inside a body — a type argument — in the current function's
    /// parameter scope.
    pub(super) fn resolve_written_type(&mut self, written: &TypeName) -> Type {
        let mut errors = std::mem::take(&mut self.errors);
        let scope = std::mem::take(&mut self.type_scope);
        let effect = self.effect_param.clone();
        let t = resolve_written(
            &self.types,
            self.res,
            written,
            &scope,
            effect.as_ref().map(|(n, o)| (n.as_str(), *o)),
            &mut errors,
        );
        self.type_scope = scope;
        self.errors = errors;
        t
    }

    /// Resolve explicit type arguments written at a use of a generic definition, and apply
    /// the definition to them — `Pair[Int, Str]` from `Pair` and `[Int, Str]`.
    ///
    /// The rules every type position has: a generic definition is written with its
    /// arguments (`N0352`), a non-generic one without (`N0351`), and exactly as many as it
    /// declares (`N0353`). `at` is the name the arguments follow.
    ///
    /// `None` where the arguments were missing, so the caller checks what it can without a
    /// type to check it against rather than against one invented here.
    pub(super) fn applied(
        &mut self,
        def: Type,
        written: Option<&[TypeName]>,
        at: Span,
    ) -> Option<Type> {
        let args: Option<Vec<Type>> =
            written.map(|w| w.iter().map(|t| self.resolve_written_type(t)).collect());
        let Some(node) = TypeNode::of(def) else {
            return Some(def);
        };
        let arity = self.res.types().declared_params(node);
        match (args, arity) {
            (None, 0) => Some(def),
            (Some(_), 0) => {
                let shown = self.show(def);
                self.errors.push(not_generic(&shown, "not generic", at));
                Some(def)
            }
            (None, n) => {
                let shown = self.show(def);
                self.errors.push(
                    Diagnostic::error(
                        codes::MISSING_TYPE_ARGUMENTS,
                        at,
                        format!(
                            "`{shown}` is generic, so it is built with its {n} type argument{} \
                             written",
                            if n == 1 { "" } else { "s" }
                        ),
                    )
                    .help(format!(
                        "write them after the name: `{shown}[…]`, or use it where its type is \
                         known, or give it a payload that says (R1, 5b.14)"
                    )),
                );
                None
            }
            (Some(mut args), n) => {
                if args.len() != n {
                    let shown = self.show(def);
                    self.errors.push(wrong_arity(&shown, n, args.len(), at));
                    args.resize(n, Type::Int);
                }
                let t = apply(self.res, node, args);
                self.mention(t);
                Some(t)
            }
        }
    }

    /// Note that the body being checked mentions a type, for the native instance planner.
    /// Only a generic body records anything: a monomorphic one needs no substitution.
    pub(super) fn mention(&mut self, t: Type) {
        if let Some(id) = self.current {
            self.res.record_mention(id, t);
        }
    }

    /// Which record a type name written in an expression refers to, if it is one.
    ///
    /// Reports through [`Self::resolve_type`], so an unknown name is one diagnostic
    /// wherever it was written.
    pub(super) fn resolve_record(&mut self, name: &str, span: Span) -> Option<RecordId> {
        match self.resolve_type(name, span) {
            Type::Record(id) => Some(id),
            Type::Enum(id) => {
                // `E(x: 1)` where `E` is an enum. A real mistake with an exact fix, so it
                // says the fix rather than that `E` is not a record.
                let name = self.res.types().show(Type::Enum(id)).to_owned();
                let first = self
                    .res
                    .enums()
                    .get(id)
                    .and_then(|d| d.variants.first().map(|v| v.name.clone()));
                let mut d = Diagnostic::error(
                    codes::UNKNOWN_VARIANT,
                    span,
                    format!("`{name}` is an enum, so a value of it names one of its variants"),
                );
                if let Some(v) = first {
                    d = d.help(format!("write `{name}.{v}(…)`"));
                }
                self.errors.push(d);
                None
            }
            Type::Param(_) => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        span,
                        format!("`{name}` is a type parameter, so it has no fields to name"),
                    )
                    .help(
                        "nothing is known about a type parameter inside its definition, so \
                         there is no record to build",
                    ),
                );
                None
            }
            Type::Cap(k) => {
                // Authority is not constructible (N37): the only capabilities are the ones
                // the runtime hands `main`. Its own code, not "not a record", because the
                // question a reader has is where one comes from, not what fields it lacks.
                self.errors.push(forged(k, span));
                None
            }
            other => {
                // A known type that is not a user-defined one: `Int(x: 1)`. Distinguished
                // from an unknown name, which `resolve_type` has already reported.
                if Type::from_name(name).is_some() {
                    let shown = self.show(other);
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNKNOWN_FIELD,
                            span,
                            format!("`{shown}` is not a record, so it has no fields to name"),
                        )
                        .help(
                            "this form builds a record. A built-in type is built by its \
                             own operations",
                        ),
                    );
                }
                None
            }
        }
    }

    /// Which enum a type name written in an expression or a pattern refers to.
    pub(super) fn resolve_enum(&mut self, name: &str, span: Span) -> Option<EnumId> {
        match self.resolve_type(name, span) {
            Type::Enum(id) => Some(id),
            Type::Record(id) => {
                let shown = self.res.types().show(Type::Record(id)).to_owned();
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_VARIANT,
                        span,
                        format!("`{shown}` is a record, so it has no variants"),
                    )
                    .help("a record is built by naming its fields: `Point(x: 1, y: 2)`"),
                );
                None
            }
            Type::Param(_) => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_VARIANT,
                        span,
                        format!("`{name}` is a type parameter, so it has no variants"),
                    )
                    .help("only an `enum` has variants, and nothing is known about a parameter"),
                );
                None
            }
            other => {
                if Type::from_name(name).is_some() {
                    let shown = self.show(other);
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNKNOWN_VARIANT,
                            span,
                            format!("`{shown}` is a built-in type, so it has no variants"),
                        )
                        .help("only an `enum` has variants"),
                    );
                }
                None
            }
        }
    }
}
