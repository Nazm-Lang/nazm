## ADDED Requirements

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
