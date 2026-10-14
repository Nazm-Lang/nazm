//! R1.5 (`docs/spec.md`, *Shorter forms*): each spelling means what the longer one did.
//!
//! D1 — a result type written after `:`, in a function, a trait method, a function type and a
//! function expression; `->` still read beside it.

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

fn nazm(dir: &Path, args: &[&str]) -> Ran {
    run_bounded(Command::new(NAZM).current_dir(dir).args(args), "nazm")
}

#[test]
fn a_result_type_after_a_colon_means_the_arrow_everywhere() {
    differential::agrees(
        "trait Shape { fn area(self: Self): Int; fn name(self: Self): Str { \"shape\" } }
struct Sq { side: Int, }
impl Shape for Sq { fn area(self: Sq): Int { self.side * self.side } }

fn fib(n: Int): Int {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn apply(f: fn(Int): Int, x: Int): Int { f(x) }

fn old(n: Int) -> Int { n + 1 }

fn parse(s: Str): Result[Int, Str] {
    if s == \"\" { Err(\"empty\") } else { Ok(str_len(s)) }
}

fn main(out: OutCap): Int ! {io} {
    let twice = fn(n: Int): Int { n * 2 }
    let k = match parse(\"abc\") { Ok(n) => n, Err(e) => 0 }
    print(f\"{fib(15)} {apply(twice, 21)} {Sq(side: 3).area()} {Sq(side: 1).name()} {old(1)} {k}\\n\")
    0
}
",
        "610 42 9 shape 2 3\n0",
    );
}

#[test]
fn fmt_keeps_the_result_marker_as_written_until_r2() {
    // The tree's own sources stay in the forms the Nazm-written compiler reads until R2, and
    // the formatter holds them as written; it neither adds nor removes a `:` or a `->`.
    let d = tempfile::tempdir().unwrap();
    let src =
        "fn a(n: Int): Int { n }\nfn b(n: Int) -> Int { n }\nfn main() { println(a(1) + b(2)) }\n";
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let r = nazm(d.path(), &["fmt", "--check", "p.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
}

#[test]
fn a_colon_without_a_type_is_refused_where_the_type_was_expected() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn f(n: Int): { n }\nfn main() { }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert!(r.stderr.contains("type"), "{r:?}");
}
