# Goal execution ledger

Contract: `docs/NAZM_AUTONOMOUS_GOAL.md` **v1.1** (Section 2.1 makes containment a condition of execution). This file is the resume point. **On
resumption, check it against Git and running processes before trusting it** — it records
what was verified when it was written, not what is true now.

---

## Position

| | |
| --- | --- |
| Branch | `main` (renamed from `master`; no remote configured, local-only is not a blocker) |
| Goal base commit | `4326100` |
| Current commit | `6ec965d` (repair `011d7ff`, contract v1.1 `fc21968`, runner `a74bdba`) |
| Working tree | the contained-runner work is in progress on top of `a74bdba` |
| Platform verified | aarch64-apple-darwin (Darwin arm64), rustc 1.98.1 pinned, Apple clang 21.0.0 |
| Disk | 50 GiB free of 460 GiB (88 % full) after reclaiming 33.5 GiB of `target/` on 2026-09-21 |

## Execution prerequisite: the contained runner

Contract v1.1 §2.1 forbids running generated stages, the selfhost suite, the bootstrap,
fuzzing, the mutation catalogue or heavy benchmarks outside a verified ceiling.
`NAZM_CONTAINED=1` is set by the runner **inside the container only**; it is never set on
the host, and `NAZM_ALLOW_UNCONTAINED=1` is never used.

    cargo xtask contained probe        # verify every bound is real on this daemon
    cargo xtask contained selfhost     # the real suite, inside the ceiling
    cargo xtask contained bootstrap    # the real bootstrap, inside the ceiling
    cargo xtask contained run '<cmd>'  # anything else, bounded

Image `nazm-contained:1.98.1` — rust 1.98.1 exactly as `rust-toolchain.toml` pins, clang
19 (clang 17 rejects at least one emitted module; tracked separately).

**Bounds, and how each was verified rather than assumed** — `contained probe`, **6/6
passed, 0 survivors**:

| bound | mechanism | probe result |
| --- | --- | --- |
| memory | `--memory` | **passed** — 400 MiB allocated *and memset* against a 256 MiB cap is OOM-killed |
| swap | `--memory-swap` = `--memory` | **passed** — `memory.swap.max` reads 0 inside |
| CPU / pids | `--cpus`, `--pids-limit` | **passed** — `cpu.max`, `pids.max` read back inside |
| disk | `--storage-opt size` | **passed** — supported on this daemon |
| scratch | sized `tmpfs` | **passed** — refuses to grow past its size |
| single file | `--ulimit fsize` | **passed** |
| deadline | kill the **container**, not the client | **passed** — 120 s sleep killed at 20 s |
| output | concurrent drain, byte cap | **passed** — classified `output-overflow` |
| descendants | `--init` + teardown | **passed** — detached grandchild reaped, 0 survivors |

The first memory probe piped `head` into `tr`, which streams through small buffers and
never allocates; it reported a pass and proved nothing. Replaced with chunked
`malloc`+`memset`. Recording it because a probe that cannot fail is worse than none.

Outcomes are five distinct words — **passed, skipped, refused, failed, unexecuted** — and
the runner never collapses them: `Unavailable` is a refusal with an explanation, not a
skip.

## Verified through the contained runner (2026-09-21)

Everything below ran inside the ceiling, serially, one workload at a time. Words are used
strictly: **passed** / **skipped** / **refused** / **failed** / **unexecuted**.

| workload | result |
| --- | --- |
| `contained probe` | **passed** — 6/6 bounds real, 0 survivors |
| `contained selfhost` | **passed** — 15/15, 0 survivors |
| `contained bootstrap` | **passed** — `ir: identical`, `runnable-bytes-differing: 0`, 5 conformance cases |
| `cargo test --locked --workspace --jobs 1 -- --test-threads 1` | **passed** — **548 passed, 0 failed, 6 ignored, 35 suites**, 0 survivors at `c3fb078` (previously recorded: 524, and 506 before that) |
| `cargo fmt --all -- --check` | **passed** |
| `cargo lint` | **passed** |
| `cargo xtask check` | **passed** — 6/6 gates (layering, codegen reach, emission reach, unsafe policy, doc claims, guide from grammar) |
| `contained bench --save` | **passed** — recorded `bench/baseline-linux-aarch64.json`; the Darwin `baseline.json` untouched |
| `contained bench` (check) | **passed** — nothing slower than the Linux baseline by more than 25% and 5 ms |
| mutation catalogue (3 targeted) | **1 failed, 2 re-running** — see below; contained, disposable copy, 0 survivors |

### Mutation re-check: one regression of mine, one real finding

Two of the three entries came back **NOT INJECTED**, and that was **my regression**:
`the-statement-conditional-selects-the-wrong-branch` and
`the-value-conditional-selects-the-wrong-branch` both `find` on
`format!("b{}", self.fresh())`, which the label-counter change renamed to
`fresh_label()`. The catalogue mutates source by literal text, so renaming the source
silently disarmed two of its fifty-one entries — they reported NOT INJECTED rather than
failing, which is precisely how a mutation suite quietly stops testing anything. Four
occurrences realigned.

The third, `the-loop-reuses-its-first-condition`, **injected** and then took the whole
container to the 4 GiB ceiling (`Killed`, outcome `memory-limit`, 0 survivors).

Cause, investigated rather than accommodated: the mutation makes the emitter's loop reuse
its first condition, so emission never terminates. `nazm-lir`'s emitter accumulates the
whole module in an `out: String` and `build.rs` writes it only at the end, so a
non-terminating emit loop grows that string without bound. `build_at` *does* bound the
call — with `run_bounded` and a deadline — but a deadline is a time bound and cannot stop
an allocation. That is the same lesson as `gtimeout` in the original incident.

The budget was **not** raised. Two honest readings, and they are different claims: the
mutation is demonstrably detected, because the mutated compiler cannot finish; but it was
detected by the ceiling rather than by an assertion, so it is not "caught" in the sense
the catalogue means. On Linux — unlike Darwin — `ulimit -v` is enforced, so bounding the
compiler child inside the container is the available fix. **Open.**

### Both emitters now number temporaries sequentially

The Rust emitter and `compiler/emit.nz` each drew SSA temporaries and block labels from
one counter. Labels are *named* (`b7:`, not `7:`) so LLVM never counts them, and every
label left a hole: `%5` emitted where the implicit counter was still at 3. clang 19 and
Apple clang 21 tolerate it; **clang 17 rejects it**.

Fixed in both (`5cba009`, `e2f75c9`). Verified end to end: Alpine clang **17.0.6** now
assembles both a single module and the **whole self-hosted compiler** emitted by the Nazm
compiler, and the C2/C3 fixpoint is undisturbed. This closes the open item recorded
earlier as "one module on one version pair" — the cause is now identified and removed
rather than merely observed.

### Benchmarks: resolved, one baseline per host

The conflict was real — `bench/baseline.json` is **aarch64-apple-darwin** and the runner
is Linux — but the tool already refused to compare across hosts on its own. So the fix
was arrangement, not policy: whichever host owns the legacy name keeps it, and every
other host gets `bench/baseline-<host>.json`. Nothing was migrated and the Darwin numbers
are untouched.

`contained bench` mounts the source read-write, alone among the workloads, because its
product is a file in the tree. That is a filesystem choice; the resource bounds are
identical either way.

**Peak memory is measured again, and exactly.** Every row had read "not measured" since
the move into the container: `peak_rss` shelled out to `/usr/bin/time -l`, the BSD
spelling, and the slim image shipped no `time` at all. It now prefers GNU `time -v`
(getrusage, exact even for a program that exits immediately) with `/proc/<pid>/status`
as a fallback. A first attempt using only `/proc` produced rows of 8 KB and 16 KB —
the poll losing a race with a short program, not a measurement — so the fallback now
refuses anything under a megabyte. All 14 rows populated, 1.2-6.2 MB.

## HALT — resource incident, 2026-09-21 (read before running anything)

Three kernel panics (08:12:30, 08:26:49, 08:44:44), all
`watchdog timeout: no checkins from watchdogd in ~93 seconds`, while
`xtask -> cargo -> selfhost` ran nine stage binaries plus `nazmc` at once
(`cargo test` uses one thread per core; `hw.ncpu = 10`).

**Cause, measured.** `lexer.nz`, `parser.nz` and `check.nz` accumulated their output with
`out = str_concat(out, ..)` in a loop. Each concatenation allocates a new buffer and
copies the whole accumulated string, and the generated runtime never frees, so retained
bytes are **quadratic in output length**. `emit.nz` had already been migrated to
`strs_push` + `str_join` for exactly this reason (see the note above `join`); the other
three had not. Measured on `nzlex`, doubling the input quadrupled the peak.

**Repaired.** All three now push parts and join once. Byte-for-byte identical output on
24 stage/input combinations, identical exit codes, identical diagnostics on malformed
input. Peaks below.

| stage | input | before | after |
| --- | --- | --- | --- |
| `nzlex` | 21,490 B | 2,889 MB | **7.1 MB** |
| `nzlex` | 43,690 B | OOM at 3 GiB | **7.4 MB** |
| `nzlex` | 88,490 B | OOM at 3 GiB | **11.1 MB** |
| `nzparse` | 21,490 B | 1,572 MB | **7.4 MB** |
| `nzcheck` | 34,117 B (1,600 diagnostics) | 501 MB | **6.9 MB** |
| `nzcheck` | 139,717 B (6,400 diagnostics) | OOM at 3 GiB | **11.1 MB** |

Measurement conditions, identical before and after: binaries baked into the image, inputs
on a read-only bind mount, one fresh container per run, no compilation or supervisor
inside the measured container. `memory.peak` is therefore execution plus page cache for
the input and output — **not** clang, scratch, or a supervisor.

**Containment is now required, not optional.** `selfhost`'s four stage builders and
`compiler/bootstrap.sh` refuse to run without `NAZM_CONTAINED=1`, because macOS cannot
cap a child's memory: `RLIMIT_AS` is an alias of `RLIMIT_RSS` and `setrlimit(2)`
documents it as advisory. `NAZM_ALLOW_UNCONTAINED=1` overrides, loudly. A bare
`cargo test --test selfhost` now fails 12 tests at the gate and spawns nothing.

**Still open — do not treat as closed.**

- `nzemit` reached ~17 GB in the panic report, but `emit.nz` was already migrated and
  **could not be reproduced**: it measures linear and modest (49 MB peak for 1.5 MB of
  output; 7 MB on the one-line programs `selfhost` actually feeds it). Unexplained.
- The panic's `processByPid.residentMemoryBytes` sums to 173.9 GB on a 32 GiB machine, so
  it is not disjoint physical RAM and its exact meaning is unconfirmed. The near-uniform
  ~16-17 GB across five different programs is as consistent with a measurement artefact
  of the stalled state as with independent allocation.
- The compiler workload is a **strong suspect** in the memory-pressure incident. Complete
  kernel-panic causation remains **unproven**: a watchdog panic reports starvation, not
  causation, and this is a beta kernel (macOS 27.2, build 26B5086k).
- Clang 17 rejected one emitted module (`review/m1e/control.ll:44`: *instruction expected
  to be numbered '%3'*) and clang 19 accepted it. That is one module on one pair of
  versions — it does **not** establish a general minimum version, and the skipped
  numbering is not proven to be the reason. Reproducer preserved; handle separately.
- Quiet uptime since 08:44 is an observation, not proof of stability.

Evidence: `~/nazm-incident-2026-09-21/` (panics, jetsam, 54 crash reports, before/after
IR and sources, Gate A and root-cause write-ups).

## What is done, and what is not

| Milestone | State |
| --- | --- |
| **A** — scalar control flow, incl. `return` | **complete** (`ac39f7b`) |
| **B** — the bootstrap subset | **complete** (`053ed22`): B1 text, B2 sequences, B3 I/O, B4 modules, B5 strategy |
| **C** — compiler in Nazm | **complete for the bootstrap subset**: lexer, parser, name resolution, type checking and LLVM emission, all in Nazm, all differentially verified |
| **D** — C1→C2→C3 bootstrap | **complete**: `compiler/bootstrap.sh`; C2 and C3 byte-identical, 5 conformance cases agreeing with the interpreter. Caveats in `docs/bootstrap.md` |
| **E** — structured concurrency | **complete**: specified first, then interpreted, then compiled. `scope`/`spawn`/`Chan` run natively as one POSIX thread per task |
| **F** — AI-native developer experience | **complete**: versioned schemas, a checked grammar artefact, a generated guide, a canonical formatter, fixes with preconditions, a capability inventory, and one scripted repair workflow |
| **G** — performance and release integration | **complete**: a measurement suite, a recorded baseline, one profile-driven pass (2.07× on the interpreter's hot loop), and a local release candidate |

## Capability matrix

**verified** = executed evidence against the current commit. **planned** = nothing exists.

| Capability | Status | Evidence |
| --- | --- | --- |
| Lexer, parser, checker, interpreter | verified | 110 language tests |
| Structured diagnostics, codes, `--json` | verified | `nazm-diag`; per-file names in merged buffers |
| Native `Int`/`Bool`, checked arithmetic, comparisons | verified | `-O0`/`-O2` against the interpreter |
| Native value/statement `if`, `while`, `break`, `continue`, `return`, `let mut`, assignment | verified | commits `60709be`, `ac39f7b` |
| Native non-recursive direct calls | verified | `4326100` |
| Native `Str` as bytes + 7 built-ins | verified | `6d722c1`, `23ee970` |
| Native `Ints`/`Strs` sequences + 12 built-ins | verified | `30c7b12`, `9e9a5be` |
| Native arguments, files, output, exit | verified | `23ee970`; **ambient authority, enforced nowhere** |
| Modules: `use`, relative resolution, diamonds, cycles | verified | `053ed22`; **no visibility control** |
| Tokeniser written in Nazm | verified | `compiler/lex.nz` + `lexer.nz`; agrees with `nazm_syntax::lex` token- and span-wise over every `.nz` in the tree |
| Parser written in Nazm | **verified** | `compiler/parser.nz`; the **whole grammar**. Same tree as the reference over 50 hand-written sources **and over its own source and the tokeniser's** — 2000+ nodes, node- and span-identical. Only a bare block used as an expression is refused |
| Name resolution in Nazm | **verified** | `compiler/check.nz`; same `(code, span)` as `nazm_core::check` for `N0200`/`N0201` over 15 sources, and accepts every standalone `.nz` in the tree |
| Tree traversal in Nazm without recursion | **verified** | child index from the post-order arena, then a pre-order walk from a marker-carrying work stack |
| Type checking in Nazm | **verified** | `compiler/analyse.nz`; same `(code, span)` as `nazm_core::check` for `N0300`/`N0301`/`N0303`/`N0304`/`N0310` over 35 sources |
| **LLVM emission in Nazm** | **verified for the whole bootstrap language** | `compiler/emit.nz`: arithmetic with every specified failure, locals, control flow, transfers, functions, calls, `Str`, sequences and all 27 built-ins |
| **Self-hosting** | **verified** | `compiler/bootstrap.sh`: C2 and C3 byte-identical in IR *and* executable; 5 conformance cases agree across C2, C3 and the interpreter |
| `use` resolution in the Nazm compiler | **done** | `compiler/module.nz`; every bootstrap stage resolves its own imports, and the concatenation workaround is gone |
| Recursion in the Nazm compiler's output | **unguarded** | the Rust backend refuses recursion pending a stack contract; the Nazm one emits it. A real divergence, recorded here |
| Native recursion | blocked by design | pending measured stack availability and frame size. **Not needed**: the compiler is iterative |
| **Structured concurrency, interpreted** | **verified** | `scope`, `spawn`, bounded `Chan`; 17 tests in `nazm-core/tests/concurrency.rs`, **every one with a deadline** so a hang fails by name |
| **Structured concurrency, native** | **verified** | one POSIX thread per task, a mutex/condvar ring per channel; 17 tests in `nazm-cli/tests/concurrent.rs` at `-O0` **and** `-O2`, each against a written expectation *and* the interpreter, including that a failure joins its scope before it is reported |
| Concurrency in the *Nazm* compiler | **done** | parsed, checked (`N0320`, `N0321` with the reference's codes and spans) and lowered — the task and channel runtime in `compiler/emit.nz`. Verified through that compiler: the worker pool answers 90, and a failing body or task joins the scope before reporting |
| Formatter, grammar artefact, fix application, benchmarks | planned | nothing exists |

## Verified commands (against `91ec4d6`, before the incident)

| Command | Result |
| --- | --- |
| `cargo test --workspace` | **506 passed, 0 failed, 6 ignored** |
| — of which self-skipping | 10 `docker_boundary` tests exercise nothing without `NAZM_REQUIRE_DOCKER=1` |
| — of which native end-to-end | 126 `nazm build` cases, each at `-O0` and `-O2` against the interpreter |
| — of which self-hosting differentials | 15, including the full bootstrap chain |
| — of which concurrency | 15 interpreted + 11 native, every one deadline-bounded |
| — of which tooling | 4 schema, 11 formatter, 3 grammar, 8 repair-workflow |
| `cargo test -p xtask --test lifecycle -- --ignored` | 6 passed (separate; not in the 443) |
| `cargo lint` (the CI alias) | 0 diagnostics |
| `cargo fmt --all -- --check` | clean |
| `cargo xtask check` | 6/6, including the guide against the grammar |
| `cargo run -p nazm-bench -- selftest` | 19/19 |
| **Clean rebuild from a fresh copy** | `git archive HEAD` into an empty tree: **offline** build in 8.9 s, **506 passed / 0 failed / 6 ignored**, `xtask check` 6/6, `bootstrap.sh` byte-identical with 5 conformance cases |
| `cargo xtask bench --check` | nothing slower than the baseline by more than 25% **and** 5 ms |
| `compiler/bootstrap.sh` | C2 and C3 byte-identical in IR and executable |

Mutation runs this session, each isolated against a disposable copy and each verified
byte-identical on restore: `return-does-not-end-the-block-it-is-in` (caught, 5 tests).
The full 50-entry catalogue has **not** been run since `8d67a7d`.

## Active processes and temporary resources

| | |
| --- | --- |
| Disposable copies | `…/scratchpad/mutcheck`, made with `git archive HEAD`. Deleted when the targeted mutation check finishes |
| Mutation lock | **not held in this tree**. Every mutation run was against a copy; `target/.nazm-mutation-lock.v2` and the backup file are both absent here |

The full catalogue ran to completion against `524cd24`: **48 caught, 0 survived, 2 not
injected** — and the two were already repaired before the run reached them. A targeted
re-run of the repairs uses `cargo xtask mutate --only NAME`.

**If a mutation run is ever interrupted:** the copy is disposable, so delete it and
start again. Never run `mutate-recover` against this working tree — nothing here is
mutated, and the command is for a tree that is.

## Release — experimental candidate at `c3fb078` (2026-09-21)

Milestone **G** is complete and the goal's Section 13 checklist is fully ticked. The
release is **experimental**: one verified target, no package ecosystem, no visibility
control, no native recursion, no cross-language performance claim.

| | |
| --- | --- |
| Tested commit | `c3fb0789bc8b0731bed083f90ab998816ecd8a09`, clean tree |
| Path | `target/release-candidate/` — **under `target/`**, so gitignored and rebuildable, not a durable store. Copy it somewhere kept if it matters |
| Archive | `nazm-release-candidate.tar.gz`, 932,147 bytes, SHA-256 `361347f63a36aaa2c9838c5685f2464f5ec985998e1f3a7af15e17ce45e357ed` |
| `MANIFEST.txt` | 7,611 bytes, SHA-256 `a121c2c46ef5423229427f64844308a5d6973266d32d1b4c52d8c31f5b259171` |
| `PROVENANCE.txt` | 2,447 bytes, SHA-256 `5d52da22c3b0dea8c670283e68a106fcb9a8e04ed0cd0cdf1446e15e82b0d773` |
| Target | Linux aarch64, inside `nazm-contained:1.98.1` — rustc 1.98.1, Debian clang 19.1.7 |
| Contents | 70 files: `bin/nazm` (the **Rust** implementation, which builds C1) and `bin/nazmc-selfhosted` (the **Nazm-written** compiler as C2, byte-identical to C3) — both Linux aarch64 ELF, neither portable off it — plus the bootstrap sources, `examples/`, `schema/`, `docs/`, both benchmark baselines, and the bootstrap record |
| Bootstrap | `ir: identical`, C2 and C3 digests both `6bd29369…`, `runnable-bytes-differing: 0`, 5 conformance cases, 5 sources / 4,501 lines |
| Suite | **548 passed, 0 failed, 6 ignored**, 35 suites, 0 survivors |
| Verified twice | the runner checked the candidate against its own manifest before archiving; the archive was then extracted on the host and re-digested independently — 70 matching, 0 mismatched, 0 missing |

**Test exclusions, stated rather than buried.**

- **6 ignored**, all in `xtask/tests/lifecycle.rs`: they drive real mutation runs, nested
  `cargo` builds, and deliberately competing processes. Run with
  `cargo test -p xtask --test lifecycle -- --ignored --test-threads=1`.
- **10 self-skipping**, `crates/nazm-bench/tests/docker_boundary.rs`: without
  `NAZM_REQUIRE_DOCKER=1` they print `skipping:` and exercise nothing, while still
  counting as passes in the 548.
- The mutation catalogue is **not** in the gate set at all.

**What the package establishes, and what it does not.**

- `PROVENANCE.txt`'s source digest covers the source **files** — `.rs`, `.nz`, `.md`,
  `.json`, `.sh`, `.ebnf` under `compiler crates xtask docs schema examples scripts
  bench/programs`, 140 files. It is **not** a whole-tree digest: 69 tracked files fall
  outside it, including `Cargo.toml` and every crate manifest, `Cargo.lock`,
  `rust-toolchain.toml`, `.cargo/config.toml`, `docker/contained.Dockerfile`,
  `xtask/mutations/mutations.toml`, and every `.expected` fixture. It corroborates the
  commit for the sources; it cannot detect a changed dependency pin or a changed
  expectation. The field in the shipped file is still labelled `source-tree-digest`;
  the script has been corrected for the next candidate.
- **Archive reproducibility is not established.** The archive was produced once, on one
  host. Rebuilding it has not been shown to yield the same bytes and nothing normalises
  timestamps or the link step. Byte-identity is established between **C2 and C3**, which
  is a different claim.

**Documentation committed after the tested commit.** `docs/delivery.md`,
`docs/performance.md`, `docs/goal-status.md` and the provenance labelling in
`scripts/release-candidate.sh` were changed **after** `c3fb078` and are therefore **not**
in the package and **not** covered by its test run. Two checklist lines in
`docs/delivery.md` and `docs/goal-status.md` had likewise moved between the first
validated run and `c3fb078` — `git diff 023364d c3fb078` is exactly those two files, no
code.

## Remaining limitations

Open, and none of them a blocker to the release as labelled:

1. **`bench/baseline.json` (Darwin) is stale.** Two things changed under it — the failure
   protocol's per-call check and the compiler benchmark's input moving from a four-file
   concatenation to the real five-file source. Re-recording it means running generated
   programs on the host, which the resource contract forbids, so it stays as it is and is
   **not a valid comparison** until someone re-records it on a Darwin machine.
   `bench/baseline-linux-aarch64.json` is current.
2. **The whole-catalogue mutation run timed out unexplained.** It reached its 3600 s
   deadline without ever reaching injection; its entire output was
   `running 1 of the catalogue`, `oom_killed=false`, `survivors=0`, cleanup verified.
   Recorded as **incomplete, not as a result**. The first guess — a cold build cache —
   is measured and wrong: a cold `cargo build -p nazm-cli` in the disposable copy is 30
   crates in about 15 s. **Do not restart a full run until the hour is understood.**
   Kept strictly separate from the focused catch below, which stands on its own.
3. **`the-loop-reuses-its-first-condition` is caught**, established by a focused run in
   30.5 s: green baseline, injection verified by counting sites (`br label %{head}`
   2 → 1 at `emit.rs:712`, the back edge), mutant compiled, and
   `the_condition_is_reevaluated_on_every_iteration` failed on its 10 s deadline.
   Container removed, `survivors=0`, this tree untouched.
4. **Recursion in the Nazm compiler's output is unguarded** — the Rust backend refuses
   recursion pending a stack contract; the Nazm one emits it. A real divergence.
5. **The performance decomposition stops one level short.** The 2.00× measured across the
   emitter revision is the revision's **overall** effect, not the isolated cost of the
   failure check: three commits touched `emit.rs`/`ir.rs` between the compared builds.
   `docs/performance.md` says so; isolating further is not started, and should not become
   a broad optimisation project.
6. **The kernel-panic causation remains unproven**, and the `nzemit` ~17 GB event is still
   unreproduced. See the incident section above; nothing there changed.

## Next exact action

**None outstanding — the release handoff is complete.** The candidate exists, its
provenance is recorded and independently checked, and the limitations above are written
down rather than implied.

Before anything heavy resumes: containment is unchanged and non-negotiable.
`NAZM_CONTAINED=1` is set by the runner **inside the container only**, never on the host;
`NAZM_ALLOW_UNCONTAINED=1` is never used; workloads stay serial, one at a time, `--jobs 1`
and `--test-threads 1`. Nothing is pushed, published or tagged — that is a person's
decision.

## Unresolved decisions

- **Recursion: settled.** Iterative algorithms with explicit `Ints` work stacks. Native
  recursion stays refused pending measured stack availability and frame size.
- **Authority: deliberately open.** I/O is ambient and labelled so in `docs/spec.md`. A
  capability parameter is the growth path; `main(io: IO)` is *not* chosen.
- **Visibility: a known future break.** Every top-level function is visible to every
  importer. Adding private-by-default later breaks existing multi-file programs, and that
  has to be settled before this part of the language is called stable. It already bit
  once: `compiler/lex.nz` had to be split from `compiler/lexer.nz` because a library
  carrying a `main` collides with its importer's.
- **`str_join`: settled, and no longer deferred.** Output assembly is an array plus one
  join. It was not an optimisation chosen in advance — the compiler written in Nazm
  exhausted memory assembling its own output with `str_concat` in a loop, which is
  quadratic, and `str_join` is what made self-hosting possible at all.
- **The next interpreter optimisation: known and not taken.** With variable lookup
  fixed, the profile is the `eval` dispatch itself, which means resolving names to slot
  indices during checking. A much larger change, and `docs/performance.md` records that
  the reason to stop was one pass being what the contract asked for — not that there is
  nothing left.

## Definition of done (Section 13)

- [x] Scalar control flow verified, including `return` and transfers inside values — `ac39f7b`
- [x] Bootstrap subset: text, structured data, collections, memory, file I/O, errors, modules — `053ed22`
- [x] Compiler frontend and lowering/emission in Nazm — `82645f2`; external dependencies (`clang`, libc) documented in `docs/bootstrap.md`
- [x] C1 → C2 → C3 bootstrap with conformance and reproducibility — `82645f2`; byte-identical, caveats stated
- [x] Ordinary independent programs compile with the Nazm-written compiler — 5 conformance cases plus ~40 end-to-end programs
- [x] Structured concurrency: transfer/capture rules, cancellation, cleanup — specified in `docs/spec.md`, interpreted, and compiled; 34 tests, all deadline-bounded; both implementations join a scope before reporting a failure
- [x] Developer interfaces, grammar/guide, formatting, repair workflow — `7243447`; schemas in `schema/`, the grammar checked against the lexer and the parser, the guide generated from it, 27 tests across `schemas.rs`, `format.rs`, `grammar.rs` and `workflow.rs`
- [x] Performance baselines, profiling/optimisation pass, no unsupported speed claims — `3bc0505`; `bench/baseline.json` and `docs/performance.md`, which states that nothing has been compared to another language
- [x] Clean rebuild of the integrated state on a supported target — `git archive HEAD` into an empty tree, offline, 506 tests, every gate, byte-identical bootstrap
- [x] Final local review/release package — produced and validated inside the contained runner at `c3fb078`: every gate passed (fmt, lint, `xtask check`, the whole suite, release build, the bootstrap chain, benchmarks against the re-recorded baseline), the candidate was checked against its own manifest — **70 files**, every digest matching, counts compared both ways so a file copied in but not listed fails — and it was then extracted and re-digested independently on the host. See **Release** below for the path, size, digests and the limits of what that establishes
