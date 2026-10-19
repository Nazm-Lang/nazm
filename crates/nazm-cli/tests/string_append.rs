//! R1b, D2: linear-time string building (`docs/user/spec.md`, *What this costs*).
//!
//! `s = str_concat(s, t)` on a `let mut` binding appends in place where `s`'s storage is its own
//! and has room, and otherwise copies into storage with room to grow. The tests ask every
//! implementation for the same bytes — strings sharing storage, a slice kept across an append, a
//! string appended to itself, a literal — with nothing live, and count what a natively compiled
//! loop of appends allocates.

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

/// The program under the interpreter and the LIR oracle, and built and run by each backend,
/// each with the memory report asked for.
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
            run_bounded(
                Command::new(NAZM)
                    .current_dir(d.path())
                    .args(args)
                    .env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
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
            run_bounded(
                common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
                label,
            ),
        ));
    }
    out
}

/// The memory report's line, which every implementation but the LIR oracle writes when asked.
fn report(r: &Ran) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .unwrap_or_else(|| panic!("no memory report: {r:?}"))
        .to_owned()
}

fn nothing_live(line: &str) -> bool {
    line.split_whitespace()
        .filter(|w| w.starts_with("live="))
        .all(|w| w == "live=0")
}

const APPENDS: &str = "fn main(out: OutCap) -> Int ! {io} {
    let mut s = \"\";
    let mut i = 0;
    while i < 2000 { s = str_concat(s, \"x\"); i = i + 1; }
    let mut d = \"ab\";
    d = str_concat(d, d);
    d = str_concat(d, d);
    let mut k = \"hello\";
    let kept = str_slice(k, 0, 5);
    k = str_concat(k, \"!!\");
    // Two names for one string's storage: each append is its own.
    let mut a = str_concat(\"base\", \"\");
    a = str_concat(a, \"-\");
    let mut b = a;
    a = str_concat(a, \"1\");
    b = str_concat(b, \"2\");
    print(str_concat(a, str_concat(\" \", str_concat(b, \"\\n\"))));
    let lit = \"lit\";
    let mut m = lit;
    m = str_concat(m, \"-x\");
    print(str_concat(d, str_concat(\" \", str_concat(kept, str_concat(\" \", str_concat(k, str_concat(\" \", str_concat(lit, str_concat(\" \", str_concat(m, \"\\n\"))))))))));
    str_len(s)
}
";

#[test]
fn an_append_is_a_concat_everywhere_and_nothing_is_live() {
    for (label, r) in everywhere(APPENDS) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout
                .starts_with("base-1 base-2\nabababab hello hello!! lit lit-x\n2000"),
            "{label}: {}",
            r.stdout
        );
        if label != "lir oracle" {
            let line = report(&r);
            assert!(nothing_live(&line), "{label}: {line}");
        }
    }
}

#[test]
fn a_loop_of_appends_allocates_a_logarithmic_number_of_strings() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main() -> Int { let mut s = \"\"; let mut i = 0; \
         while i < 100000 { s = str_concat(s, \"x\"); i = i + 1; } str_len(s) }\n",
    )
    .unwrap();
    for backend in ["llvm", "cranelift"] {
        let exe = d.path().join(crate::common::exe_name(backend));
        let b = run_bounded(
            Command::new(NAZM).current_dir(d.path()).args([
                "build",
                "--no-cache",
                "--backend",
                backend,
                "p.nz",
                "-o",
                exe.to_str().unwrap(),
            ]),
            "nazm build",
        );
        assert_eq!(b.code, Some(0), "{backend}: {}", b.stderr);
        let r = run_bounded(
            common::bounded_exe(&exe).env(nazm_lir::MEMORY_VARIABLE, "1"),
            backend,
        );
        assert!(r.stdout.starts_with("100000"), "{backend}: {r:?}");
        let line = report(&r);
        let allocated: u64 = line
            .split("strings allocated=")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(u64::MAX);
        // Doubling: about log2(100000) strings, not one per append.
        assert!(allocated < 64, "{backend}: {line}");
        assert!(nothing_live(&line), "{backend}: {line}");
    }
}
