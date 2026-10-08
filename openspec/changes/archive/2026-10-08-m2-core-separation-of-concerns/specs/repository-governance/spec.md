## ADDED Requirements

### Requirement: A split module declares its responsibility and keeps its entry thin
`cargo xtask check` SHALL fail when a module directory listed in the module-boundaries register
lacks a `//!` responsibility sentence on any of its files, or when its `mod.rs` defines anything
other than module declarations, re-exports, the façade's entry points, the shared context types and
their orchestration — each permitted item named in the register.

#### Scenario: a new concern is added to an entry module
- **WHEN** a function not named in the register is defined at the top level of a registered `mod.rs`
- **THEN** the module boundaries gate fails, naming the file and the function and asking for it to
  be placed in a module of its own

#### Scenario: a file without a responsibility line
- **WHEN** a `.rs` file in a registered module does not open with a `//!` line
- **THEN** the gate fails, naming the file

#### Scenario: a registered module that does not exist
- **WHEN** the register names a directory that is not in the tree
- **THEN** the gate fails rather than passing silently

### Requirement: Gates that read source by path follow moved code
A `cargo xtask check` gate that inspects named source files SHALL read every file of a module that
was split into a directory, so that moving code never narrows what the gate inspects.

#### Scenario: a checker file becomes a directory
- **WHEN** `crates/nazm-core/src/check.rs` is split into `crates/nazm-core/src/check/`
- **THEN** the gates that read the checker read every `.rs` file under `check/`
