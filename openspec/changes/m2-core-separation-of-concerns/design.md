## Context

The survey that motivates this change is `target/gates/m2-survey/` (metrics, churn, and three
concern-by-concern reports). The reviewer accepted a reduced scope (M2-Core) on 2026-10-08 after Q1
closed at `660c962`. Execution, evidence and handoff state are in
`.agent/plans/m2-core-separation-of-concerns.md`.

## Goals / Non-Goals

**Goals:** Gate 2 lands in modules with one responsibility each; duplicated helpers exist once; a
gate keeps the new boundaries from eroding.

**Non-Goals:** behaviour change; public renames; performance work; crate reorganisation; splitting
test files for size (deduplication only); `parser.rs` and `mir/lower.rs` (split only if Gate 2
directly needs them); a full mutation campaign (moved mutants run against their verified tier-1
killers; the full catalogue belongs to the final v1 gate).

## Decisions

1. **A commit moves code or changes it, never both.** A move commit relocates code and adjusts
   `mod`/`use`/`pub use`, visibility (`pub(super)`/`pub(crate)` on items now reached from a sibling)
   and the mutation catalogue's `file =`; it edits no test. Anything else is its own commit before
   or after.
2. **The entry module is the façade.** `check/mod.rs` re-exports today's public names, so no caller
   outside the module changes. Child modules add their own `impl Checker` blocks; a child sees its
   parent's private fields, so no field visibility widens.
3. **Path-reading gates first.** Before a move, a separate commit makes every gate that reads the
   file by path read "the file or the directory of that name" (`xtask` interpreter boundary, single
   resolver, checker codes; `tests/codes.rs`). Then the move cannot narrow what they read.
4. **Module boundaries are responsibility-based, not "no top-level fn".** A register in
   `xtask/src/gates/boundaries` lists each split module and the items its `mod.rs` may define
   (façade entry points, shared context, orchestration). Every file in the module opens with a `//!`
   responsibility line. A new top-level item in `mod.rs` fails until it is placed or registered.
5. **Mutants move with their code.** Each moved entry's `file =` is repointed in the move commit; a
   `find` whose indentation changes is re-indented in the same commit (the text is unchanged). The
   `mutations apply` gate proves every entry still applies uniquely. Each split's moved entries are
   then run contained with their verified tier-1 killers.
6. **The LIR oracle stays independent.** `nazm-lir`'s `op/interp.rs` re-implements number rules on
   raw `u64` deliberately: it is the differential cross-check of `nazm_sema::numeric`, and sharing
   the rules would make a rule defect invisible to both. Shared are only the type inventory, the edge
   corpus and the test vectors. `docs/maintainability.md` §6 says so.

## Risks / Trade-offs

- **A mutant's `find` loses uniqueness or meaning after a move** → the `mutations apply` gate and a
  contained run of the moved entries with killers verified.
- **A gate silently narrows** → decision 3, plus the new requirement's scenario.
- **Recursion depth:** moving functions between modules does not change inlining or frame size; the
  `#[inline(never)]` attributes move with their functions.
- **Review cost of large moves** → `git diff -M --stat` and a per-commit check that the moved lines
  are the same lines (a script compares sorted non-blank lines before and after, recorded in
  `target/gates/m2/`).
