## L1 — Linux is a real target of the shipped `nazm`

- [x] 1.1 `nazm build`/`run` on a Linux host build for that host's Linux triple without `NAZM_CONTAINED`; the self-build's own containment checks move to its runner — 2026-11-28, `cargo xtask contained --profile p4t4 linux-selfbuild` at 46dfde0e (clean tree) passed in 1141.0 s: stage 3's `nazm`, with `NAZM_CONTAINED` unset (`env -u`), built `hello.nz` for aarch64-unknown-linux-gnu and `nazm run hello.nz` printed the expected line; `linux-selfbuild.sh` refuses outside the contained runner (`stage_init ... contained`)
- [x] 1.2 `--release` on Linux: strip, PIE, no signature; refusals list the Linux triples where they apply (01eb7694, and 02391703: a Linux release unit carries `PIC Level`/`PIE Level` 2, without which the aarch64 PIE link refused the canary's `__stack_chk_guard`) — 2026-11-28, `cargo xtask contained --profile p4t4 linux-selfbuild` at 46dfde0e (clean tree) passed in 1141.0 s: `readelf` shows the release `hello` and all 9 stage 3 programs aarch64 ELF64, DYN with FLAGS_1 PIE, no `.symtab`, BIND_NOW, GNU_RELRO, GNU_STACK `RW`; both run
- [ ] 1.3 Tests: the Linux admission and `--release` in the contained suite; the macOS and Windows behaviour unchanged

## L2 — the aarch64 Linux self-build, release grade

- [x] 2.1 `toolchain/selfbuild/linux-selfbuild.sh`: seed → 1 → 2 → 3, `--release --backend llvm --no-cache --heap-limit 2G`, every check `host-selfbuild.sh` makes (d70469fb; the frozen Rust seed hardens only macOS, so its builds — stage 0, the corpus references, the reference `nazm-check` — are LLVM without `--release`, said in the script) — 2026-11-28, `cargo xtask contained --profile p4t4 linux-selfbuild` at 46dfde0e (clean tree) passed in 1141.0 s: stage1 = stage2 = stage3, 9 of 9; Core IR 9/9 identical; corpus 66 agree, 0 disagree, 0 not built; interpreter 68/68; `nazm-check` byte for byte (15 gates); the 256 MiB probe stopped with N0406; largest peak 1567.9 MiB (`nazm` build); log ~/nazm-linux-lsb-2.log
- [x] 2.2 It writes `nazm.selfbuild-record/1` (`script linux-selfbuild.sh`, the environment, the source digest) only on a pass: build-input digest a7d50d06…176b, Linux 7.0.14-linuxkit, Debian clang 19.1.7, glibc 2.41
- [x] 2.3 A `contained` workload runs it and exports the record directory; `verify-selfbuild.sh` knows its stage pair (7b76569c, 15544055): exported `target/linux-selfbuild/linux-selfbuild.tar.gz` (74,289,549 B) and `fixpoint.txt`; `scripts/verify-selfbuild.sh --record` on the unpacked record: ok, 30 files with the record's digests, stage1 = stage2 = stage3, this checkout's build-input digest
- [ ] 2.4 A passed run on main's commit, recorded here with its numbers (NOT TICKED: the pass above is on branch `linux-toolchain` at 46dfde0e, not on main; rerun after the merge)

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
