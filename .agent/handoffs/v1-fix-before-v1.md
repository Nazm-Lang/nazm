# Handoff: the FIX-BEFORE-V1 inventory

| | |
| --- | --- |
| Starting HEAD | `1028af1` (Gate 4 light) |
| Final HEAD | the commit adding this file |
| Working tree | clean but for `docs/prompts-from-chatgpt/` (user-owned, untracked) |
| Order | the reviewer's: an auditable inventory first, then each correctness defect reproduced, its intended behaviour taken from the spec, fixed at the root with a regression test across backends, levels and targets, mutants where they establish detection, in reviewable commits |

## The inventory, and how each item closed

Built from the repository — `docs/security-qualification.md`'s finding register, `limitations.md`
*Carried from Q1*, the Q1 audits under `target/gates/q1/`, and the tree — not from summaries. An item
is closed only with evidence below. Statuses: **fixed** (code, with a test), **corrected**
(documentation, with the text that now holds), **refused by name** (an unsupported operation
rejected; the capability itself not built), **decision** (waiting on a person).

| ID | What was wrong | Status | Commit | Evidence |
| --- | --- | --- | --- | --- |
| Q1-A1-01 | a frame larger than the stack left faulted on the guard page | fixed | `b1b48f2` | `scheduler_pool.rs` frame test (both backends, `-O0`/`-O2`); ignored `freestanding.rs`, `wasm.rs` cases, in `ignored.log`; mutants |
| Q1-A1-04 | an export on a small C thread faulted | fixed | `b1b48f2` | `ffi_v3.rs` small-thread test (macOS; Linux in the contained mutation run) |
| Q1-A1-05 | a C pointer crossed as `i64` on 32-bit targets | **refused by name** (`N0381`); **pointer-width support not implemented** | `b1b48f2` | `ffi_v3.rs` — every pointer crossing refused on `wasm32-*` and `thumbv7m`; `Int`, `Bool` and a C struct of them still build on `wasm32-wasip1` |
| Q1-B-05 | `exit_with` outside 0..255 ended differently per engine | fixed | `6a2350c` | `spec.md` *Ending*: the language's status is `0..=255`, the host observes exactly that; outside it `N0416` and status 2 everywhere. `exit_status.rs` (interpreter, LIR oracle, LLVM `-O0`/`-O2`, Cranelift, a task); selfhost case (contained) |
| Q1-B-06 | a 16-deep generic got a 2.4 MB symbol; ld asserted | fixed | `78fbe97` | `generics.rs` builds and runs it under both backends, every `define` line under 1 KiB |
| Q1-B-07 | the LIR oracle refused float and `UInt64` text | fixed | `0341dd5` | `lir_oracle.rs` edges held to LLVM and Cranelift; the `numbers` compat case byte for byte |
| Q1-D-04 | VERIFIED sub-claims on ignored tests with no kept log | fixed for this tree | `8e1fdc6` | 38 ignored verification tests run at `af57a42`, 38 passed, `target/gates/v1/ignored.log`, cited in matrix area 33 |
| Q1-D-09 | an unreadable root reported at a prelude line | fixed | `8d641bd` | `exit_contract.rs` — missing, not UTF-8, unreadable import; text and JSON |
| Q1-D-10 | closed schema files vs the additive law | corrected | `15a1247` | `stability.md` *Closed schema files* |
| Q1-D-11 | no code registry; a distinctness test of 25 | fixed | `5e72f51` | `diagnostics.md` registry of 153 codes, held by `codes.rs`; `every_code_is_distinct` reads every declaration |
| Q1-D-13 | `N0101` a catch-all | corrected; **classification is a decision** | `5e72f51` | named the umbrella code in the registry; the audit called it ACCEPTED-LIMITATION, the register FIX-BEFORE-V1 |
| Q1-D-14 | reserved codes | corrected; **classification is a decision** | `5e72f51` | `N0620` reserved in the registry and `lib.rs`; the audit called it POST-V1 |
| Q1-D-15 | deprecation promised as a warning with no warning severity | corrected | `15a1247` | `stability.md` *How a change is made* §3 |
| Q1-D-16 | strict input readers undocumented | corrected | `15a1247` | `stability.md` *Inputs read strictly* |
| Q1-D-17 | no compat case for epoch 34 or `N0359` | fixed | `5e72f51` | `run/imported-impl/`, `refuse/vec-ownership-cycle.nz` (and `trap/exit-status-outside.nz`) |
| Q1-D-18 | `stability.md` omitted `Bytes`/`Text` (*"carried to Gate 2's close" — not done there*) | corrected | `15a1247` | stable list and memory contract |
| Q1-D-19 | Gate 2 untested in formatter, provenance, validators (*not done at Gate 2's close*) | fixed | `af57a42` | `format.rs`, `provenance.rs`, `mir.rs`, `core_ir.rs` |
| Q1-D-20 | the self-hosted compiler's scope unsaid in areas 29, 31 | corrected | `15a1247` | both areas |
| Q1-D-24 | stale text in VERIFIED rows | corrected | `15a1247` | each line found by its text and dated or corrected |
| Q1-A2-07 | the contained runner's isolation untested | fixed | `4c3c5e5` | `run_argv`; `a_contained_run_is_isolated_by_its_arguments`; three mutants |
| Q1-M-01 | `Bytes`/`Text` absent from the memory tables (*not done at Gate 2's close*) | corrected | `15a1247` | `spec.md` four kinds, *What is not promised*; area 4 |
| Q1-M-02 | `Bytes`/`Text` reclamation held by one program (*not done at Gate 2's close*) | fixed | `15a1247` | `a_buffer_and_a_text_are_reclaimed_in_every_shape_on_every_path` |
| Q1-M-08 | a warm check named a parameter `#1` | fixed | `8d641bd` | `incremental.rs` compares whole again |
| Q1-M-12 | native deadlines on the wall clock | fixed | `c3a8153`, `ea0530c` | `select.rs` steps the wall clock back an hour under both backends and schedulers; the pre-fix build hangs under the same shim |
| Q1-M-17 | a parked select's wake not forced by tests | fixed | `c3a8153` | handshakes; the never-woken mutant caught by both tests, run by hand |
| Q1-F-01 | fuzz hangs counted, not kept | fixed | `0341dd5` | `OUT/hangs/`; the fuzz crate's test. Q1's 36 are not recovered |

Duplicates reconciled: Q1-D-18 and Q1-M-01 are one gap (the memory contract), closed together.

## Gates on the resulting tree

Evidence under `target/gates/v1/`. Host refusals are kept apart from passes: selfhost refuses to
run outside the container by design, so a host suite's selfhost "failures" are refusals, not tests
that ran and failed.

| Check | Commit | Result | Log |
| --- | --- | --- | --- |
| Host suite, every batch | each batch's commit | only selfhost's host refusals (40, then 41 once a selfhost case was added); doc tests pass; `cargo xtask check` 23/23 and `cargo fmt --check` clean before each commit | `host-suite-b1.log` … `host-suite-b8.log`, `host-suite-lic.log` |
| Host suite, final | `779c54c` | 2,751 run: 2,710 passed, **41 selfhost host refusals**, 0 other failures; doc tests pass | `host-suite-final.log` |
| Contained workspace suite | `8e1fdc6` | passed: 2,736 passed, 0 failed (1,687 s) | `contained-tests.log` |
| Contained selfhost | `8e1fdc6` | **44/44**, including `the_nazm_written_compiler_holds_exit_with_to_0_to_255` | `contained-selfhost.log` |
| Contained bootstrap | `8e1fdc6` | passed | `contained-bootstrap.log` |
| Ignored verification tests | `af57a42` | 38/38 (QEMU, wasm/WASI, Wine, EVM, GPU, Bionic, Cocoa, gdb) | `ignored.log` |
| Lifecycle | `779c54c` | 19/19 under `CARGO_TARGET_DIR` (see the harness fix below) | `lifecycle-4.log` |
| Mutants, `b1b48f2`'s | `b1b48f2` | 10/10 caught | `v1-mutants.log` |
| Mutants M1 | `8e1fdc6` | 9/10 caught at tier 1; `v1-the-pool-reads-the-wall-clock` survived its killer and ran out the session at the workspace tier | `v1-mutants-m1.log`, `v1-mutants-m1-resume.log` |
| Mutants M2 | `ea0530c` | 5/5 caught at tier 1, including `v1-the-pool-reads-the-wall-clock` against its new killer | `v1-mutants-m2.log` |
| Mutant `r1-the-digest-forgets-the-lockfile`, repointed | `ea0530c` | caught at tier 1 | `v1-mutants-r1.log` |

**Found by the gates, and fixed:**

- `v1-the-pool-reads-the-wall-clock` survived the stepped-clock test: with the pool's clock wrong,
  select and sleep still end on time — they look again on the monotonic clock — but a parked task is
  woken at once and parks again, so its worker spins. `a_pool_task_waiting_on_a_deadline_parks_without_spinning`
  measures CPU time in a four-second wait (2.66 s under the mutant, by hand); the mutant's killer is
  repointed to it (`ea0530c`).
- The lifecycle suite failed ten of nineteen under `CARGO_TARGET_DIR=~/.cache/nazm-target-g2` — and
  nine at Gate 3's `841208d`, where the same command had passed: its fixture campaigns inherited the
  target directory and, run in parallel, read one another's test binaries. A harness fragility, not
  a product change; fixed in `779c54c` (each fixture builds in its own target).

**Also committed in this span:** the licence decision, `Apache-2.0 WITH LLVM-exception`, copyright
Ferdous Alam (`d4c72c5`, `docs/licensing.md`).

## Outstanding

- **The full release mutation run** — the whole catalogue — is not executed; it belongs to the final
  release gate. Only the mutants added or repointed here were run (above).
- **Decisions:** the Tier-1 platform list (`stability.md`'s three stable targets against the final
  gate's five required hosts) — or a macOS-first 0.x release, which the user is considering;
  Q1-D-13's and Q1-D-14's classification; whether the commit `4850431` prompt files are removed from
  history; the licensing items still open in `docs/licensing.md` (contributor terms, third-party
  notices in archives, templates, SPDX headers, a spec licence, a trademark policy).
- **Machines:** a Linux x86_64 and a Windows x86_64 host for the final gate's Tier-1 runs.
- Not FIX-BEFORE-V1 and not touched: the POST-V1 and ACCEPTED-LIMITATION rows of the register.
