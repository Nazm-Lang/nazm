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
use nazm_bench::engine::{self, ContainerEngine, EngineError, Ownership, ResourceId, ResourceKind};
use nazm_bench::sandbox::attempt::{AttemptConfig, Disposition, Limits, Names, Sandbox};
use nazm_bench::sandbox::ledger::{Ledger, State};
use nazm_bench::sandbox::network::{self, AllocationError, SubnetBlock};
use nazm_bench::sandbox::recovery;

/// Harness-side scratch for the attempt (database init script). Leaked deliberately: it
/// must outlive the sandbox that mounts it, and the process is short.
fn scratch_dir() -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().to_path_buf();
    std::mem::forget(dir);
    path
}

fn config() -> AttemptConfig {
    AttemptConfig {
        run: "r1".to_owned(),
        attempt: "a1".to_owned(),
        owner: "owner-token-run-1".to_owned(),
        candidate_image: "node:22-alpine@sha256:test".to_owned(),
        postgres_image: "postgres:16-alpine@sha256:test".to_owned(),
        block: SubnetBlock::default(),
        limits: Limits::default(),
        scratch: scratch_dir(),
        die_after_create: None,
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

/// The intent must be on disk *before* the create is attempted, not merely afterwards.
///
/// Checking the ledger once the call has returned cannot distinguish the two orderings:
/// both leave the same entry. So this looks at the world **during** the create, which is
/// the only moment they differ — and it is how the mutation for this ordering was caught
/// after it survived a test that only inspected the end state.
#[test]
fn ownership_is_recorded_before_creation_is_attempted() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("resources.jsonl");
    let led = Ledger::open(&ledger_path, "r1").unwrap();
    let engine = FakeEngine::new();

    // Records, for each create, whether the ledger already claimed that name.
    let seen: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_for_hook = Arc::clone(&seen);
    let path_for_hook = ledger_path.clone();
    engine.on_create(Box::new(move |id| {
        let claimed = std::fs::read_to_string(&path_for_hook)
            .map(|t| t.contains(&id.name))
            .unwrap_or(false);
        if let Ok(mut v) = seen_for_hook.lock() {
            v.push((id.name.clone(), claimed));
        }
    }));

    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();

    let observed = seen.lock().unwrap().clone();
    assert!(!observed.is_empty(), "no creates were observed");
    for (name, claimed_first) in &observed {
        assert!(
            *claimed_first,
            "{name} was created before the ledger claimed it — an interruption there \
             leaves a resource nothing knows the name of"
        );
    }

    assert_eq!(sb.teardown(), Disposition::Clean);
}

#[test]
fn a_failed_create_still_leaves_the_claim_on_disk() {
    // The consequence of writing first: even a create that never lands is recorded, so
    // recovery has something to check ownership against.
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
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
        "the network must be claimed despite its create failing: {outstanding:?}"
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

/// Q1-A2-08: overlap is decided on the bits of the shorter prefix, so supernets whose length is
/// not a whole number of octets are seen; an IPv6 network is not an IPv4 collision; and a
/// subnet that does not parse counts as occupied.
#[test]
fn a_supernet_of_any_prefix_length_counts_as_a_collision() {
    let block = SubnetBlock {
        prefix: [10, 201],
        slots: vec![7],
    };
    for occupied in [
        "10.200.0.0/15",
        "10.201.0.0/20",
        "10.0.0.0/8",
        "0.0.0.0/0",
        "garbage",
    ] {
        assert!(
            matches!(
                network::allocate(&block, &[occupied.to_owned()]),
                Err(AllocationError::BlockExhausted { .. })
            ),
            "{occupied} contains 10.201.7.0/24"
        );
    }
    for free in ["10.202.0.0/15", "10.201.16.0/20", "fd00::/64"] {
        assert_eq!(
            network::allocate(&block, &[free.to_owned()])
                .unwrap()
                .cidr(),
            "10.201.7.0/24",
            "{free} does not contain 10.201.7.0/24"
        );
    }
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

#[test]
fn a_name_collision_with_a_foreign_resource_never_removes_it() {
    // The hole this closes: the ledger records an intention to create a *name*. It cannot
    // establish that whatever now bears that name is ours. A create failing with
    // NameConflict is exactly the case where it is not — and a cleanup pass acting on its
    // own recorded intention would delete a stranger's container.
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();

    // Something else already holds the name this attempt will ask for.
    let names = Names::for_attempt("r1", "a1");
    engine.plant_foreign_container(&names.db);

    let (err, disposition) = Sandbox::start(&engine, &led, config()).err().unwrap();
    assert!(
        format!("{err}").contains("already exists"),
        "error was: {err}"
    );

    // Teardown must not have removed it.
    let survivors: Vec<_> = engine.surviving().into_iter().map(|r| r.name).collect();
    assert!(
        survivors.contains(&names.db),
        "the foreign container was removed: {survivors:?}"
    );

    // And the run is clean from the harness's point of view, because everything it
    // actually created was cleaned up.
    assert_eq!(
        disposition,
        Disposition::Clean,
        "own resources should still be cleaned"
    );

    // The ledger records the name as not ours, rather than leaving an open claim.
    let outstanding = led.outstanding().unwrap();
    assert!(
        !outstanding.iter().any(|e| e.resource.name == names.db),
        "a disowned name must not stay outstanding: {outstanding:?}"
    );
}

#[test]
fn ownership_is_checked_before_removal_not_inferred_from_the_ledger() {
    let engine = FakeEngine::new();
    engine.plant_foreign_container("someone-elses-api");
    let id = ResourceId::new(ResourceKind::Container, "someone-elses-api");

    assert_eq!(
        engine::ownership(&engine, &id, "owner-token-run-1"),
        Ownership::Foreign {
            found: Some("not-our-token".to_owned())
        }
    );
    assert!(!engine::ownership(&engine, &id, "owner-token-run-1").may_remove());

    // An absent resource is a different answer from a foreign one, and only one of them
    // means "nothing to do".
    let gone = ResourceId::new(ResourceKind::Container, "never-existed");
    assert_eq!(
        engine::ownership(&engine, &gone, "owner-token-run-1"),
        Ownership::Absent
    );
}

/// Reconstruction from persisted state — **not** a cross-process test.
///
/// It drops the sandbox without tearing down and rebuilds a `Ledger` from the same file,
/// which demonstrates that recovery works from what is on disk. It does not demonstrate
/// process interruption: everything here runs in one process, and `mem::forget` is not a
/// kill. The genuine version needs a child process terminated mid-create and a separate
/// recovery process, which requires a real daemon to share state through — it lands with
/// the Docker adapter, not here.
#[test]
fn recovery_works_from_persisted_state_alone() {
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("resources.jsonl");
    let engine = FakeEngine::new();

    // A stranger's resource sits alongside, with a name the harness never asked for.
    engine.plant_foreign_network("another-teams-net", "192.168.77.0/24");

    // --- "process one": bring an attempt up, then vanish without tearing down. ---
    {
        let led = Ledger::open(&ledger_path, "r1").unwrap();
        let sb = Sandbox::start(&engine, &led, config())
            .map_err(|(e, _)| e)
            .unwrap();
        std::mem::forget(sb); // skip teardown; note this is NOT process death
    }

    let leaked = engine.surviving().len();
    assert!(
        leaked >= 5,
        "expected the attempt's resources to survive, saw {leaked}"
    );

    // --- "process two": a fresh Ledger over the same file, and recovery. ---
    let led2 = Ledger::open(&ledger_path, "r1").unwrap();
    let outstanding = led2.outstanding().unwrap();
    assert!(!outstanding.is_empty(), "recovery must find the leftovers");

    // Recovery has to apply the same ordering discipline as teardown. Writing this test
    // naively — iterating the ledger in creation order — made the fake refuse with
    // StillReferenced, which is how the ordering rule ended up in the library rather than
    // in this test.
    let result = recovery::reconcile(&engine, &led2, "owner-token-run-1").unwrap();

    assert!(
        result.is_clear(),
        "unresolved leftovers: {:?}",
        result.unresolved
    );
    assert!(!result.removed.is_empty(), "recovery removed nothing");
    assert!(
        result.foreign.is_empty(),
        "nothing ledger-claimed should be foreign"
    );

    // The stranger's network is still there. It was never in our ledger, so recovery
    // never considered it — which is the property that matters on a machine running
    // fourteen unrelated Docker networks.
    let survivors: Vec<_> = engine.surviving().into_iter().map(|r| r.name).collect();
    assert_eq!(
        survivors,
        vec!["another-teams-net".to_owned()],
        "recovery should have left exactly the foreign network: {survivors:?}"
    );
}

#[test]
fn teardown_refuses_a_resource_that_changed_hands() {
    // Contrived sequence, real guard. Teardown removes from its own creation list, so
    // nothing in an ordinary run reaches the ownership check with a foreign resource —
    // which is exactly why the check went untested until `cargo xtask mutate` reported
    // `removal-skips-the-ownership-check` as SURVIVED.
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();

    let work = ResourceId::new(ResourceKind::Volume, &sb.names().work_volume);
    engine.retag_owner(&work, "some-other-run");

    match sb.teardown() {
        Disposition::Quarantine { reason, .. } => {
            assert!(
                reason.contains("not ours") || reason.contains("refusing"),
                "teardown must say why it declined: {reason}"
            );
        }
        Disposition::Clean => panic!("a resource we no longer own must not be removed silently"),
    }

    // And it is still there.
    let survivors: Vec<_> = engine.surviving().into_iter().map(|r| r.name).collect();
    assert!(
        survivors.contains(&sb.names().work_volume),
        "it was removed: {survivors:?}"
    );
}

#[test]
fn recovery_refuses_a_claimed_name_now_held_by_a_stranger() {
    // The realistic version of the same hazard: process one records an intent to create
    // `…-db`, dies before the create lands, and something unrelated takes that name.
    // Process two must not remove it on the strength of the recorded intention.
    let dir = tempfile::tempdir().unwrap();
    let ledger_path = dir.path().join("resources.jsonl");
    let engine = FakeEngine::new();

    let names = Names::for_attempt("r1", "a1");
    let db = ResourceId::new(ResourceKind::Container, &names.db);

    // Process one: intent written, create never completed.
    {
        let led = Ledger::open(&ledger_path, "r1").unwrap();
        led.intend("a1", &db).unwrap();
    }
    // Meanwhile, a stranger takes the name.
    engine.plant_foreign_container(&names.db);

    let led2 = Ledger::open(&ledger_path, "r1").unwrap();
    let result = recovery::reconcile(&engine, &led2, "owner-token-run-1").unwrap();

    assert_eq!(
        result.removed,
        Vec::<ResourceId>::new(),
        "nothing of ours existed to remove"
    );
    assert_eq!(
        result.foreign.len(),
        1,
        "the claimed name must be reported as foreign"
    );
    let (reported, _) = result.foreign.first().expect("foreign entry");
    assert_eq!(*reported, db);

    let survivors: Vec<_> = engine.surviving().into_iter().map(|r| r.name).collect();
    assert!(
        survivors.contains(&names.db),
        "the stranger's container was removed"
    );
}

#[test]
fn unknown_ownership_blocks_the_next_attempt_but_foreign_does_not() {
    // The two share "do not delete" and nothing else. Foreign resolves the claim — the
    // name is not ours and never will be. Unknown leaves it outstanding and stops the
    // run, because the resource may exist even though we never learned its id, and that
    // is precisely the state that reaches the next attempt.
    let dir = tempfile::tempdir().unwrap();
    let names = Names::for_attempt("r1", "a1");

    // Foreign: a stranger holds a name we claimed.
    let foreign_path = dir.path().join("foreign.jsonl");
    let engine = FakeEngine::new();
    let db = ResourceId::new(ResourceKind::Container, &names.db);
    {
        let led = Ledger::open(&foreign_path, "r1").unwrap();
        led.intend("a1", &db).unwrap();
    }
    engine.plant_foreign_container(&names.db);
    let led = Ledger::open(&foreign_path, "r1").unwrap();
    let foreign = recovery::reconcile(&engine, &led, "owner-token-run-1").unwrap();

    assert!(
        foreign.is_clear(),
        "a foreign resource must not block the next attempt"
    );
    assert_eq!(foreign.foreign.len(), 1);
    assert!(
        led.outstanding().unwrap().is_empty(),
        "a foreign claim must be resolved, not re-reported forever"
    );

    // Unknown: the engine cannot say who owns it.
    let unknown_path = dir.path().join("unknown.jsonl");
    let engine2 = FakeEngine::new();
    let net = ResourceId::new(ResourceKind::Network, &names.network);
    {
        let led2 = Ledger::open(&unknown_path, "r1").unwrap();
        led2.intend("a1", &net).unwrap();
    }
    engine2.plant_foreign_network(&names.network, "10.201.9.0/24");
    // Every ownership query fails, including the retries.
    for _ in 0..8 {
        engine2.fail_next(
            Op::Inspect,
            EngineError::DaemonUnreachable("socket".to_owned()),
        );
    }
    let led2 = Ledger::open(&unknown_path, "r1").unwrap();
    let unknown = recovery::reconcile(&engine2, &led2, "owner-token-run-1").unwrap();

    assert!(
        !unknown.is_clear(),
        "an unresolved claim must block the next attempt"
    );
    assert_eq!(unknown.unresolved.len(), 1);
    assert!(
        !led2.outstanding().unwrap().is_empty(),
        "an unresolved claim must stay outstanding — clearing it loses the only record \
         that something may be out there"
    );
}

#[test]
fn a_transient_unknown_resolves_on_retry() {
    // A daemon mid-restart answers the second time. Declaring it unresolved immediately
    // would quarantine a run for a hiccup.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retry.jsonl");
    let engine = FakeEngine::new();
    let names = Names::for_attempt("r1", "a1");
    let db = ResourceId::new(ResourceKind::Container, &names.db);

    {
        let led = Ledger::open(&path, "r1").unwrap();
        led.intend("a1", &db).unwrap();
    }
    // One failure, then the truth: the resource was never created.
    engine.fail_next(
        Op::Inspect,
        EngineError::DaemonUnreachable("restarting".to_owned()),
    );

    let led = Ledger::open(&path, "r1").unwrap();
    let result = recovery::reconcile(&engine, &led, "owner-token-run-1").unwrap();

    assert!(
        result.is_clear(),
        "a transient failure must not quarantine: {result:?}"
    );
    assert_eq!(result.already_absent.len(), 1);
}

#[test]
fn removal_addresses_the_resource_by_its_engine_id() {
    // Names are a namespace someone else can take. Once a create returns an id, that is
    // what later operations use, so a name changing hands cannot redirect a removal.
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();
    assert_eq!(sb.teardown(), Disposition::Clean);

    // Everything the ledger recorded as created carried an engine id.
    let text = std::fs::read_to_string(led.path()).unwrap();
    let created_without_id = text
        .lines()
        .filter(|l| l.contains("\"created\""))
        .filter(|l| l.contains("\"engine_id\":null"))
        .count();
    assert_eq!(
        created_without_id, 0,
        "a created resource must carry its engine id"
    );
}

#[test]
fn a_reused_name_in_a_later_attempt_is_a_separate_claim() {
    // The invariant that keeps correctness off the naming convention.
    //
    // Constructed so an unscoped key gives a different answer: attempt one's claim is
    // still OUTSTANDING when attempt two claims the same name. Keyed by name alone, the
    // second entry overwrites the first and one of two live claims disappears — recovery
    // would then never look for it. Keyed by (run, attempt, kind, name), both survive.
    //
    // An earlier version of this test had attempt one already resolved, which both the
    // scoped and unscoped keys report identically — it could not fail, and
    // `cargo xtask mutate` said so.
    let dir = tempfile::tempdir().unwrap();
    let led = Ledger::open(dir.path().join("reuse.jsonl"), "r1").unwrap();
    let id = ResourceId::new(ResourceKind::Volume, "shared-name");

    // Attempt one creates it and is interrupted: the claim stays live.
    led.intend("a1", &id).unwrap();
    led.created("a1", &id).unwrap();

    // Attempt two takes the same name and is also interrupted.
    led.intend("a2", &id).unwrap();

    let outstanding = led.outstanding().unwrap();
    assert_eq!(
        outstanding.len(),
        2,
        "both attempts' claims must survive; keying by name alone loses one: {outstanding:?}"
    );

    let attempts: Vec<&str> = outstanding.iter().map(|e| e.attempt.as_str()).collect();
    assert!(
        attempts.contains(&"a1"),
        "attempt one's claim vanished: {attempts:?}"
    );
    assert!(
        attempts.contains(&"a2"),
        "attempt two's claim vanished: {attempts:?}"
    );
}

#[test]
fn the_candidate_never_receives_admin_credentials_or_the_init_script() {
    // Two separate secrets and one privileged file. The candidate is entitled to exactly
    // one of the three: the restricted role's URL. Everything else would hand it either
    // the superuser password or the means to read what created the role.
    let dir = tempfile::tempdir().unwrap();
    let led = ledger(&dir);
    let engine = FakeEngine::new();
    let mut sb = Sandbox::start(&engine, &led, config())
        .map_err(|(e, _)| e)
        .unwrap();

    let creds = sb.credentials().clone();
    let (db, cand) = sb.specs();

    // The candidate gets the app URL and nothing else.
    let cand_env: Vec<&str> = cand.env.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        cand_env,
        vec!["DATABASE_URL"],
        "candidate env: {cand_env:?}"
    );

    let cand_env_values = cand
        .env
        .iter()
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        !cand_env_values.contains(&creds.superuser_password),
        "the superuser password reached the candidate"
    );
    assert!(
        cand_env_values.contains(&creds.app_password),
        "the candidate needs the app password to connect at all"
    );

    // The init script is mounted into the database and nowhere else. It is the file that
    // creates the role; a candidate that can read it learns how its own limits were set,
    // and in an earlier draft it carried the password in plain text.
    let cand_mounts = format!("{:?}", cand.mounts);
    assert!(
        !cand_mounts.contains("pginit"),
        "candidate mounts the init script: {cand_mounts}"
    );
    assert!(
        !cand_mounts.contains("docker-entrypoint-initdb.d"),
        "candidate mounts the init directory: {cand_mounts}"
    );

    let db_mounts = format!("{:?}", db.mounts);
    assert!(
        db_mounts.contains("docker-entrypoint-initdb.d"),
        "the database needs it"
    );
    assert!(
        db_mounts.contains("read_only: true"),
        "the init mount must be read-only: {db_mounts}"
    );

    // And the admin secrets exist only on the database side.
    let db_env: Vec<&str> = db.env.iter().map(|(k, _)| k.as_str()).collect();
    assert!(db_env.contains(&"POSTGRES_PASSWORD"));
    assert!(!cand_env.contains(&"POSTGRES_PASSWORD"));
    assert!(!cand_env.contains(&"APP_PASSWORD"));

    assert_eq!(sb.teardown(), Disposition::Clean);
}
