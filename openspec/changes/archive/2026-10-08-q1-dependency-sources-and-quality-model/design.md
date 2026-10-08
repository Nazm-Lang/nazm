## Context

`deny.toml` stated a sound source policy — crates.io only, no unknown git — but `cargo deny` needs
a network for its advisory database, so nothing in `cargo xtask check` ran it, and it had silently
started failing (workspace path dependencies read as wildcards). Q1 needed a rule a gate can hold
offline, and one vocabulary for its 95 findings.

## Goals / Non-Goals

**Goals:** an offline, deterministic check of where every locked crate comes from; a
classification every gate uses. **Non-goals:** advisory lookup in the gate (it needs a network and
changes daily — it stays `cargo deny check`, run at gates and recorded); a vetting process
(`cargo-vet` is not installed).

## Decisions

- **Read `Cargo.lock`, not `cargo metadata`.** The lockfile is what a `--locked` build uses and
  needs no resolution; `toml` is already an xtask dependency.
- **A workspace member has no `source`.** Cargo records none for path packages, so "no source"
  means "ours"; anything with a source must be crates.io and carry a checksum.
- **Severity and disposition are separate columns.** A HIGH defect in an experimental surface can
  be POST-V1; a LOW one that falsifies a stable claim cannot. One score would hide which.

## Risks / Trade-offs

A future git dependency fails the gate on purpose; adding one is a decision that edits `deny.toml`
and this gate together.
