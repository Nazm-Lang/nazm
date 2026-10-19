## Why

Q1 found the repository's supply-chain policy (`deny.toml`) failing and run by nothing, and no
shared way to classify a finding: severity and release impact were argued case by case. Both are
repository process, not language. (Q1's language and toolchain fixes are recorded in the Nazm
authorities — `docs/internal/security-qualification.md`, `docs/user/limitations.md`, `docs/user/spec.md` — and are
not part of this change.)

## What Changes

- A `cargo xtask check` gate, **dependency sources**, that reads `Cargo.lock` (no network) and
  fails when a package comes from anywhere but crates.io, or comes from crates.io without a
  checksum. `deny.toml` passes `cargo deny check`, its one advisory exception recorded with its
  path.
- `docs/internal/quality-model.md`: two independent labels per finding (technical severity, programme
  disposition), the taxonomy, discovery origins, the broken-feature policy and the release-blocking
  rules.
- `docs/user/vulnerability-process.md`: intake, triage, private fix, advisory, patch release,
  disclosure, the supported line.
- No change to Nazm semantics, schemas, the runtime or the standard library.

## Capabilities

### New Capabilities

### Modified Capabilities
- `repository-governance`: adds the dependency-source requirement and the finding-classification
  requirement.

## Impact

`xtask/src/supply.rs`, `xtask/src/main.rs`, `deny.toml`, two new documents. Authority sections:
`docs/internal/security-qualification.md` *Supply chain*; `docs/internal/quality-model.md`.
