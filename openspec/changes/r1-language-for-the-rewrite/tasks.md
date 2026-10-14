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

- [x] 4.1 L7 `Cell[T]`
- [x] 4.2 L10 closures in generic functions; `let mut` capture (shared, see design.md)
- [x] 4.3 L11 impls for generic types; default methods (L11a default methods, L11b impls of generic types, 2026-10-13)
- [x] 4.4 L12 `defer`
- [ ] 4.9 Open finding (2026-10-13, found measuring L13b's stack): on a **debug** build a call
  nested in an expression in the recursive position (`1 + f(g(down(n - 1)))`) costs several
  evaluator frames per Nazm call, so it overflows the host stack at about 361 calls, below the
  default `--max-depth` of 512, where it should report `N0403`. Present before L13b (`ee7ce87`
  measures the same); a release build carries 15,000. The budget must bound the frames per Nazm
  call, or the evaluator must not recurse per nested call. Fix before R1 closes.
- [x] 4.5 L13 checked arithmetic, widening multiply, `Int128`/`UInt128` — checked arithmetic (L13a), `Int128`/`UInt128` and `widening_mul` (L13b), 2026-10-13
- [x] 4.6 L14 stack size (`nazm build --stack-size`; the interpreter keeps `--max-depth`)
- [x] 4.8 L15 a warning severity, `--deny-warnings`; L3's covering warning on `_` (`nazm check`, `nazm build`, the service; `nazm run`/`test` and a `@deprecated` use remain)
- [x] 4.7 D2 linear `s = str_concat(s, t)` (native and interpreter; only storage an append made grows)

## 5. R1c — serialisation

- [x] 5.1 S1 `@std/toml` (2026-10-13; `toml-test` 1.0, 553 of 553, LLVM and Cranelift)
- [x] 5.2 L2 `@derive(json)` and `@derive(toml)`, with JSON Schema output — L2a `@derive(json)` with its codec and schema, L2b `@codec(…)`, L2c `@derive(toml)`, 2026-10-13

## 5b. Developer experience and speed (the copyright holder's decision, 2026-10-13)

Measured against Go, Python, C, C++ and Rust on the same programs; the targets are minimums. Option
C for `main`'s authority. Published with the rest of R1 as 0.10.0.

- [x] 5b.1 Option C: `OutCap` implicit in `main`; every other authority still written; `nazm build` and
      `nazm inspect` report the program's authority (2026-10-14)
- [x] 5b.2 A function with no `-> T` returns nothing
- [x] 5b.3 `main`'s `Int` result is the exit status, not printed
- [x] 5b.4 `println`
- [x] 5b.5 Effects inferred for a function that is not `pub` — the rule since N36 (`spec.md`, *Declared and
      inferred*), found to need no change; pinned by `main_ends::a_private_function_declares_no_effects`
- [x] 5b.6 `s + t` on strings
- [x] 5b.7 `m[k]`, `m[k] = v` and `m[k] += 1` on maps
- [ ] 5b.8 `fib(35)` ≤ 60 ms (141.6 before; C 37, Go 41) — the stack check inlined, and a program of
      one thread, known from `main`'s authority, keeping its failure state and stack limit in plain
      globals: 68.3 ms
- [ ] 5b.9 Word frequency over 2,000,000 words ≤ 200 ms (453 before; Go 127, C 123): integer text
      without `snprintf`, fewer allocations, the hash map's probe, retains and releases — 392 ms,
      then 222 ms (integer text, the map's table of positions, one string per interpolated integer,
      inlined reference counting)
- [ ] 5b.10 The benchmark in the repository, re-run for each publication, and its results on nazm.dev
- [x] 5b.11 `else if` — the grammar and the guide have promised it since 2026-09-21 and the parser
      never accepted it; a grammar `@example` of it, so the two cannot drift again
- [x] 5b.12 Compound assignment: `+=`, `-=`, `*=`, `/=`, `%=` and the bit operators'
- [x] 5b.13 `print` and `println` of any value with a text (a number, a `Bool`, a `Show`)
- [x] 5b.14 `Some(x)`, `None`, `Ok(x)`, `Err(e)` where the type is known; a variant with no payload
      without `()` (`C.R`)
- [x] 5b.15 `v[i]` and `v[i] = x` on `Vec`; methods on built-in types (`v.len()`, `s.len()`,
      `s.split(" ")`, …) as names for the existing functions
- [x] 5b.16 `let x: T = …`; `[1, 2, 3]` as a `Vec` where one is wanted; `vec_new()`'s `T` inferred
      from later use
      — `vec_new()`'s `T` settled by the type its result is wanted as (2026-10-14)
- [x] 5b.17 Methods without a trait (`impl Point { … }`), `while let`, `loop`, match guards, `?` on
      `Option` in a function returning `Option`, `/* … */` comments
      — done: an impl without a trait, `while let`, `loop`, block comments (2026-10-14); `?` on `Option`; match guards
- [ ] 5b.18 One list type: `Strs` and `Ints` interchangeable with `Vec[Str]` and `Vec[Int]`
- [x] 5b.19 Semicolon-free lines: a line break ends a statement where a `;` could, as in Go; `;`
      still accepted; `nazm fmt` writes the new form

## 6. R1d — the platform

- [ ] 6.1 D5 `--heap-limit`
- [ ] 6.2 D4 built-ins: `os_spawn_in`, `os_stdout`/`os_stderr`, `os_lock`/`os_unlock`
- [ ] 6.3 P1 process control; P2 filesystem; P3 streaming standard input

## 7. R1e — libraries in Nazm

- [x] 7.1 S1 `@std/blake3`, `@std/sha256` (official vectors), `file_walk`
- [x] 7.2 S2 `ByteBuf`; S3 `@std/ordmap`, `@std/bitset`, in-place sort
- [x] 7.3 S4 `@std/args`; S5 `@std/regex`

## 8. Close

- [ ] 8.1 Promote each R1 section from EXPERIMENTAL to stable, and add its pinned compat case to
      `tests/compat/v1/` then (the corpus holds stable surfaces only)
- [ ] 8.2 Contained tests, selfhost and bootstrap on the resulting tree; every new mutant caught.
      R1a's run was stopped after batch I on 2026-10-12 to go faster, by the copyright holder's
      choice: every mutant run so far was caught (batches A–I), and the rest — the tuple-place,
      ordering and hashing remainder, the arrays of `Str`, the constant tables, the string
      literals and the interpolation (batches J–O of `r1a-*` with `n13-*`, `n36-*`, `n38-*`,
      `n78-*` and `g2-*` repoints) — run here, with R1b–R1e's
- [ ] 8.3 Authorities updated; `openspec validate --strict`; archive with `--skip-specs`
