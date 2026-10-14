## ADDED Requirements

### Requirement: The Nazm interpreter agrees with the Rust interpreter
The interpreter in `toolchain/interp/` SHALL produce, for every program of its comparison suite,
the same standard output, the same standard error and the same exit status as the Rust
toolchain's `nazm run` (with `--interpret` once that flag exists), including under
`NAZM_RESULT_REPORT`, `NAZM_MEMORY_REPORT` and `NAZM_SCHED_REPORT`. A difference SHALL be either
fixed in the Nazm interpreter or reported as a finding; a comparison SHALL NOT be weakened to pass.

#### Scenario: a program that prints and returns
- **WHEN** the suite runs a program of `tests/compat/v1/run/` under both interpreters
- **THEN** both print the same bytes on each stream and end with the same status

#### Scenario: a run-time failure
- **WHEN** a program of `tests/compat/v1/trap/` fails at run time
- **THEN** both write the same rendered diagnostic — code, message, location, source line, caret
  and help — and end with status 2

### Requirement: The interpreter's budgets are the Rust interpreter's
The Nazm interpreter SHALL stop a program after the same number of loop turns
(`--max-iterations`, default 10,000,000) and at the same call depth (`--max-depth`, default 512)
as the Rust interpreter, with the same diagnostics, and SHALL NOT exhaust its own stack at any
depth the budget allows.

#### Scenario: runaway recursion
- **WHEN** a program recurses without a base case
- **THEN** it stops with `N0403` at the call that passed the depth budget, as the Rust interpreter does

### Requirement: Storage is counted as the Rust interpreter counts it
The Nazm interpreter SHALL count sequences, strings, channels and function values allocated and
reclaimed at the same events as the Rust interpreter, so that `NAZM_MEMORY_REPORT` prints the same
line.

#### Scenario: a string grown in a loop
- **WHEN** a program appends to a `let mut` string in a loop and the memory report is asked for
- **THEN** both interpreters report the same number of strings allocated and reclaimed
