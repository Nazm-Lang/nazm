## 1. R0 — decisions and design

- [x] 1.1 Survey the toolchain, the Nazm compiler and the language's readiness (2026-10-11, `9648a6c`)
- [x] 1.2 Record the copyright holder's decisions (this change's `design.md`, *Decisions* 1–4)
- [x] 1.3 Propose R1's language and library design (`r1-language-for-the-rewrite`) and have it approved
      (approved 2026-10-11: D2–D5, D6 deferred, L2, L4–L14, P1–P3, S2–S5)
- [x] 1.4 The performance budgets' method set (measured in R2: `r2-front-end` D26, 5.2)
## 2. R1 — language and library prerequisites

- [x] 2.1 Carry out `r1-language-for-the-rewrite`, in the Rust reference and the Nazm library
      (closed 2026-10-15; published as 0.10.0)

## 3. R2 — the Nazm compiler carries the whole language

- [x] 3.1 Every construct `compiler/*.nz` refused is carried by the new front end (`r2-front-end` 3.1*, 5.1f, 7.8: the whole
      corpus checked and lowered, nothing stopped; remaining stops: `r2-front-end/stops-audit.md`). The bootstrap chain
      (`analyse.nz`, `emit.nz`) still refuses them and is retired by `r2-front-end` 2.8b
- [x] 3.2 Diagnostics identical to Rust's, text and JSON, with error recovery (`r2-front-end` 2.7, 4.1, 4.2: check, render, facts, keys 0 differ)
- [x] 3.3 Per-module output; the performance budgets measured (R2: `r2-front-end` D26, D28)
- [ ] 3.4 The black-box tests that check, build or run programs pass with `NAZM_BIN` = the Nazm compiler. Open: `r8-cut-over`
      14.1 (2026-11-04: 896 of 1,471 pass, 536 fail only on the Nazm `nazm`, 32 on both; classified in design S14, not fixed).
      Blocks 0.11.0 unless the copyright holder accepts the classified failures (decision 2026-11-04 keeps 14.1 open)
## 4. R3 — the native pipeline

- [x] 4.1 Ownership and cleanup (MIR), LIR, the runtime generator, the object cache, debug info (`r3-pipeline`, 37 of 38 boxes; the 38th is the deferred 8.3)
- [x] 4.2a macOS (aarch64 and x86_64-apple-darwin), narrowed by the copyright holder 2026-10-15, done in R3 8.2
- [ ] 4.2b The other targets of `docs/user/support.md`: Linux, WebAssembly and WASI are in scope for 0.11.0 (the copyright holder, 2026-11-05); Android, iOS and the boards stay deferred until after 1.0 (2026-10-15); Windows x86_64 is `r8-cut-over` 17.1, `windows-toolchain` W3, before 1.0
## 5. R4 — the interpreter
- [x] 5.1 The interpreter in Nazm (`r4-interpreter`, 30 of 30 boxes; `toolchain/interp`, in the self-build's nine programs); the interpreter-driven suites pass (r4 evidence)
## 6. R5 — every command
- [x] 6.1 Every command ported: `fmt`, `check`/`fix`, `test`, `doc`, `init`, `lsp`, the MCP server, `context`/`snapshot`/`repo`, packages,
      `bindgen`, `contract`, `accel`, `bench`/`profile`, the grammar (`r5-commands`, `r8-cut-over` slices 9-11). Known
      differences left: `r5-commands` 11.2 and `r8-cut-over` 11.1 (registry manifests, `check --cache-report`, a few explain/obligations programs)
## 7. R6 — host qualification
- [x] 7.1 The Nazm toolchain builds itself on a host under a bounded heap, with recorded peak memory
      (2026-10-20, `r6-host-qualification`: contained, Linux aarch64, 2 GiB heap; stages 1–3
      identical; the interpreter and `nazm-check` wait on their move off 0.10.0)
- [x] 7.2 A recorded decision revising the runbook's containment rule (accepted by the copyright
      holder 2026-10-20, as proposed in `r6-host-qualification` design decision 3; `docs/internal/runbook.md`
      changed and the host runner written: `cargo xtask host-run`)

## 8. R7 — harness and tests
- [x] 8.1 The `cargo xtask` checks (`r7-harness` 6.1), the mutation runner and catalogue, the contained runner, bench, fuzz (`r8-cut-over` 13.2-13.6, `nzx`).
      Follow-up open: `r8-cut-over` 13.8 (the catalogue's killer names, 89 of 94 entries name a whole test)
- [ ] 8.2 The Rust test files as Nazm tests driving the binary: `r8-cut-over` 14.2 (2026-11-04) kept all 64 entries as the seed's, none
      ported through the CLI; the matching run is 14.1 (open). Blocks 0.11.0 with 3.4; the porting itself: after 0.11.0 before 1.0
## 9. R9 — Nazm's own code generator
- [x] 9.1 LIR to Mach-O and ELF objects, aarch64 and x86_64, line tables; the differential suite against LLVM (`r9-code-generator`, 33 of 34 boxes; the 34th is the ELF run, deferred after 1.0)
## 10. R8 — cut-over, then the release gate
- [ ] 10.1 The released `nazm` built by Nazm; the Rust implementation frozen as the seed; the fixpoint. Fixpoint done (`r8-cut-over` 15.1, 16.3, 16.4).
      Open: 18.1 (release path archives the Nazm-built binaries), 19.1 (the gate on the seed's paths); blocks 0.11.0
- [ ] 10.2 Authorities updated: architecture, bootstrap, capability matrix, support, licensing, runbook. Runbook amendment for host runs done (`r8-cut-over` 16.5); the rest is `r8-cut-over` 20.1; blocks 0.11.0
- [ ] 10.3 The release gate: the whole mutation catalogue, fuzzing, archives, provenance. Part of the 0.11.0 gate itself; blocks 0.11.0 (nothing recorded as run)
