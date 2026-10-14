## 1. The proposal

- [x] 1.1 The data model (Core IR in its JSON arena, types as spellings, MIR as enums) and the order
      of work (`design.md`)
- [x] 1.2 The gaps in `nazm.core-ir/2`, and the request for `nazm.core-ir/3`

## 2. Reading Core IR

- [ ] 2.1 `cir.nz`: the document, function headers, locals, constants, typed accessors
- [ ] 2.2 `types.nz`: spellings parsed, MIR keys, substitution, the type table recovered from uses

## 3. MIR for integers, calls, branches and loops

- [ ] 3.1 The lowering of §7.42 for `Int`/`Bool` functions; the stack check for call cycles
- [ ] 3.2 Text, printed form and BLAKE3 digest identical; `fib` and the arithmetic corpus agree
- [ ] 3.3 `tests/run.sh`: build, run over every input, compare, one-line summary

## 4. Ownership

- [ ] 4.1 Strings, sequences, records, enums: copies, moves, drops, field borrows
- [ ] 4.2 `match`, `?`, `return`/`break`/`continue` cleanup, scopes, spawn, failure edges
- [ ] 4.3 Generic instances (the planner, sorted by identity)

## 5. The MIR validator

- [ ] 5.1 Structure and typing
- [ ] 5.2 The ownership dataflow

## 6. LIR

- [ ] 6.1 Data layout, units, symbols, linkage
- [ ] 6.2 Record and enum layouts
- [ ] 6.3 Instructions (`--ops`)
