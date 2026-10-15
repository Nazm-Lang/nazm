## 1. The proposal
- [x] 1.1 What a Nazm test is, how files, processes and temporary directories are reached, the order
      of porting (`design.md`)

## 2. The harness library
- [x] 2.1 `lib/run.nz`: a child with a stated environment, both streams drained, a deadline that ends
      its process group
- [x] 2.2 `lib/scratch.nz`: scratch directories made, filled and removed
- [x] 2.3 `lib/differential.nz`: `everywhere`, `agrees`, `codes`, `bounded_exe`
- [x] 2.4 `tests/lib_self.nz`: the library's own tests

## 3. `xtask check`'s file-reading gates
- [x] 3.1 `check/main.nz`: the runner and the report, byte for byte
- [x] 3.2 Docs corpus (what `Corpus::load` refuses; the digests and indexes it serves are R5's `nazm docs`)
- [x] 3.3 Guide from grammar
- [x] 3.4 Third-party notices (`cargo metadata` as a child, `@std/json`; a text met before is found by itself, which is what the Rust gate's BLAKE3 of it stands for)
- [x] 3.5 Module boundaries
- [ ] 3.6 Doc claims, capability matrix, dependency sources, performance claims, agent governance
      (done); mutations apply (its exactly-once half done, `nazm-mutate check`; `plan::validate`
      open)
- [x] 3.7 Compared with the Rust gates on the tree and on seeded copies (`tests/seeded.sh`, every
      gate ported so far; extended with each gate)

## 4. The black-box CLI tests
- [x] 4.1 `release_examples`
- [x] 4.2 `line_breaks`
- [x] 4.3 `short_variants` (the brief's `shorter_forms`)
- [x] 4.4 Compared with the Rust tests on the tree and under every catalogued mutation they kill
      (`tests/mutants.sh`: 18 mutations, each caught by the Rust killer and the Nazm test)
- [ ] 4.5 The remaining black-box files, `common`/`differential`/`memory_report`-only first — 124
      of the 176 test files of `crates/nazm-cli/tests/` so far (the directory's 194 entries less
      the five `cli_*` roots, `selfhost` and twelve fixture directories; every test agreeing with its
      Rust test on the tree, `tests/run.sh`; `tests/mutants.sh` over the mutations the first files
      kill: see each commit). `#[ignore]`d tests that start a container or need a GPU stay Rust
      (design decision 10). Left, for the coordinator (design, *Open Questions*):
      - the 25 that call a Rust API — most in one to three of their tests: `boards`, `build`,
        `builtins`, `core_ir`, `cranelift`, `cross`, `docs`, `documentation`, `domain_profiles`,
        `durable_identity`, `ffi`, `freestanding`, `fuzz_replay`, `incremental`, `manifests`, `mir`,
        `native_cache`, `prelude`, `profiles_v2`, `realtime`, `reexports`, `repository`,
        `resolved_units`, `runtime`, `summary` (`numbers` and `resolution` name a crate only in
        their comments, and are ported);
      - the two that drive concurrent processes and a TCP client from the test (`distributed`,
        `secure_service`; `net`'s TCP is all in its Nazm programs, and it is black-box);
      - 25 black-box files not yet ported: `accel_providers`, `bytes_text`, `cost_and_flow`,
        `debugging`, `domains`, `ecosystem`, `ffi_v2`, `ffi_v3`, `flow_v3`, `native_units`,
        `nazm_backend`, `packages`, `regex`, `schemas`, `stdlib`; six with some tests `#[ignore]`d
        for a container, which stay Rust (`debugger_v3` 2 of 7, `evm` 1 of 4, `mobile` 2 of 4,
        `wasm` 3 of 8, `wasm_contract` 1 of 3, `windows` 2 of 6); and four that need more of the
        harness: `lsp` (an interactive
        session over a child's standard input and output), `scheduler_pool` (a running child's CPU
        time, read while it runs), `select` (a C shim preloaded into the program), `fuzz_diff` (a
        seeded program generator reproduced bit for bit)
- [x] 4.6 Re-run `tests/mutants.sh` over the files ported since the rebase onto `613047d2`: the 45
      of them a catalogued mutation names a killer in, the catalogue's first such mutation each
      (`MUTANTS_PER_FILE=1`; 281 mutation-killer pairs in all, each a rebuild of `nazm` and a test
      binary): 45 agree, 0 differ, every one caught by the Rust killer and the Nazm test alike

## 5. The mutation runner's core
- [x] 5.1 Read the catalogue; apply exactly once; restore byte for byte, in the Rust runner's own
      backup record (`cargo xtask mutate-recover` repairs a tree `nazm-mutate` left mutated)
- [x] 5.2 The verdict from a killer's results, pristine, mutated and restored (`nazm-mutate try`)
- [ ] 5.3 Compared with the Rust runner's `--plan` and apply gate

## 6. Later (not this track's first pass)
- [ ] 6.1 The gates that start `cargo metadata` (layering, reach, unsafe policy)
- [ ] 6.2 `bench`, `fuzz`
- [ ] 6.3 The contained runner — when the coordinator moves it
