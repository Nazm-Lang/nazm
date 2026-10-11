## 1. R0 — decisions and design

- [x] 1.1 Survey the toolchain, the Nazm compiler and the language's readiness (2026-10-11, `c06dcd2`)
- [x] 1.2 Record the copyright holder's decisions (this change's `design.md`, *Decisions* 1–4)
- [ ] 1.3 Propose R1's language and library design (`r1-language-for-the-rewrite`) and have it approved
- [ ] 1.4 Set the performance budgets' method (measured in R2)

## 2. R1 — language and library prerequisites

- [ ] 2.1 Carry out `r1-language-for-the-rewrite`, in the Rust reference and the Nazm library

## 3. R2 — the Nazm compiler carries the whole language

- [ ] 3.1 Every construct `compiler/*.nz` refuses today (escapes, `::`, `as`, packages, widths,
      floats, `Bytes`/`Text`, files, networking, processes, atomics, constants, arrays,
      attributes, `Chan[T]`, `select`) and R1's additions
- [ ] 3.2 Diagnostics identical to Rust's, text and JSON, with error recovery
- [ ] 3.3 Per-module output; the performance budgets measured
- [ ] 3.4 The black-box tests that check, build or run programs pass with `NAZM_BIN` = the Nazm compiler

## 4. R3 — the native pipeline

- [ ] 4.1 Ownership and cleanup (MIR), LIR, the runtime generator, the object cache, debug info
- [ ] 4.2 Every target in `docs/support.md`, with its tests

## 5. R4 — the interpreter
- [ ] 5.1 The interpreter in Nazm; the interpreter-driven suites pass

## 6. R5 — every command
- [ ] 6.1 `fmt`, `check`/`fix`, `test`, `doc`, `init`, `lsp`, the MCP server, `context`/`snapshot`/`repo`,
      packages (`lock`, `update`, `publish`, `yank`, `attest`), `bindgen`, `contract`, `accel`,
      `bench`/`profile`, the command-line grammar

## 7. R6 — host qualification
- [ ] 7.1 The Nazm toolchain builds itself on a host under a bounded heap, with recorded peak memory
- [ ] 7.2 A recorded decision revising the runbook's containment rule

## 8. R7 — harness and tests
- [ ] 8.1 The `cargo xtask` checks, the mutation runner and catalogue, the contained runner, bench, fuzz
- [ ] 8.2 The Rust test files as Nazm tests driving the binary

## 9. R9 — Nazm's own code generator
- [ ] 9.1 LIR to Mach-O and ELF objects, aarch64 and x86_64, line tables; the differential suite against LLVM

## 10. R8 — cut-over, then the release gate
- [ ] 10.1 The released `nazm` built by Nazm; the Rust implementation frozen as the seed; the fixpoint
- [ ] 10.2 Authorities updated: architecture, bootstrap, capability matrix, support, licensing, runbook
- [ ] 10.3 The release gate: the whole mutation catalogue, fuzzing, archives, provenance
