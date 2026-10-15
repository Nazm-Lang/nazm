## W1 — the Rust toolchain on Windows (now)
- [ ] 1.1 Each Unix-only use (8 files) given a Windows path; `cargo build --workspace` on Windows
- [ ] 1.2 The host-safe suite on the Dell (`ssh win-dell`) and on CI's `windows-latest`; the CI job
      required once green
- [ ] 1.3 `nazm build`, `run`, `fmt`, `check`, `test` used on the Dell end to end

## W2 — the host's dependencies
- [ ] 2.1 clang/LLVM and the linker found on Windows; versions recorded; `nazm doctor` says what is missing
- [ ] 2.2 Install instructions for Windows in the README and on nazm.dev

## W3 — the Nazm-written toolchain on Windows (after the cut-over)
- [ ] 3.1 R9 writes PE/COFF objects (or LLVM serves fast builds on Windows until it does)
- [ ] 3.2 The released `nazm` built for Windows by Nazm, passing the gate on Windows

## W4 — release
- [ ] 4.1 A signed, reproducible Windows x86_64 archive in 1.0
- [ ] 4.2 An installer and a winget or Scoop entry
