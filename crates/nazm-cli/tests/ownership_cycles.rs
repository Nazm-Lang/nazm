//! N9's rule, held from outside the compiler: a value type that contains itself inline — directly,
//! through another record, or through an enum's payload — is refused (`N0336`) before anything
//! else looks at it.
//!
//! # Why a child process with a deadline
//!
//! The rule is what keeps every later walk over a type's layout finite. With the refusal gone the
//! checker does not answer wrongly, it does not answer at all: the in-process test in
//! `nazm-core/tests/enums.rs` then runs until something outside kills it, which a mutation campaign
//! can only record as "no result" (M2-C1). Run as `nazm check` under `common::run_within`, the same
//! loss is a failure of this test, at its own deadline, with the test process unharmed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::common;

use std::process::Command;
use std::time::Duration;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

/// Ample for a refusal the checker reaches in milliseconds; short enough that a lost refusal fails
/// the test well inside any campaign's killer deadline.
const LIMIT: Duration = Duration::from_secs(30);

/// `src`, checked by the binary within [`LIMIT`]: refused, and with `N0336`.
fn refused_as_a_cycle(src: &str) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), src).unwrap();
    let ran = common::run_within(
        Command::new(NAZM)
            .current_dir(d.path())
            .args(["check", "p.nz"]),
        LIMIT,
    )
    .unwrap_or_else(|| {
        panic!("`nazm check` did not finish within {LIMIT:?}: the cycle was not refused\n{src}")
    });
    assert_eq!(ran.code, Some(1), "{src}\n{}", ran.stderr);
    assert!(ran.stderr.contains("N0336"), "{src}\n{}", ran.stderr);
}

#[test]
fn every_inline_cycle_is_refused_within_a_bound() {
    for src in [
        "struct Node { next: Node, }\nfn main() -> Int { 0 }\n",
        "struct A { b: B, }\nstruct B { a: A, }\nfn main() -> Int { 0 }\n",
        "struct A { b: B, }\nenum B { V(a: A), }\nfn main() -> Int { 0 }\n",
        "enum A { V(b: B), }\nenum B { V(a: A), }\nfn main() -> Int { 0 }\n",
        // A type outside the cycle that holds one inside it (found by fuzzing): the depth walk
        // must not follow the cycle forever.
        "struct V { r: V, }\nstruct M { r: V, }\nfn main() -> Int { 0 }\n",
        // The same through the task-safety walk: a channel of an enum that holds a type that
        // holds itself (found by fuzzing a mutated `@std/metrics`).
        "struct B { b: B, }\nenum M { Of(h: B), }\nfn f() -> Int { let c = chan_new_of[M](4); 0 }\nfn main() -> Int { 0 }\n",
    ] {
        refused_as_a_cycle(src);
    }
}
