//! N36 over the repository's own programs: every compilation the tree has still checks, and
//! what the checker infers of them. The census is a measurement, run by hand
//! (`--ignored --nocapture`); the acceptance is not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_core::sema::EffectSet;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for e in entries {
        if e.is_dir() {
            out.extend(walk(&e));
        } else if e.extension().is_some_and(|x| x == "nz") {
            out.push(e);
        }
    }
    out
}

/// Every root the tree compiles: the compiler's, each multi-module conformance program's,
/// each example and each benchmark program.
fn roots() -> Vec<PathBuf> {
    let root = workspace();
    let mut out = vec![
        root.join("compiler/emit.nz"),
        root.join("compiler/check.nz"),
        root.join("compiler/lexer.nz"),
        root.join("compiler/parser.nz"),
    ];
    for e in walk(&root.join("compiler/conformance/modules")) {
        if e.file_name().is_some_and(|n| n == "main.nz") {
            out.push(e);
        }
    }
    for dir in ["examples", "bench/programs"] {
        for e in walk(&root.join(dir)) {
            if !e.components().any(|c| c.as_os_str() == "bad") {
                out.push(e);
            }
        }
    }
    out
}

struct Census {
    functions: usize,
    by_set: BTreeMap<String, usize>,
    declared: usize,
    effect_diagnostics: usize,
    micros: u128,
}

fn census(path: &Path) -> Option<Census> {
    let decided = nazm_service::root::decide(path, None).ok()?;
    let loaded = nazm_service::load::load(path, decided.path()).ok()?;
    let start = std::time::Instant::now();
    let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
    let micros = start.elapsed().as_micros();
    let prelude = loaded.graph.prelude();
    let mut by_set = BTreeMap::new();
    let mut functions = 0;
    let mut declared = 0;
    for f in a.resolution.functions() {
        if Some(f.module) == prelude {
            continue;
        }
        functions += 1;
        declared += usize::from(f.declared_effects.is_some());
        let set = f
            .inferred_effects
            .map_or_else(|| "unchecked".to_owned(), |s: EffectSet| s.to_string());
        *by_set.entry(set).or_insert(0) += 1;
    }
    let effect_diagnostics = a
        .diagnostics
        .iter()
        .filter(|d| ["N0366", "N0367", "N0368"].contains(&d.code.as_str()))
        .count();
    Some(Census {
        functions,
        by_set,
        declared,
        effect_diagnostics,
        micros,
    })
}

fn on_the_service_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn")
            .join()
            .expect("join")
    })
}

#[test]
fn no_program_the_tree_has_is_refused_for_its_effects_and_undeclared_ones_are_inferred() {
    on_the_service_stack(|| {
        let mut checked = 0;
        for path in roots() {
            let Some(c) = census(&path) else { continue };
            checked += 1;
            // Nothing that existed before N36 declares a set, so nothing may be refused for
            // one; `examples/effects/` declares sets that hold.
            assert_eq!(c.effect_diagnostics, 0, "{}", path.display());
            assert!(c.by_set.get("unchecked").is_none_or(|n| *n <= c.functions));
        }
        assert!(checked >= 15, "only {checked} roots");
    });
}

#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn census_of_the_effects_the_checker_infers_across_the_tree() {
    on_the_service_stack(|| {
        let root = workspace();
        let mut total: BTreeMap<String, usize> = BTreeMap::new();
        let (mut functions, mut declared) = (0, 0);
        for path in roots() {
            let Some(c) = census(&path) else { continue };
            let rel = path.strip_prefix(&root).unwrap().display().to_string();
            println!(
                "{rel}: {} functions, {} declared, {:?}, analyse {} ms",
                c.functions,
                c.declared,
                c.by_set,
                c.micros as f64 / 1000.0
            );
            functions += c.functions;
            declared += c.declared;
            for (k, v) in c.by_set {
                *total.entry(k).or_insert(0) += v;
            }
        }
        println!("ALL: {functions} function checks, {declared} declared, {total:?}");
    });
}
