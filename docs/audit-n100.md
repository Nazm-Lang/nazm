# The N100 audit

`nazm.audit/1` — N100, 2026-10-04 (`architecture.md` §7.101). Every row of the capability matrix
against the evidence that was executed for it, and the status that evidence supports. Nothing here
moves a row by itself: the matrix is the authority, and where this audit proposes a status, it is
the matrix's. The objective — no row PARTIAL, RESEARCH, BLOCKED or MISSING — **is not met**, and the
table says where and why.

**Candidate.** `56ddf13`, through the combined gate below, with the test fixes of `c38da8b`.

## The table

| # | Area | Status | Remaining limitation | Owner | Executed evidence | Proposed final |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Syntax, parser, CST | VERIFIED | the compiler written in Nazm's parser is N98's parity | N76 | `crates/nazm-syntax/tests/incremental.rs`, `cst.rs`; fuzzing (N93) | VERIFIED |
| 2 | Name resolution and modules | PARTIAL | re-export across modules | N77 | `crates/nazm-cli/tests/` resolution suites | PARTIAL |
| 2a | Incremental semantic checking | VERIFIED | — | N16 | incremental suites | VERIFIED |
| 3 | Type system | PARTIAL | no trait objects, generic traits or default methods | N78 | trait suites | PARTIAL |
| 4 | Memory model and reclamation | VERIFIED for the current type universe | new types bring new obligations | N43 | memory suites | VERIFIED as scoped |
| 5 | Effects | PARTIAL | no handlers, user effects or divergence | N79 | effect suites | PARTIAL |
| 6 | Capabilities | PARTIAL | the default inherits authority | N79 | capability suites | PARTIAL |
| 7 | Provenance and information flow | PARTIAL | no implicit flow; no labels a program writes | N80 | provenance suites | PARTIAL |
| 8 | Core IR | VERIFIED (v1) | — | N39 | `core_ir.rs` | VERIFIED |
| 9 | MIR | VERIFIED (v1) | — | N40 | MIR suites; MIR's validator fuzzed (N93) | VERIFIED |
| 10 | LIR and the backend contract | PARTIAL | operations still translated per backend | N81 | LIR suites | PARTIAL |
| 10a | Per-module code generation | VERIFIED | — | N24 | `native_units.rs` | VERIFIED |
| 10b | Native object reuse | VERIFIED | — | N25 | `native_cache.rs` | VERIFIED |
| 11 | LLVM backend | VERIFIED | — | — | the suite; the tier differential (N99) | VERIFIED |
| 12 | Cranelift backend | VERIFIED (v1) | the JIT BLOCKED (`unsafe` forbidden; no JIT crate offline) | N41, N99 | `cranelift.rs`; `tiers.rs`; DWARF (N91) | VERIFIED as scoped; JIT BLOCKED |
| 13 | Runtime | PARTIAL | one OS thread per task by default | N82 | runtime suites | PARTIAL |
| 14 | Structured concurrency | VERIFIED | — | N44 | `scheduler.rs` | VERIFIED |
| 15 | Advanced scheduler | VERIFIED as scoped | Linux compile-only; no stealing or preemption | N83 | pool suites | VERIFIED as scoped |
| 16 | Standard library | VERIFIED as scoped | no networking, cryptography or floats | N84 | `stdlib.rs`, API manifest | VERIFIED as scoped |
| 17 | FFI and ABI | VERIFIED as scoped | no floats, by-value structs, variadics, `dlopen` | N85 | `ffi_v3.rs` | VERIFIED as scoped |
| 18 | Embedded | VERIFIED as scoped | emulated only; atomics, interrupts, heap DESIGNED | N86 | `boards.rs` under QEMU | VERIFIED as scoped |
| 19 | Critical profile | VERIFIED as scoped | not certification | N87 | `contracts.rs` | VERIFIED as scoped |
| 20 | Cybersecurity profile | VERIFIED as scoped | constant time RESEARCH; not certification | N87 | profile suites | VERIFIED as scoped |
| 21 | AI/HPC | PARTIAL | one GPU provider; vector LIR DESIGNED | N88 | `accel.rs` | PARTIAL |
| 22 | Diagnostics | VERIFIED | — | — | diagnostic suites | VERIFIED |
| 23 | Formatter | VERIFIED | — | — | formatter suites | VERIFIED |
| 24 | Machine-applicable fixes | VERIFIED | — | — | fix suites | VERIFIED |
| 25 | LSP and MCP | PARTIAL | an editor not driven; MCP plans no workspace rename | N89 | `workspace_edit.rs` | PARTIAL |
| 26 | Package and build tooling | PARTIAL | no signatures, remote registry, features or multi-root workspaces | N90 | `crates/nazm-package/tests/registry.rs` | PARTIAL |
| 27 | Debugger and profiler | PARTIAL | no Cranelift variables, DAP or Linux sampler | N91 | `debugger_v3.rs`, live gdb | PARTIAL |
| 28 | Performance measurement | PARTIAL | eight current claims unreproduced; two regressions since N75 | N92 | `bench/claims.toml`, contained bench | PARTIAL |
| 29 | Differential testing | VERIFIED | — | N73 | `fuzz_diff.rs` | VERIFIED |
| 30 | Mutation testing / fuzzing | VERIFIED / PARTIAL | no sanitizers, Miri, Loom, backend/FFI/registry fuzzing | N93 | the catalogue; `fuzz/`, `fuzz_replay.rs` | VERIFIED / PARTIAL |
| 31 | Bootstrap | VERIFIED | the compiler written in Nazm at 13 of 21 parity probes | N98 | contained bootstrap; `compiler/parity.json` | VERIFIED; parity not reached |
| 32 | Reproducibility and provenance | PARTIAL | one host and one image; no transparency log | N94 | `attestation.rs`, `build_provenance.rs` | PARTIAL |
| 33 | Platform and target matrix | PARTIAL | x86_64 Linux compile-only; Windows a non-goal | N95 | `platform_matrix.rs` | PARTIAL |
| 34 | Smart contracts | PARTIAL | sBPF BLOCKED; no events or external calls | N96 | `contract_property.rs` under py-evm and Node | PARTIAL |

**Count.** Twenty-two rows VERIFIED (several as scoped, area 30 for mutation testing), fifteen
PARTIAL, none RESEARCH or MISSING; BLOCKED parts named inside areas 12 (JIT), 34 (sBPF) and the
formal evidence (proofs, N97). **The zero-partial objective fails.**

## The combined gate

**First run, candidate `15b6529`.** The contained workspace suite failed four tests, each a finding:
three whole-repository oracles (`cst.rs`, `symbols.rs`, `semantic.rs`) that did not know a foreign
declaration or a trait named where a type is written — constructs N98's parity probes were the first
repository sources to hold — and N95's CPU check, which held a Linux clang to Apple clang's choices.
Fixed forward (`0950cc6`, `56ddf13`). The run's continuation then filled the disk and was stopped.

**Second run, candidate `56ddf13`**, contained (`target/gates/n100/logs`):

| Step | Result |
| --- | --- |
| workspace suite (`contained --profile p4w2t4 tests`) | passed, 587 s |
| selfhost suite | passed, 96 s |
| bootstrap | passed, 37 s |
| mutation campaign, 215 mutants (every one N76–N99 added or repointed), six sessions | 198 decided: 196 caught, 1 survived, 1 unusable; stopped with 17 left |

The 17 left were the mutants whose declared killers need macOS (lldb, `sample`, Apple clang,
`ssh-keygen`) or have no declared killer; the Linux container skips the former and runs a full suite
for each. They and the two non-catches were checked on the macOS host instead
(`target/gates/precheck.py`: apply, run the declared killers, restore byte for byte), after the
fixes of `c38da8b`:

| Verdict | Count | Mutants |
| --- | ---: | --- |
| caught, contained | 196 | — |
| caught, on the host by its declared killer | 9 | n48 (second killer), n85 ×2, n86, n91 ×2, n94 ×3 |
| caught after a test was strengthened | 4 | n84 carriage return, n91 Mach-O addresses, n77 occurrence order, n82 runtime beside the artifact |
| caught after the mutant was repaired | 1 | n80 traces (its replacement did not build) |
| **survived** | 1 | `n79-a-held-io-cap-is-read-alone`: no program found that reaches an ambient call needing `out` alone; possibly equivalent |
| no verdict: suite tier, no declared killer | 4 | every-definition-is-exported, a-record-drop-does-not-recurse, task-safety-stops-at-the-first-level, lowered-propagation-falls-through-on-error |

**Resolved by N101 (2026-10-05).** The survivor is reachable — the probe above ran in a directory
holding a warm check cache — and is caught by `authority::a_held_io_cap_authorises_its_own_print`
in a fresh directory. The four without a verdict are caught by focused killers (`architecture.md`
§7.102). The table stands as what N100 observed.

Five mutants were caught only after the tests or the mutant changed — each is a finding, recorded in
`c38da8b`. Zero survivors is not claimed: one survives and four were not run to a verdict.

**Container-backed runs** on the final tree, all passed: live gdb on both backends (N58, N91), the
QEMU AArch64 image (N62) and both boards (N86), WebAssembly (N64, N71), the EVM (N70), the contract
properties on py-evm and Node (N96).

**Not run here, and why.** Release assembly and an independent re-digest of a release archive: N100
produces a candidate, not a release. Coverage-guided fuzzing at release scale: N93's campaign stands
(3.9 M executions); not rerun. Proof checks: no proof assistant exists here (N97).

**Human procedure for a release.** From a clean checkout of the candidate: `cargo xtask check`;
`cargo xtask contained tests`, `selfhost`, `bootstrap`; `cargo xtask contained mutate` over the
catalogue; the ignored container tests; `cargo xtask contained bench --save`; then the release script
of N75. Nothing is tagged, pushed or published by this programme.
