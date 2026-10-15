## ADDED Requirements

### Requirement: The toolchain runs on Windows
`nazm` SHALL build, run, format, check and test programs on Windows x86_64 as it does on macOS.

#### Scenario: A Windows developer builds a program
- **WHEN** `nazm build hello.nz` runs on Windows with the documented dependencies installed
- **THEN** it writes `hello.exe`, which prints what the macOS build prints
