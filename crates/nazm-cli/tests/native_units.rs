//! Native compilation across module boundaries.
//!
//! # Why a file of its own
//!
//! `tests/build.rs` compiles one file at a time, which is the right shape for testing
//! arithmetic and refusals and the wrong one for testing what happens *between* modules.
//! Everything here has at least two, and the questions are the ones a second module
//! creates: whose symbol is this, whose file is that position in, and what does the
//! linker see.
//!
//! # Two oracles, as everywhere else
//!
//! Where a program produces a value or a failure, the interpreter is asked the same
//! question and the two must agree. A backend that disagreed with the interpreter about
//! which file a division by zero is in would be reporting about a program nobody wrote.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(&path, text).expect("write");
}

/// What running something produced, with the paths it printed left exactly as they were.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn ran(out: &std::process::Output) -> Ran {
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    }
}

/// Build the first file of `files` and run the executable, from inside the project
/// directory so that the paths a program prints are the relative ones the user typed.
fn build_and_run(dir: &Path, files: &[(&str, &str)]) -> Ran {
    for (name, text) in files {
        write(dir, name, text);
    }
    let built = Command::new(NAZM)
        .args(["build", files[0].0, "-o", "prog"])
        .current_dir(dir)
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    ran(&Command::new(dir.join("prog"))
        .current_dir(dir)
        .output()
        .expect("execute"))
}

fn interpret(dir: &Path, files: &[(&str, &str)]) -> Ran {
    for (name, text) in files {
        write(dir, name, text);
    }
    ran(&Command::new(NAZM)
        .args(["run", files[0].0])
        .current_dir(dir)
        .output()
        .expect("run"))
}

#[test]
fn a_runtime_failure_names_the_module_it_happened_in() {
    // Until 2026-09-22 the emitted message carried the file `nazm build` was invoked on,
    // for every failure in every module. The line and column were already right — they
    // come from the `SourceMap` — so this printed `main.nz` with `lib.nz`'s position: a
    // location that points somewhere real and wrong, which is worse than one that points
    // nowhere.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[
        ("main.nz", "use \"lib.nz\";\nfn main() -> Int { boom(1) }\n"),
        (
            "lib.nz",
            "fn zero() -> Int { 0 }\npub fn boom(n: Int) -> Int { n / zero() }\n",
        ),
    ];

    let native = build_and_run(dir.path(), files);
    assert_eq!(native.code, Some(2));
    assert_eq!(
        native.stderr,
        "nazm: error[N0401]: division by zero at lib.nz:2:32"
    );

    // And the interpreter agrees about which file and where in it. The rendering differs
    // — a compiled program has no diagnostic framework linked into it — but the location
    // is the same claim, and the two must not make different ones.
    let interpreted = interpret(dir.path(), files);
    assert_eq!(interpreted.code, Some(2));
    assert!(
        interpreted.stderr.contains("--> lib.nz:2:32"),
        "{}",
        interpreted.stderr
    );
}

#[test]
fn the_root_module_still_names_itself() {
    // The other half, and the reason the fix is a change of *source* rather than of
    // behaviour: a single-file program reports exactly what it always did.
    let dir = tempfile::tempdir().expect("tempdir");
    let files = &[("one.nz", "fn main() -> Int { 9 / 0 }\n")];

    let native = build_and_run(dir.path(), files);
    assert_eq!(native.code, Some(2));
    assert_eq!(
        native.stderr,
        "nazm: error[N0401]: division by zero at one.nz:1:22"
    );
}
