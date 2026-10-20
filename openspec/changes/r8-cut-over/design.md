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
