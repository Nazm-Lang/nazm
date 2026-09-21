# Goal execution ledger

Contract: `docs/NAZM_AUTONOMOUS_GOAL.md` v1.0. This file is the resume point. **On
resumption, check it against Git and running processes before trusting it** — it records
what was verified when it was written, not what is true now.

---

## Position

| | |
| --- | --- |
| Branch | `master` (no remote configured; local-only is not a blocker) |
| Goal base commit | `4326100` |
| Current commit | `82645f2` |
| Working tree | clean except this file |
| Platform verified | aarch64-apple-darwin (Darwin arm64), rustc 1.98.1 pinned, Apple clang 21.0.0 |
| Disk | 13 GiB free of 460 GiB (**97 % full**) — `target/` alone is 3.7 GiB. Remove disposable copies promptly |

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

## Verified commands (against `9bf4078`)

| Command | Result |
| --- | --- |
| `cargo test --workspace` | **452 passed, 0 failed, 6 ignored** |
| — of which self-skipping | 10 `docker_boundary` tests exercise nothing without `NAZM_REQUIRE_DOCKER=1` |
| — of which native end-to-end | 126 `nazm build` cases, each at `-O0` and `-O2` against the interpreter |
| — of which self-hosting differentials | 15, including the full bootstrap chain |
| `cargo test -p xtask --test lifecycle -- --ignored` | 6 passed (separate; not in the 443) |
| `cargo lint` (the CI alias) | 0 diagnostics |
| `cargo fmt --all -- --check` | clean |
| `cargo xtask check` | 5/5 |
| `cargo run -p nazm-bench -- selftest` | 19/19 |
| **Clean rebuild from a fresh 2 MB copy** | builds in ~9 s, **`--offline` works**, 443 tests pass, `xtask check` 5/5 |

Mutation runs this session, each isolated against a disposable copy and each verified
byte-identical on restore: `return-does-not-end-the-block-it-is-in` (caught, 5 tests).
The full 50-entry catalogue has **not** been run since `8d67a7d`.

## Active processes and temporary resources

None. Disposable copies live under `/tmp/claude-501/` and are removed after use. No
mutation lock is held (`target/.nazm-mutation-lock.v2` absent).

## Current task

**Final verification.** Milestones A–G are complete. What remains is confirming the
integrated state: the clean offline rebuild on a fresh copy, and the release candidate
with every gate passed.

## Next exact action

1. **The clean rebuild**, on a fresh copy of the tree with an empty `target/`, offline:
   `cargo build --workspace --offline`, `cargo test --workspace --offline`,
   `cargo xtask check`. Remove the copy immediately afterwards — the disk is at 98%.
2. **`sh scripts/release-candidate.sh`**, which runs every gate cheapest-first and then
   assembles `target/release-candidate/` with a manifest of digests. A failing gate
   stops it; that is the point.
3. Record both in `docs/delivery.md` and here.

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
- **`str_join` deferred.** Output assembly uses `str_concat` in a loop, which is O(n²).
  Milestone G is where that gets measured rather than guessed at.

## Definition of done (Section 13)

- [x] Scalar control flow verified, including `return` and transfers inside values — `ac39f7b`
- [x] Bootstrap subset: text, structured data, collections, memory, file I/O, errors, modules — `053ed22`
- [x] Compiler frontend and lowering/emission in Nazm — `82645f2`; external dependencies (`clang`, libc) documented in `docs/bootstrap.md`
- [x] C1 → C2 → C3 bootstrap with conformance and reproducibility — `82645f2`; byte-identical, caveats stated
- [x] Ordinary independent programs compile with the Nazm-written compiler — 5 conformance cases plus ~40 end-to-end programs
- [x] Structured concurrency: transfer/capture rules, cancellation, cleanup — specified in `docs/spec.md`, interpreted, and compiled; 26 tests, all deadline-bounded
- [x] Developer interfaces, grammar/guide, formatting, repair workflow — `7243447`; schemas in `schema/`, the grammar checked against the lexer and the parser, the guide generated from it, 27 tests across `schemas.rs`, `format.rs`, `grammar.rs` and `workflow.rs`
- [x] Performance baselines, profiling/optimisation pass, no unsupported speed claims — `3bc0505`; `bench/baseline.json` and `docs/performance.md`, which states that nothing has been compared to another language
- [x] Clean rebuild of the integrated state on a supported target — fresh copy, offline, 443 tests
- [ ] Final local review/release package — `scripts/release-candidate.sh` and `docs/delivery.md` exist; the run that produces the artefact is the remaining step
