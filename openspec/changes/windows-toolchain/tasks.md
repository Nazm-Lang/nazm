## W1 — the Rust toolchain on Windows (now)
- [x] 1.1 Each Unix-only use (8 files) given a Windows path; `cargo build --workspace` on Windows
      (cross-built for `x86_64-pc-windows-gnu`, lint clean; `eval/host.rs`; xtask's process
      groups POSIX-only)
- [x] 1.2a The host-safe suite on the Dell (`ssh win-dell`) and on CI's `windows-latest`: the Dell with llvm-mingw (`docs/user/support.md`,
      *The toolchain on a Windows host*), and the CI platform jobs and `windows-tls` green together (ddd2fac0, from 844 failures;
      the tests' `.exe` names fixed in the CI rounds that followed)
- [ ] 1.2b The Windows job required in the repository's branch protection: a repository setting, not verifiable from the tree.
      Not blocking 0.11.0; when: before 1.0
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
- [x] 3.1 R9 writes PE/COFF objects (`r9-code-generator` 8.1-8.5: `coff.nz`, `tests/coff.sh`). Not claimed: that R9's own writer built the programs that ran on Windows; `r8-cut-over` 17.1 built them with LLVM (llvm-mingw), and the Windows objects from R9's writer wait for 1.0
- [x] 3.2 The released `nazm` built for Windows by Nazm, passing the gate on Windows: `r8-cut-over` 17.1, accepted by the copyright holder 2026-11-05 as satisfying this for 0.11.0 with the shortfalls stated in its design S17 (the self-build ran on macOS and cross-built the nine; no Windows-native self-build, no `--backend nazm`, no `nazm test` on Windows: 1.0)
## W4 — release
- [ ] 4.1 A signed, reproducible Windows x86_64 archive: 1.0 (the signature is the copyright holder's)
- [ ] 4.2 An installer and a winget or Scoop entry: 1.0; not started
