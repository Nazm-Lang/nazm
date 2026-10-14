//! R1, 5b.18 (`docs/spec.md`, *Sequences are shared, mutable, and never freed*, One list type):
//! `Ints` is `Vec[Int]` and `Strs` is `Vec[Str]`, so each name is accepted where the other is
//! wanted and every family of built-ins applies to both.
//!
//! The same output and memory report in every implementation, nothing live.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

#[test]
fn one_list_type_runs_alike_everywhere() {
    differential::agrees(
        "use \"@std/text\"

fn total(xs: Vec[Int]) -> Int {
    let mut t = 0
    for x in xs { t += x }
    t
}

fn names() -> Strs { \"a b c\".split(\" \") }

fn main(out: OutCap) -> Int ! {io} {
    let xs = ints_new()
    ints_push(xs, 3)
    vec_push(xs, 4)
    xs.push(5)
    let v: Vec[Int] = xs
    println(f\"{total(xs)} {vec_len(v)} {ints_get(v, 2)} {v[0]}\")
    let s: Vec[Str] = names()
    strs_push(s, \"d\")
    vec_push(s, \"e\")
    println(f\"{str_join(s, \"-\")} {strs_len(s)} {vec_get(s, 0)}\")
    let w = vec_new[Str]()
    w.push(\"x\")
    let back: Strs = w
    println(f\"{strs_len(back)} {strs_pop(back)} {vec_len(w)}\")
    0
}
",
        "12 3 5 3\na-b-c-d-e 5 a\n1 x 0\n0",
    );
}

#[test]
fn diagnostics_name_the_shorter_name() {
    let got = differential::codes("fn main() -> Int { let v: Vec[Int] = strs_new(); 0 }\n");
    assert_eq!(got, ["N0300"]);
}
