//! R1a: string interpolation and fixed-point floats (`docs/user/spec.md`, *String interpolation*).
//!
//! Every implementation — the interpreter, the LIR oracle, LLVM at `-O0` and `-O2`, and Cranelift
//! — is asked for what the rules work out by hand, with every string reclaimed; then what is
//! refused, by which code.

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

fn nothing_live(r: &Ran) -> bool {
    r.stderr
        .lines()
        .find(|l| l.starts_with("nazm-memory:"))
        .is_some_and(|l| {
            l.split_whitespace()
                .filter(|w| w.starts_with("live="))
                .all(|w| w == "live=0")
        })
}

const INTERP: &str = r###"use "@std/show";

struct Point { x: Int, y: Int, }
impl Show for Point { fn show(self: Point) -> Str { f"({self.x}, {self.y})" } }

fn show(out: OutCap, s: Str) -> Int ! {io} { print(str_concat(s, "\n")) }
fn render[T: Show](v: T) -> Str { f"<{v}>" }

fn main(out: OutCap) -> Int ! {io} {
    let name = "ab";
    let ratio = 2.0 / 3.0;
    // Width and side, a number to the right by default and text to the left; precision; Show;
    // braces; a Bool.
    show(out, f"{name:<5}|{42:>5}|{ratio:.3}|{Point(x: 1, y: -2)}|{{x}}|{true}");
    show(out, f"{name:5}|{7:3}|{Float32(0.125):.2}|{1.5}|{UInt8(255)}|{-0.0:.1}|{0.375:.2}");
    // Through a bound; a nested string and call; the built-ins directly.
    show(out, render(Point(x: 3, y: 4)));
    show(out, f"{str_concat("q", "r")}{float_to_str_fixed(1e10, 0)}");
    show(out, str_concat(str_pad_start("7", 3, "0"), str_pad_end("x", 3, ".")));
    // Each hole once, in order.
    let mut n = 0;
    let marks = vec_new[Int]();
    show(out, f"{vec_push(marks, 1)}-{vec_push(marks, 2)}");
    // Multi-line, with a hole on each line.
    show(out, f"""
        a = {n}
          b = {name}
        """);
    0
}
"###;

/// By hand: `0.125` and `0.375` to two places are exact ties, to even — `0.12` and `0.38`.
const EXPECTED: &str = "ab   |   42|0.667|(1, -2)|{x}|true\nab   |  7|0.12|1.5|255|-0.0|0.38\n<(3, 4)>\nqr10000000000\n007x..\n1-2\na = 0\n  b = ab\n";

#[test]
fn interpolation_means_the_same_everywhere_and_is_reclaimed() {
    for (label, r) in everywhere(INTERP) {
        assert_eq!(r.code, Some(0), "{label}: {r:?}");
        assert!(r.stdout.starts_with(EXPECTED), "{label}: {:?}", r.stdout);
        if label != "lir oracle" {
            assert!(nothing_live(&r), "{label}: {}", r.stderr);
        }
    }
}

#[test]
fn a_built_in_outside_its_range_stops_the_program_everywhere() {
    for (src, says) in [
        (
            "fn main() -> Int { str_len(float_to_str_fixed(1.0, 31)) }\n",
            "0 to 30 decimals",
        ),
        (
            "fn main() -> Int { str_len(str_pad_start(\"a\", 3, \"ab\")) }\n",
            "fill is one byte",
        ),
    ] {
        for (label, r) in everywhere(src) {
            assert_eq!(r.code, Some(2), "{label}: {r:?}");
            assert!(
                r.stderr.contains("N0405") && r.stderr.contains(says),
                "{label}: {}",
                r.stderr
            );
        }
    }
}

/// `nazm check` on `src`: every error code, in order, and the first message.
fn refused(src: &str) -> (Vec<String>, String) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = run_bounded(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        "nazm check",
    );
    let codes = r
        .stderr
        .split("error[")
        .skip(1)
        .map(|s| s[..5].to_owned())
        .collect();
    let first = r
        .stderr
        .lines()
        .find(|l| l.starts_with("error["))
        .unwrap_or("")
        .to_owned();
    (codes, first)
}

#[test]
fn what_an_interpolation_refuses_and_by_which_code() {
    for (body, code, says) in [
        ("let s = f\"{}\"; 0", "N0006", "no expression"),
        ("let s = f\"a}b\"; 0", "N0006", "closes nothing"),
        ("let s = f\"{1:x}\"; 0", "N0006", "format"),
        (
            "let s = f\"{1:.2}\"; 0",
            "N0006",
            "a precision is a float's",
        ),
        (
            "let s = f\"{let x = 1; x}\"; 0",
            "N0006",
            "not one expression",
        ),
        (
            "let v = vec_new[Int](); let s = f\"{v}\"; 0",
            "N0300",
            "`Ints`", // `Vec[Int]` by its shorter name (R1, 5b.18)
        ),
        ("let s = f\"{nope}\"; 0", "N0200", "nope"),
    ] {
        let src = format!("fn main() -> Int {{ {body} }}\n");
        let (codes, first) = refused(&src);
        assert_eq!(
            codes.first().map(String::as_str),
            Some(code),
            "{src}: {codes:?}"
        );
        assert_eq!(codes.len(), 1, "{src}: one mistake, one error: {codes:?}");
        assert!(first.contains(says), "{src}: {first}");
    }
    // A name `f` is still a name, and a module's own `str_pad_start` hides the built-in.
    let (codes, _) = refused("fn main() -> Int { let f = 1; f }\n");
    assert!(codes.is_empty(), "{codes:?}");
    let (codes, _) =
        refused("fn str_pad_start(n: Int) -> Int { n }\nfn main() -> Int { str_pad_start(3) }\n");
    assert!(codes.is_empty(), "{codes:?}");
}
