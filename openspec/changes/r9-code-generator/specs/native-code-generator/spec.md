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

### Requirement: A failure crosses calls as the LLVM build's does
On `aarch64-apple-darwin` and `x86_64-apple-darwin` a failure SHALL unwind as the LLVM backend's
does (forced unwind, the C personality, a landing pad only where the failure edge has work, every
plain entry landing it), so that objects of the two backends link into one program, and every
frame on the way out SHALL release what it owns.

#### Scenario: a failure deep in a recursion, every frame owning a string
- **WHEN** a program fails four hundred frames deep, each frame holding a string
- **THEN** the generated build prints the same output and diagnostic, exits with status 2 and
  reports every string reclaimed, as the LLVM build does

#### Scenario: a program linked from both backends' units
- **WHEN** a program of several units is linked with some units' objects from the generator and
  the others from LLVM
- **THEN** it behaves as the LLVM build does, failures included
