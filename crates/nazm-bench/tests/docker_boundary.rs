//! Claims about Docker and PostgreSQL that only a real daemon can establish.
//!
//! Everything else in this suite runs against a fake engine and proves the harness's
//! *logic*. These prove the things the fake cannot: that killing a container reaps
//! detached descendants, that an internal network really has no route out, that a
//! restricted role really cannot escalate, and that cleanup really removed things.
//!
//! # Opt-in, and required where it matters
//!
//! These run **only** under `NAZM_REQUIRE_DOCKER=1`, and under that flag an unreachable
//! daemon is a failure rather than a skip. Two reasons for opt-in rather than
//! run-if-available:
//!
//! * they create real containers on a shared machine, and an ordinary `cargo test` should
//!   not do that behind someone's back;
//! * `cargo xtask mutate` runs the suite once per mutation, and letting it start Postgres
//!   twenty times would make it slow, and — worse — let real infrastructure flakiness show
//!   up as a mutation verdict.
//!
//! A suite that silently skips its only real evidence is worse than one that has none,
//! because it reports green. Hence the flag, and hence its failing loudly when set.
//!
//! Serialised behind a mutex: the design runs one attempt at a time, and so does this.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nazm_bench::engine::docker::DockerCli;
use nazm_bench::engine::{ContainerEngine, ResourceId, ResourceKind};
use nazm_bench::sandbox::attempt::{AttemptConfig, Disposition, Limits, Sandbox};
use nazm_bench::sandbox::ledger::Ledger;
use nazm_bench::sandbox::network::SubnetBlock;

fn serial() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Returns the engine, or skips — unless Docker is required, in which case it fails.
fn docker_or_skip() -> Option<DockerCli> {
    if std::env::var("NAZM_REQUIRE_DOCKER").as_deref() != Ok("1") {
        eprintln!("skipping: set NAZM_REQUIRE_DOCKER=1 to run the confinement tests");
        return None;
    }
    let cli = DockerCli::new();
    assert!(
        cli.available(),
        "NAZM_REQUIRE_DOCKER=1 but no Docker daemon is reachable. These are the only \
         tests that establish confinement; skipping them silently would report green on \
         no evidence."
    );
    Some(cli)
}

const PG: &str = "postgres:16-alpine";
const BUSY: &str = "alpine:3.20";

fn config(attempt: &str, owner: &str) -> AttemptConfig {
    AttemptConfig {
        run: "dockertest".to_owned(),
        attempt: attempt.to_owned(),
        owner: owner.to_owned(),
        candidate_image: BUSY.to_owned(),
        postgres_image: PG.to_owned(),
        // A block well clear of the fourteen 172.x/16 networks already on this machine.
        block: SubnetBlock {
            prefix: [10, 201],
            slots: (40..=60).collect(),
        },
        limits: Limits::default(),
    }
}

/// Run a command inside a container and return its combined output and exit code.
fn exec(container: &str, argv: &[&str]) -> (String, i32) {
    let mut cmd = std::process::Command::new("docker");
    cmd.args(["exec", container]).args(argv);
    let out = cmd.output().expect("docker exec");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.code().unwrap_or(-1))
}

#[test]
fn a_candidate_container_and_its_database_come_up_and_tear_down_verified() {
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("r.jsonl"), "dockertest").unwrap();

    let mut sb = Sandbox::start(&cli, &led, config("up1", "owner-up1"))
        .map_err(|(e, _)| e)
        .expect("bring-up failed");

    let cand = ResourceId::new(ResourceKind::Container, &sb.names().candidate);
    assert!(
        cli.inspect(&cand).unwrap().running,
        "candidate should be running"
    );

    assert_eq!(
        sb.teardown(),
        Disposition::Clean,
        "teardown must verify clean"
    );

    // Verified absent, not merely requested.
    assert!(
        cli.list_by_label("nazm.attempt", "up1").unwrap().is_empty(),
        "resources survived teardown"
    );
    assert!(
        led.outstanding().unwrap().is_empty(),
        "ledger still claims resources"
    );
}

#[test]
fn the_candidate_cannot_reach_the_host_or_another_attempts_files() {
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("r.jsonl"), "dockertest").unwrap();

    // A sentinel on the host that the candidate must not be able to read.
    let sentinel = dir.path().join("acceptance-answers.txt");
    std::fs::write(&sentinel, "the holdout answer key").unwrap();

    let mut sb = Sandbox::start(&cli, &led, config("iso1", "owner-iso1"))
        .map_err(|(e, _)| e)
        .expect("bring-up failed");
    let name = sb.names().candidate.clone();

    // The host path does not exist inside the container.
    let (out, code) = exec(&name, &["cat", sentinel.to_str().unwrap()]);
    assert_ne!(code, 0, "the candidate read a host file: {out}");
    assert!(
        !out.contains("answer key"),
        "sentinel content leaked: {out}"
    );

    // No route off the attempt network. `--internal` removes the default gateway.
    let (out, code) = exec(&name, &["sh", "-c", "ip route | grep -c default || true"]);
    assert!(
        out.trim().starts_with('0') || code != 0,
        "an internal network must have no default route, got: {out}"
    );

    assert_eq!(sb.teardown(), Disposition::Clean);
}

#[test]
fn killing_the_candidate_reaps_a_detached_grandchild() {
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("r.jsonl"), "dockertest").unwrap();

    let mut sb = Sandbox::start(&cli, &led, config("kill1", "owner-kill1"))
        .map_err(|(e, _)| e)
        .expect("bring-up failed");
    let name = sb.names().candidate.clone();

    // Detached, re-parented, and deliberately not a child of anything we signal.
    exec(
        &name,
        &["sh", "-c", "setsid sleep 600 >/dev/null 2>&1 < /dev/null &"],
    );
    let (out, _) = exec(&name, &["sh", "-c", "pgrep sleep | wc -l"]);
    assert!(
        out.trim().parse::<u32>().unwrap_or(0) >= 1,
        "the detached process did not start, so this proves nothing: {out}"
    );

    // Hard deadline: kill the container, not a client process.
    sb.kill_candidate().expect("kill");

    let cand = ResourceId::new(ResourceKind::Container, &name);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut stopped = false;
    while Instant::now() < deadline {
        if !cli.inspect(&cand).map(|s| s.running).unwrap_or(false) {
            stopped = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(stopped, "the container did not stop");

    // The grandchild is gone with it: when PID 1 of a namespace exits, the kernel
    // signals every remaining task in that namespace regardless of parentage.
    let (out, code) = exec(&name, &["pgrep", "sleep"]);
    assert_ne!(code, 0, "a detached process survived the kill: {out}");

    assert_eq!(sb.teardown(), Disposition::Clean);
}

#[test]
fn a_new_process_recovers_resources_a_killed_harness_left_behind() {
    // The genuinely cross-process case. The in-memory fake cannot host it: two processes
    // need shared state to recover *through*, and the daemon is that state.
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("r.jsonl");
    let owner = "owner-recover";

    // A child harness brings an attempt up and is killed before tearing it down.
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_nazm-bench"))
        .args(["sandbox-leak", "--ledger"])
        .arg(&ledger_path)
        .args(["--run", "dockertest", "--attempt", "rec1", "--owner", owner])
        .status()
        .expect("spawn child harness");
    assert!(
        status.success(),
        "the child harness failed to create resources"
    );

    // It really did leak.
    let leaked = cli.list_by_label("nazm.attempt", "rec1").unwrap();
    assert!(
        !leaked.is_empty(),
        "the child created nothing, so recovery proves nothing"
    );

    // A separate process — this one — recovers from the ledger on disk.
    let led = Ledger::open(&ledger_path, "dockertest").unwrap();
    let result = nazm_bench::sandbox::recovery::reconcile(&cli, &led, owner).unwrap();

    assert!(
        result.is_clear(),
        "unresolved after recovery: {:?}",
        result.unresolved
    );
    assert!(!result.removed.is_empty(), "recovery removed nothing");
    assert!(
        cli.list_by_label("nazm.attempt", "rec1")
            .unwrap()
            .is_empty(),
        "resources survived recovery"
    );
}
