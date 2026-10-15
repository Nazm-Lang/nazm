## Designs (first, each in design.md before any code)
- [x] 0.1 C cloud: libraries and the service model — design.md ## C (2026-10-15)
- [x] 0.2 M microcontrollers: boards, flashing, debugging — design.md ## M (2026-10-15)
- [x] 0.3 D drivers: Linux module and Windows driver under `kernel` — design.md ## D (2026-10-15)
- [x] 0.4 H hardware: the synthesisable subset, its types and its Verilog backend — design.md ## H (2026-10-15)
- [x] 0.5 Q quantum: linear qubit types, circuits, OpenQASM 3 — design.md ## Q (2026-10-15)

## Implementation and gates
- [ ] 1.1 C gate: a TLS HTTP service under load, in a container and on WASI
  - [ ] C1 the `tls` runtime part and `@std/tls`
  - [x] C2 `@std/http` HTTP/1.1 server, client and router over plain TCP (2026-10-16; `spec.md`, *HTTP*; `crates/nazm-cli/tests/http.rs`; mutants `c2-*`)
    - open: handlers under `http_serve` run on one task, because a function value may not cross into a task (`N0321`); the parallel shape is a task per connection naming its handler. Lifting `N0321` for capture-free or crossable-capture function values is a language decision for the copyright holder
    - open: a `Date` field, a whole-head deadline and `drain_ms` need a clock the server is not given (C4)
  - [ ] C3 HTTP/2, HPACK, flow control, ALPN; `h2spec`
  - [ ] C4 `@std/signal` and the drain deadline, `@std/config`, `@std/metrics`, `@std/log` fields
  - [ ] C5 least-loaded placement, `SO_REUSEPORT`, the musl targets, `nazm image`
  - [ ] C6 `wasm32-wasip2` and `--http-handler`
  - [ ] C7 the gate harness
- [ ] 1.2 M gate: HAL programs on real Cortex-M and RISC-V boards
- [ ] 1.3 D gate: a Linux module and a Windows driver load and serve a device
- [ ] 1.4 H gate: designs simulate identically to a reference simulator; one runs on an FPGA
- [ ] 1.5 Q gate: circuits match known results on a simulator; OpenQASM accepted by a provider
