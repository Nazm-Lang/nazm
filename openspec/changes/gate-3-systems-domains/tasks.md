## 1. Constitution

- [x] 1.1 `docs/systems-domains.md`, `docs/v1-domain-matrix.md`, this change (3A)

## 2. Hosts

- [x] 2.1 Linux x86_64 execution; prebuilt archives; clean installs (3B): the aarch64 archive and clean install (3B-1); the Cranelift x86-64 narrow-conversion fix (3B-2); x86_64 execution, archive and clean install carried to final v1 (user, 2026-10-10)
- [x] 2.2 Windows x86_64: constitution, Win32 runtime port, evidence under Wine (3C); real Windows host execution carried to final v1

## 3. Systems, embedded, realtime

- [x] 3.1 Atomics, statics, sections, board manifests, interrupts, arena, failure hook; kernel workload (3D): emulator-verified on AArch64; RISC-V interrupts refused (N0626)
- [x] 3.2 Cortex-M or its blocker; `@std/hal`; timers; state-machine workload (3E): `thumbv7m-none-eabi`, NVIC and SysTick, `@std/hal`, the traffic light on both boards — emulator-verified
- [x] 3.3 Realtime rules; fixed-priority board scheduler (3F): `bounded-stack`, `no-heap`, `@task`, `N0414`; emulator-verified on both boards

## 4. Domains

- [x] 4.1 Robotics, industrial, high assurance, aerospace, automotive, medical-style, cyber (3G): seven profiles, three domain rules, `@std/linalg`, `@std/audit`, the board watchdog; six workloads emulator-verified, the secure service run-verified
- [x] 4.2 AI/HPC CPU provider, float kernels, SIMD (3H): the provider boundary, `Float32`/`Float64` bit for bit, the SIMD path measured (no speed-up claimed)
- [x] 4.3 Storage engine; distributed foundation (3I): `pagestore`, crashed at every write boundary; `@std/dist` and the `ledger`, a server restarted mid-request
- [x] 4.4 WASI (3J): `wasm32-wasip1`, a heap, streams, arguments, clocks, files beneath the preopen; run-verified under Node's WASI
- [x] 4.5 Android, iOS, desktop host (3K): Android run on a Linux kernel as static Bionic; iOS link-verified (no simulator runtime); a Cocoa window on a Nazm library
- [x] 4.6 Web3 qualification; requirements traceability; the domain matrix completed (3L): the qualification table, gas bounds through counted loops, `nazm trace`

## 5. Evidence

- [ ] 5.1 Contained gate: workspace, lifecycle, selfhost/bootstrap where affected, targeted mutations, platform tests
