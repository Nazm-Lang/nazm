//! R1, 5b.12 (`docs/user/spec.md`, *Mutation is opt-in and visible at the binding*): `place op= e`
//! is `place = place op e`, for each of the ten operators, on a binding, a field and an element.
//!
//! The same values, failures and memory report in every implementation; what it cannot mean is
//! refused by name; and each operator is one token, which `nazm fmt` keeps.

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
fn every_operator_writes_what_its_long_form_writes_everywhere() {
    differential::agrees(
        "struct P { x: Int, y: Int }

fn main(out: OutCap) -> Int ! {io} {
    let mut a = 10
    a += 5
    a -= 3
    a *= 4
    a /= 3
    a %= 7
    let mut b = 12
    b &= 10
    b |= 1
    b ^= 3
    b <<= 2
    b >>= 1
    let mut w = UInt8(250)
    w -= UInt8(9)
    let mut p = P(x: 1, y: 2)
    p.x += 40
    p.y *= p.x
    let mut arr = [1, 2, 3]
    let i = 1
    arr[i] += 10
    arr[0] *= 7
    arr[2] -= arr[i]
    print(f\"{a} {b} {w} {p.x} {p.y} {arr[0]} {arr[1]} {arr[2]}\\n\")
    0
}
",
        "2 20 241 41 82 7 12 -9\n0",
    );
}

#[test]
fn a_compound_assignment_fails_as_its_operator_does() {
    for (label, r) in differential::everywhere(&[(
        "prog.nz",
        "fn main() -> Int {\n    let mut m = 9223372036854775807\n    m += 1\n    m\n}\n",
    )]) {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert!(r.stderr.contains("error[N0400]"), "{label}: {r:?}");
    }
}

#[test]
fn what_a_compound_assignment_cannot_mean_is_refused_by_name() {
    for (src, code, why) in [
        (
            "fn k() -> Int { 0 }\nfn main() -> Int {\n    let mut v = [1, 2]\n    v[k()] += 1\n    0\n}\n",
            "N0100",
            "an index computed twice",
        ),
        (
            "fn main() -> Int { let x = 1; x += 1; x }\n",
            "N0310",
            "an immutable binding",
        ),
        (
            "fn main() -> Int { let mut x = 1; x + = 1; x }\n",
            "N0100",
            "an operator written as two tokens",
        ),
        (
            "fn main() -> Int { let mut s = \"a\"; s *= \"b\"; 0 }\n",
            "N0304",
            "an operator its operands do not have",
        ),
    ] {
        assert_eq!(differential::codes(src), [code], "{why}");
    }
}

#[test]
fn each_operator_is_one_token_and_the_formatter_keeps_it() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("p.nz"),
        "fn main() -> Int {\nlet mut a = 1\na<<=3\na>>=1\na+=a<=4\na\n}\n",
    )
    .unwrap();
    let f = nazm(d.path(), &["fmt", "p.nz"]);
    assert_eq!(f.code, Some(0), "{f:?}");
    let laid = std::fs::read_to_string(d.path().join("p.nz")).unwrap();
    assert!(laid.contains("    a <<= 3\n    a >>= 1\n"), "{laid}");
    // `a <= 4` is a comparison, `Bool`, so adding it to an `Int` is refused: `<=` stayed `<=`.
    let r = nazm(d.path(), &["check", "p.nz"]);
    assert!(
        r.stderr.contains("N0304") || r.stderr.contains("N0300"),
        "{r:?}"
    );
}
