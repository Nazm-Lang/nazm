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
| Current commit | `60709be` — M1d+M1e checkpoint |
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
| **Native `return`** | **planned — Milestone A / M1f** | refused by name today |
| Native recursion | blocked by design | pending a measured native stack contract (roadmap) |
| Native `Str`, collections, records, modules, I/O | planned — Milestone B | nothing exists |
| Compiler written in Nazm | planned — Milestone C | nothing exists |
| C1→C2→C3 bootstrap | planned — Milestone D | nothing exists |
| Structured concurrency | planned — Milestone E | nothing exists |
| Grammar artefact, formatter, fix application, repair workflow | planned — Milestone F | diagnostics exist; the rest does not |
| Performance baselines | planned — Milestone G | `docs/performance.md` is claims, not measurements |
| Mutation runner, lifecycle tests, repository gates | verified | 49-entry catalogue, 6 lifecycle tests, `xtask check` 5/5 |

## Verified commands (this session, against `60709be`)

| Command | Result |
| --- | --- |
| `git log/status/remote` | base `4326100`, M1d+M1e committed as `60709be`, no remote |
| `rustc --version` / `clang --version` | 1.98.1 (pinned) / Apple clang 21.0.0 |
| `ps` for cargo/rustc/xtask/nazm | none running |
| `ls target/.nazm-*` | no mutation lock; tree is not owned |
| `cargo test --workspace` | 383 passed, 0 failed, 6 ignored — **10 of the 383 are `docker_boundary` tests that self-skip** |
| `cargo test -p xtask --test lifecycle -- --ignored` | 6 passed (not part of the 383) |
| `cargo lint`, `cargo fmt --check`, `cargo xtask check`, `nazm-bench selftest` | clean / clean / 5-of-5 / 19-of-19 |

## Active processes and temporary resources

None. Disposable mutation copies live under `/tmp/claude-501/` and are removed after use.

## Current task

**Milestone A / M1f — native `return`.** Scope table in `docs/roadmap.md`.

## Next exact action

Implement `Stmt::Return` in `crates/nazm-lir/src/ir.rs`, `lower.rs` and `emit.rs`, using
direct typed `ret` per return site (no shared exit block — nothing needs cleanup yet).

## Unresolved decisions

- None open. Recursion's strategy is deferred to Milestone B5 by the contract.

## Definition of done (Section 13)

- [ ] Scalar control flow verified, including `return` and transfers inside values
- [ ] Bootstrap subset: text, structured data, collections, memory, file I/O, errors, modules
- [ ] Compiler frontend + lowering/emission written in Nazm
- [ ] C1 → C2 → C3 bootstrap with conformance and reproducibility
- [ ] Ordinary independent programs compile with the Nazm-written compiler
- [ ] Structured concurrency: transfer/capture rules, cancellation, cleanup
- [ ] Developer interfaces, grammar/guide, formatting, repair workflow
- [ ] Performance baselines, profiling/optimisation pass, no unsupported speed claims
- [ ] Clean rebuild of the integrated final state on one supported target
- [ ] Final local review package with source state, verification, skips, limitations, repro steps
