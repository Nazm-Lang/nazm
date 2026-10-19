## 1. Deduplicate

- [x] 1.1 `nazm-service` tests: one shared `common` module (stack runner, workspace, walker)
- [x] 1.2 One differential harness for the closure-style test files
- [x] 1.3 Shared memory-counter helpers in the CLI test `common`
- [x] 1.4 Library helpers: one digest, one path relativiser, one CLI command prologue, one failure
      summary; in `xtask` one root finder, walker, `ps` probe and shell helper

## 2. Gates follow directories

- [x] 2.1 Path-reading gates and `tests/codes.rs` read a file or the directory of that name

## 3. Splits (move-only commits)

- [x] 3.1 `xtask/src/main.rs` → `gates/` + command modules
- [x] 3.2 `nazm-cli/src/main.rs` → `cli.rs`, `prepare.rs`, `interpret.rs`, thin dispatch
- [x] 3.3 `nazm-core/src/eval.rs` → `eval/`
- [x] 3.4 `nazm-core/src/check.rs` → `check/`
- [x] 3.5 `nazm-cli/src/build.rs` → `build/`

## 4. Guard and documents

- [x] 4.1 Module boundaries gate, with unit tests that break each rule
- [x] 4.2 `docs/internal/maintainability.md`: layout, the LIR oracle as a deliberate cross-check, §14

## 5. Evidence and close

- [x] 5.1 Contained run of every moved mutant with its tier-1 killer verified
- [x] 5.2 Contained workspace suite, selfhost and bootstrap
- [x] 5.3 `openspec validate --strict`; archive into `repository-governance`; handoff

Not done, by decision: 1.4's digest and path helpers (one-liners in separate crates, or different
contracts — `target/gates/m2/dedup.md`); `parser.rs` and `mir/lower.rs` (only if Gate 2 needs them).
