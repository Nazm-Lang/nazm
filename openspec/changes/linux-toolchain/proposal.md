## Why

0.11.0 (2026-11-28) shipped the toolchain written in Nazm for macOS (aarch64, x86_64) and Windows
x86_64. Linux, wasm32 and WASI were moved to 0.12.0 on 2026-11-05 to ship sooner, and 0.12.0's
proposal (`release-0-12-0`, decided 2026-11-28) puts Linux first, because the million-connection
run and the platform scope need it. This change is the Linux half: `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu` as release targets of the Nazm-built toolchain. wasm32 and WASI are a
separate change.

What exists (mapped 2026-11-28 from the tree): the pipeline and the Nazm code generator accept both
Linux triples and write ELF; `cargo xtask contained elfrun` gave 129 agree, 0 differ on aarch64 Linux
(`r9-code-generator` 6.3); `cargo xtask contained selfbuild` builds the nine programs stage over
stage inside the aarch64 Linux container. What blocks a release:

1. The Nazm-built `nazm` builds for a Linux triple only under `NAZM_CONTAINED=1`
   (`toolchain/commands/build.nz`), so a shipped Linux `nazm` refuses its own host;
2. `--release` is refused off macOS (its finish step is `strip` and `codesign`);
3. the container self-build uses the `nazm` backend without `--release`, writes no
   `nazm.selfbuild-record/1` and exports nothing;
4. `scripts/release-archive.sh` knows only the three 0.11.0 triples and Mach-O/PE images;
5. nothing x86_64 Linux has run here: no amd64 image, and this Mac emulates amd64 with qemu at
   about 9× native (a shell loop: 42.6 s against 4.8 s, 2026-11-28), not with Rosetta.

## What Changes

- **L1 — Linux is a real target of the shipped `nazm`.** A `nazm` running on Linux builds for its own
  Linux triple without `NAZM_CONTAINED`; `--release` on Linux strips and makes a PIE (no signature:
  Linux has none to make); the refusals name the Linux triples where they now apply. The containment
  rules of the toolchain's own self-build are unchanged: they move to where they belong, the
  self-build's runner, not the user's `nazm build`.
- **L2 — the aarch64 Linux self-build, release grade.** `toolchain/selfbuild/linux-selfbuild.sh`,
  run inside the aarch64 container by a `contained` workload: seed → stage 1 → 2 → 3, each stage the
  nine programs built `--release --backend llvm --no-cache --heap-limit 2G`, with every check
  `host-selfbuild.sh` makes (stages byte-identical, Core IR equal to the seed's, the corpus and the
  interpreter agreeing, the 256 MiB heap probe stopping with N0406), writing the same
  `nazm.selfbuild-record/1` record, exported out of the container. The container's cgroup (4 GiB,
  no network) is the ceiling `host-run` is on macOS.
- **L3 — x86_64 Linux, cross-built and run under emulation.** As Windows was in 0.11.0: the stage 3
  of a passed aarch64 Linux self-build builds the nine for `x86_64-unknown-linux-gnu` twice (they must
  be identical), linked against Debian's amd64 cross libc in the same image; the nine and the corpus
  then run in a `linux/amd64` container and must agree with the aarch64 answers. The emulation is
  named in every record and claim, and no timing is claimed from it. A native x86_64 self-build waits
  for an x86_64 Linux host or Docker's Rosetta emulation.
- **L4 — the archives.** `release-archive.sh` accepts the two Linux triples and checks their images
  (ELF 64-bit, the architecture, PIE, stripped, the counted heap at 2 GiB); `INSTALL.txt` for Linux
  (`clang` and the C library's headers are what `nazm build` needs). The clean-install test runs each
  archive in a fresh Debian container of its architecture.
- **L5 — the claim.** `docs/user/support.md` drops "Linux triples are internal" for exactly what was
  run; getting started and limitations gain Linux; the release notes' Linux rows are filled from the
  gate. The release gate becomes the 0.11.0 gate plus L2-L4.

## Order

L1, then L2 (aarch64 is native here and is the builder of L3), L3, L4, L5. Each slice reaches main
through its own pull request and the usual checks; the self-builds are contained runs.

## Capabilities

### New Capabilities
- `linux-toolchain`: the Nazm-built `nazm` runs on and builds for Linux x86_64 and aarch64.

## Impact

`toolchain/commands/build.nz` (target admission, the release finish step), `toolchain/selfbuild/`
(a Linux self-build), `xtask` (a contained workload and its export), `docker/contained.Dockerfile`
(the amd64 cross libc), `scripts/release-archive.sh`, `scripts/verify-selfbuild.sh`,
`scripts/clean-install-test.sh`, `docs/user/`. Nothing a program means changes. Not in this change:
wasm32/WASI, a Linux package manager entry, Homebrew on Linux, a native x86_64 self-build.
