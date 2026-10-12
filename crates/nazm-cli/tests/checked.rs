//! R1b, L13a: checked arithmetic (`docs/spec.md`, *Numbers*, **Functions**).
//!
//! `checked_add`, `checked_sub` and `checked_mul` give `Some` of the exact result, or `None` where
//! it does not fit its type. The tests ask every implementation for the edges — each bound, `-1`
//! times the minimum, zero — at `Int`, `Int8` and `UInt8`, with nothing live; and hold the class
//! and the prelude name.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

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
        ("interpreter", &["run", "prog.nz"][..]),
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
        let exe = d.path().join(label.replace([' ', '-'], ""));
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

const CHECKED: &str = "fn show(o: Option[Int]) -> Str { match o { Option.Some(value: v) => num_to_str(v), Option.None() => \"-\", } }
fn show8(o: Option[Int8]) -> Str { match o { Option.Some(value: v) => num_to_str(Int(v)), Option.None() => \"-\", } }
fn showu(o: Option[UInt8]) -> Str { match o { Option.Some(value: v) => num_to_str(Int(v)), Option.None() => \"-\", } }
fn line(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, \"\\n\")) }
fn main(out: OutCap) -> Int ! {io} {
    let big = 9223372036854775807;
    let small = 0 - 9223372036854775807 - 1;
    line(out, str_concat(show(checked_add(1, 2)), str_concat(show(checked_add(big, 1)), show(checked_sub(small, 1)))));
    line(out, str_concat(show(checked_mul(3, 4)), str_concat(show(checked_mul(big, 2)), str_concat(show(checked_mul(0 - 1, small)), show(checked_mul(0, big))))));
    line(out, str_concat(show8(checked_add(Int8(100), Int8(27))), str_concat(show8(checked_add(Int8(100), Int8(28))), show8(checked_mul(Int8(-1), Int8(-128))))));
    line(out, str_concat(showu(checked_sub(UInt8(3), UInt8(4))), str_concat(showu(checked_mul(UInt8(16), UInt8(15))), showu(checked_mul(UInt8(16), UInt8(16))))));
    line(out, str_concat(show(checked_mul(0 - 1, big)), show(checked_mul(4611686018427387904, 2))));
    0
}
";

#[test]
fn checked_arithmetic_finds_every_edge_everywhere() {
    for (label, r) in everywhere(CHECKED) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(
            r.stdout
                .starts_with("3--\n12--0\n127--\n-240-\n-9223372036854775807-\n"),
            "{label}: {}",
            r.stdout
        );
        if label != "lir oracle" {
            let line = report(&r);
            assert!(nothing_live(&line), "{label}: {line}");
        }
    }
}

fn run(src: &str) -> Ran {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["run", "p.nz"]),
        "nazm run",
    )
}

#[test]
fn checked_arithmetic_is_for_integers_and_its_names_are_prelude_names() {
    let r = run("fn main() -> Int { match checked_add(1.5, 2.0) { Option.Some(value: v) => 1, Option.None() => 0, } }\n");
    assert!(r.stderr.contains("error[N0300]"), "{r:?}");
    // A 1.0 program's own `checked_add` means what it meant.
    let r = run(
        "fn checked_add(a: Int, b: Int) -> Int { a - b }\nfn main() -> Int { checked_add(9, 2) }\n",
    );
    assert!(r.stdout.starts_with('7'), "{r:?}");
}
