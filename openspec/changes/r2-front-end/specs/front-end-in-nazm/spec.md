## ADDED Requirements

### Requirement: The Nazm front end reads every program as the reference does
The front end written in Nazm (`compiler/*.nz`) SHALL produce, for every input of the R2 corpus,
the same tokens as the Rust lexer and the same syntax tree as the Rust parser, compared in the
forms `openspec/changes/r2-front-end/design.md` (D2, D3) fixes.

#### Scenario: an input that lexes and parses
- **WHEN** `toolchain/frontend-tests/run.sh` compares an input the reference parses without diagnostics
- **THEN** the Nazm lexer's token dump and the Nazm parser's tree dump are byte-identical with the
  reference dumper's

#### Scenario: an input with syntax errors
- **WHEN** the reference reports lexing or parsing diagnostics for an input
- **THEN** the Nazm front end reports the same diagnostics, in the same order, and the tree it
  recovered is the reference's

### Requirement: The Nazm checker's diagnostics are the reference's
For every input of the R2 corpus the Nazm checker SHALL report exactly the diagnostics
`nazm check --json` reports — code, severity, message, spans, labels, help and fixes.

#### Scenario: a refused program
- **WHEN** the reference refuses an input
- **THEN** the Nazm front end refuses it with identical diagnostic objects

### Requirement: The Nazm front end's Core IR is the reference's
For every input the reference accepts, the Nazm front end SHALL print Core IR byte-identical with
`nazm core-ir`.

#### Scenario: an accepted program
- **WHEN** an input checks under the reference
- **THEN** the two `nazm.core-ir/2` documents are byte-identical, digests included

### Requirement: The compiler keeps compiling itself
Every slice SHALL leave `compiler/` compiling itself: the reference checks its source, and the
bootstrap chain reaches its fixpoint.

#### Scenario: a slice closes
- **WHEN** a slice of R2 is committed
- **THEN** the track's host check passed on that commit, and the contained bootstrap is named as
  the coordinator's remaining check
