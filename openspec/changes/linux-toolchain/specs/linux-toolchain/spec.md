## ADDED Requirements

### Requirement: The toolchain runs on Linux
The released `nazm` for `aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu` SHALL build and
run programs for its own host on Linux without any environment variable beyond `PATH`, with `clang`
and the C library's headers installed.

#### Scenario: A Linux developer builds a program
- **WHEN** `nazm build hello.nz` runs on Linux from an unpacked release archive whose `bin/` is on `PATH`
- **THEN** it writes `hello`, an ELF executable for the host, which prints what the macOS build prints

#### Scenario: A release build on Linux
- **WHEN** `nazm build --release hello.nz` runs on Linux
- **THEN** the executable is a stripped position-independent ELF and no signature step runs

### Requirement: The Linux archives come from a passed self-build
A Linux release archive SHALL be made only from a `nazm.selfbuild-record/1` record that says
`result passed` for the archive's commit and tree, and whose stages agree byte for byte.

#### Scenario: The aarch64 archive
- **WHEN** `scripts/release-archive.sh aarch64-unknown-linux-gnu RECORD OUT` runs on a record of `linux-selfbuild.sh`
- **THEN** it writes `nazm-aarch64-unknown-linux-gnu.tar.gz` only if the record passed and each image is an aarch64 ELF, PIE, stripped, with the counted heap at 2 GiB

#### Scenario: The x86_64 archive is cross-built
- **WHEN** the x86_64 Linux record is made by an aarch64 Linux stage 3
- **THEN** the nine programs are built twice and identical, run under amd64 emulation agreeing with aarch64, and the record names its builder's record and the emulation
