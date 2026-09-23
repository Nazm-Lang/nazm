# Runbook — operating environment and execution safety

**This document owns every operational rule in the repository.** If a heavy workload is
about to run, this is the file to read, and it is the only one that has to be read.

**Facts measured on the development machine on 2026-09-19 and 2026-09-21, not
assumptions.** Re-measure before trusting any figure here; every one is machine-specific
and several are the kind that change without anyone noticing.

---

## Mandatory containment

*Absorbed 2026-09-21 from `NAZM_AUTONOMOUS_GOAL.md` §2.1, which was the completed Era-1
execution contract. These rules did not expire with it.*

### What must never run uncontained

Only through the verified contained runner, `cargo xtask contained <workload>`:

- any generated compiler stage — `nzlex`, `nzparse`, `nzcheck`, `nzemit`, `nazmc`;
- the selfhost suite and `compiler/bootstrap.sh`;
- fuzzing, the mutation catalogue, and heavy benchmarks.

**An environment variable is a declaration, not a ceiling. Never set `NAZM_CONTAINED=1` on
the host, and never use `NAZM_ALLOW_UNCONTAINED=1`.** The runner sets `NAZM_CONTAINED=1`
only *inside* an environment where a real ceiling already applies.

The reason is specific to this platform: **macOS cannot cap a child's memory.** `RLIMIT_AS`
is an alias of `RLIMIT_RSS` there, and `setrlimit(2)` documents it as advisory — the system
*prefers* to reclaim from an offender rather than refusing its allocation. The ceiling must
come from outside the process.

`compiler/bootstrap.sh` and selfhost's four stage builders enforce this themselves: they
refuse to start rather than run unbounded.

### What the runner must enforce

- **Memory and swap set equal.** A cap that can be swapped past is not a cap.
- CPU and pid limits.
- **Wall-clock deadlines** — and a deadline is not a memory bound. Both are required.
- **Bounded stdout and stderr, drained concurrently**, so a flood cannot grow the parent.
- Bounded temporary and artefact storage. A memory limit does not stop a disk fill.
- Descendant cleanup and reaping. Killing a client is not killing the workload.
- **Compilation bounded too**, not only the execution of generated programs.
- **Explicit failure when containment is unavailable.** A silent skip reporting green on no
  evidence is a defect, not a pass.
- Effective limits verified by probes that allocate *and touch* memory, whose own maximum
  stays safe even if enforcement fails. Require a confirmed limit event, not a generic
  nonzero exit. `cargo xtask contained probe` is that check.

Refusal tests prove the gate works; they do not prove self-hosting works. The real selfhost
suite and the real bootstrap must run through the runner and produce correct results within
budget, with verified cleanup.

### Serial execution

One Cargo build job and one test thread where applicable. No competing heavy workloads.
**When a workload exceeds a budget, investigate the cause rather than raising the limit.**
Do not change machine-wide settings and do not delete unrelated files.

### Autonomy limits

Do not publish, push, tag, deploy, buy services, use paid model APIs, expose credentials,
change account permissions, delete unrelated files, reset user work, or prune shared Docker
resources. Do not disable Gatekeeper, SIP or watchdogs, and do not downgrade or reinstall
the OS. A missing remote is not a blocker to local development or local checks.

### Status vocabulary

**passed**, **skipped**, **refused**, **failed** and **unexecuted** are five different
outcomes and are never reported as one another. A timeout is a failure, not a result.

---

## The incident that produced these rules — 2026-09-21

*Condensed from the Era-1 ledger. The full narrative, the panic reports and the before/after
measurements are in Git history and at `~/nazm-incident-2026-09-21/`.*

Three kernel panics (08:12:30, 08:26:49, 08:44:44), all *"watchdog timeout: no checkins
from watchdogd in ~93 seconds"*, while `xtask → cargo → selfhost` ran nine stage binaries
plus `nazmc` at once — `cargo test` uses one thread per core and this machine has ten.

**Cause, measured.** `lexer.nz`, `parser.nz` and `check.nz` accumulated output with
`out = str_concat(out, ..)` in a loop. Each concatenation allocates a new buffer and copies
everything accumulated, and **the generated runtime never frees**, so retained bytes are
quadratic in output length. `emit.nz` had already been migrated to `strs_push` + `str_join`;
the other three had not. Doubling the input quadrupled the peak.

**Repaired**, with byte-identical output across 24 stage/input combinations. `nzlex` on
21,490 B went from 2,889 MB to **7.1 MB**; inputs that had OOM'd at 3 GiB now peak near
11 MB.

**Still open, and not to be reported as closed:**

- `nzemit` was reported at ~17 GB in the panic yet **could not be reproduced** — it
  measures linear and modest. Unexplained.
- The panic's `residentMemoryBytes` sums to 173.9 GB on a 32 GiB machine, so it is not
  disjoint physical RAM and its exact meaning is unconfirmed.
- **Kernel-panic causation is unproven.** A watchdog panic reports starvation, not
  causation, and this was a beta kernel (macOS 27.2, build 26B5086k). The compiler workload
  is a strong suspect, not a demonstrated cause.
- Quiet uptime since is an observation, not proof of stability.

The standing lesson is the one the rules above encode: **a deadline does not bound memory.**
A wall-clock timeout was already in place when the machine went down.

---

## Docker address pool — the most imminent failure, and it is not disk

Docker's built-in default pool is `172.17.0.0/12` carved into /16s, giving fifteen usable
networks (`172.17`–`172.31`). Measured:

```
$ docker network ls -q | wc -l          → 16 networks
$ …inspect each, collect subnets        → 172.17 … 172.30, contiguous
$ docker info --format '{{json .DefaultAddressPools}}'  → null (built-in defaults)
```

**Fourteen of fifteen are already consumed** by unrelated projects on this machine
(`erp_default`, `news_default`, `rx_default`, several `fintant-*`, and others). One /16
remains.

### Measured, versus predicted — kept apart on purpose

**Measured:** fourteen of fifteen default /16s are consumed; this machine's LAN address is
`192.168.0.101` with gateway `192.168.0.1`, i.e. `192.168.0.0/24`.

**Documented behaviour:** when the `172.17.0.0/12` pool is exhausted Docker's next default
pool is `192.168.0.0/16`, allocated in /20 chunks — the first of which, `192.168.0.0/20`,
overlaps this machine's LAN.

**Not established:** that Docker *would* actually hand out the overlapping subnet. Its IPAM
performs some host-route conflict avoidance, and whether that is sufficient here has not
been tested — deliberately, because testing it means provoking the failure on a machine
someone is using. So this is a **potential conflict**, not a predicted outage.

The distinction matters for how much weight the mitigation carries: the harness allocates
from an explicitly configured block precisely so that none of this has to be predicted
correctly. The overlap is a reason not to rely on the default allocator, not a forecast.

**Consequence for the harness:** a single leaked network per attempt exhausts the
remaining /16 quickly, after which behaviour depends on a fallback path nobody here has
tested.
Networks are therefore allocated from an explicitly reserved block with a collision check
at allocation time, ownership tracked, and cleanup verified.

**On the reserved block.** `10.201.x.0/24` was proposed. Measured: no `10.x` routes exist
on the host (`netstat -rn -f inet`), and the only non-loopback address is the 192.168 LAN
one. Four `utun*` interfaces exist (VPN/tunnel), currently carrying no `10.x` routes — but
a VPN that comes up later could. **So the block is a configurable default, not a
reservation**, and the allocator checks for collision at the moment it allocates rather
than trusting this measurement to still hold.

The harness never modifies or removes a network it did not create.

## Disk

```
$ df -h ~          → 19Gi free of 460Gi (96% full)
```

Docker Desktop's `Docker.raw` grows to its high-water mark and does not shrink when space
is freed inside the VM. Reclaiming inside the VM (`docker builder prune`, `image prune`)
frees space for containers but does not necessarily return it to macOS.

**Reclaiming space is an operator action, never an agent's.** No run prunes anything —
not `docker container prune`, `volume prune`, `network prune`, `image prune`,
`builder prune` or `system prune` — and nothing stops, removes, recreates or renames a
Docker resource it did not create. The harness cleans up only what it owns, through the
contained runner, and verifies that it did. If the disk is too full to run, stop and say
so; the operator decides what to reclaim, and whether `Docker.raw` then needs Docker
Desktop's own purge to return the space to macOS.

This paragraph used to say *"Before a long run: prune"*, and on 2026-09-23 a run did
exactly that: `docker container prune` removed the exited containers of unrelated projects
on this machine. Their volumes were reported intact and were left alone. A prune is
machine-wide by construction — it cannot tell this project's resources from anyone
else's — which is why it is never the harness's to run.

## Toolchain

```
$ rustc --version  → 1.98.1 (pinned in rust-toolchain.toml)
```

The local `stable` was 1.93.1 (seven months behind) when this project started; the pin is
what makes that irrelevant. `cranelift` 0.135 declares MSRV 1.95.0, so the pin is
load-bearing for Stage 7 even though nothing depends on Cranelift yet.

---

## Recovering an interrupted mutation run

`cargo xtask mutate` mutates the working tree in place and restores each file after
testing it. If the run is killed in between, the tree is left holding a mutation and a
write-ahead record at `target/.nazm-mutation-backup` describing how to undo it.

**Recovery is not automatic, and that is deliberate.** Killing a run releases its lock at
once — the runner's `cargo` and `rustc` descendants never held it, because Rust opens
files `O_CLOEXEC` — so those descendants keep running and keep writing to the tree. The
runner has no way to establish that they have stopped:

* a **pid** may have been reused, and the one in the record belongs to a process that is
  already gone;
* a **content digest** describes the file at the instant it was read, not what will be
  written to it next;
* a **delay** proves nothing at any length.

Restoring the source while one of them is still running, and then injecting the next
mutation on top, would produce verdicts about a file two processes are editing. So a
stranded record makes the next run **refuse to start**, and repair is an operator action.

### The procedure

1. **Establish that nothing is still writing to the tree.**

   These commands help you look. **Neither is proof**, and the difference matters:

   ```sh
   pgrep -g <pgid>        # the interrupted run's process group, if you know it
   pgrep -fl "$(pwd)"     # build and test processes mentioning this path
   ```

   A process-group search misses anything that called `setsid` or `setpgid`. A
   path-or-name search misses a process whose command line does not mention the
   checkout, one that holds an open descriptor to a file whose path has since changed,
   and anything that has not started yet. Both are **investigative aids**; what
   establishes quiescence is your knowledge of what you were running, and nothing here
   can substitute for it.

2. **Repair.**

   > Since the catalogue moved inside `cargo xtask contained mutate`, it runs against a
   > disposable copy on the container's writable layer, never against the checkout. An
   > interrupted contained run therefore leaves **nothing here to recover** — the copy
   > dies with the container, and `git status` on this tree stays clean. What follows
   > applies to a tree that was mutated directly, which is the uncontained path the
   > contract now forbids; it is kept because a tree mutated before that rule still needs
   > repairing.

   ```sh
   cargo xtask mutate-recover --confirm-no-writers
   ```

   The flag is **your attestation that the workspace is quiet** — not a check the tool
   performs, and not a claim the searches above succeeded. The tool has no way to
   perform such a check, which is precisely why recovery is not automatic and why no
   automatic mechanism is being added. Recovery itself is still conservative: it restores the original bytes only
   if the file still matches what the record describes, reports `already-restored` if the
   run was killed after restoring but before clearing its record, and refuses outright if
   the contents match neither. A file you edited by hand since the crash is never
   overwritten.

3. **Re-run.**

   ```sh
   cargo xtask contained mutate
   ```

### If recovery refuses

It will say the file matches neither the mutation it applied nor the original it saved.
That means something changed the file after the interruption. The original is still in
`target/.nazm-mutation-backup`, after its fourth line. Compare it with the working copy,
resolve by hand, then delete the record.

### Interoperating with an older runner

The lock protocol changed: it was a file whose *existence* was the lock, and it is now an
advisory lock (`flock`) on `target/.nazm-mutation-lock.v2`, which is never unlinked. **The
two protocols do not coordinate reliably, and the supported boundary is not to run both
against one tree.** The new runner takes the old marker while it runs so an old runner
usually stands down, and refuses to start if the old marker names a live process — but a
first-protocol runner that has already passed its own check and not yet published is not
covered by anything. Use one version.
