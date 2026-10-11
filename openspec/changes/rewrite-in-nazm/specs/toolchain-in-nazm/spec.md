## ADDED Requirements

### Requirement: The shipped toolchain is written in Nazm
Every program in a Nazm release — `nazm`, the MCP server, and the runtime generator — SHALL be
compiled from Nazm source by a Nazm compiler, and the release SHALL contain no executable code
compiled from another language except what the platform supplies (the system C library, and the
`clang` a user installs).

#### Scenario: a release after cut-over
- **WHEN** a release archive is built after R8
- **THEN** its build provenance names Nazm source for every shipped executable, and its SBOM lists
  no third-party library linked into one

### Requirement: Behaviour is held by the existing tests
The Nazm toolchain SHALL pass every black-box test the Rust toolchain passes, run against it through
`NAZM_BIN`, before the phase that ports that behaviour closes. A test SHALL NOT be weakened, deleted
or skipped to let a phase close; a reduction in coverage SHALL be recorded in the capability matrix
with the phase that restores it.

#### Scenario: a phase closes
- **WHEN** a phase of the rewrite is declared complete
- **THEN** its tests passed against both the Rust and the Nazm-built binary, and the log is cited

#### Scenario: the Cranelift cases before R9
- **WHEN** the Cranelift backend is removed at cut-over before its replacement passes them
- **THEN** the capability matrix records the coverage reduction and names R9 as restoring it

### Requirement: Nazm builds from source without a downloaded binary
The frozen Rust implementation SHALL build a Nazm toolchain that builds itself to a fixpoint: stage 2
and stage 3 byte-identical once identifiers that vary by build are stripped.

#### Scenario: a distribution bootstraps
- **WHEN** the frozen Rust seed builds stage 1, stage 1 builds stage 2 and stage 2 builds stage 3
- **THEN** stage 2 and stage 3 are identical, and stage 3 passes the release test suite

### Requirement: Nothing is published before the release gate
No archive, tag, package or binary of the toolchain SHALL be published until R8 and the release
gate are complete and the copyright holder has authorised publication.

#### Scenario: a request to publish mid-programme
- **WHEN** a phase of the rewrite is complete but R8 or the release gate is not
- **THEN** nothing is published, and the request is reported as blocked by this requirement
