//! R1, 5b.15 (`docs/spec.md`, *Methods and indexes by name*): a method no trait gives is the
//! function `<prefix>_m` by name, and `xs[i]` and `xs[i] = v` on a sequence are its get and set.
//!
//! The same output and memory report in every implementation, nothing live; a trait's method
//! still wins, and what has no function of the name is still refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

#[test]
fn methods_and_indexes_by_name_run_alike_everywhere() {
    differential::agrees(
        "use \"@std/text\"
use \"@std/hash\"
use \"@std/hashmap\"
use \"@std/show\"

struct P { x: Int }

// A trait's method of the name wins over a function by name.
trait Size { fn len(self: Self) -> Int; }
impl Size for P { fn len(self: P) -> Int { 100 + self.x } }
fn p_len(p: P) -> Int { 0 }

fn main(out: OutCap) -> Int ! {io} {
    let v = vec_new[Int]()
    v.push(10)
    v.push(20)
    v[1] = v[0] + 5
    v[0] += 1
    println(f\"{v[0]} {v[1]} {v.len()} {v.get(1)}\")
    let s = vec_new[Str]()
    s.push(\"a\")
    s[0] = f\"{s[0]}b\"
    println(s[0])
    let words = \"x y z\".split(\" \")
    println(f\"{strs_len(words)} {words[2]} {\"hello\".len()} {42.to_str().len()}\")
    let b = bytes_new(2)
    b[1] = UInt8(7)
    println(b[1])
    let m = hashmap_new[Str, Int]()
    m.set(\"k\", 5)
    match m.get(\"k\") {
        Some(x) => println(x),
        None => println(\"none\"),
    }
    println(P(x: 3).len())
    0
}
",
        "11 15 2 15\nab\n3 z 5 2\n7\n5\n103\n0",
    );
}

#[test]
fn what_has_no_function_of_the_name_is_refused() {
    for (src, code, why) in [
        (
            "fn main() -> Int { let v = vec_new[Int](); v.nothing() }\n",
            "N0605",
            "a method no trait gives and no function names",
        ),
        (
            "fn main() -> Int { let v = vec_new[Int](); v.push(\"a\"); 0 }\n",
            // What `vec_push(v, "a")` says, said of the method call.
            "N0355",
            "a by-name call's argument checked as the function's",
        ),
        (
            "struct Q { y: Int }\nfn main() -> Int { let q = Q(y: 1); q[0] }\n",
            "N0624",
            "an index on something that is neither an array nor a sequence",
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
