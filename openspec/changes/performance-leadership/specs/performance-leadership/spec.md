## ADDED Requirements

### Requirement: Claims rest on reproducible evidence
Nazm SHALL NOT be described as faster, smaller or leaner than another language except where the
published comparison measures it, reproducibly, with the safety of each row stated.

#### Scenario: A result is published
- **WHEN** `docs/performance.md` reports a lead
- **THEN** the workload, inputs, host, runs and confidence interval behind it are published with it

### Requirement: Safety is never traded for speed
An optimisation SHALL remove a check only where the compiler proves the check cannot fail.

#### Scenario: An optimisation removes a check
- **WHEN** a pass drops a bounds, overflow or range check
- **THEN** a test shows a program where the check is needed still stops as the language says
