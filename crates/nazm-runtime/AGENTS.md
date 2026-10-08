# crates/nazm-runtime/ — the runtime, as LLVM text

Specialises the root `AGENTS.md` for this subtree; it relaxes nothing there.

- **A runtime contract change bumps `RUNTIME_ABI_REVISION`** (`src/lib.rs`) in the same commit, with
  its line in the revision history beside it and the row in `docs/stability.md`.
- **Every function the runtime defines or imports is in `INVENTORY` / `IMPORTS`** with its exact
  signature, part, failure and ownership; the crate's tests hold the text to the inventory, and a
  reserved C symbol the runtime declares must also be in `RESERVED_C_SYMBOLS`
  (`crates/nazm-core/src/check/foreign.rs`; held by `tests/ffi.rs`).
- **Texts listed in the `runtime parity` gate** (`xtask/src/main.rs`, `check_runtime_parity`
  `GROUPS`) are mirrored line for line in `compiler/emit.nz`. A new runtime part gets **its own
  constant**, outside those groups unless the self-hosted compiler needs it (`TEXT_RUNTIME` is the
  pattern), rather than growing a parity-checked one.
- Platform facts — the OS declaration list (`src/os.rs`), context switch and errno location
  (`src/switch.rs`) — stay in their files; `docs/maintainability.md`, *Platform*, is the map.
