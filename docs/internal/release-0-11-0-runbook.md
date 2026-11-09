# Release 0.11.0: the runbook for steps D2 to D7

Written 2026-11-09 from the headers and checks of the scripts themselves, to be read beside
[the plan](release-0-11-0-plan.md). Each command is the one its script documents; verify the
script's header again at the step, because a script is the authority, not this page. Nothing here
publishes by itself: the tag, the release upload, the signature, the deploy and the tap push are
each named where they happen.

## Rules that decide the order

1. **One commit, clean tree.** Every archive is built from the commit that will be tagged.
   `scripts/release-archive.sh` refuses unless the self-build record names that commit and that
   tree's build-input digest (`scripts/source-digest.sh`), and `--allow-dirty` is never used for a
   release.
2. **The macOS host-selfbuild runs under `host-run`**: one Nazm-built program at a time, 2 GiB
   counted heap, and it refuses beside a running container. So nothing contained runs during D3,
   and the mutation campaign and bench are finished first.
3. **At most two contained runs at once** (D2).
4. **Bench is run last and alone**, before the bump, in a quiet window; it is not repeated on the
   bumped commit unless the bump changed anything but version text.

## D1. The bump

Redo the dry run (draft PR #46) on the final main: the same edits, `docsdata.nz` regenerated with
`python3 toolchain/commands/tests/gen-docsdata.py . > toolchain/commands/docsdata.nz`, `capdata.nz`
checked with `gen-capdata.py` against a built `nazm`, the release notes filled (every
`TODO(gate)`), and the seed-freeze exemption's digest recomputed from `cargo xtask check`'s own
output after the last edit. A PR with green CI on its exact head, then merge.

## D2. Contained selfhost, bootstrap and selfbuild on the bumped commit

From a clean worktree of the merged commit (not the campaign's worktrees):

```sh
cargo xtask contained --profile p4t4 selfhost
cargo xtask contained --profile p4t4 bootstrap
cargo xtask contained --profile p4t4 selfbuild      # about 15 minutes
cargo xtask contained --profile p4t4 tests          # about 55 minutes
```

At most two at a time. Fuzz and the host legs do not need a rerun for a version-only bump, but a
change beyond version text reruns what it touches.

## D3. The archives

Nothing contained may be running. Use one worktree, clean, on the merged commit.

1. **macOS self-builds** (each a serial chain of `host-run` steps):

   ```sh
   toolchain/selfbuild/host-selfbuild.sh aarch64 OUT_A
   toolchain/selfbuild/host-selfbuild.sh x86_64  OUT_X
   ```

   Each writes `fixpoint.txt` ending `result passed`. A refusal, or status 70 (the rule withdrawn),
   stops it and leaves a marker for a person: do not rerun around it.
2. **The Windows self-build**, on the Dell: its builder is the stage 3 `nazm` of the passed
   aarch64 self-build above (`STAGE` is an environment variable; the script's default points at an older run).

   ```sh
   STAGE=OUT_A/stage3 toolchain/selfbuild/windows-selfbuild.sh win-dell OUT_W
   ```

   The Dell must answer `ssh win-dell` first, and the remote directory
   (`C:\Users\nazmci\nazm-w17`) must not exist. The target is `x86_64-pc-windows-gnu`, built by
   MinGW-w64's clang and linker from `C:\Users\nazmci\nazm-tools`.
3. **Package** (macOS host only; it verifies the ad hoc signature with `codesign`):

   ```sh
   scripts/release-archive.sh aarch64-apple-darwin    OUT_A OUTDIR
   scripts/release-archive.sh x86_64-apple-darwin     OUT_X OUTDIR
   scripts/release-archive.sh x86_64-pc-windows-gnu   OUT_W OUTDIR
   ```

   Each checks the record, the stage digests, the image format and architecture, the 2 GiB counted
   heap, the signature and that no debug map ships; it writes `OUTDIR/nazm-TRIPLE.tar.gz` and
   rewrites `OUTDIR/SHA256SUMS` over every archive there. Reproducibility is shown by the records
   themselves: stage 2 equals stage 3 on macOS, and the Windows build equals its second build.
4. **Clean-install check** of each Mac archive on the host:
   `scripts/clean-install-test.sh OUTDIR/nazm-aarch64-apple-darwin.tar.gz` (and the x86_64 one);
   it refuses to run if a Rust toolchain is on `PATH`.
5. Also publish `sbom.json` and `index.json` as 0.10.0 did: `cargo xtask evidence OUTDIR` writes them
   (every file's BLAKE3; `--verify` checks them).

## D4. The signature (the holder's step)

`scripts/sign-release.sh OUTDIR` signs `SHA256SUMS` keylessly with Sigstore and opens a browser for
a GitHub login; it is the copyright holder's step and is never run by an agent. After it,
`scripts/verify-release.sh OUTDIR IDENTITY ISSUER` must pass with the published identity. The
unattended run stops here. See the 2026-11-09 row of `decisions-log.md`.

## D5. Provenance, tag and release

Each of these is outward-facing and may be refused by the auto-mode classifier; on a refusal, stop
and tell the holder, and do not route around it.

1. The release notes (`docs/user/releases/0.11.0.md`): every `TODO(gate)` filled from the logs of
   the gate on this commit, the draft banner removed.
2. Annotated tag `v0.11.0` on the merged commit; push `main` and the tag only (never another
   branch).
3. GitHub pre-release `v0.11.0` with the three archives, `SHA256SUMS`, `SHA256SUMS.sigstore.json`,
   `SIGNER`, `sbom.json` and `index.json`; download each file back and compare its digest.

## D6. nazm.dev

```sh
cd suhail && NO_DEPLOY=1 ./deploy.sh v0.11.0     # build the tag's docs/user, check build/book
./deploy.sh v0.11.0                              # rsync to daakharkara-vps:/var/www/nazm.dev
```

`deploy.env` already names the host. Check `https://nazm.dev` shows the 0.11.0 page and that
internal pages answer 404.

## D7. homebrew-tap

A draft formula for both Mac archives is on the `nazm-0-11-0-draft` branch of `homebrew-tap`
(unpushed). Fill the two checksums from `SHA256SUMS`, then:

```sh
brew audit --strict --new nazm-lang/tap/nazm
brew install nazm-lang/tap/nazm && brew test nazm-lang/tap/nazm && brew uninstall nazm
```

Only then push to the tap. The draft installs the archive's layout whole under `libexec` and
points `NAZM_LIBEXEC` at it, because Homebrew's symlinked `bin` would hide the `libexec/nazm/`
directory `nazm` looks for beside its own `bin/`; the `brew test` proves that choice or replaces it.

## E1. Afterwards

Tick R8 11.1 and 14.1 and R1.5 8.1 and 8.4 in the `openspec` task lists, update the plan page,
`progress.json` and the resume memory.
