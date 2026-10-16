## 1. Build time
- [x] 1.1 Precompiled runtime object per target, cached standard-library objects (design D12)
      — 2026-10-15: the user cache, architecture §7.113, `tests/user_cache.rs`, three mutants
- [x] 1.2 The comparison reports cached and uncached builds — 2026-10-15: two Nazm rows in
      `compare.py`, `--no-cache` and `(user cache warm)`; measured in `performance.md`, *R1.5 1.2*

## 2. Running
- [x] 2.1 `nazm run` compiles with Cranelift and executes; `--interpret` keeps the interpreter
      (2026-10-15: `crates/nazm-cli/src/compiled_run.rs`, tests `run_compiled.rs`; fib 5.24 s → 0.17 s)

## 3. Word frequency: speed and memory
- [x] 3.1 Size-class allocator in the runtime — 2026-10-16: the small-block heap (`heap::emit_small`,
      architecture §7.119, runtime ABI 39) for a hosted executable that calls no C; a string-and-list
      loop 284.7 → 115.4 ms (2.5×), four tasks −16%, words unchanged (no allocation in its loop);
      `tests/small_heap.rs`, four mutants
- [ ] 3.2 Native string-hash built-in with the same values — 2026-10-16, measured first: at `-O2`
      `@std/hash`'s `hash_bytes` already compiles to a five-instruction loop with no checks (load,
      xor, multiply, count, branch) and the finaliser inlined — what a native built-in of the same
      values would be; its cost was the calls between units, which 3.4 removes. No built-in added
      (it would be a checker change, epoch); left open for the copyright holder to close or keep
- [x] 3.3 Allocation-free `split`/`words` iterator — 2026-10-15: `for w in s.split(sep)` lowered
      through `text_split_end`/`text_split_next` (Core IR, `split_loop`), no list; word frequency
      189 → 151 ms (−20%), one test, two mutants; `words` stays a list
- [x] 3.4 Whole-program optimisation of runtime and program — 2026-10-16: ThinLTO for `-O2` LLVM
      builds on macOS, the link's cache in `.nazm/lto` (architecture §7.120); words 114.1 → 103.6 ms
      (−9.2%), the allocation loop −29%, cold builds 7–13% faster, a warm rebuild of words +5.7%;
      `tests/whole_program.rs`, four mutants; Windows keeps per-unit optimisation (MinGW reads no
      bitcode)
- [ ] 3.5 Streaming `file_lines`

## 4. Size and lines
- [x] 4.1 Dead-stripping; unused standard functions left out (design D14) — 2026-10-15:
      architecture §7.114, `tests/dead_strip.rs`, sizes in `performance.md`, two mutants
- [x] 4.2 Prelude imports of the common standard modules — 2026-10-15 (D9): loader
      (`prelude_named`), implicit imports yield to the module's own names, spec *Shorter forms*,
      epoch 85, four tests, four mutants
- [x] 4.3 `top(n)` and `sort_by(key)` on maps — 2026-10-15 (D10): `hashmap_top`, `hashmap_sort_by`
      (over `(key, value)` pairs), API-1.0, a test, a mutant
- [x] 4.4 Expression-bodied functions: `fn f(n: Int): Int = …`, and a private one with no result type
      written, `fn add(a: Int, b: Int) = a + b` (inferred; a `pub` function still writes its type)
      — 2026-10-15 (D2): parser, checker (`check/infer.rs`), `N0634`, spec *Shorter forms*, epoch 77

## 4b. Found writing the multicore benchmark (2026-10-15)
- [x] 4.5 `@std/option` and `@std/result` join D9's prelude modules, so `r.unwrap_or(1)` needs no
      `use` (`result_unwrap_or` exists; the method was refused for want of the import)
      — 2026-10-15: `PRELUDE` gains both (no name either shares with the other five), and a method
      in an interpolation's hole names its module (D9 had missed it); epoch 86, two tests, three
      mutants
- [x] 4.6 A line break separates `match` arms as it separates statements (5b.19): today an arm
      without its `,` before the next on a new line is a syntax error (`N0100`)
      — 2026-10-15: parser (`match_expr`), grammar prose and example, spec *Shorter forms*, guide,
      epoch 87, two tests, two mutants; `nazm fmt` unchanged (it keeps the tokens written)

## 6. Syntax: short and conventional (the copyright holder's choices, 2026-10-14)
- [x] 6.1 Result types written with `:` like parameters — `fn fib(n: Int): Int`, function types
      `fn(Int): Int`; `->` still read, removed at 1.0; `nazm fmt` keeps the form written until R2
      (design D1) — 2026-10-15: parser, grammar, spec *Shorter forms*, epoch 76
- [x] 6.2 `[1, 2, 3]` is a list by default; a fixed array only where its type says `[Int; 3]`
      — 2026-10-15 (D6): checker (`check_array_literal`), `N0618` for an untyped `[]`, epoch 84,
      tests and corpus migrated by meaning, a breaking change listed; `nazm fix`'s annotation open
- [ ] 6.3 Map literals `{}` and `{"a": 1}`, and the name `Map[K, V]` — 2026-10-15 (D5): `{"a": 1}` and
      `Map[K, V]` done (epoch 80), and with D9 they need no `use`; the empty `{}` is still an empty
      block — an empty map is `hashmap_new()` — until a typed `let m: Map[K, V] = {}` is settled
- [x] 6.4 Short closures `|a, b| a < b`, parameter and result types inferred — 2026-10-15 (D3):
      parser `short_closure`, types from the expected function type, `N0635`, epoch 78
- [x] 6.5 Positional enum payloads `Circle(2)` and patterns `Circle(r)`; the enum prefix only where
      a variant name is ambiguous
      — 2026-10-15 (D4): records too (`Point(1, 2)`), bare unit variants (`Dot`), `N0636`, epoch 79
- [x] 6.6 `use std.json` without quotes or `@`; `List[Int]` taught for `Ints`/`Strs` (the old names
      keep working)
      — 2026-10-15 (D7): `use std.json`, epoch 81 (dc2e0974); `List[T]` is `Vec[T]`, R1a's sequence
      `Chain[T]` with `chain_*`, `N0637` for a `list_` built-in, epoch 83
- [x] 6.7 Printing in your own program: its functions may print without being passed `out: OutCap`;
      a library or package function still needs it passed, and files, the network and processes stay
      declared in `main`
      — 2026-10-15 (D8): the authority check holds `OutCap` in the program's own modules (not `@std`,
      the prelude, or a package); epoch 82

## 5. The comparison
- [ ] 5.1 Safety-equivalent rows and a safety column; re-run and publish

## 7. Security (the copyright holder's choices, 2026-10-14)
- [x] 7.1 `nazm build --release`: symbols and debug information stripped, PIE, stack protection,
      hardened-runtime signing on macOS — 2026-10-15 (D15): `build/release.rs`, architecture
      §7.117, `tests/release_build.rs` (a test per property), four mutants; macOS and the LLVM
      backend only, the rest refused by name
- [ ] 7.2 Every release archive signed (minisign or Sigstore) and checked reproducible in CI — 2026-10-15:
      Sigstore chosen; `scripts/sign-release.sh` and `scripts/verify-release.sh` written (D16); the
      first real signature is at 1.0, and the reproducibility check in CI is still to do

## 8. Faster verification (the copyright holder's choices, 2026-10-14)
- [ ] 8.1 A faster linker (mold or lld) for the contained image's Rust builds; measured against the
      current image on the build, suite and mutation cells of `docs/performance.md`
- [ ] 8.2 An eight-CPU profile calibrated like P4T4: approved by the copyright holder, and adopted only
      where it stays inside a memory ceiling the calibration justifies (P6 crossed 4 GiB, 2026-09)
- [ ] 8.3 Mutants scheduled by crate, leaf crates together, so consecutive mutants rebuild less
- [ ] 8.4 The baseline suite a contained mutate session runs before its suite tier is kept across
      `--resume`, so a mutant its killers miss costs one suite run, not a whole session per resume
      (the R1 close lesson: such a mutant used up a 50-minute session and repeated on each resume)
