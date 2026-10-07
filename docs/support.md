# Support matrix

What Nazm v0.3.0 has shown on each target and for each optional feature, in words that mean one
thing each. The target table is the compiler's own — `nazm_lir::backend::SUPPORT`, printed by
`nazm inspect` as `toolchain.support` — and `crates/nazm-cli/tests/docs.rs` fails if this page and
that table disagree. The evidence behind every cell is in `capability-matrix.md`, area 33.

## The words

| Status | Means |
| --- | --- |
| **run-verified** | Programs built for the target ran — on the host, under Rosetta 2, in a Linux container or on a WebAssembly engine — and agreed with the interpreter |
| **emulator-verified** | Run-verified where the run was a full-system emulator (QEMU): no hardware has run it |
| **compile-only** | Objects are built and read back from their headers; nothing built for it has run |
| **unsupported** | Refused by name, with the reason |
| **non-goal** | Refused by decision, not for want of a machine |
| **blocked** | Wanted, and stopped by something outside this repository: a missing toolchain, machine or permission |

No cell says just "supported".

## Targets

| Target | LLVM | Cranelift | Where programs ran | Debugger |
| --- | --- | --- | --- | --- |
| `aarch64-apple-darwin` | run-verified | run-verified | the host, Apple silicon | lldb, breakpoints resolved statically, both backends |
| `x86_64-apple-darwin` | run-verified | run-verified | under Rosetta 2 on Apple silicon | not exercised |
| `aarch64-unknown-linux-gnu` | run-verified | run-verified | the contained runner's Linux container | gdb, live, both backends (nazm-debug image) |
| `x86_64-unknown-linux-gnu` | compile-only | compile-only | nowhere: no x86_64 Linux machine or emulator here | not exercised |
| `aarch64-unknown-none` | emulator-verified | unsupported | QEMU `virt`, Cortex-A53 | not exercised |
| `riscv64gc-unknown-none-elf` | emulator-verified | unsupported | QEMU `virt`, RV64GC, machine mode | not exercised |
| `wasm32-unknown-unknown` | run-verified | unsupported | Node.js's engine, the reference host | DWARF sections present; a debugger not exercised |

Cranelift's two `unsupported` reasons: its object writer has no format for a target without an
operating system, and this Cranelift is built without a WebAssembly backend.

**Restrictions by target.** The hosted targets run the whole language and the M:N task pool. A
program that calls C runs a thread per task unless `NAZM_SCHEDULER=pool`. A target whose linker
this host lacks is built to objects (`--objects`) for its own toolchain; there is no bundled
sysroot. The boards are freestanding: no heap, `@std`, tasks, channels, closures or C library,
objects and a `link.ld` linked by `ld.lld`, and RISC-V needs a `clang` with a RISC-V code generator
(not Apple's). WebAssembly is the freestanding subset: no heap, so no strings built at run time,
sequences, channels, tasks or closures; imports `nazm_host.write` and `nazm_host.report` only, no
WASI. Details: `limitations.md`, *Native code*.

**x86_64 Linux.** Compile-only, because no Nazm-built program has been run on x86_64 Linux. It
moves to run-verified when one is — the differential corpus built for it and run on such a machine
or VM, with the result recorded — not because CI, which runs on x86_64 Linux, compiled the
compiler.

## Not supported, by decision

| | |
| --- | --- |
| Windows | unsupported: no Windows linker, SDK or machine here, and the runtime has no Windows port |
| 32-bit hosted targets | a non-goal: `Int` is 64-bit and every hosted layout assumes 64-bit pointers; wasm32 is the one 32-bit target, freestanding |
| big-endian targets | a non-goal: no big-endian machine or image is here to run one, and the DWARF writer and layouts assume little-endian |

## Optional features

| Feature | Status in v0.3.0 | What exists, and what stops more |
| --- | --- | --- |
| WebAssembly | verified, scoped | the freestanding subset, run on Node.js's engine and held to the interpreter; no WASI, no heap |
| GPU (`nazm accel`) | verified, scoped; experimental | maps, zips and reductions of `Int` kernels through OpenCL on Apple's GPU (macOS); a second provider **blocked** — no Metal or SPIR-V toolchain here |
| EVM contracts (`nazm contract --evm`) | verified, scoped; experimental | EVM bytecode run in py-evm, the reference EVM, and held to the simulator on generated transaction sequences; no chain deployment |
| WebAssembly contracts | verified, scoped; experimental | run under the reference host only; no chain's host interface |
| sBPF / Solana | **blocked** | signer analysis and account metadata only; no sBPF toolchain or validator exists here, so nothing is built or run |
| JIT | **blocked**; not part of this release | an in-process JIT needs `unsafe`, which the workspace forbids; the interactive tier (`nazm repl`, `nazm comptime`) is the interpreter |
| Formal proof | **blocked**; bounded model checking verified, scoped | every program of a small core up to stated bounds is checked against the interpreter and the checker (`formal-core.md`); no proof assistant is installed, so nothing is proved |
| Debugging | verified, scoped | DWARF from both backends; lldb (statically) and gdb (live) as above; no debug adapter (DAP) |
| Profiling (`nazm profile --sample`) | verified, scoped | macOS's `sample` only; refused elsewhere |
| Restriction profiles | verified, scoped | `embedded`, `realtime`, `critical`, `cyber`, `web3` and others enforce stated rules; **not certification** of any kind |

"Verified, scoped" means verified within the scope stated beside it and nothing more;
`capability-matrix.md` records each scope and its evidence.
