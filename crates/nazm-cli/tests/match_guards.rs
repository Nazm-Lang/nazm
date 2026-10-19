//! R1, 5b.17 (`docs/user/spec.md`, *Patterns*, Guards): `P if G => E` is chosen where `P` matches and
//! `G`, with `P`'s names bound, is true; otherwise the arms below are tried.
//!
//! The same output and memory report in every implementation, nothing live — a guard binds a
//! payload that holds a string and is released; what a guard cannot cover is still refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

#[test]
fn guards_choose_alike_everywhere() {
    differential::agrees(
        "enum Shape { Circle(r: Int), Label(text: Str), Empty }

fn sign(n: Int) -> Str {
    match n {
        x if x < 0 => \"negative\",
        0 => \"zero\",
        x if x % 2 == 0 => \"even\",
        _ => \"odd\",
    }
}

fn describe(s: Shape) -> Str {
    match s {
        Shape.Circle(r: r) if r > 10 => \"big circle\",
        Shape.Circle(r: r) => f\"circle {r}\",
        Shape.Label(text: t) if t.len() > 3 => f\"long {t}\",
        Shape.Label(text: t) => f\"short {t}\",
        Shape.Empty => \"empty\",
    }
}

fn first_even(o: Option[Int]) -> Int {
    match o {
        Some(n) if n % 2 == 0 => n,
        Some(n) => n + 1,
        None => 0,
    }
}

fn main(out: OutCap) -> Int ! {io} {
    println(f\"{sign(0 - 3)} {sign(0)} {sign(4)} {sign(7)}\")
    println(describe(Shape.Circle(r: 12)))
    println(describe(Shape.Circle(r: 2)))
    println(describe(Shape.Label(text: \"hello\")))
    println(describe(Shape.Label(text: \"hi\")))
    println(describe(Shape.Empty))
    println(f\"{first_even(Some(4))} {first_even(Some(5))} {first_even(None)}\")
    let mut total = 0
    for i in 0..10 {
        match i {
            n if n % 3 == 0 => { total += n },
            _ => {},
        }
    }
    println(total)
    0
}
",
        "negative zero even odd\nbig circle\ncircle 2\nlong hello\nshort hi\nempty\n4 6 0\n18\n0",
    );
}

#[test]
fn what_a_guard_cannot_mean_is_refused() {
    for (src, code, why) in [
        (
            "fn main() -> Int { match 1 { n if n => 1, _ => 0 } }\n",
            "N0301",
            "a guard that is not a `Bool`",
        ),
        (
            "fn main() -> Int { match 1 { n if n > 0 => 1, 0 => 0 } }\n",
            "N0343",
            "a guarded arm covers nothing",
        ),
        (
            "fn main() -> Int { match 1 { _ => 0, n if n > 0 => 1 } }\n",
            "N0344",
            "a guarded arm below one that matches everything never runs",
        ),
        (
            "fn main() -> Int { match 1 { n if m > 0 => 1, _ => 0 } }\n",
            "N0200",
            "a guard sees only its pattern's names and those around the `match`",
        ),
    ] {
        let got = differential::codes(src);
        assert_eq!(
            got.first().map(String::as_str),
            Some(code),
            "{why}: {got:?}"
        );
    }
}
