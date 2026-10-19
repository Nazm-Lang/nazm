# Nazm 1.0 — the competitor barrier map

Gate 2, 2026-10-09 (`docs/internal/general-purpose.md` §25). For each language a team might choose instead:
why it is chosen, the problem underneath that choice, the Nazm mechanism aimed at that problem, the
1.0 evidence for it, and what remains missing. **No row claims Nazm is better at anything.** A
comparison of speed, size or effort would need a measurement filed in `docs/internal/performance-log.md`, and
none is filed against these languages. Where a row names a gap, that gap is real, and most of them
are ecosystems that take years, not mechanisms a gate adds.

Evidence names a document or a test; `capability-matrix.md` holds each area's status, and
`limitations.md` is the authoritative list of what 1.0 does not do.

**Gate 4 light (2026-10-11).** Revised after Gate 3, which changed the C, C#, Swift, Kotlin, Julia,
Ada/SPARK and Erlang rows, and two rows added: CUDA/Futhark/Chapel and Solidity/Web3. Each row's
evidence is one of two kinds, and says which: **documented capability** — a mechanism with tests that
show it works, which says nothing about how it compares — or **executed comparison** — a measurement
against that language's own implementation, filed in `performance.md` *Gate 4 light: a small
cross-language set*. Only the C, Rust, Go and Python rows have an executed comparison, and it is a few
small programs on one host: see that section for what it does and does not show. Every other row is
documented capability only, and **unmeasured** against its language.

## C

- **Why chosen.** Every platform has a C compiler and every other language binds to C; control
  of memory and layout; small runtime.
- **The problem underneath.** A portable, predictable native artifact that any other language
  can call.
- **Nazm's mechanism.** Native code through LLVM and Cranelift; fixed-width integers, floats and
  arrays of plain data (Gate 2); `extern "C"` both ways — calls out, exports in, opaque handles,
  C structs by pointer, strings, `errno`, callbacks; static and shared libraries with a generated
  header (N42, N55, N85, Gate 2 §23); two freestanding boards (N62, N86).
- **1.0 evidence.** Documented capability: `c_foundation.rs` (every number across, C and Python,
  both backends); `ffi.rs`; Gate 3's systems foundation — atomics, statics, `@section`, board
  manifests, interrupts, a heap arena (`atomics.rs`, `statics.rs`, `sections.rs`, `interrupts.rs`,
  `heap.rs`); capability matrix areas 11, 12, 17, 18. Executed comparison: the sieve against `clang
  -O2` (`performance.md`, Gate 4 light).
- **Remaining gap.** No by-value structs, variadics or `dlopen`; C pointers do not cross on 32-bit
  targets (`N0381`); no allocator control beyond a board's arena; no inline assembly; three boards,
  emulated, against C's every chip; the runtime is generated text, not a separately audited library.

## C++

- **Why chosen.** Zero-cost abstraction over C's model, templates, a vast library and toolchain
  base; game engines, browsers, finance.
- **The problem underneath.** Abstraction without paying at run time, on existing native code.
- **Nazm's mechanism.** Generics with trait bounds, statically dispatched (N78); a C ABI to reach
  C-compatible libraries; reclamation decided by the compiler's ownership graph rather than by hand.
- **1.0 evidence.** `spec.md`, *Traits and methods*, *The memory constitution*; matrix areas 3, 4.
- **Remaining gap.** No C++ interop beyond `extern "C"`; no trait objects, specialisation or
  compile-time metaprogramming beyond constants; no library ecosystem of C++'s size.

## Rust

- **Why chosen.** Memory safety without a garbage collector; fearless concurrency; Cargo.
- **The problem underneath.** Native performance without the memory and data-race defects of C.
- **Nazm's mechanism.** No unsafe surface in the language; a sequence cannot cross into a task
  (no shared mutable storage), channels and structured scopes (N53–N54); capabilities as values
  for authority (N37, N104); effects checked (N36, N79); packages with a lock file (N46, N56).
- **1.0 evidence.** Documented capability: `spec.md`, *Structured concurrency*, *Capabilities*;
  `concurrent.rs`; matrix areas 5, 6, 14. Executed comparison: the sieve against `rustc -O`
  (`performance.md`, Gate 4 light) — one program's speed, not a safety comparison.
- **Remaining gap.** No borrow-checked references or lifetimes, so no zero-copy sharing of mutable
  data across tasks; no `unsafe` escape for systems code beyond C; crates.io's ecosystem; no
  async I/O model of Rust's breadth (a task parks on the pool instead, Gate 2 §10).

## Zig

- **Why chosen.** C replacement with comptime, explicit allocation, superb cross-compilation.
- **The problem underneath.** Simple, explicit native code that builds for any target from one host.
- **Nazm's mechanism.** `--target` with object output for another toolchain (N57); freestanding
  targets (N62); compile-time constants (Gate 2 §15) and `nazm comptime` (N65); `@cfg` per target
  (Gate 2 §17).
- **1.0 evidence.** Documented capability: `cross.rs`, `attributes.rs`; matrix areas 13, 18.
  Unmeasured: no Zig toolchain is installed here.
- **Remaining gap.** No general comptime execution in types; no allocator parameters; cross
  linking needs the target's own toolchain; Zig's C-header import is `nazm bindgen`'s narrower
  subset (N85).

## Go

- **Why chosen.** Simple language, fast builds, goroutines, a strong standard library for servers.
- **The problem underneath.** Writing network services that many people can maintain.
- **Nazm's mechanism.** M:N tasks on a bounded pool (N83, N106); channels and `select` with
  deadlines (N54); TCP/UDP with a task parking on the pool for I/O (Gate 2 §9–§10); JSON, files,
  processes, logging in `@std` (Gate 2).
- **1.0 evidence.** Documented capability: `net.rs` (a one-worker pool serving many connections),
  `files.rs`, `process.rs`; the `kvd` reference application (§24). Executed comparison: the sieve
  and a channel hand-off against Go (`performance.md`, Gate 4 light) — Nazm's hand-off measured about
  21 times slower; no server throughput measured.
- **Remaining gap.** No HTTP, TLS or DNS in `@std`; no preemption or work stealing; no
  profiler-integrated tracing; Go's module ecosystem.

## Java

- **Why chosen.** A managed runtime that runs everywhere, a huge ecosystem, enterprise tooling.
- **The problem underneath.** Large teams building long-lived services safely.
- **Nazm's mechanism.** Memory safety for checked Nazm code outside `extern "C"` calls, as
  `spec.md`'s *The memory constitution* states it and the differential corpus holds it; checked
  effects and capabilities as architecture rules in the source; restriction profiles (N47, N87); stable diagnostics and
  machine-readable tooling (N31, N74).
- **1.0 evidence.** `spec.md`, *Effects*, *Capabilities*, *Restriction profiles*; matrix areas 19,
  20, 22.
- **Remaining gap.** No JIT, garbage-collected object graphs, reflection or dynamic loading; no
  JVM interop; the enterprise library and framework ecosystem.

## C#

- **Why chosen.** Productive managed language, .NET libraries, Windows and game (Unity) platforms.
- **The problem underneath.** Productivity with a rich, consistent platform library.
- **Nazm's mechanism.** A typed standard library of twenty-nine modules; an LSP and machine
  fixes (N74); a C header any P/Invoke binding reads (Gate 2 §23).
- **1.0 evidence.** Documented capability: `library/std/API-1.0`; `c_foundation.rs`; matrix areas
  16, 24, 25; `x86_64-pc-windows-gnu` run-verified **under Wine** only (Gate 3C). Unmeasured.
- **Remaining gap.** No Windows host run, no MSVC toolchain or Windows ARM64; no LINQ-like query
  syntax, async/await, exceptions or generics with variance; .NET's platform library.

## Swift

- **Why chosen.** Apple platforms, value types, safety with native speed.
- **The problem underneath.** Safe native applications on Apple devices.
- **Nazm's mechanism.** Native builds for aarch64 and x86-64 macOS; records as values; a C ABI
  Swift imports through a module map; `aarch64-apple-ios` libraries and programs (Gate 3K); a
  Cocoa window calling a Nazm library (`examples/desktop/counter`).
- **1.0 evidence.** Documented capability: matrix areas 11, 12, 17 (macOS hosts); `mobile.rs` —
  iOS is **compile-only**, linked against the SDK and never run (no simulator runtime here).
  Unmeasured.
- **Remaining gap.** Nothing run on iOS; no UI framework binding or Objective-C interop; no
  reference counting of user-visible classes; Apple's frameworks.

## Kotlin

- **Why chosen.** JVM interop with a modern language; Android.
- **The problem underneath.** Modern language features on an existing platform's libraries.
- **Nazm's mechanism.** A shared library and header a JNI wrapper can bind (Gate 2 §23); an
  `aarch64-linux-android` target with Bionic (Gate 3K).
- **1.0 evidence.** Documented capability: `c_foundation.rs`; `mobile.rs` — Android programs
  **run on an aarch64 Linux kernel** as static Bionic executables, never on an Android device or
  emulator. Unmeasured.
- **Remaining gap.** No JVM target or JNI binding generated; nothing run on Android itself; no
  coroutine interop; Kotlin's platform position is not something a language mechanism provides.

## Python

- **Why chosen.** Readability, a vast library ecosystem (data, ML, scripting), instant iteration.
- **The problem underneath.** Getting a result quickly, with libraries for everything.
- **Nazm's mechanism.** An interpreter for immediate runs (`nazm run`), a REPL (N65), and the same
  program compiled natively; a shared library Python loads with `ctypes` (Gate 2 §23); seeded
  randomness and floats (Gate 2).
- **1.0 evidence.** Documented capability: `c_foundation.rs` (Python `ctypes` calls a Nazm
  library); `differential/` (interpreter and native agree). Executed comparison: the sieve and the
  channel hand-off against CPython (`performance.md`, Gate 4 light) — a runtime contrast, not a
  productivity one.
- **Remaining gap.** No NumPy-class array library, no notebooks, no dynamic typing; Python's
  ecosystem is the reason it is chosen, and it is not replaced.

## Julia

- **Why chosen.** Numeric performance with a high-level syntax; multiple dispatch.
- **The problem underneath.** Fast numeric code written by scientists.
- **Nazm's mechanism.** `Float32`/`Float64`, fixed arrays, vectorised sums (N66), accelerator
  kernels for `Int`, `Float32` and `Float64` on a GPU and a CPU provider (N67, N88, Gate 3H);
  `@std/linalg`'s `Vec3` and `Mat3`; the `nbody` reference application (§24).
- **1.0 evidence.** Documented capability: `numbers.rs`, `arrays.rs`, `accel_providers.rs`; matrix
  area 21 (PARTIAL). Unmeasured: no Julia is installed here.
- **Remaining gap.** No multidimensional arrays or general linear algebra beyond 3×3, no SIMD
  types, no multiple dispatch; Julia's scientific packages.

## Ada/SPARK

- **Why chosen.** High-integrity systems: strong typing, contracts, formal proof.
- **The problem underneath.** Software whose failure is unacceptable, with evidence for it.
- **Nazm's mechanism.** Contracts (`requires`/`ensures`, N87), the `critical` profile and an
  obligation census, checked integer boundaries that trap rather than wrap, provenance and
  information-flow checks (N38, N80).
- **1.0 evidence.** Documented capability: `spec.md`, *Function contracts*, *Provenance*, *Domain
  profiles*; matrix areas 7, 19; the `realtime`, `assurance` and `aerospace` profiles and their
  workloads under QEMU (Gate 3F–3G, `domains.rs`); requirements traceability (`nazm trace`, Gate
  3L). Unmeasured.
- **Remaining gap.** No proof of contracts — they are checked at run time and counted, not
  discharged; no certification evidence (DO-178C and kin); emulators and simulated sensors only;
  Ada's full tasking model (Nazm's board scheduler is fixed-priority run-to-completion).

## Erlang/Elixir

- **Why chosen.** Fault-tolerant concurrent systems: lightweight processes, supervision, hot
  reload.
- **The problem underneath.** Services that keep running when parts fail.
- **Nazm's mechanism.** Structured concurrency — a scope does not end before its tasks, and a
  failure cancels its siblings (N53–N54); channels; `reload-check` for a compatible reload (N65).
- **1.0 evidence.** Documented capability: `concurrent.rs`, `select.rs`; `@std/dist` and the
  two-process `ledger`, whose client's retry is applied once across a server restarted mid-request
  (Gate 3I). Unmeasured.
- **Remaining gap.** No supervision trees, process isolation of failures or hot code loading; the
  distributed foundation is message framing and a retry, not a runtime; a trap ends the program.

## OCaml/Haskell

- **Why chosen.** Expressive static types, algebraic data types, pattern matching, purity.
- **The problem underneath.** Encoding invariants in types so wrong programs are refused.
- **Nazm's mechanism.** Closed enums with exhaustive matching, generics, inferred and checked
  effects with a pure default, function values and closures (N50–N51).
- **1.0 evidence.** `spec.md`, *Effects*, *Function values*; matrix areas 3, 5.
- **Remaining gap.** No higher-kinded types, type classes beyond nominal traits, effect handlers,
  laziness or type inference across function signatures.

## TypeScript

- **Why chosen.** Types over JavaScript; the web and Node ecosystems; editor tooling.
- **The problem underneath.** Safer code where JavaScript already runs.
- **Nazm's mechanism.** A WebAssembly target whose outside is its imports (N64, N71); an LSP and
  machine-readable diagnostics (N74); JSON in `@std` (Gate 2 §14).
- **1.0 evidence.** `wasm.rs`; matrix area 25.
- **Remaining gap.** No JavaScript interop beyond WebAssembly imports, no DOM binding and no npm
  packaging; structural typing and union types are absent.

## CUDA / Futhark / Chapel

- **Why chosen.** Data-parallel throughput on GPUs (CUDA), a functional array language that
  compiles to them (Futhark), parallelism at cluster scale (Chapel).
- **The problem underneath.** Using parallel hardware without rewriting the program for it.
- **Nazm's mechanism.** Accelerator kernels — maps, zips and reductions — written in Nazm, held to
  the interpreter bit for bit, run behind a provider boundary on Apple's GPU through OpenCL or on a
  CPU thread pool (N67, N88, Gate 3H).
- **1.0 evidence.** Documented capability: `accel_providers.rs`, `performance.md` (SIMD measured,
  memory-bound, no speed-up claimed). Unmeasured against any of the three.
- **Remaining gap.** No CUDA, ROCm, Metal or SPIR-V provider; no NVIDIA GPU here; no
  multidimensional arrays, scans beyond reductions, or distribution across machines.

## Solidity / Web3

- **Why chosen.** The EVM's language, with its tooling and audited libraries.
- **The problem underneath.** Contracts whose behaviour and cost are known before deployment.
- **Nazm's mechanism.** A chain-neutral contract model and simulator (N69); an EVM backend run in
  py-evm (N70) with gas bounds, through counted loops since Gate 3L; a WebAssembly adapter (N71);
  signed writes and account metadata (N72).
- **1.0 evidence.** Documented capability: `evm.rs`, both VMs held to the simulator on generated
  transaction sequences (N96); `systems-domains.md` Part R. Unmeasured against Solidity.
- **Remaining gap.** No chain deployment; sBPF **blocked** (no toolchain); no audited contract
  library; Solidity's ecosystem.

## Across the map

Nazm 1.0's mechanisms are aimed at problems these languages are chosen for; what none of them can
supply is an ecosystem. The gaps above are recorded so a team choosing Nazm knows what it gives
up. Any claim that Nazm is faster, smaller or cheaper than one of these languages needs a measured,
filed entry in `docs/internal/performance-log.md` first. Gate 4 light files one small cross-language set; it
supports no language-wide claim, and a result inside the bench's own 25 % regression threshold says
nothing about competitiveness.
