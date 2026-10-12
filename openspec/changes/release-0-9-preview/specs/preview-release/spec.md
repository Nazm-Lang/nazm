## ADDED Requirements

### Requirement: A preview states what it does not promise
A preview release SHALL be versioned below 1.0 and SHALL describe the language as a 1.0 draft:
no document shipped with it SHALL promise compatibility across versions before 1.0.

#### Scenario: a reader of the preview's documents
- **WHEN** a document in the 0.9.0 archive names the language's or the toolchain's version
- **THEN** it says 0.9.0 for the toolchain and "1.0 draft" for the language

### Requirement: A shipped binary carries the notices it owes
A release archive SHALL contain the licence text and notice of every third-party library linked
into a shipped executable, generated from the lockfile it was built from.

#### Scenario: the 0.9.0 archive
- **WHEN** `THIRD-PARTY-NOTICES.md` in the archive is compared with `Cargo.lock`
- **THEN** every crate linked into `bin/nazm` appears with its licence expression and text

### Requirement: The preview names what was not done
The release notes SHALL name every release check not run, and every platform not supported.

#### Scenario: the waived mutation run
- **WHEN** the release notes are read
- **THEN** they say the full mutation catalogue was not run for 0.9.0, and that only macOS on Apple
  Silicon is supported
