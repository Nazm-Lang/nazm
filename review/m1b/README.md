# M1b review bundle

Prepared for the adviser. Local files only — nothing has been pushed or published.

| File | What it is |
| --- | --- |
| `direction.md` | Current `docs/direction.md`, complete |
| `roadmap.md` | Current `docs/roadmap.md`, complete — includes the M1b accepted/rejected table and the M1a/M1b status sections |
| `source-changes.md` | The full diff of tracked files **plus the complete contents of every new untracked file** (`crates/nazm-lir/`, `crates/nazm-cli/src/build.rs`, `crates/nazm-cli/tests/build.rs`, `xtask/tests/lifecycle.rs`, `docs/direction.md`, `docs/roadmap.md`). `Cargo.lock` excluded as noise |
| `verification.md` | Every command run, its result, and what was **not** exercised |
| `nested-conditional.nz` | The representative program: nested `if`, `if` in a condition, branch-local shadowing, checked arithmetic in a branch |
| `nested-conditional.ll` | Its emitted LLVM IR, exactly as `nazm build --emit-ir` produced it (absolute paths rewritten to `<repo>`) |

Reproduce the program:

```sh
nazm run   review/m1b/nested-conditional.nz            # 101
nazm build review/m1b/nested-conditional.nz -o /tmp/nc --emit-ir && /tmp/nc   # 101
```
