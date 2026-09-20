# Goal execution ledger

Contract: `docs/NAZM_AUTONOMOUS_GOAL.md` v1.0. This file is the resume point. **On
resumption, check it against Git and running processes before trusting it** — it records
what was verified, not what is true now.

---

## Position

| | |
| --- | --- |
| Branch | `master` (no remote configured; local-only is not a blocker) |
| Goal base commit | `4326100` |
| Current commit | `23ee970` — B3 (arguments, files, output, exit) |
| Uncommitted | see *Current task* |
| Platform verified | aarch64-apple-darwin (Darwin arm64), rustc 1.98.1 pinned, Apple clang 21.0.0 |
| Disk | 12 GiB free of 460 GiB (**97 % full**) — avoid large artefacts, clean build dirs |

## Capability matrix

Status words: **verified** (executed evidence this session or an unbroken commit),
**implemented** (code exists, evidence older), **partial**, **planned**, **blocked**.

| Capability | Status | Evidence |
| --- | --- | --- |
| Lexer, parser, checker, interpreter (`Int`/`Bool`/`Str`) | verified | 98 language tests, 23 syntax tests |
| Structured diagnostics, codes, JSON output | implemented | `nazm-diag`, `--json` |
| Controlled interpreter stack, iteration/depth budgets | implemented | `on_interpreter_stack`, `N0402`/`N0403`/`N0404` |
| Native: `Int`/`Bool`, checked arithmetic, comparisons | verified | 89 build tests at `-O0`/`-O2` |
| Native: value-position `if`, statement `if`, `while`, `break`, `continue`, `let mut`, assignment | verified | commit `60709be` |
| Native: non-recursive direct calls | verified | `60709be` |
| Native `return` | **verified** | `ac39f7b`; Milestone A complete, conformance table in the roadmap |
| Native `Str` as bytes + 6 built-ins | **verified** | `6d722c1`; `{ptr, len}` by value, slices share storage |
| Native `Ints`/`Strs` sequences | **verified** | `30c7b12`; handle to a `{len, cap, data}` header, never freed |
| Native arguments, files, output, exit | **verified** | `23ee970`; ambient authority, labelled as such |
| Discarded expression statements | **verified** | `30c7b12` |
| Native recursion | blocked by design | pending a measured native stack contract. **Not needed**: `docs/bootstrap-design.md` B5 chooses iterative compiler algorithms |
| Modules / multi-file resolution | planned — Milestone B4 | nothing exists |
| A tokeniser written in Nazm | **planned — next** | B's exit criterion, and Milestone C's first component |
| Compiler written in Nazm | planned — Milestone C | nothing exists |
| C1→C2→C3 bootstrap | planned — Milestone D | nothing exists |
| Structured concurrency | planned — Milestone E | nothing exists |
| Grammar artefact, formatter, fix application, repair workflow | planned — Milestone F | diagnostics exist; the rest does not |
| Performance baselines | planned — Milestone G | `docs/performance.md` is claims, not measurements |
| Mutation runner, lifecycle tests, repository gates | verified | 49-entry catalogue, 6 lifecycle tests, `xtask check` 5/5 |

## Verified commands (against `23ee970`)

| Command | Result |
| --- | --- |
| `git log/status/remote` | base `4326100`; six commits added; no remote; working tree clean |
| `rustc --version` / `clang --version` | 1.98.1 (pinned) / Apple clang 21.0.0 |
| `ps` for cargo/rustc/xtask/nazm | none running |
| `ls target/.nazm-*` | no mutation lock; tree is not owned |
| `cargo test --workspace` | **428 passed, 0 failed, 6 ignored** — 10 of the 428 are `docker_boundary` tests that self-skip; 120 are native build cases run at `-O0` and `-O2` against the interpreter |
| `cargo test -p xtask --test lifecycle -- --ignored` | 6 passed (not part of the 383) |
| `cargo lint`, `cargo fmt --check`, `cargo xtask check`, `nazm-bench selftest` | clean / clean / 5-of-5 / 19-of-19 |

## Active processes and temporary resources

None. Disposable mutation copies live under `/tmp/claude-501/` and are removed after use.

## Current task

**Milestone B — the bootstrap subset.** B1 (text), B2 (sequences) and B3 (I/O) are
implemented and verified; B5's strategy is decided (iterative algorithms, no native
recursion needed). What remains in B is **B4 (modules)** and B's exit criterion: *small
Nazm programs that tokenise a source file, build and traverse syntax data, and report
located errors, executing natively*.

## Next exact action

Write `compiler/lexer.nz` — a Nazm tokeniser for Nazm source, using the parallel-array
design in `docs/bootstrap-design.md` and iterative loops only. Run it natively over a
real `.nz` file and check its token stream against `nazm-syntax`'s lexer. It is a
**component of the eventual compiler**, not a demonstration. Then B4 (modules), so the
compiler can be split across files.

## Unresolved decisions

- **Recursion: settled.** `docs/bootstrap-design.md` B5 chooses iterative algorithms with
  explicit `Ints` work stacks, so self-hosting is not blocked on native recursion. Native
  recursion stays refused pending measured stack availability and frame size.
- **Authority: deliberately open.** I/O is ambient and labelled so. A capability
  parameter is the growth path; `main(io: IO)` is not chosen.
- **`str_join` deferred.** Output assembly uses `str_concat` in a loop, which is O(n²).
  If profiling in Milestone G shows it matters, add `str_join` then — that is what the
  measurement milestone is for.

## Definition of done (Section 13)

- [x] Scalar control flow verified, including `return` and transfers inside values — `ac39f7b`
- [ ] Bootstrap subset: text ✅, structured data ✅, collections ✅, memory ✅, file I/O ✅, errors ✅, **modules ⬜**
- [ ] Compiler frontend + lowering/emission written in Nazm
- [ ] C1 → C2 → C3 bootstrap with conformance and reproducibility
- [ ] Ordinary independent programs compile with the Nazm-written compiler
- [ ] Structured concurrency: transfer/capture rules, cancellation, cleanup
- [ ] Developer interfaces, grammar/guide, formatting, repair workflow
- [ ] Performance baselines, profiling/optimisation pass, no unsupported speed claims
- [ ] Clean rebuild of the integrated final state on one supported target
- [ ] Final local review package with source state, verification, skips, limitations, repro steps
