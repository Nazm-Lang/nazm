## ADDED Requirements

### Requirement: Each command in Nazm behaves as the Rust command
A command written in Nazm under `toolchain/commands/` SHALL, for every input of the comparison,
write the same bytes to standard output and standard error, exit with the same status and leave
the same files behind as the same command of the Rust `nazm` built from the same checkout.

#### Scenario: fmt over the corpus
- **WHEN** `toolchain/commands/tests/run.sh` formats a copy of an input with each side, with and
  without `--check`
- **THEN** both runs' standard output, standard error and exit status, and the file's bytes
  afterwards, are byte-identical with the reference's

#### Scenario: a file that does not lex
- **WHEN** an input has a lexing error
- **THEN** the Nazm `fmt` renders the same diagnostics as the reference, writes nothing to that file
  and fails as the reference does

### Requirement: A command refuses what it does not carry
A command written in Nazm SHALL NOT print an output the reference would not print. Where it meets
something it does not carry, it SHALL say so on standard error and exit with status 6.

#### Scenario: an unported option
- **WHEN** the Nazm `fmt` is given an option it does not carry
- **THEN** it exits 6 with a line naming the option, and the comparison counts it as refused, not
  as agreeing
