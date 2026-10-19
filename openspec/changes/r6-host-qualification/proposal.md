## Why

`rewrite-in-nazm` (R6, tasks 7.1–7.2) asks two things before the cut-over: that the toolchain written
in Nazm builds itself under the bounded heap R1 added (`--heap-limit`, `docs/user/spec.md` *A bounded
heap*), with its peak memory recorded per stage; and a recorded decision on whether the runbook's
containment rule (`docs/internal/runbook.md`, *Mandatory containment*) can be revised on that evidence.
Containment stays in force while the evidence is gathered (`rewrite-in-nazm` design, decision 9):
every run of a Nazm-built stage here is a contained one.

## What Changes

- **`toolchain/selfbuild/selfbuild.sh`** (new): the self-build. The seed (the Rust `nazm`) builds
  the builder — the front end (`compiler/diagnose.nz --coredoc`), the pipeline
  (`toolchain/pipeline/main.nz`) and the code generator (`toolchain/codegen/main.nz`) — and each
  stage's builder then builds the builder and every other toolchain program with no Rust between
  source and object, each program under a heap limit. Stages 1, 2 and 3 are compared byte for byte
  at every step. Peak resident memory of every step is read from the kernel (`wait4`) inside the
  container. It refuses to run outside the contained runner, as `compiler/bootstrap.sh` does.
- **`cargo xtask contained selfbuild [stages=N]`** (new workload): the only way to run it; P1 by
  default, the checkout's verification cache like the bootstrap's.
- **`toolchain/pipeline/main.nz`**: `--target aarch64-unknown-linux-gnu` and `x86_64-unknown-linux-gnu`
  accepted. The contained runner is Linux, so the self-build is a Linux build; the runtime
  generator already carried Linux, the code generator already wrote ELF. Linux is still not a
  supported target (`docs/user/support.md`), and nothing else about the 1.0 narrowing changes.
- **A proposed revision of the containment rule** (`design.md`, decision 3, and `runbook-draft.md`),
  for the copyright holder to accept or refuse. It is **not applied** to `docs/internal/runbook.md`.

## Capabilities

### New Capabilities
None (archived with `--skip-specs`, as every phase of `rewrite-in-nazm`).

### Modified Capabilities
None.

## Impact

`toolchain/selfbuild/`, `toolchain/pipeline/main.nz`, `xtask/src/commands/contained.rs`,
`xtask/src/contained.rs`. `docs/internal/runbook.md` only if the proposal is accepted.
