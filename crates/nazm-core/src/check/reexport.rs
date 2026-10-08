//! Re-exports: a module's `pub use` of what it imports, applied to its interface.

use super::*;

/// Apply every `pub use` (N103): each module's interface gains the definitions it
/// re-exports, under their own names or the names it gives them, each the **same**
/// definition — its id, and so its identity, is the defining module's. With `full`, the
/// functions and impls too, and every refusal reported; without, types and traits only,
/// silently, for the type interfaces the importers' types resolve against.
///
/// A module re-exporting from one that re-exports is applied after it; a cycle of re-exports
/// has no order and is refused (`N0213`). Returns the modules a refusal was reported for.
pub(super) fn apply_reexports(
    units: &[Unit<'_>],
    ifaces: &mut [UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
    full: bool,
) -> Vec<ModuleId> {
    let mut unclean = Vec::new();
    let mut done: Vec<bool> = units
        .iter()
        .map(|u| u.imports.iter().all(|d| d.reexport.is_none()))
        .collect();
    let finished = |done: &[bool], m: ModuleId| {
        units
            .iter()
            .position(|u| u.module == m)
            .is_none_or(|i| done.get(i).copied().unwrap_or(true))
    };
    let mut progressed = true;
    while progressed {
        progressed = false;
        for (n, u) in units.iter().enumerate() {
            if done.get(n).copied().unwrap_or(true) {
                continue;
            }
            if !u
                .imports
                .iter()
                .filter(|d| d.reexport.is_some())
                .all(|d| finished(&done, d.on))
            {
                continue;
            }
            for d in u.imports.iter() {
                let Some(spec) = &d.reexport else { continue };
                let before = errors.len();
                reexport_one(u.module, d.on, spec, ifaces, res, errors, full);
                if errors.len() != before {
                    unclean.push(u.module);
                }
            }
            if let Some(slot) = done.get_mut(n) {
                *slot = true;
            }
            progressed = true;
        }
    }
    if full {
        for (n, u) in units.iter().enumerate() {
            if done.get(n).copied().unwrap_or(true) {
                continue;
            }
            for d in u.imports.iter() {
                if let Some(spec) = &d.reexport {
                    errors.push(
                        Diagnostic::error(
                            codes::REEXPORT_CYCLE,
                            spec.at,
                            "this re-export is part of a cycle of re-exports",
                        )
                        .help(
                            "a re-export offers what another module offers, so a cycle of \
                             them offers nothing in particular: re-export from the module \
                             that defines the name",
                        ),
                    );
                    unclean.push(u.module);
                }
            }
        }
    }
    unclean
}

/// One `pub use`: what module `from` offers, added to `module`'s interface.
fn reexport_one(
    module: ModuleId,
    from: ModuleId,
    spec: &nazm_sema::ReexportSpec,
    ifaces: &mut [UnitInterface],
    res: &mut Resolution,
    errors: &mut Vec<Diagnostic>,
    full: bool,
) {
    let Some(src) = ifaces.get(from.index()).cloned() else {
        return;
    };
    res.traits_mut().record_reexport_source(module, from);
    // What `from` offers, by the name an importer writes.
    let mut types: Vec<(String, Type)> = Vec::new();
    for r in src.records().iter().filter(|r| r.nameable) {
        if let Some(d) = res.records().get(r.def) {
            types.push((d.name.clone(), Type::Record(r.def)));
        }
    }
    for e in src.enums().iter().filter(|e| e.nameable) {
        if let Some(d) = res.enums().get(e.def) {
            types.push((d.name.clone(), Type::Enum(e.def)));
        }
    }
    types.extend(src.renamed().iter().cloned());
    let traits: Vec<(String, TraitId)> = res
        .traits()
        .iter()
        .filter(|(_, t)| t.module == from && t.visibility.is_exported())
        .map(|(id, t)| (t.name.clone(), id))
        .chain(
            res.traits()
                .reexported_by(from)
                .filter_map(|id| res.traits().get(id).map(|t| (t.name.clone(), id)))
                .collect::<Vec<_>>(),
        )
        .collect();
    let functions: Vec<Export> = src.exports().to_vec();

    // The chosen names, or every one, with the name each is offered under.
    struct Chosen {
        name: String,
        offered: String,
        at: Span,
        renamed: bool,
    }
    let chosen: Vec<Chosen> = match &spec.names {
        Some(list) => list
            .iter()
            .map(|i| Chosen {
                name: i.name.clone(),
                offered: i.offered().to_owned(),
                at: i.span,
                renamed: i.alias.is_some(),
            })
            .collect(),
        None => functions
            .iter()
            .map(|e| e.name.clone())
            .chain(types.iter().map(|(n, _)| n.clone()))
            .chain(traits.iter().map(|(n, _)| n.clone()))
            .map(|name| Chosen {
                offered: name.clone(),
                name,
                at: spec.at,
                renamed: false,
            })
            .collect(),
    };

    for c in chosen {
        let function = functions.iter().find(|e| e.name == c.name);
        let ty = types.iter().find(|(n, _)| *n == c.name).map(|(_, t)| *t);
        let tr = traits.iter().find(|(n, _)| *n == c.name).map(|(_, t)| *t);
        let Some(here) = ifaces.get(module.index()) else {
            return;
        };
        // A name this module already offers, its own or another re-export's.
        let taken_fn = here.exports().iter().any(|e| e.name == c.offered);
        let taken_ty = here.user_type(res.types(), &c.offered).is_some()
            || res
                .traits()
                .iter()
                .any(|(_, t)| t.module == module && t.name == c.offered)
            || res
                .traits()
                .reexported_by(module)
                .any(|id| res.traits().get(id).is_some_and(|t| t.name == c.offered));
        if function.is_none() && ty.is_none() && tr.is_none() {
            if full {
                errors.push(
                    Diagnostic::error(
                        codes::REEXPORT_UNKNOWN,
                        c.at,
                        format!(
                            "`{}` is not exported by the module this re-exports from",
                            c.name
                        ),
                    )
                    .help("a module re-exports only what the module it names exports"),
                );
            }
            continue;
        }
        if let Some(e) = function {
            if full {
                if taken_fn {
                    errors.push(taken(&c.offered, c.at));
                    continue;
                }
                let mut offered = e.clone();
                offered.name.clone_from(&c.offered);
                // Its identity is where it is defined, however many modules pass it on.
                if offered.origin.is_none() {
                    offered.origin = Some((from, e.name.clone()));
                }
                if let Some(i) = ifaces.get_mut(module.index()) {
                    i.add_reexport(offered);
                }
                if c.at != spec.at {
                    res.record_call(c.at, Callee::Function(e.def));
                }
            }
        }
        if let Some(t) = ty {
            if taken_ty && full && function.is_none() {
                errors.push(taken(&c.offered, c.at));
                continue;
            }
            if !taken_ty {
                if let Some(i) = ifaces.get_mut(module.index()) {
                    i.add_reexported_type(t, c.renamed.then(|| c.offered.clone()));
                }
                if c.at != spec.at {
                    match t {
                        Type::Record(id) => res.record_type_name(c.at, TypeNode::Record(id)),
                        Type::Enum(id) => res.record_type_name(c.at, TypeNode::Enum(id)),
                        _ => {}
                    }
                }
            }
        }
        if let Some(t) = tr {
            if c.renamed {
                if full {
                    errors.push(
                        Diagnostic::error(
                            codes::REEXPORT_UNKNOWN,
                            c.at,
                            format!("`{}` is a trait, which is re-exported under its own name", c.name),
                        )
                        .help("a trait's name is how an importer's bounds and calls find it: drop `as`"),
                    );
                }
                continue;
            }
            // Recorded at the type stage already: the full stage only reports.
            let again = res.traits().reexported_by(module).any(|x| x == t);
            if taken_ty && !again && full && function.is_none() && ty.is_none() {
                errors.push(taken(&c.offered, c.at));
                continue;
            }
            res.traits_mut().record_reexport(module, t);
            if c.at != spec.at {
                res.traits_mut().record_mention(c.at, t);
            }
        }
    }
    if full {
        if let Some(i) = ifaces.get_mut(module.index()) {
            i.add_reexported_traits(src.traits(), src.impls());
        }
    }
}

/// `N0212`: a name this module offers twice.
fn taken(name: &str, at: Span) -> Diagnostic {
    Diagnostic::error(
        codes::REEXPORT_COLLIDES,
        at,
        format!("`{name}` is already offered by this module"),
    )
    .help(
        "a module offers each name once: re-export it under another name with `as`, or \
         rename the definition",
    )
}
