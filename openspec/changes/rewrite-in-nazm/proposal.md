## Why

The copyright holder decided on 2026-10-11 that the whole Nazm toolchain — the product, the harness
and the tests — is rewritten in Nazm **before anything is published**. A language whose own tools
are written in it is its own largest test, and a toolchain that links no third-party code owes no
third-party notices (`docs/licensing.md`, item 7). Today the product is ~133k lines of Rust in 19
crates, the harness ~25k, the tests ~100k; the compiler written in Nazm (`compiler/*.nz`, ~16.8k
lines) compiles a declared subset of the language (`docs/bootstrap.md`, `limitations.md`).

## What Changes

A programme of phases, each its own OpenSpec change, gated as the v1 gates were:

- **R0** — this change and `r1-language-for-the-rewrite`: the decisions, and the language and
  library design the rewrite needs, for approval before code.
- **R1** — the language and library prerequisites, built in the Rust reference first.
- **R2** — the Nazm compiler carries the whole language, with Rust's diagnostics, JSON included.
- **R3** — the native pipeline in Nazm: ownership and cleanup, LIR, the runtime generator, every
  target, the object cache, debug information.
- **R4** — the interpreter in Nazm.
- **R5** — every command of `nazm` and the MCP server in Nazm.
- **R6** — host qualification: Nazm-built tools run on a host under a bounded heap, and the
  containment rule is revised by a recorded decision.
- **R7** — the harness and the tests in Nazm.
- **R9** — Nazm's own code generator, written in Nazm, replacing Cranelift.
- **R8** — cut-over: the released `nazm` is built by Nazm, from Nazm; the Rust implementation is
  frozen as the bootstrap seed.
- Then the release gate, then publication. Nothing is published before it.

## Capabilities

### New Capabilities
- `toolchain-in-nazm`: the observable requirements of the programme as a whole. Archived with
  `--skip-specs`; the normative text lands in the authorities as each phase closes.

### Modified Capabilities
None.

## Impact

Every crate under `crates/`, `xtask/`, `fuzz/`, `compiler/`, `library/`, the tests, the runbook's
containment rule (R6), the release path (R8), `docs/bootstrap.md`, `docs/architecture.md`,
`docs/capability-matrix.md`, `docs/support.md` (the Cranelift column), `docs/licensing.md`. Versions
move per phase, recorded there. The 1.0 language promises (`docs/stability.md`) are unchanged:
additions arrive as 1.x-compatible extensions.
