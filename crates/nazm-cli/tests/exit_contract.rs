//! The v1 command-line exit contract (Q1-C1, `docs/stability.md`, *Diagnostics and exit
//! statuses*): six statuses, each meaning one thing, each reached here from real commands, and
//! the machine summary carrying the same status the process returns.
//!
//! | 0 | success |
//! | 1 | the source, program, package or request is invalid |
//! | 2 | the program ran and failed |
//! | 3 | a file or resource could not be found or read |
//! | 4 | the compiler, a tool it runs, or the machine failed |
//! | 5 | the command line itself is wrong |

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::{Command, Output};

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn nazm_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(NAZM)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

/// The process status, with the command's stderr for a failure message.
fn status(dir: &Path, args: &[&str]) -> (Option<i32>, String) {
    let o = nazm_in(dir, args);
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    std::fs::write(d.path().join("ok.nz"), "fn main() -> Int { 40 + 2 }\n").unwrap();
    std::fs::write(
        d.path().join("wrong.nz"),
        "fn main() -> Int { \"not an int\" }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("traps.nz"),
        "fn sq(n: Int) -> Int { n * n }\nfn main() -> Int { sq(9223372036854775807) }\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join("uses_missing.nz"),
        "use \"absent.nz\";\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    d
}

/// Each (args, expected status), run in one project.
fn expect(cases: &[(&[&str], i32)]) {
    let d = project();
    for (args, want) in cases {
        let (got, err) = status(d.path(), args);
        assert_eq!(got, Some(*want), "nazm {}: {err}", args.join(" "));
    }
}

#[test]
fn status_0_is_success() {
    expect(&[
        (&["check", "ok.nz"], 0),
        (&["run", "ok.nz"], 0),
        (&["--help"], 0),
        (&["--version"], 0),
        (&["help", "check"], 0),
    ]);
}

#[test]
fn status_1_is_an_invalid_program_or_request() {
    expect(&[
        // A program that does not check.
        (&["check", "wrong.nz"], 1),
        (&["run", "wrong.nz"], 1),
        // A request the command refuses: an executable over its own source.
        (&["build", "ok.nz", "-o", "ok.nz"], 1),
        // A package name that is not one.
        (&["init", "Not_A_Name"], 1),
    ]);
}

#[test]
fn status_2_is_a_program_that_ran_and_failed() {
    expect(&[(&["run", "traps.nz"], 2)]);
    // The same program built: the executable's own status, and the oracle's.
    let d = project();
    let (built, err) = status(d.path(), &["build", "traps.nz", "-o", "traps"]);
    assert_eq!(built, Some(0), "{err}");
    let o = Command::new(d.path().join("traps")).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn status_3_is_a_file_that_could_not_be_found_or_read() {
    expect(&[
        (&["check", "absent.nz"], 3),
        (&["run", "absent.nz"], 3),
        // A module a `use` names.
        (&["check", "uses_missing.nz"], 3),
        // A foreign object `--link` names.
        (&["build", "ok.nz", "--link", "absent.o", "-o", "out"], 3),
    ]);
}

#[test]
fn status_4_is_the_compiler_or_its_tools_failing() {
    // No `clang` on `PATH`: the machine cannot build a program that is fine.
    let d = project();
    let empty = tempfile::tempdir().unwrap();
    let o = Command::new(NAZM)
        .current_dir(d.path())
        .env("PATH", empty.path())
        .args(["build", "ok.nz", "-o", "out"])
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!d.path().join("out").exists());
}

#[test]
fn status_5_is_a_wrong_command_line() {
    expect(&[
        (&["--no-such-option"], 5),
        (&["no-such-command"], 5),
        (&[], 5),
        // A missing argument.
        (&["run"], 5),
        // An option value or combination the command line cannot take.
        (&["build", "ok.nz", "--layout", "sideways", "-o", "out"], 5),
        (
            &["build", "ok.nz", "--target", "no-such-triple", "-o", "out"],
            5,
        ),
        (&["build", "ok.nz", "--stack-watermark", "-o", "out"], 5),
    ]);
}

/// A machine consumer reads the same status from the summary as from the process.
#[test]
fn the_summary_carries_the_status_the_process_returns() {
    let d = project();
    for (args, want, state) in [
        (&["check", "ok.nz", "--summary-json"][..], 0, "success"),
        (&["check", "wrong.nz", "--summary-json"][..], 1, "rejected"),
        (
            &["check", "absent.nz", "--summary-json"][..],
            3,
            "unreadable",
        ),
        (
            &["build", "ok.nz", "-o", "ok.nz", "--summary-json"][..],
            1,
            "refused",
        ),
        (
            &[
                "build",
                "ok.nz",
                "--layout",
                "sideways",
                "-o",
                "out",
                "--summary-json",
            ][..],
            5,
            "refused",
        ),
    ] {
        let o = nazm_in(d.path(), args);
        assert_eq!(o.status.code(), Some(want), "{args:?}");
        let line = String::from_utf8_lossy(&o.stdout);
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap_or_else(|e| {
            panic!("{args:?}: no summary ({e}): {line}");
        });
        assert_eq!(v["exit_code"], want, "{args:?}: {v}");
        assert_eq!(v["status"], state, "{args:?}: {v}");
    }
}
