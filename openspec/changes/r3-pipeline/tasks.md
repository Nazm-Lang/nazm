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
- [x] 4.5 Comparison: 102 of 105 inputs identical to `nazm mir`; the 3 that differ are Core IR /2
      gaps (`@export` symbols in 2, array parameter types in 1)

## 5. The MIR validator

- [ ] 5.1 Structure and typing
- [ ] 5.2 The ownership dataflow

## 6. LIR

- [ ] 6.1 Data layout, units, symbols, linkage
- [ ] 6.2 Record and enum layouts
- [ ] 6.3 Instructions (`--ops`)
