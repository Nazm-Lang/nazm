## Why

A whole-repository survey (766 tracked source files; `target/gates/m2-survey/`) found the most
changed files are also the most mixed: `nazm-core/src/check.rs` (8,776 lines, ~18 concerns, 54
commits since September), `nazm-core/src/eval.rs` (8 concerns, 40), `nazm-cli/src/main.rs` (75),
`xtask/src/main.rs` (52) and `nazm-cli/src/build.rs` (one 785-line `run`, 36). Gate 2 next grows
exactly those files. Test and library helpers are duplicated: one helper is byte-identical in 21
`nazm-service` test files, a differential harness exists in five copies, six blake3 digest helpers,
four path-relativisers, and a command prologue in 19 CLI modules. The human decided on separation of
concerns rather than a line-count cap, and the reviewer reduced the original 65-file M2 to this core.

## What Changes

- **Deduplication:** a shared `nazm-service` test module, a shared differential harness, shared
  memory-counter helpers, and one shared digest / path / command-prologue / failure-summary helper
  each where copies exist.
- **Splits, moving code only:** `check.rs` → `check/`, `eval.rs` → `eval/`, `build.rs` → `build/`,
  `nazm-cli/src/main.rs` → `cli.rs` + `prepare.rs` + a thin dispatcher, `xtask/src/main.rs` →
  `gates/` + command modules. Every outside caller keeps its path through `pub use`.
- **A module-boundaries gate** in `cargo xtask check`: each module split by M2 opens with a `//!`
  sentence naming its responsibility, and its `mod.rs` holds declarations, re-exports, the façade,
  shared context and orchestration only.
- **The LIR oracle's number rules stay an independent implementation**, documented as a deliberate
  cross-check of `nazm_sema::numeric`.
- No change to Nazm semantics, any schema, diagnostic code, exit status, the runtime ABI or the
  standard library. No epoch bump.

## Capabilities

### New Capabilities

### Modified Capabilities
- `repository-governance`: adds the module-boundaries requirement.

## Impact

`crates/nazm-core`, `crates/nazm-cli`, `crates/nazm-service` (tests), `xtask`, the mutation
catalogue (`file =` repointed for moved entries), `docs/internal/maintainability.md`. Path-reading gates in
`xtask` and `crates/nazm-cli/tests/codes.rs` follow the moved files.
