## Designs (first, each in design.md before any code)
- [x] 0.1 C cloud: libraries and the service model — design.md ## C (2026-10-15)
- [x] 0.2 M microcontrollers: boards, flashing, debugging — design.md ## M (2026-10-15)
- [x] 0.3 D drivers: Linux module and Windows driver under `kernel` — design.md ## D (2026-10-15)
- [x] 0.4 H hardware: the synthesisable subset, its types and its Verilog backend — design.md ## H (2026-10-15)
- [x] 0.5 Q quantum: linear qubit types, circuits, OpenQASM 3 — design.md ## Q (2026-10-15)

## Implementation and gates
- [ ] 1.1 C gate: a TLS HTTP service under load, in a container and on WASI
- [ ] 1.2 M gate: HAL programs on real Cortex-M and RISC-V boards
- [ ] 1.3 D gate: a Linux module and a Windows driver load and serve a device
- [ ] 1.4 H gate: designs simulate identically to a reference simulator; one runs on an FPGA
- [ ] 1.5 Q gate: circuits match known results on a simulator; OpenQASM accepted by a provider
