//! Q1-C1: every guarded heavy entry point refuses a host that declares `NAZM_CONTAINED=1` without
//! a ceiling — exercised as the runbook allows (`docs/internal/runbook.md`, *A second incident, recorded*):
//! each runs as a **child process** whose environment this test builds, so the variable exists in
//! that child only, never in an agent's or a developer's shell.
//!
//! macOS only: there is no cgroup there, so no `memory.max` can make the declaration true, and a
//! refusal is the only correct answer. On Linux the same checks read the real cgroup, and the
//! contained runner — where they must pass — is exercised by every contained run.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// `cmd` with a clean slate of the two containment variables, then `NAZM_CONTAINED=1` set for the
/// child alone.
fn declared(mut cmd: Command) -> Output {
    cmd.current_dir(root())
        .env_remove("NAZM_ALLOW_UNCONTAINED")
        .env("NAZM_CONTAINED", "1")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "a host with no cgroup: macOS")]
fn the_bootstrap_refuses_a_declaration_without_a_ceiling() {
    let mut c = Command::new("sh");
    c.arg("compiler/bootstrap.sh");
    let o = declared(c);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("no memory ceiling applies here"),
        "{}",
        text(&o)
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "a host with no cgroup: macOS")]
fn the_fuzz_campaign_refuses_a_host() {
    let mut c = Command::new("sh");
    c.args(["fuzz/run.sh", "1"]);
    let o = declared(c);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(
        text(&o).contains("refusing to run with no memory ceiling"),
        "{}",
        text(&o)
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "a host with no cgroup: macOS")]
fn the_mutation_runner_refuses_a_host() {
    let mut c = Command::new(env!("CARGO_BIN_EXE_xtask"));
    c.args(["mutate", "--only", "=no-such-mutant"]);
    let o = declared(c);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("refusing to run `cargo xtask mutate` with no resource ceiling"),
        "{}",
        text(&o)
    );
}

/// The supervisor runs on the host: either variable in its own environment is refused before
/// Docker is asked anything.
#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "a host with no cgroup: macOS")]
fn the_contained_supervisor_refuses_either_variable_in_its_own_environment() {
    for var in ["NAZM_CONTAINED", "NAZM_ALLOW_UNCONTAINED"] {
        let o = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .current_dir(root())
            .env_remove("NAZM_CONTAINED")
            .env_remove("NAZM_ALLOW_UNCONTAINED")
            .env(var, "1")
            .args(["contained", "run", "true"])
            .output()
            .unwrap();
        assert!(!o.status.success(), "{var}: {}", text(&o));
        assert!(
            text(&o).contains(&format!("`{var}` is set in the host's environment")),
            "{var}: {}",
            text(&o)
        );
    }
}
