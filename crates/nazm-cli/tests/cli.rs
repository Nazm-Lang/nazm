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
    let out = Command::new(env!("CARGO_BIN_EXE_nazm"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run nazm");
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
    let (stdout, _, code) = nazm(&["run", &example("max.nz")]);
    assert_eq!(stdout.trim(), "25");
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
fn loops_and_mutation_run() {
    let (stdout, _, code) = nazm(&["run", &example("sum.nz")]);
    assert_eq!(stdout.trim(), "6074");
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
