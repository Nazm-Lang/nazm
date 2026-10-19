## Why

The toolchain is to be rewritten in Nazm (`rewrite-in-nazm`). Today's Nazm can express it only at a
cost the existing compiler shows: every tree an index arena of parallel arrays, no recursion, string
building quadratic, and no cryptographic hash, TOML, recursive directory walk or controlled child
environment in the library. This change adds what the rewrite cannot reasonably do without, as
1.x-compatible additions, in the Rust reference first and in the Nazm compiler in R2.

## What Changes

Proposed for the copyright holder's approval (`design.md`, *Decisions*):

- **Immutable indirection** — `Box[T]` and `List[T]`, immutable shared values through which a
  definition may refer to itself, so syntax trees, JSON, TOML and protocol messages are ordinary
  recursive types. Cycles through a mutable handle stay refused (`N0359`).
- **Linear-time string building** — `s = str_concat(s, t)` costs amortised `O(len t)`, a guarantee
  in the cost table; the existing idiom stops being quadratic.
- **`for` loops** — `for x in xs { … }` over `Ints`, `Strs`, `Vec[T]`, `List[T]`, and `for i in a..b`,
  each defined by the `while` loop it stands for.
- **Library** — `@std/blake3` and `@std/sha256` in Nazm, `@std/toml`, `file_walk`, raw writes to
  standard output and errors, a child started in a directory with an environment, and file locks.
- **A bounded heap** — a native program built with `--heap-limit` ends with a run-time failure when
  its live heap would pass the limit, instead of exhausting the machine.

Optional, recommended deferred: closures in generic functions (`N0387` lifted).

## Capabilities

### New Capabilities
- `language-for-the-rewrite`: the observable requirements. Archived with `--skip-specs`; the
  normative text goes into `docs/user/spec.md`, `docs/user/stability.md` and the library reference.

### Modified Capabilities
None.

## Impact

`docs/user/spec.md` (*Generics*, *The memory constitution* and its cost table, *Cycles*, a new *for*
section, the library tables, the process and file sections), `docs/user/grammar.ebnf`, `docs/user/stability.md`,
`docs/user/diagnostics.md`. Code: `nazm-syntax` (`for`, `..`), `nazm-sema` (the ownership graph, `Box`,
`List`), `nazm-core` (the interpreter), `nazm-mir`/`nazm-lir`/`nazm-runtime` (layout, release,
in-place append, the heap counter), `nazm-codegen-clif` (until cut-over), `library/std`.

Versions: the **semantic epoch** moves for each acceptance change (52 onward); the **runtime ABI**
moves for the heap counter, the new platform functions and in-place append; the library API grows.
Every program valid under 1.0 keeps its meaning.
