## 1. The proposal
- [x] 1.1 What a Nazm test is, how files, processes and temporary directories are reached, the order
      of porting (`design.md`)

## 2. The harness library
- [ ] 2.1 `lib/run.nz`: a child with a stated environment, both streams drained, a deadline that ends
      its process group
- [ ] 2.2 `lib/scratch.nz`: scratch directories made, filled and removed
- [ ] 2.3 `lib/differential.nz`: `everywhere`, `agrees`, `codes`, `bounded_exe`
- [ ] 2.4 `tests/lib_self.nz`: the library's own tests

## 3. `xtask check`'s file-reading gates
- [ ] 3.1 `check/main.nz`: the runner and the report, byte for byte
- [ ] 3.2 Docs corpus
- [ ] 3.3 Guide from grammar
- [ ] 3.4 Third-party notices
- [ ] 3.5 Module boundaries
- [ ] 3.6 Doc claims, capability matrix, dependency sources, mutations apply, performance claims,
      agent governance
- [ ] 3.7 Compared with the Rust gates on the tree and on seeded copies

## 4. The black-box CLI tests
- [ ] 4.1 `release_examples`
- [ ] 4.2 `line_breaks`
- [ ] 4.3 `short_variants` (the brief's `shorter_forms`)
- [ ] 4.4 Compared with the Rust tests on the tree and under every catalogued mutation they kill
- [ ] 4.5 The remaining black-box files, `common`/`differential`-only first

## 5. The mutation runner's core
- [ ] 5.1 Read the catalogue; apply exactly once; restore byte for byte; journal
- [ ] 5.2 The verdict from the killers' results
- [ ] 5.3 Compared with the Rust runner's `--plan` and apply gate

## 6. Later (not this track's first pass)
- [ ] 6.1 The gates that start `cargo metadata` (layering, reach, unsafe policy)
- [ ] 6.2 `bench`, `fuzz`
- [ ] 6.3 The contained runner — when the coordinator moves it
