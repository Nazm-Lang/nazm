## ADDED Requirements

### Requirement: Every v1 domain claim has executable evidence
Every domain `docs/v1-domain-matrix.md` marks VERIFIED SHALL have a reference workload that was
built and executed — natively, under Rosetta, in an emulated container, under Wine or under QEMU —
and the row SHALL name which.

#### Scenario: a domain without a run is not verified
- **WHEN** a domain's workload has only been compiled
- **THEN** its row says PARTIAL, PLANNED or BLOCKED with the reason, never VERIFIED

### Requirement: Profiles restrict and never reinterpret
A domain profile SHALL be a set of refusals over the one language, and a program a profile accepts
SHALL mean the same with and without it.

#### Scenario: a profile only refuses
- **WHEN** a program passes a domain profile
- **THEN** its output in every implementation equals its output under `general`

### Requirement: Platform evidence names where it ran
Evidence from Wine SHALL be labelled "run-verified under Wine" and SHALL NOT be presented as
evidence from a Windows host; Rosetta and emulated-container evidence SHALL be labelled likewise.

#### Scenario: a Windows row
- **WHEN** Windows programs have run only under Wine
- **THEN** the Windows row says so and lists real Windows execution as remaining
