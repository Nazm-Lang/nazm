//! R1, 5b.11 and 5b.19 (`docs/spec.md`, *`else if`, and a line break for `;`*): `else if` means
//! `else { if … }`, and a line break — or a block's `}` — ends a statement where a `;` would.
//!
//! Each program is asked of the interpreter and of each backend, with the same output and memory
//! report; what still needs a `;` is refused by name; and `nazm fmt` lays a program without
//! semicolons out as it lays one with them.

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
fn else_if_chains_mean_nested_ifs_everywhere() {
    differential::agrees(
        "fn size(n: Int) -> Int {
    if n < 10 { 1 } else if n < 100 { 2 } else if n < 1000 { 3 } else { 4 }
}

fn sign(o: Option[Int]) -> Int {
    if let Option.Some(value: v) = o {
        if v > 0 { 1 } else if v < 0 { 0 - 1 } else { 0 }
    } else if true {
        9
    } else {
        8
    }
}

fn pick(o: Option[Int]) -> Int {
    if false { 1 } else if let Option.Some(value: v) = o { v } else { 7 }
}

fn main(out: OutCap) -> Int ! {io} {
    let steps = size(5) * 1000 + size(50) * 100 + size(500) * 10 + size(5000);
    // As a statement: no value wanted, and no final `else`.
    if steps > 99999 { print(\"big\\n\"); } else if steps > 999 { print(\"mid\\n\"); }
    print(f\"{steps} {sign(Option[Int].Some(value: 0 - 3))} {sign(Option[Int].None())} \
{pick(Option[Int].Some(value: 4))} {pick(Option[Int].None())}\\n\");
    0
}
",
        "mid\n1234 -1 9 4 7\n0",
    );
}

const WITHOUT: &str = "use \"@std/text\"

const LIMIT: Int = 5

// Every statement on its own line, without a `;`.
fn total(n: Int) -> Int {
    let mut s = 0
    let mut i = 0
    while i < n {
        i = i + 1
        if i == 2 { continue }
        s = s +
            i
    }
    s
}

fn main(out: OutCap) -> Int ! {io} {
    let a = total(LIMIT) // a comment
    let b = a
        * 2
    let words = text_split(\"a b c\", \" \")
    print(f\"{a} {b} {strs_len(words)}\\n\")
    let mut k = 0
    while true {
        k = k + 1
        if k == 3 { break }
    }
    let kind = if a > 10 { \"big\" } else if a > 5 { \"mid\" } else { \"small\" }
    print(kind); print(\"\\n\")
    k
}
";

#[test]
fn a_program_without_semicolons_runs_alike_everywhere() {
    differential::agrees(WITHOUT, "13 26 3\nbig\n3");
}

#[test]
fn a_value_nobody_wants_is_discarded_and_reclaimed_everywhere() {
    // A loop body's last line, and the branches of an `if` that is a statement, ending in
    // values of different types or in none.
    differential::agrees(
        "fn name(i: Int) -> Str { f\"n{i}\" }

fn main(out: OutCap) -> Int ! {io} {
    let mut i = 0
    while i < 3 {
        i = i + 1
        name(i)
    }
    for k in 0..3 {
        print(f\"{k}\")
        name(k)
    }
    print(\"\\n\")
    if i > 5 { name(1) } else if i > 1 { print(\"two\\n\") } else { name(2) }
    if i > 1 { name(3) } else if i > 0 { name(4) }
    i
}
",
        "012\ntwo\n3",
    );
    // Where a value is wanted, the branches must still agree.
    assert_eq!(
        differential::codes("fn main() -> Int { let x = if true { 1 } else { \"a\" }; 0 }\n"),
        ["N0302"]
    );
    assert_eq!(
        differential::codes(
            "fn f(c: Bool) -> Int { if c { 1 } else if !c { 2 } }\nfn main() -> Int { f(true) }\n"
        ),
        ["N0302"]
    );
}

#[test]
fn what_a_line_break_does_not_end_is_still_refused() {
    for (src, why) in [
        (
            "fn main() -> Int {\n    let a = 1 let b = 2;\n    b\n}\n",
            "two statements on one line",
        ),
        ("fn main() -> Int {\n    let a = 1 a\n}\n", "a value after a `let`"),
        (
            "struct P { x: Int, }\nfn main() -> Int {\n    let p = P(x: 1);\n    let n = p.\n        x;\n    n\n}\n",
            "a member's name on the next line",
        ),
    ] {
        let codes = differential::codes(src);
        assert_eq!(codes, ["N0100"], "{why}");
    }
}

#[test]
fn the_formatter_lays_out_a_program_without_semicolons_and_keeps_it() {
    let d = tempfile::tempdir().unwrap();
    let ugly = "fn main(out: OutCap) -> Int ! {io} {\nlet a = 1\n      let b = a +\n2\nif a > 0 { print(\"x\\n\") } else if b > 0 { print(\"y\\n\") }\nb\n}\n";
    std::fs::write(d.path().join("p.nz"), ugly).unwrap();
    let f = nazm(d.path(), &["fmt", "p.nz"]);
    assert_eq!(f.code, Some(0), "{f:?}");
    let once = std::fs::read_to_string(d.path().join("p.nz")).unwrap();
    assert_eq!(
        once,
        "fn main(out: OutCap) -> Int ! { io } {\n    let a = 1\n    let b = a +\n            2\n    \
         if a > 0 { print(\"x\\n\") } else if b > 0 { print(\"y\\n\") }\n    b\n}\n"
    );
    let again = nazm(d.path(), &["fmt", "--check", "p.nz"]);
    assert_eq!(again.code, Some(0), "{again:?}");
    let r = nazm(d.path(), &["run", "p.nz"]);
    assert_eq!(r.stdout, "x\n3\n", "{r:?}");
}
