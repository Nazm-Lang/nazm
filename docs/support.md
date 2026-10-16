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
| `thumbv8m.main-none-eabihf` | emulator-verified | unsupported | QEMU `mps2-an505`, Cortex-M33 with its FPU (M1) | not exercised |
| `riscv32imc-unknown-none-elf` | emulator-verified | unsupported | QEMU RISC-V `virt` with an RV32IMC hart (`lowrisc-ibex`), machine mode (M1) | not exercised |
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
(not Apple's). The microcontrollers (M1) run from flash: `Float64` is refused on the Cortex-M33
and every float on RV32IMC (`N0854`), and an image that does not fit is `N0850`. WebAssembly is the freestanding subset: no heap, so no strings built at run time,
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

*Nazm's own code generator (`--backend nazm`, R9 section 8, 2026-10-16).* It builds
`x86_64-pc-windows-gnu` too: PE/COFF objects with Windows x64's convention, a `.pdata`/`.xdata`
unwind entry per function and `___chkstk_ms` probes, the failure bit as LLVM keeps it on Windows.
Run-verified on a Windows host: `BACKEND=nazm scripts/windows-host.sh` on the same laptop
(10.0.18362.1256), 68 agree, 0 differ, 8 skipped, the native builds also `--backend nazm`; and
`toolchain/codegen/tests/coff.sh` compares each unit object with LLVM's for the triple and links
every program of the differential set with MinGW-w64's `gcc` (120 of 120).

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
| Windows x86_64 (`windows-latest`) | did not build until W1 (the interpreter used Unix file descriptors); since W1 the workspace builds and lints for `x86_64-pc-windows-gnu` here (below). Not yet run on CI since; MSVC is CI's default host target there |

The three new jobs gather evidence (`continue-on-error`) until each is green once.

What they found, and what changed (2026-10-16):

- **Windows: CRLF.** The runner's git checks text out with CRLF (`core.autocrlf`), and the
  repository reads its own sources as data — the mutation catalogue's multi-line patterns,
  `compiler/emit.nz`'s runtime functions — so `cargo xtask check` failed there alone.
  `.gitattributes` now checks every text file out with LF on every host; the fixtures whose line
  endings are the point stay byte for byte.
- **macOS: Nazm's own code generator's LSDAs.** The `--backend nazm` objects did not say
  `MH_SUBSECTIONS_VIA_SYMBOLS` and named no symbol at each LSDA, so `__gcc_except_tab` was one
  atom. The runner's linker (the selected Xcode's, older than the developer's) tied every
  function with landing pads to that atom's start — the first function's LSDA — so no landing pad
  was found below the first frame and every failure ended in `abort` (a signal, not status 2).
  The developer's linker resolves the address inside the atom, which hid it. Inferred, not yet
  observed on the runner: the failures were exactly the images with more than one function with
  landing pads (a C export whose one LSDA is its entry's passed there), and the developer's
  machine has no older linker to show it; the next run on the runner verifies it. The objects now carry
  both, as LLVM's do (`toolchain/codegen/macho.nz`; the test
  `nazm_backend::every_lsda_of_a_mach_o_object_is_an_atom_of_its_own`). LLVM's own builds were
  never affected: their nazm functions describe their frames in `__eh_frame`.
- **macOS: LLVM 23 renamed a remark.** The board's stack bound reads clang's frame-size remarks,
  whose pass is `prologepilog` to LLVM 22 and `prolog-epilog` from LLVM 23, which says nothing for
  the old name; `nazm` asks for both (`backend.rs`). Homebrew's `llvm` is 23 since October 2026.
- **macOS: a mismatched SDK on the runner.** Homebrew's clang uses the Command Line Tools' SDK
  (27.0 on the macos-26 image) while it links with the selected Xcode's older `ld`, which cannot
  read that SDK's `libSystem.tbd` (`tapi error`, the x86_64 links). The job points Homebrew's clang
  at the selected Xcode's SDK; a machine whose tools match needs nothing.

### The toolchain on a Windows host (W1, 2026-10-16)

`nazm` itself, cross-built here for `x86_64-pc-windows-gnu` and run on the copyright holder's
Windows 10 laptop (`ssh win-dell`, a standard account, Windows 10 Version 10.0.18362.113). Nothing
was installed there: no Rust, no clang, no MinGW-w64.

| What | Result |
| --- | --- |
| `cargo build --workspace` and `cargo clippy --workspace --all-targets -D warnings` for `x86_64-pc-windows-gnu` | passed (cross-built on macOS with MinGW-w64; the interpreter's files, processes and sockets have a Windows path, `crates/nazm-core/src/eval/host.rs`) |
| `nazm run --interpret`, `nazm check`, `nazm fmt --check` on `bench/compare/nazm/*.nz`, `examples/*.nz`, `examples/tour/*.nz`, `examples/std/*.nz` (`scripts/windows-toolchain.sh`) | 60 agree with macOS's output, error output and exit status, 0 differ (20 files × 3 commands) |
| `nazm build`, and `nazm run` without `--interpret` | exit 4: `clang` was not found on `PATH` — a native build needs clang and a linker on Windows (W2, below); its advice then named Xcode |
| The workspace's test binaries but `selfhost` (contained-only): 95, run on the laptop | 2014 passed, 892 failed, 83 ignored, none timed out. Of the failures, 785 are a native build without clang and about 40 more a tool the laptop has not got (clang or `cc` spawned directly, `cargo`, `ps`); the rest were ours: on a second run after W1's fixes, the language server's 20, workspace rename's 6, the user cache's and scratch directory's 2 and the MCP server's 7 passed; the mutation runner's 5 (`ps`, a locked file read) are POSIX-only now, campaigns running contained on Linux; `@cfg(os)` on a Windows host, a temporary directory named by `TMP`, and the DLL refusal are fixed in the tests or the compiler but wait on clang to be shown passing |
| Not run on Windows | `selfhost` (contained); the interpreter's own POSIX tests (`platform_files`, `process_control`: links, modes, `sh` children); the mutation lifecycle; tests whose child is an `sh` script |

### The toolchain's own dependencies on Windows (W2, 2026-10-16)

On the same laptop and account, with llvm-mingw 20261006 (`llvm-mingw-20261006-ucrt-x86_64.zip`,
Apache-2.0, SHA-256 `317492c456aa27ee607a5919f1d2d38dcdc1112516a24d0bf4b00d078f52d17a`, equal to
the digest GitHub publishes for the release asset; the project publishes no checksum file of its
own) unpacked under `C:\Users\nazmci\nazm-tools` and put on `PATH` for the session alone: no
administrator, no installer, no system change, no Microsoft SDK, no licence accepted. It gives
clang 23.1.3 (default target `x86_64-w64-windows-gnu`), LLD 23.1.3 and llvm-ar 23.1.3, which is what
`nazm doctor` reports there.

| What | Result |
| --- | --- |
| `nazm doctor` | clang, a program linked from IR through `ld.lld`, and `ar` all found; without llvm-mingw on `PATH` it names clang and `ar` as missing and says where to get them |
| `nazm run --interpret`, `check`, `fmt --check`, native `nazm run`, and `nazm build` with the executable then run, on the 15 files `scripts/windows-toolchain.sh` takes by default, and `nazm test` over `examples/requirements/heater.nz` and `examples/apps/nbody` (both legs) — `TOOLS=… scripts/windows-toolchain.sh` | 74 agree with macOS's output, error output and exit status (15 × 5 commands and `nazm test`, its 3 cases passing), 2 differ: `httpd`'s native run and build, which reach TLS and are refused (`N0830`) until Windows' TLS engine has run on a Windows host |
| The 95 test binaries again (all but `selfhost`), built for Windows and run on the laptop with llvm-mingw on `PATH` | 94 finished: 2392 passed, 544 failed, 89 ignored; `context` (the repository context packets) passed what it ran but stopped at the 20-minute bound. Of the failures, about 430 are tests spawning the program they built by its name without `.exe` — Windows starts nothing by that name, and a Windows executable is now `prog.exe` — and about 45 more expect the extensionless file (`native_cache`, `runtime_artifact`, `build`): the tests' names, not the builds, which `windows-toolchain.sh` shows working; 11 are TLS (`N0830`); 11 the benchmark harness's Postgres sandbox; 9 WebAssembly, which llvm-mingw's clang has no target for; about 30 a path or file a test names in POSIX form; 4 a C compiler or header the tests reach directly |

W2 changed three things for this: clang is given `TMP`, `TEMP` and `SystemRoot` (with none of them
it cannot make a temporary file to link with), a Windows host's link names MinGW-w64's libraries as
`link.txt` does, and a Windows executable is published as `prog.exe`, the name MinGW-w64's driver
writes.

Interpreter differences on Windows, beside the spec's (*Windows x86_64*): `os_lstat`'s device and
inode are `0` (the volume serial and file index need `GetFileInformationByHandle`, which a crate
that forbids `unsafe` cannot call on stable Rust); entropy is 128 bits of the system's generator
per call, expanded by SipHash, for the same reason. The benchmark harness's Postgres sandbox reads
`/dev/urandom` and does not run on Windows.

## Not supported, by decision

| | |
| --- | --- |
| Windows with Microsoft's toolchain, and Windows ARM64 | unsupported: `x86_64-pc-windows-msvc` needs Microsoft's SDK and C runtime, which are not downloaded or accepted here, and no Windows ARM64 machine or emulator is here; Windows x86_64 is `x86_64-pc-windows-gnu`, with MinGW-w64's |
| 32-bit hosted targets | a non-goal: `Int` is 64-bit and every hosted layout assumes 64-bit pointers; wasm32 — freestanding, or with WASI's imports and its own heap — and the microcontroller boards (`thumbv7m-none-eabi`, `thumbv8m.main-none-eabihf`, `riscv32imc-unknown-none-elf`) are the 32-bit targets, where a C handle, a C string result and a callback are refused by name (`N0381`, Q1-A1-05) because a C pointer crosses as 64 bits |
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
