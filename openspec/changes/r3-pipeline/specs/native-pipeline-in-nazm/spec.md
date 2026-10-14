## ADDED Requirements

### Requirement: MIR identical to the Rust toolchain's
The Nazm pipeline SHALL print, for a checked program's Core IR, the `nazm.mir/1` text `nazm mir`
prints for that program, byte for byte, executable digests included.

#### Scenario: An input both can lower
- **WHEN** `toolchain/pipeline/tests/run.sh` runs over an input with Core IR
- **THEN** the pipeline's MIR equals `nazm mir FILE`, or the difference is listed as a finding with
  its reason

### Requirement: LIR identical to the Rust toolchain's
The Nazm pipeline SHALL print the `nazm.lir/2` text `nazm lir` and `nazm lir --ops` print for that
program.

#### Scenario: An input both can lower to LIR
- **WHEN** the comparison runs over an input whose LIR the Rust toolchain prints
- **THEN** the pipeline's LIR equals it, or the difference is listed with its reason

### Requirement: Differences are never hidden
The comparison SHALL count an input the pipeline cannot lower, or lowers differently, as a
difference; it SHALL NOT normalise either side to make them agree.

#### Scenario: A construct the pipeline does not yet carry
- **WHEN** an input uses a construct the pipeline refuses
- **THEN** the summary counts it under `differ`, and the reason is printed
