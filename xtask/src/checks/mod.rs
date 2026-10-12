//! `cargo xtask check`: runs every architectural gate and reports each, plus the helpers the
//! gates share for finding the repository and its source files.

pub(crate) mod architecture;
pub(crate) mod cache;
pub(crate) mod docs;
pub(crate) mod modules;
pub(crate) mod mutations;
pub(crate) mod reach;
pub(crate) mod runtime_parity;

use std::process::ExitCode;

use crate::checks::architecture::{
    check_core_ir_boundary, check_sema_purity, check_single_resolver, check_span_identity,
};
use crate::checks::cache::{check_cache_soundness, check_native_identity, check_object_cache};
use crate::checks::docs::{check_capability_matrix, check_doc_claims, check_docs_corpus};
use crate::checks::modules::check_module_boundaries;
use crate::checks::mutations::check_mutations_apply;
use crate::checks::reach::{
    check_codegen_reach, check_emission_reach, check_layering, check_network_reach,
    check_tokenizer_reach, check_unsafe_policy,
};
use crate::checks::runtime_parity::check_runtime_parity;
use crate::{claims, governance, guide, supply};

pub(crate) fn run_checks() -> ExitCode {
    report(&[
        ("layering", check_layering()),
        ("tokenizer reach", check_tokenizer_reach()),
        ("network reach", check_network_reach()),
        ("codegen reach", check_codegen_reach()),
        ("emission reach", check_emission_reach()),
        ("unsafe policy", check_unsafe_policy()),
        ("doc claims", check_doc_claims()),
        ("capability matrix", check_capability_matrix()),
        ("one resolver", check_single_resolver()),
        ("core ir boundary", check_core_ir_boundary()),
        ("span identity", check_span_identity()),
        ("sema purity", check_sema_purity()),
        ("cache soundness", check_cache_soundness()),
        ("native identity", check_native_identity()),
        ("object cache", check_object_cache()),
        ("runtime parity", check_runtime_parity()),
        ("mutations apply", check_mutations_apply()),
        ("guide from grammar", guide::check()),
        ("docs corpus", check_docs_corpus()),
        ("performance claims", claims::check(&guide::root())),
        ("agent governance", governance::check(&guide::root())),
        ("dependency sources", supply::check(&guide::root())),
        ("third-party notices", crate::notices::check(&guide::root())),
        ("module boundaries", check_module_boundaries(&guide::root())),
    ])
}

pub(crate) fn report(results: &[(&str, Result<(), Vec<String>>)]) -> ExitCode {
    let mut failed = false;
    for (name, result) in results {
        match result {
            Ok(()) => println!("ok    {name}"),
            Err(problems) => {
                failed = true;
                println!("FAIL  {name}");
                for p in problems {
                    println!("        {p}");
                }
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Every `.rs` under a directory. Tests live outside `src/`, so assertions about IR
/// content in `tests/` are not mistaken for the capability itself.
pub(crate) fn collect_rs(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// The workspace root: this crate's manifest directory, one level up.
pub(crate) fn workspace_root() -> Option<std::path::PathBuf> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::Path::to_path_buf)
}

/// The files of the Rust module at `base` (repository-relative, without `.rs`): `base.rs` when
/// it is one file, else every `.rs` file under the directory `base/`, sorted. Empty when it is
/// neither. A gate that names a module by path reads it through this, so that splitting the
/// file into a directory never narrows what the gate inspects.
pub(crate) fn module_files(root: &std::path::Path, base: &str) -> Vec<String> {
    let file = format!("{base}.rs");
    if root.join(&file).is_file() {
        return vec![file];
    }
    let Ok(found) = collect_rs(&root.join(base)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = found
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    out.sort();
    out
}

/// Every `.rs` file under `crates/`, as a repository-relative path.
///
/// `xtask/` is not scanned, and does not need to be: `rules.rs` gives it no internal
/// dependencies at all, so nothing in it can reach the compiler's types. This file names
/// the method it is checking for, which would otherwise be its own first finding.
pub(crate) fn source_files(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("crates")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_module_is_its_file_or_every_file_of_its_directory() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("a/split/deeper")).unwrap();
        std::fs::write(root.join("a/one.rs"), "").unwrap();
        for f in [
            "a/split/mod.rs",
            "a/split/part.rs",
            "a/split/deeper/leaf.rs",
        ] {
            std::fs::write(root.join(f), "").unwrap();
        }
        std::fs::write(root.join("a/split/notes.md"), "").unwrap();
        assert_eq!(super::module_files(root, "a/one"), ["a/one.rs"]);
        assert_eq!(
            super::module_files(root, "a/split"),
            [
                "a/split/deeper/leaf.rs",
                "a/split/mod.rs",
                "a/split/part.rs"
            ]
        );
        assert!(super::module_files(root, "a/absent").is_empty());
    }
}
