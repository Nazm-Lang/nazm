//! The `nazm` command as a caller sees it: stdout, stderr, and exit status.
//!
//! Exit status is part of the interface. A caller that has to parse stderr to learn
//! whether a program failed to type-check or failed while running is doing work the
//! process could have told it in one byte.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;

fn nazm(args: &[&str]) -> (String, String, i32) {
    finish(
        Command::new(env!("CARGO_BIN_EXE_nazm"))
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR")),
    )
}

/// `nazm run` of an example as a user runs it: without the result report the suite sets
/// (R1, 5b.3), so what it prints is what it printed, and its status is `main`'s.
fn run_as_user(name: &str) -> (String, String, i32) {
    finish(
        Command::new(env!("CARGO_BIN_EXE_nazm"))
            .args(["run", &example(name)])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env_remove(nazm_lir::RESULT_VARIABLE),
    )
}

fn finish(c: &mut Command) -> (String, String, i32) {
    let out = c.output().expect("run nazm");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Paths are relative to the workspace root, two levels up from this crate.
fn example(name: &str) -> String {
    format!("../../examples/{name}")
}

#[test]
fn running_the_example_prints_its_value_and_exits_zero() {
    let (stdout, _, code) = run_as_user("max.nz");
    assert_eq!(stdout, "25\n");
    assert_eq!(code, 0);
}

#[test]
fn a_type_error_exits_one_and_names_the_code() {
    let (stdout, stderr, code) = nazm(&["run", &example("bad/undefined_variable.nz")]);
    assert_eq!(code, 1, "a check failure is exit 1");
    assert!(
        stdout.trim().is_empty(),
        "nothing should be printed as a value"
    );
    assert!(stderr.contains("N0200"), "{stderr}");
    assert!(stderr.contains("`y` is not defined"), "{stderr}");
}

#[test]
fn a_runtime_error_exits_two_so_it_is_distinguishable_from_a_type_error() {
    let (_, stderr, code) = nazm(&["run", &example("bad/division_by_zero.nz")]);
    assert_eq!(code, 2, "a runtime failure is exit 2, not 1");
    assert!(stderr.contains("N0401"), "{stderr}");
}

#[test]
fn a_missing_file_exits_three() {
    let (_, stderr, code) = nazm(&["run", "does-not-exist.nz"]);
    assert_eq!(code, 3);
    assert!(stderr.contains("cannot read"), "{stderr}");
}

#[test]
fn check_does_not_evaluate() {
    // The program divides by zero. `check` must not discover that, because it must not
    // run it.
    let (stdout, _, code) = nazm(&["check", &example("bad/division_by_zero.nz")]);
    assert_eq!(code, 0, "it type-checks fine");
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn json_output_is_one_object_per_line() {
    let (_, stderr, code) = nazm(&["run", &example("bad/wrong_argument_count.nz"), "--json"]);
    assert_eq!(code, 1);
    let line = stderr.lines().next().expect("a diagnostic line");
    let parsed: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
    assert_eq!(parsed["code"], "N0303");
    assert!(
        parsed["primary"]["span"]["start"].is_number(),
        "spans must be machine-usable"
    );
}

#[test]
fn every_bad_example_is_refused_with_its_own_code() {
    // A shared code would make two different mistakes indistinguishable to anything
    // matching on it.
    let cases = [
        ("bad/undefined_variable.nz", "N0200"),
        ("bad/wrong_argument_count.nz", "N0303"),
        ("bad/condition_not_bool.nz", "N0301"),
        ("bad/invalid_operands.nz", "N0304"),
        ("bad/unsupported_construct.nz", "N0101"),
        ("bad/division_by_zero.nz", "N0401"),
        ("bad/assign_to_immutable.nz", "N0310"),
        ("bad/mutable_parameter.nz", "N0311"),
        ("bad/break_outside_loop.nz", "N0312"),
        ("bad/unreachable_after_break.nz", "N0313"),
        ("bad/break_as_value.nz", "N0314"),
        ("bad/bare_return.nz", "N0101"),
        ("bad/return_type_mismatch.nz", "N0300"),
        ("bad/runaway_recursion.nz", "N0403"),
    ];
    for (file, code) in cases {
        let (_, stderr, status) = nazm(&["run", &example(file)]);
        assert_ne!(status, 0, "{file} should fail");
        assert!(
            stderr.contains(code),
            "{file} should report {code}, got:\n{stderr}"
        );
    }
}

#[test]
fn deep_trees_and_deep_recursion_report_rather_than_abort() {
    // Both of these run as child processes on purpose. Remove either guarantee — the
    // parser's nesting limit, or sizing the stack from the depth budget — and the program
    // dies of stack exhaustion. An in-process test cannot survive its own subject to
    // report that; here the abort is just a child exit the parent reads.
    let (_, stderr, code) = nazm(&["run", &example("bad/nesting_too_deep.nz")]);
    assert!(stderr.contains("N0102"), "got:\n{stderr}");
    assert_eq!(code, 1, "a check failure, not a crash");

    // Nested parentheses, which no tree bound can catch: they build no node, so the
    // parser-recursion bound is the only thing refusing them. Out of process because
    // removing that bound leaves nothing bounding them at all.
    let (_, stderr, code) = nazm(&["check", &example("bad/paren_nesting.nz")]);
    assert!(stderr.contains("N0102"), "got:\n{stderr}");
    assert_eq!(code, 1);

    // A long arithmetic chain: the likeliest deep tree in a real program, and the one
    // shape the nesting bound originally missed because its parse does not recurse.
    let (_, stderr, code) = nazm(&["run", &example("bad/long_chain.nz")]);
    assert!(stderr.contains("N0102"), "got:\n{stderr}");
    assert_eq!(code, 1);

    // `nazm check` never reaches the evaluator, and parsing recurses just as deep.
    let (_, stderr, code) = nazm(&["check", &example("bad/nesting_too_deep.nz")]);
    assert!(stderr.contains("N0102"), "got:\n{stderr}");
    assert_eq!(code, 1);

    // 15,000 nested calls is past what a fixed reservation held: measured at about
    // 10,000 before the stack was sized from the budget.
    let (_, stderr, code) = nazm(&[
        "run",
        &example("bad/deep_recursion.nz"),
        "--max-depth",
        "15000",
    ]);
    assert!(stderr.contains("N0403"), "got:\n{stderr}");
    assert_eq!(code, 2, "a runtime failure, not a crash");
}

#[test]
fn a_depth_budget_the_stack_could_not_back_is_refused() {
    // Not a safety boundary — the reservation grows with the budget — but a number no
    // machine would grant is a crash waiting for the right program, so it is refused with
    // the figure rather than accepted and discovered later.
    let (_, stderr, code) = nazm(&["run", &example("max.nz"), "--max-depth", "99999999"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("GiB of stack"),
        "should say what it would cost, got:\n{stderr}"
    );
}

#[test]
fn one_mistake_produces_one_diagnostic() {
    // Each of these files contains a single mistake. Reporting a second, derived error —
    // "`f` is not a function" because its body failed to parse — would make an agent fix
    // something that was never wrong. Counted through `--json`, where each diagnostic is
    // exactly one line.
    for file in [
        "bad/bare_return.nz",
        "bad/return_type_mismatch.nz",
        "bad/undefined_variable.nz",
        "bad/unsupported_construct.nz",
    ] {
        let (_, stderr, _) = nazm(&["run", &example(file), "--json"]);
        let diagnostics: Vec<&str> = stderr.lines().filter(|l| l.starts_with('{')).collect();
        assert_eq!(
            diagnostics.len(),
            1,
            "{file} should report one diagnostic, got:\n{stderr}"
        );
    }
}

#[test]
fn recursion_runs_and_runaway_recursion_reports() {
    let (stdout, _, code) = run_as_user("search.nz");
    assert_eq!(stdout, "1723\n", "17 * 23 = 391");
    assert_eq!(code, 0);

    // The iteration budget does not bound recursion: without its own guard this would
    // abort the process with no diagnostic at all.
    let (_, stderr, code) = nazm(&["run", &example("bad/runaway_recursion.nz")]);
    assert!(stderr.contains("N0403"), "got:\n{stderr}");
    assert_eq!(code, 2, "a runtime failure, not a type error");

    // Mutual recursion counts against the same budget; per-function counting would let
    // this pair run until the stack ran out.
    let (_, stderr, code) = nazm(&["run", &example("bad/mutual_recursion.nz")]);
    assert!(stderr.contains("N0403"), "got:\n{stderr}");
    assert_eq!(code, 2);

    // And the bound is adjustable, so a deliberately deep recursion is not walled off.
    let (_, stderr, _) = nazm(&[
        "run",
        &example("bad/runaway_recursion.nz"),
        "--max-depth",
        "16",
    ]);
    assert!(
        stderr.contains("deeper than 16"),
        "--max-depth should be honoured, got:\n{stderr}"
    );
}

#[test]
fn loops_and_mutation_run() {
    let (stdout, _, code) = run_as_user("sum.nz");
    assert_eq!(stdout, "6074\n");
    assert_eq!(code, 0);
}

#[test]
fn a_runaway_loop_reports_instead_of_hanging() {
    // Without this guard the process would consume a core until someone noticed. A tool
    // that hangs is worse than one that says what happened.
    let (_, stderr, code) = nazm(&[
        "run",
        &example("bad/runaway_loop.nz"),
        "--max-iterations",
        "1000",
    ]);
    assert_eq!(code, 2, "a runaway loop is a runtime failure");
    assert!(stderr.contains("N0402"), "{stderr}");
    assert!(
        stderr.contains("--max-iterations"),
        "the diagnostic should say how to raise it: {stderr}"
    );
}

#[test]
fn break_and_continue_run() {
    let (stdout, _, code) = run_as_user("primes.nz");
    assert_eq!(stdout, "1060\n", "the sum of the primes below 100");
    assert_eq!(code, 0);
}
