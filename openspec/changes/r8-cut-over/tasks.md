## 0. The plan
- [x] 0.1 The inventory, the slices in order with sizes and models, the open questions (`design.md`)
- [ ] 0.2 The copyright holder's answers to `design.md`'s four open questions

## 1. Slice 1 — R6 gap 1: the interpreter and `nazm-check` on the checkout's language
- [x] 1.1 `toolchain/interp/{types,values,vm}.nz` and `toolchain/harness/check/supply.nz`: `List[T]`
      and `list_*` to `Chain[T]` and `chain_*` (R1.5 D7); the checkout's `nazm check` finds no error
- [x] 1.2 `toolchain/selfbuild/selfbuild.sh`: `interp` and `check` among the programs every stage
      builds (so in the fixpoint); the last stage's interpreter runs the corpus against the seed's
      `run --interpret`, its `nazm-check` reports the tree against the seed's build of it
- [x] 1.3 The scripts that built them with 0.10.0 build them with the checkout's `nazm` and a 2 GiB
      heap (`toolchain/interp/tests/run.sh`, `toolchain/harness/tests/{run,seeded}.sh`); the
      interpreter's suite refuses to run uncontained
      (`cargo xtask contained run 'cargo build --locked -p nazm-cli && sh toolchain/interp/tests/run.sh'`)
- [ ] 1.4 `cargo xtask contained selfbuild`: stages 1–3 identical with the two programs added, the
      interpreter and `nazm-check` agreeing — **unexecuted** at this commit: the copyright holder's
      gate held the machine; the host evidence is `design.md`, *Slice 1 — the evidence*

## 2. Slice 2 — the comparison scripts under the R6 rule
- [ ] 2.1 `cargo xtask host-run`: the program's exit status passed through; its report on standard
      error or in a file, not on standard output
- [ ] 2.2 Every script under `toolchain/` that runs a Nazm-built program (front end, pipeline,
      code generator, commands, harness, interpreter) builds it with the checkout's `nazm` and a heap
      limit, and runs it contained or through `host-run`; none defaults to the 0.10.0 release

## 3. Slice 3 — R6 gap 2
- [ ] 3.1 `callee_at` (`call`, `call_indirect`, `call_trait`) and a payload binding's `at` in
      `--coredoc-sources`; the self-build compares Core IR whole (`r6-host-qualification` 1.8)

## 4. Slice 4 — `nazm build` in Nazm: the driver
- [ ] 4.1 Design (Opus): one program or `libexec/` children; the object cache; the counted heap in
      the LIR; the one-thread decision; linking per target
- [ ] 4.2 `build` group 1 (`design.md`) for the macOS triples, Windows x86_64 and the internal Linux
      one; compared with the Rust `build --backend nazm` on the corpus: objects with `--objects`,
      the executables' behaviour
- [ ] 4.3 `selfbuild.sh` builds through the stage's `nazm build` instead of its glue

## 5. Slice 5 — the LLVM path
- [ ] 5.1 LIR → LLVM IR text (`nazm-lir`'s `op/llvm.rs`, `op/soa.rs`, `debug.rs`), byte for byte
      with `nazm build --emit-ir`
- [ ] 5.2 `build` group 2: `--backend llvm` the default, `--opt-level`, `--emit-ir`, `--debug`,
      `--layout soa`, `--release`

## 6. Slice 6 — the rest of `build`
- [ ] 6.1 `build` group 3; `nazm runtime build` and `verify`

## 7. Slice 7 — the interactive tier
- [ ] 7.1 Design (Opus): `run` without Cranelift; the interpreter inside `nazm`
- [ ] 7.2 `run` (compiled, `--interpret`, `--json`, arguments, exit status), `repl`, `comptime`;
      `test` without `NAZM_TEST_DRIVER`

## 8. Slice 8 — the inspection commands
- [ ] 8.1 `core-ir`, `mir`, `lir`, `capabilities`, `inspect`, `obligations`, `explain-cost`,
      `explain-flow`, `trace`, `profile`

## 9. Slice 9 — the agent commands whole
- [ ] 9.1 R2 publishes a function's effects; `context` and MCP `nazm.semantic_context`
- [ ] 9.2 `repo` (`nazm-repo` ported); MCP `nazm.repository_map`, `nazm.task_context`

## 10. Slice 10 — every command routed
- [ ] 10.1 `bench`, `accel`; `contract` as question 1 decides

## 11. Slice 11 — R5's refusals
- [ ] 11.1 `check`, `lsp`, `patch`, `references`, `resolve`, `fix`, the MCP server: every refusal
      `design.md` lists, with R2's hand-backs

## 12. Slice 12 — a `nazm` that needs no `NAZM_LIBRARY`
- [ ] 12.1 The standard library and the documentation corpus carried or installed; the archive
      layout

## 13. Slice 13 — the harness remainder (`r7-harness` 6.2, 6.3)
- [ ] 13.1 Design (Opus): the contained runner and fuzzing in Nazm (question 3)
- [ ] 13.2 The contained runner, `host-run`, the mutation campaign runner, `bench`, fuzzing

## 14. Slice 14 — the tests are the contract
- [ ] 14.1 The black-box suite against the Nazm-built `nazm`; `rewrite-in-nazm` 3.1, 3.2, 3.4
- [ ] 14.2 `stays-rust.txt` revisited: each test ported through the CLI or kept as the seed's

## 15. Slice 15 — the chain on Linux
- [ ] 15.1 Seed → stage 1 → 2 → 3 through Nazm's `nazm build`, the released set, contained

## 16. Slice 16 — the fixpoint on macOS
- [ ] 16.1 As question 2 decides; aarch64, and x86_64 under Rosetta

## 17. Slice 17 — Windows x86_64
- [ ] 17.1 The released set built by Nazm for `x86_64-pc-windows-gnu`; the host-safe suite on the
      Windows host (`windows-toolchain` W3 3.1, 3.2)

## 18. Slice 18 — the cut-over
- [ ] 18.1 The release path builds and archives the Nazm-built binaries (`rewrite-in-nazm` 10.1)

## 19. Slice 19 — the seed frozen
- [ ] 19.1 A gate on the seed's paths; `docs/bootstrap.md`'s chain

## 20. Slice 20 — the authorities (`rewrite-in-nazm` 10.2)
- [ ] 20.1 `docs/architecture.md`, `bootstrap.md`, `capability-matrix.md`, `support.md`,
      `licensing.md`, `runbook.md`
