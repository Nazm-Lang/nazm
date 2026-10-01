# Known limitations of the v1 foundation

One list, kept current, of what Nazm v1 does **not** do or does only partly. Each line names the
capability-matrix area that owns it (`capability-matrix.md`), where the evidence and the status
live; this file adds nothing a reader could not find there, and exists so nothing material is
found only by reading all of it.

## Language

- No function values, closures, traits or methods: no `map`, `filter` or higher-order library
  code (areas 3, 16).
- No qualified names: a standard module's functions carry its name (`text_split`) (area 16).
- No string escapes: `str_from_byte(10)` is the newline (area 1).
- No logical operators: `&&`, `||` and `!` do not exist; conditions nest (area 1).
- Recursion runs under `nazm run` and is refused by both native backends (areas 11, 12).
- Allocation is not tracked; no profile can restrict it (areas 4, 18).

## Effects, authority and information flow

- Capabilities are coarse: an `IoCap` authorises every file and stream alike; no attenuation or
  revocation (area 6).
- An undeclared function exercises its caller's authority (N37's bridge) unless a restriction
  profile refuses it (areas 6, 19, 20).
- Provenance restricts one sink, `write_file`'s path, in functions with declared effects; no
  declassification; containers are coarse (area 7).

## Native code

- Host target only (aarch64-apple-darwin verified); other v1 triples parse and are refused for
  building (area 33).
- Cranelift keeps every non-scalar local in a stack slot; its loops are slower than LLVM's
  (area 12, `performance.md`).
- A Cranelift build still needs clang for the runtime and the link (area 12).
- Debug information: function and statement positions through the LLVM backend only; no
  variables; no live debugger session verified (area 27).

## FFI

- `Int` and `Bool` only; no pointers, strings, structs, callbacks, variadics or `errno`; import
  only; C is trusted entirely once called; `nazm run` cannot call C (area 17).

## Runtime and concurrency

- One OS thread per task; no worker pool, cancellation, timeouts, select or detached tasks;
  deadlock is possible and not detected (areas 14, 15). So the tasks alive at once are bounded by
  the host's thread limit — a container's process ceiling counts every thread — and a `spawn` the
  host refuses is `N0404`, not a wait.
- Channels carry `Int` only (area 14).
- No runtime crate: the runtime is LLVM text, tested through a C harness (area 13).

## Packages and builds

- Path dependencies only: no registry, no remote sources, no version ranges, no multi-root
  workspaces (area 26).
- Reproducibility is verified on one host; nothing is claimed across hosts or toolchains
  (area 32).
- The compiler written in Nazm understands neither packages nor `@std` imports nor foreign
  declarations (areas 16, 17, 26).

## Profiles

- Five rules; no allocation, stack, determinism, constant-time or declassification rule; a
  profile costs one extra check (areas 18–20).
