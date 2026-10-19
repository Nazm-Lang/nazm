//! Representative tasks over this repository: what each context costs against an honest
//! naive baseline, and — first — that it holds what the task needs, checked against the
//! authorities themselves rather than against the planner. N33.
//!
//! Run with `--nocapture` to print the measurements `docs/internal/performance-log.md` records.
//!
//! Each naive baseline is what an agent without the planner reads for the same task: the
//! whole files the facts are in and the whole documents the sections are in, and — where a
//! task needs tests — the catalogue entries naming the file, as a search for its path would
//! return them. Byte counts, never tokens.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::cast_possible_truncation
)]

use std::time::Instant;

use nazm_repo::{Budget, Intent, TaskRequest};
use serde_json::Value;

mod scenarios;
use scenarios::{read, root, task};

#[test]
fn representative_tasks_hold_what_they_need_in_less_than_the_naive_context() {
    let rows = scenarios::all();
    println!("\n| Task | Naive bytes | Context JSON bytes | Reduction | Included source+text bytes | Entities | Checks | ms (this build) |");
    println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    for r in &rows {
        let reduction = 100.0 * (1.0 - r.json.len() as f64 / r.naive_bytes() as f64);
        let used = &r.context["used"];
        println!(
            "| {} | {} | {} | {reduction:.1}% | {} | {} | {} | {:.0} |",
            r.name,
            r.naive_bytes(),
            r.json.len(),
            used["source_bytes"].as_u64().unwrap() + used["doc_bytes"].as_u64().unwrap(),
            used["entities"],
            r.checks,
            r.ms
        );
    }
    // Every task but the one-function program is smaller than reading its files.
    for r in rows.iter().filter(|r| !r.name.starts_with('C')) {
        assert!(r.json.len() < r.naive_bytes(), "{}", r.name);
    }
}

#[test]
fn the_map_is_a_small_fraction_of_what_it_indexes() {
    let map = nazm_repo::map(&root()).to_json();
    let v: Value = serde_json::from_str(&map).unwrap();
    assert_eq!(v["status"], "available");
    let docs: usize = nazm_docs::CORPUS.iter().map(|d| read(d.path).len()).sum();
    let mut sources = 0;
    for s in nazm_repo::SOURCES {
        for e in std::fs::read_dir(root().join(s.dir)).unwrap().flatten() {
            let n = e.file_name().into_string().unwrap();
            if n.ends_with(".nz") && e.path().is_file() {
                sources += std::fs::read(e.path()).unwrap().len();
            }
        }
    }
    let mut metadata = read("Cargo.toml").len() + read("xtask/mutations/mutations.toml").len();
    for e in v["entities"].as_array().unwrap() {
        if e["kind"] == "package" || e["kind"] == "schema" {
            metadata += read(e["source"].as_str().unwrap()).len();
        }
    }
    let whole = docs + sources + metadata;
    println!(
        "\nmap {} bytes; canonical documents {docs}, indexed Nazm source {sources}, manifests, catalogue and schemas {metadata}: {whole}; {:.1}%",
        map.len(),
        100.0 * map.len() as f64 / whole as f64
    );
    assert!(map.len() * 10 < whole);
}

/// What a map and a task cost here: the first map of the process, repeats of it, its
/// serialisation apart, and tasks of each intent. Printed, not asserted: a build's speed is
/// the machine's. Run by hand, in a release build:
/// `cargo test --release -p nazm-repo --test benchmark -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, printed and not asserted"]
fn what_a_map_and_a_task_cost() {
    let time = |f: &mut dyn FnMut()| {
        let start = Instant::now();
        f();
        start.elapsed().as_secs_f64() * 1000.0
    };
    let mut map = None;
    let cold = time(&mut || map = Some(nazm_repo::map(&root())));
    let repeat: Vec<f64> = (0..3)
        .map(|_| time(&mut || map = Some(nazm_repo::map(&root()))))
        .collect();
    let map = map.unwrap();
    let mut json = String::new();
    let serialise = time(&mut || json = map.to_json());
    println!(
        "\nmap: first {cold:.0} ms, then {:.0} / {:.0} / {:.0} ms; serialising {serialise:.2} ms, {} bytes",
        repeat[0],
        repeat[1],
        repeat[2],
        json.len()
    );
    for (intent, seed) in [
        (Intent::Understand, "doc:spec:returns"),
        (Intent::Test, "package:nazm-docs"),
        (
            Intent::Diagnose,
            "diagnostic:examples-bad:return_type_mismatch.nz:N0300@return_type_mismatch.nz:59-69#0",
        ),
        (Intent::Understand, "def:compiler:lex.nz::fn is_alnum"),
        (Intent::Edit, "def:compiler:module.nz::fn attach_core"),
        (
            Intent::Understand,
            "def:compiler:analyse.nz::fn check_program",
        ),
    ] {
        let runs: Vec<f64> = (0..3).map(|_| task(intent, seed).2).collect();
        let (v, size, _) = task(intent, seed);
        let mut serialise = 0.0;
        let r = nazm_repo::task(
            &root(),
            &TaskRequest {
                intent: Some(intent),
                seeds: vec![seed.to_owned()],
                state: None,
                budget: Budget::default(),
            },
        );
        serialise += time(&mut || drop(r.to_json()));
        println!(
            "task {intent:?} {seed}: {:.0} / {:.0} / {:.0} ms, serialising {serialise:.2} ms, {size} bytes, {} items",
            runs[0],
            runs[1],
            runs[2],
            v["items"].as_array().unwrap().len()
        );
    }
}
