//! The container-lifecycle completion checklist, driven through the fake engine.
//!
//! **Scope.** These establish the harness's *logic*: creation order, teardown order,
//! cleanup verification, ownership recording, and what happens after a partial startup
//! failure. They establish nothing about Docker. The kill guarantee, read-only mount
//! enforcement, and the restricted database role are claims about Docker and PostgreSQL,
//! and only Docker-gated integration tests can touch them.
//!
//! Each test is named for the scenario it discharges from that checklist.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nazm_bench::engine::fake::{FakeEngine, Op};
use nazm_bench::engine::{ContainerEngine, EngineError, ResourceId, ResourceKind};
use nazm_bench::sandbox::attempt::{AttemptConfig, Disposition, Limits, Names, Sandbox};
use nazm_bench::sandbox::ledger::{Ledger, State};
use nazm_bench::sandbox::network::{self, AllocationError, SubnetBlock};

fn config() -> AttemptConfig {
    AttemptConfig {
        run: "r1".to_owned(),
        attempt: "a1".to_owned(),
        candidate_image: "node:22-alpine@sha256:test".to_owned(),
        postgres_image: "postgres:16-alpine@sha256:test".to_owned(),
        block: SubnetBlock::default(),
        limits: Limits::default(),
    }
}

fn ledger(dir: &tempfile::TempDir) -> Ledger {
    Ledger::open(dir.path().join("resources.jsonl"), "r1").unwrap()
}

#[test]
fn normal_execution_removes_every_attempt_resource() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();

    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();
    assert_eq!(sb.teardown(), Disposition::Clean);

    assert!(
        engine.surviving().is_empty(),
        "leaked: {:?}",
        engine.surviving()
    );
    assert!(
        led.outstanding().unwrap().is_empty(),
        "ledger still claims resources: {:?}",
        led.outstanding().unwrap()
    );
}

#[test]
fn failure_after_network_creation_removes_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    // The network is created, then the first volume fails.
    engine.fail_next(Op::CreateVolume, EngineError::NoSpaceLeft);

    let err = Sandbox::start(&engine, &led, config()).err().unwrap();
    assert!(
        matches!(err.1, Disposition::Clean),
        "partial startup must still clean up"
    );
    assert!(
        engine.surviving().is_empty(),
        "the network created before the failure leaked: {:?}",
        engine.surviving()
    );
}

#[test]
fn failure_after_database_startup_removes_container_volume_and_network() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    // Network, both volumes, and the database are created; the candidate fails.
    engine.fail_next(
        Op::CreateContainer,
        EngineError::ImagePullFailed {
            image: "node:22-alpine@sha256:test".to_owned(),
            reason: "not found".to_owned(),
        },
    );

    let (_, disposition) = Sandbox::start(&engine, &led, config()).err().unwrap();
    assert_eq!(disposition, Disposition::Clean);
    assert!(
        engine.surviving().is_empty(),
        "leaked: {:?}",
        engine.surviving()
    );
}

#[test]
fn a_hard_timeout_terminates_the_candidate_and_termination_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();

    let cand = ResourceId::new(ResourceKind::Container, &sb.names().candidate);
    assert!(engine.inspect(&cand).unwrap().running);

    sb.kill_candidate().unwrap();

    let state = engine.inspect(&cand).unwrap();
    assert!(!state.running, "termination must be verified, not assumed");
    assert_eq!(state.exit_code, Some(137));

    assert_eq!(sb.teardown(), Disposition::Clean);
}

#[test]
fn an_unverifiable_cleanup_quarantines_rather_than_passing() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();

    // The daemon goes away at verification time. Not "probably fine": an unverifiable
    // teardown is indistinguishable from a leaked one, and only one of those is safe.
    engine.fail_next(
        Op::List,
        EngineError::DaemonUnreachable("socket closed".to_owned()),
    );

    match sb.teardown() {
        Disposition::Quarantine { reason, .. } => {
            assert!(
                reason.contains("could not be verified"),
                "reason was: {reason}"
            );
        }
        Disposition::Clean => panic!("an unverifiable cleanup must never report Clean"),
    }
}

#[test]
fn surviving_resources_quarantine_the_next_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();

    // Removal of the work volume fails; the resource is still there afterwards.
    engine.fail_next(
        Op::Remove,
        EngineError::RemovalInProgress {
            id: ResourceId::new(ResourceKind::Container, "x"),
        },
    );

    match sb.teardown() {
        Disposition::Quarantine { surviving, .. } => {
            assert!(!surviving.is_empty(), "quarantine must name what survived");
        }
        Disposition::Clean => panic!("a failed removal must not report Clean"),
    }
}

#[test]
fn ownership_is_recorded_before_creation_so_an_interruption_is_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();

    // Interrupt at the moment of the very first create: the intent is already on disk.
    engine.fail_next(
        Op::CreateNetwork,
        EngineError::DaemonUnreachable("killed".to_owned()),
    );
    let _ = Sandbox::start(&engine, &led, config());

    let outstanding = led.outstanding().unwrap();
    let names = Names::for_attempt("r1", "a1");
    assert!(
        outstanding
            .iter()
            .any(|e| e.resource.name == names.network && e.state == State::Intended),
        "the network must be claimed as Intended even though its create failed: {outstanding:?}"
    );
}

#[test]
fn a_subnet_conflict_fails_clearly_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();

    // Someone else's networks occupy the whole configured block.
    let block = SubnetBlock {
        prefix: [10, 201],
        slots: vec![1, 2],
    };
    engine.plant_foreign_network("someone-elses-stack", "10.201.1.0/24");
    engine.plant_foreign_network("another-teams-db", "10.201.2.0/24");

    let cfg = AttemptConfig {
        block: block.clone(),
        ..config()
    };
    let (err, disposition) = Sandbox::start(&engine, &led, cfg).err().unwrap();

    assert!(format!("{err}").contains("occupied"), "error was: {err}");
    assert_eq!(
        disposition,
        Disposition::Clean,
        "nothing was created, so nothing leaked"
    );

    // The foreign networks are untouched. This machine runs fourteen unrelated Docker
    // networks; a cleanup pass that reclaimed what merely looked free would eventually
    // delete someone else's database.
    let survivors: Vec<_> = engine.surviving().into_iter().map(|r| r.name).collect();
    assert!(survivors.contains(&"someone-elses-stack".to_owned()));
    assert!(survivors.contains(&"another-teams-db".to_owned()));
}

#[test]
fn allocation_skips_an_occupied_slot_rather_than_failing() {
    let block = SubnetBlock {
        prefix: [10, 201],
        slots: vec![1, 2, 3],
    };
    let in_use = vec!["10.201.1.0/24".to_owned(), "172.17.0.0/16".to_owned()];
    let got = network::allocate(&block, &in_use).unwrap();
    assert_eq!(got.cidr(), "10.201.2.0/24");
}

#[test]
fn a_containing_supernet_counts_as_a_collision() {
    // Conservative on purpose: a /16 that contains the candidate /24 must read as
    // occupied. Costing a slot is cheaper than handing out a colliding network.
    let block = SubnetBlock {
        prefix: [10, 201],
        slots: vec![7],
    };
    let in_use = vec!["10.201.0.0/16".to_owned()];
    assert!(matches!(
        network::allocate(&block, &in_use),
        Err(AllocationError::BlockExhausted { .. })
    ));
}

#[test]
fn teardown_removes_containers_before_networks_and_volumes() {
    // The rule that makes ordering load-bearing: a network with attached endpoints and a
    // volume with a referencing container both refuse removal. If this order ever
    // reverses, the fake refuses exactly as a daemon would.
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();
    assert_eq!(sb.teardown(), Disposition::Clean);

    let log = engine.log();
    let first_container_removal = log
        .iter()
        .position(|l| l.starts_with("remove container"))
        .expect("no container removal");
    let first_network_removal = log
        .iter()
        .position(|l| l.starts_with("remove network"))
        .expect("no network removal");
    let first_volume_removal = log
        .iter()
        .position(|l| l.starts_with("remove volume"))
        .expect("no volume removal");

    assert!(first_container_removal < first_network_removal);
    assert!(first_container_removal < first_volume_removal);
}
