//! The speed milestone: a program of one thread, known from `main`'s authority.
//!
//! A program whose `main` holds no `SpawnCap`, `NetCap` or `ForeignCap`, and which is not a
//! library, cannot start a thread nor be called from one C started — so its failure state and
//! stack limit are plain globals, not thread-local ones (on macOS every thread-local read is a
//! call). Every other program keeps them thread-local. The stack check is inlined either way. A
//! failure and a stack overflow are reported in a program of one thread exactly as before, under
//! LLVM and Cranelift.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const DOWN: &str = "fn down(n: Int) -> Int { if n == 0 { 0 } else { 1 + down(n - 1) } }\n";

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// The IR a build of `main_src` writes, every unit concatenated.
fn ir(main_src: &str, library: bool) -> String {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), format!("{DOWN}{main_src}")).unwrap();
    let mut args = vec!["build", "--no-cache", "--emit-ir", "p.nz", "-o", "p"];
    if library {
        args.push("--lib");
    }
    let b = nazm(d.path(), &args);
    assert_eq!(b.code, Some(0), "{main_src}: {b:?}");
    let mut text = String::new();
    for e in std::fs::read_dir(d.path()).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "ll") {
            text.push_str(&std::fs::read_to_string(p).unwrap());
        }
    }
    text
}

/// How many of the failure state's and stack limit's declarations and definitions are
/// thread-local.
fn thread_local_state(ir: &str) -> usize {
    ir.lines()
        .filter(|l| {
            [
                "@nz.failed = ",
                "@nz.errmsg = ",
                "@nz.errlen = ",
                "@nz.stack_limit = ",
            ]
            .iter()
            .any(|g| l.starts_with(g))
                && l.contains("thread_local")
        })
        .count()
}

#[test]
fn main_s_authority_decides_whether_the_state_is_thread_local() {
    let one = ir(
        "fn main(out: OutCap) -> Int ! {io} { print(\"x\\n\"); down(3) }\n",
        false,
    );
    assert_eq!(thread_local_state(&one), 0, "{one}");
    // The check is inlined: the runtime is called only on its slow path.
    assert!(one.contains("sc.slow."), "{one}");
    for (main, why) in [
        (
            "fn main(t: SpawnCap) -> Int ! {spawn} { scope { spawn down(3); } 0 }\n",
            "a task",
        ),
        ("fn main(n: NetCap) -> Int { down(3) }\n", "the reactor"),
        ("fn main(f: ForeignCap) -> Int { down(3) }\n", "C"),
    ] {
        let many = ir(main, false);
        assert!(thread_local_state(&many) > 0, "{why}: {many}");
    }
    let library = ir(
        "pub extern \"C\" fn twice(n: Int) -> Int ! {} = \"twice\" { down(n) * 2 }\n",
        true,
    );
    assert!(thread_local_state(&library) > 0, "a library: {library}");
}

#[test]
fn a_program_of_one_thread_still_reports_its_failures() {
    let d = tempfile::tempdir().unwrap();
    for backend in ["llvm", "cranelift"] {
        for (main, code) in [
            ("fn main() -> Int { down(100000000) }\n", "N0408"),
            (
                "fn main() -> Int { down(3) + 9223372036854775807 }\n",
                "N0400",
            ),
        ] {
            std::fs::write(d.path().join("p.nz"), format!("{DOWN}{main}")).unwrap();
            let exe = d.path().join(format!("p-{backend}"));
            let b = nazm(
                d.path(),
                &[
                    "build",
                    "--no-cache",
                    "--backend",
                    backend,
                    "p.nz",
                    "-o",
                    exe.to_str().unwrap(),
                ],
            );
            assert_eq!(b.code, Some(0), "{backend}: {b:?}");
            let r = run_bounded(&mut common::bounded_exe(&exe), backend);
            assert_eq!(r.code, Some(2), "{backend} {main}: {r:?}");
            assert!(
                r.stderr.contains(&format!("error[{code}]")),
                "{backend}: {r:?}"
            );
        }
    }
}

/// Whether `name`'s definition in `ir` — the runtime's, or a unit's copy to inline — adjusts a
/// reference count atomically, for every definition of it there is.
fn atomic_counts(ir: &str, name: &str) -> Vec<bool> {
    let mut out = Vec::new();
    let mut rest = ir;
    while let Some(at) = rest.find(&format!(" @{name}(")) {
        let line_start = rest[..at].rfind('\n').map_or(0, |i| i + 1);
        let tail = &rest[at..];
        let end = tail.find("\n}\n").map_or(tail.len(), |e| e + 3);
        if rest[line_start..].starts_with("define ") {
            let body = &rest[line_start..at + end];
            out.push(body.contains("atomicrmw add ptr %") || body.contains("atomicrmw sub ptr %"));
        }
        rest = &rest[at + name.len() + 2..];
    }
    out
}

#[test]
fn a_program_of_one_thread_counts_its_strings_without_atomics() {
    // A string retained and released on every step: the runtime's counts, and the unit's copies
    // of them, are plain loads and stores where nothing can start a thread.
    let strings = "fn main(out: OutCap) -> Int ! {io} {\n    let s = \"a\" + \"b\"\n    \
                   let t = s\n    print(t + \"\\n\")\n    down(3)\n}\n";
    let one = ir(strings, false);
    for name in ["nz.str_retain", "nz.str_release"] {
        let atomic = atomic_counts(&one, name);
        assert!(!atomic.is_empty(), "{name} is defined: {one}");
        assert!(atomic.iter().all(|a| !a), "{name} counts plainly: {one}");
    }
    // A program that may start a task keeps every count atomic.
    let tasks = "fn main(t: SpawnCap, out: OutCap) -> Int ! {spawn, io} {\n    \
                 let s = \"a\" + \"b\"\n    let t2 = s\n    print(t2 + \"\\n\")\n    \
                 scope { spawn down(3); }\n    0\n}\n";
    let many = ir(tasks, false);
    for name in ["nz.str_retain", "nz.str_release"] {
        let atomic = atomic_counts(&many, name);
        assert!(!atomic.is_empty(), "{name} is defined: {many}");
        assert!(
            atomic.iter().all(|a| *a),
            "{name} counts atomically: {many}"
        );
    }
}
