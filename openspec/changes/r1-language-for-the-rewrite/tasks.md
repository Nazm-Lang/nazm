## 1. Approval

- [x] 1.1 The copyright holder approved D1–D5 and D8's list (L1–L14, P1–P3, S1–S5), 2026-10-11

## 2. How every item is carried out

Each item is its own commit series (`CONTRIBUTING.md`, *The constitution comes before the code*):
spec text first (`docs/spec.md`, `docs/grammar.ebnf`, `docs/stability.md` epoch entry,
`docs/diagnostics.md` for a new code); then the parser, checker, interpreter and both native
backends; tests across the interpreter, LLVM `-O0`/`-O2` and Cranelift with the memory report
compared; mutants mapped to the new responsibility; host suite before each
commit; the capability matrix and limitations updated. The Nazm compiler (`compiler/*.nz`) refuses
each new construct by name until R2 carries it.

## 3. R1a — the core language

- [x] 3.1 D3 `for x in xs` and `for i in a..b` (EXPERIMENTAL until 8.1)
- [x] 3.2 L1 `Box[T]`, `List[T]` and the ownership rule (D1); release by worklist (runtime ABI 29)
- [x] 3.3 L3 patterns: `_`, or-patterns, nested and literal patterns, `if let`, `let … else` (the covering warning with L15)
- [x] 3.4 L5 tuples
- [x] 3.5 L6 derived `Hash` and `Ord`
- [x] 3.6 L8 `pub const` and constant arrays of `Str` and records
- [x] 3.7 L9 multi-line and raw string literals
- [x] 3.8 L4 string interpolation and float formatting

## 4. R1b — functions, traits, arithmetic

- [ ] 4.1 L7 `Cell[T]`
- [ ] 4.2 L10 closures in generic functions; non-escaping `let mut` capture
- [ ] 4.3 L11 impls for generic types; default methods
- [ ] 4.4 L12 `defer`
- [ ] 4.5 L13 checked arithmetic, widening multiply, `Int128`/`UInt128`
- [ ] 4.6 L14 stack size
- [ ] 4.8 L15 a warning severity, `--deny-warnings`; L3's covering warning on `_`
- [ ] 4.7 D2 linear `s = str_concat(s, t)`

## 5. R1c — serialisation

- [ ] 5.1 S1 `@std/toml`
- [ ] 5.2 L2 `@derive(json)` and `@derive(toml)`, with JSON Schema output

## 6. R1d — the platform

- [ ] 6.1 D5 `--heap-limit`
- [ ] 6.2 D4 built-ins: `os_spawn_in`, `os_stdout`/`os_stderr`, `os_lock`/`os_unlock`
- [ ] 6.3 P1 process control; P2 filesystem; P3 streaming standard input

## 7. R1e — libraries in Nazm

- [ ] 7.1 S1 `@std/blake3`, `@std/sha256` (official vectors), `file_walk`
- [ ] 7.2 S2 `ByteBuf`; S3 `@std/ordmap`, `@std/bitset`, in-place sort
- [ ] 7.3 S4 `@std/args`; S5 `@std/regex`

## 8. Close

- [ ] 8.1 Promote each R1 section from EXPERIMENTAL to stable, and add its pinned compat case to
      `tests/compat/v1/` then (the corpus holds stable surfaces only)
- [ ] 8.2 Contained tests, selfhost and bootstrap on the resulting tree; every new mutant caught
- [ ] 8.3 Authorities updated; `openspec validate --strict`; archive with `--skip-specs`
