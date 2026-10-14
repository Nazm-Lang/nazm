//! R1.5, D11: `nazm run` compiles with Cranelift and runs the executable; `nazm run --interpret`
//! is the interpreter (`docs/spec.md`, *`nazm run` compiles*).
//!
//! The two are asked the same questions and must give the same answers: what the program
//! writes to each stream, the memory report it reads from the environment, and the status it
//! ends with — whether `main`'s result, `exit_with`'s or a runtime failure's. A program only a
//! native build refuses is refused by `nazm run` with help naming `--interpret`, and runs under
//! `--interpret`: neither path falls back to the other. Nothing `nazm` says about a build reaches
//! the program's streams, the interpreter's limits are refused without the interpreter, and
//! the temporary directory the executable is built in is gone afterwards.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// What one run did.
#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// `nazm run` with `flags` before the file, `args` after `--` and `input` on standard input,
/// with the memory report on and `main`'s result as the exit status — the default a user sees,
/// whatever cargo's environment says. `TMPDIR` is `tmp`, so what the run leaves there is seen.
fn run(dir: &Path, tmp: &Path, flags: &[&str], file: &str, args: &[&str], input: &str) -> Ran {
    let mut child = Command::new(NAZM)
        .current_dir(dir)
        .arg("run")
        .args(flags)
        .arg(file)
        .arg("--")
        .args(args)
        .env_remove(nazm_lir::RESULT_VARIABLE)
        .env(nazm_lir::MEMORY_VARIABLE, "1")
        .env("TMPDIR", tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start nazm");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait for nazm");
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The program `src` run both ways: compiled first, then interpreted.
fn both(src: &str, args: &[&str], input: &str) -> (Ran, Ran) {
    let d = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let compiled = run(d.path(), tmp.path(), &[], "prog.nz", args, input);
    let interpreted = run(
        d.path(),
        tmp.path(),
        &["--interpret"],
        "prog.nz",
        args,
        input,
    );
    // The executable and the directory it was built in are removed, whatever the program did.
    let left: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "`nazm run` left {left:?} behind");
    (compiled, interpreted)
}

const ECHO: &str = r#"use "@std/io";

fn main(io: IoCap) -> Int ! { io } {
    let v = vec_new[Str]();
    vec_push(v, arg(0));
    println(str_concat("arg: ", vec_get(v, 0)));
    match io_stdin_read_line(io) {
        Result.Ok(value: o) => match o {
            Option.Some(value: s) => print(f"read {s}\n"),
            Option.None() => print("end\n"),
        },
        Result.Err(error: e) => print("error\n"),
    }
    eprintln("to stderr");
    7
}
"#;

#[test]
fn a_compiled_run_and_the_interpreter_agree_on_every_stream_and_the_status() {
    let (compiled, interpreted) = both(ECHO, &["first"], "a line\n");
    assert_eq!(compiled, interpreted);
    assert_eq!(compiled.code, Some(7), "{compiled:?}");
    // Nothing about the build: no executable path on stdout, no authority line on stderr.
    assert_eq!(compiled.stdout, "arg: first\nread a line\n", "{compiled:?}");
    assert!(
        compiled.stderr.starts_with("to stderr\nnazm-memory: "),
        "{compiled:?}"
    );
}

#[test]
fn the_exit_status_passes_through() {
    for (body, code) in [
        ("0", 0),
        ("3", 3),
        ("255", 255),
        // `exit_with`'s status, and a runtime failure's.
        ("exit_with(42)", 42),
        ("let d = 0;\n    10 / d", 2),
    ] {
        let src = format!("fn main(io: IoCap) -> Int ! {{ io }} {{\n    {body}\n}}\n");
        let (compiled, interpreted) = both(&src, &[], "");
        assert_eq!(compiled.code, Some(code), "{body}: {compiled:?}");
        assert_eq!(compiled.code, interpreted.code, "{body}: {interpreted:?}");
        assert_eq!(compiled.stdout, interpreted.stdout, "{body}");
    }
}

#[test]
fn a_program_only_a_native_build_refuses_is_refused_with_help_naming_interpret() {
    for (src, code, interpreted_says) in [
        // Outside the backend's subset (`N0101`).
        ("fn main() -> Bool {\n    true\n}\n", "N0101", "true\n"),
        // Polymorphic recursion: native instances that would never end (`N0357`).
        (
            "fn grow[T](x: T, n: Int) -> Int {\n    if n == 0 { 0 } else {\n        \
             let v = vec_new[T]();\n        vec_push(v, x);\n        grow(v, n - 1) + 1\n    \
             }\n}\nfn main() -> Int {\n    print(\"ran\\n\");\n    grow(1, 3)\n}\n",
            "N0357",
            "ran\n",
        ),
    ] {
        let (compiled, interpreted) = both(src, &[], "");
        assert_eq!(compiled.code, Some(1), "{code}: {compiled:?}");
        assert_eq!(compiled.stdout, "", "{code}: nothing ran");
        assert!(compiled.stderr.contains(code), "{code}: {compiled:?}");
        assert!(
            compiled.stderr.contains(
                "help: `nazm run` compiles, with Cranelift, and falls back to nothing; \
                           `nazm run --interpret` runs this program in the interpreter"
            ),
            "{code}: {compiled:?}"
        );
        // The interpreter runs it: neither path falls back to the other, and both exist.
        assert!(
            interpreted.stdout.starts_with(interpreted_says),
            "{code}: {interpreted:?}"
        );
    }
}

#[test]
fn a_program_that_does_not_check_gets_no_interpret_help() {
    // The interpreter would refuse it too, so naming `--interpret` would send the reader nowhere.
    let (compiled, interpreted) = both("fn main() -> Int {\n    nothing_here\n}\n", &[], "");
    assert_eq!(compiled.code, Some(1), "{compiled:?}");
    assert_eq!(compiled.code, interpreted.code);
    assert!(compiled.stderr.contains("N0200"), "{compiled:?}");
    assert!(!compiled.stderr.contains("--interpret"), "{compiled:?}");
}

#[test]
fn the_interpreters_limits_are_refused_without_the_interpreter() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), "fn main() -> Int { 0 }\n").unwrap();
    for flag in ["--max-iterations", "--max-depth"] {
        let compiled = run(d.path(), d.path(), &[flag, "100"], "prog.nz", &[], "");
        assert_eq!(compiled.code, Some(5), "{flag}: {compiled:?}");
        assert!(
            compiled.stderr.contains("--interpret"),
            "{flag}: {compiled:?}"
        );
        let interpreted = run(
            d.path(),
            d.path(),
            &["--interpret", flag, "100"],
            "prog.nz",
            &[],
            "",
        );
        assert_eq!(interpreted.code, Some(0), "{flag}: {interpreted:?}");
    }
}
