//! The security configuration, asserted as data.
//!
//! Argv construction is pure, so every one of these runs without a daemon. They are
//! written as **negative properties** — what must never appear — because that is the form
//! a configuration regression takes: someone adds a flag for a good reason and removes a
//! guarantee nobody was checking.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nazm_bench::engine::docker::{
    classify, container_create_argv, network_create_argv, volume_create_argv,
};
use nazm_bench::engine::{
    ContainerSpec, EngineError, Labels, Mount, MountSource, NetworkSpec, VolumeSpec,
};

fn labels() -> Labels {
    Labels {
        run: "r1".to_owned(),
        attempt: "a1".to_owned(),
        role: "cand".to_owned(),
        owner: "tok".to_owned(),
    }
}

fn candidate() -> ContainerSpec {
    ContainerSpec {
        name: "nazm-r1-a1-cand".to_owned(),
        image: "node:22-alpine@sha256:abc".to_owned(),
        labels: labels(),
        network: Some("nazm-r1-a1-net".to_owned()),
        network_alias: None,
        mounts: vec![
            Mount {
                source: MountSource::Volume("nazm-r1-a1-work".to_owned()),
                target: "/work".to_owned(),
                read_only: false,
            },
            Mount {
                source: MountSource::HostPath("/corpus/task".to_owned()),
                target: "/nazm/task".to_owned(),
                read_only: true,
            },
        ],
        env: Vec::new(),
        command: vec!["sleep".to_owned(), "infinity".to_owned()],
        memory_mb: 2048,
        milli_cpus: 2000,
        pids_limit: 512,
        init: true,
        cap_add: Vec::new(),
    }
}

fn joined(argv: &[String]) -> String {
    argv.join(" ")
}

#[test]
fn the_candidate_never_publishes_a_port() {
    let a = container_create_argv(&candidate());
    assert!(!a
        .iter()
        .any(|x| x == "-p" || x == "--publish" || x == "--publish-all"));
}

#[test]
fn the_candidate_never_mounts_the_docker_socket() {
    // The one mount that converts a container into control of the host.
    let a = joined(&container_create_argv(&candidate()));
    assert!(!a.contains("docker.sock"), "argv: {a}");
}

#[test]
fn the_candidate_never_shares_a_host_namespace() {
    let a = joined(&container_create_argv(&candidate()));
    for forbidden in [
        "--pid=host",
        "--network=host",
        "--ipc=host",
        "--uts=host",
        "--privileged",
    ] {
        assert!(!a.contains(forbidden), "{forbidden} present in argv: {a}");
    }
}

#[test]
fn the_candidate_joins_its_own_network_not_the_default_bridge() {
    let a = container_create_argv(&candidate());
    let net = a
        .iter()
        .position(|x| x == "--network")
        .expect("no --network");
    assert_eq!(a.get(net + 1).map(String::as_str), Some("nazm-r1-a1-net"));
}

#[test]
fn every_container_drops_all_capabilities_and_forbids_privilege_escalation() {
    let a = container_create_argv(&candidate());
    let cap = a
        .iter()
        .position(|x| x == "--cap-drop")
        .expect("no --cap-drop");
    assert_eq!(a.get(cap + 1).map(String::as_str), Some("ALL"));
    assert!(joined(&a).contains("--security-opt no-new-privileges"));
}

#[test]
fn memory_swap_equals_memory_so_the_limit_is_real() {
    // Without this the container swaps instead of being killed, and the memory cap
    // becomes a latency cliff rather than a limit — which would make any
    // `ResourceExhausted` outcome report something that never happened.
    let a = container_create_argv(&candidate());
    let mem = a
        .iter()
        .position(|x| x == "--memory")
        .and_then(|i| a.get(i + 1))
        .cloned();
    let swap = a
        .iter()
        .position(|x| x == "--memory-swap")
        .and_then(|i| a.get(i + 1))
        .cloned();
    assert_eq!(mem, swap, "memory and memory-swap must match: {a:?}");
    assert_eq!(mem.as_deref(), Some("2048m"));
}

#[test]
fn the_container_is_created_not_run() {
    // `docker run` in the foreground attaches a client whose death does not stop the
    // container, and whose exit code is the client's rather than the workload's.
    let a = container_create_argv(&candidate());
    assert_eq!(a.first().map(String::as_str), Some("create"));
    assert!(!a.iter().any(|x| x == "run"));
}

#[test]
fn a_read_only_mount_is_marked_readonly() {
    let a = joined(&container_create_argv(&candidate()));
    assert!(a.contains("target=/nazm/task,readonly"), "argv: {a}");
    assert!(
        !a.contains("target=/work,readonly"),
        "the work mount must be writable: {a}"
    );
}

#[test]
fn the_attempt_network_is_internal_and_explicitly_subnetted() {
    let spec = NetworkSpec {
        name: "nazm-r1-a1-net".to_owned(),
        subnet: "10.201.7.0/24".to_owned(),
        labels: labels(),
        internal: true,
    };
    let a = container_argv_of(&network_create_argv(&spec));
    assert!(a.contains("--internal"), "argv: {a}");
    assert!(a.contains("--subnet 10.201.7.0/24"), "argv: {a}");
}

fn container_argv_of(argv: &[String]) -> String {
    argv.join(" ")
}

#[test]
fn every_resource_carries_the_ownership_label() {
    // The label removal decisions are made on. A resource created without it would be
    // unrecognisable to its own harness and would never be cleaned up.
    for argv in [
        container_create_argv(&candidate()),
        network_create_argv(&NetworkSpec {
            name: "n".to_owned(),
            subnet: "10.201.1.0/24".to_owned(),
            labels: labels(),
            internal: true,
        }),
        volume_create_argv(&VolumeSpec {
            name: "v".to_owned(),
            labels: labels(),
        }),
    ] {
        assert!(
            joined(&argv).contains("--label nazm.owner=tok"),
            "missing ownership label: {argv:?}"
        );
    }
}

#[test]
fn stderr_is_classified_into_the_typed_taxonomy() {
    assert!(matches!(
        classify(
            1,
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock."
        ),
        EngineError::DaemonUnreachable(_)
    ));
    assert!(matches!(
        classify(1, "write /data: no space left on device"),
        EngineError::NoSpaceLeft
    ));
    assert!(matches!(
        classify(
            125,
            "Conflict. The container name \"/x\" is already in use by container abc"
        ),
        EngineError::NameConflict { .. }
    ));
    assert!(matches!(
        classify(1, "Pool overlaps with other one on this address space"),
        EngineError::SubnetOverlap { .. }
    ));
    assert!(matches!(
        classify(
            1,
            "could not find an available, non-overlapping IPv4 address pool"
        ),
        EngineError::AddressPoolExhausted
    ));
    assert!(matches!(
        classify(1, "network abc has active endpoints"),
        EngineError::StillReferenced { .. }
    ));
    // Anything unrecognised stays CliFailure rather than being guessed into a variant: a
    // wrong guess sends the attempt down the wrong disposition.
    assert!(matches!(
        classify(7, "something nobody has seen before"),
        EngineError::CliFailure { code: 7, .. }
    ));
}
