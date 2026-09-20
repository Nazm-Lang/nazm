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
| Current commit | `940875d` |
| Working tree | clean except this file |
| Platform verified | aarch64-apple-darwin (Darwin arm64), rustc 1.98.1 pinned, Apple clang 21.0.0 |
| Disk | 13 GiB free of 460 GiB (**97 % full**) — `target/` alone is 3.7 GiB. Remove disposable copies promptly |

## What is done, and what is not

| Milestone | State |
| --- | --- |
| **A** — scalar control flow, incl. `return` | **complete** (`ac39f7b`) |
| **B** — the bootstrap subset | **complete** (`053ed22`): B1 text, B2 sequences, B3 I/O, B4 modules, B5 strategy |
| **C** — compiler in Nazm | **started**: tokeniser done, **parser done** (whole grammar). No checker, lowering or emission |
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
| Parser written in Nazm | **verified** | `compiler/parser.nz`; the **whole grammar**. Same tree as the reference over 50 hand-written sources **and over its own source and the tokeniser's** — 2000+ nodes, node- and span-identical. Only a bare block used as an expression is refused |
| Checker / lowering / emission in Nazm | planned | nothing exists |
| Native recursion | blocked by design | pending measured stack availability and frame size. **Not needed**: the compiler is iterative |
| Concurrency, formatter, grammar artefact, fix application, benchmarks | planned | nothing exists |

## Verified commands (against `9bf4078`)

| Command | Result |
| --- | --- |
| `cargo test --workspace` | **444 passed, 0 failed, 6 ignored** |
| — of which self-skipping | 10 `docker_boundary` tests exercise nothing without `NAZM_REQUIRE_DOCKER=1` |
| — of which native end-to-end | 126 `nazm build` cases, each at `-O0` and `-O2` against the interpreter |
| — of which self-hosting differentials | 7 |
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

**Milestone C — a checker in Nazm.** The front end's first two passes are done and
verified against the reference; what remains for a compiler is name resolution and type
checking, then lowering and LLVM emission, then the driver.

## Next exact action

Write `compiler/check.nz` over the node arena `compiler/parser.nz` produces.

The arena is **post-order with arity**, which is a stack machine's natural input: a single
left-to-right pass with an `Ints` operand stack visits every node exactly once, children
before parents, with no traversal recursion at all. Type checking is therefore a fold —
push each leaf's type, pop `arity` types at each internal node, check them, push the
result — which is why the dump format was chosen for the comparison in the first place.

Needed:

1. A symbol table as parallel arrays: `sym_name: Strs`, `sym_kind: Ints`,
   `sym_type: Ints`, `sym_scope: Ints`. Lookup walks outward by scope index; a hash map
   is not needed and is not proposed.
2. A signature pass over the `fn` nodes before any body, so a forward reference resolves
   — the same two-pass shape `nazm-lir::lower` uses.
3. Diagnostics as `diag_code: Strs`, `diag_start/diag_end: Ints`, `diag_message: Strs`,
   reported with the **same codes** the reference uses so the two can be compared.
4. A differential test in `crates/nazm-cli/tests/selfhost.rs` comparing the set of
   `(code, span)` pairs against `nazm_core::check` over the same sources — including the
   invalid ones, because a checker that accepts everything agrees with nothing.

After that: lowering and LLVM emission in Nazm (Milestone C), then the C1→C2→C3 chain
(Milestone D).

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
