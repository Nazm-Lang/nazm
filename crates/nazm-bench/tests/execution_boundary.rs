//! Workspace-management and grading-integrity tests.
//!
//! These use real directories, real files, and real processes — not fixtures. The unit
//! tests prove the accounting is right about numbers it is handed; these prove the harness
//! is handed the right numbers in the first place, which is the part that cannot be
//! checked synthetically.
//!
//! **Scope, stated precisely.** These test the harness's own behaviour. They are *not*
//! evidence that a candidate process is sandboxed: every check here runs in the test
//! process, not inside a spawned agent. A real agent can open any absolute path it likes —
//! another attempt's tree, the acceptance corpus, the harness binary. Enforcement the
//! candidate cannot decline (OS sandbox or per-attempt container, corpus outside the mount
//! namespace) is **not implemented**, and until it is, no claim of isolation should rest on
//! this file.
//!
//! Each test is named for the way the benchmark would otherwise lie.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

use nazm_bench::acceptance::{AcceptanceSuite, CheckResult, Verdict};
use nazm_bench::workspace::Workspace;

/// The submission is correct iff `answer.txt` says `42`. Stands in for a real test
/// process; what matters is that it reads the *frozen* submission.
fn answer_is_correct(s: &nazm_bench::Submission) -> CheckResult {
    match std::fs::read_to_string(s.path.join("answer.txt")) {
        Ok(text) if text.trim() == "42" => CheckResult::Pass,
        Ok(_) => CheckResult::Fail,
        // The candidate produced no output. That is the candidate's fault, not the
        // evaluator's, and it must be counted rather than voided.
        Err(e) => CheckResult::CandidateFault(format!("no answer.txt: {e}")),
    }
}

/// A check whose own dependency is unavailable — the evaluator's fault, not the
/// candidate's. Stands in for "the database would not start".
fn evaluator_is_broken(_s: &nazm_bench::Submission) -> CheckResult {
    CheckResult::EvaluatorFault("test database refused connections".to_owned())
}

fn suite() -> AcceptanceSuite {
    AcceptanceSuite::new("demo-01").with_check("answer_is_correct", answer_is_correct)
}

#[test]
fn editing_the_visible_tests_does_not_change_independent_acceptance() {
    let frozen = tempfile::tempdir().unwrap();
    let ws = Workspace::create("attempt-1").unwrap();

    // A wrong answer, plus a visible test the agent has rewritten to pass regardless.
    ws.write("answer.txt", "41").unwrap();
    ws.write("visible/test.sh", "#!/bin/sh\nexit 0\n").unwrap();

    let submission = ws.freeze(frozen.path()).unwrap();
    let verdict = suite().grade(&submission);

    assert!(
        matches!(verdict, Verdict::Rejected { .. }),
        "the agent rewrote the visible test; the independent grade must be unmoved: {verdict:?}"
    );

    // And the outcome records the overfitting signature, not a plain wrong answer.
    let outcome = verdict.into_outcome(/* visible_passed */ true);
    assert!(
        matches!(
            outcome,
            nazm_bench::Outcome::VisiblePassedAcceptanceFailed { .. }
        ),
        "visible-pass plus acceptance-fail is the informative case and must be recorded as such"
    );
}

#[test]
fn a_process_that_outlives_its_timeout_cannot_change_the_graded_submission() {
    let frozen = tempfile::tempdir().unwrap();
    let ws = Workspace::create("attempt-2").unwrap();
    ws.write("answer.txt", "41").unwrap();

    let submission = ws.freeze(frozen.path()).unwrap();

    // The agent — or an orphaned child — keeps writing after the freeze.
    ws.write("answer.txt", "42").unwrap();
    assert_eq!(
        ws.read("answer.txt").unwrap().trim(),
        "42",
        "the live tree did change"
    );

    let verdict = suite().grade(&submission);
    assert!(
        matches!(verdict, Verdict::Rejected { .. }),
        "grading must see the frozen 41, not the later 42: {verdict:?}"
    );
}

#[test]
fn a_submission_modified_after_freezing_is_void_not_rescored() {
    let frozen = tempfile::tempdir().unwrap();
    let ws = Workspace::create("attempt-3").unwrap();
    ws.write("answer.txt", "41").unwrap();
    let submission = ws.freeze(frozen.path()).unwrap();

    // Something reached into the frozen snapshot itself. That is not a better score; it
    // means the record and the artifact disagree, and the attempt is not usable.
    std::fs::write(frozen.path().join("answer.txt"), "42").unwrap();

    let verdict = suite().grade(&submission);
    assert!(
        matches!(verdict, Verdict::Void { .. }),
        "a drifted snapshot must be void, never graded: {verdict:?}"
    );
    assert!(verdict.into_outcome(false).is_void());
}

#[test]
fn the_evaluator_grades_exactly_the_submission_named_in_the_record() {
    let frozen_a = tempfile::tempdir().unwrap();
    let frozen_b = tempfile::tempdir().unwrap();

    let a = Workspace::create("attempt-a").unwrap();
    a.write("answer.txt", "42").unwrap();
    let sub_a = a.freeze(frozen_a.path()).unwrap();

    let b = Workspace::create("attempt-b").unwrap();
    b.write("answer.txt", "41").unwrap();
    let sub_b = b.freeze(frozen_b.path()).unwrap();

    assert_ne!(
        sub_a.digest, sub_b.digest,
        "different submissions must digest differently"
    );
    assert_eq!(suite().grade(&sub_a), Verdict::Accepted);
    assert!(matches!(suite().grade(&sub_b), Verdict::Rejected { .. }));

    // Identical content digests identically regardless of which attempt produced it, so
    // the digest identifies the artifact rather than the run.
    let c = Workspace::create("attempt-c").unwrap();
    c.write("answer.txt", "42").unwrap();
    let frozen_c = tempfile::tempdir().unwrap();
    let sub_c = c.freeze(frozen_c.path()).unwrap();
    assert_eq!(sub_a.digest, sub_c.digest);
}

#[test]
fn evaluator_failure_is_void_but_candidate_failure_is_counted() {
    // The distinction this test originally got backwards. An earlier version used a
    // candidate that produced no output as the "infrastructure failure" case, which
    // voided it — deleting an arm's worst attempts from the denominator and making an
    // unreliable arm look better than one that merely gets answers wrong.

    // 1. Candidate produced nothing. Its fault. COUNTED.
    let frozen_a = tempfile::tempdir().unwrap();
    let ws_a = Workspace::create("attempt-4a").unwrap();
    ws_a.write("readme.txt", "no answer here").unwrap();
    let sub_a = ws_a.freeze(frozen_a.path()).unwrap();

    let verdict_a = suite().grade(&sub_a);
    assert!(
        matches!(verdict_a, Verdict::CandidateFault { .. }),
        "a candidate that emits nothing has failed, not voided the run: {verdict_a:?}"
    );
    assert!(!verdict_a.is_void());
    assert!(
        !verdict_a.clone().into_outcome(false).is_void(),
        "a candidate fault must stay in the denominator"
    );

    // 2. The evaluator's own dependency is down. Not the arm's fault. VOID.
    let frozen_b = tempfile::tempdir().unwrap();
    let ws_b = Workspace::create("attempt-4b").unwrap();
    ws_b.write("answer.txt", "42").unwrap();
    let sub_b = ws_b.freeze(frozen_b.path()).unwrap();

    let broken = AcceptanceSuite::new("demo-01").with_check("db_reachable", evaluator_is_broken);
    let verdict_b = broken.grade(&sub_b);
    assert!(
        matches!(verdict_b, Verdict::EvaluatorFault { .. }),
        "an evaluator dependency failure must void: {verdict_b:?}"
    );
    assert!(verdict_b.clone().into_outcome(false).is_void());
}

/// The harness will not itself route one attempt's file operations into another's tree.
///
/// Note what this does not show: nothing here stops a *spawned candidate process* from
/// opening the other workspace by absolute path. See the module docs.
#[test]
fn the_harness_does_not_route_one_attempt_into_another() {
    let a = Workspace::create("attempt-a").unwrap();
    let b = Workspace::create("attempt-b").unwrap();

    a.write("secret.txt", "attempt a state").unwrap();

    assert_ne!(a.path(), b.path(), "attempts must not share a tree");
    assert!(
        b.read("secret.txt").is_err(),
        "attempt b must not see attempt a's files"
    );

    // And b cannot climb out to reach it.
    assert!(b.resolve("../attempt-a/secret.txt").is_err());
    assert!(b.resolve("/etc/passwd").is_err());
}

#[test]
fn a_workspace_is_removed_when_the_attempt_ends() {
    // Otherwise a long run accumulates every prior attempt's state on disk, and
    // "isolated" becomes true only until something walks the temp directory.
    let path = {
        let ws = Workspace::create("attempt-5").unwrap();
        ws.write("answer.txt", "42").unwrap();
        ws.path().to_path_buf()
    };
    assert!(
        !path.exists(),
        "the attempt tree must not survive the attempt"
    );
}

#[test]
fn a_symlink_out_of_the_workspace_is_not_dragged_into_the_submission() {
    let frozen = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("elsewhere.txt"), "not part of the work").unwrap();

    let ws = Workspace::create("attempt-6").unwrap();
    ws.write("answer.txt", "42").unwrap();

    let status = Command::new("ln")
        .arg("-s")
        .arg(outside.path().join("elsewhere.txt"))
        .arg(ws.path().join("link.txt"))
        .status()
        .unwrap();
    assert!(status.success(), "could not create the symlink fixture");

    let submission = ws.freeze(frozen.path()).unwrap();
    assert!(
        !submission.files.keys().any(|k| k.contains("link.txt")),
        "a symlink must not pull outside content into the graded submission: {:?}",
        submission.files.keys().collect::<Vec<_>>()
    );
    assert_eq!(suite().grade(&submission), Verdict::Accepted);
}
