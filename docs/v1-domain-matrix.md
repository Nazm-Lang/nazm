# Nazm v1 — the domain matrix

Every v1 domain claim points here (`systems-domains.md`, Part T). A row is **VERIFIED** only when its
reference workload has executable evidence of the kind its *Evidence* column names; otherwise it is
**PARTIAL** (some evidence, gaps named), **PLANNED** (Gate 3 work not yet done) or **BLOCKED** (cannot
be run here, and why). Evidence labels are `systems-domains.md`'s: *run-verified (native)*, *under
Rosetta*, *emulated host*, ***under Wine** (not evidence from a Windows host)*, *emulator-verified*.
[`capability-matrix.md`](capability-matrix.md) remains the implementation truth; where they
disagree, it is right.

*Opened 2026-10-10 at `5caa642`, the start of Gate 3: each status below is the state before Gate 3's
work, and is updated by the step that changes it.*

## Hosts

| Host | Targets | Evidence | Status | Remaining gap | Non-claim |
| --- | --- | --- | --- | --- | --- |
| macOS ARM64 | `aarch64-apple-darwin` | run-verified (native), both backends (`cross.rs`) | VERIFIED (programs); prebuilt archive and clean install PLANNED (3B) | — | — |
| macOS x86_64 | `x86_64-apple-darwin` | run-verified **under Rosetta**, both backends | PARTIAL — Rosetta, not an Intel host | native Intel execution, if final Tier-1 policy requires it | not "verified on Intel hardware" |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | run-verified in a container, both backends | VERIFIED (programs); archive PLANNED (3B) | — | — |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | compile-only | PLANNED (3B) | execution | — |
| Windows x86_64 | `x86_64-pc-windows-gnu` (MinGW-w64), under Wine | none | PLANNED (3C) | real Windows host execution — a final-v1 item; the MSVC environment | Wine evidence is not Windows-host evidence |
| Windows ARM64 | — | none | Tier 2 / preview at most | — | — |

## Domains

| Domain | Profile | Targets | Required mechanisms | Reference workload | Evidence | Status | Remaining ecosystem gap | Non-claim |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Systems / OS foundation | `embedded` (+ `no-heap`) | `aarch64-unknown-none`, `riscv64gc-unknown-none-elf` | atomics, statics, sections, board manifests, interrupts, arena, failure hook (3D) | timer-interrupt kernel image | emulator-verified | PLANNED — MMIO, boards and freestanding builds exist (N62, N86) | drivers, filesystems, paging | not "OS-ready" |
| Embedded | `embedded` | the two boards + Cortex-M `thumbv7m-none-eabi` (3E) | `@std/hal`, timers, interrupts, linker layout, manifests | timer + GPIO state machine | emulator-verified | PARTIAL — two emulated boards, no MCU, no HAL | vendor HALs, real boards | no hardware run |
| Realtime | `realtime` | boards; host | bounded loops/stack/blocking, static topology, deadlines, fixed-priority mode (3F) | periodic tasks with deadlines | emulator-verified, profile reports | PARTIAL — counted loops, stack bound (N63) | WCET analysis | not "hard real-time" |
| Robotics | robotics = `realtime` + device authority | host, boards | float, `@std/linalg`, timers, bounded channels, HAL | sense → control → actuate at a fixed rate | run- and emulator-verified | PLANNED (3G) | ROS/DDS | — |
| Industrial | industrial = `realtime` + `critical` + `watchdog` | boards | watchdog, state machine, device I/O | controller with fail-safe state | emulator-verified | PLANNED (3G) | Modbus, EtherCAT, PROFINET, OPC UA | — |
| High assurance | `critical`, strengthened | host, boards | bounded memory/stack/loops, no unknown effects, strict FFI, reproducible artefact, obligations, trace | controller with every obligation `proved` or `checked` | run-verified, obligation and trace reports | PARTIAL — contracts and obligations (N87) | proof, qualified toolchain | not certified |
| Aerospace | aerospace = `embedded` + `realtime` + `critical` + domain rules | boards | as above + deterministic floats | sample → estimate → actuate, fault transition, telemetry | emulator-verified | PLANNED (3G) | ARINC, MIL-STD, SpaceWire | no DO-178C claim |
| Automotive | automotive = `realtime` + `critical` | boards, host | CAN-like frame boundary | periodic ECU task, state machine, failure state | emulator- and run-verified | PLANNED (3G) | AUTOSAR | no ISO 26262 claim |
| Medical-device style | medical = `critical` + `realtime` + `audit` | boards, host | audit records, fault state, restricted authority | device controller with audit log | run-verified | PLANNED (3G) | — | no regulatory approval |
| Cybersecurity | `cyber` | hosts | locked build, provenance, attestation, explicit authority | attested secure service | run-verified | PARTIAL — profile, provenance, attestation (N73, N87, N94) | TLS, cryptography | no sandboxing or non-interference claim |
| AI / HPC | — (accelerator) | macOS (OpenCL), CPU | CPU provider, float kernels, SIMD (3H) | map/zip/reduce held to an oracle | run-verified | PARTIAL — one provider, `Int` only, no CPU fallback (N67, N88) | Metal, CUDA, ROCm, SPIR-V | no performance claim without a filed measurement |
| Storage engine | — | hosts | binary layout, WAL, recovery, locking (3I) | page store with crash recovery | run-verified | PARTIAL — `kvstore` (Gate 2) | mmap, direct I/O if unsound | no database-maturity claim |
| Distributed | — | hosts | node identity, framed transport, retry, idempotency (3I) | two processes, one restarted | run-verified | PLANNED — TCP/UDP, select, deadlines exist | consensus, TLS | — |
| WASM / Web | — | `wasm32-unknown-unknown`, `wasm32-wasip1` (3J) | heap, streams, files under capability rules | WASI program on a reference runtime | run-verified | PARTIAL — freestanding wasm32 on Node (N64) | browser DOM | — |
| Mobile | — | `aarch64-linux-android`, `aarch64-apple-ios` (3K) | C-ABI library, host shell | Nazm library called from a minimal host | Android: as run; iOS: after Xcode | PLANNED | UI frameworks | not run-verified where not run |
| Desktop integration | — | macOS | C ABI, callback, event loop | a Cocoa window driven by Nazm logic | run-verified | PLANNED (3K) | a Nazm GUI framework | — |
| Web3 | `web3`, `accounts` | EVM, wasm32 | contract model, metering, upgrade checks | simulator/EVM/WASM agreement | run-verified (py-evm, Node) | PARTIAL — qualified in 3L (N69–N72, N96) | sBPF (blocked), chain host interfaces | not audited, not deployed |
| Requirements traceability | — | all | requirement ids, `DefKey`-keyed obligations, `nazm trace` (3L) | high-assurance controller's trace | run-verified | PLANNED | — | no certification claim |
