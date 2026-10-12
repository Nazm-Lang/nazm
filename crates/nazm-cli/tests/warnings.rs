//! R1b, L15: a warning severity (`docs/spec.md`, *Patterns*; `docs/diagnostics.md`).
//!
//! A `_` or a name in a `match` of an enum that covers variants no arm names is a warning,
//! `N0632`: the program is accepted, `nazm check` and `nazm build` report it, and
//! `--deny-warnings` refuses it. A match that names every variant, an `if let`, and a `let … else`
//! draw none; a warning survives the check cache, and JSON gives its severity.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const COVERING: &str = "enum E { A, B(n: Int), C, }
fn f(e: E) -> Int { match e { E.A() => 1, _ => 0, } }
fn g(e: E) -> Int { match e { E.A() | E.C() => 1, E.B(n: x) => x, } }
fn h(e: E) -> Int { match e { E.B(n: 0) => 1, other => 2, } }
fn i(e: E) -> Int { if let E.A() = e { 1 } else { 0 } }
fn main() -> Int { f(E.C()) + g(E.A()) + h(E.A()) + i(E.B(n: 1)) }
";

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("w.nz"), COVERING).unwrap();
    d
}

#[test]
fn a_wildcard_covering_variants_is_a_warning_and_the_program_checks() {
    let d = setup();
    let r = nazm(d.path(), &["check", "w.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    assert!(r.stdout.starts_with("ok"), "{r:?}");
    // `f`'s `_` and `h`'s name, each listing exactly what no arm names; not `g`, which names every
    // variant, and not the `if let`.
    assert_eq!(
        r.stderr.matches("warning[N0632]").count(),
        2,
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("`_` covers `E.B`, `E.C`, which no arm names"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("this name covers `E.A`, `E.C`, which no arm names"),
        "{}",
        r.stderr
    );
    // Reused from the cache, it is still said.
    let again = nazm(d.path(), &["check", "w.nz"]);
    assert_eq!(
        again.stderr.matches("warning[N0632]").count(),
        2,
        "{again:?}"
    );
    // JSON gives the severity.
    let j = nazm(d.path(), &["check", "--json", "w.nz"]);
    assert!(
        format!("{}{}", j.stdout, j.stderr).contains("\"severity\":\"warning\""),
        "{j:?}"
    );
}

#[test]
fn deny_warnings_refuses_what_the_checker_warns_of() {
    let d = setup();
    let r = nazm(d.path(), &["check", "--deny-warnings", "w.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("refused by --deny-warnings"), "{r:?}");
    let b = nazm(
        d.path(),
        &["build", "--deny-warnings", "w.nz", "-o", "denied"],
    );
    assert_ne!(b.code, Some(0), "{b:?}");
    assert!(!d.path().join("denied").exists());
    // Without it, a build reports the warnings and builds.
    let b = nazm(d.path(), &["build", "w.nz", "-o", "built"]);
    assert_eq!(b.code, Some(0), "{b:?}");
    assert!(b.stderr.contains("warning[N0632]"), "{b:?}");
    let r = run_bounded(&mut common::bounded_exe(&d.path().join("built")), "built");
    assert!(r.stdout.starts_with('3'), "{r:?}");
}
