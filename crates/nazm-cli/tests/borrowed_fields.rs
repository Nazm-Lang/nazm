//! R1, 5b.9 (`docs/internal/performance-log.md`): a field of a local read only to be an operation's operand is
//! borrowed where it is — no reference taken, none given back — and it is never released, on the
//! normal path or on the failure path.
//!
//! Every implementation reclaims everything, nothing released twice, and the MIR names the
//! borrow.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::differential;

const PROGRAM: &str = "struct Bag { xs: Ints, name: Str, n: Int }

fn total(b: Bag) -> Int {
    let mut t = 0
    let mut i = 0
    while i < ints_len(b.xs) {
        t += ints_get(b.xs, i)
        i += 1
    }
    t + str_len(b.name) + b.n
}

fn main(out: OutCap) -> Int ! {io} {
    let xs = ints_new()
    ints_push(xs, 3)
    ints_push(xs, 4)
    let b = Bag(xs: xs, name: \"bag\", n: 1)
    println(total(b))
    println(ints_get(b.xs, 5))
    0
}
";

#[test]
fn a_borrowed_field_is_released_by_nobody_on_either_path() {
    // The last read fails (`N0405`) with the borrowed `b.xs` live: the failure path releases what
    // the frames own, and the borrow is not among them.
    let runs = differential::everywhere(&[("prog.nz", PROGRAM)]);
    let first = differential::memory_line(&runs[0].1);
    for (label, r) in &runs {
        assert_eq!(r.code, Some(2), "{label}: {r:?}");
        assert_eq!(r.stdout, "11\n", "{label}");
        assert!(r.stderr.contains("N0405"), "{label}: {r:?}");
        let line = differential::memory_line(r);
        assert_eq!(line, first, "{label}");
        assert!(
            line.split_whitespace()
                .filter(|w| w.starts_with("live="))
                .all(|w| w == "live=0"),
            "{label}: {line}"
        );
    }
}

#[test]
fn the_mir_names_the_borrow() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prog.nz"), PROGRAM).unwrap();
    let r = differential::nazm(d.path(), &["mir", "prog.nz"]);
    assert_eq!(r.code, Some(0), "{r:?}");
    assert!(r.stdout.contains("borrow"), "{}", r.stdout);
}
