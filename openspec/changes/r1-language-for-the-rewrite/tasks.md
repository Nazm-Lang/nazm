## 1. Approval

- [ ] 1.1 The copyright holder approves, amends or rejects D1–D5 (and D6's deferral)

## 2. Specification first (`CONTRIBUTING.md`, *The constitution comes before the code*)

- [ ] 2.1 `docs/spec.md`: `Box`/`List` (a new value kind in *The memory constitution*), the
      ownership rule and the reclamation argument in *Generics* and *Cycles*, the `N0616` scope
- [ ] 2.2 `docs/spec.md`: the cost-table row for `s = str_concat(s, t)`
- [ ] 2.3 `docs/spec.md` and `docs/grammar.ebnf`: `for` and `a..b`
- [ ] 2.4 `docs/spec.md`: the library additions; `os_spawn_in`, `os_stdout`/`os_stderr`, `os_lock`
- [ ] 2.5 `docs/spec.md`: `--heap-limit` and its run-time failure
- [ ] 2.6 `docs/stability.md` (epoch entries), `docs/diagnostics.md` (any new code)

## 3. The Rust reference, one commit per decision

- [ ] 3.1 D3 `for` — parser, checker, interpreter, lowering; tests across the interpreter, LLVM `-O0`/`-O2`, Cranelift
- [ ] 3.2 D1 `Box`/`List` — checker (ownership graph), interpreter, MIR/LIR layout and release (iterative), runtime; memory-report tests
- [ ] 3.3 D2 in-place append — lowering and runtime; sharing-shape tests and a timing test
- [ ] 3.4 D4 platform built-ins — runtime ABI bump, interpreter, both backends; Wine, WASI and board refusals stated
- [ ] 3.5 D4 library modules in Nazm — `@std/blake3`, `@std/sha256` (official vectors), `@std/toml`, `file_walk`
- [ ] 3.6 D5 heap limit — runtime counter, the flag and manifest key, the failure; tests with and without it

## 4. Evidence

- [ ] 4.1 Compat cases in `tests/compat/v1/` for each acceptance change, pinned
- [ ] 4.2 Mutants mapped to each new responsibility; contained run, every one caught
- [ ] 4.3 Host suite, contained tests, selfhost and bootstrap on the resulting tree
- [ ] 4.4 Capability matrix, limitations and architecture updated; `openspec validate --strict`; archive with `--skip-specs`
