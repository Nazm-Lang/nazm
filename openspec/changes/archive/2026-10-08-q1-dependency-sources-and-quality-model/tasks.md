## 1. Dependency sources

- [x] 1.1 `xtask/src/supply.rs`: `check_lock`, with unit tests that break each rule
- [x] 1.2 Register the **dependency sources** gate in `cargo xtask check`
- [x] 1.3 `deny.toml`: `allow-wildcard-paths`, the `paste` advisory recorded with its path
- [x] 1.4 Mutant `q1-a-git-dependency-is-allowed`

## 2. Classification and process

- [x] 2.1 `docs/internal/quality-model.md`
- [x] 2.2 `docs/user/vulnerability-process.md`
- [x] 2.3 `docs/internal/security-qualification.md` classifies every Q1 finding by it

## 3. Close

- [x] 3.1 `openspec validate --strict`; archive into `repository-governance`
