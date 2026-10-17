//! `nazm resolve` and `nazm references` (N77, `docs/architecture.md` §7.78): a compilation's
//! resolved units, and every occurrence of one entity read from them.
//!
//! Both run the cached check `nazm check` runs — the same store, the same keys — and read the
//! units it leaves: a reused module contributes the unit its entry kept, and its bodies are
//! not checked again. Nothing here resolves a name; every target is the checker's.

use std::path::Path;
use std::process::ExitCode;

use nazm_iface::ResolvedUnit;
use serde_json::json;

use crate::checking::Cache;
use crate::{exit, load, root};

/// The units of the compilation rooted at `file`, and how many modules were checked and
/// reused — or the exit code of a compilation that did not check.
fn units(
    file: &Path,
    declared: Option<&Path>,
    json: bool,
) -> Result<(Vec<ResolvedUnit>, usize, usize), ExitCode> {
    let source_root = root::decide(file, declared).map_err(|e| {
        eprintln!("nazm: {e}");
        ExitCode::from(exit::root_status(&e))
    })?;
    let loaded = match load::load(file, source_root.path()) {
        Ok(l) => l,
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, json);
            return Err(ExitCode::from(exit::load_status(&diagnostics)));
        }
    };
    let map = &loaded.map;
    let mut cache = Cache::with_units(map, crate::derived::checks(source_root.path()));
    let diagnostics = nazm_core::check_map_reusing(map, &loaded.graph, &mut cache);
    if !diagnostics.is_empty() {
        crate::emit(&diagnostics, map, json);
        return Err(ExitCode::from(exit::CHECK_FAILED));
    }
    let (all, checked, reused) = cache.units();
    Ok((all.into_iter().flatten().collect(), checked, reused))
}

/// `nazm resolve FILE`: the compilation's units, one `nazm.resolved/1` per line, in module
/// order — the bytes a check entry stores.
pub(crate) fn resolve(file: &Path, declared: Option<&Path>) -> ExitCode {
    let (units, _, _) = match units(file, declared, true) {
        Ok(u) => u,
        Err(code) => return code,
    };
    for u in &units {
        match u.to_json() {
            Ok(j) => println!("{j}"),
            Err(e) => {
                eprintln!("nazm: {e}");
                return ExitCode::from(exit::TOOLCHAIN_FAILED);
            }
        }
    }
    ExitCode::from(exit::OK)
}

/// `nazm references DEF FILE`: every occurrence of the entity `DEF` — a durable key
/// `module::kind name`, optionally `.member` — in the compilation, as one
/// `nazm.references/1` object.
pub(crate) fn references(def: &str, file: &Path, declared: Option<&Path>) -> ExitCode {
    let (item, member) = match def
        .split_once(" ")
        .and_then(|(_, rest)| rest.split_once('.'))
    {
        Some((_, m)) => (
            def.get(..def.len() - m.len() - 1).unwrap_or(def).to_owned(),
            Some(m.to_owned()),
        ),
        None => (def.to_owned(), None),
    };
    let (units, checked, reused) = match units(file, declared, true) {
        Ok(u) => u,
        Err(code) => return code,
    };
    let is_it = |t: &nazm_iface::resolved::DurableTarget| t.item == item && t.member == member;
    let mut declaration = None;
    let mut uses = Vec::new();
    let mut kind = None;
    for u in &units {
        for d in u.definitions.iter().filter(|d| is_it(&d.target)) {
            kind = Some(d.target.kind.clone());
            declaration = Some(json!({"module": u.module, "start": d.start, "end": d.end}));
        }
        for o in u.occurrences.iter().filter(|o| is_it(&o.target)) {
            kind = Some(o.target.kind.clone());
            uses.push(json!({"module": u.module, "start": o.start, "end": o.end}));
        }
    }
    let Some(kind) = kind else {
        eprintln!("nazm: `{def}` names nothing this compilation declares or uses");
        return ExitCode::from(exit::CHECK_FAILED);
    };
    println!(
        "{}",
        json!({
            "schema": "nazm.references/1",
            "item": item,
            "member": member,
            "kind": kind,
            "declaration": declaration,
            "uses": uses,
            "checked": checked,
            "reused": reused,
        })
    );
    ExitCode::from(exit::OK)
}
