## Why

The cross-language comparison (`docs/performance.md`, *R1, 5b.10*) puts Nazm first or tied on start-up,
memory and binary size for small programs and among the shortest compiled programs, but not first on
build time, on `fib` and word-frequency run time, on the word-frequency program's memory and length,
or on the interpreter's speed. The copyright holder asked (2026-10-14) that Nazm be the best choice in
every comparison it can honestly win, as a milestone after the 0.10.0 publication.

## What Changes

- **Build time** — a precompiled runtime object per target and cached standard-library objects;
  the comparison reports cached and uncached builds.
- **`nazm run` compiles with Cranelift and executes**; the interpreter stays as `--interpret` and as
  the oracle.
- **Word-frequency speed and memory** — a size-class allocator in the runtime, a native string-hash
  built-in with the same values (map order unchanged), a `split`/`words` iterator that allocates
  nothing, whole-program optimisation of the runtime with the program, and a streaming
  `file_lines` iterator.
- **Smaller binaries** — dead-stripping and unused standard functions left out.
- **Fewer lines** — a prelude that imports the common standard modules, `top(n)`/`sort_by(key)` on
  maps, and expression-bodied functions (`fn f(n: Int) -> Int = …`).
- **Syntax that is short and conventional** — result types with `:` (`fn fib(n: Int): Int`), inferred
  for a private one-liner (`fn add(a: Int, b: Int) = a + b`); `[…]` a list by default; `{}` map
  literals and `Map[K, V]`; `|a, b| a < b` closures; positional enum payloads and unprefixed variants;
  `use std.json`; `List[Int]`; printing without passing `out` in your own program's functions.
- **An honest table** — safety-equivalent rows (Rust with overflow checks, Zig ReleaseSafe, C with
  sanitizers), a safety column, and the comparison re-run.

## Capabilities

### New Capabilities
- `performance-and-dx`: the measured targets above. Archived with `--skip-specs`.

## Impact

Runtime, LIR and both backends, the CLI's `run` and `build`, the standard library, the spec's prelude
and function sections, `bench/compare/` and `docs/performance.md`. Language additions are 1.x
additions; nothing a 0.10.0 program means changes.
