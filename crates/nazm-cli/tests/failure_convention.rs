//! How a failure crosses a call (`docs/architecture.md` §7.106, *The failure bit*, and §7.115).
//! R1, 5b.8: a Nazm function returns whether it failed beside its result, and its caller leaves by
//! that bit rather than by reading the thread's failure flag — still so on WebAssembly, WASI, the
//! boards, Windows, under Cranelift and in the LIR oracle. 4.2: on the hosted targets under LLVM a
//! failure is a forced unwind instead, landing only in frames with something to release, and a
//! plain entry lands it so C is never unwound into.
//!
//! A failure deep in a recursion — direct, through a function value, through a trait method, in
//! a task, inside a C export, through a callback C calls, ten thousand frames deep through
//! `defer`s, and with the stack exhausted — still ends the program with status 2 and the
//! diagnostic at the failing line, and every frame on the way out still releases what it owns:
//! the same answer on the interpreter, on LLVM at `-O0` and `-O2`, on Cranelift, on Nazm's own
//! code generator where it builds (R9 7.5, D15: macOS, where it unwinds as LLVM does), and — where
//! its subset reaches — on the LIR oracle.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;

use crate::{common, differential};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every implementation: the first call's answer, then status 2 with the code `at` begins with —
/// at `at` where the implementation names a place — and everything reclaimed, the same counts
/// everywhere.
fn fails_everywhere(src: &str, first: &str, at: &str, oracle: bool) {
    let runs = differential::everywhere(&[("prog.nz", src)]);
    let counts = differential::memory_line(&runs[0].1);
    let code = &at[..5];
    for (label, r) in &runs {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, format!("{first}\n"), "{label}");
        assert!(r.stderr.contains(code), "{label}: {r:?}");
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
    let mut backends = vec![
        ("llvm", "0"),
        ("llvm", "2"),
        ("cranelift", "0"),
        ("cranelift", "2"),
    ];
    if cfg!(target_os = "macos") {
        backends.push(("nazm", "0"));
    }
    for (backend, opt) in backends {
        let b = nazm_by(
            backend,
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
        let r = common::run_bounded(
            &mut common::bounded_exe(&d.path().join(crate::common::exe_name("host"))),
            "host",
        );
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

/// `nazm ARGS` in `dir`, for `backend`: Nazm's own code generator, where it is the backend, is
/// the one this test process built (`differential::nzcg`).
fn nazm_by(backend: &str, dir: &std::path::Path, args: &[&str]) -> common::Ran {
    let mut c = Command::new(NAZM);
    c.current_dir(dir).args(args);
    if backend == "nazm" {
        c.env(nazm_codegen_nazm::VARIABLE, differential::nzcg());
    }
    common::run_bounded(&mut c, "nazm")
}

/// Each native build of `src`, run with the memory report on: `(label, result)`.
fn natively(d: &std::path::Path, extra: &[&str]) -> Vec<(String, common::Ran)> {
    let mut out = Vec::new();
    let mut backends = vec![("llvm", "0"), ("llvm", "2"), ("cranelift", "0")];
    if cfg!(target_os = "macos") {
        backends.push(("nazm", "0"));
    }
    for (backend, opt) in backends {
        let label = format!("{backend} -O{opt}");
        let exe = d.join(crate::common::exe_name(format!("p-{backend}-{opt}")));
        let mut args = vec![
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--opt-level",
            opt,
        ];
        args.extend(extra);
        args.extend(["prog.nz", "-o", exe.to_str().unwrap()]);
        let b = nazm_by(backend, d, &args);
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d).env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.clone(), common::run_bounded(&mut c, &label)));
    }
    out
}

#[test]
fn a_failure_ten_thousand_frames_deep_through_defers_runs_none_and_releases_every_frame() {
    // 4.2: every frame owns a string and has a `defer`; the unwind lands in each to release its
    // string, and runs no `defer` (`docs/spec.md`, `defer`): the first call prints `ddd`, the
    // second nothing before the overflow. Native only: the interpreter's recursion limit is lower.
    let src = "fn down(out: OutCap, n: Int, k: Int, tag: Str) -> Int ! {io} {
    let held = str_concat(tag, \"!\")
    defer { print(\"d\") }
    if n == 0 { k + str_len(held) } else { down(out, n - 1, k, tag) + 1 }
}

fn main(out: OutCap) -> Int ! {io} {
    println(down(out, 2, 1, \"a\"))
    println(down(out, 10000, 9223372036854775807, \"x\"))
    println(7)
    0
}
";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let runs = natively(d.path(), &[]);
    let counts = differential::memory_line(&runs[0].1);
    assert!(
        counts.contains("strings allocated=10006 reclaimed=10006 live=0"),
        "{counts}"
    );
    for (label, r) in &runs {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, "ddd5\n", "{label}");
        assert!(
            r.stderr
                .contains("error[N0400]: this addition overflows `Int` at prog.nz:4:19"),
            "{label}: {r:?}"
        );
        assert_eq!(differential::memory_line(r), counts, "{label}");
    }
}

#[test]
fn a_failure_with_the_stack_exhausted_unwinds_in_its_headroom() {
    // 4.2: `N0408` is a failure like any other; the unwinder runs in the headroom the stack's
    // limit leaves, through every frame that owns a string, and the process reports and exits 2.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn deep(out: OutCap, n: Int, tag: Str) -> Int ! {io} {
    let held = str_concat(tag, \"!\")
    defer { print(\"d\") }
    deep(out, n + 1, tag) + str_len(held)
}

fn main(out: OutCap) -> Int ! {io} {
    println(1)
    println(deep(out, 0, \"x\"))
    0
}
",
    )
    .unwrap();
    for (label, r) in natively(d.path(), &[]) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, "1\n", "{label}");
        assert!(
            r.stderr
                .contains("error[N0408]: the native call stack is exhausted at prog.nz:1"),
            "{label}: {r:?}"
        );
        let line = differential::memory_line(&r);
        assert!(
            line.split_whitespace()
                .filter(|w| w.starts_with("live="))
                .all(|w| w == "live=0"),
            "{label}: {line}"
        );
    }
}

#[test]
fn a_failure_deep_in_a_callback_c_calls_ends_the_process_with_its_diagnostic() {
    // 4.2: Nazm calls C, which calls an export; the failure four hundred frames into the export
    // is landed by its C entry — never unwinding into C — which reports it and ends the process.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "extern \"C\" fn apply(f: fn(Int) -> Int, x: Int) -> Int = \"apply\";

fn down(n: Int, k: Int, tag: Str) -> Int {
    let held = str_concat(tag, \"!\")
    if n == 0 { 10 / k } else { down(n - 1, k, tag) + str_len(held) }
}

pub extern \"C\" fn cb(n: Int) -> Int ! {} = \"nz_cb\" { down(n, 400 - n, \"x\") }

fn main(out: OutCap, f: ForeignCap) -> Int ! {io, foreign} {
    println(apply(cb, 3))
    println(apply(cb, 400))
    println(7)
    0
}
",
    )
    .unwrap();
    std::fs::write(
        d.path().join("fixture.c"),
        "#include <stdint.h>\nint64_t apply(int64_t (*f)(int64_t), int64_t x) { return f(x) + 1; }\n",
    )
    .unwrap();
    let c = Command::new("clang")
        .current_dir(d.path())
        .args(["-c", "-O1", "fixture.c", "-o", "fixture.o"])
        .output()
        .unwrap();
    assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    for (label, r) in natively(d.path(), &["--link", "fixture.o"]) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, "7\n", "{label}");
        assert!(
            r.stderr
                .contains("error[N0401]: division by zero at prog.nz:5:20"),
            "{label}: {r:?}"
        );
    }
}

#[test]
fn hosted_code_unwinds_and_the_boards_and_webassembly_keep_the_bit() {
    // 4.2, by target. Hosted: no Nazm function returns a `{T, i1}` pair; a call into a bare
    // failure exit is a plain call (`fib` has no landing pad, so no personality), a call whose
    // failure edge releases something is an `invoke`, a failure exit calls `nz.raise`, and a
    // plain entry invokes its body. WebAssembly, WASI, the boards and Windows: no unwinder.
    let src = "fn fib(n: Int) -> Int { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }
fn tagged(n: Int) -> Int { let s = str_concat(\"a\", \"b\"); fib(n) + str_len(s) }
fn main() -> Int { tagged(10) }
";
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let b = differential::nazm(
        d.path(),
        &["build", "--no-cache", "--emit-ir", "prog.nz", "-o", "p"],
    );
    assert_eq!(b.code, Some(0), "{}", b.stderr);
    let unit = std::fs::read_to_string(d.path().join("p.m0.ll")).unwrap();
    let define = |name: &str| {
        unit.lines()
            .find(|l| l.starts_with("define ") && l.contains(&format!("_2Enz.{name}(")))
            .unwrap_or_else(|| panic!("no {name}:\n{unit}"))
            .to_owned()
    };
    assert!(!unit.contains(", i1} @nz.m."), "{unit}");
    assert!(define("fib").starts_with("define internal i64 "), "{unit}");
    assert!(!define("fib").contains("personality"), "{unit}");
    assert!(unit.contains("call i64 @nz.m.prog_2Enz.fib("), "{unit}");
    assert!(
        define("tagged").contains("personality ptr @__gcc_personality_v0"),
        "{unit}"
    );
    assert!(unit.contains("invoke i64 @nz.m.prog_2Enz.fib("), "{unit}");
    assert!(define("main").contains("personality"), "{unit}");
    assert!(
        unit.contains("invoke i64 @nz.m.prog_2Enz.main.body("),
        "{unit}"
    );
    assert!(
        unit.contains("  call void @nz.raise()\n  unreachable"),
        "{unit}"
    );
    let r = common::run_bounded(
        &mut common::bounded_exe(&d.path().join(crate::common::exe_name("p"))),
        "p",
    );
    // `main`'s result, fib(10) + 2: the exit status, or printed where the run asks for it.
    assert!(
        r.code == Some(57) || (r.code == Some(0) && r.stdout == "57\n"),
        "{r:?}"
    );
    // A program every target builds: a recursion whose addition can fail.
    std::fs::write(
        d.path().join("add.nz"),
        "fn add(a: Int, b: Int) -> Int { if a == 0 { b } else { add(a - 1, b + 1) } }\n\
         fn main() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    for (target, unwinds) in [
        ("x86_64-unknown-linux-gnu", true),
        ("aarch64-unknown-none", false),
        ("wasm32-unknown-unknown", false),
        ("wasm32-wasip1", false),
        ("x86_64-pc-windows-gnu", false),
    ] {
        let b = differential::nazm(
            d.path(),
            &[
                "build",
                "--no-cache",
                "--target",
                target,
                "--objects",
                target,
                "add.nz",
            ],
        );
        assert_eq!(b.code, Some(0), "{target}: {}", b.stderr);
        let object = std::fs::read(d.path().join(target).join("00-m0.o")).unwrap();
        let names = |s: &str| object.windows(s.len()).any(|w| w == s.as_bytes());
        assert_eq!(names("_Unwind_ForcedUnwind"), unwinds, "{target}");
        assert_eq!(names("__gcc_personality_v0"), unwinds, "{target}");
    }
}

/// `nazm doctor` (W2): with no clang on `PATH`, it names what is missing and exits 4, the
/// toolchain's status; with this machine's own `PATH`, it answers either way and says so.
#[test]
fn doctor_names_what_a_native_build_lacks() {
    let empty = tempfile::tempdir().unwrap();
    let out = Command::new(NAZM)
        .arg("doctor")
        .env("PATH", empty.path())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(4), "{said}");
    assert!(
        said.contains("clang") && said.contains("MISSING") && said.contains("not found on PATH"),
        "{said}"
    );
    let out = Command::new(NAZM).arg("doctor").output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    match out.status.code() {
        Some(0) => assert!(said.contains("has what it needs"), "{said}"),
        Some(4) => assert!(said.contains("MISSING"), "{said}"),
        other => panic!("nazm doctor exited {other:?}: {said}"),
    }
}
