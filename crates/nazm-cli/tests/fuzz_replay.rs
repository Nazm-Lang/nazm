//! N93: the coverage-guided fuzzer's persistent corpus, replayed by the ordinary suite.
//!
//! `docs/architecture.md` §7.94. `fuzz/corpus/TARGET/` holds the minimised inputs a contained
//! campaign kept — together they reach every edge it reached — and each regression a finding left.
//! Every one is run here through the target's entry point, exactly as the fuzzer runs it: none may
//! panic, and MIR lowered from any program the checker accepts must verify.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

fn inputs(target: &str) -> Vec<(PathBuf, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/corpus")
        .join(target);
    let mut out: Vec<(PathBuf, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .map(|p| {
            let t = String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned();
            (p, t)
        })
        .collect();
    out.sort();
    out
}

fn replay(target: &str, run: impl Fn(&str) + std::panic::RefUnwindSafe) {
    let all = inputs(target);
    assert!(!all.is_empty(), "no corpus for `{target}`");
    for (path, text) in all {
        let r = std::panic::catch_unwind(|| run(&text));
        assert!(r.is_ok(), "{} panics", path.display());
    }
}

#[test]
fn the_front_end_corpus_replays_without_a_panic() {
    replay("front", |s| {
        let _ = nazm_core::check_source(s);
    });
}

#[test]
fn the_lowering_corpus_replays_and_its_mir_verifies() {
    replay("lower", |s| {
        let (map, graph) = nazm_core::one_file(s);
        let g = &graph;
        let _ = nazm_core::on_lowered(&map, &graph, nazm_core::Entry::Absent, |core, res| {
            let mir = nazm_mir::lower(core, res, g)?;
            let problems = nazm_mir::verify(&mir);
            assert!(problems.is_empty(), "{problems:?}");
            Ok(())
        });
    });
}

#[test]
fn the_interpreter_corpus_replays_within_its_budgets() {
    replay("run", |s| {
        let limits = nazm_core::eval::Limits {
            max_iterations: 20_000,
            max_depth: 64,
        };
        let _ = nazm_core::run_with_limits(s, limits);
    });
}

#[test]
fn the_manifest_corpus_replays_without_a_panic() {
    replay("manifest", |s| {
        let _ = nazm_package::Manifest::parse(s, "nazm.toml");
        for line in s.lines() {
            let _ = nazm_package::VersionReq::parse(line.trim());
        }
    });
}
