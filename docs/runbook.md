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

No competing heavy workloads: one contained workload at a time, and one mutant at a time.
**When a workload exceeds a budget, investigate the cause rather than raising the limit.**
Do not change machine-wide settings and do not delete unrelated files.

*History.* Until N14.1 this section also said "one Cargo build job and one test thread where
applicable". That rule came from the 2026-09-21 incident, when an unbounded selfhost run
took the host down, and it was kept because nothing had measured what parallelism *inside*
a bounded container cost. N14.1 measured it (`performance.md`, *Development and evidence
pipeline parallelism*) and relaxed exactly one thing: the **internal** parallelism of a
workload, chosen by a named profile.

| Workload | Default profile | Why |
| --- | --- | --- |
| `contained tests` | P4T4 — 4 CPUs, 4 jobs, 4 test threads | 3.03× faster, inside every memory gate |
| `contained selfhost` | P4T4 | 3.44× faster, inside every memory gate |
| `contained mutate` | P1 — 1 CPU, 1 job, 1 thread | every parallel profile crossed the total-memory gate |
| `contained bootstrap`, `release`, `run` | P1 | not calibrated |
| `contained bench` | P1, permanently | a measurement shares the machine with nothing |

`cargo xtask contained --profile p1 <workload>` reproduces any older result. What did not
change: containment is mandatory, the memory, swap, pid, storage and deadline ceilings are
the same numbers, workloads never overlap, mutants never run two at once, and a campaign's
journal is bound to the profile it ran under — a resume under another profile reuses
nothing.

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

## Mutation campaigns

*Mutation harness v2, N12.2, 2026-09-24. The mechanism is `xtask/src/campaign.rs`,
`plan.rs` and `procs.rs`; this section is how to operate it.*

### The one command

```sh
cargo xtask contained mutate [--only NAME]… [--strategy targeted|full] [--resume] [--verify-killers]
```

Always contained. One container is one **session** of a campaign: the tree is copied to a
disposable directory on the container's capped writable layer (`.git`, `target/` and every
derived `.nazm/` cache excluded), built once, the workspace suite run once as the
**baseline**, and then every selected mutant is injected, judged, restored, verified and
journaled in catalogue order. `/src` stays read-only and the checkout is never mutated.

### The warm-worker law — N32-H, 2026-09-29

**A session is one warm worker, never one container per mutant.** One copy, one pristine
build, one baseline, one killer listing; then each mutant in turn — at most one injected at
a time, restored to its original bytes and digest-checked before the next is touched —
against the same target directory. The target is the container's own `/work/target`, which
the image warms with the workspace's registry dependencies: it lives on the capped writable
layer, dies with the container, and is shared with nothing. Workspace crates are compiled from
the copy at `/work/mut`, a path no image artefact was built for; only dependencies Cargo's
fingerprints say are unchanged are reused. Nothing is `cargo clean`ed between mutants: Cargo
rebuilds from the bytes on disk, and `xtask/tests/lifecycle.rs` holds that to account — A,
then B, then A again in one target, every killer run observed against its own bytes.

**`--verify-killers` verifies in the same session.** Around the one injection a verdict is
taken from, each declared killer is run on the pristine tree, on the mutant, and on the
restored tree, and is verified only as passes, fails, passes — `--verify-killer`'s law,
unchanged. It changes no verdict and is not journaled, so a resumed session that reuses a
verdict says those killers were not verified here and ends incomplete. `--verify-killer M
--killer K` remains, for proving a new candidate.

**P1 means one active mutation worker**, with one CPU, one job and one thread — not one fresh
container per mutation. No second worker was needed (`performance.md`, *The warm mutation
session*); every parallel profile for this workload crossed the total-memory gate in N14.1.

**Stranded containers.** A supervisor killed by a signal leaves its container running until
its own `timeout`; the checkout is read-only, so the container holds a disposable copy and
nothing of the host's. Every contained run starts by removing, by exact name, any
`nazm-<workload>-<pid>` container whose supervisor pid is no longer a process — and nothing
else. To stop a run at once, signal its whole process group (Ctrl-C does), so the Docker client
forwards the signal and the container ends with it.

### Verification gates

| Gate | What | Target on the M1 Pro | When |
| --- | --- | --- | --- |
| **DEV** | focused tests, host probes of new mutations, `fmt`, `clippy`, `cargo xtask check` | < 15 min | while iterating |
| **MILESTONE** | every affected killer verified and one targeted campaign, in one warm session; the contained workspace suite; the lifecycle suite; selfhost; bootstrap; the milestone's own benchmark; the gates | < 60 min | to close a milestone |
| **RELEASE** | broader and full campaigns, fuzzing, platforms | as long as it takes | a release, not every milestone |

MILESTONE is not DEV with a new name: its speed comes from not repeating setup, never from
dropping a killer, the campaign, the suite, selfhost or bootstrap. N32's gate measured 40 min
(`performance.md`).

### Evidence tiers — escalate only while a mutant is still alive

The default strategy, `targeted`, runs for each mutant, stopping at the first failure:

1. **Direct**: each verified killer the entry names, one exact test each;
2. **Focused**: each whole target of the entry's profile (`[[profile]]` in the catalogue);
3. **Full**: the workspace suite, exactly the legacy invocation.

A failure at any tier is **CAUGHT at that tier**, and it is the same evidence as a failure
in the workspace suite: the killer is a member of that suite, it passed in the baseline,
and it failed with the mutant installed. **Only tier 3 can report SURVIVED.** A killer or
a profile that passes escalates; it never concludes.

`--strategy full` is the legacy shape — build, then the workspace suite, for every mutant —
and is kept as the oracle the targeted strategy is compared with.

### Verdicts, and what is never converted into one

| word | meaning |
| --- | --- |
| `caught` | a tier's tests failed with the mutant installed |
| `SURVIVED` | every step passed, the workspace suite last |
| `UNUSABLE` | the mutated tree does not build, tests included |
| `NOT INJECTED` | the pattern matched zero or several times, or changed nothing |
| `MUTANT CRASH` | a test binary died with the mutant installed |
| `TIMEOUT` | a step passed its deadline — **no verdict** |
| `RUNNER ERROR` | a step did not run to completion — **no verdict** |
| `NOT RESTORED` | the source could not be put back — the run stops |

A timeout is never caught and never survived. A no-verdict mutant is not escalated past:
a later tier passing would not mean the earlier one said nothing.

### Serial, and alone

One mutant at a time, and nothing else heavy on the machine: no bootstrap, no benchmark, no
second campaign, no full verification. The speed of v2 comes from running fewer tests, which
holds on a constrained machine; it must never come from running mutants concurrently, which
makes deadline-bearing verdicts a function of scheduling.

A campaign runs under P1 — one CPU, one Cargo job, one test thread — by default. N14.1 tried
P4T4 and a two-job variant: verdicts and tiers matched P1, but both crossed the frozen
total-memory gate (`performance.md`), so the default did not move. `--profile` still sizes
each step if given, and the profile is part of the campaign identity: a journal recorded
under one profile is never read by a resume under another.

### Deadlines and process trees

Every build and test the campaign starts runs through one subprocess runner
(`procs.rs`), in **a process group of its own** with a **watchdog** in a separate group:

* at a deadline, the tree is walked by parent id and stopped, then killed — so a compiled
  program a test put in *another* group dies too;
* after an ordinary exit, anything left in the group is swept, so a grandchild holding a
  pipe cannot hang the reader;
* if the harness itself dies (SIGINT, SIGKILL, a panic), the watchdog sees its pipe close
  and kills the group.

Deadlines bound pathology, not speed: build 1800 s, a killer 1200 s, a profile target
1500 s, the workspace suite 2400 s. Lowering them to make a campaign faster converts slow
catches into no-verdicts.

**`kill -KILL -<pgid>` does not work in the container.** procps `kill` reads `-123` after a
signal as an option and signals nothing; `kill -KILL -- -<pgid>` works there and on macOS.
N12.2 found N12.1's test deadlines using the first spelling and corrected them.

**A test that runs a compiled program needs its own deadline.** `nazm build` emits no
iteration budget, so a backend mutant that makes a loop run away makes a bare `.output()`
wait for ever. `value_blocks.rs`, `schemas.rs`, `workflow.rs` (N12.1) and `memory.rs`
(N12.2) bound theirs; a new suite that runs programs must too, or its tier times out and
the mutant gets no verdict.

### Sessions, the journal and `--resume`

A session has a 3000 s container deadline and a 2400 s budget: past the budget it starts
no new mutant, ends **incomplete** (exit 3), and says which mutants were not reached.
Nothing is established about those. A mutant still in flight at the deadline is killed
with the container, is never journaled, and runs again; that is also reported as
incomplete, never as a memory limit. Continue with the same selection:

```sh
cargo xtask contained mutate [same --only …] --resume
```

Each finalized mutant — verdict known, source restored **and read back** — is printed as a
journal line and collected on the host at `target/mutation-campaigns/<key>/journal.jsonl`.
`<key>` is a digest of the campaign's **identity**: the tree's contents, the catalogue, the
harness, the toolchain, the strategy, the selection, the deadlines and the cache-isolation
rule. A resumed session reuses only verdicts journaled under the identical key; a changed
file anywhere, a changed catalogue or a different selection is a different campaign and
reuses nothing. A timeout or runner error is never reused — it is attempted again.

The journal is evidence, not a cache: an old verdict is never inferred from an old binary or
log. Journal lines reach the host when the container ends; a supervisor killed with the
container running loses that session's lines, and those mutants are simply run again.

### Authoring a mutation (the N13 workflow)

1. Write the semantic regression test that observes the rule.
2. Add the `[[mutation]]` entry; `cargo xtask check-mutations` validates it statically.
3. Verify the killer — pristine passes, mutant fails, restored passes:

   ```sh
   cargo xtask contained mutate --verify-killer NAME --killer nazm-cli:test=memory:the_test
   ```

   (`PACKAGE:lib:PATH`, `PACKAGE:test=T:PATH` or `PACKAGE:bin=B:PATH`.) It prints the
   `killers = [...]` line to add.
4. Add it, then `cargo xtask contained mutate --only NAME`, which now stops at tier 1.
5. Commit.

`cargo xtask mutate --plan [--only NAME]` prints each mutant's tiers and runs nothing.

A killer is authoritative only after step 3 observed it. Never write one from a test's
name; never name the workspace suite or the bootstrap as a killer — an entry with no narrow
killer simply has none, and says why in `no_killer` if it names a profile.

### Artefacts and disk

Everything a campaign writes on the host is under `target/mutation-campaigns/<key>/`. The
Cargo build of a session lives in the container and goes with it (about 2.6 GB of the 8 GB
writable layer). Nothing here cleans up automatically, and nothing ever runs `cargo clean`,
`rm -rf target` or a Docker prune; delete a campaign's directory by hand when it is no
longer evidence.

## Recovering an interrupted mutation run

*This section is about a tree mutated in place, which only an uncontained run does. A
contained campaign (above) mutates a disposable copy and leaves nothing here to recover.*

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
