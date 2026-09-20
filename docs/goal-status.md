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
| Current commit | `9bf4078` |
| Working tree | clean except this file |
| Platform verified | aarch64-apple-darwin (Darwin arm64), rustc 1.98.1 pinned, Apple clang 21.0.0 |
| Disk | 13 GiB free of 460 GiB (**97 % full**) — `target/` alone is 3.7 GiB. Remove disposable copies promptly |

## What is done, and what is not

| Milestone | State |
| --- | --- |
| **A** — scalar control flow, incl. `return` | **complete** (`ac39f7b`) |
| **B** — the bootstrap subset | **complete** (`053ed22`): B1 text, B2 sequences, B3 I/O, B4 modules, B5 strategy |
| **C** — compiler in Nazm | **started**: tokeniser done, parser done except `if` |
| **D** — C1→C2→C3 bootstrap | not started |
| **E** — structured concurrency | not started |
| **F** — AI-native developer experience | not started (structured diagnostics and `--json` predate this goal) |
| **G** — performance and release integration | not started |

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
| Parser written in Nazm | verified **for its scope** | `compiler/parser.nz`; same tree as the reference over 34 sources. **`if` and bare-block expressions refused by name** |
| Checker / lowering / emission in Nazm | planned | nothing exists |
| Native recursion | blocked by design | pending measured stack availability and frame size. **Not needed**: the compiler is iterative |
| Concurrency, formatter, grammar artefact, fix application, benchmarks | planned | nothing exists |

## Verified commands (against `9bf4078`)

| Command | Result |
| --- | --- |
| `cargo test --workspace` | **443 passed, 0 failed, 6 ignored** |
| — of which self-skipping | 10 `docker_boundary` tests exercise nothing without `NAZM_REQUIRE_DOCKER=1` |
| — of which native end-to-end | 126 `nazm build` cases, each at `-O0` and `-O2` against the interpreter |
| — of which self-hosting differentials | 6 |
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

**Milestone C — `if` in `compiler/parser.nz`.** It is the last piece of the grammar.

## Next exact action, with the design

`if` is the only construct that **suspends an expression part-way** to parse a block, so
the shunting-yard stacks must outlive one `parse_expr` call. The change:

1. Hoist `ops`, `opat`, `cname`, `cargs`, `vs`, `ve` out of `parse_expr` into
   `parse_program`, and pass them in. They are sequences, so they are references and
   persist across calls for free.
2. Give `parse_expr` a `resuming: Bool` — when true it starts in the postfix loop rather
   than the prefix loop — and a one-element `Ints` status cell, because the language has
   no tuples: `0` = the expression finished, `1` = it suspended at an `if`'s `{`.
3. In `parse_expr`'s primary position, `if` pushes sentinel `93` with the `if`'s start
   and continues, so the condition parses as an ordinary expression. When `{` is reached
   with `93` innermost, reduce to the sentinel and return status `1`.
4. Add frame kinds `frame_if_then` and `frame_if_else`. On status `1` the frame machine
   pushes `frame_if_then`. Closing it emits the block node, then either sees `else`
   (expect `{`, push `frame_if_else`) or emits `if` with arity 2, pops sentinel `93`,
   pushes the `if`'s span onto `vs`/`ve`, and calls `parse_expr` with `resuming = true`.
   Closing `frame_if_else` emits the block and then `if` with arity 3.
5. The statement context must also become a stack — `pk`/`pstart`/`ptext` — because the
   frame machine calls `parse_expr` from five places (`let` value, assignment value,
   `return` value, `while` condition, statement-or-tail) and each needs a different
   continuation after the expression finally completes.

Reference spans, already measured: `if` spans from the `if` keyword to the last block's
`}`; a block from `{` to `}`; the arity is 2 without `else` and 3 with.

After that, in order: name resolution and type checking in Nazm (Milestone C), then D.

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
- [ ] Compiler frontend and lowering/emission in Nazm — **lexer and parser done; parser lacks `if`; no checker, lowering or emission**
- [ ] C1 → C2 → C3 bootstrap with conformance and reproducibility
- [ ] Ordinary independent programs compile with the Nazm-written compiler
- [ ] Structured concurrency: transfer/capture rules, cancellation, cleanup
- [ ] Developer interfaces, grammar/guide, formatting, repair workflow
- [ ] Performance baselines, profiling/optimisation pass, no unsupported speed claims
- [x] Clean rebuild of the integrated state on a supported target — fresh copy, offline, 443 tests
- [ ] Final local review/release package
