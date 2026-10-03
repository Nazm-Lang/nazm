# Release-candidate notes — `cf664fd` (post-v1, N49–N75)

**Frozen evidence for one tested commit.** This file records what was verified at
`cf664fd45e4920913550f21a10130f34c5c45a6f` and is not updated as the project moves. Current
implementation truth is [`../capability-matrix.md`](../capability-matrix.md); what is not done is
[`../limitations.md`](../limitations.md).

**Nothing has been tagged, pushed or published.** This is a candidate. Making it a release is a
person's decision, and the procedure for it is at the end of this file.

## What this candidate is

Nazm after the post-v1 programme, N49–N74, audited in N75: the v1 foundation of
[`84d3c80.md`](84d3c80.md) plus function values, closures and effect parameters (N50, N51), resource
and information-flow facts (N52), the runtime as its own crate with typed channels (N53), select,
deadlines and cancellation (N54), C strings and exported libraries (N55), a local package registry
(N56), cross-compilation to six targets (N57, N62, N64), a live debugger and a profile (N58), SSA
temporaries in the LLVM emitter (N59), restriction profiles v2 (N60, N63, N69, N72), a formal core under
bounded model checking (N61), a freestanding board target and real-time bounds (N62, N63),
WebAssembly (N64), a REPL, comptime and reload checks (N65), a vectorised summation (N66), a GPU map
kernel (N67), field-by-field vector layout (N68), contracts — a chain-neutral model, an EVM backend, a
WebAssembly adapter and account metadata (N69–N72) — seeded fuzzers, build provenance and an evidence
bundle (N73), and project templates, API docs, C-header bindings and an editor client (N74).

## Identities

| | |
| --- | --- |
| Commit | `cf664fd45e4920913550f21a10130f34c5c45a6f` — differs from `34e2380`, where the gate's stages ran, by one declared killer in the mutation catalogue (`53c6ef7`) and the release script's copy of the conformance corpus (`cf664fd`); no compiler, runtime or test source |
| Toolchain | rustc 1.98.1, pinned in `rust-toolchain.toml` |
| Semantic epoch | 20 (`crates/nazm-cache/src/identity.rs`) |
| Interface schema | `nazm.interface/7` |
| Runtime ABI revision | 10 (`crates/nazm-runtime/src/lib.rs`) |
| Cranelift | 0.136.1, pinned exactly (`crates/nazm-codegen-clif/Cargo.toml`) |
| Package manifest / lock / registry | `nazm.package/1` / `nazm.lock/1` / `nazm.registry-index/1` |
| New machine documents since `84d3c80` | `nazm.cost/1`, `nazm.flow/1`, `nazm.profile/1`, `nazm.bounds/1`, `nazm.accel/1`, `nazm.reload-check/1`, `nazm.contract/1`, `nazm.contract-run/1`, `nazm.evm/1`, `nazm.evm-abi/1`, `nazm.evm-storage/1`, `nazm.evm-upgrade/1`, `nazm.evm-run/1`, `nazm.wasm-contract/1`, `nazm.wasm-contract-run/1`, `nazm.sbpf-accounts/1`, `nazm.provenance/1`, `nazm.evidence/1`, `nazm.sbom/1`, `nazm.api-doc/1`, `nazm.bindgen/1`, `nazm.publish-plan/1`; `nazm.capabilities/1` gained three optional lists |
| Restriction profiles | `general` (none); `embedded` (no-io, no-spawn, bounded-allocation, no-recursion); `critical` (declared-effects, no-spawn, no-foreign, locked-build, no-declassification, no-ambient-time, no-select); `cyber` (declared-effects, no-foreign, locked-build, contents-stay-in-files); `realtime` (no-io, no-spawn, bounded-allocation, no-ambient-time, no-recursion, no-blocking, bounded-loops); `web3` (declared-effects, no-io, no-spawn, no-foreign, no-ambient-time, no-recursion, no-blocking); `accounts` (`web3`'s and signed-writes) |

## Targets, and what was run on each

Compile-only and run-verified are kept apart; a self-skip is not evidence.

| target | backends | evidence at this candidate |
| --- | --- | --- |
| `aarch64-apple-darwin` | LLVM, Cranelift | run-verified on the host (macOS, Apple clang 21.0.0): the host suites and the nine platform runs below |
| `aarch64-unknown-linux-gnu` | LLVM, Cranelift | run-verified inside the contained runner (Debian clang 19.1.7): the whole workspace suite, selfhost and bootstrap |
| `x86_64-apple-darwin` | LLVM, Cranelift | built on the host and run under Rosetta by the suite (`a_program_for_the_other_macos_architecture_runs_where_rosetta_does`) |
| `x86_64-unknown-linux-gnu` | LLVM, Cranelift | compile-only: ELF x86-64 objects; no amd64 machine or emulator here |
| `aarch64-unknown-none` | LLVM | run-verified on QEMU's `virt` board (`nazm-qemu:n62`): the image boots and reports on the UART; a run stays inside the stated stack bound |
| `wasm32-unknown-unknown` | LLVM | run-verified under Node 20 (`nazm-wasm:n64`): the corpus agrees with the interpreter; an absent authority is an absent import; the contract adapter agrees with the simulator |
| EVM bytecode (contracts) | direct from Core IR | run-verified in py-evm 0.12.1b1 (`nazm-evm:n70`): agrees with the simulator, inside its gas bounds |
| GPU (OpenCL, Apple M1 Pro) | `nazm accel` | run-verified on the host: a kernel agrees with the interpreter; a failure is the lowest failing index |
| sBPF / SVM | — | BLOCKED: no toolchain, validator or emulator; account metadata only |

**Reproducibility.** One program, one toolchain, two clean directories: byte-identical executables
under both backends (N46) and one provenance record (N73). Across toolchains, for
`aarch64-unknown-linux-gnu`: Apple clang 21 and clang 19 give byte-identical program and entry
objects at `-O0` and `-O2`; the runtime object differs — one symbol-table byte at `-O0`, code layout
at `-O2` (N73's measurement).

## The gate

Contained (Docker, offline, `nazm-contained:1.98.1`, the runner image's compatibility key
`6a1d76f8…`), 2026-10-02/03, from a clean worktree: every stage at `34e2380`; at `53c6ef7`, whose only change is one mutant's declared
killer, that mutant and the checks; at `cf664fd`, whose only further change is the release script, the
release assembly — which itself runs formatting, lints, the architectural gates, the whole workspace
suite, the release build and the bootstrap chain again at that commit.

| Stage | Result |
| --- | --- |
| Workspace suite (`p4w2t4 tests`) | passed, 651 s: 2,249 passed, 0 failed, 38 ignored, 172 suites |
| Lifecycle (`xtask --test lifecycle --ignored`) | passed, 188 s |
| Self-hosting (`contained selfhost`) | passed, 75 s |
| Bootstrap C1 → C2 → C3 | passed, 37 s |
| Compiler benchmark (`contained bench`) | measured, 396 s; see *Performance* |
| Mutation, full catalogue | composed, below: 1,089 of 1,089 caught |
| `xtask check`, the docs and counts tests | passed at `53c6ef7` |
| Release assembly (`contained release`) | passed at `cf664fd`, 2,303 s: fmt, lints, `xtask check`, `cargo test --workspace`, release build, bootstrap C1 → C2 → C3 byte-identical, 188 files verified against `MANIFEST.txt` |

Beside it, on the host at this commit: the nine platform runs of the table above (all passed); the
seeded fuzzers at release scale — the front end over 20,000 mutated sources, the differential tiers
over 100 programs of seed 75 — clean; the formal core's seven properties (`nazm-formal`) hold to
their bound. These are separate classes of evidence and are reported apart.

**What the gate and the audit found, every item fixed before the candidate** (commit, then what):

- `cf664fd` — **the release script could not assemble a candidate**: `compiler/conformance` has held the
  directories `modules/` and `refused/` since N14E, and the script's plain `cp` of its entries fails
  under GNU `cp` with `set -e` — found by the first assembly to reach that step. Copied with `tar`,
  recursively and without `.nazm` caches: 107 files, as tracked.

- `53c6ef7` — the full catalogue's one non-verdict: `a-channel-goes-while-another-reference-remains`
  (a channel's storage released while a task still holds it) had no declared killer; in the workspace
  suite a task waited for ever and the tier timed out at 2,400 s. The runtime harness test
  `the_runtime_s_services_keep_their_contract_when_called_directly` observes the early release under
  its own deadline in seconds; declared, and verified contained: pristine passes, mutant fails,
  restored passes.
- The harness, not the tree: the last sessions needed more than the runner's fixed 3,000 s — each
  began with the verification cache over its 16 GiB cap and emptied, a cold build and the pristine
  suite, and the next mutant needed the whole suite — so the same mutant was restarted every session.
  Sessions 24 on ran under a supervisor built from `34e2380`'s `xtask` with two constants changed (a
  7,200 s deadline, a 40 GiB cap) and the unmodified tree mounted, so the campaign's identity and
  journal carried over; the release assembly likewise ran with a 4,800 s deadline and
  `--skip-bench` (its benchmark is the gate's own stage), after the stock 2,400 s run reached its
  deadline in that step having passed the suite, the release build and the bootstrap chain.

- `34e2380` — the release audit: the capability matrix's summary still counted N48's rows (one
  MISSING); `limitations.md` said "no `select`" beside N54's select, "four targets" beside six, and
  "eleven rules, no bounded-loop rule" beside fourteen with one; N73's and N74's four schemas were not
  documented in `docs/diagnostics.md` — found first by the N74 workspace run's docs suite.
- Earlier in the programme, each composed gate's survivors and the fixes they forced are recorded in
  the milestone reports (Gate C 163/163; Gate D's one survivor, `n46-a-cycle-is-not-noticed`, killed by
  `18eb20e`; Gate E 99/99).

## Results

`cargo test --workspace`, run through `cargo xtask contained` on Linux at `34e2380`, the gate's
workspace stage (and again, passing, inside the release assembly at `cf664fd`):
**2249 passed, 0 failed, 38 ignored.** Across 172 suites.

| | count | what it establishes |
| --- | ---: | --- |
| Other integration suites (`nazm-cli/tests/*.rs` beyond those below, `nazm-core`, `nazm-mcp`, `nazm-repo`, the benches, `xtask`'s evidence suite) | 1150 | each milestone's end-to-end behaviour: modules, packages and the registry, profiles, FFI, the runtime, the scheduler, the standard library, the cache, the debugger and profiler, the machine interfaces |
| Unit tests and doc-tests in the crates | 409 | each crate's local invariants, the harness's own included |
| Language service (`nazm-service`) | 218 | references, rename, completion, hierarchy, context packets and the rest, across a package boundary since N74 |
| Native end-to-end (`nazm-cli/tests/build.rs`) | 135 | every case compiled at `-O0` and `-O2` and compared with the interpreter |
| Language semantics (`nazm-core/tests/language.rs`) | 116 | each rule in `docs/spec.md` a refactor could quietly reverse |
| Post-v1 targets and tiers (N62–N72: board, real-time, WebAssembly, contracts, accelerators, SIMD, layout, the interactive tier) | 58 | what runs everywhere of each; their platform runs are the ignored tests above |
| Syntax, CST, grammar and formatter (`nazm-syntax`) | 48 | the lossless tree, the grammar against the parser, the formatter's laws |
| Self-hosting differentials (`nazm-cli/tests/selfhost.rs`) | 38 | the Nazm-written compiler against the reference |
| Trust and ecosystem (N73–N74: fuzzers, provenance, templates, API docs, bindings, schemas) | 35 | the fuzzers' properties, the build record, every template end to end, every machine document against its schema |
| MIR (`nazm-cli/tests/mir.rs`) | 35 | the validator's laws and MIR against Core IR |
| Formal core (`nazm-formal`, N61) | 7 | five properties of the small core and its bound, by exhaustive enumeration |

## Mutation catalogue accounting

Every one of the catalogue's **1,089** entries has a verdict, and every verdict is **caught**:

| | |
| --- | --- |
| catalogue total | 1,089 |
| accounted | 1,089 |
| caught | 1,089 — 908 by a declared killer, 175 by the owning profile's tests, 6 by the workspace suite |
| survived | 0 |
| unusable | 0 |
| mutant crash | 0 |
| timed out | 0 (one at `34e2380`, given a killer and caught at `53c6ef7`) |
| not injected | 0 |
| runner error | 0 |
| unexecuted | 0 |

**Composed, and how.** 1,088 verdicts at `34e2380`, one campaign (journal `db2d0899a035bcb8`) across
24 resumed sessions, each verdict decided once and reused thereafter; and 1 at `53c6ef7`, the mutant
that timed out, with its killer verified in that session. Every killer that decided a verdict was
verified — pristine passes, mutant fails, restored passes — in the session that decided it. No
verdict was reclassified. The journals are kept with the gate's logs.

## Performance at this candidate

[`../performance.md`](../performance.md), *The post-v1 baseline — N75*: the contained figures at this
candidate, which replace N48's as the reference, and host figures for the scheduler, packages, a
contract's gas bound and the freestanding images, each scoped to how it was measured. No universal
score; nothing here compares Nazm with another language.

## Known limitations

[`../limitations.md`](../limitations.md), by area, audited in N75. The ones a user meets first: one OS
thread per task, so the tasks alive at once are bounded by the host's thread limit; no traits or
methods; the foreign boundary passes `Int`, `Bool` and borrowed strings only; a local registry only;
the freestanding and WebAssembly targets refuse the heap; contracts run on one EVM implementation and
under one reference WebAssembly host; fuzzing is seeded, not coverage-guided; verified on aarch64
hosts and the emulators named above only.

## Evidence index

Kept outside the repository with the gate's working files, as an evidence bundle of 48 files indexed
by `cargo xtask evidence` (`nazm.evidence/1`, BLAKE3 per file; `index.json` SHA-256 `2f1dc77eafbcf9414664fe4de3d7e8834070ac8eda41c0462e48156b7cdec0da`) and
verified by `cargo xtask evidence --verify`:

| | |
| --- | --- |
| `release-candidate/` | `nazm-release-candidate.tar.gz` (SHA-256 `2ef978a91c76a894a9610352af2f4f3ad7a4026d3ca921a1511c0f32026d9b2d`), `MANIFEST.txt` (SHA-256 `8f1be533421232faae92a76ec1cb3b91eaf888041ff6f780d1c282f5a3a286cc`), `PROVENANCE.txt` |
| `gate/logs/` | every contained stage's log, the release assembly's, and the gate's timeline |
| `gate/mutation/` | all 26 sessions' logs and both campaign journals (`db2d0899a035bcb8`: 1,088 caught and the one timeout; `b34a1c6775310a50`: that mutant caught) |
| `gate/bench-record.json` | the benchmark stage's record |
| `platform/` | the nine platform runs, the fuzzers at release scale and the formal core, the host measurements |
| `reproducibility/` | N73's cross-toolchain measurement |
| `sample/` | a `cli` template built with `--provenance` and an attestation from a local `shasum` |
| `sbom.json` | the compiler's bill of materials: 261 crates from `Cargo.lock` and the toolchain pin |

**Independently verified on the host** (macOS, `shasum` and Python's `hashlib`, not the tool that made
them): the archive extracted, every file's SHA-256 recomputed — 188 listed, 188 present, none
mismatched, missing or unlisted.

## The human release procedure

Every step is a person's, in this order, from a clean checkout of exactly this commit. Stop at the
first step that does not pass.

1. **Check out the commit.** `git checkout cf664fd45e4920913550f21a10130f34c5c45a6f` and confirm
   `git status --porcelain` prints nothing.
2. **Build the images** (network needed here and nowhere after): `docker build -f
   docker/contained.Dockerfile -t nazm-contained:1.98.1 .`, and for the platform runs
   `docker/qemu.Dockerfile` (`nazm-qemu:n62`), `docker/wasm.Dockerfile` (`nazm-wasm:n64`),
   `docker/evm.Dockerfile` (`nazm-evm:n70`) and the debugger image (`nazm-debug:n58`).
3. **Run the gates, contained and offline**, each of which must exit 0:
   - `cargo xtask contained --profile p4w2t4 tests`
   - `cargo xtask contained --profile p1 run "cd /src && cargo test -p xtask --test lifecycle -- --ignored --test-threads 1"`
   - `cargo xtask contained selfhost` and `cargo xtask contained bootstrap`
   - `cargo xtask contained bench` (compare against *The post-v1 baseline — N75*)
   - `cargo xtask check` and `cargo test -p nazm-cli --test docs`
   - the mutation campaign: `cargo xtask contained --profile p4t4 mutate --strategy targeted --verify-killers`,
     resumed with `--resume` until it completes; it must report 0 survived and no mutant without a verdict.
4. **Run the platform tests on the host** (Docker for the images, macOS for the GPU):
   `cargo test -p nazm-cli --test accel --test freestanding --test realtime --test wasm --test wasm_contract --test evm --test debugger -- --ignored`.
5. **Assemble the local candidate:** `cargo xtask contained release` — the stock runner gives it
   2,400 s, which one CPU did not fit with the benchmark comparison; run it with a longer deadline or
   with `--skip-bench` passed to `scripts/release-candidate.sh` (step 3's bench is the measurement), as
   this candidate's was. It runs every gate again, builds the artefact under
   `target/`, writes `MANIFEST.txt` and `PROVENANCE.txt`, and verifies the artefact against the
   manifest. It tags, pushes and uploads nothing.
6. **Verify independently on the host:** extract the artefact and compare every file's SHA-256 with
   `MANIFEST.txt` (counts must match both ways); then `cargo xtask evidence --verify DIR` over the
   evidence bundle, which recomputes every BLAKE3 digest of `index.json`.
7. **Decide.** Only then, and only by a person: tag (`git tag -a v2.0.0-rc.1 cf664fd -m "…"`), push the
   branch and the tag, and publish the artefact with this file beside it. None of this has been done.
