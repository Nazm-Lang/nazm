# Integration testing against external services

Decided in M1, 2026-10-08. Governs tests whose subject includes a real external dependency — a
database, a cache, a message broker, an HTTP or TCP peer, a fault-injecting proxy, object storage,
a package registry. It does not govern the language: parser, checker, IR, memory, backend and
runtime semantics are tested without any external service, and stay so.

## The decision: Testcontainers is not adopted; the harness's own owned-resource engine is

| | A — host-side owned orchestrator | B — isolated Docker-in-Docker | C — the existing harness, no Testcontainers |
| --- | --- | --- | --- |
| Isolation | owned containers on the host daemon, on an `--internal` network | a separate daemon inside a privileged container | as A |
| Ownership | per-run token and labels | the inner daemon is disposable | **an owner token on every resource, labels, write-ahead ledger** (`crates/nazm-bench/src/sandbox/`); the token is the caller's — Q1 found no generator, so a caller must make it unique |
| Lifecycle and recovery | Testcontainers: drop guards and the Ryuk reaper | daemon teardown | intend → created → removed, recorded before each step; recovery acts only on ledger-claimed resources |
| Cleanup | Ryuk mounts the host Docker socket and removes by session label | the inner daemon, all at once | exact identities from the ledger: by engine id when the ledger has one, otherwise by name after its owner label is checked (a narrow window between that check and the removal); never by name alone, never prune |
| Disk and resources | image cache shared with the host | a second image store, privileged | as the host, bounded per attempt |
| CI portability | needs Docker | needs privileged Docker | needs Docker |
| Readability | familiar API | heavy | Rust, in-repository, reviewed against the runbook |
| Recovery after interruption | Ryuk is a sidecar: if it is not running, nothing recovers | inner daemon gone | `sandbox::recovery::reconcile` reads the ledger and removes only what it claims |

**Chosen: C — the owned-resource engine in `nazm-bench` — with A's shape**: the test process on the
host orchestrates owned dependency containers on an internal network. Reasons, against Nazm's
containment law rather than popularity:

1. **It already meets every ownership guarantee M1 requires**, with tests
   (`crates/nazm-bench/tests/{sandbox_lifecycle,docker_boundary,execution_boundary}.rs`): a run
   token the caller supplies (unique only if the caller makes it so — corrected in Q1), ownership labels, exact container / network / volume identity, a write-ahead record
   before any resource exists, interruption recovery, no foreign-resource deletion, no prune, no
   name-only cleanup.
2. **Testcontainers' cleanup model conflicts with the law.** Its default reaper mounts the host's
   Docker socket into a container (the runbook forbids handing the socket to a workload casually);
   disabling the reaper leaves drop-guard cleanup, which an interruption skips and nothing recovers.
3. **Image acquisition must be explicit.** Testcontainers pulls on first use, inside the test; the
   network policy below forbids that during a correctness run.
4. **No new dependency and no async runtime** for the test crates.

Revisit when: an external-service suite needs a service the engine cannot express, *and*
Testcontainers can run with a ledgered owner, no socket-mounted reaper and no implicit pull.

**Model B is rejected** for routine use: a privileged inner daemon is a larger isolation hole than
the one it closes, doubles image storage on a disk-constrained machine, and its teardown is
all-or-nothing.

## Network policy

- **Internal test network:** the test process and its owned dependency containers may talk to each
  other, on a network the run created (`--internal`, an explicitly allocated subnet —
  `crates/nazm-bench/src/sandbox/network.rs`), and nothing else.
- **Public Internet: forbidden during correctness execution.** Images are acquired in an explicit
  **preparation** step (`docker pull <pinned image>`), never as a side effect of a test. A missing
  image fails the test with the command that would fetch it.

## The `integration-external` category

| | |
| --- | --- |
| What | a test whose assertions need a real external service |
| Marking | `#[ignore = "integration-external: <service>"]`, plus the `NAZM_REQUIRE_DOCKER=1` guard: without it the test refuses rather than passing; with it and no daemon, it fails (`docker_boundary.rs`'s `docker_or_skip` is the pattern) |
| Never in | `cargo test` defaults, `cargo xtask workspace-tests --host-safe`, or the contained release suite's semantic tests — the ignore attribute keeps it out |
| Prerequisites | Docker reachable; pinned images present (preparation step); a free subnet slot |
| Run | `NAZM_REQUIRE_DOCKER=1 cargo test -p <crate> --test <file> -- --ignored --test-threads=1`, one suite at a time (`docs/internal/runbook.md`, *Serial execution*) |
| Resource budget | per attempt, from the engine's `Limits`: memory and swap equal, CPUs, pids, a wall-clock deadline |
| Readiness | polled against real state with a deadline (`wait_ready` in `docker_boundary.rs`), never a fixed sleep |
| Teardown | the ledger's removal of exactly what it created; a failed teardown is reported, not retried by name |
| Evidence | the test output, plus the ledger file, kept with the run under `target/gates/<run>/`. Today's two tests open their ledger in a temporary directory that is deleted (Q1-A2-09); a new test keeps it |

Today the category holds `crates/nazm-bench/tests/docker_boundary.rs` (PostgreSQL) and
`crates/nazm-cli/tests/resource_regression.rs`. Both predate this rule and carried only the guard, so a default run printed "skipping" and counted
them **passed** on no evidence; M1 gave them the attribute, and a default run now reports them
**ignored**. Every new test in the category uses both from the start. Gate 2's networking will add TCP and HTTP peers;
where a peer can be a process the test itself starts on loopback, it is not an external service
and belongs with the ordinary tests.

## CI

Three tiers, kept distinct:

1. **Host-safe CI** (`.github/workflows/ci.yml`): no external services; these tests are ignored.
2. **Integration-external CI**, if one is ever added: a runner with Docker, a preparation step that
   pulls pinned images, then the category's run command. Not configured today.
3. **The final contained release gate** (`docs/internal/runbook.md`): unchanged by any of this.

Nothing is pushed to trigger remote CI.
