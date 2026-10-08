## ADDED Requirements

### Requirement: Every locked dependency comes from crates.io or the workspace
`cargo xtask check` SHALL fail when `Cargo.lock` records a package whose source is anything but
the crates.io registry, or a crates.io package without a checksum, and SHALL decide this from the
lockfile alone, with no network.

#### Scenario: a git dependency is locked
- **WHEN** `Cargo.lock` records a package with a `git+` source
- **THEN** the dependency sources gate fails, naming the package and its source

#### Scenario: a registry package has no checksum
- **WHEN** `Cargo.lock` records a crates.io package without a `checksum`
- **THEN** the gate fails, naming the package

#### Scenario: an unreadable lockfile
- **WHEN** `Cargo.lock` does not parse or lists no packages
- **THEN** the gate fails rather than passing

### Requirement: Findings carry a severity and a disposition
A gate's findings SHALL each be classified by `docs/quality-model.md` with a technical severity and,
separately, a programme disposition, and a gate SHALL NOT be accepted while a finding it classified
RELEASE-BLOCKER or GATE-BLOCKER is open.

#### Scenario: a gate report lists its findings
- **WHEN** a gate's report lists a finding
- **THEN** the finding names its severity, its disposition, its taxonomy tag and its status
