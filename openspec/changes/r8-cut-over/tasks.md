## 0. The plan
- [x] 0.1 The inventory, the slices in order with sizes and models, the open questions (`design.md`)
- [x] 0.2 The copyright holder's answers to `design.md`'s four open questions (accepted 2026-10-20)
- [x] 0.3 The copyright holder's answers to questions 5 and 6 (`design.md`, *Open questions —
      slices 7, 13 and 16*): both recommendations accepted 2026-10-20 (`design.md` states it)

## 1. Slice 1 — R6 gap 1: the interpreter and `nazm-check` on the checkout's language
- [x] 1.1 `toolchain/interp/{types,values,vm}.nz` and `toolchain/harness/check/supply.nz`: `List[T]`
      and `list_*` to `Chain[T]` and `chain_*` (R1.5 D7); the checkout's `nazm check` finds no error
- [x] 1.2 `toolchain/selfbuild/selfbuild.sh`: `interp` and `check` among the programs every stage
      builds (so in the fixpoint); the last stage's interpreter runs the corpus against the seed's
      `run --interpret`, its `nazm-check` reports the tree against the seed's build of it
- [x] 1.3 The scripts that built them with 0.10.0 build them with the checkout's `nazm` and a 2 GiB
      heap (`toolchain/interp/tests/run.sh`, `toolchain/harness/tests/{run,seeded}.sh`); the
      interpreter's suite refuses to run uncontained
      (`cargo xtask contained run 'cargo build --locked -p nazm-cli && sh toolchain/interp/tests/run.sh'`)
- [x] 1.4 `cargo xtask contained selfbuild`: stages 1–3 identical with the two programs added, the
      interpreter and `nazm-check` agreeing — passed 2026-10-20: 68 programs run by the self-built
      interpreter as by the seed's, `nazm-check` byte for byte (16 gates). The one disagreement the
      first run found (calls.nz, fail_defer.nz) was the runaway-recursion message, which the seed's
      interpreter words by its host stack ("at N calls") and the Nazm one by its fixed depth of 512;
      the comparison now requires N0403 from both and the same output and status

## 2. Slice 2 — the comparison scripts under the R6 rule
- [x] 2.1 `cargo xtask host-run`: the program's exit status passed through; its report on standard
      error or in a file, not on standard output (`--report FILE`; status 124 deadline, 125 signal, 126
      output past `--max-output-mib`, 70 withdrawn; output passed as bytes; tests in
      `xtask/tests/host_run.rs`)
- [x] 2.2 Every script under `toolchain/` that runs a Nazm-built program (front end, pipeline,
      code generator, commands, harness, interpreter) builds it with the checkout's `nazm` and a heap
      limit, and runs it contained or through `host-run`; none defaults to the 0.10.0 release
      (`toolchain/lib/stage.sh`, `stage-run.sh`; the scripts that run programs in parallel, built
      without a limit, or as a campaign refuse to start uncontained; host smoke: a built program
      through `stage-run.sh` and the port's `check` under a 2 GiB limit; the suites themselves
      unexecuted here)

## 3. Slice 3 — R6 gap 2
- [x] 3.1 `callee_at` (`call`, `call_indirect`, `call_trait`) and a payload binding's `at` in
      `--coredoc-sources`; the self-build compares Core IR whole (`r6-host-qualification` 1.8)

## 4. Slice 4 — `nazm build` in Nazm: the driver
- [x] 4.1 Design (Opus): one program or `libexec/` children; the object cache; the counted heap in
      the LIR; the one-thread decision; linking per target (`design.md`, *Slice 4*, S4.1–S4.7)
- [x] 4.2 `build` group 1 (`design.md`) for the macOS triples, Windows x86_64 and the internal Linux
      one; compared with the Rust `build --backend nazm` on the corpus: objects with `--objects`,
      the executables' behaviour (`toolchain/commands/build.nz`; `nazm-pipeline build`/`put` and
      the summation rewrite, `toolchain/pipeline/vectorize.nz`; `toolchain/commands/tests/build.sh`:
      see *Slice 4 — the evidence* in `design.md`)
- [x] 4.3 `selfbuild.sh` builds through the stage's `nazm build` instead of its glue —
      `cargo xtask contained selfbuild` passed 2026-10-20 (`design.md`, *Slice 4 — the evidence*)
- [ ] 4.4 Left by 4.2: the runtime's units through the object store (clang's `-###` probe, R3's
      keys) and the user cache for shared units; `build --help` and clap's usage errors; Rust's
      exact refusal texts for the targets it names

## 5. Slice 5 — the LLVM path
- [x] 5.1 LIR → LLVM IR text (`toolchain/pipeline/{llvm,lldebug,llpost}.nz`, `--layout soa` in `ops.nz`), byte for
      byte with `nazm build --emit-ir`: the 78 buildable programs of the corpus for `aarch64-apple-darwin`,
      `x86_64-apple-darwin`, `x86_64-pc-windows-gnu`, `--heap-limit`, `--debug`, `--release`, and the
      toolchain's own nine programs (`design.md`, *Slice 5 — the evidence*)
- [x] 5.2 `build` group 2: `--backend llvm` the default, `--opt-level` (ThinLTO at `-O2` and above on
      macOS), `--emit-ir`, `--debug` (`dsymutil`), `--layout`, `--release` (`strip`, `codesign`); the
      comparison `toolchain/commands/tests/build.sh`: 123 inputs × 14 LLVM modes **1722 agree, 0 differ**,
      `--layout soa` 246 agree, slice 4's six modes 738 agree (two defects the toolchain's programs found in
      slice 4's text are fixed: a module outside the source root named `#N`, and tuples of different arity
      sharing one layout)
- [x] 5.3 `cargo xtask contained selfbuild backend=llvm`: the seed's, stage 1's, 2's and 3's builders
      identical (`nazm`, `pl`, `cg` of stage 0 equal stage 1's: Nazm's text is the Rust build's), 9 programs
      per stage identical, 66 corpus programs run as the seed's, the interpreter and `nazm-check` agree;
      the default (`backend=nazm`) self-build passes again
- [ ] 5.4 Left: the LLVM objects through the object store and the ThinLTO cache (derived data),
      `--build-report`, `--summary-json`, `--timings`, `--lib`; the released set's backend is slice 15's

## 6. Slice 6 — the rest of `build`
- [x] 6.1 `build` group 3; `nazm runtime build` and `verify`
      Evidence (17f9105c): `toolchain/commands/tests/build.sh` over the whole corpus, every mode (nazm
      backend, LLVM, report, summary, timings, lib, lib-shared, lib-objects, rtlink, prov, prov-sign, link)
      and the command lines (`BUILD_ARGS=1`): 4585 agree, 0 differ, 16 refused (the Linux/WASM/iOS/board
      targets the cut-over removes), after one fix (a runtime artifact that does not fit is silent on
      standard error under `--summary-json`; the args part rerun: 158 agree, 0 differ, 16 refused).
      Contained self-builds from a checkout under $HOME: `backend=nazm` passed in 591.3 s, `backend=llvm`
      passed in 709.9 s (stages 1 to 3 byte-identical; 66 programs agree with the seed's builds).

## 7. Slice 7 — the interactive tier
- [x] 7.1 Design (Opus): `design.md`, *Slice 7*, S7.1–S7.8
- [x] 7.2 The interpreter as a module: `toolchain/interp/entry.nz` (`interp_exec`, `interp_run`, `interp_constant`, `ie_show` with the
      reference's `Display`); `interp/main.nz` calls it
- [x] 7.3 `toolchain/commands/interpret.nz` (the one importer of `toolchain/interp/`); `nazm.nz`'s `main` gains `OutCap`, `NetCap`,
      `RandomCap`; the reports never fire (`interactive.sh interpret`, the report variables)
- [x] 7.4 `--harness SPEC` (`toolchain/commands/harness.nz`), accepted by `build` and `run`
- [x] 7.5 `run --interpret` and its flags and value parsers (`--max-depth`'s, clap's `last(true)` after `--`)
- [x] 7.6 `bd_build(Opts)` with `run` mode; compiled `run` (`runexe.nz`): `--backend nazm`, no heap limit, cache on, run from the
      build's own work directory, three relays, `passed_through`'s status
- [x] 7.7 `comptime`: the entry-less document keeps every root function (no `compiler/core.nz` change needed); `N0393`'s refusals
- [x] 7.8 `repl` (`live.nz`)
- [x] 7.9 `test` without `NAZM_TEST_DRIVER`: the driver found by `own_path` (decisions log: `arg(0)` is not the program's name)
- [x] 7.11 the front end builds `toolchain/commands/nazm.nz` in a quarter of the heap (76347cf5: span maps by buckets, the Core IR
      document indented as each value is read); the 2 GiB rule stands. Evidence: the contained self-build's stage 1 `nazm` build peaks at
      817424 KiB (`backend=nazm`, 792.6 MiB largest step) and 811644 KiB (`backend=llvm`), where it needed about 1.95 GB before (N0406 at
      the 2 GiB ceiling); stages 1/2 and 2/3 byte-identical (9 files each), 66 corpus programs and 68 interpreted programs agree, nazm-check
      agrees; `contained selfhost` and `contained bootstrap` pass. Output unchanged: `coresources` 2015 agree, 0 differ (byte for byte,
      104 MB documents), `core` 3361/0, `coredigests` 3361/0, `mir` 2015/0; `inspect.sh` 0 differ in every part; `interactive.sh`
      args 79/0, interpret 1344/0, comptime 1449/0, repl 6/0, test 230/0, run 450 agree and 1 differ (`examples/bad/mutual_recursion.nz`:
      the position a native stack overflow is reported at, 5:4 against 1:4, a frame-layout matter of the two backends, not of the front
      end); `build.sh` args 159/0 and a corpus pass (`exe llvm`) 403+/0. Harness fixes found on the way: `host-run` kept only 64 MiB of
      output (nazm.nz's document is 104 MB: the four toolchain inputs looked truncated), growth.py kept the 4 MiB default, `build.sh` named
      `/var` where messages say `/private/var`
- [x] 7.10 `toolchain/commands/tests/interactive.sh`, `tests/repl/*.in`, `tests/test-fixture/`, `tests/args/{run,comptime,repl}.txt`;
      evidence in `design.md`, *Slice 7 — the evidence*

## 8. Slice 8 — the inspection commands
- [x] 8.1 `core-ir`, `mir`, `lir`, `capabilities`, `inspect`, `obligations`, `explain-cost`,
      `explain-flow`, `trace`, `profile`. Evidence (`tests/inspect.sh`, Nazm `nazm` against the Rust
      one, stdout, stderr and status byte for byte, one host-run at a time, heap limit 2G; 0 differ
      everywhere). Over the corpus: core-ir 1748 agree, 0 refused; mir 1064 agree, 247 refused
      (245 `--spans`: R3 hand-back, the MIR printer names no statement's position; 2 not lowered);
      lir 1744 agree, 4 refused (not lowered); capabilities 2 agree; obligations 868 agree, 6 refused
      (proved-call witness: R2 hand-back); inspect 429 agree, 8 refused (package inspect: slice 6;
      foreign function; not lowered); explain-cost 384 agree, 490 refused (cost sites, vectorizer
      reasons, layouts: R3 hand-back); explain-flow 466 agree, 408 refused (sinks and origin traces:
      R2 hand-back); trace 25 agree, 2 refused (non-TOML requirements); profile 0 agree, 2 refused
      (builds with counters, wall time). Command lines (`args` part): core-ir 22/0/0, mir 17/0/2,
      lir 23/0/4, capabilities 10/0/0, obligations 15/0/3, inspect 16/0/0, explain-cost 9/0/2,
      explain-flow 9/0/2, trace 11/0/2, profile 11/0/7 (agree/differ/refused). `build.sh`
      `BUILD_PART=args` after the cli.nz merge with slice 6: 158 agree, 0 differ, 16 refused.

## 9. Slice 9 — the agent commands whole
- [x] 9.1 R2 publishes a function's effects; `context` and MCP `nazm.semantic_context`. Evidence (`tests/agent.sh`, design *Slice 9 — the
      evidence*): effects 437 agree, 0 differ (`refdump effects`); `context` 3635 agree, 0 differ, 99 refused (97 errors that leave the
      facts' index unlike the reference's, 2 a record the facts do not publish); command lines 52/0/1.
- [x] 9.2 `repo` (`nazm-repo` ported); MCP `nazm.repository_map`, `nazm.task_context`. Evidence: `repo --map` and 568 `--task` requests
      568 agree, 0 differ, 0 refused; command lines 54/0/2 (no `--root`). MCP: `nazm.semantic_context` (the offsets, the absolute path and 7 argument
      errors), `nazm.repository_map` and `nazm.task_context` (every kind of seed, every intent, a budget, the stale state and 12 invalid requests) through
      the Nazm `nazm-mcp` asking the Nazm `nazm`, 16 inputs of about 60 messages each (`agent.sh mcp-gen|mcp-serve|mcp-cmp`): 16 agree, 0 differ, 0 refused. `toml.nz`'s new forms, re-compared: `build.sh` args 159/0/15, `run.sh` args (all but `attest`, which needs `ssh-keygen`) 530/0/7, doc 5/0/1, init 24/0/0, `inspect.sh` trace 25/0/2 (check read no inputs: the container has no git).

## 10. Slice 10 — every command routed
- [x] 10.1 `bench`, `accel`; `contract` as question 1 decides. Every command line routed. Evidence (`tests/tools.sh`, design *Slice 10 — the evidence*):
      `args` 88 agree, 0 differ, 34 refused (122 command lines); `bench` 41/0/3 (no benchmark run: nothing here builds a `@bench` function), `shape` 21/0/0
      (the row and the harness line, on their own); `accel` 1709/0/0 (`--print-kernel` byte for byte, the refusals of an ineligible function, the inputs and their
      number); `contract` 18/0/985. **Refused:** `contract --evm` and `--wasm` (deferred targets, open question 1) and every contract that loads (the `web3`
      profile is slice 11's, and R2 publishes no contract facts); `accel`'s run (closed by 10.2 below);
      `bench`'s `@cfg` of a feature, package or profile. **Intended difference:** a `bench` row's numbers are the clock's, compared by shape only.
- [x] 10.2 (slice 11d) `accel`'s run in Nazm: failure sites recorded, the embedded C host written out and compiled by the platform's `clang`, run, the interpreter called per element, the fold,
      the `nazm.accel/3` report (design S10.2). `tests/accel-run.sh`: `cpu` 178, `zip` 178, `reduce` 178, `opts` 267, `auto` 89, `opencl` 89, `errors` 397 agree; 0 differ, 0 refused. **Intended
      difference:** every measured time (`timings_ms`, a transfer's and the fold's `ms`) masked in both outputs, shape compared only. **Refused:** the CPU provider on a Windows host.
      (`contract`'s `--interface`, `--txs`, `--accounts` and `--upgrade-check` closed in slice 11.)

## 11. Slice 11 — R5's refusals
- [ ] 11.1 (not closed. Closed in slice 11b/11d: `N0357` in the pipeline (`expanding-instances.nz` agrees in mir, lir, explain-cost, contract, check --profile), MCP argument wording and no-`--docs` sessions, manifest `heap-limit` and unenabled `[features]`. Completion closed in slice 11c. Left: `check --cache-report` without `--no-cache` stays refused until the check cache is ported; `[registry]`, registry dependencies and `default`/dependency features stay refused (solver, N90); S11.2) `check`, `lsp`, `patch`, `references`, `resolve`, `fix`, the MCP server: every refusal
      `design.md` lists, with R2's hand-backs. Closed but for what `design.md` S11.2 lists and leaves open (`check --cache-report`, registry manifests and dependency features, `lir` of `ffi_v2-012`, `explain-flow` of `flow_v3-015` and `contract --upgrade-check` of `patterns-001` as S11.3 lists them; N0357 is evidenced by `expanding-instances.nz` and a sample, not the whole corpus; the
      deferred targets). Evidence: `refusals.sh` over 2865 inputs (the corpora and the Rust suites' programs): check, fix, resolve, snapshot, diagnostics 2865/0/0, patch 2870/0/0,
      `lsp-*` 14 modes (with `complete` and `holes`: completion closed) 2846 inputs each (multi 2822), 0 differ, 0 refused; profiles 12826/0/26 (counted before the N0357 closure, not re-measured)

## 12. Slice 12 — a `nazm` that needs no `NAZM_LIBRARY`
- [x] 12.1 The standard library and the documentation corpus carried or installed; the archive
      layout (carried as generated modules; archive.sh 256/0/0; `design.md`, *Slice 12*)

## 13. Slice 13 — the harness remainder (`r7-harness` 6.2, 6.3)
- [x] 13.1 Design (Opus): `design.md`, *Slice 13*, S13.1–S13.8; questions 5 and 6 open
- [ ] 13.2 (part done 2026-11-01: `build.sh`, `nzx host-run`, its self-check, its tests and `stage-run.sh` are in; the host runs were measured: `tests/host_run.sh both` 37 agree, 0 differ, 2 skipped (19 cases on nzx, 18 on xtask; `not-macos` skipped on macOS), `host_run.sh compare` 21 agree, 0 differ, `host_run_self.nz` 14 of 14 interpreted; left: a contained run of `host_run_self.nz` natively, and `build.sh` for `nazm-fuzz`/`nazm-bench` with 13.5/13.6) `toolchain/harness/build.sh` (`target/nzx/`); `nzx`'s self-check of its own image
      (`runner-unbounded`); `nzx host-run` byte for byte with `xtask/src/host_run.rs` (S13.2); its
      tests ported from `xtask/tests/host_run.rs`, run against both runners; `stage-run.sh` prefers
      `nzx`
- [x] 13.3 `nzx contained` (S13.3), done 2026-11-01: the workload table, `run_argv` element for element with
      `--print-argv` (and the seed's matching `--print-argv`, added: its only change), the volume key equal to Rust's,
      supervision, reaping, the tar export, the host-run lock; then `nzx contained probe` and
      `nzx contained selfbuild` pass. Measured: `tests/contained_argv.sh` 56 cases agree, 0 differ, 2036 argv
      elements compared (every workload, the profiles, `--cache bench`, the probe's six, the refusals; the plans
      that read the `linux/amd64` image refuse in both runners here, the image is not built: only `--platform
      amd64 release` was compared); the volume names printed equal Rust's for `src`, `mutate` and `bench` on the real
      image id and `Cargo.lock`, and `nzx contained cache list` equals `xtask contained cache list`;
      `contained_self.nz` 11 of 11 (interpreted); `contained_run.nz` 7 of 7 (interpreted, against the daemon: pass,
      fail(7), the export, six refusals, a symlink/directory/oversize/short file, a deadline, reaping by labels);
      `nzx contained probe` 6 of 6 ok, survivors 0; `nzx contained --profile p8t8 selfbuild` passed (stages 1 to 3
      byte-identical, 66 programs agree, 68 run as by the seed's; 848.6 s wall clock, not a measurement), survivors 0,
      no new volume (the `src` volume the Rust runner named was found by the same key). Not run: the live runs of
      `tests`, `selfhost`, `bootstrap`, `release`, `mutate` (journal collection, statuses 3), `bench`, `fuzz`, `archive`
      (their plans are compared, their scripts are the reference's until 13.4 to 13.6), anything on `linux/amd64`,
      `cache clear`, and the oversize/PAX tar paths of a real archive (only the header parser's units)
- [x] 13.4 done 2026-11-03, with a shortfall stated plainly: the comparison with the Rust campaign is over 5 entries, not the 20 asked for, because only 5 of the 94 `compiler/`/`library/`/`toolchain/` entries have every killer mapped (89 of 94 killers are whole-test names, not `FILE::fn`); widening the catalogue is a follow-up for 13.7, not done here. Measured: interpreted tests `procs_self.nz` 7 of 7, `session_self.nz` 13 of 13, `mutate_self.nz` 8 of 8, `plan_self.nz` 5 of 5, `lib_self.nz` 7 of 7 (needs `NAZM_HARNESS_ROOT`, as `tests/run.sh` sets it); `tests/contained_argv.sh` 63 cases agree, 2365 argv elements compared, 0 differ; live `nzx contained --profile p8t8 mutate` over the 5 entries (`n45-a-wrapper-drops-its-effect`, `n45-a-wrapper-needs-no-capability`, `n45-an-error-is-swallowed`, `n45-a-parse-overflows-instead-of-failing`, `n45-a-search-reads-past-the-end`): 5 caught at tier 1 (direct), 0 survived, 0 unusable, unmapped 0, restore digest equal, journal under `target/mutation-campaigns-nz/`; the Rust campaign (`nzx contained --profile p4t4 mutate-seed`, same 5): 5 caught at tier 1 by the same tests, 0 survived: the verdicts agree 5 of 5, tier for tier. The first Rust runs at `p8t8` returned UNUSABLE ("does not build (tests included)"): the build log shows `ld terminated with signal 9 [Killed]`, the linker killed by the container's 4096 MiB memory with 8 parallel Cargo jobs (a runner setting, not the campaign: at `p4t4`, 4 jobs, all 5 build; the Rust campaign was not changed); `mutate --plan` 2113 selected; `--verify-killer n45-an-error-is-swallowed --killer stdlib::option_result_and_seq_helpers_work_for_every_type` VERIFIED (one killer only) The campaign runner (S13.4): `nazm-mutate session` (entry split, killer mapping,
      `unmapped`, the warm session, `use`-closure rebuilds, three tiers, `lib/procs.nz`, journal
      `nazm.mutation-campaign/2` under `target/mutation-campaigns-nz/`, resume, lock, `recover`,
      `--verify-killer`); `nzx contained mutate` and `mutate-seed`; compared with the Rust campaign
      on 20 mapped entries
- [x] 13.5 done 2026-11-03 (S13.5 as built, `design.md`). Measured: interpreted `fuzz_self.nz` 10 of 10 (plus `procs_self` 7/7, `session_self` 13/13, `mutate_self` 8/8, `plan_self` 5/5, `lib_self` 7/7, `contained_self` 12/12, unchanged); `contained_argv.sh` 66 cases agree, 2503 argv elements, 0 differ; contained (the plan's own argv for `nzx contained --profile p4t4 fuzz 60`, image swapped for a thin layer adding `libclang-rt-19-dev`, cache volume swapped for a private one): all five targets built with SanitizerCoverage and run 60 s each, 0 crashes, 0 hangs, 0 oom (front 2770 execs/21615 covered, lower 2478/32938, run 2734/31343, lir 2530/40492, manifest 74519/1292), then `nazm-fuzz diff 120 121364944846963` exit 0 (its summary line was not captured: the script then did not print the report; now it does); `fuzz_diff.nz` (now on `lib/gen.nz`) 1 of 1 contained. Shortfalls: 60 s a target is a smoke run, not a campaign; the shared image `nazm-contained:1.98.1` was NOT rebuilt (Dockerfile edited only), so `nzx contained fuzz` as is fails at the link until it is rebuilt; diff has 3 legs (no `--backend nazm`, no Cranelift/LIR interpreter); `lir` has no LIR-interpreter step; no seeded defect was injected to prove a crash is caught (the exit paths are by reading); the lifecycle suite for `docker/` was not run; the minimised corpora were not committed. Original text: Fuzzing (S13.5), after slice 5: the image probe (and `libclang-rt-19-dev` if needed);
      `build --emit-units` (hidden); the five targets; `shim.c`; `nazm-fuzz build|run`; the
      `nazm.fuzz/1` report and exports; `lib/gen.nz` and `nazm-fuzz diff`; `nzx contained fuzz`
- [x] 13.6 done 2026-11-03 (S13.6 as built, `design.md`), verified WITHOUT any timing. Measured: interpreted `bench_self.nz` 9 of 9 (the figures of `Measurement`'s Rust test number for number; `--check` on all 22 fixed pairs of `fixtures/pairs.txt` producing exactly the text the Rust function wrote to `pairs.verdict`, boundaries at the 25 % and the 5 ms included; the schema of `bench/baseline-linux-aarch64.json`, a record the Rust bench wrote, valid; that record taken apart and rendered again byte for byte, every line but `mad_ms`, which the record rounds; seven rule-breaking records each named); `cargo test -p xtask bench::` 3 of 3; `fuzz_self` 10/10, `procs_self` 7/7, `session_self` 13/13, `mutate_self` 8/8, `plan_self` 5/5, `lib_self` 7/7, `contained_self` 13/13, `host_run_self` 14/14 (with `NAZM` set, as the suites need) unchanged; `contained_argv.sh` 67 cases agree, 2591 argv elements, 0 differ (`bench-seed` against `xtask contained bench`, `bench` as plan facts); `cargo fmt --check`, `cargo lint`, `cargo xtask check` clean. Contained (`nzx contained --profile p4t4 --cache bench run`, cold cache): `nazm-bench` built natively; with no ceiling declared it refuses, status 2; `verify`: 7 of 7 programs print their recorded answer interpreted and at -O2; `--fake-clock --runs 1` ran every stage, the self-compile, the references (C and Rust measured; Go and Python absent, recorded as not measured), 7 sizes and 14 peaks (real, from `getrusage`), wrote a record that `validate` accepts. Shortfalls: no real measurement was taken or compared (agents never time), so the harness overhead inside the timed region and the 25 %/5 ms gate on real numbers are unverified; `--save`/`--check` against a real baseline were not run (`judge` and the fixed pairs cover the verdict, the host check and the baseline path rule are by reading); `nzx contained bench` as a whole (the Cargo release build, `build.sh`, the export manifest) was not run end to end, only its pieces and its plan; the amd64 plan was not compared (image absent); `floor.expected` was fixed (it made the Rust bench's own check fail); `cpus` is `nproc` capped by the cgroup quota, not `available_parallelism`. Original text: `nazm-bench` (S13.6), verified without timing: the schema, the expected-output check,
      `--check`'s verdicts on fixed record pairs against the Rust function's
- [x] 13.7 done 2026-11-03 (S13.7 as built, `design.md`): the drafts of the runbook's changes for slices 19 and 20 (`nzx` named; `xtask`'s
      `contained` and `host-run` removed; question 5's answer recorded) are in
      `openspec/changes/r8-cut-over/runbook-draft-slices-19-20.md` (Part A slice 19, Part B slice 20; not applied to `docs/internal/runbook.md`)
- [ ] 13.8 Follow-up logged by 13.4: widen the mutation catalogue's killer names (89 of 94 entries name a whole test, only 5 map to `FILE::fn`), then repeat the Nazm/Rust campaign comparison over at least 20 entries before slice 19 (draft, last section)

## 14. Slice 14 — the tests are the contract
- [ ] 14.1 (follow-up 2026-11-04: `NAZM_RESULT_REPORT` trailing `0` from `nazm`/`nazm-pipeline` fixed (the ~43 `--json` and `check` failures), unreadable-file exit 3 fixed; 11 files re-run, rest refusals; full suite re-run 2026-11-04 after the fixes: 975 of 1471 pass, 457 fail only on it, 32 fail on the seed too, 7 not run; about 410 of the 457 are the documented refusals (Cranelift 158, nazm.root 90, --cache-report 44, lir --run 38, other targets ~35, Linux ELF archiver ~10); ~10 unexplained remain (patch not_in_compilation 4, lsp 'no message from the server' 2, `main` not returning Int 2, pipeline 'not JSON' 2); NOT closed) (first run 2026-11-04, NOT closed: 896 of 1471 tests pass on the Nazm-built `nazm`, 536 fail only on it, 32 fail on the seed too, 7 not run; classified in `design.md`, S14 as built; `toolchain/harness/tests/contract.sh`) The black-box suite against the Nazm-built `nazm`; `rewrite-in-nazm` 3.1, 3.2, 3.4
- [x] 14.2 done 2026-11-04 as a decision, shortfall stated: all 64 entries kept as the seed's (none ported through the CLI; each category's reason is in `design.md`, S14 as built); no entry was individually tried as a CLI port. `stays-rust.txt` header updated


## 15. Slice 15 — the chain on Linux
- [x] 15.1 Seed → stage 1 → 2 → 3 through Nazm's `nazm build`, the released set, contained — done 2026-11-04 with the shortfall stated in `design.md`, *S15 as built*: `cargo xtask contained --profile p4t4 selfbuild backend=llvm` (the default, released backend) on the tree after the slice 14 fixes passed in 1140.6 s: stages 1 to 3 each built the 9 programs (`nazm`, `pl`, `cg`, `fe`, `mcp`, `mutate`, `jtool`, `interp`, `check`) through the previous stage's `nazm build`; stage 0 (seed) equals stage 1 for `nazm`, `pl`, `cg`; stage 1 vs 2 and 2 vs 3: 9 of 9 identical; Core IR 9 documents identical; 66 corpus programs agree; 68 run by the self-built interpreter agree; `nazm-check` byte for byte (16 gates); the 256 MiB heap probe stopped with N0406

## 16. Slice 16 — the fixpoint on macOS
- [x] 16.0 Design (Opus): `design.md`, *Slice 16*, S16.1–S16.5 (the runbook amendment as a draft)
- [x] 16.1 `toolchain/selfbuild/programs.txt` shared by both scripts; `host-selfbuild.sh ARCH OUT`
      (S16.1): its refusals, every Nazm-built step one `host-run`, the release configuration,
      `SELFBUILD_BACKEND=nazm`, `peaks.txt` in bytes, `environment.txt`, `SDKROOT` and
      `MACOSX_DEPLOYMENT_TARGET` pinned — written 2026-11-04 and verified **without running any
      Nazm-built program**: `bash -n`, shellcheck (only the `CDPATH=` idiom `selfbuild.sh` also has),
      the refusals exercised (usage, arch, backend, `NAZM_CONTAINED`, `NAZM_ALLOW_UNCONTAINED`,
      withdrawal marker, non-empty OUT, not-Darwin by a `uname` stub, contained-workload by a `docker`
      stub, `STAGE_HEAP` over 2 GiB), and `SELFBUILD_DRY_RUN=1` for both architectures and both
      backends (27 stage builds + the corpus, Core IR, interpreter, `nazm-check` and probe steps listed
      as `would host-run`, `environment.txt` written, Rosetta and `rosetta-clang` probes answering).
      `selfbuild.sh` reads the same `programs.txt` (parse checked; its contained run not repeated)
- [x] 16.2 The comparisons (S16.2): the fixpoint whole with the localisation on a difference, Core
      IR, the corpus at `--heap-limit 256M` (programs with no counted heap counted apart), the
      interpreter, `nazm-check`, the 256 MiB probe — in `host-selfbuild.sh`; **written, not yet
      exercised** (they run only after a real build, which is 16.3): checked by reading and by
      the `peaks.txt` summary awk on sample lines. Whatever 16.3 shows may need adjusting
- [x] 16.3 aarch64: the run passes on the host (PASSED 2026-11-04 19:11-19:51, `host-selfbuild.sh aarch64`, llvm backend, exit 0, every Nazm-built step one `host-run` at 2 GiB: stages 1 to 3 9/9 byte-identical, Core IR 9/9 identical to the seed's, corpus 57 programs agree and 9 not run on the host for no counted heap (counted apart), interpreter 68/68 agree, `nazm-check` 15 gates byte for byte, the 256 MiB heap stops with N0406; largest peak `nazm` build 1527 MiB; log ~/nazm-hostsb-aarch64-2.log; the check fix 4b9b52d6 was needed) (first run 2026-11-04: fixpoint 9/9 passed, then `host-run`
      refused `fe` because the release image is stripped and the check looked for a symbol; fixed in
      `host-run` and the script's logging, design.md *S16.3 first run*; a re-run is needed)
- [x] 16.4 x86_64 under Rosetta (S16.4): the `PATH` and `rosetta-clang` probe; the run passes
      (2026-11-04: first run stopped at stage 0, the seed refused `--release` for x86_64; fixed in seed and Nazm
      `build`, see design S16.4 first run; second run stopped at stage 1: clang's default architecture under Rosetta,
      fixed by `bd_cross` in `build.nz` and the seed rebuild added to the script, design S16.4 second run; third run
      2026-11-04/05 passed: `host-selfbuild.sh x86_64 ~/nazm-hostsb-x86_64-3` exit 0, stage3 `nazm` is Mach-O x86_64, stages 1 to 3
      byte-identical, 57 corpus programs agree (9 not run on the host), 68 run by the self-built interpreter as by the seed's,
      `nazm-check` agrees, the 256 MiB heap stops with N0406 (probe peak 6.1 MiB, exit 2); largest stage `nazm` 1561.9 MiB)
- [ ] 16.5 The runbook amendment (S16.5) applied with 16.3's evidence

## 17. Slice 17 — Windows x86_64
- [ ] 17.1 The released set built by Nazm for `x86_64-pc-windows-gnu`; the host-safe suite on the
      Windows host (`windows-toolchain` W3 3.1, 3.2)

## 18. Slice 18 — the cut-over
- [ ] 18.1 The release path builds and archives the Nazm-built binaries (`rewrite-in-nazm` 10.1)

## 19. Slice 19 — the seed frozen
- [ ] 19.1 A gate on the seed's paths; `docs/internal/bootstrap.md`'s chain

## 20. Slice 20 — the authorities (`rewrite-in-nazm` 10.2)
- [ ] 20.1 `docs/internal/architecture.md`, `bootstrap.md`, `capability-matrix.md`, `support.md`,
      `licensing.md`, `runbook.md`
