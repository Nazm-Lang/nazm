## 1. Build time
- [ ] 1.1 Precompiled runtime object per target, cached standard-library objects
- [ ] 1.2 The comparison reports cached and uncached builds

## 2. Running
- [ ] 2.1 `nazm run` compiles with Cranelift and executes; `--interpret` keeps the interpreter

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
- [ ] 4.4 Expression-bodied functions: `fn f(n: Int): Int = …`, and a private one with no result type
      written, `fn add(a: Int, b: Int) = a + b` (inferred; a `pub` function still writes its type)

## 6. Syntax: short and conventional (the copyright holder's choices, 2026-10-14)
- [ ] 6.1 Result types written with `:` like parameters — `fn fib(n: Int): Int`, function types
      `fn(Int): Int`; `->` still read, rewritten by `nazm fmt`, removed at 1.0
- [ ] 6.2 `[1, 2, 3]` is a list by default; a fixed array only where its type says `[Int; 3]`
- [ ] 6.3 Map literals `{}` and `{"a": 1}`, and the name `Map[K, V]`
- [ ] 6.4 Short closures `|a, b| a < b`, parameter and result types inferred
- [ ] 6.5 Positional enum payloads `Circle(2)` and patterns `Circle(r)`; the enum prefix only where
      a variant name is ambiguous
- [ ] 6.6 `use std.json` without quotes or `@`; `List[Int]` taught for `Ints`/`Strs` (the old names
      keep working)
- [ ] 6.7 Printing in your own program: its functions may print without being passed `out: OutCap`;
      a library or package function still needs it passed, and files, the network and processes stay
      declared in `main`

## 5. The comparison
- [ ] 5.1 Safety-equivalent rows and a safety column; re-run and publish
