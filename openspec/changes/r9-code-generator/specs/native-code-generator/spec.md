## ADDED Requirements

### Requirement: The generator turns LIR into linkable objects
Nazm's own code generator SHALL read a program's units as `nazm lir --ops` prints them and SHALL
write one relocatable object per unit for the target, which links with the runtime and entry
objects `nazm build --objects` writes in place of that command's unit objects.

#### Scenario: fib on aarch64-apple-darwin
- **WHEN** `bench/compare/nazm/fib.nz` is built with the generator's object for its unit and run
- **THEN** it prints `9227465` and exits with status 0, as the LLVM build does

### Requirement: Behaviour agrees with the LLVM build
For every input of the differential suite, the program linked from the generator's objects SHALL
produce the same standard output, standard error and exit status as the same program built by
`nazm build` with LLVM. A difference SHALL be fixed in the generator or reported as a finding; the
comparison SHALL NOT be weakened.

#### Scenario: a failing program
- **WHEN** an input fails at run time (an overflow, an index out of bounds)
- **THEN** both builds write the same failure text to standard error and exit with the same status

### Requirement: What is not carried is refused
The generator SHALL refuse, by name and with exit status 3, any LIR form it does not translate, and
the suite SHALL count such an input as refused, never as agreeing.

#### Scenario: an unsupported instruction
- **WHEN** a unit contains an instruction the generator does not translate yet
- **THEN** no object is written, standard error names the instruction, and the suite's summary
  counts the input under `refused`

### Requirement: Output is deterministic
The generator SHALL write the same bytes for the same LIR and target.

#### Scenario: two runs
- **WHEN** the generator runs twice over the same input
- **THEN** the objects are byte-identical
