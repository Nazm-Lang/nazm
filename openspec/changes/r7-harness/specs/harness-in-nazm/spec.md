## ADDED Requirements

### Requirement: A ported check gives the Rust gate's verdict
A gate of `cargo xtask check` ported to Nazm SHALL print, on the same tree, the same report lines as
the Rust gate — `ok` or `FAIL`, its name, and each problem — and SHALL exit non-zero exactly when
the Rust gate fails.

#### Scenario: the tree as it is
- **WHEN** `nazm-check ROOT` and `cargo xtask check` run on the same checkout
- **THEN** their report lines for every ported gate are identical

#### Scenario: a seeded defect
- **WHEN** a scratch copy of the tree breaks one rule a ported gate enforces
- **THEN** both report the gate as `FAIL` with the same problem lines

### Requirement: A ported test is held to the Rust test
A test file ported to Nazm SHALL pass where the Rust file passes, and each of its tests SHALL fail
under every catalogued mutation whose recorded killer is the corresponding Rust test. A test SHALL
NOT be weakened, skipped or narrowed to agree.

#### Scenario: the pristine tree
- **WHEN** the Rust file and the Nazm file run against the same `nazm`
- **THEN** every test passes in both

#### Scenario: a catalogued mutation
- **WHEN** a mutation naming a Rust test of the file as its killer is applied and `nazm` rebuilt
- **THEN** the Rust test and the Nazm test of the same name both fail, and both pass once the
  mutation is restored

### Requirement: The harness states what a child receives
Every child a harness program starts SHALL be given its directory and its whole environment
explicitly, an empty standard input, and a deadline after which its process group is ended.

#### Scenario: a child that does not end
- **WHEN** a child outlives its deadline
- **THEN** its process group is ended and the run reports that it timed out, never a pass
