# Known limitations

One list, kept current, of what Nazm does **not** do or does only partly — the v1 foundation and
everything N49–N74 added to it, as audited in N75. Each line names the
capability-matrix area that owns it (`capability-matrix.md`), where the evidence and the status
live; this file adds nothing a reader could not find there, and exists so nothing material is
found only by reading all of it.

## Language

- Traits and methods (N78) are nominal and static: no trait objects or dynamic dispatch, no default
  methods, associated items, generic traits or methods, supertraits, impls with bounds or for
  generic types, or inherent impls; one bound per type parameter; an impl is usable where its
  module is the caller's or one it imports directly. A persisted interface's traits are not
  rehydrated. *Narrowed in N78:* until then, no traits or methods. Function values and
  closures exist since N50, and effect parameters since N51 — one per function, named only in
  function types (`N0388`), with no effect subtyping; no closure or
    named function value inside a generic function (`N0387`); a closure captures by copy and
  never a `let mut` binding (`N0386`); no attenuated capabilities and no revocation
  (`architecture.md` §7.53); a field of function type is called by
  binding it first; no function value crosses into a task (`N0321`); a path that went through a
  function value carries what every function value of its type may return, and a call through a
  trait bound what every impl of that method may (N80). The compiler written in Nazm has none of it
  (areas 3, 5, 7, 16). *Narrowed in N50 and N80.*
- A module cannot re-export a name or export one under another name (area 2). A module's
  persisted resolution (N77, `nazm.resolved/1`) holds its names, not its types: `nazm build` and
  `nazm run` still check every module, and only `nazm check` and `nazm references` read it.
  *Narrowed in N77:* until then, nothing a check decided about a body outlived the process.
- A standard module's functions keep their module's name (`text_split`), qualified or not:
  `use "@std/text" as t;` gives `t::text_split`, not `t::split` (areas 2, 16). *Narrowed in N49,
  which added qualified names.*
- `\xHH` stops at `\x7F`; a lone byte above 127 needs `str_from_byte`. `\u{…}` (N76) spells a
  scalar value's UTF-8 bytes, so every valid UTF-8 literal is writable in ASCII (area 1).
  *Narrowed in N49, which added escapes, and N76, which added `\u{…}`.*
- Parsing is incremental only inside the language service, for an open document's own text, per
  top-level item: an edit reparses its whole item, one that leaves a delimiter open reparses to the
  end of the file, imported files and every other command parse in full. Recovery reaches list
  members, not a lone operand (`1 + * 2` costs its statement); a declaration or body that needed
  recovery is absent from the abstract tree by design (area 1). *Narrowed in N76:* until then,
  whole-file parsing only, and recovery only at statements.
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
- Contracts run on one chain backend, the EVM, verified against one reference EVM (py-evm), with a
  loose gas bound and none for loops; an asset is a declared conserved quantity checked per
  transaction, not a linear type; no events, external calls or signers beyond `caller` (area 34).
  WebAssembly contracts (N71) run under the reference host only; no chain's host interface is bound.
  Account-oriented (sBPF) contracts have signer analysis and metadata only: no sBPF toolchain or
  validator exists here, so none is built or run (N72).
  *Narrowed in N69, N70 and N71:* until then, no contract model, then no backend.
- Layout specialisation is one opt-in layout: `--layout soa` lays out every `Vec` of a record of
  `Int`s and `Bool`s one array per field, whether or not its loops gain — it is 2× slower for a loop
  reading every field; the compiler does not choose; LLVM only (area 4). *Narrowed in N68:* until
  then, every element was laid out whole.
- Accelerators are one command over one API: `nazm accel` maps a pure `(Int) -> Int` function over
  `Int`s through macOS's OpenCL on Apple's GPU; no kernels in the language, no other vendor, API or
  operating system, no layout specialisation (area 21). *Narrowed in N67:* until then, no GPU path.
- Vectorisation is one idiom: the counted `Ints` summation loop, replaced by `ints_sum_from` in native
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

- Capabilities are coarse: an `IoCap` authorises every file alike, and the one narrower kind is
  `OutCap`, the standard streams. No per-file or per-directory kind exists, and revocation is
  outside the model by design (area 6). *Narrowed in N79.*
- By default an undeclared function exercises its caller's authority (N37's bridge). Since N79 that
  inheritance is recorded per function (`nazm inspect`'s `inherits`) and refused under the
  `authority` profile, but the default is unchanged until the corpus and the compiler written in
  Nazm declare their authority (areas 6, 19, 20).
- Effect subsumption applies where a function value is passed or returned, not inside another
  type: a `Vec[fn() -> Int]` is not a `Vec[fn() -> Int ! { io }]` (area 5).
- Provenance has three sink classes (`path`, `output`, `file-data`) solved through helpers,
  closures and modules, and one language restriction (`N0372`, the path); the rest is profile
  policy, checked by the same engine. Declassification is `str_vouch` under a `VouchCap`, recorded.
  A container is precise by **type**, not by handle or index: one write of a file's contents into
  any `Strs` reaches every `Strs` read, and a container in generic code reaches every cell. Records
  and variants are precise by field only within a body. Implicit flow is not tracked (area 7).
  *Narrowed in N52 and N80.*

## Native code

- Six targets, no others: four hosted — `aarch64`/`x86_64` × macOS/Linux — and two freestanding,
  below; no Windows, 32-bit or CPU-feature selection; a target the host has no linker for is built to objects (`--objects`) for its own
  toolchain, with no bundled sysroot or linker; `x86_64-unknown-linux-gnu` is compile-only here
  (area 33). *Narrowed in N57:* until then, the host only.
- A build generates its runtime unless given `--runtime DIR` (N82): the artifact is the way to build
  a runtime once and check what is linked, not yet the default. The runtime is LLVM text, not Rust,
  and has no allocator of its own (area 13).
- The two native backends each translate MIR's operations themselves; their agreement rests on
  differential testing (N73, N81), not on one lowering. Byte layouts and ownership are `nazm-lir`'s
  and validated on every build (N81), but LLVM lays its own aggregates out, an enum as the product
  of its variants (area 10).
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

- By default one OS thread per task; with `NAZM_SCHEDULER=pool` (N83) tasks run on a bounded pool
  of workers, pinned, without work stealing or preemption — a task that computes without waiting
  holds its worker, a blocking foreign call too, and a task's stack is fixed (`NAZM_TASK_STACK`).
  The pool is verified on macOS only, and the interpreter and the compiler written in Nazm keep a
  thread per task. No detached tasks; deadlock is possible and not detected, on either model
  (areas 14, 15). *Narrowed in N54:* select over typed channels, a select deadline under
  a `TimeCap`, and cancellation by closing a channel a task selects on exist; nothing is cancelled
  on a sibling's failure, nothing selects over sends or over the `Int` channel, and the stackful
  pool §7.56 chose is not built. So the tasks alive at once are bounded by
  the host's thread limit — a container's process ceiling counts every thread — and a `spawn` the
  host refuses is `N0404`, not a wait.
- A channel carries one element type, and only one that may cross into a task — `Int`, `Bool`,
  `Str`, channels, and records and enums of those (`N0390`); no unbounded channel, no channel of
  sequences or closures (area 14). *Narrowed in N53:* until then, `Int` only.
  *Narrowed in N54:* `select` exists for `Chan[T]` (lowest ready index; receives only).
- The runtime is its own crate (`nazm-runtime`, N53) but still LLVM text both backends link,
  tested through a C harness; no Rust implementation, no runtime shared across programs as a
  library (area 13).

## Packages and builds

- A local registry and four requirement forms (N56); no remote registry or network source, no
  signatures, no features or pre-releases, one version per package and no backtracking solve, no
  multi-root workspaces (area 26). *Narrowed in N56:* until then, path dependencies only.
- Project tooling (N74): six templates, a library check, `nazm doc` from semantic definitions, a
  publish dry run; no `nazm update`, no hosted documentation, no template beyond the six (area 26).
  `nazm bindgen` translates only the `int64_t`/`bool`/`const char *` subset and reads declarations,
  not the C preprocessor (area 17). The editor client (`editors/vscode`) is not exercised inside an
  editor here or published; rename across a package boundary is refused (area 25).
- Reproducibility is verified on one host. Across toolchains it is measured, not claimed: Apple
  clang 21 and clang 19 give the same program objects and a different runtime object (N73, area 32).
  A provenance record is evidence of what a build read and wrote, not a proof; Nazm neither signs nor
  verifies an attestation. The compiler's bill of materials is what `Cargo.lock` claims; nothing
  checks a crate against its upstream.
- Fuzzing is seeded generation, not coverage-guided; the differential generator writes `Int` and
  `Bool` programs only (N73, area 30). *Narrowed in N73:* until then, no fuzzing.
- The compiler written in Nazm understands neither packages nor `@std` imports nor foreign
  declarations (areas 16, 17, 26).

## Profiles

- Formal evidence is a bounded model check of a small core (`docs/formal-core.md`, N61): every
  program of integers, booleans, `if` and `let` up to five nodes, against the interpreter and the
  checker; no proof assistant, nothing beyond the bound, and nothing about functions, loops,
  strings, effects, tasks or the native backends' code (area 19).

- Fifteen rules: N79 added `explicit-authority` (the `authority` profile); N60 added `no-recursion`, `no-select` and `no-ambient-time` to N52's, N63
  `no-blocking` and `bounded-loops` (the `realtime` profile), N72 `signed-writes` (`accounts`); the
  stack rule bounds depth, not bytes, and is `unknown` through a function value; no constant-time
  rule, and a loop is bounded only in its counted form; `critical` stays a restriction set, not a
  certification; a profile costs one extra check, `embedded` a MIR lowering (+13 % on the compiler's
  source) (areas 18–20).
