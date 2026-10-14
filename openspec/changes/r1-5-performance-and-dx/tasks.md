## 1. Build time
- [ ] 1.1 Precompiled runtime object per target, cached standard-library objects
- [ ] 1.2 The comparison reports cached and uncached builds

## 2. Running
- [x] 2.1 `nazm run` compiles with Cranelift and executes; `--interpret` keeps the interpreter
      (2026-10-15: `crates/nazm-cli/src/compiled_run.rs`, tests `run_compiled.rs`; fib 5.24 s → 0.17 s)

## 3. Word frequency: speed and memory
- [ ] 3.1 Size-class allocator in the runtime
- [ ] 3.2 Native string-hash built-in with the same values
- [ ] 3.3 Allocation-free `split`/`words` iterator
- [ ] 3.4 Whole-program optimisation of runtime and program
- [ ] 3.5 Streaming `file_lines`

## 4. Size and lines
- [ ] 4.1 Dead-stripping; unused standard functions left out
- [ ] 4.2 Prelude imports of the common standard modules
- [ ] 4.3 `top(n)` and `sort_by(key)` on maps
- [x] 4.4 Expression-bodied functions: `fn f(n: Int): Int = …`, and a private one with no result type
      written, `fn add(a: Int, b: Int) = a + b` (inferred; a `pub` function still writes its type)
      — 2026-10-15 (D2): parser, checker (`check/infer.rs`), `N0634`, spec *Shorter forms*, epoch 77

## 6. Syntax: short and conventional (the copyright holder's choices, 2026-10-14)
- [x] 6.1 Result types written with `:` like parameters — `fn fib(n: Int): Int`, function types
      `fn(Int): Int`; `->` still read, removed at 1.0; `nazm fmt` keeps the form written until R2
      (design D1) — 2026-10-15: parser, grammar, spec *Shorter forms*, epoch 76
- [ ] 6.2 `[1, 2, 3]` is a list by default; a fixed array only where its type says `[Int; 3]`
- [ ] 6.3 Map literals `{}` and `{"a": 1}`, and the name `Map[K, V]`
- [x] 6.4 Short closures `|a, b| a < b`, parameter and result types inferred — 2026-10-15 (D3):
      parser `short_closure`, types from the expected function type, `N0635`, epoch 78
- [ ] 6.5 Positional enum payloads `Circle(2)` and patterns `Circle(r)`; the enum prefix only where
      a variant name is ambiguous
- [ ] 6.6 `use std.json` without quotes or `@`; `List[Int]` taught for `Ints`/`Strs` (the old names
      keep working)
- [ ] 6.7 Printing in your own program: its functions may print without being passed `out: OutCap`;
      a library or package function still needs it passed, and files, the network and processes stay
      declared in `main`

## 5. The comparison
- [ ] 5.1 Safety-equivalent rows and a safety column; re-run and publish

## 7. Security (the copyright holder's choices, 2026-10-14)
- [ ] 7.1 `nazm build --release`: symbols and debug information stripped, PIE, stack protection,
      hardened-runtime signing on macOS
- [ ] 7.2 Every release archive signed (minisign or Sigstore) and checked reproducible in CI

## 8. Faster verification (the copyright holder's choices, 2026-10-14)
- [ ] 8.1 A faster linker (mold or lld) for the contained image's Rust builds; measured against the
      current image on the build, suite and mutation cells of `docs/performance.md`
- [ ] 8.2 An eight-CPU profile calibrated like P4T4: approved by the copyright holder, and adopted only
      where it stays inside a memory ceiling the calibration justifies (P6 crossed 4 GiB, 2026-09)
- [ ] 8.3 Mutants scheduled by crate, leaf crates together, so consecutive mutants rebuild less
- [ ] 8.4 The baseline suite a contained mutate session runs before its suite tier is kept across
      `--resume`, so a mutant its killers miss costs one suite run, not a whole session per resume
      (the R1 close lesson: such a mutant used up a 50-minute session and repeated on each resume)
