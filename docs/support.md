# Support matrix

What the Nazm toolchain (0.10.0 preview) has shown on each target and for each optional feature, in words that mean one
thing each. The target table is the compiler's own — `nazm_lir::backend::SUPPORT`, printed by
`nazm inspect` as `toolchain.support` — and `crates/nazm-cli/tests/docs.rs` fails if this page and
that table disagree. The evidence behind every cell is in `capability-matrix.md`, area 33. **The
0.10.0 preview is released for `aarch64-apple-darwin` only**; the other rows say what this tree has
shown, not what is released.

## The words

| Status | Means |
| --- | --- |
| **run-verified** | Programs built for the target ran — on the host, under Rosetta 2, in a Linux container or on a WebAssembly engine — and agreed with the interpreter |
| **emulator-verified** | Run-verified where the run was a full-system emulator (QEMU): no hardware has run it |
| **run-verified on a Windows host** | Windows executables, linked by MinGW-w64's `gcc`, ran on a real Windows x86_64 machine and agreed with the same programs built natively (`scripts/windows-host.sh`) |
| **run-verified under Wine** | Windows executables ran under Wine in a Linux container and agreed with the interpreter: **not** evidence from a Windows host |
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
| `x86_64-pc-windows-gnu` | run-verified on a Windows host | run-verified on a Windows host | a Windows 10 laptop (version 1903, 10.0.18362.113), linked by MinGW-w64's `gcc`: `scripts/windows-host.sh`, 68 agree, 0 differ, 7 skipped under each backend (2026-10-15); and Wine 8 in a Debian x86_64 container (`docker/wine.Dockerfile`) | not exercised |
| `aarch64-unknown-none` | emulator-verified | unsupported | QEMU `virt`, Cortex-A53 | not exercised |
| `riscv64gc-unknown-none-elf` | emulator-verified | unsupported | QEMU `virt`, RV64GC, machine mode | not exercised |
| `thumbv7m-none-eabi` | emulator-verified | unsupported | QEMU `mps2-an385`, Cortex-M3 (Gate 3) | not exercised |
| `wasm32-unknown-unknown` | run-verified | unsupported | Node.js's engine, the reference host | DWARF sections present; a debugger not exercised |
| `wasm32-wasip1` | run-verified | unsupported | Node.js's `node:wasi`, preview 1, in the `nazm-wasm:n64` container (Gate 3) | DWARF sections present; a debugger not exercised |
| `aarch64-linux-android` | run-verified on a Linux kernel | unsupported | static Bionic executables, linked by the NDK r27c's `clang` for API level 30, run on an aarch64 Linux kernel in a Debian container; no Android device or emulator (Gate 3) | not exercised |
| `aarch64-apple-ios` | compile-only | unsupported | linked against Xcode 27's iOS SDK, an archive and a program; never run — no simulator runtime is installed (Gate 3) | not exercised |

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

**Windows x86_64.** Built to objects (`--objects`) and linked by MinGW-w64's `gcc` as `link.txt`
says: `-static`, Winsock (`-lws2_32`) and `BCryptGenRandom` (`-lbcrypt`); nothing of Microsoft's
SDK or C runtime is used. Files, sockets and the reactor (`WSAPoll`), processes and their pipes,
the clocks, UTF-8 arguments, a trap and an exhausted stack run under Wine as they do natively,
with two differences the spec states: `os_family()` is 2, and `os_kill` ends a child with exit
code 1. The main thread reserves 8 MiB (`-Wl,--stack,8388608`), as macOS and Linux give it, and
a frame of a page or more is probed by MinGW-w64's `___chkstk_ms`. A DLL is refused by name
(`--shared`); a static library (`--lib`) is built.

*On a Windows host (2026-10-15).* `scripts/windows-host.sh HOST` builds each program of the
differential set for Windows, links it here, runs it on the host over `ssh`, and compares its
standard output, standard error and exit status with the same program built natively. On a
Windows 10 laptop (1903, 10.0.18362.113): 68 agree, 0 differ, 7 skipped — programs that call C
or read arguments, input or files, counted and never passed — under LLVM and under Cranelift
(`BACKEND=cranelift`) alike. The first run, before two fixes, found 64 agree, 1 differ and 3 that
did not link: LLVM's inline stack-probe attribute names a function on Windows, and Windows'
1 MiB main stack met `N0408` where 8 MiB does not. Files, sockets, processes and arguments are
still shown only under Wine; the host run covers what the differential set prints.

**x86_64 Linux.** Compile-only, because no Nazm-built program has been run on x86_64 Linux. It
moves to run-verified when one is — the differential corpus built for it and run on such a machine
or VM, with the result recorded — not because CI, which runs on x86_64 Linux, compiled the
compiler.

## Prebuilt toolchains (Gate 3)

A host archive is `nazm-TRIPLE.tar.gz`, made by `scripts/host-archive.sh`: `bin/nazm` with the
standard library and runtime inside it, the licence, the README, the getting-started and support
pages, and `INSTALL.txt`. `nazm run`, `check` and `fmt` need nothing else; `nazm build` needs a
C toolchain (clang) on `PATH`. **Rust is never needed.** `scripts/clean-install-test.sh` unpacks an
archive where no `cargo` or `rustc` is reachable, then checks, runs, builds and runs a program.

| Host | Archive built | Clean install | Where it ran |
| --- | --- | --- | --- |
| macOS ARM64 (`aarch64-apple-darwin`) | on the host | passed | the host, natively |
| macOS x86_64 (`x86_64-apple-darwin`) | on the host, cross-built | passed | **under Rosetta 2** on Apple silicon |
| Linux ARM64 (`aarch64-unknown-linux-gnu`) | in the contained runner (`cargo xtask contained archive`) | passed | a clean Debian container with only clang (`docker/clean-install.Dockerfile`) |
| Linux x86_64 (`x86_64-unknown-linux-gnu`) | not yet | not yet | — |
| Windows x86_64 | not yet | not yet | — |

Nothing is uploaded or published: the archives are local, under `target/archives/`.

### The toolchain's own tests on GitHub's hosts (since 2026-10-15)

CI (`.github/workflows/ci.yml`) builds the toolchain and runs its host-safe test subset — not the
contained gates — on GitHub's machines. This is evidence about **the toolchain as a host**, not a
target made run-verified:

| host | result |
| --- | --- |
| Linux x86_64 (`ubuntu-latest`) | passes, the required job |
| Linux ARM64 (`ubuntu-24.04-arm`) | passed on its first run (`c86f2f81`) |
| macOS ARM64 (`macos-latest`) | three wasm tests failed only for Apple's clang lacking the wasm target; Homebrew's LLVM installed since `27923d5d` |
| Windows x86_64 (`windows-latest`) | **the toolchain does not build**: `nazm-core`'s interpreter uses Unix file descriptors (`std::os::fd`, `std::os::unix`) for files, processes and sockets. A Windows host is a port, not yet started |

The three new jobs gather evidence (`continue-on-error`) until each is green once.

## Not supported, by decision

| | |
| --- | --- |
| Windows with Microsoft's toolchain, and Windows ARM64 | unsupported: `x86_64-pc-windows-msvc` needs Microsoft's SDK and C runtime, which are not downloaded or accepted here, and no Windows ARM64 machine or emulator is here; Windows x86_64 is `x86_64-pc-windows-gnu`, with MinGW-w64's |
| 32-bit hosted targets | a non-goal: `Int` is 64-bit and every hosted layout assumes 64-bit pointers; wasm32 — freestanding, or with WASI's imports and its own heap — and the Cortex-M board (`thumbv7m-none-eabi`) are the 32-bit targets, where a C handle, a C string result and a callback are refused by name (`N0381`, Q1-A1-05) because a C pointer crosses as 64 bits |
| big-endian targets | a non-goal: no big-endian machine or image is here to run one, and the DWARF writer and layouts assume little-endian |

## Optional features

| Feature | Status in 0.10.0 | What exists, and what stops more |
| --- | --- | --- |
| WebAssembly | verified, scoped | the freestanding subset, run on Node.js's engine and held to the interpreter; and `wasm32-wasip1` (Gate 3J), with a heap that is never freed, standard streams, arguments, clocks and files beneath a preopened directory, run under Node.js's `node:wasi`; no tasks, channels, network or directory listing |
| GPU (`nazm accel`) | verified, scoped; experimental | maps, zips and reductions of `Int`, `Float32` and `Float64` kernels behind a provider boundary (Gate 3H): OpenCL on Apple's GPU (macOS) and a CPU provider, each the other's oracle; `Float64` refused on Apple's GPU; Metal, CUDA, ROCm and SPIR-V providers **blocked** — no toolchain for them here |
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
