## 1. The proposal

- [x] 1.1 The data model (Core IR in its JSON arena, types as spellings, MIR as enums) and the order
      of work (`design.md`)
- [x] 1.2 The gaps in `nazm.core-ir/2`, and the request for `nazm.core-ir/3`

## 2. Reading Core IR

- [x] 2.1 `cir.nz`: the document, function headers, locals, constants, typed accessors
- [x] 2.2 `types.nz`: spellings parsed, MIR keys, substitution, the type table recovered from uses

## 3. MIR for integers, calls, branches and loops

- [x] 3.1 The lowering of §7.42 for `Int`/`Bool` functions; the stack check for call cycles
- [x] 3.2 Text, printed form and BLAKE3 digest identical; `fib` and the arithmetic corpus agree
- [x] 3.3 `tests/run.sh`: build, run over every input, compare, one-line summary

## 4. Ownership

- [x] 4.1 Strings, sequences, records, enums: copies, moves, drops, field borrows
- [x] 4.2 `match`, `?`, `return`/`break`/`continue` cleanup, scopes, spawn, failure edges
- [x] 4.3 Generic instances (the planner, sorted by identity), trait calls resolved by the impl
      functions' names, derived `Hash`
- [x] 4.4 Closures (captures recovered as the locals after the parameters) and function-value
      thunks (synthesised into the document, as Rust synthesises their Core IR)
- [x] 4.5 Array types recovered across calls (`refine.nz`); the instance plan bounded (`N0358`)
- [x] 4.6 Comparison: 103 of the brief's 105 inputs identical to `nazm mir`, and all 83 of
      `compiler/` with MIR and this pipeline itself (its own ~500 functions, `@std` included): 187
      agree, 2 differ. The 2 are the Core IR /2 `@export` gap (`desktop/counter`,
      `check/export-numbers`): the digest's canonical text names the C symbol

## 5. The MIR validator

- [x] 5.1 Structure and typing — all of Rust's checks but a primitive's argument and result types
      (the built-ins' signature table is the checker's; to be shared with R2) and the signatures of
      function values and arrays (spelled `?` in Core IR /2)
- [x] 5.2 The ownership dataflow, as Rust's: empty, owned or either per tracked local, joined by
      union, source blocks first
- [x] 5.3 `pipeline mutants`: five kinds of breakage (a drop removed from a source block, a
      failure edge removed, a jump past the last block, a return of another type, a parameter
      moved), the first place each fits in every function: 11941 made over the inputs, all caught;
      `pipeline mir` prints only what the validator accepted, as `nazm mir` does

## 6. LIR

- [ ] 6.1 Data layout, units, symbols, linkage
- [ ] 6.2 Record and enum layouts
- [ ] 6.3 Instructions (`--ops`)
