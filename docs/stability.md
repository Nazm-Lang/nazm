# Versions and stability — the Nazm 1.x compatibility constitution

> **Status in 0.9.0.** The toolchain is a **public preview** and the language a **1.0 draft**
> (`openspec/changes/release-0-9-preview`). This document is the contract **1.0 will make**; until
> then it is the target the preview is held to, not a promise in force: a 0.x release may still
> change what it says, and says so in its release notes.

What a Nazm release number means, which other identities exist beside it, and what Nazm 1.x
promises a person writing code against it. Settled in R1 (2026-10-07) for the pre-1.0 line and
replaced in Gate 1 of the v1 programme (2026-10-07) by the 1.x constitution below. The pre-1.0
policy is kept, as history, at the end.

## The promise

> **A program valid under stable Nazm 1.0, using only stable public surfaces, does not silently
> change meaning under a later 1.x toolchain.**

*Valid* means `nazm check` accepts it. *Stable public surfaces* are the ones this document
classifies **STABLE 1.x**; anything classified experimental or internal is outside the promise.
*Meaning* is what `docs/spec.md` says the program does: the values it computes, what it prints,
where it traps and with which code, which authority it needs and which effects it performs.
*Silently* is the operative word: the only permitted ways for such a program's meaning to change
within 1.x are the ones under *What a 1.x release may change*, and each is announced.

What the promise does **not** say: that performance, diagnostic prose, compiler-private layouts,
cache formats or the runtime ABI stay the same; that the ecosystem is complete; or that the
implementation is free of defects. Nazm 1.0 is a stable language and toolchain contract, not a
certification, a proof or a bug-free claim.

## The release lines

| Release | What it may contain | What it may not |
| --- | --- | --- |
| **1.0.x** (patch) | defect, security and tooling fixes; new diagnostics on programs that were already refused; documentation | an intended change to any stable semantics; a new stable surface |
| **1.x.0** (minor) | backward-compatible additions: new syntax that was refused before, new built-ins, standard-library items, commands, options, schema fields, diagnostic codes, targets; promotion of an experimental surface to stable; deprecations | removing or changing the meaning of anything stable: every program valid under 1.0 on stable surfaces keeps its meaning |
| **2.0.0** (major) | intentional incompatible changes to stable contracts, each written into `docs/spec.md` first and listed in the release record | a silent change: even in 2.0 a program whose meaning changes is refused with a diagnostic rather than run with the new meaning, wherever the compiler can tell |

There are no editions. Should one ever be needed, it is how a program would opt into a changed
meaning inside one major line; until then, an incompatible change waits for 2.0.

### What a 1.x release may change, and how it is announced

1. **A defect against the specification.** Where the toolchain and a STABLE section of
   `docs/spec.md` disagree, the specification governs and the toolchain is fixed. If the fix
   refuses a program 1.0 accepted, the program is refused with a diagnostic naming the rule —
   never run with a different result — and the release record lists the defect, the rule and
   the diagnostic. If the fix changes what an accepted program does, that program was relying on
   behaviour the specification contradicts; the release record says so with an example.
2. **A soundness or security defect.** The same, with `SECURITY.md`'s process; a fix may
   refuse programs that were accepted only because of the defect.
3. **Experimental and internal surfaces.** These may change or be removed in a minor release,
   with the change listed in its release record. The schema law below still holds for them: a
   versioned output never changes shape without a new major of its schema.
4. **Limits that are the implementation's.** Iteration and depth limits of `nazm run`, nesting
   bounds, stack sizes and resource ceilings may be raised in any release and lowered only in
   2.0 (`docs/spec.md`, *The iteration and depth limits*).

`tests/compat/v1/` is the part of this promise a machine checks: every 1.x toolchain runs it, it
is append-only, and an edited expectation is one of the four cases above or a 2.0 change.

## Identities, before and after 1.0

A release carries several version identities. They answer different questions and move
independently: a toolchain release does **not** imply a new semantic epoch, interface schema or
runtime ABI, and none of those implies a new toolchain release. Gate 1 moved the two that name the
release; Gate 1-C1 then moved the semantic epoch, because the checker began refusing programs it
had accepted. Gate 2 moved the semantic epoch again and the runtime ABI, adding the language's
numbers before the first 1.0.0 release; its later slices move what they change and say so here.

| Identity | Before (R1, `996984f`) | Now (0.9.0 preview) | Why it moved, or did not | Where it is reported |
| --- | --- | --- | --- | --- |
| **Language version** | v0.3 | **1.0 draft** | the settled semantics are the draft of the 1.x contract, frozen at 1.0 | `docs/spec.md`'s status line; `nazm --help` |
| **Toolchain version** | 0.3.0 | **0.9.0** | the public preview (the tree said 1.0.0 before it; never tagged) | `nazm --version`; `toolchain.compiler` in `nazm inspect`; the release manifest |
| **Semantic epoch** | 29 | **63** | Gate 1-C1 (30): a closure capturing a value that can hold a function value behind a counted handle moved from accepted to refused (`N0616`), and `Type::ALL`, in the checker's identity, gained five capability types. Gate 2 (31): numbers — integer widths, floats, bit operators and conversions — moved from refused to accepted, and literals take the type expected. Gate 2 (32): fixed arrays of plain data and module constants moved from refused to accepted. Gate 2 (33): `Bytes`, `Text` and their built-ins moved from refused to accepted, a string literal is a `Text` where one is expected, and the two type names and the new built-ins' names became the language's. M1 (34): a direct or UFCS call of a method whose impl is in another module is checked against its trait method's effects, where it had been refused as an undeclared function's. Gate 2 (35): an impl for a numeric type other than `Int` moved from refused (`N0604`) to accepted. Gate 2 (36): `OsHandle` and the `os_` built-ins moved from refused to accepted, and their names became the language's. Gate 2 (37): `NetCap` and the network built-ins moved from refused to accepted, and using an open handle stopped needing an `IoCap`. Gate 2 (38): the nanosecond clocks and the sleep moved from refused to accepted. Gate 2 (39): `ProcessCap`, the environment and child processes moved from refused to accepted. Gate 2 (40): `RandomCap` and `os_random_bytes` moved from refused to accepted. Gate 2 (41): an export of any number moved from refused (`N0391`) to accepted. Gate 2 (42): attributes before a declaration moved from refused to accepted (`N0619` for one outside the vocabulary), and a declaration whose `@cfg` does not hold is not there. G2-C1 (43): `@deprecated` before anything but a `pub` function, `struct` or `enum` moved from accepted to refused (`N0619`). (Gate 1 alone moved nothing: `1.5`'s refusal changed code, `N0100` → `N0101`, not acceptance) Gate 3 (44): the platform layer's functions (`nz_os_*`, `nz_net_*`) became reserved C symbols, so a foreign declaration of one moved from accepted to refused (`N0383`). Gate 3 (45): `Atomic` and the `atomic_` built-ins moved from refused to accepted, and their names became the language's. Gate 3 (46): `static` items moved from refused to accepted, and assigning one is refused (`N0625`). Gate 3 (47): `@section` before a function or a static moved from refused (`N0619`) to accepted, and `mmio_read64` and `mmio_write64` became the language's. Gate 3 (48): `@interrupt` before a function moved from refused (`N0619`) to accepted, held to a handler's shape (`N0627`). Gate 3 (49): `@on_failure` likewise, held to a hook's shape and one hook (`N0627`). Gate 3 (50): `@task` likewise, held to a task's shape (`N0627`). Gate 3 (51): the `watchdog_feed` built-in. R1a (52): `for x in e` and `for x in a..b` moved from refused (`N0101`) to accepted, and `..` became a token (`N0628` for iterating what is not a sequence). R1a (53): `Box[T]` and `List[T]` — a definition reaching itself through one moved from refused (`N0359`) to accepted, and the two types and six built-ins became prelude names, which a module's own declaration of the name hides. R1a (54): patterns — `_`, names, literals, or-patterns and nesting, a `match` of any type a pattern fits, `if let` and `let … else` (`N0629`, `N0630`). R1a (55): tuples — `(A, B)` types, `(a, b)` values, `t.0`, tuple patterns, `let P = e;` and `for (a, b) in e`. R1a (56): ordering and hashing derived — `<` on `Bool`, `Str`, records, enums and tuples by the names of their fields and variants, `==` on `Box` and `List`, `@std/hash`'s `Hash` without an impl — and a tuple of more than eight elements refused (`N0101`). R1a (57): arrays of strings, constants of record, enum, tuple and array types, `pub const` and `m::NAME` moved from refused to accepted. R1a (58): multi-line (`"""`) and raw (`r"…"`, `r#"…"#`) string literals moved from refused to accepted (`N0005`). R1a (59): interpolation (`f"…"`) and `float_to_str_fixed`, `str_pad_start` and `str_pad_end` moved from refused to accepted, the three as prelude names a module's own of the name hides (`N0006`). R1b (60): `Cell[T]` and `cell_new`, `cell_get` and `cell_set` moved from refused to accepted, as prelude names. R1b (61): a closure, and a named function's value, inside a generic function moved from refused (`N0387`) to accepted, and capturing a `let mut` binding moved from refused (`N0386`) to accepted, shared through a cell, unless the binding can hold a function value. R1b (62): a trait method's default body moved from refused (a syntax error) to accepted, and an impl that leaves such a method out moved from refused (`N0602`) to accepted. R1b (63): an impl with type parameters for a generic record or enum, `impl[T: Show] Show for Pair[T]`, moved from refused (`N0100`) to accepted. | `toolchain.semantic_epoch` in `nazm inspect`; build provenance |
| **Interface schema** | `nazm.interface/11` | **`nazm.interface/12`** | R1a (L8): a module's interface carries its exported constants and their values, which an `/11` reader would ignore | `toolchain.interface` in `nazm inspect` |
| **Runtime ABI** | 14 | **30** | Gate 2 (15): floats and narrow integers cross runtime entries, and the numbers part (`nz.format_float`, `nz.format_u64`, `nz.float_parses`, `nz.float_parse`) and the C library's mathematics join the runtime. Gate 2 (16): files and handles — the `nz.os_*` entries, the handle table, and the per-target `os` unit a build reaching them links. Gate 2 (17): networking — the socket entries, a 64-byte handle slot with a wait record and a deadline, the reactor thread, and the `os` unit's `nz_net_*`. Gate 2 (18): `nz.time_now_ns`, `nz.time_wall_ns` and `nz.time_sleep_ms`. Gate 2 (19): the environment entries, `nz.os_spawn` and the child's, pipes as handles, and the `os` unit's `nz_os_environ`. Gate 2 (20): `nz.os_random_bytes`. Gate 3 (21): the platform layer — every system call through a per-target function of the `os`, `stack` and (Windows) `init` units, the clocks among them, and the entry's argument hook. Gate 3 (22): the atomic entries `nz.atomic_load`, `nz.atomic_store`, `nz.atomic_swap` and `nz.atomic_cas`. Gate 3 (23): `nz.mmio_read64` and `nz.mmio_write64`. Gate 3 (24): a board's `nz.report` calls the entry's `nz_board_on_failure`, weakly, before stopping the machine. Gate 3 (25): a board's entry calls `nz.uart_init`, and a Cortex-M board's runtime defines 64-bit atomics and division. Gate 3 (26): `nz.watchdog_feed`, defined by a board's entry whose manifest has a `[watchdog]`; and a 64-bit board's runtime defines the C library's memory functions with or without a heap. v1 stack fixes (27): `nz.stack_check_frame`, which a function whose own frame is large is entered through, and a stack unit whose `nz_os_stack_size` answers the calling thread's own stack (Q1-A1-01, Q1-A1-04). v1 monotonic deadlines (28): `nz.ctimedwait` takes a monotonic time and `nz.timed_nap` turns it into a capped wall-clock nap (Q1-M-12). R1a (29): the release queue — `nz.vec_release` and an element's release take the queue of the release in progress, so releasing a value takes stack bounded by its type, not its depth. R1a (30): `nz.format_fixed`, C's `%.*f`, for `float_to_str_fixed` and an interpolation's precision | `toolchain.runtime_abi` in `nazm inspect`; `nazm.runtime/1` manifests |
| **Package and lock schemas** | `nazm.package/1`, `nazm.lock/1`, `nazm.registry-index/1` | **`nazm.lock/2` added** | Gate 2: `nazm.package/1` gained `[features]` and a dependency's `features`, optional keys a manifest without them never meets; a lockfile with an enabled feature is `nazm.lock/2`, and `/1` is still written without one and still read | the `schema` field of each file |
| **Standard library API** | 1.0 | **1.1** | additions only: Gate 2 added `@std/hash`, `@std/hashmap`, `@std/hashset` and `@std/deque`; every 1.0 line is unchanged | `library/std/API-1.0` |
| **Machine schemas** | each `name/major` | **unchanged** | none changed shape | the `schema` field of each output |

From 1.0 the language version is the toolchain's major and minor numbers: toolchain 1.x.y
implements language 1.x. Before it, a 0.x toolchain implements the 1.0 draft. The public tag for a
release is the toolchain version with a `v`: `v0.9.0` for the preview.

## What is stable

Each row is STABLE 1.x: covered by the promise. Where a row names a section, that section is the
definition and this is a summary.

### The language

`docs/spec.md` classifies every one of its sections (*Stability classification for 1.0*). The
STABLE 1.0 surface is:

- **Values and operators.** `Int` (64-bit signed, overflow traps in every build), `Bool`, `Str`
  (bytes, with escapes), `Ints`, `Strs`; arithmetic, comparison, logical operators; division and
  remainder by zero trap; the four integer boundaries; left-to-right evaluation.
- **Numbers, arrays and constants (Gate 2).** The fixed-width integers, `Float32` and `Float64`,
  bit operators and conversions (`spec.md`, *Numbers*); `[T; N]` of plain data with checked
  indexing (*Arrays*); module-private `const` (*Constants*).
- **Bytes and text (Gate 2).** `Bytes`, a counted handle to a view of a fixed-length byte buffer,
  its views and reads and writes by width and order, and `Text`, UTF-8 by construction, with their
  built-ins (`spec.md`, *Bytes*, *Text*); listed here since v1 (Q1-D-18).
- **Bindings and control.** `let` and `let mut`, same-scope shadowing refused, `while`, `break`,
  `continue`, `return`, `if`/`else` as a statement and an expression, blocks whose completion is
  decided by checking, unreachable code refused.
- **Types.** Records and enums (nominal, transparent, construction naming every field, `match`
  exhaustive and the only elimination form, recursion through inline containment refused);
  first-order generics with `Vec[T]`; derived equality and the `Equality` requirement; `Result`,
  `Option` and `?` (on `Result` only); function values and closures (capture by copy; a `let mut`
  or a capability is not captured); traits, `impl Trait for Type` and methods (nominal and
  static: no trait objects).
- **Effects, capabilities and provenance** — below.
- **Structured concurrency** — below.
- **Modules and packages.** A file is a module; `pub` exports; `use "PATH"`, `use "PATH" as
  NAME` and `NAME::item`; `pub use` re-exports; packages with `nazm.toml`, `use "NAME:path"` and
  `nazm.lock`.
- **The C boundary** — below.

**Explicitly not in 1.0, and refused by name rather than half-present:** `loop`, loop
labels, valueless `return`, mutable parameters, trait
objects and dynamic dispatch, user-defined effects and effect handlers, more than one effect
parameter, macros, generic function values. Adding one is a 1.x minor addition (it was refused,
so no valid program changes meaning); none is promised. *`for` was on this list until R1a added it
(`spec.md`, *`for` loops*; a `for` of any other shape is still refused by name). Floating point and integer types other
than `Int` were on this list until Gate 2 added them, before the first 1.0.0 release (`spec.md`,
*Numbers*); the compatibility corpus keeps the refusal case that pinned the old answer, listed as
superseded.*

### The memory contract

Frozen as `docs/spec.md`, *The memory constitution*, states it, and observable only through it:

- **Four kinds of value.** Immediates (`Int`, `Bool`, the other numbers and arrays of plain data)
  copy; `Str` and `Text` are shared immutable values whose sharing is unobservable; `Ints`, `Strs`,
  `Vec[T]` and `Bytes` are owned mutable handles — binding, passing and returning share the
  storage, and every view of a `Bytes` buffer shares it; `Chan` and `Chan[T]` are runtime-managed
  handles. A record or enum is a value composed of these, copied field by field, so a handle
  inside one is shared by every copy.
- **Reclamation is deterministic.** Storage is reclaimed when the last reference to it dies, and
  never before; an expression abandoned by a trap, `return`, `break` or `continue` releases what
  it had produced. There are no destructors and no user-observable destruction order.
- **Closures** capture by copy when they are created: a value is copied, a handle is the same
  storage. A function value is a handle to its closure, and its environment is a node of the
  ownership graph that never changes after it is made.
- **Tasks.** `Int`, `Bool`, `Str` and channels cross into a task, and a record or enum crosses
  when every field of it (of every variant) does — derived, not declared; a sequence, a `Vec`, a
  function value and an opaque C handle do not (`N0321`, `N0390`).
- **Cycles.** Inline containment cycles (`N0336`) and ownership cycles through `Vec` or a
  channel in type definitions (`N0359`) are refused, and so, since Gate 1-C1, is a closure
  capturing a value that can hold a function value behind a counted handle (`N0616`) — the one
  way a closure environment could come to own itself. With the three, every value of every type
  a program can write is reclaimed by counting, environments included; no collector exists or is
  needed (`docs/spec.md`, *Cycles*). Gate 1 found the closure route open and Gate 1-C1 closed it.
- **Not frozen.** Heap layouts, reference-count placement, string sharing versus copying, the
  `--layout` choice, and what an optimisation removes. These are the compiler's.

### Effects, capabilities and provenance

- **Effects.** Exactly three, compiler-owned: `io`, `spawn`, `foreign`. A function's effects are
  inferred (least fixed point through calls); a declared set `! { … }` is a contract, and `! {}`
  is pure. **Pure is not total**: a pure function may diverge, trap or block on a channel. One
  effect parameter per function, bound from arguments; a function value with fewer effects is
  accepted where more are expected, and inside another type nothing is converted. An undeclared
  function is seen by other modules as performing every effect.
- **Capabilities.** Authority is a value: `IoCap`, `SpawnCap`, `ForeignCap`, `VouchCap`,
  `TimeCap`, `MmioCap`, `OutCap`. Only `main` is handed capabilities, as its parameters; none can
  be constructed (`N0370`). Holding one is being able to name a binding of it in scope; nothing is
  inherited (N104). `IoCap` attenuates to `OutCap` by passing, and nothing converts back.
  Capabilities are static (no revocation), coarse (a kind, not a path or a host) and copyable
  like any value; a closure may capture one (N51), and its body then holds that authority.
- **Provenance.** Explicit flows only: a value's origins (`argument`, `file`, `authority`,
  `unknown`) flow through computation, containers, calls through values and function summaries,
  across modules. One flow is refused by the language: a `write_file` path from a file's contents
  or from C (`N0372`). `str_vouch` with a `VouchCap` declassifies, and every vouch is recorded.
  **No implicit-flow or non-interference claim**: a value chosen by branching on a secret carries
  no origin from it.

### Structured concurrency

`scope { … }` starts tasks with `spawn f(args)` and its closing brace waits for every one of
them, including when the body itself failed; a failure leaves through the scope after that wait,
and a sibling's failure cancels nothing (spec: *Structured concurrency*). `Chan`
and `Chan[T]` are bounded, blocking, closable queues; `chan_select_of` returns the lowest ready
index and `chan_select_until` adds a deadline with a `TimeCap`; cancellation is cooperative, by
closing a channel a task waits on. **Not promised:** deadlock freedom, deterministic
interleaving, starvation freedom, fairness, or anything distributed. The M:N scheduler a native
build uses is an implementation choice and not part of the contract; nothing a program can
observe through the stable surface depends on it, other than timing.

### The C boundary

Stable: `extern "C" fn name(…) -> R = "symbol";` with `Int` as `int64_t` and `Bool` as `bool` by
value (spec *Foreign functions*), `Str` in and out and `pub extern "C"` exports (*Foreign functions
v2*), opaque handles (`extern "C" struct Name;`), C-layout structs (`extern "C" struct Name { … }`),
`Option` of a handle for a null result, `errno` through `c_errno()` and callbacks that are an
export's name (*Foreign functions v3*). Every call has the `foreign` effect and needs a
`ForeignCap`; linking is `nazm build --link FILE`; `nazm run` refuses a program that calls C
(`N0385`). C's own failures are outside every Nazm guarantee. `nazm bindgen`, which writes these
declarations from a header, is experimental.

### The standard library

`library/std/API-1.0` lists the stable API: every item it names may gain siblings, is never
removed in 1.x, and does not change signature, effects or documented behaviour. An item not in
the manifest is internal. A removal or change is 2.0.

### Packages

Stable: `nazm.toml` (`nazm.package/1`), `nazm.lock` (`nazm.lock/1`, and `nazm.lock/2` with features, Gate 2), the resolver's exact and
deterministic resolution and its refusals (`N0500`–`N0509`), version requirements and local
registries (`nazm.registry-index/1`), `--locked`, and `nazm lock`, `update`, `publish`, `yank`.
A toolchain that cannot read a manifest or lock refuses it by schema rather than guessing; a
lockfile written by 1.0.0 is read by every 1.x. There is no hosted registry and none is
promised. `nazm init` templates are a convenience: what they write may change.

### Diagnostics and exit statuses

Diagnostic **codes** are stable and never reused, and a code's meaning does not change; new codes
may appear in any release. Message, label and help text are prose and may change in any release
— match on `code`. The process exit statuses are stable:

| Status | Meaning |
| --- | --- |
| 0 | success — nothing to report |
| 1 | the source, the program, a package or what was asked is invalid: a diagnostic, a refusal of the request (an executable over its own source, a freestanding program reaching a hosted service, a signature that does not verify), a command's own "no" (`fmt --check` with a file to change, `bindgen` with a declaration refused) |
| 2 | the program ran and failed at run time (or ran past the LIR oracle's budget) |
| 3 | a file or resource the command needs — named on the command line, in a `use`, by `--link` or `--runtime` — could not be found or read |
| 4 | the compiler, a tool it runs, or the machine failed: no `clang`, a linker or assembler that failed, an output the filesystem would not take, a target this host cannot build or link, a compiler defect (a panic is 4, not Rust's 101) |
| 5 | the command line itself is wrong: an unknown command or option, a missing or unparseable argument, an option value or combination the command cannot take (`--layout sideways`, `--target` naming no target, `--stack-watermark` for a hosted target) |

*Frozen in Q1-C1, 2026-10-08, before the first public release.* Until then a usage error exited 2
— clap's status, colliding with "the program failed at run time" — and several user errors exited
4. Each status now means one thing, and a command returns the one its failure is. The
`--summary-json` summary's `exit_code` is always the process's status. Every exit path, before and
after, is in the Q1-C1 inventory (`target/gates/q1c1/exit-inventory.md`); `tests/exit_contract.rs`
holds each status.

### Machine-readable output

Every versioned output names its schema as `name/major`. **The law for every schema, whatever its
class:** a new field, code, enum value or array element is compatible and may appear in any
release; removing or renaming a field, changing a field's type or meaning, or reusing a code
requires a new major (`docs/diagnostics.md`, *What compatibility means*). The class says what
else is promised:

- **STABLE 1.x** — no new major within 1.x; only additive changes.
- **EXPERIMENTAL** — may take a new major, or be removed, in a 1.x minor release, listed in its
  release record.
- **DEBUG/INTERNAL** — versioned so a reader can refuse what it does not know; no other promise.

**Closed schema files (v1, Q1-D-10).** The `additionalProperties: false` and closed enums in
`schema/` describe **one toolchain's output exactly**, not the format for all of 1.x: a later minor's
output may carry a field or an enum value that an earlier file closes. A consumer that validates
strictly validates against the file the producing toolchain ships; one that keeps a file must
accept what the law above allows (`docs/diagnostics.md`, *What compatibility means*).

**Inputs read strictly (v1, Q1-D-16).** For the formats the toolchain *reads* — `nazm.package/1`,
`nazm.lock/1`, `nazm.lock/2`, `nazm.registry-index/1` — the law runs one way: every 1.x toolchain
reads every file an earlier 1.x wrote, and a toolchain refuses a field it does not know, by name,
rather than ignoring it, since an ignored manifest field would be a build that silently means
something else. A field added in a minor release is therefore one a manifest opts into, and a file
using it needs that release or later.

| Class | Schemas |
| --- | --- |
| **STABLE 1.x** | `nazm.diagnostic/1`, `nazm.check-report/1`, `nazm.command-summary/1`, `nazm.test/1`, `nazm.test-summary/1`, `nazm.capabilities/1`, `nazm.fix/1`, `nazm.build-report/1`, `nazm.provenance/1`, `nazm.publish-plan/1`, `nazm.api-doc/2`, `nazm.inspect/2` (its shape; its digests are the toolchain's), `nazm.package/1`, `nazm.lock/1`, `nazm.lock/2`, `nazm.registry-index/1` |
| **EXPERIMENTAL** | `nazm.bench/1` (a measurement on one machine, Gate 2); the agent and editor surface — `nazm.context/3`, `nazm.snapshot/3`, `nazm.delta/3`, `nazm.diagnostic-index/1`, `nazm.diagnostic-detail/1`, `nazm.repository-map/1`, `nazm.task-context/1`, `nazm.patch/1`, `nazm.docs-index/1`, `nazm.docs-section/1`; analyses — `nazm.cost/2`, `nazm.flow/2`, `nazm.obligations/1`, `nazm.trace/1`, `nazm.profile/2`, `nazm.profile-report/1`, `nazm.bounds/1`, `nazm.timings/1`; optional subsystems — `nazm.accel/3`, `nazm.kernel/1`, `nazm.contract/1`, `nazm.contract-run/1`, `nazm.evm/1` and its `evm-*` siblings, `nazm.wasm-contract/1`, `nazm.wasm-contract-run/1`, `nazm.sbpf-accounts/1`, `nazm.reload-check/1`, `nazm.bindgen/1`; measurement — `nazm.token-cost/1`, `nazm.agent-benchmark/1` |
| **DEBUG/INTERNAL** | compiler views — `nazm.core-ir/2`, `nazm.mir/1`, `nazm.lir/2`, `nazm.resolved/1`, `nazm.references/1`, `nazm.interface/12`, `nazm.formal-core/2`; caches and identity domains — `nazm.check/5`, `nazm.check-key/1`, `nazm.checker/1`, `nazm.source/1`, the `nazm.object*/1`, `llvm-input/1`, `backend-tool/1`, `codegen-config/1`, `package-digest/1`, `repository-state/1` and `docs-*` domains; `nazm.runtime/1` (bound to the runtime ABI); repository tooling — `nazm.bench/2`, `nazm.evidence/1`, `nazm.sbom/1`, `nazm.mutation-campaign/1` |

Promotion from experimental to stable is a minor-release addition. `schema/` holds a JSON Schema
for most stable outputs; `nazm.package/1`, `nazm.lock/1`, `nazm.lock/2`, `nazm.registry-index/1`,
`nazm.inspect/2` and `nazm.profile-report/1` are specified by `docs/spec.md` and their tests, not
yet by a schema file.

### The command line

Stable machine output is defined by the schemas above, not by human prose: a command's text
output may change in any release.

| Class | Commands |
| --- | --- |
| **Stable public** | `run`, `check`, `build`, `test`, `fmt`, `fix`, `init`, `capabilities`, `doc`, `inspect`, `lock`, `update`, `publish`, `yank` |
| **Experimental** | `bench`, `lsp`, `context`, `snapshot`, `delta`, `diagnostics`, `docs`, `repo`, `patch`, `repl`, `comptime`, `reload-check`, `contract`, `accel`, `bindgen`, `attest`, `trace`, `profile`, `explain-cost`, `explain-flow`, `obligations` |
| **Debug/internal** | `runtime`, `core-ir`, `mir`, `lir`, `resolve`, `references`, `interface` |

A stable command's options are stable except these, which are experimental: `build --layout`,
`--runtime`, `--stack-watermark`, `--emit-ir`, `--timings`, `--attest-with`, `--sbom`, and
`--target` for any target other than the host; `--profile` and `--profile-report` on any
command, because the restriction profiles' rule sets may grow (the rule that a profile only
refuses and never changes what an accepted program does is stable). `nazm-mcp` is experimental.

### Targets

Stable means run-verified evidence for the whole language, not a triple in a list: in 1.0 that
is `aarch64-apple-darwin`, `x86_64-apple-darwin` and `aarch64-unknown-linux-gnu`, run-verified
under both backends (`docs/support.md`). The evidence is not the same for the three:
`aarch64-apple-darwin` runs the whole suite on every gate, and `aarch64-unknown-linux-gnu` runs it
in the contained runner; `x86_64-apple-darwin` has one automated program (`tests/cross.rs`, under
Rosetta 2, skipped where Rosetta is absent) and a recorded run — Q1, 2026-10-08 — of the v1
compatibility corpus's run and trap cases and the examples, 62 programs, each built by both
backends for `x86_64-apple-darwin`, run under Rosetta 2 and agreeing byte for byte, exit status
included, with `aarch64-apple-darwin` (`target/gates/q1/rosetta/results.txt`). The final release gate
re-runs that set; until it has, the claim rests on the Q1 record. Every other target `nazm capabilities` lists is
experimental in 1.0: `x86_64-unknown-linux-gnu` is compile-only, the two freestanding boards are
emulator-verified for a restricted subset, and `wasm32-unknown-unknown` is run-verified on a
reference host for a subset. A target is promoted by evidence, in a minor release.

## What is not stable

- **The runtime ABI.** Toolchain-private and versioned. Objects and prebuilt runtimes (`nazm
  runtime build`) link only with the toolchain and ABI revision that produced them, and a
  mismatch is refused. 1.x source compatibility does **not** imply binary compatibility of
  objects or runtimes across releases. It becomes a public interface only if an external
  consumer needs one, as a decision of its own.
- **Caches.** A cached check result is keyed by, among other inputs, the toolchain version, the
  semantic epoch and the interface schema; a cached object by its exact backend input, the
  backend's identity and the code-generation options. One written by another toolchain is
  recomputed, never reused.
- **Performance.** Measured and recorded (`docs/performance.md`), never promised.
- **The compiler written in Nazm.** Parity with the reference compiler holds on its declared
  conformance surface; it is a second implementation of a subset, not a stable product
  (`docs/bootstrap.md`).
- **Everything classified experimental or internal above**, and every facility
  `docs/support.md` marks experimental.

## How a change is made

1. **Constitution before code.** A semantic change is written into `docs/spec.md` (and, where
   architectural, `docs/architecture.md`) before it is implemented.
2. **The right identity moves in the same commit.** A changed checking rule bumps the semantic
   epoch (`cargo xtask check`'s cache-soundness gate fails when the checker can raise a code the
   identity does not list, and says to bump it); a changed runtime contract bumps the ABI
   revision; a changed persisted shape bumps its schema's major version.
3. **Deprecation.** Something stable to be removed in 2.0 is first marked deprecated in the spec,
   with `@deprecated` where it is a declaration (published by `nazm doc`), and named in the release
   record, for at least one minor release. A *warning* at each use, carrying a fix, needs a warning
   severity the compiler does not have in 1.0 (`limitations.md`); it is promised only once one
   exists, and until then a deprecation is announced, not reported at the use (v1, Q1-D-15).
4. **Evidence follows.** `docs/capability-matrix.md` records what is verified and how;
   `docs/limitations.md` records what is not done; `tests/compat/v1/` gains a case for every new
   stable rule.

## History: the pre-1.0 policy

*0.9.0 (2026-10-12).* The tree said toolchain 1.0.0 from Gate 1 on, but no 1.0.0 was tagged. The
first public release is the 0.9.0 preview of the 1.0 draft, built by the Rust implementation, for
macOS on Apple Silicon; 1.0 waits for the toolchain rewritten in Nazm and its release gate. 0.9 was
chosen over 0.3, which an earlier internal candidate used (`docs/releases/996984f.md`).

Until Gate 1 this document said Nazm was pre-1.0 and experimental: *"v0.x may evolve, but a
breaking change is deliberate, documented and versioned, and never silently reinterprets
existing code"*; a minor release (0.3 → 0.4) could change the language; the runtime ABI was not
a public interface; and *"What 1.0 would mean — not promised, and not scheduled."* R1 released
the v0.3.0 candidate under that policy (`docs/releases/996984f.md`). Before R1 the crates said
`0.0.1` while the specification said v0.3, and project records called the work "the v1
foundation" — an architecture phase of the roadmap, not a semantic version.
