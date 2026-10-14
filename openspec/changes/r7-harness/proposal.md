## Why

`rewrite-in-nazm` (R7, tasks 8.1–8.2) rewrites the repository's harness and tests in Nazm, so the
toolchain is checked by Nazm programs. Today they are Rust: `cargo xtask check` (twenty-four gates),
the mutation runner, the contained runner, `bench`, `fuzz`, and about three thousand Rust tests, 109
of 130 CLI test files of which drive the `nazm` binary and compare text. Every one of them can be
written now against the Rust `nazm` (0.10.0), and held to the Rust original by running both on the
same tree, so R7 runs in parallel with R2–R4 and R9 rather than after them.

## What Changes

- **`toolchain/harness/`** (new): Nazm programs that drive `$NAZM` and read the tree.
  - `lib/` — the harness library: running a child (`run.nz`: directory, environment stated in
    full, both streams drained, a 60-second deadline that ends the child's process group),
    scratch directories (`scratch.nz`), the differential harness (`differential.nz`: interpreter,
    LLVM `-O0`/`-O2`, Cranelift, memory reports, `check --json` codes), and the tree's root
    (`tree.nz`).
  - `cli/NAME.nz` — each ported black-box test file of `crates/nazm-cli/tests/NAME.rs`, its `#[test]`
    functions as `@test` functions of the same names, run by `nazm test`.
  - `check/` — `nazm-check ROOT [GATE…]`, the file-reading gates of `cargo xtask check`, printing
    the same `ok`/`FAIL` report.
  - `mutate/` — the mutation runner's core: read the catalogue, apply one mutation to a tree
    exactly once, restore it byte for byte, and turn the killers' results into a verdict. The
    contained runner and Docker stay Rust (the coordinator decides when they move).
  - `tests/run.sh` — builds what needs building, runs everything against the Rust originals, and
    prints `N agree, M differ`.
- **No change outside `toolchain/harness/` and this change.** What R7 needs elsewhere (a Rust test
  turned into a call of `run.sh`, a docs or runbook line) is asked for at hand-back.

## Capabilities

### New Capabilities
- `harness-in-nazm`: what a ported check or test must do to count as ported. Archived with
  `--skip-specs`, as `rewrite-in-nazm` is; the normative text lands in `docs/runbook.md` and
  `CONTRIBUTING.md` when the coordinator merges.

### Modified Capabilities
None.

## Impact

New files under `toolchain/harness/` only. The Rust harness and tests are unchanged and stay the
gate until R8; the Nazm ones run beside them and are compared with them.
