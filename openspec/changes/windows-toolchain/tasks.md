## W1 — the Rust toolchain on Windows (now)
- [x] 1.1 Each Unix-only use (8 files) given a Windows path; `cargo build --workspace` on Windows
      (cross-built for `x86_64-pc-windows-gnu`, lint clean; `eval/host.rs`; xtask's process
      groups POSIX-only)
- [ ] 1.2 The host-safe suite on the Dell (`ssh win-dell`) and on CI's `windows-latest`; the CI job
      required once green — **the Dell done** (`docs/user/support.md`, *The toolchain on a Windows
      host*: 2014 passed, 892 failed, 785 of them for want of clang; again with llvm-mingw (W2):
      2392 passed, 544 failed, ~475 of them tests naming a built program without `.exe`); CI's run
      and flip are the coordinator's; the tests' `.exe` names are left
- [x] 1.3 `nazm build`, `run`, `fmt`, `check`, `test` used on the Dell end to end — `run
      --interpret`, `check`, `fmt` agree with macOS (`scripts/windows-toolchain.sh`, 60/60); with
      llvm-mingw (`TOOLS=…`), native `run`, `build` and `test` too: 74 agree, 2 differ (`httpd`,
      TLS refused, `N0830`)

## W2 — the host's dependencies
- [x] 2.1 clang/LLVM and the linker found on Windows; versions recorded; `nazm doctor` says what is missing
      — llvm-mingw 20261006 (clang, LLD, llvm-ar 23.1.3) on `PATH`; `docs/user/support.md`, W2
- [x] 2.2 Install instructions for Windows in the README and on nazm.dev (`docs/user/getting-started.md`,
      *On Windows*)

## W3 — the Nazm-written toolchain on Windows (after the cut-over)
- [ ] 3.1 R9 writes PE/COFF objects (or LLVM serves fast builds on Windows until it does)
- [ ] 3.2 The released `nazm` built for Windows by Nazm, passing the gate on Windows

## W4 — release
- [ ] 4.1 A signed, reproducible Windows x86_64 archive in 1.0
- [ ] 4.2 An installer and a winget or Scoop entry
