# Known limitations

One list, kept current, of what Nazm does **not** do or does only partly — the v1 foundation and
everything N49–N74 added to it, as audited in N75, and kept current through the 1.0 draft (Gate 1,
2026-10-07; the 0.9.0 preview, 2026-10-12; 0.10.0, 2026-10-15). What 1.0 will promise about what is here is
`stability.md`. Each line names the
capability-matrix area that owns it (`capability-matrix.md`), where the evidence and the status
live; this file adds nothing a reader could not find there, and exists so nothing material is
found only by reading all of it.

## Language

- A closure may not capture a value that can hold a function value behind a counted handle — a
  `Vec[fn(…) -> R]`, a `Vec` of records or enums with a function field, or a record or enum holding
  one (`N0616`, Gate 1-C1). It is decided by type where the closure is written, so it also refuses
  a closure that would never have closed a cycle, such as one that only reads a vector of
  handlers; pass the vector as an argument instead (area 4). *Narrowed from a defect:* Gate 1
  found that such a closure, stored into the `Vec` it captured, was never reclaimed; the refusal
  closes that route and no collector was added.
- `loop`, loop labels, valueless `return` and mutable parameters are not in 1.0, each
  refused by name (`N0101`, `N0311`). *Narrowed in Gate 2:* floating point and integers other than
  the 64-bit `Int` exist (`spec.md`, *Numbers*). *Narrowed in R1a:* `for x in e` and `for x in a..b`
  exist (`spec.md`, *`for` loops*); a `for` of any other shape is still refused by name. The
  compiler written in Nazm (`compiler/*.nz`) does not carry `for` until R2 of the rewrite.
- Patterns (R1a, stable since 0.10.0; guards since R1, 5b.17): no range, record or `@` patterns, no `..`
  in a payload; the compiler written in Nazm parses only the plain variant pattern until R2.
- Warnings (R1b, L15, stable since 0.10.0): `nazm check`, `nazm build` and the language
  service report them; `nazm run` and `nazm test` do not. A module that warns is checked again on
  every run rather than reused from the check cache. A use of a `@deprecated` item still draws no
  warning (`N0620` stays reserved): that needs deprecation carried in a module's interface.
- Interpolation (R1a, stable since 0.10.0): a hole stays on one line; a format is
  alignment, width in bytes and a float's precision — no fill character, no zero padding, no
  hexadecimal (`@std/fmt` has `fmt_int_zero` and `fmt_hex`); a hole's own parse errors are one
  `N0006` at its braces rather than the parser's own diagnostic; the compiler written in Nazm does
  not read `f"…"` until R2.
- Multi-line and raw strings (R1a, stable since 0.10.0): an escape refused inside a
  multi-line string is reported at its opening delimiter rather than at the escape; the compiler
  written in Nazm does not read either form until R2.
- Constant tables (R1a, stable since 0.10.0): every use of an aggregate constant builds
  it where it is used, so indexing a large table in a loop builds the table each time — a
  read-only table in the binary is an optimisation not yet made; an imported constant is not an
  array's length in a type declaration (field types are resolved before imports' constants are
  known); a constant's parts are not calls, so a table of function values is not a constant.
- Derived ordering and hashing (R1a, stable since 0.10.0): no `T: Ordered` requirement on
  a type parameter until `@std/ordmap` (S3); no customised `<`; inside a generic body `Pair[T]` does
  not derive `Hash`, only a concrete type does; a record holding a type with a written `Hash` impl
  needs its own impl, which a generic type has through an impl of the generic type (L11b) and a
  tuple cannot have.
- `defer` (R1b, stable since 0.10.0) runs on the ways a block is left, not on a failure
  that stops the program — there is no unwinding. The compiler written in Nazm does not carry it
  until R2.
- Impls of generic types (R1b, stable since 0.10.0) are for a generic record or enum
  applied to exactly the impl's parameters: no impl for one instance (`Pair[Int, Str]`), none for
  `Vec[T]` or the other built-in generic types, and no specialisation. The compiler written in Nazm
  carries neither L11 part until R2.
- Tuples (R1a, stable since 0.10.0): two to eight elements, no unit type `()`, no tuple
  structs; the compiler written in Nazm does not carry tuples until R2.
- `Box[T]` and `Chain[T]` (R1a, stable since 0.10.0; the sequence was `List[T]` until R1.5)
  compare by what they hold (L6) but have no order or derived hash, and do not cross into a task (`N0321`); a `Chain` is built
  only from a `Vec` (`chain_from_vec`) or empty — there is no functional push, by design, since one
  would copy. The compiler written in Nazm does not carry either until R2.
- `Cell[T]` (R1b, stable since 0.10.0) has no equality and does not cross into a task, as
  a `Vec` does not; there is no swap or update-in-place, only `cell_get` and `cell_set`. The
  compiler written in Nazm does not carry it until R2.
- Arrays (Gate 2) hold plain data only — numbers, `Bool`s, and records, enums and arrays of them —
  at most 64 KiB, and do not cross the C boundary; there are no slices or views of an array, no
  iteration form beyond `while`, and a method call on an element is written `(xs[i]).m()` —
  as is completion after an element whose index is a name: at a hole, `xs[i].` reads as type
  arguments, as every `name[…]` did before Gate 2.
  Constants (Gate 2) are module-private (`pub const` is refused) and are computed from literals,
  other constants, operators and numeric conversions only.
- Bytes and text (Gate 2): a `Bytes` does not cross into a task or the C boundary yet, and
  there is no view of an array or a `Str` as `Bytes` without a copy (`bytes_from_str`). `Text` has
  no grapheme, normalisation, case-mapping or locale operations, and `Text(s)` validates every
  time — nothing remembers that a `Str` was checked.
- Numbers (Gate 2): nothing converts implicitly — every mixed operation is written with a
  conversion — and a local has no type annotation, so a typed constant is written `UInt8(250)`.
  The C library's `float_sin` and its kin are as accurate as the host's library and may differ
  in the last place between hosts. There is no `Int64` alias, no 128-bit integer, no decimal
  floating point, no access to floating-point flags or rounding modes, and no SIMD vector type in
  the language. The EVM target refuses every number but `Int`; an accelerator kernel computes in one
  type, `Int`, `Float32` or `Float64`, never two (Gate 3).
- Attributes (Gate 2): a closed vocabulary — `@cfg`, `@test`, `@bench`, `@fuzz`, `@deprecated` —
  written only before a top-level declaration, not before a method inside an `impl` or a
  `trait`. `@deprecated` is metadata-only (G2-C1): `nazm doc` publishes it on a `pub` function,
  `struct` or `enum`, and no diagnostic reports a use — compiler warnings are not part of 1.0. `@cfg(feature = …)` holds for nothing until a package can enable a feature.
- Package features (Gate 2): additive only — no way to switch a dependency's `default` off;
  `--features` reaches the root package alone; the lockfile records the manifests' sets, not a
  command's `--features`.
- Tests (Gate 2): a test or fuzz target answers `Result[Int, Str]` and takes only capabilities
  (and a fuzz target's `Bytes`); each test is built separately, so a module of many tests builds
  many times. Fuzzing is a fixed sequence of inputs up to 64 bytes, with no coverage feedback, no
  corpus and no minimisation. `nazm bench` takes one measurement of a fixed count, with no
  warm-up or statistics. Tests run for the host's configuration only.

- Traits and methods (N78) are nominal and static: no trait objects or dynamic dispatch, no default
  methods, associated items, generic traits or methods, supertraits, impls with bounds or for
  generic types, or inherent impls; one bound per type parameter; an impl is usable where its
  module is the caller's or one it imports directly. A check against a persisted interface alone,
  without the module's source, cannot use its traits (a cached check can: N107 checked it). *Narrowed in N78:* until then, no traits or methods. Function values and
  closures exist since N50, and effect parameters since N51 — one per function, named only in
  function types (`N0388`), with effect subsumption only where a function value is passed or
  returned (N79); a closure captures by copy, except a `let mut` binding, which it shares
  (R1b), and never one whose type can hold a function value (`N0386`); no attenuation beyond `OutCap` (N79) and no revocation
  (`architecture.md` §7.53); a field of function type is called by
  binding it first; no function value crosses into a task (`N0321`); a path that went through a
  function value carries what every function value of its type may return, and a call through a
  trait bound what every impl of that method may (N80). The compiler written in Nazm has closures,
  function values and traits since N102, and not provenance (areas 3, 5, 7, 16). *Narrowed in N50, N80
  and N102.*
- A module's persisted resolution (N77, `nazm.resolved/1`) holds its names, not its types:
  `nazm build` and `nazm run` still check every module — a warm build reuses objects, not checks —
  and only `nazm check` and `nazm references` read it (area 2). A trait is re-exported under its own
  name only, and `pub use "PATH" as m;` is refused. *Narrowed in N77:* until then, nothing a check
  decided about a body outlived the process. *Narrowed in N103:* a module may re-export a name, and
  under another name.
- The standard library is the 1.0 draft (N84), to be stable within 1.x from 1.0, but has no networking, no
  cryptography and no floating point; `@std/map` inserts in linear time (a sorted array), and
  channel helpers exist for `Int` and `Str` only, since a channel cannot carry a type parameter
  (area 16).
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
- The compiler written in Nazm refuses string escapes, `::` and `as` by name: its bootstrap subset
  is narrower than the language (area 31). *Narrowed in N102: `&&`, `||`, `!` and recursion.*
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
- Accelerators are one command over two providers: `nazm accel` maps a pure `(T) -> T` function,
  or zips a `(T, T) -> T` one over two inputs — `T` an `Int`, or since Gate 3 a `Float32` or
  `Float64` — through macOS's OpenCL on Apple's GPU or the CPU provider (the same kernel compiled as
  C on a thread pool), and folds the results (`add`, `min`, `max`) on the host in index order; no
  kernels in the language, no other GPU vendor or API (Metal, CUDA, ROCm and SPIR-V named future
  providers; Metal and SPIR-V BLOCKED here for want of a toolchain), no device-side reduction, no
  layout specialisation; a light workload is slower than a native CPU loop once transfers are
  counted (area 21). A float kernel calls only `float_sqrt` and `float_abs`; Apple's GPU has no
  `Float64` (`cl_khr_fp64`), so such a kernel runs on the CPU provider; a device that flushes
  subnormal `Float32`s would differ from the interpreter there, and the check reports it. The CPU
  provider's GPU cross-check is a second oracle, not a proof. *Narrowed in Gate 3:* until then, one
  provider and `Int` only; *in N88:* maps of one sequence only; *in N67:* no GPU path.
- Vectorisation is one idiom: the counted `Ints` summation loop, replaced by `ints_sum_from` in native
  builds at every `--opt-level`, on both backends (until Q1 this line said `2` and above, which the
  code never did), and only when nothing between the counter's `= 0` and the loop can write the
  counter (Q1-B-01); no element-wise maps, no vector types, baseline target
  features only, and a fallback 1.33× slower than the scalar loop when elements exceed 2^56
  (area 28). *Narrowed in N66:* until then, checked arithmetic kept every loop scalar.
- No JIT: an in-process JIT needs `unsafe` code, which the workspace forbids; the interactive tier
  is the interpreter. Comptime is a command, not a language construct; nothing is hot-reloaded —
  `nazm reload-check` states the rule a reloader would follow (area 12). *Narrowed in N65:* until
  then, no REPL, comptime or reload rule existed.
- The mobile targets (Gate 3) are libraries and programs over the C ABI, not apps: Android's are
  run only as static Bionic executables on an aarch64 Linux kernel — never on an Android device or
  emulator — and iOS's only linked, never run, until a simulator runtime is installed; no UI
  framework, no Java, Kotlin or Swift bindings, no `.xcframework`. The desktop example is one Cocoa
  window on this host; a Nazm GUI framework is not v1's.
- WebAssembly with WASI (`wasm32-wasip1`, Gate 3) is preview 1 on one thread: no tasks, channels
  or sockets (refused by name), no directory listing, an empty environment, permission bits
  reported as `0o644`, and a heap that is a bump arena never returned — a long-running module's
  memory only grows. Verified under Node's `node:wasi` only; wasmtime is not installed here.
- WebAssembly is the freestanding subset only: no heap, so strings built at run time, sequences,
  channels, tasks and closures are refused; imports are `nazm_host.write` and `.report` alone, no
  WASI; only `nazm_main` is exported; verified on Node's engine only, linked by `wasm-ld` outside
  this host (area 33). *Narrowed in N64:* until then, no WebAssembly target.
- Real-time bounds cover an analysable subset only: a loop is bounded only in the one counted
  form (`realtime`'s `bounded-loops`), a stack bound exists only for a board image without a call
  cycle or a call through a value, and no execution time, latency or jitter is bounded — `wcet` is
  always `null` (area 18). *Narrowed in N63:* until then, no loop or stack bound was stated.
- On a board, `float_round`, `float_min` and `float_max` call the C library, which a board has not
  got: a build that reaches them is refused by name; `float_sqrt`, `float_abs`, `float_floor`,
  `float_ceil` and `float_trunc` are instructions (Gate 3). The domain workloads round and clamp by
  comparison.
- The storage engine (Gate 3) is a reference application, not a database: sixteen fixed pages of
  `Int` keys and values, a redo log of whole page images, one process by a lock file a crash leaves
  (`recover` breaks it) rather than `flock`, no mmap or direct I/O, no compaction of a full page.
  Its crash test stops a process with a write torn, as a power cut would leave the file; the
  operating system's cache survives a process, so the test cannot observe a missing `fsync`.
  The distributed foundation is a library contract and two processes: no consensus, replication,
  membership or partition tolerance, no TLS; idempotency is the receiver's record of keys, which
  `@std/dist` leaves to it.
- The domain profiles (Gate 3) are compositions of rules, and their workloads run on emulators with
  simulated sensors: no certification, qualification or regulatory approval, no hardware, no CAN
  — the automotive frame is text over a UART.
- A board's tasks are run to completion, one at a time, by priority (Gate 3, `@task`): no task
  preempts another, a missed deadline is detected — a task released again before it ran, `N0414` —
  never prevented, and no task's execution time is known; verified on emulators only.

## Effects, authority and information flow

- Capabilities are coarse: an `IoCap` authorises every file alike, and the one narrower kind is
  `OutCap`, the standard streams. No per-file or per-directory kind exists, and revocation is
  outside the model by design (area 6). *Narrowed in N79.*
- An undeclared function in another module is assumed to have every effect, `{ io, spawn }`: its
  interface publishes no set. That is an effect assumption and grants nothing — since N104 no
  function inherits authority — but it makes such a call count against a caller's declared set
  (area 5). *Narrowed in N104:* the authority bridge is gone.
- The core's scope, as N107 audited it (`audit-n107.md`): effects are the compiler's three (`io`, `spawn`,
  `foreign`), with no handlers and no user-defined effects, and **pure is not total** — a pure function may diverge;
  capabilities are static, coarse and shareable — nothing checks them at run time, none is revoked,
  and none is linear; information flow is explicit data flow only, never non-interference. Each
  absent construct is refused by name.
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

- Nine targets, no others: five hosted — `aarch64`/`x86_64` × macOS/Linux, and `x86_64` Windows
  with MinGW-w64's toolchain (Gate 3) — and four freestanding, below, two of them 32-bit (wasm32
  and, since Gate 3, a Cortex-M3 board, with no floating point and no 64-bit device access); no
  32-bit hosted target and no CPU-feature selection; the Cortex-M board has no heap in 1.0 (the
  arena's runtime is a 64-bit board's); a target the host has no linker for is built to objects (`--objects`) for its own
  toolchain, with no bundled sysroot or linker; `x86_64-unknown-linux-gnu` is compile-only here,
  Windows is run-verified **under Wine** only — no Windows host has run it, no DLL is built, and
  the MSVC environment and Windows ARM64 are refused — and Cranelift builds no freestanding or
  WebAssembly target (area 33). 32-bit hosted and big-endian targets are non-goals, each with its
  reason in `nazm inspect`'s support matrix (N95). An LLVM object's CPU is the C compiler driver's
  default for its triple, recorded, not selectable.
  *Narrowed in N57, N95 and Gate 3:* until N57, the host only.
- A build generates its runtime unless given `--runtime DIR` (N82): the artifact is the way to build
  a runtime once and check what is linked, not yet the default. The runtime is LLVM text, not Rust,
  and has no allocator of its own (area 13).
- Both native backends translate one instruction-level LIR (N105): every failure, guard, offset,
  runtime call and retain or release is decided once, in `nazm-lir`. LIR's interpreter — the oracle
  they are held to — runs the sequential subset only: a program with tasks, channels, a foreign call
  or the clock is refused by it, and there the backends are held by their agreement and by the
  memory and concurrency suites. *Narrowed in N105:* until then each backend translated MIR's
  operations itself, their agreement rested on differential testing alone, and LLVM laid its own
  aggregates out (area 10).
- Two boards, both emulated and neither run on hardware: `aarch64-unknown-none` and (N86)
  `riscv64gc-unknown-none-elf`, each on its QEMU `virt` machine: objects and `link.ld`, linked by
  `ld.lld` outside this host; RISC-V is built only where `clang` has a RISC-V code generator (not
  Apple's — the `nazm-qemu:n86` image's), refused by name elsewhere. Since Gate 3D a board may be
  described by a manifest (`--board`), with sequentially consistent atomics and statics, sections,
  interrupts — a GICv2 and its timer on AArch64, an NVIC and SysTick on the Cortex-M3 — and none on RISC-V (refused by name), a bump
  arena that is never returned to, and a failure hook — every one verified under QEMU, none on
  hardware; there is no `@std` for a board yet, no weaker memory ordering, no nested interrupts
  and no interrupt priorities. `.bss` is zero because QEMU's loaded memory is, which a hardware
  loader need not do; a board program that reaches input and output, tasks, channels, number
  formatting or the C library is refused, and allocation without a `[heap]`; Cranelift builds no
  board (areas 18, 33).
  *Narrowed in N86:* until then, one board and 32-bit device registers only; *in N62:* no target
  without an operating system.
- Cranelift keeps every non-scalar local in a stack slot; its loops are slower than LLVM's
  (area 12, `performance.md`).
- Both backends still need clang for the runtime's LLVM text and the C compiler driver for the
  link; `nazm inspect` names both (areas 11, 12). The LLVM emitter promotes only scalar temporaries
  within one block; other locals keep a slot at `-O0` (N59).
- Debug information: functions, lines, lexical scopes and `Int`, `Bool` and `Str` variables under
  LLVM; functions and lines under Cranelift, which describes no variables; a live session verified
  with gdb in the Linux container, not under lldb on macOS, whose developer mode a non-interactive
  run cannot enable; no debug adapter (DAP). Sampling (`nazm profile --sample`) uses macOS's
  `sample` and is refused on a host without it; no Linux sampler, timeline or event trace (area 27).
  *Narrowed in N58 and N91.*

## FFI

- A practical C ABI subset (N85, area 17): `Int`, `Bool`, a `Str` argument borrowed and a `Str`
  result copied, opaque handles, C-layout structs by borrowed `const` pointer, `Option` results for
  null, `c_errno()`, exports as callbacks, static and shared libraries. Not crossing, each refused by
  name: floating point, integers other than `int64_t`, variadics, by-value structs and struct
  results, unions, arrays, bit-fields, closures as callbacks, out-buffers C writes into, `dlopen`,
  ABIs other than `"C"`. C is trusted entirely once called — a handle used after C freed it is C's
  undefined behaviour; `errno` is per thread, so a pool task that blocks between a call and
  `c_errno()` may read another task's; `nazm run --interpret` cannot call C (a compiled `nazm run`
can, since R1.5 D11, from the system's libraries); one Nazm library per C program,
  since each carries a runtime. Shared libraries are linked and run on macOS; Linux's link is the
  same `-shared` command, not run here. *Narrowed in N85:* until then, no handles, structs, string
  results, callbacks, `errno` or shared libraries; *in N55:* import only and no strings.

## Runtime and concurrency

- A native program's tasks run on a bounded pool of workers by default (N83, the default since
  N106), pinned, without work stealing or preemption — a task that computes without waiting holds its
  worker, and a task's stack is fixed (`NAZM_TASK_STACK`, 256 KiB). A program that calls C runs a
  thread per task unless `NAZM_SCHEDULER=pool`, because nothing hands a blocking foreign call off
  its worker. The interpreter and the compiler written in Nazm keep a thread per task. *Narrowed in
  N106:* until then one OS thread per task by default, and the pool verified on macOS only. No detached tasks; deadlock is possible and not detected, on either model
  (areas 14, 15). *Narrowed in N54:* select over typed channels, a select deadline under
  a `TimeCap`, and cancellation by closing a channel a task selects on exist; nothing is cancelled
  on a sibling's failure, and nothing selects over sends or over the `Int` channel. Where a task is
  a thread — the interpreter, the compiler written in Nazm, a program that calls C, or
  `NAZM_SCHEDULER=threads` — the tasks alive at once are bounded by the host's thread limit — a
  container's process ceiling counts every thread — and a `spawn` the host refuses is `N0404`, not a
  wait.
- A channel carries one element type, and only one that may cross into a task — `Int`, `Bool`,
  `Str`, channels, and records and enums of those (`N0390`); no unbounded channel, no channel of
  sequences or closures (area 14). *Narrowed in N53:* until then, `Int` only.
  *Narrowed in N54:* `select` exists for `Chan[T]` (lowest ready index; receives only).
- Files and handles (Gate 2): an `OsHandle` is closed by `os_close` or when the process ends,
  not when its last copy goes; reading a file or standard input holds the task's worker on the
  pool until the system answers (only the network will not, §10); listing one directory handle
  from two tasks at once is serialised by the handle table's lock. The native side is verified on
  macOS (aarch64) on the host and on Linux in the contained run; x86-64 macOS's `$INODE64` symbols
  are written but unverified. No memory mapping, symbolic-link creation or file times other than
  the modification time; `dir_list` reads a whole directory into memory. *Narrowed in R1d:* a
  path's own facts (`os_lstat`), permissions, canonical paths, temporary directories and advisory
  whole-file locks exist (`spec.md`, *The platform, further*); a lock waited for holds the task's
  worker; nothing removes a tree, since the names a directory lists are `file`-origin (`N0372`);
  the Windows forms are compiled and not executed, and on WASI permissions, canonical paths and
  locks answer `ENOSYS`.
- Networking (Gate 2): TCP and UDP over literal IPv4 and bracketed IPv6 addresses; no TLS, no
  Unix-domain sockets, no multicast or broadcast, no socket options beyond the deadline and
  address reuse, and no polling of many sockets from one task (a task per socket is the shape).
  Name resolution holds the worker while the resolver answers, and every resolver failure is
  `NotFound`. The interpreter notices a close of a socket a task is receiving datagrams on within
  20 ms rather than at once. The reactor is verified on macOS (kqueue) on the host and on Linux
  (epoll) in the contained run; x86-64 Linux's packed `epoll_event` is written but runs only where
  the contained run is x86-64.
- Process and environment (Gate 2): no signal handling — a program cannot watch for `SIGINT` or
  `SIGTERM` — and a native program that has started a child ignores `SIGPIPE` from then on.
  Waiting for a child's exit holds the task's worker. The environment is read, never set.
  *Narrowed in R1d:* a child may be given a directory, an environment (ordered by name), a new
  process group and a null standard input, waited for with a bound, and signalled with its group
  (`spec.md`, *The platform, further*); a bounded wait looks again every 10 ms at most rather than
  being woken. The interpreter cannot ask a child's peak memory without C and answers `ENOSYS`, and
  signals a group through the system's `kill`, so a refusal there is `EPERM` when `kill` says so and
  `ESRCH` otherwise. On Android (API level 30) a child cannot be given a directory (`ENOSYS`); on
  Windows a group is its leader alone. A child's standard output and errors are still pipes.
- Logging (Gate 2): `@std/log` writes records to standard error only; there is no `task_id()` or
  `runtime_stats()` built-in, and no trace context carried between tasks.
- The runtime is its own crate (`nazm-runtime`, N53) but still LLVM text both backends link,
  tested through a C harness; no Rust implementation, no runtime shared across programs as a
  library (area 13).

## Packages and builds

- A local registry and four requirement forms (N56); no remote registry or network source, no
  signatures, no features or pre-releases, one version per package, no multi-root workspaces
  (area 26). *Narrowed in N56:* until then, path dependencies only. *Narrowed in N90:* resolution
  backtracks, bounded at 10,000 candidates — past the bound it refuses saying it stopped, which is
  not a proof that no answer exists — and `nazm update` exists.
- Project tooling (N74): six templates, a library check, `nazm doc` from semantic definitions, a
  publish dry run; no hosted documentation, no template beyond the six (area 26).
  `nazm bindgen` translates the subset §7.86 lists and reads declarations, not the C preprocessor
  (area 17). The editor client (`editors/vscode`) is not exercised inside an editor here or
  published. An exported rename (N89) needs a declared workspace and is never of a library package's
  API, whose importers are other packages; the language server offers its edit only when every file
  it touches is open, and MCP neither plans it nor applies a plan (area 25). *Narrowed in N89:* until
  then, every exported rename was refused.
- Reproducibility is verified on one host. Across toolchains it is measured, not claimed: Apple
  clang 21 and clang 19 give the same program objects and a different runtime object (N73, area 32).
  A provenance record is evidence of what a build read and wrote, not a proof. `nazm attest` signs and
  verifies in-toto statements of it with OpenSSH keys the user holds — no transparency log, no keyless
  signing, no SLSA level claimed — and reproducibility is verified on one host and one image, not on
  independent machines (N94). The compiler's bill of materials is what `Cargo.lock` claims; nothing
  checks a crate against its upstream.
- Coverage-guided fuzzing covers five targets — the front end, lowering with MIR's validator, the
  interpreter, package manifests and LIR — at 300 s each per campaign; no sanitizer, Miri or Loom run,
  no backend, FFI or registry-archive fuzzing, and the differential generator writes `Int` and
  `Bool` programs only (area 30). A minimised corpus reaches 99.3 % or more of its campaign's edges.
  *Narrowed in N73 and N93.*
- The compiler written in Nazm builds all 21 parity probes as the reference does (N102,
  `compiler/parity.json`), which is parity on those probes and not on the language: it reads no
  package manifest and refuses a package import by name; it has no `Chan[T]` of any element type, no
  `select`, clock, device registers or `c_errno`; a foreign declaration takes and returns `Int` only;
  it checks no provenance, no profile and no information flow; it has no string escapes, `::` or
  `as`. It emits one LLVM module for a whole program (areas 16, 17, 26, 31). *Narrowed in N102.*
- Checking is slower than at N75: `nazm check compiler/emit.nz` 107 → 166 ms in the contained
  benchmark, the steps at N76 and N80; `channels` builds 13–31 % slower (area 28, `performance.md`).
- Of the 71 measured sections of `performance.md`, 8 current claims (N80, N82–N88) have no command
  that regenerates them, and 53 are dated records of earlier trees; `bench/claims.toml` names each
  (area 28). The claims gate checks that a reproducible entry's command exists, not that its numbers
  still hold. No energy measurement: no counter is readable here without privileges. No Zig
  reference (no Zig toolchain); the Go reference is measured only where Go is installed, which the
  contained runner is not. *Narrowed in N92.*

## Profiles

- Formal evidence is a bounded model check of a small core (`docs/formal-core.md`, N61): every
  program of integers, booleans, `if` and `let` up to five nodes, against the interpreter and the
  checker; and, since N97, every program of a stated bound of loops, mutation, early return and
  one recursive function (12,900 programs, fuel-bounded). No proof assistant is installed here, so
  nothing is proved; nothing beyond the bounds, and nothing about records, strings, ownership,
  effects, flows, tasks or the native backends' code (area 19).

- Contracts (N87) are `requires` and `ensures` and nothing else: no loop invariants, ranges or
  ghost state; the only proof is evaluating a call whose arguments are all literals, of a function of
  the same module; a clause is checked every time, never assumed, and costs its evaluation; the
  obligation census's `proved` entries are the whole of what is proved — every other overflow, index
  and division is checked as the program runs, which is the language's guarantee, not a proof of
  absence; the compiler written in Nazm checks a contract's shape and the clauses at run time, as the
  reference does, and proves nothing (area 30). Not certification (area 19).
- Twenty-two rules: Gate 3 added `no-heap` (the `kernel` and `realtime` profiles), `bounded-stack` (`realtime`), `no-float-nondeterminism` (`aerospace`), `watchdog` (`industrial`, a runtime watchdog on the scheduler's ticks, not a hardware one) and `audit` (`medical`, over a static atomic's changes, the one place a board program keeps state, and a call in the same function, not a proof the record precedes the change), and the domain profiles, compositions that certify nothing; N87 added `no-unknown-calls` (to `critical` and `cyber`), N80 `checked-paths`, N79 `explicit-authority` (the `authority` profile); N60 added `no-recursion`, `no-select` and `no-ambient-time` to N52's, N63
  `no-blocking` and `bounded-loops` (the `realtime` profile), N72 `signed-writes` (`accounts`); the
  stack rule bounds depth, not bytes, and is `unknown` through a function value; no constant-time
  rule, and a loop is bounded only in its counted form; `critical` stays a restriction set, not a
  certification; a profile costs one extra check, `embedded` a MIR lowering (+13 % on the compiler's
  source) (areas 18–20).

## Carried from Q1 (2026-10-08)

Each was found by Q1's qualification and is stated here so that no claim contradicts it; the
classification and evidence of each is in `docs/security-qualification.md`, and those marked
*fix before v1* must be closed or re-decided at the final release gate.

**Native code and runtime**

- *Fixed before v1 (Q1-A1-01):* a task frame larger than the stack left to it faulted on the task's
  guard page rather than failing with `N0408`. A function whose own frame is a page or more is now
  entered through a check that the whole frame fits, made before its prologue lays the frame out.
  A chain of smaller non-recursive frames deeper than the headroom is still unmeasured (*Native
  recursion*).
- *Fixed before v1 (Q1-A1-04):* an export called from a C thread on a small stack faulted instead of
  failing with `N0408`; the stack check now budgets from the calling thread's own stack.
- *Narrowed before v1 (Q1-A1-05):* a C pointer crosses as a 64-bit word, so on the 32-bit targets
  (`wasm32-unknown-unknown`, `wasm32-wasip1`, `thumbv7m-none-eabi`) a foreign function taking a
  handle or a callback, returning a handle or a C string, or taking a C-layout record holding a
  handle is refused by name (`N0381`) rather than imported with a signature C does not have.
  Crossing pointers at the target's width there is not built.
- *Fixed before v1 (Q1-B-05):* `exit_with` with a status outside 0..255 ended with a different status
  in the interpreter, a native build and the LIR oracle. The specification now defines the status as
  `0..=255`, reported unchanged; anything else fails with `N0416` everywhere (*Ending*).
- *Fixed before v1 (Q1-B-06):* a generic instantiated through many levels of nesting got a symbol
  name that grew exponentially, and sixteen levels exceeded the linker's limits. Past 512 bytes of
  spelled type arguments an instance is named by their BLAKE3 digest; building such a program still
  takes time and memory exponential in the nesting, since every instance is generated.
- *Fixed before v1 (Q1-M-12):* native select and sleep deadlines were on the wall clock, so a step
  of it ended a wait early or never; they are on the monotonic clock now, as the interpreter's and
  `time_now_ms`'s are, and a wait on a thread naps at most 100 ms of wall clock before looking again
  — the most a wall clock stepped back delays it. A blocking built-in (`read_file`, …) holds a pool
  worker while it blocks, as a C call does (Q1-M-13).
- A `nazm build` stopped by a signal leaves its scratch directory beside the output (Q1-FI-01).

**FFI**: `RESERVED_C_SYMBOLS` lists what the runtime imports, not the runtime's own definitions
(`nazm_main`, `nazm_init`, …) (Q1-A1-11); with the opt-in pool scheduler and C, `errno` is per
thread rather than per task and C runs on a task's 256 KiB stack (Q1-A1-12).

**Checker and caches**: *fixed before v1 (Q1-M-08):* a warm check named a parameter of a module the
cache reused by position (`#1`) where a cold check named it; a definition now carries its parameters'
names from its signature, and the two checks agree whole. Checking very wide records and many impls is quadratic, and a huge identifier gives
a diagnostic of that size, with no cap on their number (Q1-B-09, Q1-B-10).

**Packages**: a path dependency may name any directory, and a symlinked `root` or `main` is
followed (imports are still held inside the canonical source root) (Q1-A2-12); lockfile digests
are enforced under `--locked` only and are not signatures (Q1-A2-13); writes to a local registry's
index are not locked against a concurrent publish (Q1-A2-14).

**Self-hosted compiler**: the compiler written in Nazm does not carry Gate 2 — integer widths,
floats, arrays, constants, `Bytes`, `Text`, attributes and their built-ins are refused by it, as its table of
built-ins not carried records (`crates/nazm-cli/tests/selfhost.rs`, `NOT_CARRIED`); the bootstrap
holds the subset it does carry (Q1-D-20).

**Profiles**: `signed-writes` is syntactic — a condition on the caller that is always true,
`caller == caller || true`, satisfies it (Q1-C, informational).

**Command line**: the exit statuses were frozen in Q1-C1 (`docs/stability.md`): a usage error is
5, no longer 2, and user errors are 1 or 3, no longer 4 (Q1-D-06, Q1-D-07 — closed). `nazm fmt`
lays out a file that lexes but does not parse, as its documented guarantee allows (Q1-D-22).

**Fuzzing**: the `run` target counts an input still running after two seconds as a hang — a program
that blocks, which Nazm does not promise against. *Fixed before v1 (Q1-F-01):* each is now kept, as
it was, under `OUT/hangs/` for triage; until then it was counted and dropped. Q1's 36 are not
recovered: the next campaign keeps its own.

## Carried from M2-Core (2026-10-09) — closed in M2-C1

M2-Core's campaign left three catalogue entries without a verdict, each older than M2:
`n85-a-shared-library-is-an-archive` (its killer was macOS-only), `n86-clang-is-not-asked` (its
killer depended on whether the host's `clang` builds RISC-V) and
`an-inline-containment-cycle-is-not-refused` (its killer ran the checker in process, which on the
mutant never returns). M2-C1 gave each a killer that decides it in the Linux runner — the
artifact's header, a fake driver on the child's `PATH`, and `nazm check` as a bounded child — and
all three are caught at tier 1 with their killers verified (campaign `bf4128ecd290fbe4`). Nothing
from M2 is carried.
