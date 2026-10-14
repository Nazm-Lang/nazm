//! R1.5 (`docs/spec.md`, *Shorter forms*): each spelling means what the longer one did.
//!
//! D1 — a result type written after `:`, in a function, a trait method, a function type and a
//! function expression; `->` still read beside it.
//! D2 — an expression body, `= e`, and a private function's result type taken from it.
//! D3 — a short closure, `|a, b| a < b`, its types taken from the function type expected.

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

#[test]
fn an_expression_body_and_its_inferred_result_type_mean_the_block_everywhere() {
    differential::agrees(
        // `main` first, and `quad` before what it calls: each inferred type is settled before
        // any caller is checked, wherever the caller is written.
        "fn main(out: OutCap): Int ! {io} {
    let f = quad
    print(f\"{double(21)} {add(2, 3)} {greet(\"nazm\")} {f(5)} {even(4)} {first(7, 8)} {pick(Some(9))}\\n\")
    0
}

fn quad(n: Int) = add(double(n), double(n))
fn double(n: Int): Int = n * 2
fn add(a: Int, b: Int) = a + b
fn greet(name: Str) = f\"hello {name}\"
fn even(n: Int) = n % 2 == 0
fn first[T](a: T, b: T) = a
fn pick(o: Option[Int]) = match o { Some(v) => v, None => 0 }
",
        "42 5 hello nazm 20 true 7 9\n0",
    );
}

#[test]
fn a_result_type_that_must_be_written_is_refused_by_name() {
    let d = tempfile::tempdir().unwrap();
    for (src, why) in [
        ("pub fn p(n: Int) = n\nfn main() { }\n", "`pub`"),
        (
            "fn r(n: Int) = if n < 1 { 0 } else { r(n - 1) }\nfn main() { }\n",
            "reaches itself",
        ),
        (
            "fn a(n: Int) = b(n)\nfn b(n: Int) = a(n)\nfn main() { }\n",
            "reaches itself",
        ),
        (
            "trait T { fn m(self: Self): Int; }\nimpl T for Int { fn m(self: Int) = self }\nfn main() { }\n",
            "method",
        ),
    ] {
        std::fs::write(d.path().join("p.nz"), src).unwrap();
        let r = nazm(d.path(), &["check", "p.nz"]);
        assert_ne!(r.code, Some(0), "{src}: {r:?}");
        assert!(r.stderr.contains("N0634"), "{src}: {r:?}");
        assert!(r.stderr.contains(why), "{src}: {r:?}");
    }
}

#[test]
fn a_short_closure_takes_its_types_from_what_is_expected_everywhere() {
    differential::agrees(
        "fn apply(f: fn(Int, Int) -> Bool, a: Int, b: Int) -> Bool { f(a, b) }
fn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }

fn main(out: OutCap): Int ! {io} {
    let inc = |n: Int| n + 1
    let k = 10
    let add_k = |n: Int| { n + k }
    let zero = || 0
    let shout = |s: Str| str_concat(s, \"!\")
    let typed: fn(Int) -> Int = |x| x - 1
    print(f\"{apply(|a, b| a < b, 1, 2)} {apply(|a, b| a > b, 1, 2)} {twice(|x| x * 3, 2)} \")
    print(f\"{inc(41)} {add_k(5)} {zero()} {typed(1)} {shout(\"hi\")}\\n\")
    0
}
",
        "true false 18 42 15 0 0 hi!\n0",
    );
}

#[test]
fn a_short_closure_parameter_with_nothing_to_take_its_type_from_is_refused() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main() { let f = |a, b| a < b }\n",
    )
    .unwrap();
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert_ne!(r.code, Some(0), "{r:?}");
    assert_eq!(r.stderr.matches("N0635").count(), 2, "{r:?}");
}
