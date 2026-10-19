//! v1, Q1-B-05: `exit_with`'s status is one contract in every implementation.
//!
//! `docs/user/spec.md`, *Ending*: the language's status is `0..=255`, and the host reports exactly that
//! value; anything outside fails with `N0416` and exit status 2. Before the contract, a status
//! outside the range ended with 1 in the interpreter, 2 natively and something else in the LIR
//! oracle — each host's own truncation. Every implementation is asked: the interpreter, LLVM at
//! `-O0` and `-O2`, Cranelift, and the LIR interpreter the native backends are held to; and from
//! inside a task, whose `exit_with` ends the process as `main`'s does.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// The program under the interpreter, the LIR oracle, and built and run by each backend.
fn everywhere(src: &str) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let mut out = Vec::new();
    for (label, args) in [
        ("interpreter", &["run", "--interpret", "prog.nz"][..]),
        ("lir oracle", &["lir", "--run", "prog.nz"][..]),
    ] {
        out.push((
            label.to_owned(),
            run_bounded(Command::new(NAZM).current_dir(d.path()).args(args), label),
        ));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d
            .path()
            .join(crate::common::exe_name(label.replace([' ', '-'], "")));
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "prog.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        out.push((
            label.to_owned(),
            run_bounded(&mut common::bounded_exe(&exe), label),
        ));
    }
    out
}

fn ending(status: &str) -> String {
    format!("fn main(io: IoCap) -> Int ! {{ io }} {{ print(\"x\\n\"); exit_with({status}) }}\n")
}

#[test]
fn a_status_in_range_reaches_the_host_unchanged_everywhere() {
    for status in [0, 1, 7, 2, 254, 255] {
        for (label, r) in everywhere(&ending(&status.to_string())) {
            assert_eq!(r.code, Some(status), "{label}: {r:?}");
            assert!(r.stdout.starts_with('x'), "{label}: {r:?}");
            assert!(!r.stderr.contains("N0416"), "{label}: {r:?}");
        }
    }
}

#[test]
fn a_status_outside_0_to_255_fails_with_n0416_everywhere() {
    for status in [
        "256",
        "0 - 1",
        "4294967298",
        "0 - 9223372036854775807",
        "512",
    ] {
        for (label, r) in everywhere(&ending(status)) {
            assert_eq!(r.code, Some(2), "{label} exit_with({status}): {r:?}");
            assert!(
                r.stderr.contains("N0416") && r.stderr.contains("outside 0 to 255"),
                "{label} exit_with({status}): {r:?}"
            );
        }
    }
}

#[test]
fn a_task_s_status_is_held_to_the_same_contract() {
    let task = |status: &str| {
        format!(
            "fn stop(io: IoCap, n: Int) -> Int ! {{ io }} {{ exit_with(n) }}\n\
             fn main(io: IoCap, t: SpawnCap) -> Int ! {{ io, spawn }} {{\n\
                 scope {{ spawn stop(io, {status}); }}\n\
                 0 }}\n"
        )
    };
    // The LIR oracle runs the sequential subset only, and refuses a task by name.
    let concurrent = |src: &str| {
        everywhere(src)
            .into_iter()
            .filter(|(label, _)| label != "lir oracle")
    };
    for (label, r) in concurrent(&task("9")) {
        assert_eq!(r.code, Some(9), "{label}: {r:?}");
    }
    for (label, r) in concurrent(&task("300")) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert!(r.stderr.contains("N0416"), "{label}: {r:?}");
    }
}
