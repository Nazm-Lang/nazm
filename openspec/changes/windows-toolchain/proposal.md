## Why

Nazm is a general-purpose language for every user's device (the copyright holder, 2026-10-15). Since
2026-10-15 programs built for `x86_64-pc-windows-gnu` run on a real Windows 10 laptop and agree with
the macOS build (`scripts/windows-host.sh`). The toolchain itself — `nazm` running on a Windows PC —
does not build there: GitHub's Windows runner showed `nazm-core`'s interpreter using Unix file
descriptors. A Windows developer must be able to install `nazm` and write, build, run, format,
check and test programs on their own machine.

## What Changes

- **W1 — the Rust toolchain builds and passes its host-safe tests on Windows.** Eight files use
  Unix-only APIs (`std::os::unix`, `std::os::fd`, `libc`: three in nazm-core, two in nazm-cli, one each
  in nazm-codegen-nazm, nazm-bench, nazm-agent-bench). Each gets a Windows path; CI's
  `windows-latest` job turns green and becomes required. Needed beyond today: the Rust toolchain is
  the frozen bootstrap seed, and a clean Windows machine must be able to build the Nazm toolchain
  from source with it.
- **W2 — the host's own dependencies.** On Windows `nazm build` uses LLVM's `clang` for optimised
  builds and MinGW-w64's (or LLVM's `lld`) linker; how they are found, what versions, and what the
  install says. No Microsoft SDK or C runtime licence is downloaded or accepted (the earlier rule).
- **W3 — the Nazm-written toolchain on Windows.** After the cut-over (R8) the released `nazm` is
  Nazm built by Nazm, so it runs on Windows by being built for the Windows target — the runtime's
  Windows layer already runs there. R9's code generator writes Mach-O today; it gains a PE/COFF
  object writer, or Windows' fast builds use LLVM until it has one.
- **W4 — release.** 1.0 ships a Windows x86_64 archive beside macOS's, signed and reproducible as
  macOS's is; the Dell (`ssh win-dell`) and CI run the Windows gate; an installer and a package
  manager entry (winget or Scoop) follow.

## Order

W1 now (small, and the seed needs it). W2 with W1. W3 after the cut-over, in stage P's window.
W4 at the 1.0 gate. Linux follows the same steps after 1.0.

## Capabilities

### New Capabilities
- `windows-toolchain`: `nazm` runs on Windows x86_64. Archived with `--skip-specs`.

## Impact

The eight Unix-specific files, `scripts/` packaging, CI, `docs/user/support.md`, R9's object writer, the
release gate. Nothing a program means changes.
