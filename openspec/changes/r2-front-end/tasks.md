## 1. The proposal and the harness

- [x] 1.1 The missing constructs in dependency order (`proposal.md`) and the decisions (`design.md`)
- [x] 1.2 The inputs: `toolchain/frontend-tests/extract-inputs.py` → `toolchain/frontend-tests/inputs/rs/`
- [x] 1.3 The reference dumper `toolchain/frontend-tests/refdump/` (tokens, tree)
- [x] 1.4 `toolchain/frontend-tests/run.sh`: builds the drivers with the reference under the heap limit, compares
      every input, prints `N agree, M differ` per comparison
- [x] 1.5 `toolchain/frontend-tests/host-check.sh`: the reference checks the compiler and the drivers (D6; no
      stage runs on the host)

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

- [ ] 3.1 Modules: `as`, `::`, re-exports, packages and manifests — the loader is done (`load.nz`: the
      root, its imports depth-first, `@std/`, the core prelude, global offsets, every loader
      diagnostic but a package's); packages, derives and the module graph's semantics are not
- [x] 3.1a The `check` comparison (`run.sh check`): loader, every file's syntax diagnostics at their
      global offsets, and the missing `main` — every difference left is a checker diagnostic
- [ ] 3.1b The checker's first stages (`compiler/sema.nz`, D13): one module and the prelude; type
      names, fields, function declarations, bodies of the core subset, the `main` rules; the built-in
      table generated (D14). `run.sh check`: 1,908 agree before, 2,112 after; the checker carries
      922 inputs and all 922 agree (the loader stops at a package, which it does not resolve)
- [ ] 3.1c Effects and authority (`effects::analyse`): declared sets (N0366, N0367, N0368), each body's
      authority (N0369, R1.5 D8's output authority held), the generated table's sinks; a call that
      writes to a path stops the checker until provenance (N38) is carried, and so does a module of a
      package. `run.sh check`: 2,159 agree, 577 differ; the checker carries 1,247, all agree
- [ ] 3.1d Generics (`check_generic_call`, D15): type parameters of functions, records and enums
      (N0331, N0350, N0101, N0365), `Equality` requirements (N0364, N0304's help), calls by
      written or inferred type arguments (N0353, N0354, N0355, N0351), the generic built-ins by
      class, generic enums' arguments inferred — the prelude's `Some`/`None`/`Ok`/`Err` — and
      constructions with written arguments (N0352), `main` generic (N0356), the nesting bound
      (N0338), `List[T]`; `Vec`s and applications of any type. `run.sh check`: 2,238 agree, 498
      differ; the checker carries 1,436, all agree
- [ ] 3.1e `match` and patterns (`check_match`, `usefulness`): every pattern form but a tuple's
      (N0342, N0345, N0341, N0333, N0332, N0334, N0629, N0300, N0202), guards, `if let`,
      `while let`, `let … else` (N0630) and `let` of a pattern, arm types (N0302), reachability
      (N0344) and exhaustiveness with the reference's witness (N0343), and N0632's warning,
      published only where there is no error. `run.sh check`: 2,307 agree, 429 differ; the
      checker carries 1,598, all agree
- [ ] 3.1f `?` (`check_propagate`): the prelude's `Result` and `Option` by definition (N0360,
      N0361, N0362, N0300). `run.sh check`: 2,342 agree, 394 differ; the checker carries 1,648,
      all agree
- [ ] 3.1g Imports and module graphs (D16): every stage over every module in load order, each
      module against its direct imports' interfaces — `pub` visibility, plain imports and imports
      under a name (`alias::f`, `alias::T`, qualified constructions, variants and patterns), their
      collisions (N0207, N0208, N0209), private and unimported names (N0206, N0201 and N0305 with
      where the name is), unknown qualified names (N0210), another module's undeclared effects
      assumed, a standard module's authority; re-exports stop. Hand-written cases in
      `toolchain/frontend-tests/inputs/modules/`. `run.sh check`: 2,369 agree, 392 differ; the
      checker carries 1,704, all agree
- [ ] 3.1h Interpolation (`check_hole`): `f"…"` and what `print(x)` is written as — a precision only
      on a float (N0006), a `Str`, a number or a `Bool` written as it is, anything else refused
      (N0300: no compilation the checker carries has a trait, so none has a `Show`). `run.sh check`:
      2,375 agree, 388 differ; the checker carries 1,793, all agree
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

- [x] 6.0 Parsed as main's reference parses them (D1–D5): `:` result types, `= e` bodies and
      `inferred_result`, `|a, b|` closures, map literals, fields and payloads by position, variants
      without their enum, `use std.json`
- [ ] 6.1 Checked: R1.5's forms in the checker; D6–D8 once final
