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
use nazm_bench::sandbox::ledger::{Ledger, State};
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

/// Harness-side scratch for the attempt (database init script). Leaked deliberately: it
/// must outlive the sandbox that mounts it, and the process is short.
fn scratch_dir() -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().to_path_buf();
    std::mem::forget(dir);
    path
}

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
        scratch: scratch_dir(),
        die_after_create: None,
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
        .arg("--scratch")
        .arg(dir.path())
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

/// Run psql on the attempt network as the restricted role.
fn psql(
    network: &str,
    creds: &nazm_bench::sandbox::postgres::Credentials,
    sql: &str,
) -> (String, i32) {
    let out = std::process::Command::new("docker")
        .args(["run", "--rm", "--network", network, "-e"])
        .arg(format!("PGPASSWORD={}", creds.app_password))
        .args([PG, "psql", "-v", "ON_ERROR_STOP=1", "-h", "db", "-U"])
        .arg(&creds.app_role)
        .args(["-d", &creds.database, "-tAc", sql])
        .output()
        .expect("psql");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.code().unwrap_or(-1))
}

/// Poll until the application role can genuinely connect to its own database.
///
/// Not `pg_isready`, and not the Docker healthcheck. `pg_isready` over the Unix socket
/// succeeds during the bootstrap phase — while the temporary server is up but the app role
/// and database do not exist yet — which is a false positive that shows up later as
/// intermittent, unexplained failures. The readiness gate is the exact thing the work
/// needs to be true.
fn wait_ready(network: &str, creds: &nazm_bench::sandbox::postgres::Credentials) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if psql(network, creds, "SELECT 1").1 == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

#[test]
fn the_restricted_role_can_work_and_cannot_escalate() {
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("r.jsonl"), "dockertest").unwrap();

    let mut sb = Sandbox::start(&cli, &led, config("db1", "owner-db1"))
        .map_err(|(e, _)| e)
        .expect("bring-up failed");
    let network = sb.names().network.clone();
    let creds = sb.credentials().clone();

    assert!(
        wait_ready(&network, &creds),
        "the app role never became able to connect"
    );

    // It can do the work a task needs.
    let (out, code) = psql(
        &network,
        &creds,
        "CREATE TABLE items(id serial primary key, qty int not null);          INSERT INTO items(qty) VALUES (10); SELECT qty FROM items;",
    );
    assert_eq!(code, 0, "the app role could not do ordinary work: {out}");
    assert!(out.contains("10"), "expected the inserted row back: {out}");

    // And it has none of the escalation routes. Every value must be false.
    let (out, code) = psql(
        &network,
        &creds,
        &nazm_bench::sandbox::postgres::privilege_assertion_sql(&creds.app_role),
    );
    assert_eq!(code, 0, "privilege assertion failed to run: {out}");
    for (i, line) in out.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        assert_eq!(line.trim(), "f", "privilege check {i} was not false: {out}");
    }

    // The decisive one, tested directly rather than inferred: server-side program
    // execution. A superuser could start a process in the database container, which
    // killing the candidate container would not touch.
    let (out, code) = psql(
        &network,
        &creds,
        "COPY (SELECT 1) TO PROGRAM 'touch /tmp/pwned'",
    );
    assert_ne!(code, 0, "COPY TO PROGRAM succeeded: {out}");
    assert!(
        out.to_lowercase().contains("permission denied")
            || out.to_lowercase().contains("must be superuser"),
        "expected a privilege error, got: {out}"
    );

    assert_eq!(sb.teardown(), Disposition::Clean);
}

#[test]
fn each_attempt_gets_fresh_database_state() {
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("r.jsonl"), "dockertest").unwrap();

    // Attempt one writes a row.
    {
        let mut sb = Sandbox::start(&cli, &led, config("fresh1", "owner-f1"))
            .map_err(|(e, _)| e)
            .expect("bring-up 1");
        let (net, creds) = (sb.names().network.clone(), sb.credentials().clone());
        assert!(wait_ready(&net, &creds));
        let (out, code) = psql(
            &net,
            &creds,
            "CREATE TABLE leftover(x int); INSERT INTO leftover VALUES (1);",
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(sb.teardown(), Disposition::Clean);
    }

    // Attempt two must not see it. Independence between attempts is the property the
    // whole per-attempt volume exists for.
    {
        let mut sb = Sandbox::start(&cli, &led, config("fresh2", "owner-f2"))
            .map_err(|(e, _)| e)
            .expect("bring-up 2");
        let (net, creds) = (sb.names().network.clone(), sb.credentials().clone());
        assert!(wait_ready(&net, &creds));
        let (out, code) = psql(&net, &creds, "SELECT count(*) FROM leftover");
        assert_ne!(code, 0, "attempt two saw attempt one's table: {out}");
        assert_eq!(sb.teardown(), Disposition::Clean);
    }
}

#[test]
fn the_candidate_cannot_read_another_attempts_workspace_or_the_acceptance_corpus() {
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("r.jsonl"), "dockertest").unwrap();

    // The acceptance corpus lives on the host and is mounted into no candidate, ever.
    let corpus = dir.path().join("acceptance");
    std::fs::create_dir_all(&corpus).unwrap();
    std::fs::write(corpus.join("expected.txt"), "THE-ACCEPTANCE-ANSWER").unwrap();

    // Attempt A: write a sentinel into its work volume, then tear it down leaving the
    // volume behind, so there is something real for attempt B to fail to reach.
    let mut a = Sandbox::start(&cli, &led, config("probea", "owner-probea"))
        .map_err(|(e, _)| e)
        .expect("bring-up A");
    let a_work = a.names().work_volume.clone();
    exec(
        &a.names().candidate,
        &["sh", "-c", "echo ATTEMPT-A-SECRET > /work/secret.txt"],
    );

    // POSITIVE CONTROL 1: the sentinel really is there and really is readable from a
    // container that mounts that volume. Without this, a later "cannot read" proves
    // only that the path was wrong.
    let seen = std::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{a_work}:/w:ro"),
            BUSY,
            "cat",
            "/w/secret.txt",
        ])
        .output()
        .expect("read A's volume");
    assert!(
        String::from_utf8_lossy(&seen.stdout).contains("ATTEMPT-A-SECRET"),
        "the sentinel was never written, so the denial below would prove nothing"
    );

    // POSITIVE CONTROL 2: the corpus file is readable when it is mounted.
    let seen = std::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/c:ro", corpus.display()),
            BUSY,
        ])
        .args(["cat", "/c/expected.txt"])
        .output()
        .expect("read the corpus");
    assert!(
        String::from_utf8_lossy(&seen.stdout).contains("THE-ACCEPTANCE-ANSWER"),
        "the corpus fixture is missing, so the denial below would prove nothing"
    );

    // Attempt B runs while A's volume still exists.
    let mut b = Sandbox::start(&cli, &led, config("probeb", "owner-probeb"))
        .map_err(|(e, _)| e)
        .expect("bring-up B");
    let bname = b.names().candidate.clone();

    // It cannot see A's work through its own mount.
    let (out, code) = exec(&bname, &["cat", "/work/secret.txt"]);
    assert_ne!(code, 0, "attempt B read attempt A's file: {out}");
    assert!(
        !out.contains("ATTEMPT-A-SECRET"),
        "attempt A's secret leaked: {out}"
    );

    // Nor by naming the volume's path on the host, which is inside the Docker VM and
    // outside B's mount namespace entirely.
    let (out, code) = exec(
        &bname,
        &[
            "sh",
            "-c",
            &format!("cat /var/lib/docker/volumes/{a_work}/_data/secret.txt"),
        ],
    );
    assert_ne!(code, 0, "attempt B reached A's volume by host path: {out}");
    assert!(
        !out.contains("ATTEMPT-A-SECRET"),
        "secret leaked by host path: {out}"
    );

    // Nor the acceptance corpus, by the path it actually occupies on the host.
    let (out, code) = exec(
        &bname,
        &["cat", corpus.join("expected.txt").to_str().unwrap()],
    );
    assert_ne!(code, 0, "the candidate read the acceptance corpus: {out}");
    assert!(
        !out.contains("THE-ACCEPTANCE-ANSWER"),
        "the answer key leaked: {out}"
    );

    assert_eq!(b.teardown(), Disposition::Clean);
    assert_eq!(a.teardown(), Disposition::Clean);
}

#[test]
fn a_harness_killed_between_creating_and_recording_is_still_recoverable() {
    // The window the ledger exists for, and the only one nothing else reaches: the
    // resource exists and its creation was never recorded, so only the `Intended` entry
    // and the ownership label connect it to this run.
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("r.jsonl");
    let owner = "owner-window";

    // The child aborts immediately after creating the network, before recording it.
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_nazm-bench"))
        .args(["sandbox-leak", "--ledger"])
        .arg(&ledger_path)
        .arg("--scratch")
        .arg(dir.path())
        .args(["--run", "dockertest", "--attempt", "win1", "--owner", owner])
        .args(["--die-after-create", "net"])
        .status()
        .expect("spawn child harness");
    assert!(
        !status.success(),
        "the child was supposed to abort, not exit cleanly"
    );

    // The network exists.
    let leaked = cli.list_by_label("nazm.attempt", "win1").unwrap();
    assert!(
        !leaked.is_empty(),
        "nothing was created, so there is nothing to recover"
    );

    // And the ledger knows only that it was *intended* — the engine id was never learned.
    let led = Ledger::open(&ledger_path, "dockertest").unwrap();
    let outstanding = led.outstanding().unwrap();
    let entry = outstanding.first().expect("an outstanding claim");
    assert_eq!(
        entry.state,
        State::Intended,
        "the create was recorded despite the abort"
    );
    assert!(
        entry.resource.engine_id.is_none(),
        "the engine id was recorded, so this is not the window under test"
    );

    // A separate process recovers it anyway, by asking who owns the name.
    let result = nazm_bench::sandbox::recovery::reconcile(&cli, &led, owner).unwrap();
    assert!(result.is_clear(), "unresolved: {:?}", result.unresolved);
    assert_eq!(
        result.removed.len(),
        1,
        "expected the network removed: {result:?}"
    );
    assert!(
        cli.list_by_label("nazm.attempt", "win1")
            .unwrap()
            .is_empty(),
        "the resource survived recovery"
    );
}

/// psql as the superuser, for the acceptance reset. Never reachable from the candidate.
fn psql_admin(
    network: &str,
    creds: &nazm_bench::sandbox::postgres::Credentials,
    sql: &str,
) -> (String, i32) {
    let out = std::process::Command::new("docker")
        .args(["run", "--rm", "--network", network, "-e"])
        .arg(format!("PGPASSWORD={}", creds.superuser_password))
        .args([
            PG,
            "psql",
            "-v",
            "ON_ERROR_STOP=1",
            "-h",
            "db",
            "-U",
            "postgres",
        ])
        .args(["-d", "postgres", "-tAc", sql])
        .output()
        .expect("psql admin");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.code().unwrap_or(-1))
}

#[test]
fn development_state_cannot_carry_into_acceptance_within_one_attempt() {
    // Distinct from freshness *between* attempts. Inside one attempt the candidate spends
    // its whole run writing to this database; grading against that state would let a
    // submission pass by leaving the right rows behind rather than by doing the work.
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("r.jsonl"), "dockertest").unwrap();

    let mut sb = Sandbox::start(&cli, &led, config("reset1", "owner-reset1"))
        .map_err(|(e, _)| e)
        .expect("bring-up failed");
    let (net, creds) = (sb.names().network.clone(), sb.credentials().clone());
    assert!(wait_ready(&net, &creds), "never became ready");

    // Development phase: the candidate leaves state behind, including an open connection.
    let (out, code) = psql(
        &net,
        &creds,
        "CREATE TABLE answers(v text); INSERT INTO answers VALUES ('left-behind');",
    );
    assert_eq!(code, 0, "{out}");

    // POSITIVE CONTROL: it really is there before the reset, so the absence afterwards
    // reflects the reset rather than a write that never happened.
    let (out, code) = psql(&net, &creds, "SELECT v FROM answers");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("left-behind"),
        "the fixture was never written: {out}"
    );

    // Reset for grading, as the superuser — a capability the candidate does not have.
    for stmt in nazm_bench::sandbox::postgres::reset_statements(&creds) {
        let (out, code) = psql_admin(&net, &creds, &stmt);
        assert_eq!(code, 0, "reset statement failed ({stmt}): {out}");
    }

    // Acceptance phase: the table is gone, and the role can still work.
    let (out, code) = psql(&net, &creds, "SELECT v FROM answers");
    assert_ne!(code, 0, "development state survived into acceptance: {out}");
    assert!(
        !out.contains("left-behind"),
        "the row survived the reset: {out}"
    );

    let (out, code) = psql(&net, &creds, "CREATE TABLE fresh(x int); SELECT 1;");
    assert_eq!(code, 0, "the role lost its grants across the reset: {out}");

    assert_eq!(sb.teardown(), Disposition::Clean);
}

/// One complete attempt, from candidate work to a finalised record.
///
/// The submission is a single `answer.txt`, and the acceptance check reads it from the
/// frozen snapshot. Small on purpose: this establishes that the *path* joins up, not that
/// any particular task is well designed.
fn run_one_attempt(
    cli: &DockerCli,
    dir: &std::path::Path,
    attempt: &str,
    candidate_script: &str,
    wall_clock: Duration,
) -> nazm_bench::sandbox::execution::AttemptRecord {
    use nazm_bench::acceptance::{AcceptanceSuite, CheckResult};
    use nazm_bench::cost::{Cost, CostComponent};
    use nazm_bench::engine::{ResourceId, ResourceKind};
    use nazm_bench::sandbox::execution as ex;

    let led = Ledger::open(dir.join(format!("{attempt}.jsonl")), "dockertest").unwrap();
    let mut sb = Sandbox::start(cli, &led, config(attempt, &format!("owner-{attempt}")))
        .map_err(|(e, _)| e)
        .expect("bring-up");
    let (net, creds) = (sb.names().network.clone(), sb.credentials().clone());
    let cand = sb.names().candidate.clone();
    assert!(wait_ready(&net, &creds), "database never became ready");

    // 1. The fake agent does its work inside the candidate container.
    let started = Instant::now();
    exec(&cand, &["sh", "-c", candidate_script]);
    let timed_out = started.elapsed() >= wall_clock;

    // 2. Stop the candidate BEFORE anything is frozen. Nothing may still be writing.
    let stop = ex::stop_candidate(&sb, Duration::from_secs(20)).expect("stop");

    // 3. Freeze: copy the work volume out to an evaluator-owned directory the candidate
    //    never mounted, and take its digest.
    let frozen = dir.join(format!("{attempt}-frozen"));
    std::fs::create_dir_all(&frozen).unwrap();
    let copied = std::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/w:ro", sb.names().work_volume),
            "-v",
        ])
        .arg(format!("{}:/out", frozen.display()))
        .args([BUSY, "sh", "-c", "cp -a /w/. /out/ 2>/dev/null || true"])
        .status()
        .expect("freeze");
    assert!(copied.success(), "could not freeze the submission");

    let ws = nazm_bench::workspace::Workspace::create(attempt).unwrap();
    for entry in std::fs::read_dir(&frozen).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            ws.write(&name, &text).unwrap();
        }
    }
    let sealed = dir.join(format!("{attempt}-sealed"));
    let submission = ws.freeze(&sealed).unwrap();

    // 4. Fresh acceptance state: the candidate's own writes must not be what it is graded
    //    against.
    for stmt in nazm_bench::sandbox::postgres::reset_statements(&creds) {
        let (out, code) = psql_admin(&net, &creds, &stmt);
        assert_eq!(code, 0, "acceptance reset failed ({stmt}): {out}");
    }

    // 5. Grade the frozen snapshot, using tests the candidate never saw.
    fn answer_is_42(s: &nazm_bench::Submission) -> CheckResult {
        match std::fs::read_to_string(s.path.join("answer.txt")) {
            Ok(t) if t.trim() == "42" => CheckResult::Pass,
            Ok(_) => CheckResult::Fail,
            Err(e) => CheckResult::CandidateFault(format!("no answer.txt: {e}")),
        }
    }
    let verdict = AcceptanceSuite::new("e2e")
        .with_check("answer_is_42", answer_is_42)
        .grade(&submission);

    // A timeout is about the candidate, not the submission, and outranks the grade.
    let outcome =
        ex::classify_candidate(&stop, timed_out).unwrap_or_else(|| verdict.into_outcome(false));

    // 6. Accounting, then cleanup, then finalise — in that order.
    //
    // The database has to be stopped before its runtime can be read: a running container
    // has no finish time, and `inspect` correctly reports that as unavailable rather than
    // inventing a zero. Timings must therefore be captured after the kill and before
    // removal, which is why teardown happens further down rather than here.
    let db_id = nazm_bench::engine::ResourceId::new(
        nazm_bench::engine::ResourceKind::Container,
        sb.names().db.clone(),
    );
    cli.kill(&db_id)
        .expect("stop the database before reading its runtime");
    cli.wait_until_exit(&db_id, Duration::from_secs(20))
        .expect("wait for the database")
        .expect("the database did not stop");

    let durations = vec![
        ("cand".to_owned(), duration_ms(cli, &cand)),
        ("db".to_owned(), duration_ms(cli, &sb.names().db)),
    ];
    let cost = Cost::new(
        CostComponent::recorded(0), // no model calls: the agent is a shell script
        ex::infrastructure_cost(&durations, 1),
        CostComponent::recorded(0),
    );

    let disposition = sb.teardown();
    let _ = ResourceId::new(ResourceKind::Container, &cand);
    ex::finalise(
        attempt,
        outcome,
        cost,
        Some(submission.digest.clone()),
        durations,
        &disposition,
    )
}

/// Container runtime in milliseconds, or `None` when it cannot be read.
///
/// `inspect` already rejects Docker's zero time (`0001-01-01…`), so a missing timestamp
/// here means the container never ran or has not stopped yet. Returning `None` rather
/// than zero is the whole point: an unreadable clock is not a zero runtime, and the cost
/// line has to be able to say which it is.
///
/// The elapsed value itself is a placeholder — parsing RFC3339Nano and differencing is
/// deferred until there is a rate to multiply it by. What is real here is the
/// present-versus-absent distinction, which is what the accounting depends on.
fn duration_ms(cli: &DockerCli, name: &str) -> Option<u64> {
    use nazm_bench::engine::{ResourceId, ResourceKind};
    let id = ResourceId::new(ResourceKind::Container, name);
    let s = cli.inspect(&id).ok()?;
    s.started_at.as_ref()?;
    s.finished_at.as_ref()?;
    Some(1)
}

#[test]
fn the_complete_path_grades_accepted_wrong_answer_and_timeout() {
    let Some(cli) = docker_or_skip() else { return };
    let _guard = serial()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    use nazm_bench::outcome::{Limit, Outcome};

    // Accepted.
    let ok = run_one_attempt(
        &cli,
        dir.path(),
        "e2eok",
        "echo 42 > /work/answer.txt",
        Duration::from_secs(60),
    );
    assert_eq!(ok.outcome, Outcome::Accepted, "{ok:?}");
    assert!(
        ok.is_final(),
        "cleanup must verify before a grade is reportable: {:?}",
        ok.cleanup
    );
    assert!(
        ok.submission_digest.is_some(),
        "the graded bytes must be identified"
    );

    // Wrong answer.
    let wrong = run_one_attempt(
        &cli,
        dir.path(),
        "e2ewrong",
        "echo 41 > /work/answer.txt",
        Duration::from_secs(60),
    );
    assert_eq!(wrong.outcome, Outcome::WrongAnswer, "{wrong:?}");
    assert!(wrong.is_final());
    assert_ne!(
        wrong.submission_digest, ok.submission_digest,
        "different submissions must be identified differently"
    );

    // Timeout: the work never finishes within the budget, and that outranks the grade —
    // the candidate did write a correct answer first.
    let slow = run_one_attempt(
        &cli,
        dir.path(),
        "e2eslow",
        "echo 42 > /work/answer.txt; sleep 3",
        Duration::from_millis(1),
    );
    assert_eq!(
        slow.outcome,
        Outcome::Timeout {
            limit: Limit::WallClock
        },
        "a timeout outranks a correct answer: {slow:?}"
    );
    assert!(slow.is_final());

    // Every attempt kept its accounting, and nothing leaked.
    for r in [&ok, &wrong, &slow] {
        assert!(
            r.cost.is_complete(),
            "cost incomplete for {}: {:?}",
            r.attempt_id,
            r.cost
        );
    }
    assert!(
        cli.list_by_label("nazm.run", "dockertest")
            .unwrap()
            .is_empty(),
        "resources survived the end-to-end run"
    );
}
