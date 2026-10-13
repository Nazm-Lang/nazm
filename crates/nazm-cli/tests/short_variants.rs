//! R1, 5b.14 (`docs/spec.md`, *Short variants*): an enum's construction infers its type
//! arguments; `Some`, `None`, `Ok` and `Err` name the prelude's variants; a variant with no
//! payload needs no `()`.
//!
//! The same output and memory report in every implementation, nothing live; where nothing
//! decides a type argument it is still `N0352`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

#[test]
fn short_variants_build_and_match_alike_everywhere() {
    differential::agrees(
        "enum Color { Red, Green, Blue }

fn name(c: Color) -> Str {
    match c {
        Color.Red => \"red\",
        Color.Green => \"green\",
        Color.Blue => \"blue\",
    }
}

fn half(n: Int) -> Option[Int] {
    if n % 2 == 0 { Some(n / 2) } else { None }
}

fn parse(s: Str) -> Result[Int, Str] {
    if s == \"one\" { Ok(1) } else { Err(f\"not a number: {s}\") }
}

fn first(o: Option[Str]) -> Str {
    match o {
        Some(t) => t,
        None => \"none\",
    }
}

fn main(out: OutCap) -> Int ! {io} {
    println(name(Color.Green))
    let c = Color.Blue
    println(name(c))
    match half(8) {
        Some(h) => println(h),
        None => println(\"odd\"),
    }
    match half(7) {
        Some(h) => println(h),
        None => println(\"odd\"),
    }
    match parse(\"two\") {
        Ok(v) => println(v),
        Err(e) => println(e),
    }
    // From the payload, with nothing around it; as an argument; written in full still.
    let o = Some(\"x\")
    if let Some(t) = o { println(t) }
    println(first(None))
    println(first(Some(\"y\")))
    let w = Option[Int].Some(value: 5)
    match w {
        Option.Some(value: v) => println(v),
        Option.None() => println(0),
    }
    0
}
",
        "green\nblue\n4\nodd\nnot a number: two\nx\nnone\ny\n5\n0",
    );
}

#[test]
fn what_nothing_decides_is_still_written() {
    for (src, code, why) in [
        (
            "fn main() -> Int { let o = None; 0 }\n",
            "N0352",
            "`None` with nothing to say what it holds",
        ),
        (
            "fn main() -> Int { let r = Ok(2); 0 }\n",
            "N0352",
            "`Ok` with nothing to say what its error is",
        ),
        (
            "fn f() -> Option[Int] { Some(\"a\") }\nfn main() -> Int { 0 }\n",
            "N0300",
            "a payload of the wrong type for the type wanted",
        ),
        (
            "enum C { R(n: Int) }\nfn main() -> Int { match C.R(n: 1) { C.R => 0 } }\n",
            "N0334",
            "a variant pattern without `()` that has a payload",
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
