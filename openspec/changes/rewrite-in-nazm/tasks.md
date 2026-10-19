## 1. R0 — decisions and design

- [x] 1.1 Survey the toolchain, the Nazm compiler and the language's readiness (2026-10-11, `91ccda5`)
- [x] 1.2 Record the copyright holder's decisions (this change's `design.md`, *Decisions* 1–4)
- [x] 1.3 Propose R1's language and library design (`r1-language-for-the-rewrite`) and have it approved
      (approved 2026-10-11: D2–D5, D6 deferred, L2, L4–L14, P1–P3, S2–S5)
- [ ] 1.4 Set the performance budgets' method (measured in R2)

## 2. R1 — language and library prerequisites

- [x] 2.1 Carry out `r1-language-for-the-rewrite`, in the Rust reference and the Nazm library
      (closed 2026-10-15; published as 0.10.0)

## 3. R2 — the Nazm compiler carries the whole language

- [ ] 3.1 Every construct `compiler/*.nz` refuses today (escapes, `::`, `as`, packages, widths,
      floats, `Bytes`/`Text`, files, networking, processes, atomics, constants, arrays,
      attributes, `Chan[T]`, `select`) and R1's additions
- [ ] 3.2 Diagnostics identical to Rust's, text and JSON, with error recovery
- [x] 3.3 Per-module output; the performance budgets measured (R2: `r2-front-end` D26, D28)
- [ ] 3.4 The black-box tests that check, build or run programs pass with `NAZM_BIN` = the Nazm compiler

## 4. R3 — the native pipeline

- [ ] 4.1 Ownership and cleanup (MIR), LIR, the runtime generator, the object cache, debug info
- [ ] 4.2 Every target in `docs/support.md`, with its tests — narrowed by the copyright holder (2026-10-15)
      to macOS (aarch64 and x86_64-apple-darwin), done in R3 8.2; the other targets after 1.0

## 5. R4 — the interpreter
- [ ] 5.1 The interpreter in Nazm; the interpreter-driven suites pass

## 6. R5 — every command
- [ ] 6.1 `fmt`, `check`/`fix`, `test`, `doc`, `init`, `lsp`, the MCP server, `context`/`snapshot`/`repo`,
      packages (`lock`, `update`, `publish`, `yank`, `attest`), `bindgen`, `contract`, `accel`,
      `bench`/`profile`, the command-line grammar

## 7. R6 — host qualification
- [x] 7.1 The Nazm toolchain builds itself on a host under a bounded heap, with recorded peak memory
      (2026-10-20, `r6-host-qualification`: contained, Linux aarch64, 2 GiB heap; stages 1–3
      identical; the interpreter and `nazm-check` wait on their move off 0.10.0)
- [x] 7.2 A recorded decision revising the runbook's containment rule (accepted by the copyright
      holder 2026-10-20, as proposed in `r6-host-qualification` design decision 3; `docs/runbook.md`
      changed and the host runner written: `cargo xtask host-run`)

## 8. R7 — harness and tests
- [ ] 8.1 The `cargo xtask` checks, the mutation runner and catalogue, the contained runner, bench, fuzz
- [ ] 8.2 The Rust test files as Nazm tests driving the binary

## 9. R9 — Nazm's own code generator
- [ ] 9.1 LIR to Mach-O and ELF objects, aarch64 and x86_64, line tables; the differential suite against LLVM

## 10. R8 — cut-over, then the release gate
- [ ] 10.1 The released `nazm` built by Nazm; the Rust implementation frozen as the seed; the fixpoint
- [ ] 10.2 Authorities updated: architecture, bootstrap, capability matrix, support, licensing, runbook
- [ ] 10.3 The release gate: the whole mutation catalogue, fuzzing, archives, provenance
