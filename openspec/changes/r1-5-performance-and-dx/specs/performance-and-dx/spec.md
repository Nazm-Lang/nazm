## ADDED Requirements

### Requirement: Measured against the comparison
Each R1.5 change SHALL be measured with `bench/compare/compare.py` before and after, on the same host.

#### Scenario: A change is measured
- **WHEN** an R1.5 task is ticked
- **THEN** `docs/internal/performance-log.md` records its before-and-after numbers
