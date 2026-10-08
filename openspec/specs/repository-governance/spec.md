# repository-governance Specification

## Purpose
TBD - created by archiving change m1-agent-governance-gate. Update Purpose after archive.
## Requirements
### Requirement: Agent instruction files stay consistent with their authorities
`cargo xtask check` SHALL fail when the repository's agent-instruction files no longer agree with
the structure `docs/agent-governance.md` defines, and SHALL do so by reading the tree only.

#### Scenario: the Claude shim stops deferring
- **WHEN** `CLAUDE.md` does not refer the reader to `AGENTS.md`
- **THEN** the agent governance gate fails, naming `CLAUDE.md`

#### Scenario: an authority link breaks
- **WHEN** `AGENTS.md` links to a repository path that does not exist
- **THEN** the gate fails, naming the missing path

#### Scenario: a nested file relaxes the root
- **WHEN** a nested `AGENTS.md` grants an action the root forbids, or does not say it specialises the root
- **THEN** the gate fails, naming the file

#### Scenario: a template loses a required field
- **WHEN** a file under `.agent/templates/` is missing or lacks a field its convention requires
- **THEN** the gate fails, naming the template and the field

### Requirement: OpenSpec holds changes, not a second specification
`cargo xtask check` SHALL fail when an OpenSpec change has no proposal, or when `openspec/specs/`
holds a capability outside the repository-process capabilities.

#### Scenario: a language capability is archived into OpenSpec
- **WHEN** `openspec/specs/` contains a capability that is not a repository-process capability
- **THEN** the gate fails, saying such a change archives with `--skip-specs` and its text belongs in the Nazm authorities

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

