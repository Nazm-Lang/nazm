## L1 — Linux is a real target of the shipped `nazm`

- [ ] 1.1 `nazm build`/`run` on a Linux host build for that host's Linux triple without `NAZM_CONTAINED`; the self-build's own containment checks move to its runner
- [ ] 1.2 `--release` on Linux: strip, PIE, no signature; refusals list the Linux triples where they apply
- [ ] 1.3 Tests: the Linux admission and `--release` in the contained suite; the macOS and Windows behaviour unchanged

## L2 — the aarch64 Linux self-build, release grade

- [ ] 2.1 `toolchain/selfbuild/linux-selfbuild.sh`: seed → 1 → 2 → 3, `--release --backend llvm --no-cache --heap-limit 2G`, every check `host-selfbuild.sh` makes
- [ ] 2.2 It writes `nazm.selfbuild-record/1` (`script linux-selfbuild.sh`, the environment, the source digest) only on a pass
- [ ] 2.3 A `contained` workload runs it and exports the record directory; `verify-selfbuild.sh` knows its stage pair
- [ ] 2.4 A passed run on main's commit, recorded here with its numbers

## L3 — x86_64 Linux, cross-built, run under emulation

- [ ] 3.1 The contained image gains Debian's amd64 cross libc; an amd64 run image exists (`--platform amd64`)
- [ ] 3.2 The nine built for `x86_64-unknown-linux-gnu` twice by a passed L2 stage 3: identical
- [ ] 3.3 The nine and the corpus run in a `linux/amd64` container (emulated, labelled) and agree with aarch64; the record names its builder's record
- [ ] 3.4 `r9-code-generator` 6.3b: `contained elfrun` on x86_64 Linux (emulated), recorded

## L4 — the archives

- [ ] 4.1 `release-archive.sh` accepts both Linux triples and checks ELF, architecture, PIE, stripped, the 2 GiB counted heap
- [ ] 4.2 `INSTALL.txt` for Linux: what `nazm build` needs (`clang`, the C library's headers)
- [ ] 4.3 The clean-install test of each archive in a fresh Debian container of its architecture

## L5 — the claim

- [ ] 5.1 `docs/user/support.md`: the Linux rows and the "internal" paragraph replaced by what ran
- [ ] 5.2 `getting-started.md` (Linux), `limitations.md`, the 0.12.0 release notes' Linux rows
- [ ] 5.3 `rewrite-in-nazm` 4.2b (Linux half) and `r9-code-generator` 6.3b updated from the evidence
