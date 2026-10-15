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
- [ ] 4.5 The remaining black-box files, `common`/`differential`/`memory_report`-only first — 85
      of the 186 test files of `crates/nazm-cli/tests/` so far (every test agreeing with its Rust
      test on the tree, `tests/run.sh`; `tests/mutants.sh` over the mutations the first files kill:
      see each commit). Docker-only `#[ignore]`d tests stay Rust (design decision 10). Left: the
      files that call a Rust API (`builtins`, `fuzz_replay`, `repository`, `resolution`,
      `documentation`, `manifests`, `domain_profiles`, `profiles_v2`, `numbers`, `cross`, `docs`,
      `boards`, `realtime`, `cranelift`, `core_ir`, `reexports`, `ffi`, `runtime`, `prelude`,
      `summary`, `mir`, `durable_identity`, `native_cache`, `incremental`, `build`, …), two that
      drive concurrent processes over TCP (`distributed`, `secure_service`), and the rest
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
