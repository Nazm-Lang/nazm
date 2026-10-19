## 1. The self-build (`rewrite-in-nazm` 7.1)
- [x] 1.1 `toolchain/selfbuild/selfbuild.sh` and `cargo xtask contained selfbuild [stages=N]`: the
      seed builds the builder; stages 1–3 each build the builder and the other toolchain programs
      under a 2 GiB heap; refuses to run uncontained
- [x] 1.2 The pipeline accepts the Linux triples (the container's), for the self-build only
- [x] 1.3 `diagnose --coredoc-sources`: the modules' sources, so failures name their line and column
- [x] 1.4 The fixpoint: stages 1, 2 and 3 identical, every file (`design.md`, *The evidence*)
- [x] 1.5 The last stage's Core IR equal to the reference's (less *Gaps*' two members); 66 programs
      it builds run as the seed's builds do; the heap probe stops with `N0406`
- [x] 1.6 Peak resident memory of every step recorded
- [ ] 1.7 The interpreter and `nazm-check` in the self-build, once their tracks move from 0.10.0 to
      the checkout's language (`design.md`, *Gaps*)
- [ ] 1.8 `callee_at` and a payload binding's `at` in `--coredoc-sources` (R2)

## 2. The containment rule (`rewrite-in-nazm` 7.2)
- [x] 2.1 The proposal: `design.md` decision 3 and `runbook-draft.md`
- [ ] 2.2 The copyright holder's decision; if accepted, the runbook changed and the host runner
      written (refuses a binary without the counted heap or with a limit over 2 GiB)
- [ ] 2.3 If accepted: the first host runs of the builder on macOS, their resident peaks recorded
