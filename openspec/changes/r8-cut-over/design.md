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
| 19 | The seed frozen: a gate on `crates/` and `xtask/`'s seed part; `docs/bootstrap.md`'s chain | S | Sonnet | 10.1 closed |
| 20 | The authorities (10.2): architecture, bootstrap, capability matrix, support, licensing, runbook | M | Sonnet | 10.2 closed |

Slices 8–12 depend only on 4 (and 7 for `trace`/`profile`); they can run in any order between 7
and 14. Slice 13 may run beside any of them (it touches only `toolchain/harness/` and `xtask/`).

## Open Questions

Direction-level, for the copyright holder:

1. **The deferred targets.** The Rust `nazm` — the 0.10.0 release included — builds for Linux,
   WebAssembly, WASI, Android, iOS and five boards, and `contract` compiles to EVM and WebAssembly.
   The 2026-10-15 narrowing defers all of them in the Nazm pipeline until after 1.0. A cut-over
   before they are ported takes them away. **Recommended:** cut over without them and record each as
   removed for 1.0 in `docs/support.md` and `docs/stability.md` (their port is after-1.0 work already
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
