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
| Current commit | `053ed22` — B4 (modules). **Milestone B complete** |
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
| Modules: `use`, relative resolution, diamonds, cycles | **verified** | `053ed22`; one merged buffer, per-file diagnostics; **no visibility** |
| A tokeniser written in Nazm | **verified** | `84010df`; `compiler/lexer.nz`, 1891 tokens on its own source, agrees with `nazm_syntax::lex` token- and span-wise |
| A parser written in Nazm | **planned — next** | Milestone C |
| Checker / lowering / emission in Nazm | planned — Milestone C | nothing exists |
| C1→C2→C3 bootstrap | planned — Milestone D | nothing exists |
| Compiler written in Nazm | planned — Milestone C | nothing exists |
| C1→C2→C3 bootstrap | planned — Milestone D | nothing exists |
| Structured concurrency | planned — Milestone E | nothing exists |
| Grammar artefact, formatter, fix application, repair workflow | planned — Milestone F | diagnostics exist; the rest does not |
| Performance baselines | planned — Milestone G | `docs/performance.md` is claims, not measurements |
| Mutation runner, lifecycle tests, repository gates | verified | 49-entry catalogue, 6 lifecycle tests, `xtask check` 5/5 |

## Verified commands (against `053ed22`)

| Command | Result |
| --- | --- |
| `git log/status/remote` | base `4326100`; six commits added; no remote; working tree clean |
| `rustc --version` / `clang --version` | 1.98.1 (pinned) / Apple clang 21.0.0 |
| `ps` for cargo/rustc/xtask/nazm | none running |
| `ls target/.nazm-*` | no mutation lock; tree is not owned |
| `cargo test --workspace` | **440 passed, 0 failed, 6 ignored** — 10 self-skip (`docker_boundary`); 126 are native build cases at `-O0`/`-O2` against the interpreter; 3 are the self-hosting differential tests |
| `cargo test -p xtask --test lifecycle -- --ignored` | 6 passed (not part of the 383) |
| `cargo lint`, `cargo fmt --check`, `cargo xtask check`, `nazm-bench selftest` | clean / clean / 5-of-5 / 19-of-19 |

## Active processes and temporary resources

None. Disposable mutation copies live under `/tmp/claude-501/` and are removed after use.

## Current task

**Milestone C — the compiler, in Nazm.** Milestone B is complete: B1 text, B2 sequences,
B3 I/O, B4 modules, B5 strategy (iterative, demonstrated by `compiler/lexer.nz`). B's
exit criterion is met for tokenising, constructing output and located errors; *building
and traversing structured syntax data* is the parser, which is C's first step.

## Next exact action

Write `compiler/parser.nz` — a Nazm parser over `compiler/lexer.nz`'s token arrays,
building the node arena from `docs/bootstrap-design.md` (`node_kind`, `node_a/b/c`,
`node_span_*`, `node_text`) with an **explicit work stack, never recursion**. Then extend
`crates/nazm-cli/tests/selfhost.rs` to compare its node tree against
`nazm_syntax::parse` on the same sources, the way the lexer is compared now.

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
- [x] Bootstrap subset: text, structured data, collections, memory, file I/O, errors, modules — `053ed22`
- [ ] Compiler frontend + lowering/emission written in Nazm
- [ ] C1 → C2 → C3 bootstrap with conformance and reproducibility
- [ ] Ordinary independent programs compile with the Nazm-written compiler
- [ ] Structured concurrency: transfer/capture rules, cancellation, cleanup
- [ ] Developer interfaces, grammar/guide, formatting, repair workflow
- [ ] Performance baselines, profiling/optimisation pass, no unsupported speed claims
- [ ] Clean rebuild of the integrated final state on one supported target
- [ ] Final local review package with source state, verification, skips, limitations, repro steps
