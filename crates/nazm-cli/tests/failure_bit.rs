//! R1, 5b.8 (`docs/performance.md`): a Nazm function returns whether it failed beside its result,
//! and its caller leaves by that bit rather than by reading the thread's failure flag.
//!
//! A failure deep in a recursion — direct, through a function value, through a trait method, in
//! a task, and inside a C export — still ends the program with status 2 and the diagnostic at the
//! failing line, and every frame on the way out still releases what it owns: the same answer on
//! the interpreter, on LLVM at `-O0` and `-O2`, on Cranelift, and — where its subset reaches — on
//! the LIR oracle, whose failure exit is the bit too.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;

use crate::{common, differential};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every implementation: the first call's answer, then status 2 with `N0401` — at `at` where the
/// implementation names a place — and everything reclaimed, the same counts everywhere.
fn fails_everywhere(src: &str, first: &str, at: &str, oracle: bool) {
    let runs = differential::everywhere(&[("prog.nz", src)]);
    let counts = differential::memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, format!("{first}\n"), "{label}");
        assert!(r.stderr.contains("N0401"), "{label}: {r:?}");
        if label != "interpreter" {
            assert!(r.stderr.contains(at), "{label}: {r:?}");
        }
        let line = differential::memory_line(r);
        assert_eq!(line, counts, "{label}");
        assert!(
            line.split_whitespace()
                .filter(|w| w.starts_with("live="))
                .all(|w| w == "live=0"),
            "{label}: {line}"
        );
    }
    if oracle {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("prog.nz"), src).unwrap();
        let o = Command::new(NAZM)
            .current_dir(d.path())
            .args(["lir", "--run", "prog.nz"])
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(2), "the LIR oracle: {err}");
        assert_eq!(String::from_utf8_lossy(&o.stdout), format!("{first}\n"));
        assert!(err.contains(at), "the LIR oracle: {err}");
    }
}

#[test]
fn a_failure_deep_in_a_direct_recursion_unwinds_every_frame() {
    // Four hundred frames, each owning a string, between `main` and the division that fails.
    let src = "fn down(n: Int, k: Int, tag: Str) -> Int {
    let held = str_concat(tag, \"!\")
    if n == 0 { 10 / k } else { down(n - 1, k, held) + str_len(held) }
}

fn main(out: OutCap) -> Int ! {io} {
    println(down(3, 5, \"a\"))
    println(down(400, 0, \"x\"))
    println(7)
    0
}
";
    fails_everywhere(src, "11", "N0401]: division by zero at prog.nz:3:20", true);
}

#[test]
fn a_failure_through_a_function_value_unwinds_every_frame() {
    // Every level calls the next through a function value: an indirect call's bit.
    let src = "fn step(n: Int, k: Int, tag: Str) -> Int {
    let held = str_concat(tag, \"!\")
    if n == 0 { 10 / k } else { let g = step; g(n - 1, k, held) + str_len(held) }
}

fn via(f: fn(Int, Int, Str) -> Int, n: Int, k: Int) -> Int { f(n, k, \"v\") }

fn main(out: OutCap) -> Int ! {io} {
    println(via(step, 3, 5))
    println(via(step, 400, 0))
    println(7)
    0
}
";
    fails_everywhere(src, "11", "N0401]: division by zero at prog.nz:3:20", true);
}

#[test]
fn a_failure_through_a_trait_method_unwinds_every_frame() {
    // The method recurses on a record it builds, and is reached through a generic instance — a
    // unit of its own, calling the impl's function in another.
    let src = "trait Walk { fn walk(self: Self, n: Int, k: Int) -> Int; }

struct W { name: Str, }

impl Walk for W {
    fn walk(self: W, n: Int, k: Int) -> Int {
        let held = str_concat(self.name, \"!\")
        if n == 0 { 10 / k } else { W(name: held).walk(n - 1, k) + str_len(self.name) }
    }
}

fn go[T: Walk](x: T, n: Int, k: Int) -> Int { x.walk(n, k) }

fn main(out: OutCap) -> Int ! {io} {
    println(go(W(name: \"w\"), 3, 5))
    println(go(W(name: \"w\"), 400, 0))
    println(7)
    0
}
";
    fails_everywhere(src, "8", "N0401]: division by zero at prog.nz:8:24", true);
}

#[test]
fn a_failure_deep_in_a_task_unwinds_the_task_and_fails_its_scope() {
    // The task's function is called by its trampoline, which reads the flag: the failure ends the
    // task, and the scope's join fails `main` with it. Outside the oracle's subset.
    let src = "fn down(n: Int, k: Int, tag: Str) -> Int {
    let held = str_concat(tag, \"!\")
    if n == 0 { 10 / k } else { down(n - 1, k, held) + str_len(held) }
}

fn worker(results: Chan, n: Int, k: Int) -> Int {
    chan_send(results, down(n, k, \"t\"))
    0
}

fn main(tasks: SpawnCap, out: OutCap) -> Int {
    let results = chan_new(4)
    scope { spawn worker(results, 3, 5); }
    let buf = ints_new()
    chan_recv(results, buf)
    println(ints_pop(buf))
    scope { spawn worker(results, 400, 0); }
    println(7)
    0
}
";
    fails_everywhere(src, "11", "N0401]: division by zero at prog.nz:3:20", false);
}

#[test]
fn a_failure_deep_inside_a_c_export_ends_the_process_with_its_diagnostic() {
    // C calls the export's C entry, which reads the flag after the Nazm function returns: the
    // failure four hundred frames down is reported and the process ends, nothing returned to C.
    let lib = "fn down(n: Int, k: Int) -> Int {
    if n == 0 { 10 / k } else { down(n - 1, k) + 1 }
}

pub extern \"C\" fn deep(n: Int, k: Int) -> Int ! {} = \"nazm_deep\" { down(n, k) }
";
    let host = "#include <stdio.h>
#include \"libnz.h\"
int main(void) {
    printf(\"%lld\\n\", (long long)nazm_deep(3, 5));
    fflush(stdout);
    printf(\"%lld\\n\", (long long)nazm_deep(400, 0));
    printf(\"not reached\\n\");
    return 0;
}
";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("lib.nz"), lib).unwrap();
    std::fs::write(d.path().join("host.c"), host).unwrap();
    for (backend, opt) in [
        ("llvm", "0"),
        ("llvm", "2"),
        ("cranelift", "0"),
        ("cranelift", "2"),
    ] {
        let b = differential::nazm(
            d.path(),
            &[
                "build",
                "--lib",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                "lib.nz",
                "-o",
                "libnz.a",
            ],
        );
        assert_eq!(b.code, Some(0), "{backend} -O{opt}: {}", b.stderr);
        let c = Command::new("clang")
            .current_dir(d.path())
            .args(["host.c", "libnz.a", "-o", "host"])
            .output()
            .unwrap();
        assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
        let r = common::run_bounded(&mut common::bounded_exe(&d.path().join("host")), "host");
        assert_eq!(r.code, Some(2), "{backend} -O{opt}: {r:?}");
        assert_eq!(r.stdout, "5\n", "{backend} -O{opt}");
        assert!(
            r.stderr.contains("N0401]: division by zero at lib.nz:2:20"),
            "{backend} -O{opt}: {r:?}"
        );
    }
}

#[test]
fn a_nazm_call_takes_the_bit_and_the_platform_calls_a_plain_entry() {
    // The LIR says so: `fib` and `main` call `fib` with the bit and `fib` fails by it, the plain
    // entry under `main`'s symbol calls `main` without it, and nothing reads the flag.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\
         fn main() -> Int { fib(10) }\n",
    )
    .unwrap();
    let r = differential::nazm(d.path(), &["lir", "--ops", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let text = r.stdout;
    assert!(text.contains(", failed v"), "{text}");
    assert!(text.contains("  fail\n"), "{text}");
    assert!(text.contains("plain (i64) -> i64"), "{text}");
    assert!(text.contains("main.body"), "{text}");
    assert!(!text.contains("nz.failed_now"), "{text}");
}
