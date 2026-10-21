## Context

Surveyed 2026-10-20 at `637a62ba`. The released `nazm` is the Rust workspace (`crates/`, ~170k
lines); 0.10.0 is released for `aarch64-apple-darwin` only. Written in Nazm and compared with it:

| piece | where | state |
| --- | --- | --- |
| front end | `compiler/` (`diagnose.nz`) | Core IR `/3` with sources; R2 3.1, 3.2, 3.4 still open |
| pipeline | `toolchain/pipeline/` | MIR, LIR, runtime units (LLVM IR text) for macOS aarch64/x86_64, Windows x86_64, Linux (internal) |
| code generator | `toolchain/codegen/` | LIR → Mach-O, ELF, COFF objects, line tables, unwind |
| interpreter | `toolchain/interp/` | `run --interpret`'s behaviour on the R4 suites |
| commands | `toolchain/commands/nazm.nz`, `mcp.nz` | 23 of 43 commands; the MCP server less three tools |
| harness | `toolchain/harness/` | `nazm-check` (16 gates), `nazm-mutate` (plan, apply, try), 175 CLI test files |
| self-build | `toolchain/selfbuild/selfbuild.sh` | Linux aarch64, contained, stages 1–3 identical (R6) |

The builder is three programs — front end, pipeline, code generator — glued by a shell script; the
program units it writes are the code generator's, the runtime's are LLVM IR text assembled by the
platform's `clang -c`, and `cc` links. There is no `nazm build` written in Nazm.

## Goals / Non-Goals

**Goals:** the plan for 10.1 and 10.2 — every user-visible capability of the Rust `nazm` either
carried by the Nazm-built one, or named here as dropped by a decision of the copyright holder; an
order of slices in which every slice leaves a working, tested tree; slice 1.

**Non-goals:** the release gate (10.3); new language; performance work beyond keeping the LLVM path
(the measured comparison is the quiet window's); timings.

## What the released `nazm` must still do

### Commands

Carried today (R5), each compared byte for byte with the Rust command over every input:
`fmt`, `init`, `check`, `doc`, `fix`, `lsp`, `test`, `interface`, `docs`, `bindgen`, `reload-check`,
`doctor`, `lock`, `update`, `publish`, `yank`, `snapshot`, `delta`, `diagnostics`, `resolve`,
`references`, `patch` (`fix`, `rename`, `apply`), `attest` (`sign`, `verify`), and `nazm-mcp`.

Not carried, with what each waits on:

| command | Rust | waits on | slice |
| --- | --- | --- | --- |
| `build` | `build/` 3.8k + `nazm-lir` 15.6k | a driver in Nazm (none exists); LLVM IR text of program units (only the runtime's is generated in Nazm) | 4, 5, 6 |
| `run` | `compiled_run.rs`, `interpret.rs` | `build` (the default leg was Cranelift, which is not ported: design decision 3 of `rewrite-in-nazm`); the interpreter inside `nazm` | 7 |
| `repl`, `comptime` | `live.rs` | the interpreter inside `nazm` | 7 |
| `test` (whole) | carried, but its two legs run the `nazm` `NAZM_TEST_DRIVER` names | `run`, `build` | 7 |
| `core-ir`, `mir`, `lir`, `runtime build`/`verify` | `core_ir.rs`, `mir.rs`, `lir.rs`, `runtime_artifact.rs` | wiring the front end and the pipeline into `nazm`; `runtime build` needs `clang` and `ar` as the Rust one does | 8 |
| `capabilities` | `capabilities.rs` | the Nazm backend's own table (no Cranelift column) | 8 |
| `inspect` | `inspect.rs` | MIR digests (the pipeline has them) and the backend's static tables | 8 |
| `obligations`, `explain-cost`, `explain-flow` | `obligations.rs`, `explain.rs` | MIR facts from the pipeline | 8 |
| `trace`, `profile` | `trace.rs`, `profile.rs`, `run_profile.rs` | `test`'s cases run; the runtime's profile counters | 8 |
| `context` (+ MCP `semantic_context`) | `context.rs`, `nazm-service` | R2: a function's effects — what it requires, what a caller must assume, the first call that requires each — are not published by `compiler/facts.nz` | 9 |
| `repo` (+ MCP `repository_map`, `task_context`) | `repo.rs`, `nazm-repo` 3.8k | a port of the map and the task planner | 9 |
| `bench` | `bench.rs` | `build`; timings | 10 |
| `accel` | `accel.rs` + two C providers | `build`; the providers are C the platform's `clang` compiles | 10 |
| `contract` | `contract.rs`, `contract_wasm.rs` | the EVM backend and the WebAssembly target — **deferred targets** (question 1) | 10 |

R5's own refusals, which are also something the user has today (slice 11): `check` with
`--cache-report` and the cache, profiles other than `general`, `--profile-report`, `--source-root`
outside the root module's directory; `lsp` with two open documents, completion, signature help,
semantic tokens, code actions, call hierarchy, rename of an exported name in a declared workspace,
index queries after the errors R2's index is not promised for (`N0203`, `N0305`, `N0334`, `N0386`,
`N0610`, `N0101`); the same index refusals in `patch`, `references`, `resolve` and the MCP server;
`fix` with a directory as its root; TOML forms `toml.nz` does not read; the MCP inputs the port
reads differently (a line that is not JSON, an `id` neither integer nor string, a first message
that is not `initialize`).

### `nazm build`'s flags

Without a Nazm `build` the cut-over has none. The flags fall in four groups:

1. **The driver** (slice 4): FILE, `-o`, `--json`, `--target` (the macOS triples, Windows x86_64),
   `--objects` with `link.txt`, `--link`, `--no-cache` and the object cache (`toolchain/pipeline/objcache.nz`),
   `--heap-limit` (the counted heap in the LIR, not the self-build's `sed`), `--stack-size`, the
   one-thread decision the self-build makes from the runtime text, `--deny-warnings`,
   `--backend nazm`. TLS's libraries. Linking with `cc`/`clang` (macOS) and MinGW-w64 (Windows).
2. **The LLVM path** (slice 5): `--backend llvm`, the default; `--opt-level`; `--emit-ir`; `--debug`
   on it; `--layout soa`; `--release` (canaries, PIE, strip, ad hoc signature). Needs LIR → LLVM IR
   text in Nazm: a port of `nazm-lir`'s `op/llvm.rs`, `op/soa.rs` and `debug.rs` (~2k lines). Without
   it every program is the R9 generator's code, which is correct but slower (`r9-code-generator`
   tasks: fib 454 against LLVM's 247 on x86_64), and the performance target is out of reach.
3. **Packages and evidence** (slice 6): the manifest's `heap-limit`, `--locked`, `--features`,
   `--function` and the rest of the selection, `--profile`, `--lib`/`--shared` with the header,
   `--runtime DIR`, `--provenance`, `--attest-with`, `--sbom`, `--build-report`, `--summary-json`,
   `--timings`.
4. **Deferred targets** (question 1): `--board`, `--stack-watermark`, and every triple the pipeline
   refuses (R3 8.3): Linux (user-facing), `wasm32-unknown-unknown`, `wasm32-wasip1`, Android, iOS,
   the five boards. `--backend cranelift` is gone by decision 3 of `rewrite-in-nazm`; the flag is
   refused by name with the backend's new list.

### The rest of the toolchain

- **The standard library and the documentation corpus.** The Rust binary carries `library/` inside
  it (`nazm-sema/src/stdlib.rs`, `include_str!`) and its documentation corpus; the Nazm commands read
  them from `NAZM_LIBRARY`. Slice 12 decides between generated modules of string constants (as
  `mcptools.nz` is generated) and a directory installed beside `bin/`; the archive layout follows.
- **The harness** (slice 13, R7 6.2 and 6.3): the contained runner, `host-run`, the mutation
  campaign runner (warm workers, journal, resume, verification cache), `xtask bench` and the
  benchmark crates, fuzzing.
- **The tests are the contract** (slice 14): the black-box suite run against the Nazm-built `nazm`
  (`rewrite-in-nazm` decision 5) — the Rust CLI tests with `NAZM_BIN` and R7's Nazm tests — and R2's
  3.1, 3.2, 3.4 closed by it. `toolchain/harness/tests/stays-rust.txt`'s 64 tests of Rust APIs are
  revisited: each ported through the CLI or kept as a test of the seed.
- **The chain** (slices 15–17): the self-build driven by the Nazm `nazm build`; the released set
  (`nazm`, `nazm-mcp`, and whatever slice 4 puts under `libexec/`) built by Nazm for
  `aarch64-apple-darwin`, `x86_64-apple-darwin` and `x86_64-pc-windows-gnu`; the fixpoint on macOS.
- **The release path** (slice 18): `scripts/host-archive.sh`, `release-candidate.sh`,
  `reproducible-check.sh`, `sign-release.sh`, the contained `release` and `archive` workloads, CI,
  the Homebrew formula — all from the Nazm-built binaries.

### What stays the Rust seed

Every crate under `crates/` (frozen at the cut-over commit, changed only to keep the seed building
the toolchain: `rewrite-in-nazm` decision 4), with its Rust-API tests and `fuzz/`'s cargo-fuzz
targets as tests **of the seed**; `nazm-codegen-clif` (Cranelift) lives only there. `xtask` stays
until slice 13 replaces what it runs; after that only what builds the seed. Nothing of the seed is
shipped.

## Decisions

1. **The order is by what unblocks the most with the least, and every slice keeps the tree green.**
   No slice switches a user-facing path before its Nazm replacement agrees with the Rust one on the
   suite that slice names; the released binary changes only at slice 18.
2. **Each port is compared with the Rust command it replaces, byte for byte, as R5 does**, and a
   difference is a defect of the port; a refusal is counted apart, with its reason, and every
   refusal is on this design's list or a slice's.
3. **Slice 1 is R6's first gap**, not the `build` driver: 14 lines against several thousand, and it
   unblocks three slices (7's interpreter inside `nazm`, 13's `nazm-check` as the gate, 14's suites)
   and puts two more programs in the self-build's fixpoint. The driver comes after the scripts that
   test it are lawful under the R6 rule (slice 2) and after Core IR's two missing positions
   (slice 3), which a `--debug` build needs.
4. **Model per slice.** Marked **Opus** where the slice makes design decisions that later slices
   rest on: 4 (the driver's process structure — the front end, pipeline and code generator as
   modules of one program or as `libexec/` children, the object cache's identity, the counted heap
   in the LIR), 7 (`run` without Cranelift, the interpreter inside `nazm`), 13 (the contained
   runner and fuzzing in Nazm), 16 (the macOS fixpoint, after question 2). The rest are ports
   against an oracle (Sonnet).

## Slices

Sizes: **S** under ~300 changed lines, **M** under ~2k, **L** under ~6k, **XL** more.

| # | slice | size | model | leaves |
| --- | --- | --- | --- | --- |
| 1 | R6 gap 1: interpreter and `nazm-check` on `Chain[T]`; both in the self-build, compared there | S | Sonnet | **done** (`tasks.md` 1) |
| 2 | Every comparison script lawful under R6: built by the checkout with `--heap-limit`, run contained or through `host-run`; `host-run` passes the exit status through and reports apart from stdout | M | Sonnet | the suites runnable as the runbook allows |
| 3 | R6 gap 2: `callee_at` and a payload binding's `at` in `--coredoc-sources` (R2, `compiler/`) | M | Sonnet | Core IR equal to the reference's whole; equal line tables |
| 4 | `nazm build` in Nazm, the driver (group 1); `selfbuild.sh`'s glue replaced by it | L | **Opus** design, Sonnet port | `build --backend nazm` agreeing on the corpus |
| 5 | LIR → LLVM IR text in Nazm; `build`'s group 2 | L | Sonnet | the default backend, `--release`, `--debug` |
| 6 | `build`'s group 3 and `nazm runtime build`/`verify` | M | Sonnet | `build` whole for the kept targets |
| 7 | `run` (compiled and `--interpret`), `repl`, `comptime`, `test` without `NAZM_TEST_DRIVER` | M | **Opus** design | the interactive tier |
| 8 | `core-ir`, `mir`, `lir`, `capabilities`, `inspect`, `obligations`, `explain-cost`, `explain-flow`, `trace`, `profile` | M | Sonnet | the inspection commands |
| 9 | R2's effect facts; `context`; `repo` (port of `nazm-repo`); the MCP server's three tools | L | Sonnet | the agent commands whole |
| 10 | `bench`, `accel`; `contract` per question 1 | M | Sonnet | every command routed |
| 11 | R5's refusals (above), with R2's hand-backs | L | Sonnet | no refusal a user meets that Rust does not |
| 12 | The standard library and the documentation corpus without `NAZM_LIBRARY` | S | Sonnet | a `nazm` that runs from its archive alone |
| 13 | The harness remainder: contained runner, `host-run`, mutation campaign runner, bench, fuzz (question 3) | XL | **Opus** design | `xtask` needed only for the seed |
| 14 | The black-box suite against the Nazm-built `nazm`; R2 3.1, 3.2, 3.4 closed; `stays-rust.txt` revisited | L | Sonnet | the tests are the contract |
| 15 | The self-build through Nazm's `nazm build`: seed → 1 → 2 → 3 for the released set, contained | M | Sonnet | the chain of decision 7 on Linux |
| 16 | The fixpoint on macOS, aarch64 and x86_64 (Rosetta), per question 2 | M | **Opus** | 10.1's fixpoint |
| 17 | Windows x86_64 executables built by Nazm; the host-safe suite on the Windows host (`ssh win-dell`); `windows-toolchain` W3 | M | Sonnet | the third release target |
| 18 | The release path switched to the Nazm-built binaries; third-party notices of the shipped binary | M | Sonnet | **the cut-over** (10.1) |
| 19 | The seed frozen: a gate on `crates/` and `xtask/`'s seed part; `docs/internal/bootstrap.md`'s chain | S | Sonnet | 10.1 closed |
| 20 | The authorities (10.2): architecture, bootstrap, capability matrix, support, licensing, runbook | M | Sonnet | 10.2 closed |

Slices 8–12 depend only on 4 (and 7 for `trace`/`profile`); they can run in any order between 7
and 14. Slice 13 may run beside any of them (it touches only `toolchain/harness/` and `xtask/`).

## Open Questions — settled by the copyright holder, 2026-10-20

All four recommendations below were **accepted on 2026-10-20**. On the first, one correction to its
wording: the removed targets are not after-1.0 work. 0.11.0 ships macOS and Windows only, and each
removed target returns with its domain track (`v1-every-domain`) before 1.0, which waits for every
domain. The second extends the R6 rule: the macOS self-build may run on the host as serial
`cargo xtask host-run` steps, recorded in `docs/internal/runbook.md` when slice 16 lands.

The questions as they were put:

1. **The deferred targets.** The Rust `nazm` — the 0.10.0 release included — builds for Linux,
   WebAssembly, WASI, Android, iOS and five boards, and `contract` compiles to EVM and WebAssembly.
   The 2026-10-15 narrowing defers all of them in the Nazm pipeline until after 1.0. A cut-over
   before they are ported takes them away. **Recommended:** cut over without them and record each as
   removed for 1.0 in `docs/user/support.md` and `docs/user/stability.md` (their port is after-1.0 work already
   decided), rather than holding the cut-over for R3 8.3 and an EVM back end.
2. **The macOS fixpoint.** The runbook keeps the self-build contained, and the container is Linux:
   a macOS fixpoint runs macOS executables, stage after stage. **Recommended:** a *serial host
   self-build* — every step one `cargo xtask host-run` (2 GiB, one at a time, the withdrawal
   conditions as now), the stage loop outside the runner — recorded as an extension of the R6 rule.
   The alternative is a macOS virtual machine with a memory ceiling as a second contained runner.
3. **Fuzzing after the seed.** R7 left fuzzing as cargo-fuzz targets of the Rust compiler's
   in-process APIs; after the cut-over they fuzz the seed, not the shipped toolchain.
   **Recommended:** coverage-guided fuzzing of the Nazm programs through slice 5's LLVM path —
   `clang`'s SanitizerCoverage and libFuzzer, the platform's, not bundled — with the R7 differential
   generators beside it; the alternative is differential fuzzing only.
4. **Where the harness must be by the cut-over.** `rewrite-in-nazm` decision 1 puts the harness in
   Nazm before publication. **Recommended:** the released `nazm` cuts over at slice 18 while slice
   13 may finish after it, before the release gate (10.3) — which runs the mutation catalogue and
   fuzzing and so needs it.

## Risks / Trade-offs

- **`build` is the largest single port** and the one everything after it rests on; slice 4 is split
  from 5 and 6 so the driver is settled on the generator the self-build already uses.
- **Containment friction.** Every comparison of a Nazm program is contained or one `host-run` at a
  time; slice 2 makes that a property of the scripts rather than of the person running them.
- **The suite against the Nazm `nazm` (slice 14) will find what the per-command comparisons did
  not** — that is its purpose; it is placed before the chain and the release path, not after.

## Slice 4 — `nazm build` in Nazm: the driver (design, 2026-10-20)

Group 1 of `build`'s flags (above), on Nazm's own code generator. The Rust `build --backend nazm`
is the oracle: its objects, its executables, its words and its statuses (decision 2).

### S4.1 Process structure: the front end in process, the pipeline and the generator as stages

`nazm build` is a command of the Nazm `nazm` (`toolchain/commands/build.nz`). It runs:

| step | where | what it holds |
| --- | --- | --- |
| the front end | **in process**: R2's `fx_check_in`, as `nazm check` runs it (`check.nz`, `front.nz`) | the whole compilation, then nothing: its diagnostics are written as the Rust build writes them and its Core IR document (`fx_core_document_with_sources`, what `compiler/diagnose.nz --coredoc-sources` prints) to the working directory, and the facts are let go before any stage starts |
| the pipeline | **a stage**: `nazm-pipeline build` (`toolchain/pipeline/main.nz`) | MIR, LIR (every unit, the heap's renaming applied), the runtime's units and a plan, written as files |
| the code generator | **a stage**: `nzcg` (`toolchain/codegen/main.nz`), once over every unit | one object per unit |
| the runtime's units | the platform's `clang -c`, as the Rust build compiles LLVM units | one object per unit |
| the link | the platform's `clang` (`cc` driver), MinGW-w64's for Windows | the executable, or `--objects DIR` |

Why not one program: the `nazm` program would carry the pipeline (its runtime text alone is 16k
lines) and the generator beside every command, and the front end building `nazm` itself — the
self-build's largest step, 1.3 GB of 2 GiB — would grow toward the limit; and the three would hold
one heap where now each is freed by its process ending. Why not the front end as a stage too: the
diagnostics must name the files as the user named them, which is `check.nz`'s port (already compared
byte for byte with the Rust `check`); a child would have to run the front end twice or `compiler/`
would grow a third driver mode. The pipeline and the generator are already programs, compared with
their Rust originals (R3, R9), and already in the self-build's fixpoint: the driver adds glue, not a
new builder. `compiler/` does not change.

The stages are found by `NAZM_PIPELINE` and `NAZM_NZCG` (the Rust build's variable), or in the
directory `NAZM_LIBEXEC` names (`nazm-pipeline`, `nzcg`); the standard library by `NAZM_LIBRARY`,
as every R5 command. A missing one is refused by name, status 4, before anything is compiled (the
Rust build's `--backend nazm` with no generator). Where they are installed is slice 12's.

### S4.2 The R6 rule when `nazm build` runs its stages on the macOS host

The rule (`docs/internal/runbook.md`, *What may run on the host*): one Nazm-built program at a time,
built with a counted heap of at most 2 GiB, admitted by `host-run`'s checks, under a deadline, its
peak recorded, anything heavier contained. The driver keeps it as follows.

1. **The driver is a host stage itself.** The Nazm `nazm` is built with `--heap-limit` of at most
   2 GiB and started on the host only by `cargo xtask host-run` (`toolchain/lib/stage-run.sh`), which
   admits it, holds the one-stage lock, and puts it and every child in one process group under one
   deadline.
2. **Each stage is admitted by host-run's checks before it starts.** The driver reads the stage's
   image as `host-run` does (`xtask/src/host_run.rs`): the counted heap linked (`nz.heap_malloc` in
   the file), one `N0406` limit in it, at most 2 GiB. Otherwise it refuses, status 4, with
   `host-run`'s name for the refusal (`no-counted-heap`, `no-heap-limit`,
   `ambiguous-heap-limit`, `heap-limit-too-large`), before anything is compiled. It does this
   everywhere, not only on the host: an unbounded stage is never run.
3. **One stage at a time.** The stages run strictly in sequence, the driver waiting on each. While
   one runs, the driver holds no work of its own: the front end's facts were released before the
   first stage started (its live counted heap is then the paths and the plan), and it starts no
   thread but the one draining the stage's error stream. The generator is run once over every unit,
   where the Rust build runs one `nzcg` per unit in parallel. The pipeline and the generator never
   run beside each other, beside the front end, or beside another `host-run`.
4. **The peak and the withdrawal conditions.** `host-run`'s `/usr/bin/time -l` reports the largest
   resident set of the driver and the children it waited for, and holds it to the driver's limit
   plus 256 MiB; so each stage is built with a limit no larger than the driver's (the self-build and
   the scripts build all four with the same `--heap-limit`). The driver also reads each stage's own
   peak (`process_peak_memory`) and applies the two conditions `host-run` applies, to the stage's own
   limit: a peak more than 256 MiB past it, or an `N0406` that did not end the stage with status 2.
   Either is reported as `WITHDRAWAL CONDITION`, `host-run`'s marker
   (`~/.cache/nazm-host-run/withdrawn`, or under `NAZM_HOST_RUN_DIR`) is written, and the build
   fails; a marker already there refuses the build (`withdrawn`), as it refuses `host-run`.
5. **Heavier stays contained.** A build is one driver and at most one stage. The self-build (many
   builds in a row), the suites and the comparison over many programs run contained, or on the host
   as a script starting one `host-run` at a time (`toolchain/commands/tests/build.sh`).

What this adds to the accepted rule, stated so the copyright holder can see it: while a stage runs,
two Nazm-built processes exist — the waiting driver and the stage — where `host-run` alone admits
one. The driver plays `host-run`'s part for its stage (the same image checks, one at a time, the
same withdrawal conditions); the counted heaps are the driver's small residue and the stage's ≤ 2
GiB, and the measured peaks (S4.6) are recorded with each run. If that reading is not accepted, the
fallback costs nothing in the design: the macOS host builds with a Rust-built `nazm` driving the same
stages, or contained only.

**Accepted by the copyright holder, 2026-10-20:** the waiting driver and one checked stage count as
one stage at a time under the R6 rule (`docs/internal/runbook.md`, *What may run on the host*).

### S4.3 What the stages are handed, and the counted heap in the LIR

`nazm-pipeline [--target T] build CORE.json OUTDIR STACK HEAP` (new, `toolchain/pipeline/main.nz`)
lowers once and writes `OUTDIR/lir.txt` (every unit as `nazm lir --ops` prints it), `OUTDIR/rt/NAME.ll`
(the runtime's units) and `OUTDIR/plan.txt` (the units' artefact names in link order, whether the
program has one thread, whether it calls C, whether it reaches TLS). The renaming `nazm build` does
in Rust is the pipeline's, not a `sed` of the driver's: under a heap limit every unit's imports of and
calls to C's `malloc`, `realloc` and `free` become the counted heap's (`nazm_lir::op::count_heap`),
every runtime unit but the stack's likewise, and the `heap` unit holds the limit; with no limit, a
hosted program that calls no C gets the small-block heap the same way (`small_heap`, `small`). The
runtime is the Nazm backend's: every part (the Rust backend's `baseline`), TLS where a unit reaches
it, `single` as the program decides it — not only the parts the units reach, which is the LLVM
backend's runtime; on Windows the C library's `snprintf` and `strtod` are MinGW's
(`c_library_for`).

### S4.4 One thread, the target and the link

- **One thread** is the pipeline's decision (`main` holds no `SpawnCap`, `NetCap` or `ForeignCap`):
  it goes to the generator as `--one-thread` and to the runtime's `single`, from the plan — not read
  back out of the runtime's text, as `selfbuild.sh` did.
- **Targets:** `aarch64-apple-darwin` (the default on Apple silicon), `x86_64-apple-darwin`,
  `x86_64-pc-windows-gnu` (objects with `--objects`, or linked where MinGW-w64 is the host's), and,
  for the self-build only, the two Linux triples, which must be named (`docs/user/support.md`: Linux
  is not a supported target). The Rust build refuses Linux for `--backend nazm`; the port's
  acceptance of it is the one intended difference, and the comparison does not count it.
- **Compiling the runtime's units and linking** are the Rust build's (`crates/nazm-cli/src/backend.rs`):
  `clang` with an environment of nothing but `PASSED` and `ZERO_AR_DATE=1`; `-O0 -Wno-override-module
  -c` and `-target` for a target not the host's, `-ffunction-sections -fdata-sections` on ELF; the
  link in the working directory under the output's own name, the objects in link order, the system
  libraries after them (none on macOS, `-lm` on Linux, MinGW-w64's on Windows), `-Wl,-dead_strip` or
  `--gc-sections`; TLS's libraries only when the first link fails and the program reaches TLS; the
  output published by rename. `--objects DIR` writes `NN-NAME.o` and `link.txt` as the Rust build
  does.

### S4.5 The object cache

An object is a function of the unit's LIR text, the generator, the target, the runtime and whether the
program has one thread; the key is the BLAKE3 digest of those (the generator's file digest, the
target, the runtime's ABI revision and digest — `nazm-pipeline runtime-digest` — the one-thread bit
and the LIR text), and the entry is R3's envelope (`toolchain/pipeline/objcache.nz`, `nazm.object/1`)
in the project's store, as the Rust build files its Nazm-backend objects. A runtime unit is LLVM text
and is keyed exactly as R3 defined it (`object_key` over the text, the backend identity and the
configuration `clang -###` resolves). The Rust key digests its in-memory LIR module, the port's the
module printed — one function of the other — so the two stores never confuse an entry; they do not
share one either. `--no-cache` compiles everything and reads nothing. The first implementation
neither reads nor writes the store (a cache is derived data: its absence changes no object), as
`check.nz` did with the check cache; the keyed store follows in this slice or the next and is listed
in `tasks.md` until then.

### S4.6 What is carried, refused and compared

Carried: FILE, `-o`, `--json`, `--target`, `--objects DIR`, `--link FILE`, `--no-cache`,
`--heap-limit`, `--stack-size`, `--deny-warnings`, `--backend nazm`, TLS's libraries. Refused by name
with status 6 (R5's convention): `--backend llvm` — the default, so `nazm build FILE` needs
`--backend nazm` until slice 5 — and `cranelift`; every flag of groups 2–4; a package's build (its
manifest's `heap-limit`, `--locked`, `--features`); `--help` and clap's usage errors (the help text is
not ported in this slice); a construct the pipeline cannot lower (its own message, status 6).

The comparison (`toolchain/commands/tests/build.sh`): over `tests/compat/v1/{run,trap,refuse,check}`,
`compiler/conformance/` and `toolchain/codegen/tests/programs/`, the Rust `nazm build --backend nazm`
(its `nzcg` run through a wrapper that takes `host-run` one at a time) against the Nazm-built one
(through `stage-run.sh`): standard output, standard error and status, the executable byte for byte,
and with `--objects` every object and `link.txt`. Agree, differ, refused (status 6, with its reason)
and skipped (the reference cannot build it) are counted apart.

### S4.7 The self-build through `nazm build`

`selfbuild.sh`'s builder becomes `nazm` (the driver and the front end), `pl` and `cg`; `fe` is still
built, for the Core IR reference check. Stage 0 is the seed's `nazm build --heap-limit`; stage *s*
builds every program with stage *s−1*'s `nazm build --backend nazm --no-cache --heap-limit 2G
--target T`, its `pl` and `cg` as the stages. The glue (`build_with`, the `sed`, the `awk` split, the
`grep` for one thread, `clang -c`, `cc`) is gone. The fixpoint compares the executables; the Core IR
check runs the last stage's `fe --coredoc-sources` beside the seed's `core-ir --with-sources`.

### Slice 4 — the evidence

- **The summation rewrite.** The first run found two programs (`tests/compat/v1/run/stdlib.nz`,
  `compiler/conformance/sequences.nz`) whose objects differed: the Rust build rewrites the summation
  idiom in Core IR before MIR (`nazm_service::vectorize`, N66) and `nazm lir --ops`, which R3 was
  compared with, does not. `toolchain/pipeline/vectorize.nz` ports it; `pipeline build` applies it.
- **The comparison**, `toolchain/commands/tests/build.sh` on the macOS host (one `host-run` at a
  time; the reference's `nzcg` through `host-run` too), 2026-10-20: 123 inputs
  (`tests/compat/v1/{run,trap,refuse,check}`, `compiler/conformance/`,
  `toolchain/codegen/tests/programs/`) × 6 modes (`exe`, `heap` with `--heap-limit 64M`, `objects`,
  `x86` and `windows` with `--objects`, `cached` built twice over one object store): **738 agree, 0
  differ, 0 refused** — executables, every object and `link.txt`, standard output and error and
  the status byte for byte, the refusals and check failures with their diagnostics included.
  Largest host-run peak (the driver with its stages, and the reference's `nzcg`): 77 MiB; no
  withdrawal condition.
- **The self-build through `nazm build`**, `cargo xtask contained selfbuild` (heap limit 2 GiB per
  built program), 2026-10-20: **passed** — stage 0 (the seed) built `nazm`, `pl`, `cg`; stages 1–3
  each built 9 programs through the previous stage's `nazm build --backend nazm`; stage 1 vs 2 and
  2 vs 3: 9 executables identical, 0 differ; Core IR: 9 documents identical to the reference's;
  66 corpus programs built by stage 3 run as the seed's builds do; 68 run by the self-built
  interpreter as by the seed's; `nazm-check` byte for byte (16 gates); the heap probe stopped with
  `N0406` at 224.6 MiB resident. Peak resident memory of a whole build (the driver and the stages
  it waited for): largest 1,483.9 MiB, `nazm build` of `toolchain/commands/nazm.nz` (the front
  end in process); `pl` 579–593 MiB, `cg` 285–292 MiB. The first run found the driver reading a
  stage's heap limit from the first `the heap limit of` text in its image — the pipeline carries
  that text as a template — and so taking the stage as unlimited: fixed (`bd_limits`).

## Slice 5 — the LLVM path (2026-10-20)

Group 2 of `build`'s flags on the default backend: `--backend llvm` (now the default), `--opt-level`,
`--emit-ir`, `--debug`, `--release`, `--layout`. The Rust `build` is the oracle, as in slice 4.

- **Where it lives.** In the pipeline stage, not the driver: `nazm-pipeline build --llvm`
  (`toolchain/pipeline/main.nz`) writes each unit as LLVM text (`OUTDIR/NAME.ll`) beside the runtime's
  units (`OUTDIR/rt/NAME.ll`) and the plan; there is no code-generator stage on this backend, so the driver
  starts one stage and `clang`. The emitter reads the unit's instruction text (what `nazm lir --ops`
  prints) rather than LIR structures: `ops.nz` already writes that text and the generator's reader
  already depends on its grammar. Two additions the LLVM text needs and the printed form lacks are
  written only for it (a flag in the needs map, so `nazm lir --ops` is unchanged): the binding each
  variable and slot holds (`vname`/`sname` lines, shown as comments) and the root file's name.
- **Ports.** `llvm.nz` is `op/llvm.rs` (failure bit or unwinding per target, `notail` in call cycles,
  weighted branches, the inlined stack check, `nz.failed_now` read in place); `lldebug.nz` is `debug.rs`;
  `llpost.nz` is what `LlvmText::generate` does after the text (plain thread-local state in a
  one-thread program, `inline_reference_counting`, the counted or small-block heap's renaming) and
  `release.rs`'s `stack_protected`; `ops.nz` gains `op/soa.rs` (`--layout soa`, for both backends). The
  runtime is the units' reach (`runtime_units`, as `pipeline runtime` already gave it), not the Nazm
  backend's every part; `--link` turns the small-block heap off, as the Rust build does.
- **The driver.** `clang -O{n} -Wno-override-module -c` per unit (a section per function on ELF,
  `-flto=thin` for whole-program), the link as `Backend::link` (`-flto=thin -Wl,-mcpu,CPU` at `-O2` and
  above on macOS, `-Wl,-pie` for `--release`, then `strip` and `codesign --force --sign - --options
  runtime`), `dsymutil` for `--debug`, `--emit-ir` beside the output. `--release` conflicts, its
  usage refusals (exit 5) and `--layout`'s are the Rust build's. The LLVM backend's objects are not
  filed in the object store (derived data), and the whole-program link keeps no ThinLTO cache.
- **A `--debug` executable is not reproducible between two processes**, Rust or Nazm: its debug map holds
  the build's working directory (`prog.nazm-PID/`), so the comparison does the units' text and objects
  byte for byte and the executable through `dwarfdump`.
- **Not carried** (still status 6): `--build-report`, `--summary-json`, `--timings`, `--lib`, the
  flags of groups 3 and 4.

### Slice 5 — the evidence

- **The text**, the pipeline's against the Rust `build --emit-ir` over the 78 programs of
  `tests/compat/v1/{run,trap}`, `compiler/conformance/` and `toolchain/codegen/tests/programs/` that
  the Rust build builds: every unit and every runtime unit byte for byte for `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, `x86_64-pc-windows-gnu`, `--heap-limit 64M`, `--debug` and (host) `--release`.
- **The comparison**, `toolchain/commands/tests/build.sh`, one `host-run` at a time, 123 inputs × the
  modes `llvm`, `llvm-heap`, `llvm-objects`, `llvm-x86`, `llvm-windows`, `llvm-x86exe`, `llvm-ir`,
  `llvm-irexe`, `llvm-O1`, `llvm-O2`, `llvm-O3`, `llvm-debug`, `llvm-debugexe`, `llvm-release`:
  **1722 agree, 0 differ, 0 refused**; `soa` and `llvm-soa` (`--layout soa`) 246 agree, 0 differ;
  slice 4's six modes again: see `tasks.md` 5.1.
- **The self-build on the LLVM backend**: see `tasks.md` 5.3.

## Slice 6 — the rest of `build` (2026-10-20)

Group 3 of `build`'s flags, the forms of the command line, and `nazm runtime build` and `verify`, for the kept
targets. The Rust `build` is the oracle, as in slices 4 and 5; `toolchain/commands/tests/build.sh` compares
every mode below, and its `args` part (`tests/args/build.txt`) every command line.

- **Where it lives.** `buildcli.nz` declares the arguments as `cli.rs` does, with clap's help text, and reads them
  through `cli.nz` (extended with possible values, hidden arguments, the `u8` and size value parsers and clap's
  required-set ordering of a usage line). `build.nz` is the driver: `bd_prepare` (a package's directory is its
  `main`, `--locked`, the profiles), `bd_preflight` (`preflight`'s checks in the Rust build's order and words),
  the front end, the stages, `--build-report`, `--summary-json`, `--timings`, `--lib`, `--runtime`, and
  `runtime build|verify`. `archive.nz` is `archive.rs` for Mach-O and COFF members, `buildrt.nz` the runtime
  artifact's `fits`, `buildprov.nz` `provenance.rs` and the SBOM. The pipeline gained `--lib`, `--prebuilt`,
  `llvm-get`, `llvm-put`, `runtime-abi`, `runtime-artifact`, and the plan's `needs`, `shared` and `export` lines.
- **The object stores.** Every LLVM unit is looked up and filed under R3's keys through the `clang -###` probe,
  the runtime's and the standard modules' units in the user cache, the rest in the project's store; a
  whole-program link names the linker's cache `STORE/lto`. The report's `reused` and `stored` agree with the Rust
  build's (`report-cached`). Not carried: the relink cache (`link/`).
- **Not carried, by name (status 6):** the targets the cut-over removes where the Rust build would go on to build
  them, `--backend cranelift`, `--harness` (slice 7), a restriction profile other than `general`, a manifest's
  `heap-limit`, `[features]` and registry dependencies (the front end reads none), a runtime manifest that is not
  JSON. Where the Rust build refuses, so does this one, in its words (`decisions-log.md`).

## Slice 1 — the evidence

- The checkout's `nazm check` of `toolchain/interp/main.nz` and `toolchain/harness/check/main.nz`:
  before, 1 `N0359`, 13 `N0637`, 1 `N0300`, 1 `N0355` (interpreter) and 1 `N0300` (`nazm-check`);
  after, no errors. `toolchain/harness/mutate/main.nz` still checks.
- Both built by the checkout's `nazm build --no-cache --heap-limit 2G`: **passed**.
- Through `cargo xtask host-run` on the host, one at a time: the interpreter over five compatibility
  programs that reach the changed code (`arrays`, `equality`, `r1-patterns-tuples`,
  `collections-encoding`, `records`): standard output and error identical to the seed's `run
  --interpret`, status agreeing as zero or not (host-run reports only that) — **5 agree, 0 differ**;
  peaks 3–12 MiB. `nazm-check` over the tree: its 16 gates' report lines are each in `cargo xtask
  check`'s report — **agree**; peak 82 MiB; both exited 0.
- `cargo xtask contained selfbuild` with the interpreter and `nazm-check` added: see `tasks.md` 1.4.

## Slice 7 — the interactive tier (design, 2026-10-20)

`run` (compiled and `--interpret`), `repl`, `comptime`, and `test` without `NAZM_TEST_DRIVER`. The
oracles are `crates/nazm-cli/src/{compiled_run,interpret,live,test,harness}.rs` and `cli.rs`'s
declarations; every output, word and status is theirs (decision 2) except the differences S7.8
lists. Depends on slice 4 (the driver) and, for `test`'s native leg as the reference runs it, on
slice 5 (the default backend).

### S7.1 Process structure: the interpreter in process, the build as `nazm build` runs it

| command | what runs | in process | children |
| --- | --- | --- | --- |
| `run FILE` | the front end, the build, the program | the front end and the driver (S4.1) | the pipeline and the generator as stages (S4.2), the platform's `clang`, then **the program** |
| `run --interpret FILE` | the front end, then the interpreter | both | none |
| `repl` | per line: the front end over the session text, then the interpreter | both | none |
| `comptime FILE F` | the front end, then the interpreter on one function | both | none |
| `test PATHS` | per case: two legs | discovery only | `nazm run --interpret …`, and `nazm build …` + the executable, as the reference |

**The interpreter is a module of `nazm`, not a stage.** `repl` and `comptime` need it in process
(a REPL line is milliseconds of work; a stage per line would cost a process and a Core IR file per
line), and `run --interpret` then shares the path. It is bounded by the `nazm` process's own
counted heap (2 GiB in the development builds; the released `nazm` is built without a limit, as the
Rust one has none — slice 18 builds it so), so it needs no admission of its own. The `interp`
program stays (the self-build compares it with the seed's `run --interpret`); both are one code
path:

- **`toolchain/interp/entry.nz`** (new): `report`, `refused`, `show`, `memory_line` and the result
  handling move here from `toolchain/interp/main.nz`, made `pub`; `interp/main.nz` keeps only its
  argument parsing and calls them. Added: `pub fn interp_run(doc: Str, root: Str, std: Str, limits:
  Limits, args: Strs, json: Bool, reports: Reports, caps: Caps) -> Int` — load the Core IR document
  (`load`), make the VM (`vm_new`), `run_program`, and write the outcome exactly as `interp/main.nz`
  does today, returning the status (never `exit_with` inside); and `pub fn interp_constant(doc,
  root, std, limits, fid) -> Result[Str, Str]` (S7.4).
- **`toolchain/commands/interpret.nz`** (new): the only module of `toolchain/commands/` that imports
  `toolchain/interp/`. It never imports `compiler/syntax.nz` or `compiler/lex.nz` directly (`Prog`
  and `fail` are exported on both sides: `N0207`); it receives the Core IR document as a `Str` from
  `front.nz`'s functions. `run.nz` and `live.nz` (`repl`, `comptime`) import it.
- **The document is the seam.** The front end prints the document in process
  (`fx_core_document_with_sources`) and the interpreter `load`s that text — the path the self-build
  already compares. Building `Prog` from the facts directly is not done: it would be a second loader
  with no oracle.
- **Capabilities.** `toolchain/commands/nazm.nz`'s `main` gains `OutCap`, `TimeCap`, `NetCap` and
  `RandomCap` (it has `IoCap`, `ProcessCap`, `SpawnCap`, `VouchCap`), passed into the interpreter's
  `Caps`.
- **The runtime's own reports.** The Nazm-built `nazm` is itself a Nazm program: its runtime answers
  `NAZM_RESULT_REPORT`, `NAZM_MEMORY_REPORT` and `NAZM_SCHED_REPORT` when `main` *returns*
  (`crates/nazm-runtime/src/entry.rs`). So every command path of `nazm.nz` ends in
  `exit_with(status)` and `main` never returns (true today; a test holds it:
  `NAZM_MEMORY_REPORT=1 NAZM_SCHED_REPORT=1 NAZM_RESULT_REPORT=1 nazm run --interpret F` prints the
  interpreted program's lines only, as the Rust `nazm` does). The interpreted program's reports are
  the interpreter's `Reports` record, which `run.nz` fills from those variables — what
  `interp/main.nz`'s flags give it.

### S7.2 `run FILE [-- ARGS]`, compiled

1. **The flags** are `cli.rs`'s: `FILE`, `ARGS` after `--`, `--json`, `--interpret`,
   `--max-iterations N` and `--max-depth N` (both `requires = "interpret"`; the reference's value
   parsers and words: depth 0 and > 1,000,000 refused), `--locked` (accepted and ignored, as the
   reference does), `--harness SPEC` (hidden; S7.6). `--profile` and `--features` are refused by
   name, status 6, until slice 6 gives `build` packages; then `run` passes them on.
2. **The build** is `build.nz`'s, called as a function: `build_command`'s body becomes `pub fn
   bd_build(req: BuildRequest) -> Int` (`BuildRequest`: file, output, target, backend, opt level,
   heap limit, cache on/off, harness spec, `run_mode: Bool`); `build_command` parses its flags into
   a request and calls it. `run` calls it with: the host target; **`--backend nazm`**, the code
   generator (the reference's `run` used Cranelift at `OPT_LEVEL 2` for compile speed; Cranelift is
   gone by decision 3 of `rewrite-in-nazm`, and the R9 generator is the fast-to-compile backend that
   replaces it; `--backend llvm` stays `build`'s default); no heap limit (the reference passes
   `heap_limit: None`); the object cache on (the project store, and the user store once 4.4 lands);
   `run_mode = true`, which is the reference's `Choice.run`: no path line, no authority line, no
   build report, no checker warnings, and the native-only refusals (`UNSUPPORTED_CONSTRUCT`,
   `UNBOUNDED_SPECIALISATION`, `GENERIC_INSTANCE_LIMIT`) carry `build/lower.rs`'s run help and end
   with its `N error(s) in compiling this program. Nothing ran; …` line and status 1. A build that
   fails returns its status unchanged; nothing runs.
3. **Where.** The executable is `WORK/STEM` (`STEM.exe` on Windows) in `WORK = os_mkdtemp($TMPDIR
   or /tmp)`, mode 0700, made by this call; failing that, the reference's "cannot make a working
   directory…" words and status 4. On every path out the executable and `WORK` are removed (the
   driver already removes its own work directory). The directory's name is not observable (the
   reference's `nazm-run-PID`, the port's `nazm-` and twelve characters): not a difference.
4. **Arguments, environment, streams.** The program is started with `os_spawn_with(EXE, ARGS, "",
   ENV, 0)`: `ARGS` exactly as given after `--`; `ENV` this process's environment whole
   (`env_vars`), so `NAZM_RESULT_REPORT` and the rest reach the program as they do through the
   reference's inherited environment; the same process group (Ctrl-C reaches both, as with the
   reference). The language has no spawn with inherited streams, so `run` **relays**, in three
   tasks: this process's standard input to the child's (64 KiB reads; at end of input the child's
   input is closed; a write the child refuses — it closed its input — ends that relay quietly); the
   child's standard output to `os_stdout()` and its errors to `os_stderr()` with `os_write`, raw
   bytes, each chunk as it arrives. `run` waits for the child, then for both output relays to reach
   end of file; the input relay is abandoned (it may be blocked on a terminal; `exit_with` ends it).
   Failing to start: "could not run the built program: …", status 4.
5. **The status** is `compiled_run.rs`'s `passed_through`: the child's code; killed by signal `s`
   (`os_wait` answers `256 + s`) → `128 + s`; anything else → 255. `run` ends with `exit_with` of it.

### S7.3 `run --interpret`

`interpret.rs` in order: the root decided (`could not read…`/`is not a directory` → 3, else 1);
`--harness` applied when given (S7.6); the program loaded and checked by the front end in process
(all-`MODULE_NOT_FOUND` → 3; other errors: their diagnostics, `nazm: N error(s)`, 1); the document
printed and handed to `interp_run` with `--max-iterations`/`--max-depth` (defaults 10,000,000 and
512), the arguments after `--`, `--json` for a failure's rendering, and the reports from the
environment. Outcomes: a runtime failure is one diagnostic and status 2; a result is
`end_with_result`'s (`NAZM_RESULT_REPORT` set: printed, 0; an `Int` 0–255: the status, nothing
printed; outside: `N0416`, 2; any other value printed, 0). The `NAZM_MEMORY_REPORT` and
`NAZM_SCHED_REPORT` lines are the interpreted program's, on standard error, after the run. The front
end's facts are released before the VM is made (S4.2's discipline: one large structure at a time).

### S7.4 `comptime FILE FUNCTION [--max-iterations N] [--max-depth N]`

- **The boundary** is the reference's: one function of the **root module**, with **no parameters**
  and declared **`! {}`** (no effect, so it cannot reach the host), evaluated whole by the
  interpreter under the iteration and depth budgets — no time limit, no partial evaluation, nothing
  else folded. Nothing of the file's `main` runs; the file needs none.
- **The steps.** The file opened as `opened::opened` does (status 3 or 1 as there); the front end
  lowers it **without an entry** (the document's `entry` is `null` and every function of the root
  module is in it: the reference's `Entry::Absent`; the slice's first task checks that
  `fx_core_document_with_sources` of a file without `main` keeps every root function, and if it
  prunes from `main`, adds the entry-less mode to `compiler/core.nz` in this slice); the function
  looked up by name among the root module's facts and refused with `N0393` and the reference's three
  messages, label and help (absent; has parameters; not pure); then `interp_constant` with its id:
  the VM's entry set to that function (`Prog.entry`), run with no capabilities, the value rendered
  with the interpreter's `show` — the reference's `Display`.
- **Output and status.** The value and a newline on standard output, 0. Every error — a refusal, a
  check error, a runtime failure (`N0400`, `N0402`, `N0403`) — is rendered as text (never JSON) on
  standard error with status **1, not 2**: the reference's choice, kept.

### S7.5 `repl`

- **What the session keeps between lines: definitions only.** An ordered list of `{name, text}` —
  `fn`, `struct` or `enum` lines (with an optional `pub `), in first-given order; a redefinition
  takes its predecessor's place. **No `let` bindings, no values, no imports** carry from one line to
  the next (each expression line is a fresh program), exactly as `live.rs`.
- **A definition line** is checked as the session text — every definition joined by `\n`, then
  `fn main() -> Int { 0 }` — through the front end's in-memory check (`fx_check_over`, the session
  text as the root's contents, under the name `live.rs` gives it). Defining `main` is refused with
  the reference's sentence. On errors, a diagnostic whose span lies in another definition (by byte
  layout: `owner`) gives "redefining `g` is refused: `f` no longer checks against it" and the
  rendered diagnostics; the session is left unchanged. Otherwise `defined `n`` or `redefined `n``.
- **An expression line** is tried in three wrappers in order, and the first that checks is run in
  the interpreter with the default limits: `fn main(io: IoCap) -> Int ! { io } { EXPR }` → `= N`;
  `… { if EXPR { 1 } else { 0 } }` → `= true`/`= false`; `… { print(EXPR); print("\n"); 0 }` → the
  printed text and nothing else. None checks: the first wrapper's diagnostics. A runtime failure is
  shown rendered.
- **Commands** `:quit`/`:q`, `:reset` ("the session is empty"), `:defs`. No prompt; blank lines
  skipped; everything — errors included — on standard output, flushed after each line; status 0 at
  end of input or `:quit`.
- **Memory.** Each line's front-end facts and VM are let go before the next line is read; the
  session text is the only state. A line that drives `nazm` to its own heap limit ends the session
  with `N0406` (status 2) in the development builds; the released `nazm` has no limit.

### S7.6 `--harness SPEC` on `run` and `build`

`test`'s legs pass `--harness test:NAME`, `fuzz:NAME:CASES` or `bench:NAME:N` to `run --interpret`
and to `build` (`harness.rs`); `build.nz` refuses `--harness` today (`bd_later`). Ported as
`toolchain/commands/harness.nz`: the spec parsed (malformed: "`--harness X` is not `test:NAME`…",
status 5); the file's text transformed exactly as `harness::text` does (its `main` blanked with
offsets kept, the generated `main(nz_harness_io: IoCap, nz_harness_out: OutCap…)` appended, an `Err`
printed and `exit_with(2)`; fuzz inputs from the xorshift seeded 88172645463325252, lengths 0..64);
the result checked through `fx_check_over` with the transformed text in memory (the file on disk is
not rewritten), so diagnostics still name the user's file and its offsets.

### S7.7 `test` whole

- **The driver** is `NAZM_TEST_DRIVER` when set (the seam stays: the comparison scripts point it at
  the reference), else **this executable**, the reference's `current_exe()`. Nazm has no
  `current_exe`; the port resolves `arg(0)`: containing a `/`, `os_realpath` of it (relative to the
  working directory); otherwise the first `PATH` entry holding an executable file of that name, then
  `os_realpath`. Not found: the reference's driver-failure words, status 4. The "not carried"
  refusal (status 6) is removed.
- **Discovery and reporting are already the reference's** (`test.nz`, R5, byte for byte): a
  directory's `.nz` files sorted; `NAME.nz` with `NAME.expected` a golden case; every top-level
  `@test`/`@fuzz` function a case `STEM::fn` (`@bench` skipped); `@test(...)` with arguments expects
  status 2; `--filter` a substring; the legs `DRIVER run --interpret [--harness SPEC] FILE` and
  `DRIVER build [--harness SPEC] FILE -o TMP/caseI` + the executable, each with
  `NAZM_RESULT_REPORT=1`, sequential; the text report, `nazm.test/1`, `nazm.test-summary/1`;
  statuses 0, 1, 3, 4. This slice changes only that the legs now exist in the `nazm` under test. The
  `@cfg` feature/package/profile refusal stays until slice 6.
- **Under the R6 rule** `test` builds and runs programs without a heap limit, so in this repository
  it runs only contained (the runbook already lists it), and so does `run`'s compiled leg (S7.8).

### S7.8 What is compared, and the intended differences

- **Comparison scripts** (`toolchain/commands/tests/interactive.sh`, contained only, as `run.sh`
  is): over `tests/compat/v1/{run,trap,refuse,check}`, `compiler/conformance/`, `bench/programs/`,
  the interpreter's R4 suites and the 66 self-build corpus programs, the Rust `nazm run`, `run
  --interpret`, `comptime` (every parameterless `! {}` function of each file, and one refusal of
  each kind per file) and `test` (each directory) against the Nazm-built one — standard output,
  standard error, status, byte for byte; `repl` over scripted sessions
  (`toolchain/commands/tests/repl/*.in`: definitions, redefinitions refused and accepted, the three
  wrappers, `:defs`, `:reset`, runtime failures) against the reference. `run` is also given
  arguments, 1 MiB of standard input, programs ending 0–255, and one killed by a signal. Agree,
  differ, refused and skipped are counted apart.
- **Intended differences, not counted:** (1) `run`'s native-refusal help says the program
  "compiles, with Nazm's code generator" where the reference says "with Cranelift"; (2) the
  runaway-recursion wording (`N0403` "at N calls" against the fixed depth), accepted in slice 1 —
  compared by code, output and status; (3) a program whose grandchild keeps its standard output open
  keeps `nazm run` waiting until that pipe closes (the relay), where the reference returns at the
  child's exit; (4) a program the code generator refuses and Cranelift compiled — each listed with
  its construct; none is expected after R9, and any found is a generator defect for slice 5's
  backlog, not a reason to route `run` through LLVM.
- **Host smoke** (`host-run`, one at a time): `run --interpret`, `comptime` and `repl` (single
  processes, the interpreter inside the bounded `nazm`) over slice 1's five programs.

### Slice 7 — the evidence

`toolchain/commands/tests/interactive.sh`, the Nazm `nazm` against the Rust one, standard output, standard error and status byte for
byte (2026-10-21). `run --interpret`, `comptime` and the command lines on the macOS host, one `host-run` at a time, heap limit 2G;
`repl`, compiled `run` and `test` (which need standard input or build without a heap limit) in `cargo xtask contained run`, from a
checkout under `$HOME`, in slices of the corpus (`INTERACTIVE_SLICE=K/N`, a contained run is held to an hour).

| part | agree | differ | refused | intended |
| --- | --- | --- | --- | --- |
| command lines (`run` 57, `comptime` 20, `repl` 7) | 84 | 0 | 0 | 0 |
| `run --interpret` over 445 inputs, plain, `--json`, with arguments, and the three report variables | 1341 | 0 | 0 | 0 |
| `comptime` (every function of no parameters, the budgets, and a refusal of each kind per input) | 1448 | 0 | 0 | 0 |
| `repl` (6 scripted sessions) | 6 | 0 | 0 | 0 |
| `run`, compiled (445 inputs; arguments, 1 MiB of input, every status, a signal) | 451 | 0 | 1 | 12 |
| `test` (every directory of inputs in four variants, and a fixture of every kind of case) | 234 | 0 | 8 | 0 |

- **Refused by name.** `run`: `compiler/conformance/refused/expanding-instances.nz` (the pipeline refuses N0358 where the Rust
  build reports it); `test`: the `@cfg` of a feature, package or profile (R5's refusal, until slice 6 gives `test` packages).
- **Intended differences, counted apart.** Eleven native refusals whose help says "with Nazm's code generator" where the reference says
  "with Cranelift" (difference 1), and the failing links of two foreign-function programs (`ffi.nz`, `fail_callback.nz`), whose
  linker lines name the working directory and the code generator's offsets while the sentence `nazm` ends with is the same.
- **Found and fixed.** `comptime` showed a non-scalar result as `value`: the reference's `Display` (`record(7 fields)`,
  `variant 0(1 fields)`, floats) is `ie_show`. The usage line of a missing `FILE` listed the options in declaration order after a
  `requires` (`cli.nz`); `--` hands the next value to the `last(true)` positional (`run -- FILE` is a missing `FILE`);
  `--max-depth`'s message names the GiB of stack `stack_for` computes (saturating at `usize::MAX`); a program ending 6 is not a
  refusal; `--harness` reaches the compiled path.
- **The runtime's own reports** never fire: `NAZM_MEMORY_REPORT`, `NAZM_SCHED_REPORT` and `NAZM_RESULT_REPORT` set, `run --interpret`
  and compiled `run` print the program's lines only, as the reference does.
- **Not done here.** The compiled leg's source-root, package and `--features` runs (slice 6's packages); the host smoke of `repl` (a
  `host-run` has no standard input).

## Slice 13 — the harness remainder in Nazm (design, 2026-10-20)

R7's 6.2 and 6.3: the contained runner, `host-run`, the mutation campaign runner, `bench` and
fuzzing, written in Nazm, so that `xtask` is needed only to build and test the seed. Question 4
(accepted) lets this finish after the cut-over (slice 18), before the release gate (10.3).

### S13.1 The programs

| program | source | runs where | replaces |
| --- | --- | --- | --- |
| `nzx` | `toolchain/harness/nzx/main.nz` | the macOS host, started directly | `cargo xtask contained`, `cargo xtask host-run` |
| `nazm-mutate` (extended) | `toolchain/harness/mutate/` | `session` inside the container; `plan`, `check`, `apply`, `restore`, `recover` anywhere | `xtask mutate`, `mutate-recover` (for non-seed entries) |
| `nazm-fuzz` | `toolchain/harness/fuzz/main.nz`, `shim.c`, `targets/*.nz` | inside the container | `fuzz/run.sh`, the Rust `nazm-fuzz` |
| `nazm-bench` | `toolchain/harness/bench/main.nz` | inside the container | `xtask bench` |

`toolchain/harness/build.sh` builds the four with the checkout's `nazm` (`stage.sh`'s
`stage_checkout_nazm`: the seed before slice 18, the Nazm-built `nazm` after) into `target/nzx/`:
`nzx` with `--heap-limit 256M`, the others with `2G`. Inside the container a workload's first step
builds what it runs the same way, from `/src` into `/work/nzx/`. They share `toolchain/harness/lib/`
(`run.nz`'s bounded child, `scratch.nz`) and a new `lib/procs.nz` (S13.4's process tree).

**The runners are themselves Nazm-built processes on the host** (**question 5**, below): `nzx` is
built with a counted heap of 256 MiB and, as its first act, checks its own image exactly as
`host-run` checks a stage (`nz.heap_malloc` present, one `N0406` limit, at most 256 MiB; refusal
`runner-unbounded`, status 1), finding its image from `arg(0)` as S7.7 does. It holds no work of its
own: it reads an image, starts one child, drains it into bounded buffers and waits.

### S13.2 `nzx host-run` — the port of `xtask/src/host_run.rs`

The runbook's contract (*What may run on the host*), byte for byte with the Rust runner: the
arguments (`--deadline-secs` 1–3600, default 900; `--report FILE`; `--max-output-mib` 1–256, default
4; hidden `--margin-bytes`, only tighter, ≤ 256 MiB); the admission order and the eleven refusal
names (`not-macos`, `host-environment`, `usage`, `no-executable` — a regular executable file of at
most 512 MiB —, `no-counted-heap`, `no-heap-limit`, `ambiguous-heap-limit`, `heap-limit-too-large`,
`withdrawn`, `another-stage-running`, `contained-workload-running`) with `host-run: refused [NAME]:
WHY` and status 1; the state directory (`NAZM_HOST_RUN_DIR`, else `~/.cache/nazm-host-run`); the lock
(`os_try_lock` on `DIR/lock`, exclusive, held for the run); `docker ps -q --filter
label=nazm.owner=nazm-contained-runner` non-empty → refused; the run (`/usr/bin/time -l -o
DIR/time-PID.txt EXE ARGS`, standard input null, a new process group, both streams drained to the cap
in tasks, `os_wait_until` against the deadline, then `os_kill_group(pgid, 9)`); the report line word
for word, the Rust `Debug` spelling `exit Some(N)` included; the withdrawal conditions (peak
unreadable unless the deadline was reached; peak > limit + margin; `error[N0406]` without status 2)
with the marker, `WITHDRAWAL CONDITION` and status 70; statuses 124, 125, 126, otherwise the
program's. Its tests are `xtask/tests/host_run.rs` ported to `toolchain/harness/nzx/tests/
host_run.nz`, run against both runners (the refusals need no stage; the runs use a fixture built
with `nazm build --heap-limit 16M`).

`stage-run.sh` uses `target/nzx/nzx host-run` when it exists and `cargo xtask host-run` otherwise,
until slice 19 removes the second.

### S13.3 `nzx contained [--platform amd64] [--profile P] [--cache bench] WORKLOAD` — the port of `contained.rs`

- **Workloads:** every one the Rust runner has — `probe`, `cache [list|clear]`, `tests`, `selfhost`,
  `bootstrap`, `selfbuild [stages=N]`, `release`, `archive`, `mutate`, `bench`, `fuzz [SECONDS]
  [TARGET…]`, `run COMMAND…` — with the same deadlines, profiles (P1, P4T4, the `p4w*` names;
  defaults `tests`, `selfhost`, `fuzz` P4T4, the rest P1), export policies (allowlists, sizes,
  destinations) and final line (`contained: NAME OUTCOME in Xs — stdout N B, stderr N B,
  oom_killed=…, survivors=N`); plus `mutate-seed` (S13.4). The table is data in
  `nzx/workloads.nz`: name, script, deadline, default profile, cache namespace, export policy.
  `tests`, `selfhost`, `bootstrap`, `mutate-seed` and `selfbuild`'s seed step run `cargo` *inside*
  the container: they build or test the seed, and the runner does not care what its script runs.
  `mutate`, `fuzz` and `bench` get new scripts that build and run S13.1's programs.
- **The `docker run` argv** is `run_argv`'s element for element: `run --rm` (or `run` with an
  export), `--name nazm-WORKLOAD-PID`, the three labels, `--memory` and `--memory-swap` equal (4096
  MiB default), `--cpus`, `--pids-limit 512`, `--storage-opt size=8G`, `--ulimit fsize=536870912`,
  `--tmpfs /scratch:rw,exec,size=512m`, `--init`, `--network none`, `--log-driver none`, the
  read-only bind of the checkout at `/src`, the optional cache volume at `/work/target`, the five
  `--env` (`NAZM_CONTAINED=1`, `NAZM_PROFILE`, `NAZM_CARGO_JOBS`, `NAZM_TEST_THREADS`,
  `CARGO_TARGET_DIR`), `--platform linux/amd64` when asked, the image, `sh -c "cd /src && …"`. A test
  asserts the never-present list (`--privileged`, `--cap-add`, `--device`, `--pid`, `--ipc`,
  `--userns`, `-v`, `--volume`, the Docker socket). `nzx contained --print-argv WORKLOAD` (hidden)
  prints the argv one element per line; the comparison diffs it with the Rust runner's for every
  workload and profile (an `xtask contained --print-argv` of the same shape is added to the seed's
  runner for this, its only change).
- **Volumes and keys:** namespaces `mutate`, `src`, `bench`; the name `nazm-verify-NS-KEY16`; the
  key BLAKE3, length-prefixed, over `nazm.verification-cache`, the cache version `1`, the image id,
  the image's `Config.Env`, BLAKE3 of `Cargo.lock` and of `rust-toolchain.toml`, and the profile —
  the Rust key exactly, so both runners share the volumes during the overlap (a test computes both
  for the same inputs). Labels, two generations per namespace, the 16 GiB emptying guard, and `cache
  list`/`clear` touching only owner-labelled volumes: as Rust.
- **Supervision:** refuse (`Unavailable`) with either containment variable set or `docker info`
  failing; reap stale containers by the exact Rust rule (the owner label, the name `nazm-W-PID` with
  the label's pid, this host — `uname -n` —, `ps -p PID` dead; nothing else, never by name alone);
  refuse a missing image with the `docker build …` hint; start; drain both streams in tasks to the 4
  MiB caps, counting the total; at the deadline `docker kill NAME` (the container, not the client);
  `docker inspect -f {{.State.OOMKilled}}`; the outcome in the Rust order (Timeout; MemoryLimit — OOM
  or 137; Passed or OutputOverflow; Failed(code)); on Passed only, the export.
- **Export:** `docker cp NAME:SRC -` read as a tar stream by `nzx/tar.nz` (ustar: name, size,
  typeflag, 512-byte blocks): the manifest first (≤ 4096 B; lines `name size`; names on the
  allowlist, none twice, sizes within the policy), then each file; a symbolic link, a directory or a
  size that disagrees with its header refused; staged under `$TMPDIR/nazm-export-NAME` and moved to
  the destination only when every file validated. Then `docker rm -f`.
- **Accounting** as Rust: wall-clock seconds in the final line (no timing is claimed from it), the
  cgroup's `memory.peak` only for `tests`, survivors counted by `docker ps -aq --filter name=nazm-`.
  Logs are printed, not stored, except the campaign journal (S13.4).
- **New, stated:** `nzx contained` holds the host-run lock (`~/.cache/nazm-host-run/lock`) for the
  container's life, refusing with `host-stage-running` when it cannot take it — so a host stage and
  a container never overlap by construction (the Rust runner relies on `host-run`'s `docker ps`
  alone), and two `nzx contained` never run at once.

### S13.4 The mutation campaign runner

- **Which entries.** One catalogue, `xtask/mutations/mutations.toml`. An entry whose `file` is under
  `crates/`, `xtask/` or `fuzz/` mutates the seed: it stays the Rust campaign's (`nzx contained
  mutate-seed`: the existing in-container `xtask mutate` session). Every other entry (`compiler/`,
  `library/`, `toolchain/`: 238 at `84ed3a61`) is the Nazm campaign's (`nzx contained mutate`).
- **Killers are mapped, not rewritten.** A killer `{package = "nazm-cli", test = T, name =
  "FILE::fn"}` maps to the Nazm test `toolchain/harness/cli/FILE.nz`, function `fn`, run as `$NAZM
  test --filter FILE::fn toolchain/harness/cli/FILE.nz` (R7 decision 1), when that file and
  function exist and `stays-rust.txt` does not list the test. Other killers have no mapping. An
  entry with no mapped killer is reported `unmapped` (a count, never a verdict) and is decided by the
  full tier alone. The schema is unchanged (`plan.rs` is the seed's and rejects unknown fields); a
  Nazm-only killer field waits for slice 19, when the Rust gate is frozen.
- **The session** (`nazm-mutate session`, inside the container; one container is one session — the
  warm-worker law): the tree copied to `/work/mut` (`.git`, `target/`, `.nazm/` excluded, fresh
  modification times); the toolchain built once from the copy — the released set (`nazm`,
  `nazm-pipeline`, `nzcg`, `nazm-mcp`) and the harness tools — with the checkout's `nazm build
  --heap-limit 2G` into `/work/mut-bin/`; each unique mapped killer run once on the pristine tree
  (the mutation baseline, keyed by the tree's digest, the killer and the parallelism, held for the
  session; a failing one refuses the session); then each selected mutant in turn, bucketed by the
  set of programs its file reaches, then first killer, then catalogue order:
  **apply** (exactly once; the v2 backup record `nazm-mutate apply` already writes);
  **build** — only the programs whose `use` closure holds the file (the `use "…"` lines followed
  transitively from each program's root; a `library/` file is in every closure), rebuilt into
  `/work/mut-bin/`; a failure is `UNUSABLE`;
  **judge** by tiers — Direct: each mapped killer; Focused: each mapped killer's whole file; Full:
  the whole Nazm suite (every `toolchain/harness/cli/*.nz` and the `toolchain/*/tests` suites), and
  only Full may say `SURVIVED`;
  **restore** and read back (digest equal, else `NOT RESTORED` and the session stops);
  **journal**. Pass 1 decides at Tier 1; deferred mutants get the Full baseline once, then pass 2 —
  the runbook's law, unchanged. At the end the tree must digest to what it began as. Before every
  killer run, `.nazm/` caches under the copy are removed, except the object store, which is keyed by
  the generator's digest and the LIR text and so cannot answer for an unmutated toolchain.
- **Deadlines and process trees** (`lib/procs.nz`, the port of `procs.rs`): every build and test in
  its own process group; at a deadline the tree walked by parent id (`ps -A -o pid=,ppid=`), stopped,
  then killed (`kill -KILL -- -PGID`); after an ordinary exit the group swept; a watchdog `sh -c
  'read _; kill -KILL -- -PGID'` in its own group, its standard input held by the session, kills the
  group if the session dies. Build 1800 s, killer 1200, focused 1500, full 2400; the session 3000 s
  with a 2400 s budget, incomplete as exit 3; output capped at 32 MiB a step.
- **Verdicts:** the runbook's eight words and rules (a timeout or runner error is never a verdict
  and never reused).
- **Journal and resume.** Identity `{schema "nazm.mutation-campaign/2", the tree's digest, the
  catalogue's BLAKE3, the harness digest (the `toolchain/harness/` tree), the toolchain digest
  (BLAKE3 of the `nazm` that built the session's programs), strategy, selected names, deadlines, the
  reset rule, parallelism, the cgroup's cpu.max}`; the key the first 16 hex characters of its
  JSON's BLAKE3. Lines as `campaign.rs`'s (`kind` `campaign` and `mutant`, the same fields; `secs`
  recorded, never compared), each fsynced and printed as `nazm-journal KEY JSON`; `nzx` appends them
  to the host's `target/mutation-campaigns-nz/KEY/journal.jsonl` — never the Rust runner's
  directory, so the two campaigns' journals cannot be confused. `--resume` reuses only an identical
  identity and only the five semantic verdicts. Lock `target/.nazm-mutation-lock.nz`
  (`os_try_lock`); a stranded backup refuses the session; `nazm-mutate recover --confirm-no-writers`
  repairs, as `mutate-recover` does.
- **`--verify-killer NAME --killer FILE::fn`** prints the pristine, mutant and restored observations
  and the catalogue's `killers = [...]` line in the Rust shape; `--plan` is `nazm-mutate plan`
  (already byte for byte with Rust's over the whole catalogue).
- **Compared** (contained, once per change to the runner): the Nazm campaign and the Rust campaign
  over the same 20 `compiler/`/`library/` entries whose killers all map (`--only`): the same verdict
  per mutant, tier for tier — the Rust killers against the Rust tests, the mapped ones against the
  ports.

### S13.5 Fuzzing — coverage-guided (SanitizerCoverage and libFuzzer, the platform's) and differential

Fuzzing stays contained only (runbook), so "the platform's `clang`" is the contained image's
(Debian trixie, clang 19). Nothing is bundled. This needs slice 5: SanitizerCoverage is an LLVM pass,
so only programs whose units are LLVM IR can be instrumented.

- **Targets** are small Nazm programs, one per Rust target, each reading the file its one argument
  names: `front` (`toolchain/harness/fuzz/targets/front.nz`: parse and check, `compiler/`); `lower`
  (check, Core IR, MIR and the MIR validator: `compiler/` and `toolchain/pipeline/`); `lir` (on to
  LIR and its validator — the Rust target's LIR interpreter has no Nazm counterpart and is not
  carried); `run` (check, Core IR, the interpreter with a 2 s budget, through S7.1's seam);
  `manifest` (`toolchain/commands/toml.nz`'s manifest reader). Built with `--heap-limit 2G`, so a
  runaway allocation is `N0406`, not an out-of-memory kill.
- **The build.** `nazm build --backend llvm --emit-units DIR` (new, **hidden**, harness-only; an
  intended difference like the Linux triples): every program and runtime unit's LLVM IR text as
  `NN-NAME.ll`, `link.txt` as `--objects` writes it, and `units.txt` naming the program's units.
  `nazm-fuzz build TARGET` compiles the program's units with `clang -O1 -c
  -fsanitize-coverage=inline-8bit-counters,pc-table`, the runtime's without; renames the runtime
  entry object's `main` to `nz_program_main` (the image's `llvm-objcopy --redefine-sym`); and links
  them with `shim.c` and `-fsanitize=fuzzer` (libFuzzer's `main`).
- **The shim** (`toolchain/harness/fuzz/shim.c`, about 150 lines, the harness's, never shipped, as
  `fuzz/cov.c` is today — **question 6**): `LLVMFuzzerTestOneInput(data, size)` writes the input to
  a `memfd`, then **forks**. The child points standard input and output at `/dev/null` and its
  errors at a second `memfd`, registers an `atexit` handler that copies the counters
  (`__start___sancov_cntrs` … `__stop___sancov_cntrs`) into a `MAP_SHARED` region the parent mapped
  before forking, and calls `nz_program_main(2, {TARGET, "/proc/self/fd/N"})`. The parent waits
  under a deadline (2 s for `run`, 10 s otherwise; past it the child is killed and the input written
  to `OUT/hangs/` — a hang, not a crash, as the Rust fuzzer keeps them), copies the shared counters
  into its own, and classifies: status 0 or 1 (and for `run`, status 2 when the failure is the
  interpreted program's) — an ordinary input; `N0406` in the errors — counted as `oom`, the input
  kept under `OUT/oom/`; a signal, status 70, or a status-2 failure whose location is a source file of
  the target program itself (a defect of the toolchain) — `abort()`, so libFuzzer records a crash.
  A fork per input keeps every run a fresh program: no state, heap or exit path of the toolchain
  has to be made re-entrant.
- **The run** (`nazm-fuzz run SECONDS [TARGET…]`, from `nzx contained fuzz`): per target, libFuzzer
  with `-max_total_time=SECONDS -seed=24301 -max_len=65536 -rss_limit_mb=3072 -timeout=30
  -dict=toolchain/harness/fuzz/nazm.dict` (the language's tokens: the Rust mutator's list)
  `-artifact_prefix=OUT/T/crashes/`, over the corpora the Rust run uses (`fuzz/corpus/T`, read-only,
  as seeds; `examples/`, `library/`, `bench/programs/`, `crates/nazm-cli/tests/fuzz-corpus`;
  `examples/` only for `manifest`), then `-merge=1` into `OUT/T/min/`. The report: one line per
  target in `fuzz/src/main.rs`'s `nazm.fuzz/1` shape (`target`, `seconds`, `seed`, `execs`,
  `guards` — the counters' length —, `covered`, `seeded`, `kept`, `hangs`, `crashes`, from
  libFuzzer's final statistics and the shim's counts) and each crash's input; `fuzz-min.tar` and
  `report.txt` exported as the Rust workload does; exit 1 on any crash. The minimised corpora become
  `toolchain/harness/fuzz/corpus/T/` only by a person's commit.
- **Differential** (`nazm-fuzz diff COUNT SEED`): R7's generator (`toolchain/harness/cli/
  fuzz_diff.nz`'s xorshift and draws) moved to `toolchain/harness/lib/gen.nz` and shared; each
  generated program run by `nazm run --interpret`, `nazm build` at `--opt-level 0` and `2`, and
  `--backend nazm` — four legs (the Rust test's Cranelift and LIR-interpreter legs go with the seed)
  — standard output, errors and status compared; a disagreement written as `OUT/diff/N.nz` with the
  four outcomes, exit 1. `nzx contained fuzz` runs it after the coverage targets, over `SECONDS × 2`
  programs.
- **First, a probe** (the fuzz part's first task): in the image, `clang -fsanitize=fuzzer` links a C
  file, and `clang -c -fsanitize-coverage=inline-8bit-counters,pc-table` of a `.ll` emits
  `__sancov_cntrs`. If libFuzzer's archive is missing, `docker/contained.Dockerfile` adds Debian's
  `libclang-rt-19-dev` (the platform's package) — a harness change, so the lifecycle suite
  `gate-plan` names for `docker/` runs.

### S13.6 `bench`

`nazm-bench` ports `xtask/src/bench.rs`: the refusal before anything is read unless contained
(`NAZM_CONTAINED=1` and a numeric `memory.max`); `bench/programs/*.nz` checked against their
`.expected` first; the stages (check, build `-O0`, build `-O2`, `run --interpret`, the native `-O2`
executable) and the self-compile, against the `nazm` the workload names (the Nazm-built one after
slice 18); the references where present (`bench/reference/`) and recorded absent otherwise; min,
median, p90, max and MAD over `--runs N` (5), native sizes, peak resident memory once per program
(`os_child_rusage`); `record.json` in `nazm.bench/2`, field for field; `--save` to `baseline.json`
or `baseline-SLUG.json`; `--check` on the same host only, a regression past both 25 % and 5 ms.
Time is read with the language's monotonic clock at the finest unit it offers; the slice adds
nothing to the language for it. **Verified without timing** (the agent rule; the quiet window
measures): a record's schema validated, the expected-output check, and `--check`'s verdicts over
fixed pairs of records equal to the Rust function's on the same pairs.

### S13.7 What `xtask` keeps

After slice 13, `xtask` is the seed's: `check`'s eight Rust-architecture gates (one resolver, Core IR
boundary, span identity, sema purity, cache soundness, native identity, object cache, runtime
parity — the other 16 are `nazm-check`'s), `workspace-tests`, `notices`, `evidence`, `gate-plan`,
`guide`, `check-mutations`, the in-container `mutate` session and `mutate-recover` for the seed's
entries, and the `lint` alias. `contained` and `host-run` stay in `xtask`, unchanged but for
`--print-argv`, until slice 19, so the runbook's commands keep working during the overlap; slice 19
removes them and slice 20 names `nzx` in the runbook (the drafts of those runbook changes are this
slice's last task).

### S13.8 Order inside the slice

1. `nzx host-run` (the smallest; every later host step uses it) and its ported tests.
2. `nzx contained`: argv, keys, supervision, export, reaping; compared by `--print-argv` and the key;
   then `nzx contained probe` passes (0 mismatches, 0 survivors) and `nzx contained selfbuild`
   passes, which exercises a long workload end to end.
3. The campaign runner (`nazm-mutate session`), compared as S13.4 says.
4. The fuzz probe, the targets, `--emit-units`, the shim, the run; then the differential mode.
5. `bench` (last: it is verified without measuring).
6. The runbook drafts for slices 19 and 20.

## Slice 16 — the fixpoint on macOS (design, 2026-10-20)

Question 2, accepted: a **serial host self-build** — every step one `host-run` (2 GiB, one at a
time, the withdrawal conditions as now), the stage loop outside the runner; aarch64 natively, then
x86_64 under Rosetta. Depends on slice 5 (the default backend), slice 15 (the chain on Linux) and,
for the release configuration, slice 18's build flags.

### S16.1 The stage loop: `toolchain/selfbuild/host-selfbuild.sh ARCH OUT`

A new script beside `selfbuild.sh` (which stays the contained Linux one; the two share only the
program list, moved to `toolchain/selfbuild/programs.txt`). It sources `stage.sh` in host mode and
refuses unless: `uname -s` is Darwin; neither containment variable is set; no container carries the
runner's label; there is no withdrawal marker; `ARCH` is `aarch64` or `x86_64`; and for `x86_64`
Rosetta answers (`arch -x86_64 /usr/bin/true`, else `no-rosetta`). **It runs no Nazm-built program
itself:** every such step is one `host-run` (`stage-run.sh`, its report appended to
`OUT/peaks.txt`), strictly one after another, with a deadline of 1800 s for a build and 900 s
otherwise. A refusal, or status 70, from any step stops the loop: the run has **failed**, never
skipped, and any withdrawal marker is left for a person.

| stage | built by | builds, into `OUT/stageS/` |
| --- | --- | --- |
| 0 | the seed, the Rust `nazm build` (not a Nazm program: run directly) | `nazm`, `pl`, `cg` for the target |
| 1, 2, 3 | `OUT/stage(S−1)/nazm build`, one `host-run` per program | the nine programs of `programs.txt` |

Every build is `NAZM_LIBRARY=$root/library NAZM_PIPELINE=$B/pl NAZM_NZCG=$B/cg $B/nazm build
--no-cache --heap-limit 2G --target T SRC -o OUT/stageS/NAME`, in the **release configuration**
slice 18 builds the released binaries with (the default LLVM backend and `--release`: canaries, PIE,
strip, the ad hoc signature): the fixpoint is of what ships. `SELFBUILD_BACKEND=nazm` runs the same
loop with `--backend nazm` (the Linux self-build's configuration) as a second, optional run. That is
27 builds per architecture, each a driver and one stage at a time (S4.2), the largest 1.5 GB on
Linux.

### S16.2 What is compared, byte for byte

1. **The fixpoint:** stage 1 against 2 and 2 against 3, every executable whole (`cmp`), signature and
   load commands included. Nothing is normalised before comparing: a difference is a failure. On a
   difference the script localises it for the report only — `otool -l` of both, `codesign -d
   -vvv`, `dwarfdump --uuid`, and `cmp` again of copies with the signature removed (`codesign
   --remove-signature`) — and the run still fails.
2. **Core IR against the seed:** the last stage's `fe --coredoc-sources` (one `host-run` each)
   against the seed's `nazm core-ir --with-sources`, nine documents.
3. **Behaviour against the seed:** the 66 corpus programs built by the last stage's `nazm build` and
   by the seed's, **both with `--heap-limit 256M`** (on the host every Nazm-built program needs a
   counted heap to be run); each executable run through `host-run` (deadline 60 s); output, errors
   and status equal. A program that never allocates links no counted heap and `host-run` refuses
   it: it is counted as `not run on the host (no counted heap)`, by name, never as agreeing — the
   contained Linux self-build is its evidence.
4. **The interpreter** (`interp`, one `host-run` each) against the seed's `run --interpret` (the Rust
   `nazm`, run directly) over the same corpus, with slice 1's `N0403` rule; **`nazm-check`** once.
5. **The bound on macOS:** the last stage's `fe` rebuilt with `--heap-limit 256M` and run on
   `toolchain/commands/nazm.nz`: `N0406` and status 2, its peak recorded — the first measurement of
   the counted heap under the macOS allocator, which the R6 caveat asked for.
6. **Memory:** `peaks.txt`, `host-run`'s lines (bytes on macOS, not `selfbuild.sh`'s KiB), summed
   per program and stage in MiB. No time is taken or reported.

### S16.3 Mach-O determinism

What could make two stages differ although their inputs agree, and what holds each still:

- **`LC_UUID`.** ld64 derives it from the output's content; with `ZERO_AR_DATE=1` (both drivers set
  it) no time enters it (N46; `bootstrap.md` records the runnable binaries identical). Kept: the
  link's environment is `PASSED` plus `ZERO_AR_DATE=1`. No `-no_uuid`: a binary without `LC_UUID`
  does not load, so it cannot be what ships.
- **The ad hoc signature.** The linker's on arm64, and `codesign --sign -` under `--release`: a code
  directory of page hashes and the identifier — the file's basename — with no time (an ad hoc
  signature carries no CMS blob and no timestamp). Kept: the same basename in every stage directory
  (`OUT/stageS/NAME`), and the link done in the work directory under that basename (S4.4).
- **Timestamps.** Neither the generator's objects (`macho.nz`: none) nor ld64's executables carry
  one; archives would (`ZERO_AR_DATE`), and none is made.
- **Paths.** The driver's work directory holds the process id. A debug map (`N_OSO`) would carry it;
  the release configuration builds without `--debug`, so the objects carry no DWARF and the
  executable no debug map. A `--debug` fixpoint is not attempted. A ThinLTO cache, if slice 5
  carries `-Wl,-cache_path_lto`, is off under `--no-cache`.
- **Linker and SDK.** `LC_BUILD_VERSION`'s `minos` and `sdk` follow `SDKROOT` and
  `MACOSX_DEPLOYMENT_TARGET` (measured: the SDK moved `minos`), and ld64 may record its own version.
  The script pins both once (`SDKROOT=$(xcrun --show-sdk-path)`; `MACOSX_DEPLOYMENT_TARGET` the value
  slice 18's release path pins, until then `11.0`, the generator's own `minos`) for every step, and
  writes `OUT/environment.txt`: `sw_vers`, `xcrun --show-sdk-version`, `clang --version`, `ld -v`,
  `uname -m`, the Rosetta answer. **The fixpoint claims one machine and one toolchain**; nothing is
  claimed across hosts, as `release-candidate.sh`'s provenance already says.

### S16.4 x86_64 under Rosetta

- **What runs as x86_64.** Stage 0: the (arm64) seed builds `nazm`, `pl`, `cg` with `--target
  x86_64-apple-darwin`. Stages 1–3: those x86_64 executables run under Rosetta — `host-run` (arm64)
  starts `/usr/bin/time`, which executes the x86_64 image, which the kernel hands to Rosetta — and
  each builds the next stage with `--target x86_64-apple-darwin` named explicitly (S16.1's command
  line), so the triple never depends on which architecture a child process runs as.
- **The `xcrun` hazard** (`scripts/clean-install-test.sh`): a translated process that starts the
  `/usr/bin/clang` shim makes it an x86_64 process, and the Command Line Tools' `libxcrun` on Apple
  silicon is arm64 only. The script puts the real compiler first —
  `PATH=$(dirname "$(xcrun --find clang)"):/usr/bin:/bin` — and exports `SDKROOT` before any step,
  and probes once, from an x86_64 process, that `clang -target x86_64-apple-darwin
  -print-target-triple` answers (`arch -x86_64 sh -c …`), else refuses with `rosetta-clang`.
- **Memory under Rosetta.** `/usr/bin/time -l` reports the translated process's resident set, which
  includes Rosetta's own; the withdrawal margin (256 MiB over the limit) is unchanged. A run that
  crosses it is a withdrawal to investigate, never a reason to raise the margin.
- **Order.** aarch64 first; x86_64 only after it has passed; never both at once (the host-run lock
  makes that so). The two fixpoints are separate claims; aarch64 and x86_64 executables are never
  compared with each other.

### S16.5 The runbook amendment (draft; applied when slice 16 lands, not before)

> ### The macOS self-build: serial host steps — R8 slice 16 (decided 2026-10-20)
>
> *Decided by the copyright holder on 2026-10-20 (`openspec/changes/r8-cut-over/`, question 2), as an
> extension of the rule above.*
>
> The self-build on macOS (`toolchain/selfbuild/host-selfbuild.sh aarch64|x86_64 OUT`) runs on the
> host as a **series of single steps**: every program written in Nazm that it builds or runs is
> started by one `host-run`, under the conditions of *What may run on the host*, one after another.
> The script itself is a shell loop: it starts no Nazm-built program except through `host-run`, runs
> nothing in parallel, and stops at the first refusal or withdrawal. A step is a `nazm build` (the
> waiting driver and one checked stage at a time, as accepted for slice 4), a front end, an
> interpreter, `nazm-check`, or a corpus program built with `--heap-limit`. A program that links no
> counted heap is not run on the host; the contained self-build is its evidence.
>
> x86_64 runs under Rosetta, only after aarch64 has passed and never beside it; its peaks are read as
> `host-run` reads every peak, with the same 256 MiB margin.
>
> The withdrawal conditions are unchanged. A withdrawal during the self-build stops it, leaves the
> marker, and puts the macOS self-build back to *not run* until a person has investigated; the Linux
> self-build stays `cargo xtask contained selfbuild`.
>
> **Still contained only:** the Linux self-build, and everything listed above. The macOS self-build is
> not a workload of the contained runner and never runs beside one.

## Open questions — slices 7, 13 and 16 (for the copyright holder)

Direction-level only; every other choice in the three designs above is made on the recommendation of
2026-10-20 (detail-level choices are delegated).

**Questions 5 and 6 were settled by the copyright holder on 2026-10-20: both recommendations accepted.**
Each Nazm runner is built with a counted heap of at most 256 MiB, checks its own image at start and holds
the host-run lock for its whole life (slice 4's acceptance extended one level); the harness-only C shim
for in-process libFuzzer is accepted, never shipped, as `fuzz/cov.c` is today.

5. **The Nazm runners on the host (slice 13).** `nzx host-run` and `nzx contained` are themselves
   programs written in Nazm. While `nzx host-run` waits on a stage, two Nazm-built processes exist;
   on a `nazm build`, three (the runner, the waiting driver, the stage); and `nzx contained` waits on
   the Docker client for up to an hour. No runner can be admitted by another. **Recommended:** accept
   them as slice 4's waiting-driver reading extended one level — each runner built with a counted
   heap of at most 256 MiB, checking its own image at start (refused otherwise), holding the
   host-run lock for its whole run, holding no work but bounded drains — and record it in the
   runbook with S13.7's change. Their own peak is not measured from outside (nothing else on the
   host is there to do it); the counted heap is their bound. **The alternative** keeps `host-run` and
   the contained launcher in Rust (`xtask`) after the cut-over, frozen with the seed: the harness is
   then in Nazm except its two outermost runners.
6. **One C file in the harness (slice 13).** In-process libFuzzer needs `LLVMFuzzerTestOneInput`, a C
   function of a pointer and a length; Nazm exports only effect-free functions of numbers (`pub
   extern "C"`), so a harness-only `shim.c` (about 150 lines, compiled by the platform's `clang`,
   never shipped — as `fuzz/cov.c` is today) forks and runs the target per input. **Recommended:**
   accept it as within `rewrite-in-nazm` decision 1's "harness in Nazm", as the runtime's units are
   LLVM IR text compiled by the platform's `clang`. **The alternative** is a language extension (an
   exported function taking `Bytes`): a constitution change made for a test tool.
