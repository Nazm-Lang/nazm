//! R1, 5b.16 and 5b.17 (`docs/spec.md`, *Conveniences for the first program*): `let x: T`,
//! inference from the type a result is wanted as, block comments, `loop`, `while let` and an
//! impl without a trait — each the spelling of something the language had.
//!
//! The same output and memory report in every implementation, nothing live; and each refusal
//! that stands.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

#[test]
fn the_conveniences_run_alike_everywhere() {
    differential::agrees(
        "/* A program /* with a nested */ block comment. */
struct Point { x: Int, y: Int }

impl Point {
    fn sum(self: Point) -> Int { self.x + self.y }
    fn scaled(self: Point, k: Int) -> Point { Point(x: self.x * k, y: self.y * k) }
}

fn empty() -> Vec[Int] { vec_new() }

fn popped(v: Vec[Int]) -> Option[Int] {
    if v.len() == 0 { None } else { Some(vec_pop(v)) }
}

fn main(out: OutCap) -> Int ! {io} {
    let p = Point(x: 2, y: 3)
    println(p.scaled(10).sum()) /* inline */
    let o: Option[Int] = None
    let n: UInt8 = 200
    let mut names: Vec[Str] = vec_new()
    names.push(\"x\")
    match o {
        Some(x) => println(x),
        None => println(f\"{n} {names.len()} {point_sum(p)}\"),
    }
    let mut k = 0
    loop {
        k += 1
        if k == 4 { break }
    }
    println(k)
    let v = empty()
    v.push(1)
    v.push(2)
    v.push(3)
    while let Some(x) = popped(v) {
        println(x)
    }
    0
}
",
        "50\n200 1 5\n4\n3\n2\n1\n0",
    );
}

#[test]
fn what_the_conveniences_do_not_make_valid_is_refused() {
    for (src, code, why) in [
        (
            "fn main() -> Int { let s: Int = \"a\"; 0 }\n",
            "N0300",
            "a value that is not its written type",
        ),
        (
            "fn main() -> Int { let v = vec_new(); 0 }\n",
            "N0354",
            "a parameter nothing settles",
        ),
        (
            "fn main() -> Int { /* never closed\n0 }\n",
            "N0007",
            "a block comment the file ends inside",
        ),
        (
            "struct P { x: Int }\nimpl P { fn twice(self: P) -> Int { self.x * 2 } }\nfn p_twice(p: P) -> Int { 0 }\nfn main() -> Int { 0 }\n",
            "N0203",
            "an impl's method and a function of the name it is given",
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
