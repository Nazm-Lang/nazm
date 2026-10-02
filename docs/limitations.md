# Known limitations of the v1 foundation

One list, kept current, of what Nazm v1 does **not** do or does only partly. Each line names the
capability-matrix area that owns it (`capability-matrix.md`), where the evidence and the status
live; this file adds nothing a reader could not find there, and exists so nothing material is
found only by reading all of it.

## Language

- No traits or methods (designed and deferred, `architecture.md` §7.52). Function values and
  closures exist since N50, and effect parameters since N51 — one per function, named only in
  function types (`N0388`), with no effect subtyping; no closure or
    named function value inside a generic function (`N0387`); a closure captures by copy and
  never a `let mut` binding (`N0386`); no attenuated capabilities and no revocation
  (`architecture.md` §7.53); a field of function type is called by
  binding it first; no function value crosses into a task (`N0321`); a path that went through a
  function value is of unknown origin, conservatively. The compiler written in Nazm has none of
  it (areas 3, 5, 7, 16). *Narrowed in N50.*
- A standard module's functions keep their module's name (`text_split`), qualified or not:
  `use "@std/text" as t;` gives `t::text_split`, not `t::split` (areas 2, 16). *Narrowed in N49,
  which added qualified names.*
- String escapes stop at `\x7F`; a byte above 127 that UTF-8 source does not spell needs
  `str_from_byte`, and there is no `\u{…}` (area 1). *Narrowed in N49, which added escapes.*
- Native recursion is bounded by the thread's stack, not by the language: `N0408` arrives before
  the stack is exhausted only while the non-recursive calls between two checks fit the quarter
  kept as headroom; no tail-call optimisation; the depth at which it arrives depends on the
  backend and the optimisation level (areas 11, 12). *Narrowed in N49, which compiles recursion.*
- The compiler written in Nazm refuses string escapes, `&&`, `||`, `!`, `::`, `as` and recursion
  by name: its bootstrap subset is narrower than the language (area 31).
- Allocation is tracked per site, not per byte: `nazm explain-cost` reports each allocation,
  retain, block, channel, foreign and task site with a count per call that is `at-most-once` or
  `unknown` (a loop), and `embedded` refuses an `unknown` allocation; there is no byte bound, no
  whole-program count, and recursion is seen only through its calls' sites (areas 4, 18).
  *Narrowed in N52.*
- Vectorisation is one idiom: the counted `Ints` summation loop, replaced by `ints_sum` in native
  builds at `--opt-level 2` and above; no element-wise maps, no vector types, baseline target
  features only, and a fallback 1.33× slower than the scalar loop when elements exceed 2^56
  (area 28). *Narrowed in N66:* until then, checked arithmetic kept every loop scalar.
- No JIT: an in-process JIT needs `unsafe` code, which the workspace forbids; the interactive tier
  is the interpreter. Comptime is a command, not a language construct; nothing is hot-reloaded —
  `nazm reload-check` states the rule a reloader would follow (area 12). *Narrowed in N65:* until
  then, no REPL, comptime or reload rule existed.
- WebAssembly is the freestanding subset only: no heap, so strings built at run time, sequences,
  channels, tasks and closures are refused; imports are `nazm_host.write` and `.report` alone, no
  WASI; only `nazm_main` is exported; verified on Node's engine only, linked by `wasm-ld` outside
  this host (area 33). *Narrowed in N64:* until then, no WebAssembly target.
- Real-time bounds cover an analysable subset only: a loop is bounded only in the one counted
  form (`realtime`'s `bounded-loops`), a stack bound exists only for a board image without a call
  cycle or a call through a value, and no execution time, latency or jitter is bounded — `wcet` is
  always `null` (area 18). *Narrowed in N63:* until then, no loop or stack bound was stated.

## Effects, authority and information flow

- Capabilities are coarse: an `IoCap` authorises every file and stream alike; no attenuation or
  revocation (area 6).
- An undeclared function exercises its caller's authority (N37's bridge) unless a restriction
  profile refuses it (areas 6, 19, 20).
- Provenance has three sink classes (`path`, `output`, `file-data`) solved through helpers,
  closures and modules, and one language restriction (`N0372`, the path); the rest is profile
  policy. Declassification is `str_vouch` under a `VouchCap`, recorded. A container is still
  coarse: whatever is read out of a sequence or channel is `unknown` (area 7). *Narrowed in N52.*

## Native code

- Four targets, no others: `aarch64`/`x86_64` × macOS/Linux, no Windows, 32-bit or CPU-feature
  selection; a target the host has no linker for is built to objects (`--objects`) for its own
  toolchain, with no bundled sysroot or linker; `x86_64-unknown-linux-gnu` is compile-only here
  (area 33). *Narrowed in N57:* until then, the host only.
- One freestanding target, `aarch64-unknown-none`, verified only on QEMU's `virt` board: objects and
  `link.ld`, linked by `ld.lld` outside this host; no atomics, interrupts, heap or `@std`; a board
  program that reaches allocation, input and output, tasks, channels, closures or the C library is
  refused; Cranelift builds no board (areas 18, 33). *Narrowed in N62:* until then, no target
  without an operating system.
- Cranelift keeps every non-scalar local in a stack slot; its loops are slower than LLVM's
  (area 12, `performance.md`).
- Both backends still need clang for the runtime's LLVM text and the C compiler driver for the
  link; `nazm inspect` names both (areas 11, 12). The LLVM emitter promotes only scalar temporaries
  within one block; other locals keep a slot at `-O0` (N59).
- Debug information through the LLVM backend only (Cranelift refuses `--debug`): functions, lines,
  and `Int`, `Bool` and `Str` variables, with no lexical scopes; a live session verified with gdb in
  the Linux container, not under lldb on macOS, whose developer mode a non-interactive run cannot
  enable; profiling is counts of one run, not sampling (area 27). *Narrowed in N58.*

## FFI

- `Int` and `Bool`, and a `Str` *argument* as a borrowed C string (N55); no pointers or opaque
  handles, `Str` results, structs, callbacks, variadics or `errno`; C is trusted entirely once
  called; `nazm run` cannot call C (area 17). *Narrowed in N55:* until then, import only and no
  strings. Exports exist — public, `! {}`, `Int`/`Bool` only — and `nazm build --lib` writes a
  static archive and its header; one Nazm library per C program, no dynamic library.

## Runtime and concurrency

- One OS thread per task; no worker pool, no detached tasks; deadlock is possible and not
  detected (areas 14, 15). *Narrowed in N54:* select over typed channels, a select deadline under
  a `TimeCap`, and cancellation by closing a channel a task selects on exist; nothing is cancelled
  on a sibling's failure, nothing selects over sends or over the `Int` channel, and the stackful
  pool §7.56 chose is not built. So the tasks alive at once are bounded by
  the host's thread limit — a container's process ceiling counts every thread — and a `spawn` the
  host refuses is `N0404`, not a wait.
- A channel carries one element type, and only one that may cross into a task — `Int`, `Bool`,
  `Str`, channels, and records and enums of those (`N0390`); no `select`, no unbounded channel,
  no channel of sequences or closures (area 14). *Narrowed in N53:* until then, `Int` only.
  *Narrowed in N54:* `select` exists for `Chan[T]` (lowest ready index; receives only).
- The runtime is its own crate (`nazm-runtime`, N53) but still LLVM text both backends link,
  tested through a C harness; no Rust implementation, no runtime shared across programs as a
  library (area 13).

## Packages and builds

- A local registry and four requirement forms (N56); no remote registry or network source, no
  signatures, no features or pre-releases, one version per package and no backtracking solve, no
  multi-root workspaces (area 26). *Narrowed in N56:* until then, path dependencies only.
- Reproducibility is verified on one host; nothing is claimed across hosts or toolchains
  (area 32).
- The compiler written in Nazm understands neither packages nor `@std` imports nor foreign
  declarations (areas 16, 17, 26).

## Profiles

- Formal evidence is a bounded model check of a small core (`docs/formal-core.md`, N61): every
  program of integers, booleans, `if` and `let` up to five nodes, against the interpreter and the
  checker; no proof assistant, nothing beyond the bound, and nothing about functions, loops,
  strings, effects, tasks or the native backends' code (area 19).

- Eleven rules since N60 (`no-recursion`, `no-select` and `no-ambient-time` added to N52's); the
  stack rule bounds depth, not bytes, and is `unknown` through a function value; no constant-time
  rule, no bounded-loop, queue or task rule; `critical` stays a restriction set, not a
  certification; a profile costs one extra check, `embedded` a MIR lowering (+13 % on the compiler's
  source) (areas 18–20).
