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
| `cargo test --locked --workspace --jobs 1 -- --test-threads 1` | **passed** — **524 passed, 0 failed, 6 ignored, 35 suites**, 0 survivors (previously recorded: 506) |
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
| `use` resolution in the Nazm compiler | **not done** | S is concatenated by the bootstrap script. Stated in `docs/bootstrap.md` |
| Recursion in the Nazm compiler's output | **unguarded** | the Rust backend refuses recursion pending a stack contract; the Nazm one emits it. A real divergence, recorded here |
| Native recursion | blocked by design | pending measured stack availability and frame size. **Not needed**: the compiler is iterative |
| **Structured concurrency, interpreted** | **verified** | `scope`, `spawn`, bounded `Chan`; 15 tests in `nazm-core/tests/concurrency.rs`, **every one with a deadline** so a hang fails by name |
| **Structured concurrency, native** | **verified** | one POSIX thread per task, a mutex/condvar ring per channel; 11 tests in `nazm-cli/tests/concurrent.rs` at `-O0` **and** `-O2`, each against a written expectation *and* the interpreter |
| Concurrency in the *Nazm* compiler | **deliberately absent** | outside the bootstrap subset, named as such in `compiler/analyse.nz` and guarded by a test. See `docs/bootstrap-design.md` |
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

## Current task

Execution prerequisites are met and the goal has resumed. Milestones A-G remain
complete; the work since the incident has been re-establishing every claim **through the
contained runner** rather than on the host, and fixing what that exposed.

Still paused: nothing. `NAZM_CONTAINED=1` is set by the runner inside the container and
is never set on the host; `NAZM_ALLOW_UNCONTAINED=1` is never used.

## Next exact action

1. **Re-run `contained bench`** once the image carrying GNU `time` finishes building, and
   confirm every peak-memory row is populated and plausible. The `/proc` fallback is
   deliberately refusing values under a megabyte after a first pass produced 8 KB and
   16 KB rows, which was the poll losing a race rather than a measurement.
2. **Record a Linux baseline** (`bench --save`) as a *separate* artefact from the Darwin
   one. The tool already refuses to compare across platforms, so the two must coexist
   rather than overwrite.
3. **The mutation catalogue**, contained, against a disposable copy — never this tree.
   `mutate` still resolves `target/` from its `root` parameter, which is correct for its
   throwaway trees but means a contained run needs a writable copy, not the read-only
   mount.
4. **`scripts/release-candidate.sh`**, contained. It calls `compiler/bootstrap.sh`, which
   now refuses without a ceiling, so the script only works through the runner.
5. Fold the numbers into `docs/delivery.md` and close Section 13.

## Unresolved decisions## Unresolved decisions

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
- [x] Structured concurrency: transfer/capture rules, cancellation, cleanup — specified in `docs/spec.md`, interpreted, and compiled; 26 tests, all deadline-bounded
- [x] Developer interfaces, grammar/guide, formatting, repair workflow — `7243447`; schemas in `schema/`, the grammar checked against the lexer and the parser, the guide generated from it, 27 tests across `schemas.rs`, `format.rs`, `grammar.rs` and `workflow.rs`
- [x] Performance baselines, profiling/optimisation pass, no unsupported speed claims — `3bc0505`; `bench/baseline.json` and `docs/performance.md`, which states that nothing has been compared to another language
- [x] Clean rebuild of the integrated state on a supported target — `git archive HEAD` into an empty tree, offline, 506 tests, every gate, byte-identical bootstrap
- [ ] Final local review/release package — `scripts/release-candidate.sh` and `docs/delivery.md` exist; the run that produces the artefact is the remaining step
