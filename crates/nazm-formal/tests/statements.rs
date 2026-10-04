//! N97: the statement core's properties over every program of its bound (`docs/formal-core.md` §2).
//!
//! S1 soundness — a well-formed program is never stuck; S2 determinism; S3 correspondence — for
//! every program that finishes within the fuel, Nazm's interpreter, run on its source, gives the
//! same value or fails with the same code; S4 the checker accepts every program of the bound. A
//! bounded, machine-checked model, not a proof.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_formal::statements::{enumerate, run, to_source, well_formed, Outcome2};
use nazm_formal::{Outcome, Value};

fn nazm_says(source: &str) -> Outcome {
    match nazm_core::run(source) {
        nazm_core::Outcome::Value(nazm_core::eval::Value::Int(n)) => Outcome::Value(Value::Int(n)),
        nazm_core::Outcome::RuntimeFailed(d) => Outcome::Trap(match d.code.as_str() {
            "N0400" => nazm_formal::OVERFLOW,
            "N0401" => nazm_formal::DIVIDE_BY_ZERO,
            other => panic!("an unexpected failure {other}: {source}"),
        }),
        other => panic!("{other:?}: {source}"),
    }
}

#[test]
fn s1_s2_a_well_formed_program_is_never_stuck_and_runs_one_way() {
    for p in enumerate() {
        assert!(well_formed(&p), "{}", to_source(&p));
        let first = run(&p).unwrap_or_else(|| panic!("stuck: {}", to_source(&p)));
        assert_eq!(run(&p), Some(first), "{}", to_source(&p));
    }
}

#[test]
fn s3_the_interpreter_means_what_the_statement_semantics_says() {
    let (mut agreed, mut traps, mut out_of_fuel) = (0usize, 0usize, 0usize);
    for p in enumerate() {
        let src = to_source(&p);
        match run(&p).unwrap() {
            Outcome2::Done(formal) => {
                assert_eq!(nazm_says(&src), formal, "{src}");
                agreed += 1;
                traps += usize::from(matches!(formal, Outcome::Trap(_)));
            }
            // No claim: the interpreter is not asked about a program the semantics did not finish.
            Outcome2::OutOfFuel => out_of_fuel += 1,
        }
    }
    // The bound's shape is part of the claim.
    assert_eq!(agreed + out_of_fuel, enumerate().len());
    assert!(
        agreed > 6_000 && traps > 500,
        "{agreed} agreed, {traps} traps"
    );
    eprintln!("statement core: {agreed} agreed ({traps} traps), {out_of_fuel} out of fuel");
}

#[test]
fn s4_the_checker_accepts_every_program_of_the_bound() {
    for p in enumerate() {
        let src = to_source(&p);
        let d = nazm_core::check_source(&src);
        assert!(d.is_empty(), "{src}: {d:?}");
    }
}

#[test]
fn the_statement_bound_covers_what_it_says() {
    let all = enumerate();
    assert_eq!(all.len(), 5 * 3 * 6 * 8 * 5 * 3 + 5 * 6 * 8 * 8 + 4 * 5 * 9);
    assert_eq!(all.iter().filter(|p| p.f.is_some()).count(), 180);
}
