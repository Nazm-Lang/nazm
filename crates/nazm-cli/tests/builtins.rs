//! Every built-in is named somewhere that runs it.
//!
//! `tests/selfhost.rs` already requires every built-in to have a signature in the Nazm
//! checker, and `tests/build.rs` requires the backend to lower it or refuse it by name.
//! Both are about the *tables* agreeing. Neither asks whether anything ever runs the
//! thing — and asking found `strs_pop`, which the compiler written in Nazm uses
//! throughout and which no test, conformance case or example had ever named.
//!
//! A built-in exercised only by the compiler's own source is exercised, but the evidence
//! is indirect: the bootstrap reaching a fixpoint says the built-in behaves the same way
//! twice, not that it behaves correctly. So the requirement is that each one appears in
//! a test, a conformance case or an example — places where a specific answer is
//! asserted.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// Everything that asserts an answer: test sources, the conformance corpus, examples.
fn everything_that_asserts() -> String {
    let root = repo_root();
    let mut out = String::new();
    for dir in [
        "crates/nazm-core/tests",
        "crates/nazm-cli/tests",
        "crates/nazm-syntax/tests",
        "compiler/conformance",
        "examples",
        "bench/programs",
    ] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            // Not this file: it is about to be searched for the very names it lists in
            // its own failure message, which would make the check pass on its own text.
            if p.file_name().is_some_and(|n| n == "builtins.rs") {
                continue;
            }
            if p.extension()
                .is_some_and(|e| e == "rs" || e == "nz" || e == "expected")
            {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push_str(&text);
                }
            }
        }
    }
    out
}

#[test]
fn every_builtin_is_exercised_where_an_answer_is_asserted() {
    let corpus = everything_that_asserts();
    let missing: Vec<&str> = nazm_core::Intrinsic::ALL
        .iter()
        .map(|i| i.name())
        .filter(|name| !corpus.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "these built-ins are never named in a test, a conformance case or an example: \
         {missing:?}. Being used by `compiler/*.nz` is not enough — the bootstrap \
         reaching a fixpoint says a built-in behaves the same way twice, not that it \
         behaves correctly."
    );
}
