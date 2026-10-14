//! N11, natively: what monomorphisation may and may not do.
//!
//! `nazm run` is parametric and `nazm build` specialises — one native function per
//! concrete list of type arguments (`docs/architecture.md` §7.12). These tests hold the
//! places where that is visible: the recursion rule applies to instances as it does to
//! every function, an instance family that would never end is refused rather than
//! discovered by running out of memory, and a failure inside an instance is reported at the
//! generic definition's own line.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).expect("write");
}

/// `(ok, stdout, stderr)` of a command in `dir`.
fn nazm(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(NAZM)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("nazm");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn polymorphic_recursion_runs_interpreted_and_is_refused_natively_by_name() {
    // `grow[T]` calls `grow[Vec[T]]`: every instance needs a larger one. The interpreter
    // does not specialise and runs it; the native backend refuses it with the call that
    // expands, before generating anything.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "fn grow[T](x: T, n: Int) -> Int {\n    if n == 0 { 0 } else {\n        \
         let v = vec_new[T]();\n        vec_push(v, x);\n        grow(v, n - 1) + 1\n    }\n}\n\
         fn main() -> Int { grow(1, 3) }\n",
    );
    let (ok, stdout, _) = nazm(dir.path(), &["run", "--interpret", "m.nz"]);
    assert!(ok);
    assert_eq!(stdout, "3");
    let (ok, _, stderr) = nazm(dir.path(), &["build", "m.nz", "-o", "prog"]);
    assert!(!ok);
    assert!(stderr.contains("N0357"), "{stderr}");
    assert!(
        stderr.contains("m.nz:5:9"),
        "the call that expands: {stderr}"
    );
    assert!(
        !stderr.contains("N0101"),
        "one mistake, one error: {stderr}"
    );
}

#[test]
fn ordinary_recursion_through_a_generic_function_compiles() {
    // `count[Str]` calling `count[Str]` is recursion, refused natively until N49 and
    // compiled since, with the stack check MIR places in the instance as in any other
    // function in a call cycle. Polymorphic recursion is still refused (`N0357`).
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "fn count[T](x: T, n: Int) -> Int { if n == 0 { 0 } else { count(x, n - 1) + 1 } }\n\
         fn main() -> Int { count(\"a\", 3) }\n",
    );
    let (ok, stdout, _) = nazm(dir.path(), &["run", "--interpret", "m.nz"]);
    assert!(ok);
    assert_eq!(stdout, "3");
    let (ok, _, stderr) = nazm(dir.path(), &["build", "m.nz", "-o", "prog"]);
    assert!(ok, "{stderr}");
    let ran = Command::new(dir.path().join("prog")).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), "3");
}

#[test]
fn an_instance_nested_past_the_stated_bound_is_refused_rather_than_generated() {
    // Seventy generic functions, each passing a `Box` of its argument to the next: finite,
    // and past what `nazm build` will generate. The bound is stated and reported.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut src = String::from("struct Box[T] { v: T, }\n");
    for i in 0..70 {
        src.push_str(&format!(
            "fn f{i}[T](x: T) -> Int {{ f{}(Box[T](v: x)) }}\n",
            i + 1
        ));
    }
    src.push_str("fn f70[T](x: T) -> Int { 0 }\nfn main() -> Int { f0(1) }\n");
    write(dir.path(), "m.nz", &src);
    let (ok, stdout, _) = nazm(dir.path(), &["run", "--interpret", "m.nz"]);
    assert!(ok, "the interpreter has no such bound");
    assert_eq!(stdout, "0");
    let (ok, _, stderr) = nazm(dir.path(), &["build", "m.nz", "-o", "prog"]);
    assert!(!ok);
    assert!(stderr.contains("N0358"), "{stderr}");
}

/// Q1-B-06: an instance whose type arguments nest sixteen deep — 65,536 leaves in their spelling —
/// builds and runs natively under both backends, its symbol a bounded digest rather than the
/// megabytes the linker refused; the interpreter agrees.
#[test]
fn an_instance_too_long_to_spell_builds_with_a_bounded_symbol() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut src = String::from("struct P[A, B] { a: A, b: B }\nfn g0[T](x: T) -> Int { 1 }\n");
    for i in 1..=16 {
        src.push_str(&format!(
            "fn g{i}[T](x: T) -> Int {{ g{p}(P[T, T](a: x, b: x)) + g{p}(x) }}\n",
            p = i - 1
        ));
    }
    src.push_str("fn main() -> Int { g16(1) }\n");
    write(dir.path(), "m.nz", &src);
    let (ok, stdout, stderr) = nazm(dir.path(), &["run", "--interpret", "m.nz"]);
    assert!(ok, "{stderr}");
    assert_eq!(stdout, "65536");
    for backend in ["llvm", "cranelift"] {
        let (ok, _, stderr) = nazm(
            dir.path(),
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "m.nz",
                "-o",
                "prog",
            ],
        );
        assert!(ok, "{backend}: {stderr}");
        let out = Command::new(dir.path().join("prog")).output().expect("run");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "65536",
            "{backend}"
        );
    }
    let (ok, _, stderr) = nazm(
        dir.path(),
        &["build", "--no-cache", "--emit-ir", "m.nz", "-o", "ir"],
    );
    assert!(ok, "{stderr}");
    let mut longest = 0;
    for e in std::fs::read_dir(dir.path()).expect("dir") {
        let p = e.expect("entry").path();
        if p.extension().is_some_and(|x| x == "ll") {
            for line in std::fs::read_to_string(&p).expect("ll").lines() {
                if line.starts_with("define ") {
                    longest = longest.max(line.len());
                }
            }
        }
    }
    assert!(
        longest > 0 && longest < 1024,
        "a define line of {longest} bytes"
    );
}

#[test]
fn a_failure_inside_an_instance_names_the_generic_definitions_line() {
    // The instance is lowered from the definition's own body, spans and all, so a failure
    // in `identity[Str]`'s code is reported where `pick` is written — in both
    // implementations, with the same code and the same position.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "fn pick[T](xs: Vec[T], i: Int) -> T {\n    vec_get(xs, i)\n}\n\
         fn main() -> Int {\n    let xs = vec_new[Str]();\n    str_len(pick(xs, 3))\n}\n",
    );
    let (ok, _, interpreted) = nazm(dir.path(), &["run", "--interpret", "m.nz"]);
    assert!(!ok);
    assert!(interpreted.contains("N0405"), "{interpreted}");
    assert!(interpreted.contains("m.nz:2:5"), "{interpreted}");
    let (ok, _, stderr) = nazm(dir.path(), &["build", "m.nz", "-o", "prog"]);
    assert!(ok, "{stderr}");
    let out = Command::new(dir.path().join("prog")).output().expect("run");
    assert_eq!(out.status.code(), Some(2));
    let native = String::from_utf8_lossy(&out.stderr);
    assert!(native.contains("N0405"), "{native}");
    assert!(native.contains("m.nz:2:5"), "{native}");
}

#[test]
fn a_generic_program_behaves_the_same_at_both_optimisation_levels() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "enum Maybe[T] { None, Some(value: T), }\n\
         fn or[T](m: Maybe[T], d: T) -> T { match m { Maybe.None() => d, Maybe.Some(value: v) => v, } }\n\
         fn main() -> Int {\n    let xs = vec_new[Maybe[Int]]();\n    let mut i = 0;\n    \
         while i < 10 {\n        if i % 2 == 0 { vec_push(xs, Maybe[Int].Some(value: i)); } \
         else { vec_push(xs, Maybe[Int].None()); }\n        i = i + 1;\n    }\n    \
         let mut total = 0;\n    let mut k = 0;\n    \
         while k < vec_len(xs) { total = total + or(vec_get(xs, k), 100); k = k + 1; }\n    total\n}\n",
    );
    for level in ["0", "2"] {
        let (ok, _, stderr) = nazm(
            dir.path(),
            &["build", "m.nz", "-o", "prog", "--opt-level", level],
        );
        assert!(ok, "{stderr}");
        let out = Command::new(dir.path().join("prog")).output().expect("run");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "520",
            "-O{level}"
        );
    }
}

#[test]
fn a_call_carries_the_types_its_callee_is_declared_with() {
    // Found by the N11 conformance corpus: `main` passes a `Maybe[Str]` it never binds
    // straight from one instance to another, so nothing in `main`'s own code mentions the
    // type — and the declaration of `or_else[Str]` named it anyway. The unit must carry
    // every type a declaration it writes names, or clang reads a type it was never given.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "enum Maybe[T] { Some(value: T), None, }\n\
         fn some[T](x: T) -> Maybe[T] { Maybe[T].Some(value: x) }\n\
         fn or_else[T](m: Maybe[T], d: T) -> T { match m { Maybe.None() => d, Maybe.Some(value: v) => v, } }\n\
         fn main() -> Int { str_len(or_else(some(\"abc\"), \"none\")) }\n",
    );
    let (ok, _, stderr) = nazm(dir.path(), &["build", "m.nz", "-o", "prog"]);
    assert!(ok, "{stderr}");
    let out = Command::new(dir.path().join("prog")).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3");
}

#[test]
fn two_instances_that_differ_only_in_a_later_argument_are_two_functions() {
    // Found by mutation: a symbol built from the first type argument alone gave
    // `second[Int, Str]` and `second[Int, Bool]` one name, and nothing noticed. Every
    // argument is in an instance's identity, and both instances run.
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "m.nz",
        "fn second[A, B](a: A, b: B) -> B { b }\n\
         fn main() -> Int {\n    let s = second(1, \"four\");\n    \
         if second(2, true) { str_len(s) } else { 0 }\n}\n",
    );
    let (ok, _, stderr) = nazm(dir.path(), &["build", "m.nz", "-o", "prog", "--emit-ir"]);
    assert!(ok, "{stderr}");
    let out = Command::new(dir.path().join("prog")).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "4");
}
