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

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;

use nazm_bench::acceptance::{AcceptanceSuite, CheckResult, Verdict};
use nazm_bench::workspace::{Workspace, WritersStopped};

/// Nothing in this file runs a candidate: every tree here is built by the test process
/// itself, so the freeze's quiescence precondition holds by construction rather than by
/// having been arranged. Stated once, used everywhere below.
fn quiet() -> WritersStopped {
    WritersStopped::none_running("built by the test process; no candidate ever ran")
}

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

    let submission = ws.freeze(frozen.path(), &quiet()).unwrap();
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

    let submission = ws.freeze(frozen.path(), &quiet()).unwrap();

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
    let submission = ws.freeze(frozen.path(), &quiet()).unwrap();

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
    let sub_a = a.freeze(frozen_a.path(), &quiet()).unwrap();

    let b = Workspace::create("attempt-b").unwrap();
    b.write("answer.txt", "41").unwrap();
    let sub_b = b.freeze(frozen_b.path(), &quiet()).unwrap();

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
    let sub_c = c.freeze(frozen_c.path(), &quiet()).unwrap();
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
    let sub_a = ws_a.freeze(frozen_a.path(), &quiet()).unwrap();

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
    let sub_b = ws_b.freeze(frozen_b.path(), &quiet()).unwrap();

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
fn every_symlink_is_refused_including_the_ones_that_look_contained() {
    // Symlinks are refused outright for this milestone, because containing them is
    // harder than it looks and a rule that is nearly right is worse than a refusal.
    //
    // Two counterexamples killed the nearly-right rule, and both are here so that
    // anything reintroducing it fails:
    //
    //   composed — `a -> .` and `b -> a/../outside`. Checked one at a time, each target
    //   looks contained: `a/../outside` simplifies to `outside`. Resolved by the
    //   filesystem, `a` is the snapshot itself, so `..` leaves it and `b` reads outside.
    //
    //   absolute — a link naming a path inside the *workspace* is "contained" by any
    //   lexical test, and recreating it in the snapshot leaves it pointing at the live
    //   tree the candidate was writing to. That defeats the freeze itself, which is the
    //   one thing the freeze exists to provide.
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "not part of the work").unwrap();

    // A function rather than a closure, so the boxed cases below borrow nothing and can
    // be `'static`.
    fn link(ws: &Workspace, target: &std::path::Path, name: &str) {
        let status = Command::new("ln")
            .arg("-s")
            .arg(target)
            .arg(ws.path().join(name))
            .status()
            .unwrap();
        assert!(status.success(), "could not create the {name} fixture");
    }

    // Each case is (name, how to build it). All must be refused.
    /// A named way to plant a link, so a failure says which shape failed.
    type Case = (&'static str, Box<dyn Fn(&Workspace)>);

    let cases: Vec<Case> = vec![
        (
            "plainly outside",
            Box::new({
                let secret = secret.clone();
                move |ws: &Workspace| link(ws, &secret, "out.txt")
            }),
        ),
        (
            "composed through a link to the root",
            Box::new({
                let outside = outside.path().to_path_buf();
                move |ws: &Workspace| {
                    link(ws, std::path::Path::new("."), "a");
                    let via = std::path::Path::new("a/../")
                        .join(outside.file_name().unwrap())
                        .join("secret.txt");
                    link(ws, &via, "b");
                }
            }),
        ),
        (
            "absolute, into the live workspace",
            Box::new(|ws: &Workspace| {
                let inside = ws.path().join("answer.txt");
                link(ws, &inside, "abs.txt");
            }),
        ),
        (
            "relative and contained",
            Box::new(|ws: &Workspace| link(ws, std::path::Path::new("answer.txt"), "alias.txt")),
        ),
        (
            "a cycle",
            Box::new(|ws: &Workspace| {
                link(ws, std::path::Path::new("loop_b"), "loop_a");
                link(ws, std::path::Path::new("loop_a"), "loop_b");
            }),
        ),
        (
            "dangling",
            Box::new(|ws: &Workspace| {
                link(ws, std::path::Path::new("never-created.txt"), "dangle.txt");
            }),
        ),
    ];

    for (name, build) in cases {
        let frozen = tempfile::tempdir().unwrap();
        let ws = Workspace::create("attempt-symlink").unwrap();
        ws.write("answer.txt", "42").unwrap();
        build(&ws);

        let err = ws
            .freeze(frozen.path(), &quiet())
            .unwrap_err_or_else_message(name);
        assert!(
            err.contains("symlink"),
            "{name}: refused for the wrong reason: {err}"
        );

        // And nothing from outside reached the snapshot on the way to the refusal.
        for e in std::fs::read_dir(frozen.path()).unwrap().flatten() {
            if let Ok(t) = std::fs::read_to_string(e.path()) {
                assert!(
                    !t.contains("not part of the work"),
                    "{name}: outside content copied in"
                );
            }
        }
    }
}

/// Small helper so each case reports which one failed.
trait UnwrapErrMessage {
    fn unwrap_err_or_else_message(self, case: &str) -> String;
}

impl UnwrapErrMessage for Result<nazm_bench::Submission, nazm_bench::workspace::WorkspaceError> {
    fn unwrap_err_or_else_message(self, case: &str) -> String {
        match self {
            Err(e) => e.to_string(),
            Ok(_) => panic!("{case}: the freeze accepted a symlink"),
        }
    }
}

#[test]
fn the_executable_bit_is_part_of_the_submission() {
    // Two submissions with identical bytes but different permissions behave differently
    // — one runs, one does not — so a digest that cannot tell them apart is not an
    // identity. `fs::write` creates with the process umask, which silently strips the
    // bit off every script a candidate produced.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let build = |executable: bool| {
            let frozen = tempfile::tempdir().unwrap();
            let ws = Workspace::create("attempt-mode").unwrap();
            ws.write("answer.txt", "42").unwrap();
            ws.write("run.sh", "#!/bin/sh\nexit 0\n").unwrap();
            if executable {
                let p = ws.path().join("run.sh");
                let mut perms = std::fs::metadata(&p).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&p, perms).unwrap();
            }
            let sub = ws.freeze(frozen.path(), &quiet()).unwrap();
            let mode = std::fs::metadata(frozen.path().join("run.sh"))
                .unwrap()
                .permissions()
                .mode();
            (sub.digest.clone(), sub.files.clone(), mode, frozen)
        };

        let (plain_digest, plain_files, plain_mode, _keep_a) = build(false);
        let (exec_digest, exec_files, exec_mode, _keep_b) = build(true);

        assert_ne!(
            plain_digest, exec_digest,
            "the same bytes with and without the executable bit digested the same"
        );
        // The recorded mask is `0o7777`, so the mode is part of identity rather than a
        // class the entry merely hints at: `0644` and `0755` are different entries, and
        // so would `0600` and `0644` be.
        assert!(
            plain_files["run.sh"].starts_with("file:0644:"),
            "{plain_files:?}"
        );
        assert!(
            exec_files["run.sh"].starts_with("file:0755:"),
            "{exec_files:?}"
        );

        assert_eq!(plain_mode & 0o111, 0, "a plain file came out executable");
        assert_ne!(
            exec_mode & 0o111,
            0,
            "the executable bit was dropped by the copy, so the grader runs a file the \
             candidate did not produce"
        );
    }
}

#[test]
fn a_mode_change_alone_is_drift() {
    // The companion to the digest check: the snapshot is re-walked by
    // `verify_unchanged`, so a permission change after freezing has to register there
    // too, or the record and the graded artifact can diverge silently.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let frozen = tempfile::tempdir().unwrap();
        let ws = Workspace::create("attempt-mode-drift").unwrap();
        ws.write("run.sh", "#!/bin/sh\nexit 0\n").unwrap();
        let submission = ws.freeze(frozen.path(), &quiet()).unwrap();
        submission.verify_unchanged().expect("unchanged at first");

        let p = frozen.path().join("run.sh");
        let mut perms = std::fs::metadata(&p).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&p, perms).unwrap();

        assert!(
            submission.verify_unchanged().is_err(),
            "making the graded file executable after the freeze went unnoticed"
        );
    }
}

#[test]
fn a_filename_that_is_not_utf8_is_refused_rather_than_renamed() {
    // `to_string_lossy` maps every invalid byte onto U+FFFD, so two distinct names can
    // collapse onto one manifest key — and in a map the second silently overwrites the
    // first. A submission could then hide a file behind another file's name with the
    // digest none the wiser. Refusing has no such failure mode.
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let frozen = tempfile::tempdir().unwrap();
        let ws = Workspace::create("attempt-utf8").unwrap();
        ws.write("answer.txt", "42").unwrap();

        // Two names that differ, and that `to_string_lossy` renders identically.
        //
        // Not every filesystem will create them: APFS refuses the bytes outright with
        // EILSEQ, ext4 accepts them. Where the fixture cannot be built the end-to-end
        // path cannot be exercised, and saying so beats passing quietly — the logic
        // itself is covered by `manifest_key`'s own tests, which need no filesystem.
        let mut created = 0;
        for bad in [b"bad\xff.txt".as_slice(), b"bad\xfe.txt".as_slice()] {
            if std::fs::write(ws.path().join(OsStr::from_bytes(bad)), "x").is_ok() {
                created += 1;
            }
        }
        if created == 0 {
            // `NAZM_REQUIRE_NONUTF8=1` turns this skip into a failure. CI sets it on the
            // Linux job, so the end-to-end path is enforced somewhere rather than
            // quietly skipped everywhere — a switch nobody enables is a test nobody
            // runs.
            assert!(
                std::env::var_os("NAZM_REQUIRE_NONUTF8").is_none(),
                "NAZM_REQUIRE_NONUTF8 is set, but this filesystem would not create a \
                 non-UTF-8 filename, so the refusal was never exercised"
            );
            eprintln!(
                "skipped: this filesystem will not create a non-UTF-8 filename. Set \
                 NAZM_REQUIRE_NONUTF8=1 on a filesystem that does to make this a failure."
            );
            return;
        }

        let err = ws
            .freeze(frozen.path(), &quiet())
            .expect_err("a non-UTF-8 name must be refused");
        assert!(
            err.to_string().contains("not valid UTF-8"),
            "the refusal should say what it refused: {err}"
        );
    }
}

#[test]
fn ordinary_submissions_are_unaffected_by_any_of_this() {
    // The cost of the three rules above must be zero for a submission that does none of
    // those things, or they are a tax on every attempt rather than a guard against a few.
    let frozen = tempfile::tempdir().unwrap();
    let ws = Workspace::create("attempt-plain").unwrap();
    ws.write("answer.txt", "42").unwrap();
    ws.write("src/main.rs", "fn main() {}").unwrap();
    ws.write("nested/deep/notes.md", "# notes").unwrap();

    let submission = ws.freeze(frozen.path(), &quiet()).unwrap();
    submission.verify_unchanged().expect("must verify");
    assert_eq!(suite().grade(&submission), Verdict::Accepted);
    assert_eq!(submission.files.len(), 3);
    assert!(
        submission.files.values().all(|v| v.starts_with("file:")),
        "{:?}",
        submission.files
    );
    #[cfg(unix)]
    assert!(
        submission.files.values().all(|v| v.split(':').count() == 3),
        "each entry carries a mode as well as a hash: {:?}",
        submission.files
    );
}

#[test]
fn evidence_about_one_attempt_cannot_freeze_another() {
    // A token is evidence that a *particular* candidate stopped. Stopping attempt A's
    // container says nothing about whether B's is still writing, so freezing B on A's
    // token would snapshot a tree still being changed — the exact failure the token
    // exists to prevent, reached by passing the wrong one. With attempts running one at
    // a time this is a latent mistake; the moment two run together it is a live one.
    let frozen = tempfile::tempdir().unwrap();
    let ws = Workspace::create("attempt-b").unwrap();
    ws.write("answer.txt", "42").unwrap();

    let about_a = nazm_bench::sandbox::execution::StopOutcome {
        attempt: "attempt-a".to_owned(),
        exit_code: Some(0),
        oom_killed: false,
        started_at: None,
        finished_at: None,
    }
    .writers_stopped();
    assert_eq!(about_a.attempt(), Some("attempt-a"));

    let err = ws
        .freeze(frozen.path(), &about_a)
        .expect_err("a token for another attempt must be refused");
    let text = err.to_string();
    assert!(
        text.contains("attempt-b") && text.contains("attempt-a"),
        "the refusal should name both attempts: {text}"
    );

    // The matching token works, so the check discriminates rather than just refusing.
    let about_b = nazm_bench::sandbox::execution::StopOutcome {
        attempt: "attempt-b".to_owned(),
        exit_code: Some(0),
        oom_killed: false,
        started_at: None,
        finished_at: None,
    }
    .writers_stopped();
    let submission = ws.freeze(frozen.path(), &about_b).expect("the right token");
    assert_eq!(suite().grade(&submission), Verdict::Accepted);
}

#[test]
fn an_unbound_assertion_still_names_its_reason() {
    // `none_running` is the escape hatch for callers with no candidate at all, and it is
    // deliberately not a default: it takes a reason, and carries no attempt, so what it
    // claims is visible both at the call site and afterwards. It is an assertion, not
    // proof — the guarantee for a real attempt rests on the audited path that obtains a
    // token from a confirmed stop.
    let t = WritersStopped::none_running("built by the test process; no candidate ever ran");
    assert_eq!(t.attempt(), None);
    assert!(t.why().contains("no candidate"));
}
