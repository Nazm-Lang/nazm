## Context

`cargo xtask check` is the repository's set of pure tree reads that protect architectural
invariants (`xtask/src/main.rs`, `run_checks`); each gate names the invariant it protects. M1 adds
governance files whose consistency is the same kind of invariant.

## Goals / Non-Goals

**Goals:** catch structural drift in the agent-instruction and OpenSpec files, cheaply, offline.

**Non-Goals:** parsing instructions as natural language; validating OpenSpec semantics (that is
the `openspec validate` CLI's job, run when changes are made); requiring Node or network for
`cargo xtask check`.

## Decisions

- **A gate in xtask, not a cargo test.** The checked files are repository text, like the other
  documentation gates, and xtask must not depend on the compiler crates.
- **Structural rules only.** Existence, a link resolving, a field heading present, a forbidden
  phrase absent. Each rule names the sentence of `docs/internal/agent-governance.md` it enforces.
- **Relaxation detection is a list, not a parser:** a nested file may not contain an allowance
  for the root's forbidden actions (push, tag, publish, prune, `NAZM_ALLOW_UNCONTAINED`, …) phrased
  as permission. The list is small and explicit.
- **OpenSpec capability rule:** `openspec/specs/<name>/` must be one of the repository-process
  capabilities the gate lists; a language capability there would make OpenSpec a second spec.

## Risks / Trade-offs

A structural check can be satisfied by a file that is wrong in substance; review still owns
substance. The gate is cheap enough to run on every check, and it turns the drift it can see into
a failure instead of a surprise.
