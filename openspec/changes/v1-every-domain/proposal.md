## Why

**Added 2026-10-16 (the copyright holder): a stable release comes first.** 0.11.0 is the stable,
fast release of everything except these domain tracks — R1–R9, stage P, Windows — through the full
gate; 1.0 then adds every domain below. 1.0 still waits for every domain.

The copyright holder decided (2026-10-15): **1.0 is a stable release meant to last twenty years, and
waits until every domain is ready** — cloud computing, quantum computing, hardware and chip design,
microcontrollers and microchip drivers — rather than shipping a stable core first and adding domains
as 1.x. 1.0 therefore moves from the March–April 2027 estimate to whenever the last domain passes
its gate — estimated on 2026-10-16, from the pace so far, as early 2027 if the boards, the quantum
account, the Windows host and the licence review are started alongside the code (`docs/roadmap.md`,
*When 0.11.0 and 1.0*).

## What Changes

Five domain tracks, each with a design first (spec before code), then an implementation held to the
same evidence as the rest of Nazm, and each ending in a gate that 1.0 requires:

- **C — cloud computing.** HTTP/1.1 and HTTP/2 server and client, TLS (through the platform's
  library, never a hand-written cipher), JSON/TOML already present, structured logging, graceful
  shutdown, container images (a static executable and a minimal image recipe), WebAssembly/WASI
  services, and the multicore scheduler measured under load.
- **M — microcontrollers.** Real boards, not only emulators: at least one Cortex-M board and one
  RISC-V board run the HAL programs; more MCU families; flashing and debugging from `nazm`.
- **D — microchip drivers.** Memory-mapped registers, interrupts and DMA as today, plus the OS driver
  models: a Linux kernel module and a Windows kernel driver written in Nazm under the `kernel`
  profile, each loading and serving a device or a virtual one.
- **H — hardware and chip design.** A synthesisable subset of Nazm — bit-precise integer types,
  registers, clocks, combinational and sequential logic — compiled to Verilog (RTL), simulated, and
  checked against a reference simulator; then an FPGA bitstream through the open toolchain.
- **Q — quantum computing.** Qubits and circuits as linear (move-only) types, gates, measurement,
  and classical control, compiled to OpenQASM 3; run on a simulator and checked against known
  results; a provider capability for real quantum hardware.

## Order

The rewrite (R2–R9), the Windows toolchain and stage P continue as planned; the five tracks start
with their designs now and run in parallel with them. 1.0's gate requires every track's gate.

## Capabilities

### New Capabilities
- `v1-every-domain`: what 1.0 must hold in each domain. Archived with `--skip-specs`.

## Impact

New standard modules, a hardware backend, a quantum backend, kernel-profile driver targets, board
tooling; the stability promise covers all of them at 1.0.
