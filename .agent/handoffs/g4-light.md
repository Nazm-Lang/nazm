# Handoff: v1 blockers and Gate 4 light

| | |
| --- | --- |
| Starting HEAD | `841208d` (Gate 3 complete, awaiting review) |
| Final HEAD | the commit adding this file |
| Commits | `b1b48f2` the three Q1 crash and ABI blockers · this commit, Gate 4 light |
| Working tree | clean but for `docs/prompts-from-chatgpt/` (user-owned, untracked) |
| Order | the user chose "fix blockers first", then Gate 4 **light** — not the full Gate 4 of the v4 programme |

## 1. The blockers fixed first (`b1b48f2`)

| Finding | Before | Now | Evidence |
| --- | --- | --- | --- |
| Q1-A1-01 oversized task frame | SIGBUS on the guard page: the stack check ran only in call cycles, and inside the function, after its prologue had laid out and probed the frame | a function whose own locals are a page or more (`nazm_mir::LARGE_FRAME`) is entered through a small entry that calls `nz.stack_check_frame(frame)` before calling its body (`nz.l.frame.N`); `N0408` at the function | `scheduler_pool.rs` `a_task_frame_larger_than_its_stack_fails_with_n0408_rather_than_faulting` (both backends, `-O0`/`-O2`, and recursion of 8 KiB frames); the board and wasm variants by the ignored `freestanding.rs` and `wasm.rs` cases, run by hand in `nazm-qemu:n62` and `nazm-wasm:n64` |
| Q1-A1-04 export on a small C thread | SIGBUS: the budget came from `RLIMIT_STACK` | `nz_os_stack_size` answers the calling thread's own stack left (Darwin `pthread_get_stack*_np`, Linux and Android `pthread_getattr_np`, Windows `GetCurrentThreadStackLimits`; `RLIMIT_STACK` as the fallback) | `ffi_v3.rs` `an_export_recursing_on_a_small_c_thread_fails_with_n0408_rather_than_faulting` (both backends; on macOS on the host, on Linux in the contained mutation run) |
| Q1-A1-05 wasm32 handle width | a handle, a C string result and a callback imported as `i64`; a struct holding a handle laid out with 8 bytes | **refused by name**, `N0381`, on `wasm32-*` and `thumbv7m-none-eabi`; crossing at the target's pointer width is **not built** (it needs a pointer-width type through Core IR, MIR and LIR) | `ffi_v3.rs` `a_c_pointer_on_a_32_bit_target_is_refused_by_name` |

Runtime ABI 26 → 27; the self-hosted compiler's `stack_runtime` regenerated (runtime parity). Mutants: eight
new `v1-*` and two repointed (`n49`, `n53`), **10/10 caught** in the contained runner
(`target/gates/v1/v1-mutants.log`, campaign `7247364b24ca90dc`). Host suite: 2,733 tests, only selfhost's
40 expected refusals (`target/gates/v1/host-suite-2.log`); `cargo xtask check` 23/23.

## 2. Gate 4 light — scope record

| Gate 4 item | Light: done | Deferred |
| --- | --- | --- |
| §1 competitor matrix | every planned group kept; Gate 3's evidence in the C, C#, Swift, Kotlin, Julia, Ada/SPARK, Erlang rows; CUDA/Futhark/Chapel and Solidity/Web3 rows added; each row's evidence labelled **documented capability** or **executed comparison** | — |
| §2 reference workload suite | the existing workloads stand (Gate 2 apps, Gate 3 domain workloads under QEMU, `pagestore`, `ledger`, WASI tour, mobile, desktop, EVM) | a single permanent suite runner over all fifteen |
| §3–§5 benchmarks | three programs — `floor`, `sieve`, `channels` — against C, Rust, Go, Python where a statement-for-statement reference exists (`bench/cross.py`; `performance.md`, *Gate 4 light*) | arithmetic, calls, collections, strings, filesystem, network, numeric and KV workloads; server throughput and latency; task scalability; a second machine |
| §6 compile latency | cold and warm builds of the three programs beside `clang`, `rustc`, `go build` | check, incremental edit, formatter and LSP latency on a representative project |
| §7 memory | peak RSS of each run | live set over time, allocation counts, task overhead |
| §8–§11 qualifications | not re-run: Gate 3's evidence stands (systems, assurance) | a concurrency comparison with Go/Rust under load; an AI-native workflow run |
| §12 agent evaluation | untouched: still a draft, not preregistered; no result run or published | — |
| §13 adoption audit | — | all of it |
| §14 claim audit | the public docs read for the listed words (README, `docs/*.md`); two unsupported claims scoped, README no longer reads as released, stale support and capability rows corrected | the research register's R5 heading reads as a result to a skimmer (borderline, left) |

**Why these workloads.** The C, Rust, Go and Python rows are the only ones with an installed toolchain
and a reference already written statement for statement; `channels` is the synchronisation the
concurrency model rests on and the shape a Go service lives on. Zig and Julia are **unmeasured** (not
installed); every other row is documented capability only.

**What the measurements say** (`performance.md`, *Gate 4 light*): the sieve sits within a few
milliseconds of a 3.9 ms process floor for every native language — not a compute comparison; Nazm's
channel hand-off is **about 21 times slower than Go's on this workload** (233 ms against 11 ms for
50,000 hand-offs; the interpreter within 15 % of native suggests — unprofiled, a hypothesis — that the
cost is the runtime's wait and wake rather than code generation);
builds with a warm object cache take 81–84 ms. No superiority is claimed; a result within the bench's
25 % regression threshold is a statement about Nazm against itself only.

Evidence: `target/gates/v1/bench/cross.json`, `cross.md`.

## 3. Blocker reconciliation, against the approved matrix

| Item | Classification | Source |
| --- | --- | --- |
| Real Windows x86_64 run | **required for the final gate**: Windows x86_64 is a required Tier-1 host; without machine evidence the final result is NOT ACCEPTED | `06_FINAL_V1_RELEASE_GATE.md` §12; the user's Gate 3 decision ("real Windows x86_64 execution … a required final-v1 qualification item") |
| Native x86_64 Linux suite, archive and install | **required for the final gate** (Tier-1 host) | §12; the user deferred it to final v1 |
| Tier-1 conflict | `stability.md` names three stable targets and calls `x86_64-unknown-linux-gnu` compile-only and experimental, while the final gate requires five Tier-1 hosts — **a product decision for the user**, not changed here | `stability.md` *What is stable*; §12 |
| Windows DLL, MSVC, Windows ARM64 | **not required**: `--shared` on Windows refused by name; MSVC and ARM64 unsupported by decision (SDK licence; no machine) | `support.md` *Not supported, by decision* |
| RISC-V interrupts, Cortex-M heap | **not required**: limits of experimental, emulator-verified boards, stated | `capability-matrix.md` area 18; `support.md` |
| iOS run | compile-only; final §14 says no compile-only target may be called run-verified — it stays compile-only unless a simulator runtime is installed | `support.md` |
| Storage durability | `pagestore` proves recovery after a **process** crash at every write boundary; it does **not** prove power-loss durability or `fsync` correctness (the OS cache survives a process exit) | `systems-domains.md` Part M |
| Other Q1 FIX-BEFORE-V1 items still `carried` | Q1-B-05 (`exit_with` outside 0..255), Q1-B-06 (generic symbol growth), Q1-B-07, Q1-D-04, Q1-D-09, Q1-D-10, Q1-D-11/-13/-14, Q1-D-15/-16, Q1-D-17, Q1-D-20, Q1-D-24, Q1-M-08, Q1-M-12, Q1-F-01, Q1-A2-07 — each must be closed or re-decided at the final gate (`limitations.md`); Q1-D-18, -D-19, M-01, -02 say "carried to Gate 2's close" and **no record shows them closed** — to be checked | `security-qualification.md` *Finding register* |

## 4. Publication items

- **Licence.** `LICENSE` is Apache-2.0 and in the tree; `docs/licensing.md` records an open decision —
  whether Apache-2.0 is intended for the runtime and standard-library code inside every compiled
  program. Not absent; undecided.
- **Security reporting.** `SECURITY.md` exists: private reporting by email; GitHub's private
  vulnerability reporting is unavailable while the repository is private.
- **Prompt files in history.** `4850431` added 47 planning-prompt files (`docs/Nazm_N40-N48_FULL_Exhaustive_Prompts/`,
  `docs/nazm_N49_N75_FULL_prompts/`); `d720fbb` removed them from the tree; they remain in history. Read
  for publication concerns: planning text, no credentials, keys, personal addresses or tokens found. A
  history rewrite needs the user's explicit authorisation and has not been done.

## 5. Labels kept

Windows: run-verified **under Wine** only. Android: run-verified **on a Linux kernel** only. iOS:
**compile-only**. x86_64 Linux: compile-only, deferred to the final qualification. sBPF: blocked. No
certification, hardware run or SIMD speed-up claimed.

## 6. Readiness for the final release gate

**Not ready.** The final gate requires Tier-1 evidence on Linux x86_64 and Windows x86_64 machines that
are not here, and §12 says a missing one makes the result NOT ACCEPTED; the Tier-1 list itself conflicts
with `stability.md` and needs the user's decision. Before entering it: decide the Tier-1 list; obtain the
two machines (or change the product decision); close or re-decide the carried Q1 FIX-BEFORE-V1 items;
decide licensing; and decide whether full Gate 4's deferred measurements are needed for v1 or after.
