## Why

`rewrite-in-nazm` R8 (tasks 10.1 and 10.2) is the cut-over: the `nazm` the project releases is built
by Nazm, from Nazm, and the Rust implementation is frozen as the bootstrap seed (that change's design,
decisions 4 and 7). At `637a62ba` the pieces exist side by side — the front end (R2), the pipeline
(R3), the interpreter (R4), most commands (R5), the host rule and the Linux self-build (R6), most of
the harness (R7), the code generator (R9) — but the released binary is still the Rust one, and the
Nazm-written `nazm` carries 23 of the 43 commands and refuses the other 20. A cut-over made now would take away
`build`, `run`, every target but the self-build's, and a dozen commands users have today.

This change is the plan that makes the cut-over lose nothing a user of the Rust `nazm` has, and the
slices that carry it out, each leaving a working, tested tree. The release gate (10.3) is not part
of it.

## What Changes

- **The inventory** (`design.md`, *What the released `nazm` must still do*): every command and flag
  of the Rust `nazm` against its Nazm port, the facts and stages each waits on, and what stays in
  the Rust seed.
- **Twenty slices in order** (`design.md`, *Slices*; `tasks.md`), sized and marked where the design
  needs the strongest model, ending with the seed frozen and the authorities updated.
- **Slice 1, done here:** R6's first gap — the interpreter (`toolchain/interp/`) and `nazm-check`
  (`toolchain/harness/check/`) moved from 0.10.0's `List[T]` to the checkout's `Chain[T]`
  (R1.5 D7): 14 lines. Both join the self-build's programs, which now also runs the self-built
  interpreter over the corpus against the seed's `run --interpret` and the self-built `nazm-check`
  over the tree against the seed's build of it. The scripts that built them with the 0.10.0
  release build them with the checkout's `nazm`; the interpreter's suite refuses to run uncontained.
- **Four direction-level questions** for the copyright holder (`design.md`, *Open questions*).

## Capabilities

### New Capabilities
None (archived with `--skip-specs`, as every phase of `rewrite-in-nazm`).

### Modified Capabilities
None.

## Impact

Slice 1: `toolchain/interp/{types,values,vm}.nz`, `toolchain/harness/check/supply.nz`,
`toolchain/selfbuild/selfbuild.sh`, `toolchain/interp/tests/run.sh`,
`toolchain/harness/tests/{run,seeded}.sh`. `compiler/` and `crates/` unchanged. The later slices
touch every `toolchain/` program, `xtask/`, `scripts/`, `.github/`, and the authorities named in
`rewrite-in-nazm` 10.2.
