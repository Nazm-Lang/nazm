//! N49: string escapes, the logical operators, native recursion and qualified names.
//!
//! `docs/architecture.md` §7.51 and the spec sections it names. What only a whole run shows:
//! that each mechanism means the same in the interpreter and in both native backends — the
//! same stdout, exit status and memory report — that refusals are refusals before anything
//! runs, and that the formatter keeps what was written.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Every program here finishes in well under a second; one that does not fails by name.
const DEADLINE: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_bounded(cmd: &mut Command, what: &str) -> Ran {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            let o = child.wait_with_output().expect("collect output");
            return Ran {
                code: o.status.code(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            };
        }
        if start.elapsed() >= DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} did not terminate within {DEADLINE:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

/// The program at `dir/main` under the interpreter, then built and run by each backend at
/// each optimisation level, with the memory report on.
fn everywhere(dir: &Path, main: &str) -> Vec<(String, Ran)> {
    let mut out = vec![("interpreter".to_owned(), nazm(dir, &["run", main]))];
    for (label, backend, opt) in [
        ("llvm -O0", "llvm", "0"),
        ("llvm -O2", "llvm", "2"),
        ("cranelift", "cranelift", "0"),
    ] {
        let exe = dir.join(label.replace([' ', '-'], ""));
        let b = nazm(
            dir,
            &[
                "build",
                "--no-cache",
                "--backend",
                backend,
                "--opt-level",
                opt,
                main,
                "-o",
                exe.to_str().unwrap(),
            ],
        );
        assert_eq!(b.code, Some(0), "{label}: {}", b.stderr);
        let mut c = common::bounded_exe(&exe);
        c.env(nazm_lir::MEMORY_VARIABLE, "1");
        out.push((label.to_owned(), run_bounded(&mut c, label)));
    }
    out
}

/// Every implementation prints `expected` and exits with `code`; every native build
/// reclaims all it allocated.
fn agrees_on(files: &[(&str, &str)], expected: &str, code: i32) {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let p = d.path().join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }
    for (label, r) in everywhere(d.path(), files[0].0) {
        assert_eq!(r.code, Some(code), "{label}: {}", r.stderr);
        assert_eq!(r.stdout.trim_end(), expected, "{label}");
        if label != "interpreter" {
            let line = r
                .stderr
                .lines()
                .find(|l| l.starts_with("nazm-memory:"))
                .unwrap_or_else(|| panic!("{label}: no memory report: {}", r.stderr));
            for kind in ["sequences", "strings", "channels"] {
                let part = line.split(kind).nth(1).unwrap_or("");
                assert!(
                    part.contains("live=0"),
                    "{label}: {kind} not all reclaimed: {line}"
                );
            }
        }
    }
}

fn agrees(src: &str, expected: &str) {
    agrees_on(&[("prog.nz", src)], expected, 0);
}

fn check_codes(files: &[(&str, &str)]) -> Vec<String> {
    let d = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(d.path().join(name), text).unwrap();
    }
    let r = nazm(d.path(), &["check", "--json", files[0].0]);
    let mut codes: Vec<String> = Vec::new();
    for line in r.stdout.lines().chain(r.stderr.lines()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(c) = v["code"].as_str() {
            codes.push(c.to_owned());
        }
        if let Some(ds) = v["diagnostics"].as_array() {
            for d in ds {
                if let Some(c) = d["code"].as_str() {
                    codes.push(c.to_owned());
                }
            }
        }
    }
    codes
}

// --- string escapes --------------------------------------------------------------------

#[test]
fn escapes_are_the_same_bytes_in_every_implementation() {
    // Each byte is printed as a number, so the comparison is of bytes, not of a terminal's
    // rendering of them.
    let src = r#"
fn bytes(s: Str) -> Int {
    let mut i = 0;
    while i < str_len(s) {
        print(str_concat(int_to_str(str_byte(s, i)), "\n"));
        i = i + 1;
    }
    str_len(s)
}
fn main() -> Int {
    let n = bytes("a\nb\t\r\0\\\"\x41\x7f");
    print("line one\nline two\n");
    print("é\x21\n");
    if "\x41" == "A" { n } else { 0 - 1 }
}
"#;
    agrees(
        src,
        "97\n10\n98\n9\n13\n0\n92\n34\n65\n127\nline one\nline two\né!\n10",
    );
}

#[test]
fn a_refused_escape_stops_the_program_before_anything_runs() {
    assert_eq!(
        check_codes(&[("prog.nz", "fn main() -> Int { str_len(\"a\\qb\") }\n")]),
        ["N0004"]
    );
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("prog.nz"),
        "fn main() -> Int { print(\"ran\"); str_len(\"\\x80\") }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["run", "prog.nz"]);
    assert_eq!(r.code, Some(1), "{}", r.stderr);
    assert!(!r.stdout.contains("ran"), "{}", r.stdout);
    assert!(r.stderr.contains("N0004"), "{}", r.stderr);
}

#[test]
fn the_formatter_keeps_an_escape_as_it_was_written() {
    let d = tempfile::tempdir().unwrap();
    let src = "fn main()->Int{str_len(\"a\\n\\x41\\\\\")}\n";
    std::fs::write(d.path().join("prog.nz"), src).unwrap();
    let r = nazm(d.path(), &["fmt", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let formatted = std::fs::read_to_string(d.path().join("prog.nz")).unwrap();
    assert!(formatted.contains("\"a\\n\\x41\\\\\""), "{formatted}");
    let again = nazm(d.path(), &["fmt", "--check", "prog.nz"]);
    assert_eq!(again.code, Some(0), "idempotent: {}", again.stderr);
}
