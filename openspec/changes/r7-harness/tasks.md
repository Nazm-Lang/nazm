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
- [x] 3.6 Doc claims, capability matrix, dependency sources, performance claims, agent governance;
      mutations apply, both halves: exactly once (`mutate/core.nz`) and everything `plan::validate`
      checks without a build (`mutate/plan.nz`: the schema, killers resolved through files, inline
      modules and doc comments, ignored killers, profiles, `no_killer`), run as the gate in
      `nazm-check` and by `nazm-mutate check`. `tests/plan_self.nz` ports `plan.rs`' tests (5)
- [x] 3.7 Compared with the Rust gates on the tree and on seeded copies (`tests/seeded.sh`, every
      gate ported so far; extended with each gate). With 3.6, 5.3 and 6.1: sixteen gates, and
      `--plan`, on the pristine copy, 53 seeds, 17 files removed one at a time, two other defects
      and the copy restored — 74 agree, 0 differ

## 4. The black-box CLI tests
- [x] 4.1 `release_examples`
- [x] 4.2 `line_breaks`
- [x] 4.3 `short_variants` (the brief's `shorter_forms`)
- [x] 4.4 Compared with the Rust tests on the tree and under every catalogued mutation they kill
      (`tests/mutants.sh`: 18 mutations, each caught by the Rust killer and the Nazm test)
- [x] 4.5 The remaining black-box files — 175 of the 176 test files of `crates/nazm-cli/tests/`
      (the directory's 194 entries less the five `cli_*` roots, `selfhost` and twelve fixture
      directories); the 176th, `fuzz_replay`, has no test that is not a Rust API's, and all four of
      its tests are listed as staying Rust. Every ported test agrees with its Rust test on the tree
      (`tests/run.sh`, each commit's numbers). The coordinator's decisions on the open questions:
      - the tests that reach a Rust API stay Rust, each named in `tests/stays-rust.txt` with its
        reason (64: a table the CLI does not print 5, LLVM text the CLI does not write 6, a library
        entry 10, the compiler's own stages R2–R3 replace 43); `run.sh` reports them as
        "stays Rust (N)", never as agreeing, and fails on an entry that is ported or gone. Where the
        CLI does print a table (`capabilities --json`, `nazm inspect`, `N0511`'s help, `docs
        --index`, `interface --hash`), the port reads it there (design, *Findings*). Revisited
        after the cut-over (R8);
      - `distributed` and `secure_service` run their servers as children the harness does not wait
        for (`lib/live.nz`) and talk to them with a `@std/net` client under `NetCap`;
      - `lsp` drives the real `nazm lsp` over its standard input and output (`lib/lsp.nz`, a 120 s
        watchdog); `scheduler_pool`, `select` and `fuzz_diff` (its generator reproducing the Rust
        test's xorshift) are ported on the same library;
      - `#[ignore]`d tests that start a container, need a GPU or open a window stay Rust, each named
        in its file (design decision 10).
- [x] 4.6 Re-run `tests/mutants.sh` over the files ported since the rebase onto `613047d2`: the 45
      of them a catalogued mutation names a killer in, the catalogue's first such mutation each
      (`MUTANTS_PER_FILE=1`; 281 mutation-killer pairs in all, each a rebuild of `nazm` and a test
      binary): 45 agree, 0 differ, every one caught by the Rust killer and the Nazm test alike

## 5. The mutation runner's core
- [x] 5.1 Read the catalogue; apply exactly once; restore byte for byte, in the Rust runner's own
      backup record (`cargo xtask mutate-recover` repairs a tree `nazm-mutate` left mutated)
- [x] 5.2 The verdict from a killer's results, pristine, mutated and restored (`nazm-mutate try`)
- [x] 5.3 Compared with the Rust runner's `--plan` and apply gate: `nazm-mutate plan ROOT [--only F]…`
      prints `cargo xtask mutate --plan`'s plan byte for byte — the whole catalogue (2016 entries,
      8092 lines), `--only` substring, exact (`=`) and repeated filters, a filter that selects
      nothing — and `tests/seeded.sh` compares the plan, or the problems it refuses on, under every
      seed. A schema violation (an unknown or missing field) is refused by both; the `toml` crate's
      error text, with its source excerpt, is not reproduced (design, decision 20)

## 6. Later (not this track's first pass)
- [x] 6.1 The gates that start `cargo metadata` (`check/reach.nz`): layering, tokenizer reach, codegen
      reach and unsafe policy read `cargo metadata --no-deps --locked`, run in the root as a child;
      network reach and emission reach, the other two reach gates, read files. The rules are
      `xtask/src/rules.rs`, read from the tree being checked (decision 21)
- [ ] 6.2 `bench`, `fuzz` — not ported in this pass (design, decision 22): every mode of `bench`
      measures and runs only under the contained runner's ceiling, and `fuzz` is the compiler's
      in-process entry points under SanitizerCoverage, a Rust API, run only by
      `cargo xtask contained fuzz` (6.3)
- [ ] 6.3 The contained runner — when the coordinator moves it
