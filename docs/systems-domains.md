# Systems, platforms and domains — Gate 3 decision record

*Gate 3 of the v1 programme, 2026-10-10, from the accepted baseline `3a2e3ba` (toolchain 1.0.0,
semantic epoch 43, `nazm.interface/11`, runtime ABI 20, standard library API 1.1). This file is the
design written before the code, as `general-purpose.md` was for Gate 2: each part states the problem,
what Nazm already has, the mechanism chosen and why it is not a second mechanism for something the
language already owns, what will count as evidence, and what is not claimed.
[`capability-matrix.md`](capability-matrix.md) decides what exists; where this record and the matrix
disagree, the matrix is right and this record is history. [`v1-domain-matrix.md`](v1-domain-matrix.md)
is where every v1 domain claim points.*

## What Gate 3 starts from

A survey of the tree at `3a2e3ba` (sources cited in each row):

| Area | At `3a2e3ba` |
| --- | --- |
| Hosts | run-verified, both backends: aarch64-apple-darwin, x86_64-apple-darwin **under Rosetta**, aarch64-unknown-linux-gnu (container); x86_64-unknown-linux-gnu **compile-only**; Windows **unsupported** — no PE/COFF, linker, SDK, runtime port or machine (`nazm_lir::backend::SUPPORT`, `support.md`, `cross.rs`); no per-target prebuilt toolchain (the release candidate archives one host) |
| Freestanding | two emulated boards — AArch64 virt Cortex-A53, RV64GC M-mode — from the `BOARDS` table: generated `_start`, `link.ld`, UART output, a stop mechanism, a static stack bound (`board.rs`, `freestanding.rs`, `boards.rs`, N62, N86); LLVM only |
| Low level | volatile device registers through `mmio_*` under `MmioCap` (N62, N86); `extern "C" struct` by pointer (N85); fixed widths, floats, bits, arrays, constants (Gate 2); **no** atomics, interrupts, statics, sections, custom entry, heap or allocator on a board, user layout control |
| Profiles | eight, in `nazm_service::profile`: `general`, `embedded`, `critical`, `cyber`, `realtime`, `web3`, `accounts`, `authority`; 17 rules; an `unknown` verdict refuses (N47, N60, N63, N87) |
| Realtime | `realtime` = `embedded` + `no-ambient-time`, `no-blocking`, `bounded-loops` (one counted `while` form); a static stack bound in `bounds.json`; no deadlines, priorities or WCET (N63) |
| Assurance | contracts checked everywhere (`N0410`/`N0411`), `nazm obligations` with three statuses, a bounded model check of the formal core; obligations keyed by name and position, not by durable identity (N87, N61, N97) |
| Supply chain | `--provenance`, `--sbom`, `nazm attest`, locked builds (N73, N94, N46) |
| AI/HPC | `nazm accel`: `Int` map/zip/reduce on one provider (macOS OpenCL), held to the interpreter as oracle; refused rather than run on the CPU; no floats in kernels (N67, N88) |
| Storage | `@std/file` with `fsync` and atomic replace; `examples/apps/kvstore` (Gate 2); no mmap, locking or direct I/O |
| Distribution | `@std/net` TCP/UDP with deadlines; `select`, cancellation by closing a channel (N54); no node identity, transport abstraction or retry semantics |
| WASM | `wasm32-unknown-unknown`, one host (Node), no heap — 14 of 64 corpus programs run, 50 refused by name (N64, N71); no WASI |
| Mobile, desktop | C-ABI static and shared libraries with a header (N55, N85, Gate 2 §23); nothing for Android, iOS or a GUI host |
| Web3 | chain-neutral contract model and simulator, EVM bytecode, WASM contracts, `accounts`; 750 transactions agree across three (N69–N72, N96); sBPF blocked |
| Traceability | durable `DefKey` identities, spec anchors, matrix paths; no requirement identity or requirement → test trace |

The host for this gate is an Apple M1 Pro (macOS 27.2) with Docker, Rosetta and QEMU. What it can
run, and therefore what can be *run-verified* here, decides several parts below.

## The rules every part follows

**One semantic language; profiles restrict, never reinterpret.** A domain is a profile — a set of
refusals over the one language — plus a target contract and a reference workload. No domain gets its
own semantics, and no profile makes a construct mean something else.

**Platform behaviour lives in the runtime's platform layer** (`nazm-runtime`'s `platform`, `board`
and `wasm` units, and the backend's target description), never in a semantic layer: the checker,
Core IR and MIR do not learn which operating system they target. A target is a row of data —
`Facts` for a hosted operating system, a board record for a freestanding one — and generated text
from it.

**Prefer the mechanism that exists.** A board manifest before a new language construct; an
attribute from the closed vocabulary before new syntax; a profile rule before a new profile kind; a
capability for authority and an effect for behaviour, as everywhere else.

**Evidence is labelled by where it ran.** `support.md`'s vocabulary is kept and extended, and no cell
or matrix row says "supported" alone:

| Label | Means |
| --- | --- |
| run-verified (native) | generated programs executed on that operating system and architecture, natively |
| run-verified under Rosetta | x86_64 macOS programs executed through Rosetta 2 on Apple silicon |
| run-verified, emulated host | Linux programs executed in a container of that architecture under emulation (x86_64 on Apple silicon) |
| **run-verified under Wine** | Windows-target PE executables executed under Wine in a Linux container — **not** evidence from a Windows host |
| emulator-verified | freestanding images booted under QEMU |
| compile-only, blocked, unsupported, non-goal | as `support.md` defines them |

**Nothing is claimed that did not run.** "OS-ready", "hard real-time", "certified", "qualified",
"DO-178C", "ISO 26262", "IEC 62304", "sandboxed" and "non-interference" are never claimed; a domain
row says what its workload proves and what it does not.

---

## A. Host installations

**Problem.** v1 names five Tier-1 hosts; today two run natively, one under Rosetta, one is
compile-only and one does not exist, and installing Nazm means building it with Rust.

**Mechanism.**
- **Linux x86_64:** generated programs built and run in an `linux/amd64` container (emulated on
  this host), every runtime part reached — files, sockets, processes, the pool, the reactor (epoll's
  x86_64 packed event layout is unverified until then). Upgraded from compile-only only on execution.
- **macOS x86_64:** stays *run-verified under Rosetta*, said so; native Intel evidence is a final-v1
  item if Tier-1 policy requires it.
- **Windows x86_64:** see Part A-W below.
- **Prebuilt toolchains:** `xtask release` builds a `nazm` archive per Tier-1 host it can build for —
  aarch64/x86_64 macOS here, aarch64/x86_64 Linux in containers — each with its runtime artifact and
  standard library, and a **clean-install test**: a fresh container (or a fresh macOS user directory)
  with no Rust, the archive unpacked, `nazm build` of a sample program, the program run.
- **Windows ARM64:** Tier 2 / preview at most; not run here.

**Deferred (user, 2026-10-10):** Linux x86_64 execution and the x86_64 archives and clean installs,
like real Windows host execution, are taken up at the final-v1 qualification rather than in Gate 3,
where an emulated suite costs hours a run. What 3B-2 found before the deferral is kept: Cranelift's
x86-64 backend converts a float only to a 32- or 64-bit integer, so a narrower conversion is made
at 32 and clamped (`cross.rs`). Gate 3 continues with 3D on this host's own architecture.

### A-W. Windows x86_64

**Decision (user, 2026-10-10):** a real Win32 port of the runtime, built with LLVM (`x86_64-pc-
windows-gnu`, MinGW-w64's C runtime and import libraries, `lld`) and **run under Wine in a Docker
container**. Every result is labelled *run-verified under Wine*; **real Windows x86_64 host
execution remains a required final-v1 qualification item**. Microsoft's CRT and SDK are not
downloaded and no licence is accepted on the user's behalf, so the MSVC environment
(`x86_64-pc-windows-msvc`) is not built in Gate 3; the calling convention is the same Windows x64
ABI either way, and the difference is the C runtime and import libraries.

**Constitution, before code** (`spec.md` gains *Windows x86_64*): PE/COFF executables and DLLs; the
Windows x64 calling convention for every C-ABI crossing; a runtime platform layer for Windows —
allocation (`HeapAlloc` through the C runtime's `malloc`), threads and mutex/condition variables
(`CreateThread`, `SRWLOCK`, `CONDITION_VARIABLE`), files (`CreateFileW`, UTF-16 paths converted at
the boundary), standard streams, the monotonic and wall clocks (`QueryPerformanceCounter`,
`GetSystemTimePreciseAsFileTime`), sockets (Winsock 2) and the reactor (a thread over `WSAPoll`, the
Windows counterpart of the kqueue/epoll reactor, not IOCP — one reactor model, three bindings),
processes (`CreateProcessW`, anonymous pipes), entropy (`BCryptGenRandom`), stack-overflow and
failure handling (a guard page and a vectored exception handler that reports `N0408`), and path
rules (both separators accepted, drive letters). Debug information: CodeView only as scoped. Each
backend is verified independently where claimed; Cranelift's COFF output is attempted and recorded
either way.

## B. Systems foundation

**Problem.** A kernel- or driver-style program needs shared state an interrupt can touch, code and
data placed where the hardware expects it, a start symbol of its own, storage without a general
heap, and a defined failure path — none of which Nazm has beyond MMIO and the board table.

**Mechanisms**, each the smallest that is not a second mechanism:

| Need | Decision | Not a second mechanism because |
| --- | --- | --- |
| Atomics | `Atomic`, a counted handle to one `Int` cell, which **may cross into a task** (as `Chan` may); `atomic_new`, `atomic_load`, `atomic_store`, `atomic_add`, `atomic_swap`, `atomic_compare_swap`; **sequentially consistent only** in v1 — no ordering parameter | a channel is the one existing shared-mutable primitive; an atomic cell is the minimal one for the cases a channel cannot serve (an interrupt, a counter), and exposing orderings is a memory-model commitment v1 does not need to make |
| Static storage | `static NAME: T = CONST;` — module-level storage initialised by a constant, of a number, a `Bool` or `Atomic` (a record, enum or array would need constant values of one, which 1.0 does not have: an extension); **no mutable global variables**: a static is read-only unless it is an atomic | constants already exist (Gate 2 §15); a static is a constant with an address, and mutation stays explicit through the one atomic primitive |
| Volatile | the existing `mmio_*` built-ins, completed with 64-bit forms | MMIO already owns device access |
| Sections, placement | `@section("NAME")` on a function, constant or static; the board's `link.ld` places it | the attribute vocabulary is the compiler's extension point (`spec.md`, *Attributes*) |
| Custom entry and runtime boundary | a **board manifest** (`board.toml`): load address, memory regions, stack size, UART, stop mechanism, vector table and timer, heap; the built-in boards become manifests. The program's entry stays `main` (settled in `spec.md`, *Boards*: a second entry would be a second mechanism) | boards are already data (N86); this opens the table to users instead of inventing an entry syntax |
| Interrupts | `@interrupt("NAME")` on a `fn() -> Int`, or one taking only an `MmioCap` (a handler that drives a device needs the authority); the manifest's vector table names it; a handler shares state only through statics and atomics | one attribute, one rule; the effect checker treats a handler as an entry point |
| Allocation | `no-heap` profile rule (no allocation site at all); a board with a heap region in its manifest gets a bump **arena** as its allocator; without one, allocation is refused at build time, as today | the runtime's allocator boundary already exists; the arena is a board's binding of it |
| Failure | the board's stop mechanism, plus an optional `@on_failure` function the runtime calls before stopping | the failure path already exists; this names the hook |
| Escape hatch | none beyond MMIO and the C ABI: no inline assembly in v1 | the C ABI is the documented low-level escape |

**Reference workload:** a minimal kernel-style image — a timer interrupt that increments an atomic
tick count, a device poll through MMIO, a fault path — booted under QEMU on the AArch64 board.
"OS-ready" is never claimed from this.

## C. Embedded

**Mechanism.** The two existing boards as manifests, and **one MCU-class family**: Cortex-M
(`thumbv7m-none-eabi`, QEMU `mps2-an385`) — 32-bit pointers, as wasm32 already has, with `Int`
staying 64-bit. If the backend cannot target it soundly in this gate, it is recorded as an explicit
blocker with the reason, not left implied. A **HAL boundary** is library-level: `@std/hal` traits
(`Pin`, `Timer`, `Uart`) implemented per board over MMIO, so device code is written against traits
and a board supplies the impls — traits are the existing abstraction (N78). Deterministic startup,
static/no-heap option, bounded stack (`bounds.json`), timers via the board's timer device and an
interrupt.

**Reference workload:** a timer + GPIO-style state machine under QEMU.

## D. Realtime

**Mechanism.** The `realtime` profile is strengthened with rules that can be enforced soundly:
`bounded-loops` extended to every loop form whose bound the checker can establish (and `unknown`
refused, as today), `bounded-stack` (refuse a build whose `bounds.json` has no bound — recursion,
function values), `no-heap-after-init` (allocation only in functions reachable from an initialisation
entry the manifest names), `static-topology` (tasks spawned only from `main`'s top-level scope, a
fixed count), `bounded-blocking` (every blocking wait carries a deadline), and **deadlines** as the
existing `select … after` (N54) made mandatory on receives. A **fixed-priority** scheduler mode on
boards (a cooperative run-to-completion loop over static tasks ordered by `@priority(N)`), with
priority inversion avoided by construction: no locks exist, only channels and atomics. No WCET bound
is claimed; the profile report keeps `wcet: null`. "Hard real-time" is never said.

**Settled in 3F (spec.md, *Restriction profiles*, *Boards*).** `realtime` gains `bounded-stack` and
`no-heap`. Narrowed: `no-heap-after-init` is `no-heap` — an initialisation phase that may allocate
would need the analysis of which functions run only before the tasks, which 1.0 does not have — and
`bounded-loops` keeps its one counted form. `static-topology` and `bounded-blocking` are what
`no-spawn` and `no-blocking` already guarantee, so neither is a rule of its own. The scheduler is
`@task(priority = "N", period = "P")`, periods in board-timer ticks, a deadline its period, a miss
`N0414`.

## E–J. Domain profiles

Each is a **composition** of existing and Part D rules, plus at most a few domain rules, with a
reference workload. None adds language semantics.

| Domain | Profile = | Domain rules | Reference workload |
| --- | --- | --- | --- |
| Robotics | `realtime` + device authority | — | sensor input, fixed-rate control loop, vector maths (`@std/linalg`, small fixed matrices over `Float64`), actuator output, deadline miss → safe stop, deterministic shutdown |
| Industrial | `realtime` + `critical` | `watchdog` (a manifest watchdog must be fed by the control loop) | a controller state machine with a fail-safe state and a watchdog |
| High assurance | `critical`, strengthened | `bounded-memory`, `bounded-stack`, `bounded-loops`, `no-unknown-effects`, strict FFI (`no-foreign`), reproducible artefact (`locked-build` + provenance) | a controller whose obligations are all `proved` or `checked`, with its trace (Part S) |
| Aerospace | `embedded` + `realtime` + `critical` | `no-float-nondeterminism` (no transcendental built-ins whose results differ by host libm) | sensor sampling, a state estimator, bounded actuator output, a fault transition, telemetry |
| Automotive | `realtime` + `critical` | — | an ECU-style periodic task, a state machine, a CAN-like frame boundary (a library over MMIO or UDP in the host simulation) |
| Medical-device style | `critical` + `realtime` | `audit` (every state transition emits a structured record) | a device controller with an explicit fault state and an audit log |

No certification, qualification or regulatory approval is claimed for any of them; each matrix row
says so.

*Settled in 3G (2026-10-11).* The profiles are `robotics`, `automotive`, `industrial`, `medical`,
`aerospace` and `assurance` (`spec.md`, *Restriction profiles*). Narrowed: *high assurance*'s
`bounded-memory` is `no-heap`, its `no-unknown-effects` is `declared-effects` with `no-unknown-calls`,
strict FFI is `no-foreign`, and its reproducible artefact is `locked-build`; *robotics*'s device
authority is `explicit-authority`. `watchdog` is the runtime's, on the scheduler's ticks — a manifest
`[watchdog] ticks = N`, `watchdog_feed()`, `N0415` — not a hardware watchdog. `audit` is over the
only state a board program changes, its static atomics: `atomic_store`, `atomic_swap` and
`atomic_compare_swap` are transitions, `atomic_add` counts. `no-float-nondeterminism` refuses the
transcendental built-ins, whose results are the host library's. The automotive frame boundary is a
library over MMIO, not CAN. Robotics' vector maths is `@std/linalg`, small fixed vectors and
matrices over `Float64`. The aerospace and robotics workloads run on AArch64 only: a Cortex-M3 has no
floating point.

## K. Cybersecurity

The existing `cyber` profile plus a reference **secure service**: a locked, provenance-recorded,
attested TCP service with explicit authority, whose profile refusals are demonstrated and whose
artefact identity is reproduced. No sandboxing or non-interference claim.

## L. AI / HPC

A **CPU provider** joins the accelerator abstraction — the same kernels compiled natively and run on a
pool, used when no device is available and as the second oracle — so the provider interface is
proved to be a boundary, not OpenCL-shaped. `Float32`/`Float64` kernels; a SIMD path through LLVM's
vectoriser with a measured comparison against scalar. One verified GPU provider remains (OpenCL on
this host); Metal/CUDA/ROCm/SPIR-V are named future providers behind the same interface.

*Settled in 3H (2026-10-11).* The boundary is the kernel's one source text and a provider that runs
it: OpenCL builds it for the GPU; the CPU provider compiles it as C with a prelude for OpenCL's words
(`__kernel`, `__global`, `get_global_id`, `atomic_min`, `mul_hi`) and runs its element function,
`nz_item`, over a thread pool. `--provider auto|opencl|cpu`; `auto` falls back to the CPU and says
so. Float kernels are one type throughout, `Float32` or `Float64`, held bit for bit: no FMA
contraction on either provider, correctly rounded division and square root on the device. Narrowed:
float kernels call only `float_sqrt` and `float_abs`; a GPU without `cl_khr_fp64` — Apple's — refuses
`Float64` and the CPU provider runs it; a device that flushes subnormals differs from the interpreter
there, and the check says so rather than hides it.

## M. Storage engine

A reference storage engine beyond `kvstore`: a page-structured file with a fixed binary layout
(`@std/binary`), a write-ahead log, recovery after a simulated crash at every write boundary, and
concurrent readers with one writer through channels; a restart/readback test. File locking is added
to the runtime if needed for the single-writer rule. mmap and direct I/O only if soundly supported;
otherwise named limitations. No database-maturity claim.

## N. Distributed foundation

A small library contract, not a new concurrency model: node identity (a value, not ambient), framed
serialization over `@std/binary`, a transport abstraction (a trait with TCP and in-process
implementations), timeouts and cancellation through the existing `select`, a retry helper with
exponential backoff and an idempotency key, and failure propagation as `Result`. Reference: two
processes exchanging requests, one killed and restarted, the other retrying to an idempotent result.

*Settled in 3I (2026-10-11).* The storage engine is a reference application, `pagestore`: sixteen
fixed 256-byte pages (number, count, fourteen key–value slots, an FNV-1a checksum) written in place
behind a redo log of whole page images, so a page torn by a crash is rewritten from its image; a
fresh store's pages are made in one atomic replacement. The single-writer rule is one owner task —
readers and the writer reach it through channels — and, across processes, a `LOCK` file a crash
leaves and `recover` breaks: no `flock` was added, and no mmap or direct I/O. The crash test cuts a
run at each of its write boundaries, half that write written, and checks every recovery against the
acknowledged writes. It found two defects in the engine as first written — a log handle kept across
the checkpoint that replaced its file, and a reopen that reset the crash counter — both fixed. The
distributed foundation is `@std/dist` and the `ledger` reference: a server stopped after logging a
credit and before answering, restarted on its address, and a client whose `retry` carries it
through, every credit applied once.

## O. WASM / WASI

Freestanding `wasm32-unknown-unknown` is kept. A **WASI preview-1 baseline** (`wasm32-wasip1`) is
added if practical: a heap (so strings, sequences and closures stop being refused), standard
streams, arguments, clocks, and files under capability rules (`IoCap` maps to preopened
directories). Reference runtime: one WASI host available in the container (wasmtime if installable,
otherwise Node's WASI); a package/example.

*Settled in 3J (2026-10-11).* `wasm32-wasip1` is WebAssembly's freestanding path with its own
runtime, `nazm_runtime::wasi`: a bump arena over linear memory, grown by `memory.grow` and never
returned, under the parts a board with a heap takes; the C library's few functions the runtime parts
call (`malloc`, `memcpy`, `memcmp`, `strlen`, `write`, `exit`), defined at the parts' own signatures
— built with bulk memory, so LLVM's copies are instructions and never a second `memcpy` at wasm32's
size; the files part over WASI's `path_open` and `fd_*`, its errors Linux's numbers. The reference
host is Node's `node:wasi`, in `nazm-wasm:n64` beside `wasm-ld`: wasmtime is not installed. Narrowed:
no directory listing, no environment, an arena that never frees, no tasks, channels or sockets.
Found on the way: Node's WASI refuses an open asking for the `sock_shutdown` right, so a file is
opened with every right but it.

## P. Mobile

Library targets over the stable C ABI, not a UI framework. **Android ARM64**: the NDK (downloaded
into `target/`, not installed system-wide) cross-builds `libnazm*.so` for `aarch64-linux-android`; a
minimal host shell (a C or JNI program) calls it; executed on the aarch64 Linux container where the
Android C library permits, otherwise compile-and-link evidence only, labelled so. **iOS ARM64**: an
`.a`/xcframework for `aarch64-apple-ios` and the simulator, linked into a minimal host — **only once
Xcode is installed** (the user is installing it); run in the iOS simulator if available, never
claimed run-verified otherwise.

## Q. Desktop integration

One example: a native window host (a C program over the platform's windowing API — Cocoa on this
host) whose event loop calls a Nazm shared library for its state and logic, through the C ABI and a
callback. A Nazm GUI framework is ecosystem, not v1.

*Settled in 3K (2026-10-11).* `aarch64-linux-android` and `aarch64-apple-ios` are hosted targets
on the existing platform layer: Android a switch target of its own — Linux's AArch64 layouts and
numbers, Bionic's `__errno` and `strerror_r` — and iOS Darwin's. Android evidence is static
executables, a C host with a Nazm library and a whole program with its pool, linked by the NDK r27c
in `target/toolchains` and run on an aarch64 Linux kernel in a container: `run-verified on a Linux
kernel`, a status of its own, never plain run-verified and never an Android device. iOS is linked
against Xcode 27's iOS SDK — an archive and a program, platform 2 — and not run: no simulator runtime
is installed, and fetching one is a multi-gigabyte download left to the user. The desktop example is
`examples/desktop/counter`: a Cocoa window whose button callbacks call a Nazm shared library's state
machine, driven by synthetic clicks and checked label by label.

## R. Web3

Qualification of what exists: the contract model, EVM backend, WASM contracts, deterministic step
and gas metering, storage, transaction simulation and upgrade checks, each with its evidence; gas
bounds tightened where loops are bounded. sBPF stays blocked while no toolchain is available.

*Settled in 3L (2026-10-11): the qualification.* What exists, each claim with the test that holds it:

| Claim | Evidence |
| --- | --- |
| A contract's facts: each entrypoint's reads and writes | `contract.rs` `a_contracts_facts_name_each_entrypoints_reads_and_writes` |
| Deterministic transactions; a revert changes nothing; a failure reverts with its code | `contract.rs` `transactions_run_deterministically_and_a_revert_changes_nothing`, `a_runtime_failure_reverts_with_its_code` |
| `web3` refuses nondeterminism and outside authority; a pure function means the same under it | `contract.rs` `the_web3_profile_refuses_nondeterminism_and_outside_authority`, `a_pure_function_means_the_same_under_web3_as_under_no_profile` |
| `accounts`: a write no signer decides on is refused, through helpers and built-up states | `accounts.rs` (four tests) |
| Generated sequences run deterministically and keep the conserved quantity | `contract_property.rs` `generated_sequences_run_deterministically_and_keep_the_conserved_quantity` |
| The EVM backend agrees with the simulator, fixed and generated, and stays inside its gas bounds | `evm.rs` `the_evm_agrees_with_the_simulator_and_stays_inside_its_gas_bounds`; `contract_property.rs` `the_evm_agrees_with_the_simulator_on_generated_sequences` (py-evm, `nazm-evm:n70`) |
| The WebAssembly contract agrees with the simulator | `wasm_contract.rs` `the_webassembly_contract_agrees_with_the_simulator`; `contract_property.rs` (Node) |
| Upgrades: an added field allowed, a removed or retyped one refused | `evm.rs` `the_upgrade_check_allows_an_added_field_and_refuses_a_removed_or_retyped_one` |
| Contract obligations, and their census | `contracts.rs` (eight tests) |

Tightened in 3L: an entrypoint's gas bound now runs through a loop in the counted form (N63), as its
turns times its condition and body, plus the last condition; any other loop still leaves the bound
`null`. sBPF stays BLOCKED: no toolchain here.

*Settled in 3L: traceability.* `nazm trace`, `nazm.trace/1`: requirements in `requirements.toml`
(`nazm.requirements/1`), each definition by `DefKey` and its Core IR semantic digest, obligations
from the census keyed by definition (the census gained each obligation's `FnId`), tests run through
`nazm test`. The artefact digest is the definition's semantic digest. A broken link fails the trace
by name.

## S. Requirements traceability

A requirement is a stable identifier in a project file (`requirements.toml`: id, text, the
definitions it constrains by `DefKey`, the tests that verify it). `nazm trace` resolves each
definition to its durable identity, collects its obligations (now keyed by `DefKey`, not by
position) and the named tests' latest results, and writes `nazm.trace/1`: requirement → definition →
obligation → test → artefact digest. A broken link (a definition renamed away, a test missing) is
reported, never silently dropped. No certification claim.

## T. Domain matrix

[`v1-domain-matrix.md`](v1-domain-matrix.md): one row per domain with its profile, targets, required
mechanisms, reference workload, evidence, status, remaining ecosystem gap and non-claim. A domain is
`VERIFIED` there only with executable evidence; otherwise `PLANNED`, `PARTIAL` or `BLOCKED` with the
reason.

---

## Order of work

Each step is a commit (or a few) with its spec sections before code, tests, mutants and matrix
evidence, as in Gate 2; the contained gate runs at the end.

| Step | Parts | Why here |
| --- | --- | --- |
| 3A | this record, the domain matrix, the OpenSpec change | constitution first |
| 3B | A: Linux x86_64 execution, prebuilt archives and clean installs — x86_64 execution deferred to final v1 (see A) | hosts before domains: every later workload needs them |
| 3C | A-W: the Windows constitution, runtime port, Wine evidence | the largest single port; independent of the rest |
| 3D | B: atomics, statics, sections, board manifests, interrupts, arena, failure hook; kernel workload | every embedded and realtime domain stands on it |
| 3E | C: Cortex-M (or its blocker), `@std/hal`, timers; state-machine workload | |
| 3F | D: realtime rules and the fixed-priority board scheduler | |
| 3G | E–J, K: domain profiles and their workloads | compositions of 3D–3F |
| 3H | L: CPU provider, float kernels, SIMD | |
| 3I | M, N: storage engine, distributed foundation | |
| 3J | O: WASI | |
| 3K | P, Q: Android (and iOS once Xcode is present), desktop host | |
| 3L | R, S, T: web3 qualification, traceability, the completed domain matrix; contained gate | |

## Versions

Expected to move: the semantic epoch (new built-ins, `static`, new attributes, new profile rules),
the runtime ABI (atomics, the Windows platform layer, arena, interrupts), the standard library API
(1.2: `@std/hal`, `@std/linalg`, distribution helpers), `nazm.profile-report` (new rules, additive),
and new schemas (`nazm.trace/1`, a board manifest schema). Each step records its own.
