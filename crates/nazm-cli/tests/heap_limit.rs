//! R1d, D5: `nazm build --heap-limit` and the manifest's `heap-limit` (`docs/spec.md`, *A bounded
//! heap*).
//!
//! What a whole run shows: a program well within its limit prints what the interpreter, the LIR
//! oracle and an unlimited build print, with the same memory report and nothing live, under LLVM
//! at `-O0` and `-O2` and under Cranelift; the same program past its limit ends with the fixed
//! `N0406` line and status 2 in every backend, keeping what it printed before; a program that
//! allocates many times its limit but holds little of it at once — tasks on the pool included —
//! runs, so a free is counted back; the manifest gives a limit and the flag overrides it; a limit
//! is refused where it cannot hold; and without one nothing in the generated program changes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use common::{run_bounded, Ran};
use differential::{memory_line, nazm, NAZM};

use std::path::Path;
use std::process::Command;

/// Holds 3000 strings of about 20 bytes, and their vector, at once: some 120 KiB with the
/// runtime's own allocations — past 64 KiB, well within 4 MiB.
const HOLDS: &str = "fn main(out: OutCap) -> Int ! {io} {
    print(\"start\\n\");
    let xs = strs_new();
    let mut i = 0;
    while i < 3000 {
        strs_push(xs, str_concat(\"the item numbered \", int_to_str(i)));
        i = i + 1;
    }
    print(f\"{strs_len(xs)} {strs_get(xs, 2999)}\\n\");
    0
}
";

/// Builds and allocates a hundred thousand strings, holding one at a time, in four tasks on the
/// pool five times over: millions of bytes allocated, a few KiB live.
const CHURNS: &str = "fn work(c: Chan, k: Int) -> Int {
    let mut total = 0;
    let mut i = 0;
    while i < 5000 {
        let s = str_concat(\"piece \", int_to_str(i + k));
        total = total + str_len(s);
        i = i + 1;
    }
    chan_send(c, total);
    0
}

fn main(t: SpawnCap) -> Int ! {spawn} {
    let c = chan_new(32);
    let mut round = 0;
    while round < 5 {
        scope { spawn work(c, 1); spawn work(c, 2); spawn work(c, 3); spawn work(c, 4); }
        round = round + 1;
    }
    let got = ints_new();
    let mut n = 0;
    while n < 20 {
        chan_recv(c, got);
        n = n + 1;
    }
    let mut sum = 0;
    let mut j = 0;
    while j < ints_len(got) {
        sum = sum + ints_get(got, j);
        j = j + 1;
    }
    sum
}
";

/// The three native builds of `prog.nz` in `dir`, each with `limit` when one is given.
fn builds(dir: &Path, limit: Option<&str>) -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.join(format!(
            "{}-{}",
            label.replace([' ', '-'], ""),
            limit.unwrap_or("none")
        ));
        let mut args = vec![
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--opt-level",
            opt,
            "prog.nz",
            "-o",
            exe.to_str().unwrap(),
        ];
        if let Some(l) = limit {
            args.extend(["--heap-limit", l]);
        }
        let b = nazm(dir, &args);
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        out.push((label.to_owned(), exe));
    }
    out
}

fn run(dir: &Path, exe: &Path) -> Ran {
    let mut c = common::bounded_exe(exe);
    c.current_dir(dir).env(nazm_lir::MEMORY_VARIABLE, "1");
    run_bounded(&mut c, "the program")
}

/// Every kind of storage the report counts was reclaimed.
fn nothing_live(report: &str) {
    for kind in ["sequences", "strings", "channels", "closures"] {
        let part = report.split(kind).nth(1).unwrap_or("");
        let live = part.split_whitespace().nth(2).unwrap_or("");
        assert_eq!(live, "live=0", "{kind} not all reclaimed: {report}");
    }
}

fn with(src: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    d
}

fn interpreted(dir: &Path) -> Ran {
    let mut c = Command::new(NAZM);
    c.current_dir(dir)
        .args(["run", "--interpret", "prog.nz"])
        .env(nazm_lir::MEMORY_VARIABLE, "1");
    run_bounded(&mut c, "nazm run")
}

#[test]
fn a_program_within_its_limit_runs_as_it_would_and_one_past_it_ends_with_n0406() {
    let d = with(HOLDS);
    let expected = "start\n3000 the item numbered 2999\n0";
    let reference = interpreted(d.path());
    assert_eq!(reference.code, Some(0), "{}", reference.stderr);
    assert_eq!(reference.stdout.trim_end(), expected);
    let report = memory_line(&reference);
    // The LIR oracle: the same answer from the instructions both backends translate.
    let oracle = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["lir", "--run", "prog.nz"]),
        "nazm lir --run",
    );
    assert_eq!(oracle.code, Some(0), "{}", oracle.stderr);
    assert_eq!(oracle.stdout.trim_end(), expected);
    // Within the limit: what the interpreter printed, the same storage, nothing live.
    for (label, exe) in builds(d.path(), Some("4M")) {
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        assert_eq!(memory_line(&r), report, "{label}");
        nothing_live(&report);
    }
    // Past it: what was printed stays printed, then the one fixed line and status 2, and no
    // memory report — the process ended where the allocation was asked for.
    for (label, exe) in builds(d.path(), Some("64K")) {
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, "start\n", "{label}");
        assert_eq!(
            r.stderr,
            "nazm: error[N0406]: out of memory: the heap limit of 65536 bytes is reached\n",
            "{label}"
        );
    }
}

#[test]
fn a_sequence_s_growth_is_counted() {
    // One `Ints` of 20,000 numbers: 160,000 bytes, grown in place by `realloc`, past 64 KiB and
    // within 4 MiB — each growth reserved before it is made.
    let d = with(
        "fn main() -> Int {
    let xs = ints_new();
    let mut i = 0;
    while i < 20000 {
        ints_push(xs, i);
        i = i + 1;
    }
    ints_get(xs, 19999)
}
",
    );
    for (label, exe) in builds(d.path(), Some("4M")) {
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), "19999", "{label}");
        nothing_live(&memory_line(&r));
    }
    for (label, exe) in builds(d.path(), Some("64K")) {
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert!(
            r.stderr.contains("the heap limit of 65536 bytes"),
            "{label}: {r:?}"
        );
    }
}

#[test]
fn what_is_freed_is_counted_back_in_every_task() {
    // Millions of bytes allocated through a limit of 64 KiB, a few held at once: every free,
    // on every pool worker, gives its bytes back to the count.
    let d = with(CHURNS);
    let reference = interpreted(d.path());
    assert_eq!(reference.code, Some(0), "{}", reference.stderr);
    let report = memory_line(&reference);
    for (label, exe) in builds(d.path(), Some("64K")) {
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(0), "{label}: {}", r.stderr);
        assert_eq!(r.stdout, reference.stdout, "{label}");
        assert_eq!(memory_line(&r), report, "{label}");
        nothing_live(&report);
    }
}

#[test]
fn a_canonical_path_is_found_within_a_limit() {
    // `realpath` once allocated its answer for the runtime to free; under a limit that free was
    // the counted heap's, given a block with no header, and the program ended with SIGTRAP.
    let d = with(
        "use std.file

fn main(io: IoCap) ! { io } {
    match file_canonical(io, \"sub/..\") {
        Ok(p) => print(\"found\\n\"),
        Err(e) => print(\"refused\\n\"),
    }
    match file_canonical(io, \"missing\") {
        Ok(p) => print(\"wrong\\n\"),
        Err(e) => print(\"missing\\n\"),
    }
}
",
    );
    std::fs::create_dir(d.path().join("sub")).unwrap();
    for (label, exe) in builds(d.path(), Some("2G")) {
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with("found\nmissing\n"), "{label}: {r:?}");
    }
}

#[test]
fn a_manifest_gives_the_limit_and_the_flag_overrides_it() {
    let d = tempfile::tempdir().unwrap();
    let manifest = |limit: &str| {
        format!(
            "schema = \"nazm.package/1\"\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n\
             main = \"main.nz\"\nheap-limit = \"{limit}\"\n"
        )
    };
    std::fs::create_dir_all(d.path().join("app/src")).unwrap();
    std::fs::write(d.path().join("app/src/main.nz"), HOLDS).unwrap();
    std::fs::write(d.path().join("app/nazm.toml"), manifest("64K")).unwrap();
    let exe = d.path().join("app-exe");
    let build = |extra: &[&str]| {
        let mut args = vec!["build", "--no-cache", "app", "-o", exe.to_str().unwrap()];
        args.extend(extra);
        nazm(d.path(), &args)
    };
    for backend in ["llvm", "cranelift"] {
        // The manifest's 64 KiB.
        let b = build(&["--backend", backend]);
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(2), "{backend}: {r:?}");
        assert!(r.stderr.contains("the heap limit of 65536 bytes"), "{r:?}");
        // The flag's 4 MiB in its place.
        let b = build(&["--backend", backend, "--heap-limit", "4M"]);
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let r = run(d.path(), &exe);
        assert_eq!(r.code, Some(0), "{backend}: {r:?}");
        assert_eq!(r.stdout.trim_end(), "start\n3000 the item numbered 2999\n0");
    }
    // A manifest's limit that is not a size is the manifest's error.
    std::fs::write(d.path().join("app/nazm.toml"), manifest("lots")).unwrap();
    let b = build(&[]);
    assert_ne!(b.code, Some(0), "{b:?}");
    assert!(
        b.stderr.contains("N0500") && b.stderr.contains("heap-limit"),
        "{b:?}"
    );
}

#[test]
fn a_limit_is_a_hosted_programs_and_a_size() {
    let d = with(HOLDS);
    let refused = |extra: &[&str]| {
        let mut args = vec!["build", "--no-cache", "prog.nz", "-o", "out"];
        args.extend(extra);
        nazm(d.path(), &args)
    };
    for size in ["1K", "12X", "2048G", ""] {
        let r = refused(&["--heap-limit", size]);
        assert_ne!(r.code, Some(0), "{size}: {r:?}");
        assert!(r.stderr.contains("--heap-limit"), "{size}: {r:?}");
    }
    for extra in [&["--target", "wasm32-unknown-unknown"][..], &["--lib"][..]] {
        let mut args = extra.to_vec();
        args.extend(["--heap-limit", "1M"]);
        let r = refused(&args);
        assert_eq!(r.code, Some(5), "{extra:?}: {r:?}");
        assert!(
            r.stderr
                .contains("a heap limit bounds a hosted program's heap"),
            "{extra:?}: {r:?}"
        );
    }
}

#[test]
fn without_a_limit_nothing_in_the_program_changes() {
    let d = with(HOLDS);
    let ir = |limit: Option<&str>| {
        let out = d.path().join(format!("ir-{}", limit.unwrap_or("none")));
        let mut args = vec![
            "build",
            "--no-cache",
            "--emit-ir",
            "prog.nz",
            "-o",
            out.to_str().unwrap(),
        ];
        if let Some(l) = limit {
            args.extend(["--heap-limit", l]);
        }
        let b = nazm(d.path(), &args);
        assert_eq!(b.code, Some(0), "{}", b.stderr);
        let stem = out.file_name().unwrap().to_string_lossy().into_owned();
        let mut units: Vec<(String, String)> = std::fs::read_dir(d.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&stem) && n.ends_with(".ll"))
            .map(|n| {
                let text = std::fs::read_to_string(d.path().join(&n)).unwrap();
                (n.trim_start_matches(&stem).to_owned(), text)
            })
            .collect();
        units.sort();
        units
    };
    let plain = ir(None);
    let counted = ir(Some("1M"));
    assert!(!plain.is_empty());
    // Without a limit: C's allocator, called directly, and no counted heap anywhere.
    assert!(
        plain.iter().all(|(_, t)| !t.contains("nz.heap_")),
        "{plain:?}"
    );
    assert!(plain.iter().any(|(_, t)| t.contains("call ptr @malloc(")));
    // With one: the same units and one more, the heap's, which alone calls C's allocator.
    assert_eq!(counted.len(), plain.len() + 1);
    for (name, text) in &counted {
        if name.contains("heap") {
            assert!(
                text.contains("@nz.heap_limit = internal global i64 1048576"),
                "{text}"
            );
        } else if name.contains("stack") {
            // A task's stack is not heap: the stack unit keeps C's allocator.
        } else {
            assert!(
                !text.contains("@malloc(")
                    && !text.contains("@free(")
                    && !text.contains("@realloc("),
                "{name} reaches C's allocator past the count"
            );
        }
    }
}
