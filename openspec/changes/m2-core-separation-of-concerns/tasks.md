## 1. Deduplicate

- [ ] 1.1 `nazm-service` tests: one shared `common` module (stack runner, workspace, walker)
- [ ] 1.2 One differential harness for the closure-style test files
- [ ] 1.3 Shared memory-counter helpers in the CLI test `common`
- [ ] 1.4 Library helpers: one digest, one path relativiser, one CLI command prologue, one failure
      summary; in `xtask` one root finder, walker, `ps` probe and shell helper

## 2. Gates follow directories

- [ ] 2.1 Path-reading gates and `tests/codes.rs` read a file or the directory of that name

## 3. Splits (move-only commits)

- [ ] 3.1 `xtask/src/main.rs` → `gates/` + command modules
- [ ] 3.2 `nazm-cli/src/main.rs` → `cli.rs`, `prepare.rs`, `interpret.rs`, thin dispatch
- [ ] 3.3 `nazm-core/src/eval.rs` → `eval/`
- [ ] 3.4 `nazm-core/src/check.rs` → `check/`
- [ ] 3.5 `nazm-cli/src/build.rs` → `build/`

## 4. Guard and documents

- [ ] 4.1 Module boundaries gate, with unit tests that break each rule
- [ ] 4.2 `docs/maintainability.md`: layout, the LIR oracle as a deliberate cross-check, §14

## 5. Evidence and close

- [ ] 5.1 Contained run of every moved mutant with its tier-1 killer verified
- [ ] 5.2 Contained workspace suite, selfhost and bootstrap
- [ ] 5.3 `openspec validate --strict`; archive into `repository-governance`; handoff
