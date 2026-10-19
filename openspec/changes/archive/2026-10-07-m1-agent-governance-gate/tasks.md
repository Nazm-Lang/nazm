## 1. The gate

- [x] 1.1 Add `xtask/src/governance.rs` with one function per rule, each naming its invariant
- [x] 1.2 Register it as `agent governance` in `run_checks`
- [x] 1.3 Unit tests: each rule fails on a broken fixture and passes on the repository

## 2. Authorities and evidence

- [x] 2.1 Describe the gate in `docs/internal/agent-governance.md` §14 and `docs/internal/change-governance.md`
- [x] 2.2 `cargo xtask check` passes; `openspec validate --strict` passes
- [x] 2.3 Archive the change with its `repository-governance` spec
