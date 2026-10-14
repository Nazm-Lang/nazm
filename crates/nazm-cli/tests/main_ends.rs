//! R1, 5b.1–5b.3 (`docs/spec.md`, *What `main` holds, and how it ends* and *A function with no
//! result type*): `main` prints with no capability written; its result is the exit status; a
//! function written with no result type returns nothing.
//!
//! Each program runs as a user runs it — without `NAZM_RESULT_REPORT`, which the rest of the suite
//! sets — under the interpreter, the LIR oracle, LLVM at both levels and Cranelift.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::common;
use crate::differential;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every implementation's run of `files[0]`, without the result report.
fn as_users_run(files: &[(&str, &str)]) -> Vec<(String, Ran)> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let main = files[0].0;
    let quiet = |c: &mut Command| {
        c.env_remove(nazm_lir::RESULT_VARIABLE);
    };
    let mut out = Vec::new();
    for (label, args) in [
        ("interpreter", vec!["run", main]),
        ("lir oracle", vec!["lir", "--run", main]),
    ] {
        let mut c = Command::new(NAZM);
        c.current_dir(d.path()).args(&args);
        quiet(&mut c);
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = d.path().join(label.replace([' ', '-'], ""));
        let b = build(d.path(), backend, opt, main, &exe);
        assert_eq!(b.code, Some(0), "{label}: {b:?}");
        let mut c = common::bounded_exe(&exe);
        c.current_dir(d.path());
        quiet(&mut c);
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

fn build(dir: &Path, backend: &str, opt: &str, main: &str, exe: &Path) -> Ran {
    run_bounded(
        Command::new(NAZM).current_dir(dir).args([
            "build",
            "--no-cache",
            "--backend",
            backend,
            "--opt-level",
            opt,
            main,
            "-o",
            exe.to_str().unwrap(),
        ]),
        "nazm build",
    )
}

#[test]
fn hello_world_is_one_line_and_prints_only_what_it_prints() {
    for (label, r) in as_users_run(&[(
        "prog.nz",
        "fn main() {\n    println(\"Hello, world!\")\n}\n",
    )]) {
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(0), "Hello, world!\n"),
            "{label}: {r:?}"
        );
    }
}

#[test]
fn main_s_result_is_the_exit_status_everywhere() {
    for (result, code, out) in [("0", 0, ""), ("7", 7, ""), ("255", 255, "")] {
        let src = format!("fn main() -> Int {{\n    println(\"ran\")\n    {result}\n}}\n");
        for (label, r) in as_users_run(&[("prog.nz", &src)]) {
            assert_eq!(r.code, Some(code), "{label} {result}: {r:?}");
            assert_eq!(r.stdout, format!("ran\n{out}"), "{label} {result}");
        }
    }
    for result in ["256", "0 - 1", "9223372036854775807"] {
        let src = format!("fn main() -> Int {{ {result} }}\n");
        for (label, r) in as_users_run(&[("prog.nz", &src)]) {
            assert_eq!(r.code, Some(2), "{label} {result}: {r:?}");
            assert!(
                r.stderr.contains("error[N0416]") && r.stdout.is_empty(),
                "{label} {result}: {r:?}"
            );
        }
    }
    // The result report prints it and ends with 0, as every program did before R1.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), "fn main() -> Int { 300 }\n").unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "p.nz"])
            .env(nazm_lir::RESULT_VARIABLE, "1"),
        "nazm run",
    );
    assert_eq!((r.code, r.stdout.as_str()), (Some(0), "300\n"), "{r:?}");
}

#[test]
fn a_function_with_no_result_type_returns_nothing() {
    for (label, r) in as_users_run(&[
        (
            "prog.nz",
            "use \"noise.nz\";

fn greet(out: OutCap, who: Str) {
    if who == \"\" { return }
    println(f\"hello, {who}\")
    str_len(who)
}

fn main() {
    greet(out, \"\")
    greet(out, \"you\")
    noise(out)
}
",
        ),
        (
            "noise.nz",
            "pub fn noise(out: OutCap) {\n    println(\"noise\")\n}\n",
        ),
    ]) {
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(0), "hello, you\nnoise\n"),
            "{label}: {r:?}"
        );
    }
}

#[test]
fn what_main_holds_and_what_returns_nothing_is_checked() {
    for (files, code, why) in [
        (
            vec![(
                "prog.nz",
                "fn main() -> Int { let s = read_file(\"x\"); 0 }\n",
            )],
            "N0369",
            "an `IoCap` is still written",
        ),
        (
            vec![(
                "prog.nz",
                "fn quiet() { }\nfn main() -> Int { let x = quiet(); x }\n",
            )],
            "N0300",
            "the value of a call of a function that returns nothing",
        ),
        (
            vec![
                (
                    "prog.nz",
                    "use \"noise.nz\";\nfn main() -> Int { noise(out) + 1 }\n",
                ),
                (
                    "noise.nz",
                    "pub fn noise(out: OutCap) {\n    println(\"noise\")\n}\n",
                ),
            ],
            "N0300",
            "an imported function that returns nothing",
        ),
    ] {
        let got = differential::check_codes(&files);
        assert_eq!(
            got.first().map(String::as_str),
            Some(code),
            "{why}: {got:?}"
        );
    }
}

#[test]
fn a_private_function_declares_no_effects() {
    // R1, 5b.5 (`docs/spec.md`, *Declared and inferred*): a function that is not `pub` writes
    // no `! { … }`; its own module's callers see what its body requires — a pure caller may
    // call a pure one — while another module's undeclared function may still do anything.
    for (label, r) in as_users_run(&[(
        "prog.nz",
        "fn double(x: Int) -> Int { x * 2 }
fn pure_user(x: Int) -> Int ! {} { double(x) }
fn report(out: OutCap, n: Int) { println(f\"n = {n}\") }
fn main() {
    report(out, pure_user(21))
}
",
    )]) {
        assert_eq!(
            (r.code, r.stdout.as_str()),
            (Some(0), "n = 42\n"),
            "{label}: {r:?}"
        );
    }
    let got = differential::check_codes(&[
        (
            "prog.nz",
            "use \"helper.nz\";\nfn f(x: Int) -> Int ! {} { twice(x) }\nfn main() -> Int { f(2) }\n",
        ),
        ("helper.nz", "pub fn twice(x: Int) -> Int { x * 2 }\n"),
    ]);
    assert_eq!(got.first().map(String::as_str), Some("N0366"), "{got:?}");
}

#[test]
fn build_and_inspect_say_what_the_program_may_touch() {
    // R1, option C: `main`'s capabilities, given or written, after a build and in `inspect`.
    for (src, line) in [
        (
            "fn main() {\n    println(\"hi\")\n}\n",
            "authority: OutCap (given to main): prints to standard output and error",
        ),
        (
            "fn main(io: IoCap, net: NetCap) -> Int { 0 }\n",
            "authority: IoCap: files, arguments, the terminal and the process's exit; NetCap: the network",
        ),
    ] {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("prog.nz"), src).unwrap();
        let exe = d.path().join("prog");
        let b = build(d.path(), "llvm", "0", "prog.nz", &exe);
        assert_eq!(b.code, Some(0), "{b:?}");
        assert!(b.stderr.lines().any(|l| l == line), "{b:?}");
        let r = run_bounded(
            Command::new(NAZM)
                .current_dir(d.path())
                .args(["inspect", "prog.nz"]),
            "nazm inspect",
        );
        let doc: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
        let first = &doc["authority"][0];
        assert_eq!(
            first["implicit"],
            src.starts_with("fn main()"),
            "{}",
            r.stdout
        );
        assert!(first["capability"].as_str().unwrap().ends_with("Cap"));
    }
}
