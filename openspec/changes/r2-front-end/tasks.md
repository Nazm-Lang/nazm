## 1. The proposal and the harness

- [x] 1.1 The missing constructs in dependency order (`proposal.md`) and the decisions (`design.md`)
- [x] 1.2 The inputs: `compiler/tests/extract-inputs.py` → `compiler/tests/inputs/rs/`
- [x] 1.3 The reference dumper `compiler/tests/refdump/` (tokens, tree)
- [x] 1.4 `compiler/tests/run.sh`: builds the drivers with the reference under the heap limit, compares
      every input, prints `N agree, M differ` per comparison
- [x] 1.5 `compiler/tests/host-check.sh`: the reference checks the compiler; C1 emits C2's IR; C1
      compiles and runs the conformance corpus (D6)

## 2. Lexer and parser complete

- [x] 2.1 Every token (group A): escapes, `::`, `..`, bit and compound operators, `@`, radix
      integers, `IntTooLarge`, floats, raw, multi-line and interpolated strings, block comments;
      every lexing error collected — compared on every input (`tokens`)
- [x] 2.2 The tree printer: the arena printed as the Rust parser's `{:#?}` (D3) — `compiler/syntax.nz`,
      `sx_print`; `compiler/tree.nz` is the driver
- [x] 2.3 Declarations (group B) parsed
- [x] 2.4 Types (group C) parsed
- [x] 2.5 Statements (group D) parsed
- [x] 2.6 Expressions and patterns (group E) parsed
- [x] 2.7 Syntax diagnostics and recovery as the reference's — every input's tree agrees (`tree`):
      code, span and message, with the reference's recovery, cascade muting and nesting limit — and
      as whole `nazm.diagnostic/1` objects, labels, help and fixes included (`syntax`;
      `compiler/diag.nz`, `compiler/diagnose.nz`)
- [ ] 2.8 The checker and emitter read `syntax.nz`'s tree; `parse.nz` retired

## 3. Names and types

- [ ] 3.1 Modules: `as`, `::`, re-exports, packages and manifests
- [ ] 3.2 Every type of group C resolved and typed
- [ ] 3.3 Generics, traits, impls, default methods, closures and effects

## 4. The checker's rules

- [ ] 4.1 Group by group, every diagnostic identical as `nazm check --json` (D4), with recovery
- [ ] 4.2 The text rendering compared
- [ ] 4.3 Built-ins of group F; `selfhost.rs`'s `NOT_CARRIED` emptied (asked of the coordinator)

## 5. Core IR

- [ ] 5.1 `nazm.core-ir/2` identical with `nazm core-ir` on every accepted input (D5)
- [ ] 5.2 Per-module output; the performance budgets measured (`rewrite-in-nazm` 3.3)

## 6. R1.5's forms (after the coordinator declares them final)

- [ ] 6.1 `:` result types, `= e` bodies, `|a, b|` closures, map literals, positional payloads
