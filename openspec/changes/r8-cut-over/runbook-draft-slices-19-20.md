# Draft — `docs/internal/runbook.md`, the runner changes of R8 slices 19 and 20 (task 13.7)

**Status: DRAFT, 2026-11-03. Not applied.** Slice 19 applies Part A when it removes `contained` and
`host-run` from `xtask`; slice 20 applies Part B when the authorities are rewritten (task 20.1). Until
then `docs/internal/runbook.md` keeps naming `cargo xtask contained …` and `cargo xtask host-run …`,
which still work (S13.7: they stay in `xtask`, unchanged but for `--print-argv`, until slice 19).
Nothing here is a decision beyond those already logged; question 5 was answered on 2026-10-20 (Part B.3).

Facts the text below rests on (S13.2 to S13.6 as built, `design.md`):

- `toolchain/harness/build.sh` builds `nzx` (heap limit 256 MiB), `nazm-mutate`, `nazm-fuzz` and
  `nazm-bench` (2 GiB each) into `target/nzx/`.
- `nzx host-run` is `xtask host-run` byte for byte (37 cases agree, 0 differ); `nzx contained` is
  `xtask contained` element for element (`--print-argv`, `tests/contained_argv.sh`, 67 cases).
- `mutate`, `fuzz` and `bench` now run the Nazm tools (`nazm-mutate session`, `nazm-fuzz`,
  `nazm-bench`). The Rust ones remain as `mutate-seed`, `fuzz-seed` and `bench-seed` (the seed's
  entries for `mutate-seed`), until slice 19.
- Cargo builds inside the contained image must run at `--profile p4t4` or less: at `p8t8` the 4096 MiB
  container kills the linker (`ld` signal 9) and every mutant is UNUSABLE (13.4).

---

## Part A — slice 19: the seed frozen, `xtask`'s `contained` and `host-run` removed

Slice 19 deletes `xtask/src/{contained,host_run}.rs` (and their tests, `--print-argv`, the
`mutate-seed`, `fuzz-seed` and `bench-seed` workloads) and applies:

1. **Every command name.** In `runbook.md` replace `cargo xtask contained <workload>` with
   `nzx contained <workload>` and `cargo xtask host-run …` with `nzx host-run …` (about 30 places,
   listed by `grep -n 'xtask contained\|xtask host-run\|xtask mutate' docs/internal/runbook.md`).
   `nzx` is the Nazm-built runner: `toolchain/harness/build.sh nzx`, then `target/nzx/nzx`.
   Its refusals keep their names and read `nzx: refused [NAME]: WHY` (status 1).
2. **`### What must never run uncontained`** begins: *Only through the verified contained runner,
   `nzx contained <workload>`:* and its last line is unchanged (fuzzing, the mutation catalogue, heavy
   benchmarks), now run by the Nazm tools (`nazm-fuzz`, `nazm-mutate`, `nazm-bench`).
3. **`### What may run on the host`**: the one way to start the stage is
   `nzx host-run [--deadline-secs N] -- EXECUTABLE [ARGS...]` (`toolchain/harness/nzx/hostrun.nz`;
   default deadline 900 s, at most 3600 s); the refusal list is unchanged
   (`not-macos`, `host-environment`, `usage`, `no-executable`, `no-counted-heap`, `no-heap-limit`,
   `ambiguous-heap-limit`, `heap-limit-too-large`, `withdrawn`, `another-stage-running`,
   `contained-workload-running`) plus `runner-unbounded` (Part B.3). The withdrawal marker
   (`~/.cache/nazm-host-run/withdrawn`) and the two lock slots are unchanged.
4. **Mutation catalogue.** `nzx contained mutate [--only NAME]… [--resume] [--verify-killers]` runs
   `nazm-mutate session`; journals are under `target/mutation-campaigns-nz/`. A campaign is run with
   `--profile p4t4` or less while its builds are Cargo's (the seed's entries, until the seed is
   frozen); `mutate-seed` and its journals (`target/mutation-campaigns/`) are removed with the Rust
   session. `nzx contained mutate --plan` prints the plan. `mutate-recover` for a tree mutated in
   place becomes `nazm-mutate recover`.
5. **Fuzzing.** `nzx contained fuzz [SECONDS]` runs `nazm-fuzz` (five SanitizerCoverage targets and
   the harness-only libFuzzer shim, then `nazm-fuzz diff` over three legs: run, LLVM -O0, -O2). The
   image needs `libclang-rt-19-dev` (in `docker/contained.Dockerfile`; the shared image is rebuilt by
   the copyright holder before the first run).
6. **Benchmarks.** `nzx contained bench` runs `nazm-bench`; the 25 % / 5 ms check and the baseline path
   rule are unchanged; `--fake-clock` runs the whole path with nothing measured and is never saved or
   checked. `contained bench` stays at P1 permanently.
7. **What `xtask` keeps** (S13.7) is named once, in `runbook.md`: `check`'s eight Rust-architecture
   gates, `workspace-tests`, `notices`, `evidence`, `gate-plan`, `guide`, `check-mutations`, and the
   `lint` alias, until the seed is frozen; the `xtask/tests/host_refusal.rs` and `lifecycle.rs`
   evidence is replaced by `nzx`'s own tests (`tests/host_run.sh`, `contained_argv.sh`,
   `contained_self.nz`, `contained_run.nz`) or retired with the Rust runner, each stated in the
   slice's own notes.
8. **Cargo inside the image.** Wherever the runbook gives a Cargo build in the container as a
   workload, it names `--profile p4t4` as the ceiling that builds (the 4096 MiB container cannot link
   8 jobs).

## Part B — slice 20: the authorities name `nzx`, question 5's answer recorded

### B.1 Where it lands

`runbook.md` (task 20.1), with `architecture.md`, `bootstrap.md`, `capability-matrix.md`, `support.md`
and `licensing.md` changed where they name the old commands. `bootstrap.md`'s chain names the
Nazm-built `nzx` as the runner of every contained step.

### B.2 The runner's section (new, after *What may run on the host*)

> ### The runners are Nazm programs — R8 slice 13 (decided 2026-10-20)
>
> `nzx host-run` and `nzx contained` are programs written in Nazm, built by
> `toolchain/harness/build.sh` with the checkout's `nazm` (the seed before the cut-over, the
> Nazm-built `nazm` after it). They are the only Nazm-built processes the harness itself starts on the
> host, and they are bounded as follows (B.3). The tools they run (`nazm-mutate`, `nazm-fuzz`,
> `nazm-bench`) run only inside the container, under its ceiling, built with a 2 GiB counted heap.

### B.3 Question 5's answer (copyright holder, 2026-10-20: recommendation accepted)

> **Accepted: the Nazm runners on the host.** While `nzx host-run` waits on a stage two Nazm-built
> processes exist (three on a `nazm build`: the runner, the waiting driver, the stage); `nzx contained`
> waits on the Docker client for up to an hour. No runner can be admitted by another. This is slice 4's
> waiting-driver reading extended one level, on these terms:
>
> - each runner is built with a counted heap of **at most 256 MiB** (`--heap-limit 256M`);
> - each **checks its own image at start** and refuses `runner-unbounded` when it cannot find the image
>   or the heap limit is missing or larger;
> - each **holds the host-run lock for its whole life** (`nzx contained` holds both slots; `--print-argv`
>   excepted, which starts nothing), so no host stage and no second container runs beside it;
> - each holds no work but bounded drains: a stage's or container's two streams are spooled to files
>   (4 MiB kept, the rest counted) and copied out in chunks.
>
> A runner's own peak is not measured from outside (nothing else on the host is there to do it); the
> counted heap is its bound. The alternative, keeping the runners in Rust after the cut-over, was not
> chosen.

### B.4 Question 6 (recorded beside it)

> The harness-only C shim for in-process libFuzzer (`toolchain/harness/fuzz/shim.c`, about 150 lines,
> compiled by the platform's `clang`) is accepted as within `rewrite-in-nazm` decision 1 and is never
> shipped, as `fuzz/cov.c` is not.

### B.5 Stated shortfalls the text must not paper over

- The mutation comparison with the Rust campaign was over 5 entries, not 20 (below).
- `nzx contained bench` was not run end to end and no real measurement was taken by an agent; the
  25 % / 5 ms gate on real numbers is the copyright holder's first run.
- `nzx contained fuzz` fails at the link until the shared image is rebuilt with `libclang-rt-19-dev`;
  60 s a target was a smoke run; `fuzz diff` has three legs, `lir` has no interpreter step.
- x86_64 plans were not compared (image absent); the lifecycle suite for `docker/` was not run for the
  Dockerfile edit.

---

## Follow-up logged by 13.4: widen the mutation catalogue's killer names

**Task, not done here.** 89 of the 94 `compiler/` / `library/` / `toolchain/` catalogue entries name a
whole test as their killer (not `FILE::fn`); only 5 map, so the Nazm and Rust campaigns could be
compared over 5 entries (agreeing 5 of 5, tier for tier), not the 20 asked for. Widening means giving
each of those entries a `FILE::fn` killer (the test function that fails on the mutant), checked with
`--verify-killer`, so `nazm-mutate`'s killer mapping covers them and the comparison can be repeated
over at least 20 entries before slice 19 removes the Rust campaign. Recorded as task 13.8 in
`tasks.md`.
