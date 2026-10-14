# Architecture

**Status: design record. Partly implemented — see below.** This documents decisions made
before writing the compiler, and the reasoning that would otherwise be lost. Where a
decision rests on a claim that has not been checked, it says so.

## Current state — Nazm 1.0 (Gate 1, 2026-10-07)

*Read this first. Everything after it is the design record: §1–§6 are the design written before
the compiler, and each §7.n is the dated record of one milestone's decisions. They are kept, not
rewritten, and a later section or a marked correction governs an earlier one. Where they disagree
with this overview or with [`capability-matrix.md`](capability-matrix.md) about what exists
today, the matrix decides.*

**The pipeline.** One front end, two ways to run what it accepts:

| Stage | Crate | What it owns |
| --- | --- | --- |
| Source | `nazm-span` | files, `FileId`-carrying spans, the source map |
| Syntax | `nazm-syntax` | lexer, error-tolerant parser, the lossless CST and the AST |
| Meaning | `nazm-sema`, `nazm-core` | one resolution, types, generics, traits, effects, capabilities, provenance, contracts — the checker, and the interpreter `nazm run` uses |
| Modules and reuse | `nazm-iface`, `nazm-cache`, `nazm-package` | persisted interfaces (`nazm.interface/13`), keyed semantic and object caches, packages, locks and local registries |
| Core IR | `nazm-cir` | what each position of a checked program means, decided once (`nazm.core-ir/2`) |
| MIR | `nazm-mir` | every copy, move and drop explicit and validated; monomorphisation for the native path |
| LIR | `nazm-lir` | the instruction-level IR both native backends translate (`nazm.lir/2`) |
| Code generation | `nazm-lir` (LLVM text, compiled by `clang`), `nazm-codegen-clif` (Cranelift, in process) | machine code; neither backend falls back to the other |
| Runtime | `nazm-runtime` | LLVM IR for the task scheduler, channels, reclamation and the outside-world built-ins, generated into every native program (runtime ABI 15) |
| Tooling | `nazm-service`, `nazm-cli`, `nazm-mcp`, `nazm-docs`, `nazm-repo`, `nazm-formal` | the language service, the command line, the MCP server, documentation and repository maps, bounded formal checks |
| Measurement | `nazm-bench`, `nazm-agent-bench`, `nazm-tokens`, `xtask` | benchmarks, the agent benchmark, token-cost counting, the repository's gates, contained runner and release tooling |

`nazm run` interprets the checked program's Core IR; `nazm build` lowers it through MIR and LIR to
native code, and the two are held to the same results by differential tests. `compiler/*.nz` is a
second compiler, written in Nazm for a declared subset of the language (`bootstrap.md`).

**What is stable.** The language surface `spec.md` classifies STABLE 1.0, the schemas, commands
and package formats `stability.md` classifies STABLE 1.x, and nothing else. The crates' Rust APIs
are not a public interface; `publish = false` holds every one.

**What remains design, not code.** §2's HIR level, §4's automatic layout selection (one opt-in
layout exists, N68), §5's parameter conventions beyond `let` and its region calculus, effect
handlers and user-defined effects. `limitations.md` is the current list.

**Corrected 2026-09-21.** This line read "Nothing below §7 exists in code", and that was
the single most-copied false sentence in the repository — the workspace router and three
other documents paraphrased it. What is now implemented: one IR crate (§2 level 7 by name,
though not by definition — see §7), the LLVM-text backend (§2 level 8), a task and channel
runtime emitted into every compiled program, and the differential-testing discipline of §6.
What remains design: §2 levels 2-6, §4 entirely, and every part of §5 except the reduced
concurrency model. [`capability-matrix.md`](capability-matrix.md) is the authority on which
is which, area by area, with evidence.

---

## 1. Does Nazm need a new compiler design?

The question that started the project. Answered layer by layer:

| Layer | Verdict | Why |
| --- | --- | --- |
| Lexer / parser / CST | **New** | Error-tolerant, incremental, IDE-grade. Nothing reusable |
| Name resolution, type + effect inference | **New** | No existing engine does effect rows plus MVS parameter conventions |
| Core IR | **New** | Must carry effects and be content-hashable |
| MIR (ownership / regions / taint) | **New — narrow the novelty** | Rust's MIR already carries exclusivity and [region inference](https://rustc-dev-guide.rust-lang.org/borrow-check.html). What is new is the *combination* — conventions, regions, effect rows, provenance in one IR — and reusing the region solve for arena and layout decisions |
| Monomorphisation + layout selection | **New within the §4 boundary** | Prior art exists for the transformation. Whole-program automatic selection is a **proposed research direction**, pending a literature review that has not been done |
| LIR (low-level SSA) | **New, thin, deliberately boring** | Just the contract both backends read |
| **Machine code generation** | **REUSE — Cranelift + LLVM** | Instruction selection, register allocation, scheduling, and target coverage are decades of work with no novelty available |
| Linker | **Reuse** | `object` crate + system linker |
| Incremental framework | **Reuse `salsa` + own persistence** | salsa's durability story is weak; do not depend on it for disk |

**New front end, new IR, new middle end, reused back end.** But the shape is closer to
rust-analyzer plus a content-addressed cache than to a batch compiler like gcc.

### Four corrections to the obvious version of that

**"Rust is slow because of LLVM" is roughly one-third true.** LLVM is 30–50% of a rustc
debug build; the rest is trait solving, monomorphisation, borrow checking, linking.
Swapping in Cranelift buys 20–40% on debug builds, not an order of magnitude. The fast
loop comes from four tiers:

| Tier | What | Role |
| --- | --- | --- |
| 0 | `nazm check` — no codegen, function-granular incremental | ~95% of what an agent needs. A **front-end** achievement, not a backend one |
| 1 | Interpret / cranelift-jit | run tests and scripts, no object files |
| 2 | Cranelift AOT | dev binaries |
| 3 | LLVM | release only |

**Do not monomorphise in dev builds.** Monomorphisation is what *generates* the LLVM work.
Dictionary passing for tiers 0–2, monomorphise only for release (Swift's model). A bigger
dev-build win than the backend swap, and it composes with it.

**Cranelift's payoff is that one component does four jobs** — comptime evaluator, REPL,
test runner, hot reload — not "faster debug builds". Its AArch64 backend is production
grade (Wasmtime runs on it), so macOS-arm64 development is fine.

**Do not link against LLVM in year one.** Emit LLVM IR as *text* and shell out to
`clang`/`opt`. Costs perhaps 10–20% of release build time, and release builds are rare.
Keeps `nazm` a small Cranelift-only binary with the LLVM toolchain an optional download.
Also practical: the development machine had 18GB free of 460GB when this was written.

### Two backends is two sets of semantics bugs

UB, floating point, overflow, atomics, unwinding, ABI. A bug reproducing only under
`--release` is the worst class there is. Mandatory once codegen exists:

- LIR stays narrow and prose-specified; backends translate, they do not optimise
- a **LIR reference interpreter** is the semantic oracle
- differential fuzzing in CI from the start
- release is **always** LLVM, so divergence never reaches a user

Differential testing catches divergence, not bugs shared by all implementations. See §6
for what must be specified before backends can be compared at all.

### Not MLIR, in v1

It offers real things — pass infrastructure, a dialect system, affine and vector dialects.
It costs a C++ build dependency in a Rust project, immature Rust bindings, coupling to
LLVM's release cadence, and decisively: it is not a content-addressed incremental database
and cannot be made into one. Revisit only if Nazm pivots toward tensor/accelerator codegen.

## 2. IR levels

| # | Level | Form | Content-addressed | Consumers |
| --- | --- | --- | --- | --- |
| 0 | Source | UTF-8 `.nz` in git | file hash | humans, agents, git |
| 1 | CST | lossless green tree, error-tolerant | interned | formatter, LSP, structured edits |
| 2 | HIR | name-resolved, path-based `DefId` (never positional) | no | inference, IDE |
| 3 | **Core** | typed, effect rows explicit, traits→dictionaries, ANF | **yes** | comptime, REPL, cache keys, oracle |
| 4 | Core′ | after comptime partial evaluation | derived | — |
| 5 | MIR | CFG over *places*: moves, projections, drops, regions, effect ops | cached | exclusivity, region/escape, taint |
| 6 | Mono MIR | monomorphised, devirtualised, layout-selected | cached | layout pass, whole-program opts |
| 7 | LIR | SSA, concrete layouts, ABI-explicit | cached | the single backend contract + oracle interpreter |
| 8 | Machine | Cranelift IR (dev) or LLVM IR text (release) | by LIR hash | — |

**The commitment that is hardest to reverse:** field access stays a symbolic
`project(place, field_id)` through level 6. Offsets appear only at level 7. Bake offsets
into the front end — as most compilers do — and layout selection becomes unrecoverable.

## 3. Source of truth, and what Unison teaches

**Source of truth is plain UTF-8 `.nz` files in git.** Non-negotiable. grep works,
`git diff` works, code review works, every editor works, and — decisively for this project
— LLMs have seen text.

[Unison](https://www.unison-lang.org/docs/the-big-idea/) shipped content-addressed code to
1.0 in November 2025. What hurt its adoption was not content addressing; it was replacing
the *file* as the unit of code, which broke every existing tool at once in exchange for
benefits users could not feel on day one. For Nazm that trade would be worse, because text
source is the transfer-learning surface and it is the largest lever the project has.

**Content addressing applies to derived artifacts only.** After name resolution and type
checking, each top-level definition is normalised (names erased to de Bruijn indices,
dependencies replaced by their hashes) and hashed → a Merkle DAG. That hash keys
type-check results, MIR, monomorphised instances, LIR, object code, test results, docs.

The payoff that motivates it: **definition-hash equality.** If an agent rewrites a function
and the normalised Core hash is unchanged, the compiler knows the edit produced an
identical definition and can say so. This detects **one class** of no-op edit — normalised
Core alpha-equivalence — and not all behaviour-preserving rewrites. Definition hashing is
Unison's idea; the contribution here is the agent-facing use of it.

### Cache-key correctness

**Determinism aids reproducibility but does not make incorrect cache hits impossible.** The
key must cover:

compiler identity · schema version · transitive dependency hashes · target triple and CPU
features · optimisation flags · ABI · every compile-time input the result depends on

Anything omitted is a silent miscompile waiting to happen.

**Comptime execution through a JIT needs a sandbox**: no filesystem, network, or clock by
default, with resource and time limits and an explicit capability to widen. Zig and Rust
restrict const-eval for exactly this reason.

## 4. The layout-selection boundary

Symbolic projections are necessary but not sufficient. Before any automatic layout change,
the spec must define behaviour for: **address identity, escaping references, FFI,
serialization, atomics, separately compiled modules.**

Conservative start: **explicitly eligible collections whose elements cannot expose a stable
address**, opt-in at the type level, compiler chooses within that envelope. Widen only with
evidence.

Prior art exists: published [AoS-to-SoA compiler work](https://arxiv.org/html/2502.16517v1),
ISPC, Julia's `StructArrays`, explicit SoA in Jai and Odin. Whole-program automatic
selection with a sound semantic boundary is **Nazm's proposed research direction** — stated
that way because no literature review has been done, so "nobody has done this" is not a
claim this project is entitled to make. The survey is a Phase 1 deliverable.

**Benchmarking rule:** layout wins are measured against **both idiomatic Rust and
hand-written SoA Rust**. Beating only idiomatic Rust measures a different representation,
not a better compiler.

## 5. Type system

### Mutable value semantics, not lifetimes

Everything is a value; no references in the type system, only *projections* that provably
cannot escape. Ownership lives in parameter conventions — `let` (read-only projection),
`inout` (exclusive mutable), `sink` (consumed), `set` (initialised) — inferable at call
sites and checked **intraprocedurally**. No lifetime variables, no annotations, no
non-local error messages.

Honest costs:

- **Expressiveness.** Cyclic and shared-observer structures need arena indices rather than
  pointers. Good Rust does this anyway, but it is a real restriction.
- **Silent copies.** MVS says "pass by value" and relies on elision. Two separate
  diagnostics, because **a no-codegen check cannot know what the backend did**:
  `nazm check` reports *potential* copies (front end, conservative, incremental, in-editor);
  `nazm build --remarks` reports *surviving* copies (backend optimisation remarks, accurate,
  post-codegen).
- **Maturity.** [Hylo](https://hylo-lang.org/) is a research language with no large
  production codebase, and **adopting the terminology does not supply the rules.** Nazm
  needs its own specification of copying, borrowing, closure capture, and concurrency
  interaction. This is risk #2 after training data.

**The hedge:** the IR carries a full region/borrow calculus even though the surface has
none. If MVS proves insufficient, the underlying reference model can be exposed without
re-founding the language. Never design a surface whose semantics the IR cannot express a
superset of.

### Effects, capabilities, provenance are three problems, not one

They share row-unification machinery. They answer different questions:

- **Effects** — what operations can this function perform?
- **Capabilities** — what authority does this code hold?
- **Information flow** — where can this data reach?

An earlier draft called provenance "nearly free once effects exist". That was overclaiming.

**Effects** are a row on every function type in Core: `fn(A) -> B / {io, alloc, panic | r}`.
Inference is unification over rows with a row variable for polymorphism; the check at each
call site is a subset test. Polymorphism is what prevents function colouring:
`fn map[T,U,e](xs: List[T], f: fn(T)->U / e) -> List[U] / e` serves pure and effectful
callers from one definition.

**Purity is a prerequisite, not a licence.** An empty row does **not** by itself authorise
dropping, reordering, memoizing, or parallelising a call:

```nazm
fn forever() -> Int / {} { loop {} }

forever()           // dropping this unused call changes the program
print("finished")   // from never printing to printing
```

Each transformation must additionally establish its own conditions — termination,
exceptions, dependencies, ownership, observable behaviour.
[LLVM](https://llvm.org/docs/LangRef.html) separates memory effects, termination/progress
(`willreturn`, `mustprogress`), and speculatability precisely because they are not
interchangeable. Nazm's effect row must do the same rather than collapse them into one bit.
**Open spec question: is divergence an effect?** See `spec.md` §Open.

**Provenance** is a lattice-valued qualifier (`Untrusted(source)`, `Secret`, `PII`) on the
same row machinery, plus a flow-sensitive MIR pass for precision. The rule is
**position-sensitive**, which is what separates taint checking people use from taint
checking people disable:

```nazm
db.query("SELECT * FROM items WHERE id = $1", [req.body.id])   // legal: bound parameter
db.query("SELECT * FROM items WHERE id = " + req.body.id)      // error: query construction
```

Same shape for HTML text vs markup, shell argv vs shell string. Still to specify: implicit
flows (branching on a secret), sanitizers, aliasing, declassification (capability-gated),
the FFI boundary.

### Concurrency

M:N green threads, work-stealing scheduler, growable stacks (copy-and-grow, not segmented),
blocking-looking code with no `async`/`await`, reactor on kqueue/epoll/io_uring, FFI
blocking handoff detected by an effect annotation on the extern declaration.

Structured concurrency is mandatory:

```nazm
scope { s =>
    s.spawn { mutate(x) }
    mutate(x)              // compile error
}
```

**The borrow region is `[spawn point, scope end]`**, and parent *or sibling* access to that
place anywhere in the region is rejected — the same discipline as Rust's `thread::scope`.
An earlier draft justified this by saying the parent is "provably blocked at the scope's
end", which is wrong: the parent runs before the join.

Channels transfer ownership (`send` takes `sink T`), so nothing aliased crosses one. That
does **not** cover captured variables, globals/statics, or foreign pointers; each needs its
own rule.

Cancellation is ambient in the scope rather than a `ctx` threaded through every signature —
it is a scoped effect handler. Detached tasks require an explicit `daemon` capability.

**Deadlock-freedom is not claimed.** Ownership prevents data races, not lock-ordering
mistakes or channel cycles. Ship a lock-order lint and a debug-build detector.

**The task-count target is withdrawn.** "1M live tasks under 2GB" was arithmetically
impossible — 1M × 4KB is 4.1GB of stack alone, and macOS arm64 uses 16KB pages, though only
*if* each stack is a separate page-granular resident allocation; a pooling allocator packing
small stacks into shared pages would not pay that. Which is why it is replaced by a
benchmark that states its terms: task states distinguished (created / parked / runnable /
running), allocation strategy stated, lazy commit explicit, **RSS measured rather than
virtual reservation**, page size recorded per platform.

## 6. Differential testing: what must be specified first

`nazm test --backend=all` compares only **specified** semantics. Before any backend
comparison the spec must fix integer overflow, floating-point semantics, panic and unwind
behaviour, and what counts as an observable output. Undefined behaviour cannot be
differentially tested — only differentially *discovered*.

Deterministic sequential programs first. Concurrent programs legitimately produce different
schedules, so those compare **permitted outcomes and invariants**, never identical traces.

| Cadence | Checks |
| --- | --- |
| Pull request | Conformance, golden diagnostics, bounded differential cases, `cargo xtask check`, `cargo lint` |
| Scheduled | Longer fuzzing, broader targets, reproducibility matrix |
| Release | Full supported matrix, plus review of every outstanding discrepancy |

## 7. Crate layout

Crates are created when their responsibility exists.

**Corrected 2026-09-20.** This section read "Phase 2 is `nazm-bench` and `xtask` only —
there is no compiler yet, deliberately", and that is no longer the case: the Phase 4
front-end crates exist and a `.nz` program executes.

**Corrected again 2026-09-21.** The sentence that followed — "everything from `nazm-hir`
downward is still design" — has itself rotted, which is worth leaving visible: a correction
is not permanent either. `nazm-lir` exists, the backend exists, and a runtime is emitted
into every compiled program. What is still design is everything *between* the AST and
`nazm-lir`: `nazm-hir`, `nazm-ty`, `nazm-db`, `nazm-mir`, `nazm-mono`. See
[`roadmap.md`](roadmap.md) for ordering and [`capability-matrix.md`](capability-matrix.md)
for what each one's absence currently costs.

**Corrected 2026-09-30 (N39).** One level between the AST and `nazm-lir` now exists: Core IR,
`nazm-cir`, the one lowering of a checked program that the interpreter runs and `nazm-lir`
lowers (§7.41). It earned its crate the way §3 of `master-architecture.md` requires — by taking
a responsibility away from two places that each held it: deciding, from the tree, what a
program's positions, `?`s, blocks and transfers mean. `nazm-hir`, `nazm-ty`, `nazm-db`,
`nazm-mir` and `nazm-mono` are still design.

**Corrected 2026-10-01 (N40).** `nazm-mir` exists: MIR, the one lowering from Core IR for the
native backend (§7.42), which took from `nazm-lir` its temporaries, its partly-built slots and its
`?` completion, and from the emitter every cleanup decision it used to make per exit. The instance
planner moved into it, so monomorphisation — the table's `nazm-mono` — is MIR lowering's first
step rather than a crate of its own. `nazm-hir`, `nazm-ty` and `nazm-db` are still design.

| Phase | Crates | State |
| --- | --- | --- |
| 2 | `nazm-bench` · `xtask` | **exists** |
| 4 | `nazm-span` · `nazm-diag` · `nazm-syntax` · `nazm-sema` · `nazm-iface` · `nazm-core` · `nazm-cli` | **exists** (`nazm-core` is the checker, the one lowering to Core IR, and the evaluator over Core IR; `nazm-sema` and `nazm-iface` are not on this diagram at all — see §7.2 and §7.3) |
| 4 | **`nazm-cir`** | **exists** (N39) — Core IR, level 3 by responsibility: typed, resolved, explicit control, effects as function metadata. A tree of structured regions, not ANF, and no traits to lower. §7.41 |
| 4 | `nazm-hir` · `nazm-ty` · `nazm-mcp` | planned |
| 6 | **`nazm-mir`** | **exists** (N40) — MIR, level 6 by responsibility: each concrete function as a typed CFG with every copy, move, drop, scope join and failure edge explicit, validated. Not SSA, and no borrow checking. §7.42 |
| 6 | `nazm-db` · `nazm-lsp` | planned |
| 7 | **`nazm-lir`** | **exists** — since N40 it reads validated MIR and holds layout, symbols and LLVM text emission; the structured tree that was once named for this level is gone. See the note below |
| 7 | **`nazm-codegen-clif`** | **exists** (N41) — the Cranelift backend behind `nazm-lir`'s `Backend`: every MIR function body to Cranelift IR to one object per unit, in process. §7.43 |
| 7 | `nazm-link` · `nazm-driver` | planned — clang links, and `nazm-cli` drives |
| 7 | `nazm-test` | **exists as a subcommand**, `nazm test`, not as a crate |
| 8 | `nazm-rt` | planned |
| 9 | `nazm-mono` | planned — and nothing is polymorphic, so there is nothing to monomorphise yet |
| 9 | `nazm-codegen-llvm` | **exists inside `nazm-lir`**, as LLVM IR emitted in text |
| 10 | `nazm-pkg` · `nazm-std-build` | planned |

One deliberate departure from this table is already recorded: [`roadmap.md`](roadmap.md)
M1 brings `nazm-lir` and `nazm-codegen-llvm` forward, and does **not** create the levels
between them and the AST. Five empty pass-through crates would match the diagram and carry
no information.

**A name is not a level.** `nazm-lir` is named for level 7 and owns a real responsibility —
it records which operations can fail and where the source said so, which the AST does not
carry. Its *form* is not what the table above describes: `While`, `If` and `Scope` nest,
locals are slot indices, and nothing is in SSA form. Reading the crate list as a progress
bar against this diagram is what produced four contradictory status claims in September
2026. The rule that replaced it is in
[`master-architecture.md`](master-architecture.md) §3: **an IR level earns its existence by
owning a semantic responsibility no existing level owns**, and matching the diagram is not
such a reason.

Roles, in dependency order: `span` (files, byte spans — depends on nothing; the `FileId`
this list promised **exists** since N1, and a `Span` is a file and a range inside it) ·
`sema` (types, built-in identity, module identity, durable keys, unit interfaces,
resolution — not a row in the table above, and §7.2 says why) · `iface` (a module's
interface as persisted, and its fingerprint — §7.3) · `diag` (stable error codes, fixits, JSON schema, renderer) · `syntax`
(lexer, parser, CST, typed AST, canonical formatter, CST-preserving edits) · `hir` (`DefId`
graph, modules, name resolution) · `ty` (types, effect rows, provenance, inference) · `core`
(the checker, the one lowering to Core IR, the reference interpreter) · `cir` (Core IR: its
invariants, its printed form, per-function digests — §7.41; Merkle hashing over definitions is
not built) · `db` (salsa queries +
on-disk store) · `mir` (exclusivity, regions, taint, drop elaboration, effect lowering) ·
`mono` (monomorphisation, devirtualisation, layout selection) · `lir` (backend contract +
oracle interpreter) · `codegen-clif` / `codegen-llvm` · `link` · `rt` (scheduler, stacks,
channels, reactor, allocator — the one crate allowed `unsafe`) · `std-build` · `driver` ·
`pkg` · `test` · `lsp` · `mcp` · `bench` · `cli` · `xtask`.

`unsafe_code` is `forbid` at the workspace level today. It becomes `deny` with a per-crate
allowance for `nazm-rt` and the FFI crates when those exist — stack switching and allocators
cannot be written without it. `xtask` enforces that the allowance stays narrow.

## 7.1 Where source identity and resolution live — settled by N1, 2026-09-21

§7's table lists crates by the level they would occupy. That was the wrong question to ask
next, and `master-architecture.md` §3 says why: **a layer earns its existence by owning a
semantic responsibility no existing layer owns.** The table would have put `nazm-hir` next
because it is the next row. The tree gave a better reason: name and signature resolution
was performed *twice* — once in `nazm_core::check`, whose answer `nazm-core/src/lib.rs`
discarded, and again in `nazm-lir/src/lower.rs`, which rebuilt an equivalent scope stack,
slot allocator and name-keyed signature table because `xtask/src/rules.rs` forbids the
backend depending on the interpreter's crate.

**No new crate was created.** The shared vocabulary went to `nazm-syntax`, which both
consumers already depend on:

| What | Where it lives now | Why there |
| --- | --- | --- |
| `FileId`, `Span { file, start, end }`, `SourceMap` | `nazm-span` | it already owned positions; a span naming its file is the same responsibility done properly |
| `Type`, `Intrinsic` | `nazm-syntax` | the language's types and built-in signatures are surface vocabulary, and the backend needs them without reaching the interpreter |
| `Resolution` — definitions, signatures, call targets, slots | `nazm-syntax` | the one type both consumers read; the **algorithm** stays in `nazm_core::check`, which produces it while checking |

That last row is the part worth stating plainly, because it is where a new crate would have
been easy to justify and wrong. `Resolution` is a **value**, not a pass. The resolver did
not move; the second one was deleted. A `nazm-hir` crate holding a copy of the checker's
answer would have been the pass-through this section was written to prevent — and the
acceptance test for such a crate, *after it exists, one of the two resolvers is gone*, is
met here without one.

**A resolution is not an IR.** It holds no expressions and no control flow. Levels 2–6 of
§2 remain absent, and `capability-matrix.md` areas 8 and 9 stay MISSING.

What N1 did **not** settle: there is still no compilation unit — every file is parsed
separately but checked together — and every id in a `Resolution` is session-local, so it is
not a cache key and not a definition identity that survives a process. `spec.md` *Open*
carries both, and `research-register.md` R2 and R3 depend on them.

## 7.2 Modules, interfaces, and the crate N2 created — settled by N2, 2026-09-22

N1 left two things open and this milestone closed one of them: **what a compilation unit
is.** `spec.md` owns the answer — one module, and today one module is one file — and this
section owns where each part of it lives.

### The passes, and what each may see

```text
load          the driver follows `use`, reads each file once by canonical path,
              and produces a SourceMap *and* a ModuleGraph. The last time a path
              is consulted.
                  ↓
parse         each file on its own text
                  ↓
declare_unit  per module: its declarations, their signatures, which are `pub`.
              Reads no other module — which is why the stage needs no ordering
              and an import cycle has none to violate.
                  ↓  UnitInterface per module
check_unit    per module: bodies, against its own definitions and the
              *interfaces* of the modules it directly imports.
                  ↓  one Resolution
lower / eval  read it. Neither resolves a name; neither knows the rule.
```

`check(program, graph)` is a loop over the two stages and holds no resolution of its own,
so a compilation of many modules and a compilation of one take the same path.

### Where each thing lives

| What | Where | Why there |
| --- | --- | --- |
| `ModuleId`, `ModuleGraph`, `Dependency` | `nazm-sema` | the semantic unit and the edges between units; built by the driver, read by everything after |
| `UnitInterface`, `Export`, `Visibility` | `nazm-sema` | what one unit offers another. No bodies, no private definitions, no hashes |
| `Resolution`, `FnDef { module, visibility, decl }`, `FnId` | `nazm-sema` | one answer, read by the evaluator and the backend |
| `Type`, `Intrinsic` | `nazm-sema` | moved from `nazm-syntax`; see below |
| the module graph's **construction** | `crates/nazm-service/src/load.rs` (in `nazm-cli` until N16) | resolving a path reads the filesystem, which is an effect, and the front end performs none |
| `declare_unit`, `check_unit`, `Env` | `nazm-core` | resolution is a pass, and the pass did not move |

### Why a crate this time, when N1 declined one

§7.1 declined `nazm-hir` because it would have held a copy of the checker's answer. This
crate is a different case, and the evidence is in the source rather than in this diagram:
**`nazm-syntax`'s lexer, parser, formatter and AST referred to `types`, `intrinsic` and
`resolve` exactly zero times.** They sat there because N1 needed a place both consumers
could reach, which was the right call for three types and the wrong one for eight.

`nazm-sema` depends on `nazm-span` and nothing else — deliberately not on `nazm-syntax`,
so the vocabulary a checker records its decisions in cannot begin describing the tree those
decisions were read off. The AST carries `Function::pub_span`, which records **that `pub`
was written and where**; what it *means* is `Visibility`, in `nazm-sema`, so visibility is
decided in one place.

It is not on §7's table, and that is the point rather than an omission. It owns a
responsibility no row owns: `nazm-hir` on that list is a graph of definitions *with
bodies*, and this holds no body at all.

### Definition identity

`FnId` is the definition identity, and it stays flat rather than becoming
`DefId { module, index }`. A flat index already distinguishes every definition in the
compilation; qualifying it by module would add a second indirection and no discriminating
power. What N2 needed was an **owner**, and that is `FnDef::module`, so "which unit owns
this definition" has an answer without changing what identifies one.

It remains session-local, in the same sense as `FileId` and `ModuleId`. Nothing here is a
cache key or survives a process; `research-register.md` R2 and R3 are unchanged by this.

### The same architecture, in the compiler written in Nazm — N2.1, 2026-09-22

Worth recording because it is a second implementation of these semantics, in a language
with no structs, no nested arrays and no map type, and it came out with the same shape:

```text
module.nz    load_modules  →  the merged text, and beside it: where each module's text
                              begins, and which module each `use` names
analyse.nz   collect_signatures  →  each definition's owner and `pub` bit
             check_imports       →  every module's visible imports, before any body,
                                    with N0207/N0208 raised at the `use`
             check_all           →  bodies, resolving each call in the asking module and
                                    **recording** what it resolved to
emit.nz                          →  reads that record; looks up no name
```

Parallel `Ints`/`Strs` columns stand in for the structs `nazm-sema` has, and a linear scan
stands in for a `HashMap`. What is not a substitution is the division of responsibility:
the parser records that `pub` was written, the module layer records relationships, the
checker decides what a name means, and emission consumes the decision. The last edge is the
one that had teeth — emission used to turn a *name* into an LLVM symbol, which was correct
under one namespace and a miscompile under modules.

### What is still whole-program

*Written at N2, and superseded by §7.5 on 2026-09-22. Kept because it is the statement N5
had to make false.*

> Lowering and code generation. `nazm_lir::lower` takes the whole `Program` and one
> `Resolution`, emits one LLVM module, and numbers its functions `@nz.fN` — which is why two
> modules' private `helper` needed no backend change at all. There are no per-module object
> files, no linker symbols and no cached interfaces, and `capability-matrix.md` says so
> rather than implying separate compilation from the existence of a unit.

What remains whole-program after N5 is two *analyses* — signature admissibility and the
recursion refusal — and not generation. §7.5 says why both stay.

## 7.3 Durable identity and persistent interfaces — settled by N3, 2026-09-22

N2 gave a definition an owner and a module an identity, and both were **session-local**:
indices into one compilation in one process. That is the right shape for a compilation and
the wrong one for anything that outlives it. N3 adds a second layer beside it, and the two
coexist on purpose.

### Two layers, not a rename

| | Answers | Shape | Lives |
| --- | --- | --- | --- |
| `FileId` | which source did this span come from, **here** | index into a `SourceMap` | one process |
| `ModuleId` | which loaded module owns this, **here** | index into a `ModuleGraph` | one process |
| `FnId` | which declaration does this reference mean, **here** | index into a `Resolution` | one process |
| **`ModuleKey`** | which logical module is this | normalised source-root-relative path | across processes |
| **`DefKey`** | which logical definition is this | `ModuleKey` + kind + name | across processes |

The session ids are not deprecated and were not renamed. They are dense, cheap and exact
within a compilation, which is what the checker and the backend need; a durable key is a
string, computed once per module, which is what a file on disk needs. `spec.md` owns what
the durable ones *mean*; this section owns where they live.

`ModuleGraph::key(ModuleId)` and `Resolution::def_key(FnId)` are the mapping, and both
return `Option` — a module outside the source root or reached under two names through a
symbolic link has **no** durable key, and is refused one rather than given a guess.

### The serialization boundary, again

`Span` taught this: a semantic type and a wire representation are different
responsibilities, and putting the second inside the first makes every use of the first
lossy. So `nazm-sema` still depends on `nazm-span` and nothing else — **no serde** — and a
separate crate owns what a module publishes:

```text
nazm-sema    semantic, session-local, and durable *identities*      → span
nazm-iface   the persisted form of a module's interface, and its
             fingerprint                                            → sema, span, serde, blake3
```

`nazm-iface` converts explicitly. `UnitInterface` (in-memory, holds `FnId` and `Span`)
becomes `PersistentUnitInterface` (durable keys, canonical type names, no spans) by a
function that takes the module's `ModuleKey` and fails without one. Nothing derives
`Serialize` on a session-local type, because the crate that holds them cannot: `cargo xtask
check`'s **sema purity** gate fails if `nazm-sema` gains a serde dependency, in the same way
and for the same reason as the span-identity gate.

### Three fingerprints, kept apart

| | Over | Used for |
| --- | --- | --- |
| **`InterfaceHash`** | the canonical bytes of a module's persisted interface | deciding whether a module's *dependents* must be re-checked |
| Implementation fingerprint | a module's own bodies | deciding whether a module's *own* output can be reused — **not built in N3** |
| `BuildKey` | compiler identity, target, flags, dependency hashes | reusing a generated artefact — **not built in N3** |

Collapsing them is the mistake worth naming: an `InterfaceHash` used as a codegen cache key
would reuse an object file after a body change, because a body is deliberately not in it.
N3 builds the first and states the other two so the first cannot be quietly promoted.

### Invalidation, as a consequence rather than an engine

The fingerprint exists so that this holds, and it is tested:

```text
B's private body changes   → B's InterfaceHash unchanged → A need not be re-checked
B's exported signature      → B's InterfaceHash changes   → A must be re-checked
```

Propagation stops where it stops being true: re-checking A produces A's interface, and if
*that* is unchanged, A's own importers are unaffected. That is the intended model and it is
written down here; **no scheduler, cache or query engine implements it**, and
`capability-matrix.md` says incremental compilation is absent.

### What the self-hosted compiler does with all this

Nothing, deliberately. Persistent interfaces are toolchain architecture, not language
semantics: no Nazm program can observe a `ModuleKey`, and the bootstrap chain neither writes
nor reads an interface file. What the two implementations must agree on is the *rule* — and
they do, by both declining durable identity exactly where they could disagree. If a later
milestone gives the Nazm-written compiler an incremental path, it needs `realpath` first;
`spec.md` records why.

## 7.4 Semantic reuse, and the keys that make it sound — settled by N4, 2026-09-22

N3 built identity and an interface fingerprint and reused nothing. N4 is the first
milestone that *skips work*, which makes it the first one where being wrong is a
correctness bug rather than a slow build.

### What is reused, precisely

**The second stage of checking one module: its bodies.** Not reading the file, not parsing
it, not reading its declarations. Every module is read, parsed and declared from source on
every run, which is what makes a skipped module invisible — the interface every other
module's key depends on, and the compilation's own facts including whether there is a
`main`, are recomputed rather than recalled.

So the claim is **incremental semantic checking**, and it is not incremental compilation.
Lowering and code generation still take the whole program (area 10); `nazm run` and `nazm
build` reuse nothing, because both need the resolution that only checking a body produces.

### A declared source root came first

`spec.md` owns the rule. The reason it had to be settled before anything was stored: under
a derived root a module's key depends on which file the compilation began at, so two
compilations of one project disagree about what their modules are. That is sound — the keys
differ, so nothing wrong is found — and it is useless, because nothing is ever shared.

`nazm.root` is a marker with no content, and the emptiness is enforced. A configuration
file is justified when it owns a responsibility; this one owns exactly one fact, and a
manifest is what it must not be allowed to become.

### Three fingerprints, and three keys

N3's table stands; N4 adds the middle row of the second.

| | Over | Answers |
| --- | --- | --- |
| `InterfaceHash` | a module's published interface | must its **dependents** be re-checked |
| **`CheckKey`** | everything semantic checking of a module reads | may its **own check** be skipped |
| `CodegenKey` | that, plus implementation, target, CPU features, backend, optimisation level, ABI | may its **object code** be reused — **not built** |

`CheckKey` is the checker's identity, the module's `ModuleKey`, its `SourceFingerprint`,
and the `(ModuleKey, InterfaceHash)` of each direct import. It is deliberately **not** a
`CodegenKey`: it carries no target and reaches a body only through the module's own source
fingerprint, so an object file keyed on one would be handed back for a different machine.

**`SourceFingerprint` normalises nothing** — not newlines, not whitespace, not comments, not
a BOM. A comment edit re-checks its own module and stops there, because the module then
publishes the same interface. Normalising would mean deciding that two byte sequences check
identically, and every such decision is somewhere the compiler and the normaliser can come
to disagree.

**What is deliberately excluded**, each checked against the implementation rather than
assumed: the target triple, CPU features and ABI (no rule in `crates/nazm-core/src/check/`
consults any of them, and `Int` is 64-bit in every configuration); the optimisation level
and backend (`nazm build`'s subset refusals live after checking); the interpreter's
iteration and depth budgets; the checkout directory and the working directory; the cache's
own location; and whether the compilation is a program, because `N0204` is a fact about a
compilation and is recomputed every run. The first row is right *today*, and a rule that
became target-sensitive would have to join the key on the day it was added.

### Compiler identity: derived where it can be, declared where it cannot

A result produced under one set of checking rules must not be reused by a compiler with
another. Nothing affordable computes "do these two binaries check identically" — a digest of
the compiler's source changes when a comment does, and a digest of the binary changes with
the optimisation level of the build that produced it. So `CheckerIdentity` derives the
built-in table with its signatures, the type set, both schemas and the crate version, and
names the rest as `SEMANTIC_EPOCH`, a counter bumped by hand.

The maintenance rule is one sentence — *bump the epoch in the commit that changes what the
checker accepts or refuses* — and it is made hard to forget rather than merely written down.
`cargo xtask check`'s **cache soundness** gate fails when the set of diagnostic codes
`nazm-core`'s checker can raise stops matching the list the identity is built from, and
nearly every new rule brings a code. What it does not catch is a tightened rule reusing an
existing code, or a parser that starts accepting a construct the checker then sees; that is
what the epoch is for, and saying so is better than claiming an automatic identity this
does not have.

### The algorithm, and why it needs no ordering

```text
stage 1   every module's declarations, from source            → its UnitInterface
          an interface depends on that module alone,
          so every InterfaceHash exists at once — cycles included
stage 2   per module: CheckKey → look it up
          hit  → its bodies are not checked
          miss → check, and if it produced no diagnostic at all, publish
```

There is no scheduler and no dependency order, because there is nothing to order. The one
thing that would have needed one — a module's interface — was already local as of N2, which
is the property that makes an import cycle legal and the same property that makes this
work.

### Invalidation, now implemented rather than described

```text
B's private body changes  → B's SourceFingerprint moves → B re-checked
                          → B's InterfaceHash does not  → A's CheckKey holds → A reused
B's exported signature    → B's InterfaceHash moves     → A's CheckKey moves → A re-checked
                          → if A's own interface holds, A's importers are unaffected
```

Propagation is interface-driven, not reachability-driven: it stops where a re-checked
module's interface turns out to be unchanged. `crates/nazm-cli/tests/incremental.rs` runs
all three cases of `A → B → C` and requires the middle one to stop at B.

### The cache is never authority

Source under this compiler decides what a program means; an entry is derived data.

* **only a clean check is written.** A module with a diagnostic in either stage publishes
  nothing, so no entry is a cached refusal and no hit can suppress an error;
* **reading validates.** The file name is a digest, and a digest match is evidence rather
  than proof, so the entry repeats the inputs it was keyed on, the key is recomputed from
  them, and the interface is revalidated through the reader that owns that format. A
  truncated, edited, mis-filed or foreign entry is a **miss** — never a hit and never a
  compilation error;
* **publication is atomic.** Write, `fsync`, `rename`. An interrupted compiler leaves a
  `.part` file that nothing looks for;
* **entries are immutable**, because they are addressed by everything they depend on. Two
  compilers racing to publish one result compute the same bytes, so the second rename lands
  the same file and no lock is needed;
* **deleting the store changes nothing** but the work done.

### Where it lives, and where it declines

`.nazm/check/` under the source root, overridable with `NAZM_CACHE_DIR`. Beside the tree
because it is derived data about that tree: deleting the checkout deletes it, a second
checkout does not inherit it, and no compiler writes to a home directory. The location is
not part of any identity — the same entry computed anywhere is the same bytes under the
same name.

*Since R1.5 (D12, §7.113)* one kind of entry does live in the user's home directory: the native
objects of the runtime and of the standard library, which are the same for every project. A
project's own check results and objects are still kept here, and nothing else changed.

A module with no durable identity — outside the source root, or given two names by a
symbolic link — is checked from source every time and publishes nothing. That is N3's
principle applied rather than re-decided: decline where the two implementations could
disagree, and never let a limit on an optimisation become a refusal to compile.

### Lifecycle: there isn't one, and that is the design

The store may be deleted at any moment, whole or in part, by anything. An entry nothing
asks for is never read, so a store full of results for source that no longer exists costs
disk and nothing else. **There is no pruning, no eviction, no size bound and no expiry**,
because each of those is a thing that deletes files, and a compiler that deletes files
under a directory a person chose needs a much better reason than tidiness. A size policy is
a later decision with its own argument; until then `rm -rf .nazm` is the whole interface,
and it is safe by construction.

The one file the compiler removes is the temporary it just failed to write. Nothing scans
the directory, nothing removes another process's leftovers, and nothing outside the store
is touched.

### What the self-hosted compiler does with all this

Nothing, deliberately, as with N3. A cache is toolchain architecture: no Nazm program can
observe a `CheckKey`, the bootstrap chain neither reads nor writes an entry, and porting a
store into the Nazm-written compiler for symmetry would be building a second thing to keep
correct. What the two implementations must agree on is the language, and that is what
`bootstrap.md` measures.

## 7.5 Separate code generation and internal linking — settled by N5, 2026-09-22

N2 gave the language a compilation unit and N4 made checking per-unit. Emission stayed
whole-program: one LLVM module, functions named `@nz.f<N>` by their position in one
numbering. N5 makes generation per-unit too.

```text
checked compilation
        ↓ global backend admissibility
lower each module            → one Unit per Nazm module
        ↓
emit each unit               → one LLVM module each, plus runtime and entry
        ↓ clang -c, once per artefact
objects
        ↓ linker
one executable
```

### A symbol is a spelling of a definition's identity

`@nz.f<N>` could not survive this: the number depends on how many functions were collected
before it, so a module's symbols would change when an unrelated module gained a function.
So a symbol is derived from the `DefKey` N3 already settled:

```text
DefKey (nazm-sema)   which definition is this — backend-independent
        ↓
NativeSymbol (nazm-lir)   what the linker calls it
```

Each part is encoded byte by byte: alphanumerics stand for themselves and everything else,
**including `_`, becomes `_XX`**. Because `_` is itself escaped it never appears literally,
so the encoding is injective and `.` is free to separate fields. A symbol is therefore
unique **by construction** — no digest, no truncation, no collision probability. That
guarantee is available because the inputs are small, and it is why this scheme is preferred
to hashing. *v1 (Q1-B-06):* the one input that is not always small is a generic instance's type
arguments, whose spelling nesting can make exponential; past 512 bytes an instance is named
`….d.<BLAKE3 hex>` instead of `….g.<spelling>`, unique by collision resistance, since `.` cannot
occur in a spelling and `d.` and `g.` therefore never meet.

```text
@nz.m.lib_2Futil_2Enz.parse       lib/util.nz :: parse
@nz.m.a_2Enz.helper               a.nz :: helper
@nz.m.b_2Enz.helper               and b.nz's, which is a different definition
@nz.u.2.helper                    a module with no durable identity: compilation-local
```

The kind is deliberately absent, which is the opposite of what `DefKey` does and right for
the same reason: a durable identity that did not say what it identified would have to be
*reinterpreted* when a second kind arrived, while a native symbol is regenerated on every
build and read back by nothing.

**A body edit cannot rename a symbol**, because a body is not an input to one. That is what
makes a future native cache able to distinguish "B changed" from "what A calls changed".

### `pub` chooses linkage, never spelling

Source-level `pub` says which **Nazm modules** may name a definition. It is not an ELF
export, not a C ABI, not a shared-library symbol and not a stability promise, and N5 must
not quietly turn it into one.

| | Linkage | Why |
| --- | --- | --- |
| a private definition | `internal` | nothing outside its module can name it, so nothing outside its object can refer to it — and LLVM may still inline, specialise and delete it |
| an exported definition | `hidden` | the linker must bind it; the finished image exports it anyway never |
| the root module's `main` | `hidden` | the entry wrapper is a separate artefact. The one place linkage does not follow visibility, and it is about where a process starts |

The symbol string is the same either way. `cargo xtask check`'s **native identity** gate
fails if visibility reaches the naming, if a session-local `FnId` reaches a symbol, if a
positional symbol returns, or if LLVM spelling reaches `nazm-sema`.

### What stays whole-program, and why that is not a defect

Two analyses run once over the whole compilation, before any module is lowered:

* **signature admissibility** — whether this backend can represent a function's types. A
  caller in another module has to know before it can emit the call;
* **the recursion refusal** — `A::f → B::g → A::f` is a cycle no single module can see.
  Per-module code generation is not a reason to stop looking for one, and weakening the
  policy to make lowering local would have traded a language rule for an implementation
  convenience.

Import cycles are unaffected and remain legal: a linker has no opinion about the order
objects were produced in, which is exactly why separate generation needs no topological
order. An import cycle and a call cycle are different things and only the second is refused.

### Runtime ownership

Every definition that must exist **exactly once per program** lives in one artefact: the
thread-local failure state, `@nz.fail`, `@nz.report`, and each helper set the program
reaches. Which sets those are is the union of what the modules reached — the one
whole-program fact emission needs, and the thing a future runtime-artefact cache would key
on.

The platform entry is a third kind of artefact. Nazm's `main` is an ordinary definition
with an ordinary symbol; `@main` is a C entry point with a C signature, in its own file.
Keeping them apart is what makes the separation between language identity and operating-
system identity visible rather than implied — and it is what stops an imported module's
`main` from ever becoming a process entry.

Constants needed no qualification at all. `private` linkage makes them local labels rather
than symbol-table entries, so two units may both number theirs `@.s0` with no collision;
that was established by linking two such objects rather than reasoned about. Declarations
go into **every** unit unconditionally: a `declare` costs nothing, emits no reference when
unused, and deciding per unit which are needed is a class of bug for no benefit.

### What an object actually depends on — measured

The question a native cache will have to answer, settled here with evidence rather than
carried forward as an assumption. `a.nz`'s artefact says exactly two things about `b.nz`:

```text
declare i64 @nz.m.b_2Enz.twice(i64)
%1 = call i64 @nz.m.b_2Enz.twice(i64 %0)
```

| B changes | A's object |
| --- | --- |
| a body, exported or private | **byte-identical** |
| a private definition added or removed | **byte-identical** |
| an export A does not call, added | **byte-identical** |
| the signature of a definition A calls | changes — it is written into A's declaration |
| a rename, which is a new `DefKey` | changes |

So A's object depends on B's **symbol identity and ABI signature** and on nothing else,
because there is no cross-module optimisation: each unit is assembled with no other unit's
IR in scope. N4's report assumed a dependent object would need its dependency's
implementation; on this architecture it does not, and a future `CodegenKey` for A may
therefore use B's interface rather than B's bodies.

**One thing does depend on bodies**: the recursion refusal, which is whole-program. A body
edit cannot change any object's contents and can change whether the program is admissible
at all.

### What N5 is not

Not a cache. Every module is regenerated on every build; there is no `CodegenKey`, no object
store and no reuse. `capability-matrix.md` says so, and the clean-build cost is measured in
`performance.md` rather than presented as a speed improvement — it is roughly twice as slow
for a small program, almost entirely because there are now several `clang` processes where
there was one.

> **Superseded in part by N6, 2026-09-22.** This section says "a future native cache" in
> three places; it exists, and §7.6 is it. The last paragraph above describes what was true
> only between N5 and N6.
>
> The measured dependency table is what N6 was built on and is unchanged. One sentence in
> it was written as a prediction and is worth correcting: a key for A's object does **not**
> use B's interface. It uses the bytes of A's own emitted unit, which already contain the
> part of B's interface that A depends on and nothing else — so the key needed no semantic
> input at all, which is less than this section expected.

## 7.6 Native object reuse — settled by N6, 2026-09-22

### The claim, stated narrowly

> If the exact LLVM text a unit would be compiled from, the configuration the object
> compiler resolves for the run, and the object compiler itself are all the ones a previous
> build recorded, the recorded object is linked instead of being compiled again.

Per-unit object reuse exists. **Incremental native compilation does not**, and the
difference is worth stating in the same breath: lowering and emission run on every build,
the final link runs on every build, there is no executable cache, no link-result cache and
no incremental linker. What is skipped is `clang -c`, which N5 measured as the expensive
part.

### Keyed on the backend's input, not on a model of the front end

The obvious design is a second dependency graph: this module's source fingerprint, each
dependency's exported signatures, an emitter epoch, the target. Every one of those is an
*approximation* of what the object compiler reads, and every approximation has the same
failure — something reaches the object that nobody remembered to add.

The emitted IR is not an approximation. It **is** the input. So

```text
ObjectKey = BLAKE3(
    "nazm.object-key/1"
  ‖ LlvmInput       = BLAKE3("nazm.llvm-input/1"      ‖ the exact bytes written for clang)
  ‖ BackendIdentity = BLAKE3("nazm.backend-tool/1"    ‖ driver ‖ its own version report)
  ‖ CodegenConfig   = BLAKE3("nazm.codegen-config/1"  ‖ the arguments the driver resolved)
)
```

with the same length-prefixed framing every digest in `nazm-cache` uses, so no field
boundary can move. N5's measured table then falls out rather than being encoded:

| Edit | The caller's emitted unit | Its object |
| --- | --- | --- |
| a dependency's body | unchanged | **reused** |
| an export it never calls | unchanged | **reused** |
| the signature of something it calls | the `declare` moves | recompiled |
| a comment that moves no emitted position | unchanged | **reused** |
| a comment above a fallible operation | the failure message's position moves | recompiled |
| the emitter, emitting the same text | unchanged | **reused** |

None of those rows is a rule anywhere in the compiler.

### What is deliberately not in the key

| Left out | Why it cannot affect the object |
| --- | --- |
| `ModuleKey` | two units with identical IR are identical inputs. Module identity already reaches the IR as a native symbol; adding it again would only refuse valid reuse |
| a dependency's `InterfaceHash` | the part of an interface an object depends on is in its own `declare` lines. Including the whole thing would invalidate a caller when an export it never mentions changes — over-invalidation, and a mutation tests for it |
| the module's source fingerprint | an edit that does not reach the IR does not reach the object |
| the input path, the output path, the working directory | measured: identical object bytes under three spellings of the first two and from two directories for the third |
| the linker, its version, its options | a linker does not produce an object. That belongs to a `LinkKey`, which does not exist |

### Asking the compiler what it is going to do

`nazm build` passes three arguments. The driver resolves about two hundred, and almost none
of the ones that decide the object are among the three: the triple with its deployment
version, `-target-cpu` and thirty `-target-feature` flags, the relocation model, the ABI,
the resource directory. A configuration built from what was *asked for* would leave every
one of them out, so two machines with one clang version and different processors would
share entries they must not.

So one `clang -###` per build reads what the compiler says it will actually run — the
version banner and the resolved command line — and only the arguments naming **this
invocation's own files** are removed from it: the job's directory, `-main-file-name`, `-o`,
the input and output paths, and the debug and coverage compilation directories. Each was
measured not to change a byte of the object. Everything else is kept, including arguments
this compiler has never heard of, because an argument wrongly kept costs a miss and an
argument wrongly removed is a wrong hit.

**The identity is the report, not the executable.** Hashing the driver's own bytes was
considered and rejected with numbers: `/usr/bin/clang` on this machine is a 200 KB shim
with 78 hard links in front of a 124 MB executable, so it would identify the wrong file,
and reading and digesting the right one costs about 200 ms — more than the entire 158 ms
warm build it would be protecting. The stated limit is therefore that two builds of a
compiler reporting one version and behaving differently are indistinguishable, and that is
when `.nazm/native` should be deleted.

### A controlled environment rather than a hashed one

Measured on 2026-09-22:

| Variable | Effect on the object of a pure LLVM input |
| --- | --- |
| `CCC_OVERRIDE_OPTIONS=+-O2` | an `-O0` invocation produced the `-O2` object, byte for byte |
| `SDKROOT` | moved `LC_BUILD_VERSION`'s `minos` from 27.0 to 27.2 |
| `MACOSX_DEPLOYMENT_TARGET` | moved it to whatever was asked for |
| the working directory | none |
| the input and output paths | none |

Hashing the environment would answer the first three with a set nobody can finish
enumerating and a key that moves when an unrelated variable does. The invocation is made
hermetic instead: the child's environment is built from nothing and ten named variables are
put into it — `PATH` and `LD_LIBRARY_PATH` so the driver can be found and started,
`TMPDIR` for a crash report, and `DEVELOPER_DIR`, `SDKROOT` and the deployment targets
because a platform build legitimately needs them. Everything passed through is still
covered, because the probe runs in the same environment as the compilations it describes:
a deployment target arrives in the key as `-triple arm64-apple-macosx12.0.0`.

### A generated artefact names a module by its durable identity

A preflight correction, and the thing that makes an object reusable between checkouts at
all. Two strings used to reach a native artefact as the path the command line spelled: the
`source_filename` and the location inside every runtime failure message. So

```text
nazm build app.nz              lib/util.nz:3:5
nazm build /tmp/a/app.nz       /tmp/a/lib/util.nz:3:5
```

were two programs, byte for byte, differing in nothing a reader of the source could see.
They are named by the module's `ModuleKey` now — the same name the symbols carry and the
same name a check entry is filed under. A module with no durable identity keeps the name
the loader gave it, consistently with everything else about such a module.

### The store

`.nazm/` became a derived-data root with one subdirectory per kind: `check/` for
`nazm.check/1`, `native/` for `nazm.object/1`. `NAZM_CACHE_DIR` moves the root rather than
one store.

An entry is **one file** — a line of JSON, a newline, then the object's exact bytes — so
there is no window in which a reader can see half of a two-file entry as complete. Six
things must hold before it is a hit: the header parses, its schema is known, every digest
is in canonical form, the inputs it records reproduce the key it is filed under, that key is
the one asked for, and the object's length and digest are the ones it claims. A file name is
how an entry is found and never why it is believed. Every failure is a miss.

Entries are immutable because the key covers every input, so two compilers racing on one key
both write and one rename lands second, with identical bytes either way. No lock, no index,
no sweep.

### Why a warm store cannot skip anything it must not

A key is a digest of an artefact. So the artefact must exist before the key does, which
means everything required to produce it has already run: the files were read, parsed and
checked; global backend admissibility ran; lowering ran; emission ran. The whole-program
recursion refusal is one of those, and a fully warm store is never reached before it.

That is not a rule this code follows. It is the shape of the pipeline, and it is why N6
needed no "run the analyses first" guard:

```text
checked compilation → admissibility → lower → emit → hash → look up → compile misses → link
```

### What still runs on every build

Reading, parsing, checking, admissibility, lowering, emission, fingerprinting, the store
lookups, one `clang -###` and one link. `performance.md` measures which of those matter: on
the five-module compiler a warm build is 157.6 ms against 665.4 ms with the cache off, and
what remains is dominated by the two external processes — the probe and the link — plus
about 36 ms of `nazm`'s own startup.

### What N6 is not

No executable cache, no `LinkKey`, no incremental linker, no persistent LIR or LLVM, no
remote or distributed cache, no LTO. The link runs every time and the report says so in a
field that is always `true`, because a report that omitted it would read as though something
had been saved there.

## 7.7 Memory: what values mean, and when storage goes — settled by N7, 2026-09-22

`docs/spec.md`'s *memory constitution* is the law. This is the evidence it was derived
from, the implementation, and the parts that are deliberately not done.

> **Two of those parts were done by N8, 2026-09-22.** *"`Str` and `Chan` storage is not
> reclaimed"* was true when this section was written and the paragraphs below give the
> reason it was. §7.8 is the answer to that reason and §7.9 is the shape it left the
> backend in; everything else here still stands, including the audit, the counts and the
> argument for a non-atomic count on a sequence.

### The audit came first, and it moved the design twice

Read out of the two implementations rather than out of the documentation:

| | Interpreter | Native | Aliasing observable | Crosses a task |
| --- | --- | --- | --- | --- |
| `Int`, `Bool` | `i64`, `bool` | `i64`, `i1` | — | yes |
| `Str` | `Vec<u8>`, **deep-copied on every read** | `{ ptr, i64 }`, **bytes shared** | no — it is immutable | yes |
| `Ints`, `Strs` | `Arc<Mutex<Vec<…>>>` | pointer to a header | **yes** | no, `N0321` |
| `Chan` | `Arc<Channel>` | pointer to a runtime header | yes | yes, by design |

The second row is the whole argument for the constitution's `Str` rule. The two
implementations have *always* disagreed about whether a `Str` is copied, and no test has
ever caught them, because the specification says the difference is unobservable. That is
not a bug to fix; it is the licence a shared-immutable value gives an implementation, and
it was already being used.

Two findings changed what the milestone could do:

**A `Str` has four provenances and does not say which.** Its bytes may live in a `private`
constant (a literal), in the process's `argv`, in a `malloc`'d buffer — or *in the middle
of any of those*, because `str_slice` is specified not to allocate and is emitted as a
`getelementptr`. `free(s.ptr)` is therefore wrong for three of the four. Reclaiming `Str`
needs a representation that records its provenance, which is a change to the hottest type
in the language and is not this milestone.

**The specification's `Str` value semantics are currently paid for with the leak.** A
`Str` read out of a `Strs` keeps its bytes after the sequence is overwritten — measured,
both implementations, `63` — and natively that is true only because nothing is ever freed.
Any scheme that reclaims `Str` has to buy that guarantee some other way.

### What the real workload does, counted

`compiler/*.nz`, 5,272 lines, 197 functions:

| | |
| --- | --- |
| parameters | `Ints` 108, `Str` 68, `Int` 68, `Strs` 60 |
| returns | `Int` 153, `Str` 28, `Bool` 15, **`Strs` 1**, `Ints` 0 |
| sequences created | 176 |
| **`let b = a;` aliasing a sequence** | **0** |
| sequence mutations | 746 — 588 through the binding that created it, 49 through a parameter |
| `scope`, `spawn`, `chan_*` actually used | **none**; every occurrence is a comment or a string in the code that compiles them |

A compiler that passes sequences in constantly, returns one exactly once, and aliases none
is the shape *borrowed parameters, transfer on return* was made for. It is also why the
reference counting below costs it nothing: the traffic is at bindings, and it has none.

### The leak, before

| Workload | Live set | Peak RSS |
| --- | --- | ---: |
| 5,000 short-lived sequences | ~400 B | 4.38 MiB |
| 20,000 | ~400 B | 12.28 MiB |
| 80,000 | ~400 B | 43.52 MiB |

Linear in **work done**, constant in **live data**. Eleven allocation sites, **zero**
deallocation sites: `@free` appeared nowhere in the emitted runtime.

### The models that were considered, and why this one

| | Why not |
| --- | --- |
| Unique ownership with explicit moves | 168 sequence parameters would each need a move or a borrow concept at the surface; the borrow is the common case, so this is the model below with ceremony added |
| Copy-on-write value semantics | `let b = a; ints_push(a, 1)` is specified to be visible through `b`, and measured to be. Changing it is a language break, and §4 of this milestone forbade doing that to make reclamation easier |
| Regions or arenas | The compiler already *is* one arena freed at exit — which is exactly why its peak is what it is. It answers nothing for a program that does not exit |
| A tracing collector | Excluded at constitution level, and unnecessary: see cycles |
| Reference counting on the handle | **Selected.** It matches the semantics the language already has, needs no annotation, and costs nothing where nothing is aliased |

### The implementation

The header grows from 24 bytes to 32: `{ i64 len, i64 cap, ptr data, i64 rc }`. Only
`malloc`'s size changed; every existing offset is where it was.

```text
@nz.arr_new      allocate, rc = 1          @nz.seq_new      that, and count it
@nz.arr_retain   rc += 1
@nz.arr_release  rc -= 1, free at zero     @nz.seq_release  that, and count the free
```

**Not an atomic**, and that is a consequence rather than an optimisation: `N0321` refuses
to let a sequence cross into a task, so no two threads can hold one handle. The comment in
the runtime says what would have to change in the same commit if that rule were relaxed.

The emitter tracks, per function, which SSA values hold a reference nobody has taken over.
`ints_new` and a call that returned a handle create one; reading a variable does not — that
is rule 2, and it is why passing a sequence emits no instruction at all. A binding takes
one over, a `return` carries one out, a discard releases one.

**Cleanup is on the exit edges, not at the closing brace.** There are exactly three ways
out of a generated function — the tail value, each `return`, and the shared `nz.unwind`
block every failure branches to — and all three release the frame. Handle slots are
initialised to null in `entry`, and releasing null is a no-op, so a path that never reached
a declaration needs no special case. Parameter slots are excluded, which *is* rule 2:
releasing a borrowed parameter would destroy storage the caller still holds.

A `phi` merging two branches that arrived at ownership differently —
`if c { ints_new() } else { xs }` — is normalised inside each branch, before its jump,
because after the `phi` there is no branch left to attribute a retain to.

The scope machinery's list of task ids uses the same allocator and is now released when
the scope is joined, on every way out. It is deliberately **not** counted: it is not a
value any program can name, and a report about a program's sequences that included one
would be answering a different question.

### The interpreter models the rule rather than inheriting it

`Arc` already reclaims at the right moment, and *"the host does the right thing"* is not
evidence. So sequence storage is wrapped in a type whose construction and `Drop` bump the
same two counters a compiled program reports, and every case in
`crates/nazm-cli/tests/memory.rs` asserts the two implementations produce the same value
**and** the same three numbers.

### Measured

| | Before | After |
| --- | ---: | ---: |
| 5,000 short-lived sequences | 4.38 MiB | **1.78 MiB** |
| 20,000 | 12.28 MiB | **1.88 MiB** |
| 80,000 | 43.52 MiB | **1.86 MiB** |
| one 2,000,000-element sequence, genuinely live | 19.08 MiB | 19.08 MiB |
| string-heavy loop, no sequences | 23.77 MiB | 23.78 MiB |
| the self-hosted compiler on `emit.nz` | 46.81 MiB | 42.59 MiB |

Flat in work where it was linear, and unchanged where the memory is live or is a `Str` —
which is what a correct reclamation of exactly one kind of value should look like.

### What this does not do

- **`Str` and `Chan` storage is not reclaimed** — *until N8, which is §7.8*. The reasons
  are above and were good ones; the answer to them is a wider value, not a cleverer guess.
- **`exit_with` reclaims nothing and reports nothing.** Found by measuring: the
  self-hosted compiler ends that way, so the entry wrapper is never reached. It is correct
  — a process that has ended returned everything — and there is a test so that nobody
  later spends real time on the largest workload here "fixing" it.
- **A failure still leaks the temporary it was holding.** A `Val::Transferred` on a guard's
  failing path cannot be followed by a release, and that path ends the program. Stated
  rather than papered over.
- No destructors, no RAII, no user-visible ordering.

## 7.8 What owns a string's bytes — settled by N8, 2026-09-22

§7.7 stopped at `Ints` and `Strs` and named the obstacle exactly: **a `Str` has four
provenances and does not say which.** Its bytes may live in a `private` constant, in the
process's `argv`, in a `malloc`'d buffer — or *in the middle of any of those*, because
`str_slice` is specified not to allocate and is emitted as a `getelementptr`. Given
`{ pointer, length }`, `free(s.ptr)` is wrong in three cases out of four and there is no
sound way to tell them apart.

Every way of telling them apart *from the pointer* is a heuristic — a range test, a length
test, a rule about which built-in produced it — and every one of them is wrong on a
program somebody will write. So the value says.

### The representation

A `Str` is `{ pointer, length, owner }`, 24 bytes.

| Field | What it is |
| --- | --- |
| `pointer` | the first byte of this **view**. Valid to read `length` bytes from |
| `length` | bytes, not characters. An embedded zero is data; nothing here is NUL-terminated |
| `owner` | the **allocation whose lifetime keeps `pointer` valid**, or null when nothing has to |

`owner` is not language-visible. No Nazm expression can read it, `docs/spec.md` still
specifies a `Str` as its bytes, and the type `nazm capabilities` reports is unchanged.

**Every** expression that produces a `Str`, and what it owns. The four origins become
three answers and one of them is free:

| Producer | Class | `owner` | What the expression holds |
| --- | --- | --- | --- |
| a literal | immortal | `null` | nothing — a retain is one comparison |
| `arg(i)` | external | `null` | nothing — the bytes are the operating system's |
| `str_concat`, `str_join`, `int_to_str`, `str_from_byte`, `read_file` | owned heap | the backing that call allocated | **one reference**, born with the allocation |
| `str_slice` | view | **the same owner as its source** | **one reference**, taken inside the built-in — see below |
| `strs_get` | view | the stored element's owner | **one reference**, taken inside the built-in |
| `strs_pop` | view | the stored element's owner | **one reference**, *transferred* — the sequence gives up its own, so nothing is retained and nothing released |

`str_slice` and `strs_get` take their reference **inside** the built-in rather than leaving
it to whoever binds the result, and the reason is the order. A built-in's arguments are
released as temporaries the moment it returns, so `str_slice(str_concat(a, b), 1, 2)` and
`strs_get(one(), 0)` would otherwise free the storage their own results point into before
anything could claim it.

Null being a valid owner is what makes *immortal* and *external* cost nothing and need no
tag: they are simply strings that own nothing, and `@nz.str_retain(null)` is a branch.

A backing is `{ i64 rc, i64 length, bytes… }` and the view's `pointer` is `owner + 16` or
anywhere after it. **That separation is the whole idea**: the bytes a value names and the
allocation that protects them are different addresses, so a slice moves one and keeps the
other. `make()` below returns a value pointing into the middle of a buffer whose only
binding died on the way out, and it is ordinary:

```nazm
fn make() -> Str {
    let s = str_concat("ab", "cd");
    str_slice(s, 1, 3)
}
```

### Why this count is atomic and a sequence's is not

Neither is a judgement about how likely sharing is. Each follows from a rule the language
already had.

`N0321` refuses to let a sequence cross into a task, so no two threads can hold one handle,
so no two threads can touch one count: an ordinary `add`. A `Str` **may** cross, by
immutable sharing, and a task can slice what it was handed and bind the slice — so two
sibling tasks really do adjust one backing's count. Atomic, therefore, with the decrement
released and an acquiring fence before the free, which is the standard pairing.

The same argument makes a channel's count atomic: a `Chan` exists to cross.

A consequence worth stating: if `N0321` were relaxed, the sequence count would have to
become atomic **in the same commit**, and `crates/nazm-lir/src/emit.rs` says so where the
count is written.

### Containers own what they hold

A `Strs` element is a 24-byte descriptor, and the sequence holds a reference to each
element's backing. That is four rules, and the order in two of them is the correctness:

| | |
| --- | --- |
| `strs_push(xs, s)` | takes a reference of its own. The caller's `s` is untouched and still valid |
| `strs_get(xs, i)` | hands back a reference of its own. This is what makes *"a `Str` already read out of a sequence is unaffected by changing the sequence"* — promised since sequences existed — true by construction rather than true because nothing was freed |
| `strs_set(xs, i, s)` | takes the new reference **before** releasing the old, so replacing an element with a view of the same backing cannot drop the count to zero in between |
| a `Strs` dying | releases every element's backing, **then** the array, then the header. Freeing only the descriptors would turn a leaked string into a leaked string inside a sequence |
| `strs_pop(xs)` | **transfers**: the sequence gives up its reference and the expression takes that same one over. No retain, no release |

### Channels: what permits destruction

A channel header carries a count like everything else, and at zero its condition variable
and mutex are destroyed — `pthread_cond_destroy`, `pthread_mutex_destroy`, which release
what the platform allocated behind an opaque handle — and then its ring and header are
freed.

**Closing is not destroying.** `chan_close` changes what the channel does and touches no
storage; a closed channel with live references still answers, and still drains. The two
events are kept apart on purpose.

The safety argument is structural rather than hopeful. A reference is a binding holding the
handle, a value on its way out of a `return`, or the argument block of a task that has not
been joined. A thread blocked inside `chan_send` or `chan_recv` **reached it from a frame
that holds the handle** — a built-in borrows its argument and the caller keeps its reference
across the call — so a waiter implies a live reference, and therefore no reference implies
no waiter. That is why `rc == 0` is a sufficient condition and no second one is needed.

**The destroy results are not checked**, and that is a decision rather than an oversight.
`pthread_mutex_destroy` returns `EBUSY` for a mutex that is still locked or waited on, which
is exactly the state the argument above rules out — so a branch on it would be a path no
test can reach and no diagnostic can name, in a helper that has no source position to report
at. If the argument were ever wrong, refusing to free would convert a use-after-free into a
leak, which is the safer failure; the reason not to write it is that an unreachable
recovery path is itself a place for a defect to live.

### What crosses into a task, and who holds it

A `spawn` argument is **held for the task**, not borrowed from the statement that started
it: the reference is taken at the spawn site, on the thread that already has one, and given
back by the task's trampoline when the call returns — on both the succeeding and the failing
path, because they meet. Without that,

```nazm
scope { spawn worker(s); s = other(); }
```

releases the bytes the task is still reading, and the window is not theoretical: the scope's
body runs concurrently with the task by construction.

### Four leaks that were nobody's feature

Found by looking for `malloc` with no matching `free` rather than by a test failing.

| | |
| --- | --- |
| a `spawn`'s argument block | one `malloc` per task, never freed. Freed at the join, after `pthread_join` and after its diagnostic has been read out |
| `@nz.cstr`'s terminated path copy | one per file operation, never freed. Freed once `fopen` has returned |
| a partly-built channel | four allocations, and a failure after the second left the first behind. One label per stage now, each freeing exactly what has been taken |
| a task that never started | its arguments and its block, on the one path where the program is already out of resources |

### Two defects in N7's own instrument

**The counters were not atomic.** `@nz.seq_allocated` was an ordinary load-add-store, and a
*task* can create a sequence — `N0321` stops a handle crossing, not a task allocating — so a
concurrent program could under-report what it had allocated. A counter that is wrong in the
direction of "less was allocated" is the worst direction for a leak check.

**The report was emitted only into programs that used a sequence.** A string-heavy program
printed nothing at all, which reads as *nothing to say* and meant *the instrument was not
built in*. Six counters and the report are unconditional now: six words of data, and every
program can be asked.

## 7.9 One classification, and where cleanup goes — settled by N8, 2026-09-22

N7 emitted cleanup for two types by testing `Ty::is_handle()` at each site. N8 makes four
types reclaimable, and repeating that test would have put `if type == Ints { … }` in seven
places across three files.

`Ty::owns()` answers, once, what a value of a type owns: nothing, a sequence of `Int`, a
sequence of `Str`, a string backing, or a channel. Every retain, every release, every frame
slot, every container and every task boundary dispatches on that one answer. Adding a type
to the language means answering it and nothing else.

It is deliberately **not** a destructor system. Nazm has no user-defined destruction, no
ordering a program can observe, and nothing a program can write runs at reclamation. Each
variant names a runtime helper that already exists; none of them is a hook.

Cleanup is on the **exit edges** of a generated function, as in N7 and for the same reason:
the tail value, each `Stmt::Return`, and the shared `nz.unwind` block a failure takes.
Parameter slots are excluded, and that exclusion *is* rule 2 — a callee borrows, so
releasing a parameter would destroy storage its caller still holds. Every owning slot is
zeroed at entry, and a zeroed slot releases to nothing, so a path that never reached a
declaration costs one comparison.

### The two compilers, held together by derivation

`compiler/emit.nz` emits the same model. It did not, for a milestone: N7 taught the Rust
emitter to reclaim and the Nazm-written one went on emitting a header with no reference
count at all, and every behavioural test passed throughout — **a program that never frees
computes the same answers as one that does.**

Two hand-kept transcriptions of a memory model is the arrangement that produced that, so
there is now one. Every line of the Nazm emitter's `runtime`, `task_runtime` and
`channel_runtime` is a line of `crates/nazm-runtime/src/core.rs`'s runtime constants (in `emit.rs` until N43, in `nazm-lir` until N53) with
`hidden` changed to `internal`, and `cargo xtask check`'s **`runtime parity`** gate performs
the derivation again and refuses a difference by line number.

What a textual gate cannot check is where each emitter *decides* to retain and release.
`the_nazm_written_compiler_emits_the_same_memory_semantics` is for that: eight programs
built by both compilers, run under `NAZM_MEMORY_REPORT`, required to print the same line —
and then required to print `live=0` in all three classes, because a report that matched
because both leaked would pass the first comparison and mean nothing.

### Where a binding hides a missing reference

One mutation survived the first run, and what it found is worth keeping. Removing
`strs_get`'s own reference to the string it hands back broke nothing: `let s = strs_get(xs,
0);` retains what it stores whether or not the built-in already did, and so does a `return`,
and so does storing it back into a sequence. Every shape in the suite had a binding standing
in for the missing reference.

The shape that has none is a sequence with **no binding** — `strs_get(one(), 0)`, where the
sequence is the result of a call. A built-in's arguments are released the moment it returns,
so the order decides it: with the reference taken inside the built-in, the retain precedes
the release; without it, the sequence and every string in it are freed first and the
binding's retain then touches storage that is gone.

The general lesson, and it applies to every rule in this section: **a compensating
mechanism makes a defect invisible without making it absent.** Reference counting is full of
them, because almost every value passes through a binding eventually. The test that finds
one has to be the shape where nothing else is holding the value.

### What this still does not do

- **Nothing about future types.** Leak freedom here is a claim about six built-in types —
  and, since N9, about records composed of them (§7.10) — and rests on the cycle argument
  in `docs/spec.md`: `Ints` holds `Int`, `Strs` holds `Str`,
  `Str` is a leaf, and a channel holds `Int`, so no value can refer to one that refers back.
  The first recursive user-defined type ends that, and the constitution says in advance what
  admitting one requires.
- **`exit_with` reclaims nothing and reports nothing.** Unchanged from §7.7, and correct: a
  process that has ended returned everything.
- **A failure still leaks the temporary it was holding** — *closed by N9; see §7.10*. A
  `Val::Transferred` on a guard's failing path still cannot be followed by a release, and
  that path ends the program. What N9 added is the release of everything the expression
  *was* holding when the failure left, which is the larger half of what this bullet meant.
- **No sanitizer result is claimed.** The language-level counters are the evidence here.

## 7.10 Records: composition instead of cases — settled by N9, 2026-09-23

N9 adds the first user-defined type. The question it exists to answer is not whether
records compile — it is whether the memory constitution N7 and N8 built can describe a type
whose ownership behaviour is **composed from arbitrary field types** rather than written
out in compiler branches.

The answer is in what did *not* change. `Owns` gained one variant, `Owns::Record`, and no
consumer gained a case: every retain, every release, every frame slot, every container and
every task boundary still dispatches on the one answer `Ty::owns()` gives. The four
properties a record has are all derived:

| | Derived as |
| --- | --- |
| needs cleanup | some field does, recursively |
| may cross into a task | every field may, recursively — and the failing path is reported |
| copy | retain each owning field, by that field's rule |
| destroy | release each owning field, by that field's rule |

A record adds no new **kind** of value. It is a composition of the four that existed, which
is also why `spec.md`'s cycle argument survives: a composition of leaves is a leaf, and a
containment cycle has no finite layout and is refused (`N0336`) before anything is lowered.

### Semantic identity, and where a field's position is decided

A record's durable identity is a `DefKey` with `DefKind::Record` — the second kind that
enum has ever had, and no key written before N9 changed meaning, which is what N3 designed
it for. `Type::Record(RecordId)` carries a session-local index and **not** a field list:
two records with identical fields are different types, so a type that held its shape would
compare equal where the language says unequal.

§4 has required since before there was a backend that *field access remains symbolic until
the low-level layout boundary*. N9 is the first feature where that costs anything, and the
boundary is one function:

```text
checker          FieldRef { record, field }      a field's identity, as a declaration index
  ↓
nazm-lir::lower  RecordLayout::of_declaration    the only place a position is decided
  ↓
emitter          extractvalue %rec, <position>
```

Nothing above `lower` knows a position, and nothing anywhere knows a byte offset — the LLVM
struct is named and its offsets are the target's business. A future AoS/SoA pass changes
`record_layouts` and nothing else.

### Why the layout is canonical by field name

`spec.md` makes declaration order non-semantic: construction is named, and moving two field
declarations is not a change to the type. Laying fields out in **name order** makes that
true of the object file as well.

The alternative — declaration order — would have produced an ABI difference from an edit
the compiler itself calls meaningless: the fingerprint would not move, the semantic cache
would reuse, and the object would differ. A compiler and a linker disagreeing about whether
anything happened is the worst kind of disagreement, because each is right.

### A projection borrows when its base does

Reading a field out of a record held in a **slot** takes no reference. The slot keeps the
field's storage alive, nothing in one expression can change it — assignment is a statement
— and rule 2 says a borrowed reference costs nothing. Reading one out of an owned
**temporary** must take a reference first, before the temporary is dropped; that ordering is
the whole of why `make().name` is valid.

Retaining unconditionally was the first implementation and it was wrong in a way only a
failure could show: `ints_get(h.values, 0)` took a reference, the built-in failed, and the
release that would have balanced it was on the path the program did not take.

### A failure now releases what it was holding

§7.9 listed, under *what this still does not do*: **"A failure still leaks the temporary it
was holding."** N8 knew about it and left it; N9 closes it, because a record projection made
the shape ordinary enough to test.

The shared `nz.unwind` block releases the frame's **slots**, which is right for everything a
binding holds and says nothing about a value in a register.
`str_concat(str_concat(a, b), int_to_str(1 / z))` has the inner result in hand when the
division fails, and the call that would have consumed it is on a path control never takes.
Both emitters now release the outstanding set at each failure site — the only place that
knows it, because which temporaries are live is a property of the program point rather than
of the block they all eventually reach.

The first attempt was unsound and `clang` said so: *Instruction does not dominate all uses*.
A value created in a `then` block does not dominate the `else` block, so the set is scoped
to the branch that created it. The refusal came from an assembler rather than from a wrong
program, which is the failure mode this backend is built to have.

### A construction is assembled in a slot when it owns anything

`Holder(name: int_to_str(1), n: 1 / z)` allocates a string and then fails. A value held only
in a register is not reachable from the frame's cleanup, so a construction with an owning
field is assembled into a hidden local instead. The "which fields are initialised"
bookkeeping is the **zeroed slot itself**: an unwritten field is null, releasing null is a
no-op in every release helper, and there is no separate flag to keep in agreement with the
value. A record whose fields own nothing gets no slot and pays for none of it.

### One helper per record per direction

A record released in forty places emits forty calls, not forty copies of its field walk —
and a nested record's helper calls the inner record's, which puts the recursion in the
**emitted program**, where there is a stack, rather than in the compiler. The self-hosted
emitter has no recursion at all, so this is not a convenience there; it is the mechanism.

### A premature free is not a counting error

One mutation survived the first run: releasing the old field **before** taking the new one.
Both orders end with the same two numbers, so the reclamation counters — the instrument
every other rule in §7.7–§7.9 is checked with — cannot see it. The existing
same-allocation case used `str_slice`, which takes its own reference inside the built-in
and so survived the wrong order by accident.

The shape that observes it is `h.name = h.name` — a projection out of a borrowed record,
which holds no reference of its own — followed by three hundred small allocations, so the
block the wrong order freed is handed back out. The program then stops with a signal rather
than returning a wrong answer.

This is §7.9's lesson in a form that section did not cover. There, the compensating
mechanism was a binding. Here it was an allocator that had not yet reused the block, and
the general rule is the same: **the test that finds a defect has to be the shape where
nothing is compensating for it.**

### The self-hosted compiler

`compiler/lex.nz`, `parse.nz`, `analyse.nz` and `emit.nz` all learned records: the keyword
and `.`, declarations and named construction and projection-path assignment, the record
table and its three declaration stages, and named LLVM aggregates with generated helpers.
There is **no divergence to record** — the reference and the Nazm-written compiler accept
and reject the same record language, and `compiler/conformance/records.nz` plus
`modules/exported-record` are checked by C2, C3 and the reference.

Two differences are inherited rather than introduced. The self-hosted emitter names a record
`%nz.t<index>` exactly as it names a function `@nz.f<index>`, which is sound because it
emits one module for a whole program and is the same limitation N5 removed for the reference
compiler and not for this one. And its `owns` answer for a record is a precomputed bit per
record rather than a walk, because a Nazm function cannot recurse.

It also **uses** one. `Place { line, column }` in `compiler/emit.nz` replaced `line_of` and
`column_of`, which were two functions because a function returns one value: the first
scanned forward counting newlines, the second scanned backward looking for one, and every
diagnostic paid for both. A column falls out of the forward scan for nothing. That is the
whole of the dogfood evidence and it is deliberately one record — see
`research-register.md` R1 for why a wider migration would not have been honest.

### What this still does not do

- **No variants, no generics, no traits, no methods, no pattern matching.** N9 is product
  types. Sum types are the next distinct semantic problem — §7.11 is where N10 answered it.
- **No layout promise.** A record's field offsets, size and calling convention are
  compiler-private. `pub` says which Nazm modules may name the type.
- **Nothing about a record inside a sequence.** `Ints` holds `Int` and `Strs` holds `Str`;
  there is no container that can hold a record, which is also why the cycle argument still
  holds. A generic container is what would end it.
- **No sanitizer result is claimed**, and the one defect class the counters cannot see —
  a premature free — is covered by exactly one test shape rather than by an instrument.

## 7.11 Enums: a composition chosen at runtime — settled by N10, 2026-09-23

N9 answered whether ownership can be *composed* from field types. N10 asks the next
question, and it is a different one: can that composition be **selected at runtime**? A
record owns every field it declares. An enum owns the fields of its active variant, and
which variant that is is a value the program carries.

The answer is again in what did not change. `Owns` gained one variant, `Owns::Enum`, and no
consumer gained a case. What changed is where the answer is computed:

| | Record | Enum |
| --- | --- | --- |
| needs cleanup | some field does, recursively | some field of **some** variant does |
| may cross into a task | every field may | every field of **every** variant may |
| copy | retain each owning field | branch on the discriminant, then retain the **active** variant's |
| destroy | release each owning field | branch, then release the active variant's |

The first two rows are static questions about a type and are computed in the checker. The
last two are runtime questions about a value, and they are computed **in the emitted
program** — one generated helper per enum, a `switch`, and one call per site. That is the
whole difference, and it is why adding a sum type added no arm to either compiler's
dispatch.

### An enum adds no new kind of value either

`spec.md`'s cycle argument holds for the same reason it held for records: an enum is a
choice between compositions of existing kinds, and a choice between leaves is a leaf. The
containment graph is now one graph over records and enums together — a record field may be
an enum and a payload may be a record — and a cycle anywhere in it is refused (`N0336`)
before anything is lowered. A walk that knew only about records would have run forever on
`struct A { b: B } enum B { V(a: A) }`, which is why `needs_cleanup`, `task_safety`,
`layout_cycle` and `depth` left `Records` for `UserTypes`. What they share is the graph;
what a record and an enum *do* with their fields is written out at each derivation.

### Semantic identity, and where a discriminant is decided

An enum's durable identity is a `DefKey` with `DefKind::Enum` — the third kind, and the
second time a key written by an earlier build kept its meaning, which is what the field
exists for. A **variant** has no key of its own: `spec.md` makes its durable meaning the
enum's identity and its name, and no persistent consumer has needed a composed one, so none
was invented.

In one session a variant is a `VariantRef { enumeration, variant }` and a payload field a
`PayloadRef { enumeration, variant, field }`, both naming **declaration indices**. Nothing
above the backend knows a discriminant, and nothing anywhere knows a byte offset.
`nazm-lir::lower`'s `enum_layouts` is the only place a tag is decided, from the canonical
variant-name order, exactly as `RecordLayout::of_declaration` is the only place a field
position is decided.

### Why the tag is canonical by variant name

The same argument the field order rests on. `spec.md` makes variant declaration order
non-semantic, so deriving the discriminant from the **names** makes that true of the object
file as well: moving two variant declarations changes no byte. Taking the parser's order
because it was there would have created an ABI difference from an edit with no meaning —
the compiler right that nothing changed, and the linker disagreeing.

### The representation, and what it costs

`{ i64, %V0, %V1, … }`: a discriminant followed by **one slot per variant**, in canonical
order, each slot that variant's payload struct. A value is built from `zeroinitializer` and
the active slot filled, so every byte is defined and copying the aggregate is defined; the
inactive slots hold zeros, which no operation reads.

This is larger than the union it stands for — an enum of three `Str` payloads is
8 + 24 × 3 bytes where an ideal tagged union is 8 + 24 — and `performance.md` measures it.
It was chosen over a sized payload area with typed loads and stores through it for two
reasons. That form needs the compiler to compute each variant's size exactly right, and an
arithmetic mistake there is a buffer overflow rather than a compile error. And this form
needs no memory at all: a variant is built with `insertvalue` and a payload read with
`extractvalue`, entirely in registers. `spec.md` promises nothing about an enum's size or
layout, so the representation can change without the language moving.

### The discriminant is written first, and that was a defect

A variant that owns something is assembled in a hidden local, for the reason a record is: an
initialiser that fails after an earlier one produced a heap string must not leak it.

The first implementation wrote the discriminant **last**, on the reasonable-sounding theory
that a partially built value should not claim a variant. It is the other way round. Cleanup
finds the fields to release by dispatching on the tag, so a tag written last leaves the slot
saying *variant 0* while variant 3's payload is the one being filled — and a failure
releases variant 0's fields, finds null in each, does nothing, and leaks everything the
initialisers produced. On the one path the program was already failing on.

Both emitters now zero the slot, write the tag, and then fill the payload. Cleanup at any
point in between dispatches on a **valid** tag and releases exactly the fields that exist:
the ones already stored, properly, and the ones not yet stored as null. The zeroed payload
*is* the "which fields are initialised" bookkeeping, exactly as it is for a record.

**Writing the emitter a second time is what found it.** The Nazm-written backend was being
written against the same design, and the question "which variant does the frame think this
is" only has to be asked once for the answer to be obviously wrong. `memory.rs` carries the
shape that observes it, with the owning variant deliberately **not** tag 0 — with the
variants the other way round the defect is invisible.

### `match` is `if` with N arms, plus one thing

The block structure is `conditional`'s: a `switch` on the discriminant, one block per arm,
a `phi` at the join, and each arm brought to the same ownership state **inside its own
block and before its jump** — the only place a retain can go, because after the `phi` there
is no arm left to attribute it to.

The one addition is that the value being matched lives in a hidden slot the frame releases.
That is what makes a failure inside any arm give it back by the ordinary path, and what lets
each arm read its payload out of something that dominates it. The release happens **after**
the `phi`, which is the ordering the whole milestone turns on: every arm has already taken
its own reference to whatever it hands to the join, so a payload returned from an arm holds
one by the time the value it came out of is let go.

A pattern binding lowers to an ordinary `Stmt::Let` over an `Expr::Payload`. Not a
convenience: a binding *is* a local that takes a reference to what it names, and `Stmt::Let`
is the one path that already gets take-before-release right when the same slot is reached
again on the next turn of a loop.

### The switch's default

Checking establishes that every variant has exactly one arm, so the default is unreachable
for any value a safe program can build. It is written as a branch to the first arm rather
than as `unreachable`: the block has to exist for the instruction to be well formed, and a
`switch` whose default is poison would turn a compiler mistake into undefined behaviour
instead of a wrong answer. `spec.md` is explicit that no runtime trap stands in for the
checker's coverage.

### The self-hosted compiler

The Nazm-written compiler implements the same language, and its data model is one flag on
the table N9 already had: **an enum is a record whose fields are its variants, and a variant
is a record whose fields are its payload.** `rkind` says which of the three an entry is.

That is not a trick to save an array. Every derivation N9 wrote over records is the one N10
needs over enums — *needs cleanup* is some field of some variant, which is the same walk; a
containment cycle through a variant is the same cycle; and the task-safety path comes out as
`work.Values.items`, naming the variant on the way, because a variant genuinely is a step in
that path. What differs is how a value is built and eliminated, and those are written out at
those two places.

Its parser needed one new decision and one new mark: `IDENT . IDENT (` is a variant rather
than a projection, and a `match` suspends its scrutinee at the `{` exactly as an `if`
suspends its condition, after which each arm's body is an ordinary pending expression — so
an `if` inside an arm needs no machinery of its own. Its emitter carries two bits where it
had one: `rowns` says *2* for an enum and *1* for a record, so every site that already asked
`owns` keeps asking exactly that, and an enum's helper switches on the tag and then calls the
**variant's** helper — which is a record's, because a payload is record-shaped.

Parity is compared on **codes rather than spans**. The self-hosted parser records one span
per arm, so a diagnostic about a payload field inside a pattern points at the arm where the
reference points at the field. Where each compiler points is not what N2.1 is about; what
each decides is.

#### And it uses one

`enum Owning { Nothing, Sequence, Strings, Backing, Channel, Record(at: Int), Enum(at: Int) }`
in `compiler/emit.nz`. It was a small integer — `0` nothing, `3` a string backing, `5` a
record — that every caller compared against a number, and the two cases carrying an index
recomputed it from the type at each site. N10 added a seventh and every one of those
comparisons had to be found by hand.

As an enum the dispatch is exhaustive, so adding a kind stops the retain and release paths
compiling until both have answered; and the index a record or an enum needs travels **in
the value that says it needs one**, so the two cannot disagree. The sites that only want
*does this own anything* go through one `owns_anything`, which is the same `match` answered
once rather than at each of them.

It is one enum rather than a migration, and the reason is the same one N9 gave for one
record: the compiler's node kinds, token kinds and type codes are integers stored in `Ints`,
and there is no container that can hold an enum. `Owning` is the case where the value is
returned and never stored, which is exactly the case a container is not needed for.

#### A record crossing a task boundary, which N9 got wrong

Making that `match` exhaustive found a defect, and it was N9's rather than N10's. The
self-hosted emitter's trampoline released four kinds of value and not a record's. Worse, the
argument block's offsets were arithmetic — twenty-four bytes for a `Str` and eight for
everything else, which was true of every type the language had when it was written. A record
is neither: `struct M { text: Str }` is twenty-four bytes and was given eight, so the store
ran past its slot and the task read the next argument's bytes as its own.

So a record passed to a task produced a **wrong answer** and leaked its fields, in the
self-hosted compiler only, since N9. The block is a named LLVM type now — `{ i64 handle,
ptr message, i64 length, … arguments }`, which is what the reference emitter always had —
so LLVM computes the offsets and the arithmetic that could not survive a new type is gone.
`the_nazm_written_compiler_emits_the_same_memory_semantics` carries the case.

Two things this says. The first is that the reference emitter was right and the second
implementation was the one that drifted, which is the usual direction and the reason the
differential test exists. The second is that N9's evidence did not reach it: records crossed
module boundaries, were returned on both edges and were copied in loops, and **no test
passed one to a task through the self-hosted compiler.** The gap was in the corpus, not in
the design.

### Leaving an expression that never finishes, which was wrong in four places

The surviving mutation of the milestone is the one worth writing down, because what it
found was not the defect it described.

`a-variant-slot-keeps-what-the-last-round-left-in-it` removes the store that zeroes a
variant's assembly slot before the value is built, and the suite still passed. Working out
*why* it passed is what found three real defects — and then showed that the mutation itself
had become inert, because the slot is zero on every path that reaches it.

The real rule is **4a** in `spec.md`, and it was satisfied on one of four paths. A
half-evaluated expression holds things: arguments already produced, fields already stored
into a record's or a variant's slot, and the value a `match` is holding while an arm runs.
There are four ways to leave it part way through, and only the first gave any of them back:

| Leaving by | Register temporaries | The frame's slots |
| --- | --- | --- |
| a failure | released — N9, `release_temporaries` | released — the shared unwind block |
| `return` | **leaked** | released — `release_frame` |
| `break` | **leaked** | **leaked**, because the next turn stores over the slot |
| `continue` | **leaked** | **leaked**, the same way |

Three shapes, all of them expressible since N8 and none of them in the corpus:

```nazm
while i < 3 { i = i + 1;
    let v = pick(int_to_str(i), if i == 1 { continue; } else { i });   // the argument
    let r = R(a: int_to_str(i), b: if i == 1 { continue; } else { i }); // the record
    total = total + match E.Text(value: int_to_str(i)) {                // the scrutinee
        E.Absent() => 0,
        E.Text(value: s) => if i == 1 { continue; } else { str_len(s) },
    };
}
```

Each leaked one string per abandoned round, in **both** backends and in the compiler
written in Nazm, since N8. The interpreter was right in every one of them, which is the
usual direction and why `memory.rs` runs the same program through both.

The fix is the one N9 already wrote for failures, on the other three paths. `transfer`
releases the temporaries this loop produced and the slots it is part way through filling;
`Stmt::Return` releases the temporaries, because the slots are the frame's and it is about
to release those anyway. Two depths make it exact rather than approximate — `Targets`
records how many temporaries were outstanding and how many slots were in flight when the
loop started, the same distinction `scope_depth` already drew for tasks. Releasing more
than that is not a leak but a **premature free**: a string produced before the loop and
read after it is still wanted. That one is invisible to the counters for the same reason
§7.10's survivor was — the premature release frees, and the ordinary release afterwards
finds freed memory and counts nothing, so both numbers come out equal — so
`a_transfer_gives_back_only_what_the_loop_it_leaves_was_holding` reads the string's
**bytes** after thirty allocations have had every chance to reuse them.

The Nazm-written emitter needed the same fix and a smaller version of it: a post-order
stack machine has no half-filled construction slot — the initialisers are complete before
the node is reached — so only the temporaries and the `match` slots are at stake there.

Two things this says, and they are the same two N9's task-block defect said. The first is
that the rule was right and the implementation satisfied it on the path a test happened to
take. The second is that **a mutation that survives is a question, not a verdict**: this
one was answered by three defects and a fourth finding, that the mutation no longer
describes a defect at all, and it was replaced by four that do.

### A transfer in a value position, in the compiler written in Nazm — N10.1

The paragraph above says the Nazm-written emitter needed only a smaller fix. That was true of
what the transfer *gives back* and said nothing about what it *produces*, and the shape the
table above opens with did not compile at all:

```nazm
let v = pick(int_to_str(i), if i == 1 { continue; } else { i });
```

The reference represents completion once, as a type: the checker's `Completion { Value(T),
Unit, Diverges }` and the emitter's `Val { Of { value, ty }, Transferred }`, and nothing
consumes an operand without matching on it. The compiler written in Nazm had neither half.

- **The checker** kept one integer per node, and `unknown` meant three things — a statement,
  an error already reported, and a branch that leaves. A two-branch `if` took its
  then-branch's type, so the `if` above had *no value*. The call's argument list was then
  built from that type, and `llvm_type(unknown)` is `"void"`: `call i64 @nz.f0({ ptr, i64,
  ptr } %19, void %24)`, which clang refuses. Records and variants took their field types
  from the declaration instead, so the same shape there compiled — correctly, by accident,
  because the else-branch's value happened to be left on the stack exactly where the
  constructor looked.
- **The emitter** is a post-order scan, and it opened every join whether or not a branch
  reached it. A construct whose branches all left therefore handed its parent an empty
  stack, and the parent popped a value somebody else had pushed: `N0405` inside the
  compiler, or a real value in the wrong operand.

The repair is one bit per node in each half. `ndiv` in `compiler/analyse.nz` records that a
node **never completes** — the reference's `Diverges`, kept beside the type rather than in
it, because a type code is also what a slot and an operand are given. An `if` or a `match`
is typed by the branches that complete; a value position with no expectation of its own
sees a diverging value as `Int`, which is the reference's `value_type(e, None)`; and what
follows a child that leaves is refused as `N0313`, which this checker had never reported.

In the emitter, **whether the block being written is live is the one answer**, and it is
asked in one place, `unreachable_here`. A node reached while it is not was never evaluated,
so nothing is emitted for it — no call with an argument missing, no store of a value nobody
produced, no arithmetic on half its operands — and what its operands left on the stack is
discarded down to the depth recorded where its subtree began, because the transfer already
gave back what they held. Nothing reopens a block except a construct whose dispatch really
was emitted (`opened`), and a join is opened only if some branch reached it (`joined`). A
`phi` is built from the incomings that arrived (`nin`, and `emit_phi` shared by `if` and
`match`), so a branch that transferred contributes nothing rather than a stand-in.

Two things were found by the audit that followed, and both are transfer rules the reference
had and this compiler did not. **A `break`, a `continue` or a `return` out of a `scope` did
not join it**, since N7: a task that failed inside a scope a `break` left was never waited
for, so the program printed a value where the reference stopped with the task's diagnostic.
`join_from` is the reference's `join_through`, and `unwind` uses it too. And **`==` on two
strings kept its operands**, in both native backends and with no transfer involved: a
temporary compared was never reclaimed. Rule 2 says a comparison borrows, as a call does.

**Why not the enum N10 made available.** `enum EmitCompletion { Value(value: Int, ty: Int),
Transferred }` is the shape the reference has, and it does not fit here for the reason N10G
gave: the emitter's operand stack is an `Ints`, and no container can hold an enum. A
post-order scan also never has a completion *in hand* to return — the node that would return
it is not the node that consumes it. The live flag is the same fact stated at the only place
this design can ask it, and there is one of it rather than one per consumer.

### No value, as a third completion — N10.2

N10.1's bit said *never completes* and left *completes with no value* inside the type code,
as `unknown` — which also meant "already reported". N10.2 replaced the bit with the
reference's three states: `ncomp` in `compiler/analyse.nz` holds `c_value`, `c_unit` or
`c_diverges` for every node, and nothing reconstructs one from another or from the type.

| Node | Completion |
| --- | --- |
| `break`, `continue`, `return` | `Diverges` |
| an expression statement | its expression's, if that diverges; otherwise `Unit` |
| `let`, assignment, `while`, `scope`, `spawn` | `Unit` |
| a block | `Diverges` if a statement is still leaving when the statements end (a tail after it is `N0313`); otherwise its tail's; with no tail, `Unit` |
| an `if` with no `else` | `Unit`, whatever its branch does: the condition may be false |
| an `if` with one | the join below |
| a `match` | its arms folded: one that leaves defers, one with no value makes the whole `Unit` |
| every other expression | `Value` |

The `if` join is the reference's, in the reference's order — the order is the rule, because
the first line decides the `Unit` and `Diverges` pairs:

| then | else | result | diagnostic |
| --- | --- | --- | --- |
| `Diverges` | anything | the else branch's completion and type | — |
| anything | `Diverges` | the then branch's | — |
| `Value(A)` | `Value(A)` | `Value(A)` | — |
| `Value(A)` | `Value(B)` | `Value(A)` | `N0302` at the else branch's value |
| `Value(A)` | `Unit`, or the reverse | `Value(A)` | `N0302` at the whole `if` |
| `Unit` | `Unit` | `Unit` | — |

An expected outer type changes none of it: the reference passes one into the branches for
recovery only, so `f(if c { 1 } else { "x" })` is `N0302` whatever `f` takes.

**`N0300` is decided in one place.** `value_child` says which children of a node must
produce a value — every operand, argument and initialiser, the value of a `let`, an
assignment and a `return`, and the condition of an `if` and a `while`; the scrutinee asks
the same question where it is finished — and `needs_value` refuses a `Unit` there at the
child's span, before the node's own rule reads the child's type. `Diverges` is not refused:
it never produces the contradictory value. After the report the value is read as the
reference reads it, `expected.unwrap_or(Int)`, so the consequences the reference reports —
`N0304` for a comparison with a `Str`, `N0342` for a scrutinee — are reported here too, and
the ones it does not are not.

Two checks the Nazm checker had never made came with it: a body is held to its signature
(`N0300` at the tail, or at the body when it produces nothing) and so is each `return`.
And a `match` whose scrutinee is not an enum discards what its arms reported, because the
reference returns after `N0342` without checking them.

**The emitter changed in one place**, as a consequence. A `match` statement whose arms are a
value and nothing is `Unit` — the reference accepts it — so the value arm's value has no
`phi` to go to. `discard_branch_value` drops it inside the arm's own block and releases it if
it was a temporary, which is what an expression statement does with a value. Before N10.2
that `match` had its value arm's type and never reached this question; it reached a
different one, the emitter's stack.

### What this still does not do

- **No generics** — *superseded by N11, §7.12*: a generic enum is expressible now, and
  `Option` and `Result` are still not built in — *and since N12, §7.13, they are ordinary
  enums of the core prelude rather than built into the checker*.
- **No wildcard arm**, no guards, no or-patterns, no literal or record patterns, no `let`
  destructuring. N10 is variant selection with named payload bindings.
- **No space promise.** The representation is one slot per variant, which is larger than a
  union; `performance.md` says by how much, and nothing here claims otherwise.
- **No stable ABI.** Discriminant values, payload layout, size and calling convention are
  compiler-private and may change between builds.
- **A bare block is still not an expression in the self-hosted parser**, so an arm body
  there may be `n + 1` but not `{ let n = …; n }`. That limitation is N2's and is unchanged;
  N10 only made it reachable in one more position — *closed by N12.1, §7.14, in both
  compilers*.

## 7.12 Generics: parametric checking, concrete code — settled by N11, 2026-09-23

N9 asked whether ownership composes from field types, and N10 whether that composition can
be chosen at runtime. N11 asks whether it survives **substitution**: whether a definition
checked once, with its parameters unknown, can be given concrete types later without any
law in §7.7–§7.11 learning that it happened. `docs/spec.md`, *Generics*, is the language;
this section is how both compilers implement it and why.

### The type representation

`Type` stays one small `Copy` value. It gained two variants, and nothing else:

| | |
| --- | --- |
| `Type::Param(TypeParam { owner, index })` | a type parameter: its owner — `Function(FnId)`, `Record(RecordId)`, `Enum(EnumId)` or `Builtin(Intrinsic)` — and its **position** in the owner's list |
| `Type::Vec(VecId)` | `Vec[T]`, an interned element type |

An **applied** record or enum — `Pair[Int, Str]` — is not a new variant. It is an entry of
the record or enum table of its own, whose `instance` is `Some(Instance { template, args })`
and whose fields are the definition's with the parameters substituted, done once, when the
application is interned (`crates/nazm-sema/src/generic.rs`). So `Type::Record(id)` still
names one entry with concrete fields, and every consumer that reads fields — copy and
cleanup derivation, task safety, layout, construction, projection, the emitters' helpers —
was written, tested and mutation-tested against exactly that, and did not change. This is
the decision the milestone turned on: a new `Type::Applied` would have given every one of
those consumers a case to get wrong.

**Interning is what keeps `==` the language's type equality.** One definition applied to one
argument list is one entry — `apply_record`/`apply_enum` look it up before creating it — so
two modules writing `Pair[Int, Str]` for the same `Pair` get the same `Type`, and
`a.nz::Box[Int]` and `b.nz::Box[Int]` do not, because their templates differ. Nothing
compares shapes.

**Completion is deferred and iterative.** A field may name a generic type declared later or
in another module, which has no fields yet while stage 0b is resolving them. So an
application is interned at once and **completed** later, from a worklist, when
`UserTypes::settle` runs; completing substitutes, which may intern more, which join the same
worklist. No completion recurses into another. A definition that would expand without bound —
`Strange[T] { next: Vec[Strange[Vec[T]]] }` — is refused by the ownership rule below before
it matters, and cannot keep the checker busy regardless: an application nested deeper than
`HARD_NESTING` (256) is interned and never completed, and there is a total
`INSTANCE_BUDGET` (100,000). Those are the last line, not the rule.

### Checking a generic definition, once

`declare_unit` reserves a function's id before resolving its signature, because the
parameters' types name their owner. The body is checked **once** with `T` a `Type::Param`,
and every rule that asks a type what it supports gets the answer an unknown type deserves:
`==` refuses it (`N0304`), a projection refuses it (`N0335`), a `match` refuses it, and task
safety says no — `may_cross_into_a_task` excludes parameters, so a generic body cannot
launder a `Vec` into a task through a `T`. What a `T` may do is what every value may do:
be bound, copied by its own law, passed, returned, stored, pushed and popped.

### Calling one, and inference

`check_generic_call` is the whole rule. Each parameter is `solved` from an argument by
`unify`, which is structural through applications and `Vec` and nothing else, or taken from
the written type arguments when there are exactly as many as the callee declares. An
argument whose declared type is already known — it mentions no parameter, or every one was
written — is checked against it as a monomorphic argument is. **The expected result type is
never consulted**, so `fn make[T]() -> T` needs `make[Int]()` (`N0354`), and a conflict
between two arguments is its own diagnostic (`N0355`) rather than the second argument read
against what the first happened to say. One mistake is one error: a parameter an argument
already failed on is *excused* from `N0354`. What the call settled is recorded on its site,
for the planner below.

### What an importer reads: `nazm.interface/4`

`crates/nazm-iface/src/wire.rs`. A type is an untagged `PersistentType` — a built-in's name,
`{ param }` (a **position**), `{ vec }`, or `{ def, args }` — and every generic definition
and export carries `generics`, its arity. **No parameter name appears anywhere**, so
alpha-renaming changes no byte and no `InterfaceHash`
(`a_generic_interface_carries_positions_and_no_parameter_names`), while reordering,
adding a parameter or changing a public generic signature, field or variant does
(`the_generic_invalidation_table_is_what_the_fingerprint_actually_does`). A body edit
changes nothing an importer reads. Reading validates every parameter position against its
owner's arity and every application against its definition's, bounds nesting at 64, and
refuses bytes that do not re-serialise to themselves (`NotCanonical`); rehydrating defers
and settles like stage 0b does. A dependent type-checks `Box[Int]`, `Maybe[Str]` and
`identity[Int]` against the deserialised interface **with the dependency's source deleted**.
`SEMANTIC_EPOCH` is 4: `[` and `]` stopped being unknown characters, a generic body is held
to old codes under new rules, and the interface a fingerprint is taken over changed shape —
the reasons are in `crates/nazm-cache/src/identity.rs`.

### Native code: parametric semantics, concrete LLVM

`nazm check` and `nazm run` are parametric — the interpreter runs a generic body once for
every type. `nazm build` **monomorphises**: one native function per generic function and
concrete argument list the program reaches. That is a property of this backend, not of the
language (`docs/spec.md`, *What a program cannot observe*), and the reason is concrete:
LLVM needs a layout and a copy/drop sequence per value, and an instance knows both at
compile time for free. Dictionary passing — a runtime description of `T`'s size and helpers
— would let one body serve every type and is the natural design for a future development
tier that trades code quality for build speed — the Cranelift tier of §1; nothing in the
semantic layers assumes monomorphisation, so it remains possible.

**Instance identity** is a `GenericInstanceKey { def: FnId, args: Vec<Type> }` inside one
compilation and, durably, the definition's `DefKey` plus each argument's **canonical type
key** (`crates/nazm-lir/src/symbol.rs`, `type_key`): a built-in is its name, a definition is
`R.m.<module key>.<name>` or `E.…` (each component encoded), an application appends its
arguments' keys in parentheses, a `Vec` is `Vec(…)`, and a module with no durable key is
`u.<index>`. The key contains no `RecordId`, `TypeId`, span or absolute path. An instance's
symbol is `nz.m.<module>.<name>.g.<encode(keys, ",")>`, so `identity[Int]` and
`identity[Str]` are different symbols by construction and the same instance requested from
two modules is one.

**Planning** (`crates/nazm-lir/src/instance.rs`) is global and deterministic. The
expanding-cycle rule runs first: nodes are (function, parameter position), a call passing a
type built from the caller's parameter to a callee's parameter adds an edge — *expanding*
when the argument is more than the parameter itself — and an expanding edge inside a
strongly connected component is polymorphic recursion, refused with the call that expands
(`N0357`). Then a breadth-first worklist starts from the calls non-generic functions make
and adds every call each instance's body makes under its substitution; everything an
instance's body mentions is interned then, so the table is complete before a layout is
written. `MAX_INSTANCES` (10,000) and `MAX_INSTANCE_NESTING` (64) are stated bounds
(`N0358`). The planned list is sorted by symbol. Ordinary recursion through a generic
function is recursion, and the definition-level recursion check refuses it as it refuses any
(`N0101`).

**Artefacts.** Each instance is lowered, from its definition's own nodes, into a unit of its
own — artefacts `g0`, `g1`, … — never into a caller's. A caller declares the instance by
symbol and signature; a declaration now carries the types its signature names into the
caller's unit, which the conformance corpus found missing (`<enum>` in a declaration of
`or_else[Str]`). A private function a generic body calls gets hidden linkage, because the
instance that calls it is another object. The object cache needed nothing new: an
instance's key is its exact bytes, as every unit's is. Measured
(`crates/nazm-cli/tests/native_cache.rs`): a new instantiation compiles the caller, the new
instance and whatever runtime it newly reaches; a **generic body edit recompiles exactly its
instances and no caller**, not even its own module's unit; renaming a parameter recompiles
nothing. `nazm.build-report/1` gained `native.instances { units, compiled, reused }` — a
compatible addition under its contract.

### `Vec[T]`

The sequence header every `Ints` already has — `{ len, cap, data, rc }` from
`@nz.seq_new` — so a `Vec` is allocated, counted, grown and reclaimed by the runtime that
already does all four. What is `T`'s is the element's layout, asked of LLVM by the address of
element 1 of an array at null (no semantic stage knows a byte size, §7.10), and what an
element owns, by `T`'s own law: push takes its own reference, get returns its own copy, set
takes the new reference before releasing the old, pop transfers. One release helper per
owning element type, `nz.vrel.<key>`, releases each element before the buffer; a `Vec` of
non-owning elements is released as an `Ints` is. Growth relocates bytes with `realloc` and
changes no count: every value this language has is movable by its bytes. `arr_grow`'s size
arithmetic is overflow-checked (`umul`) since N11.

### Ownership cycles, over definitions

`UserTypes::ownership_cycles`. The graph's nodes are **definitions** — an application is
not a declaration and is answered by the one it applies — and an edge runs from a record or
enum to every definition its fields or payloads mention, labelled **inline** or **counted**
by the path. How a generic definition uses each parameter — inline, counted, both, or not at
all — is found first by a fixpoint (`parameter_uses`), and an application's argument
inherits that use composed with how the application is reached. So `Tag[T] { n: Int }` owns
nothing of its argument and `Box[T] { v: T }` owns it inline. An all-inline cycle is
`N0336`, as before; any cycle with a counted edge is `N0359`, with the path. Two passes —
inline first, then everything, avoiding what the first reported — so one cycle is one
error.

### The compiler written in Nazm

`compiler/*.nz` implements all of it within its own subset, and did not mirror the
reference's structure where its own was simpler:

- **One table.** The record table since N9 already held records, enums and variants; N11
  adds two more kinds, `rk_vec` and `rk_param`, and an application is an entry of kind
  record or enum with `tpl` naming its definition. A type is still one `Int`: `100 + r` a
  record, enum or variant, `1_000_000 + r` a `Vec`, `2_000_000 + r` a parameter. The arrays
  travel as one `struct Table` of handles — the first record in the compiler used as a
  bundle of shared state.
- **No recursion anywhere**, which the native backend requires of the compiler compiling
  itself. Substitution is a post-order walk with an explicit stack; `apply` only enqueues,
  and the stages that hand a type on call `settle_now` — because `apply → settle → complete
  → substitute → apply` is a call cycle `nazm build` refuses, and was, the first time.
- **Written types carry their offsets.** A type with arguments is written into a node's text
  as `Pair@10[Int@15,Vec@20[Str@24]@27]@28`, and a return type always carries its offset, so
  the checker reports at the reference's spans; `selfhost.rs`'s dumper writes the same text
  from the reference's `TypeName`.
- **Instances are node ranges.** The emitter plans instances with a worklist before emitting
  anything, records each one's substituted `ntype`/`nrec` and callee instances, and emits a
  generic function's subtree once per instance with those arrays patched — one LLVM module,
  as always; the per-instance objects are the reference's decomposition, not a language
  requirement. Instance symbols are `@nz.g<n>`, numbered by the worklist, which is a
  function of the program alone. The same N0357/N0358/N0101 refusals apply.

`the_nazm_checker_agrees_with_the_reference_on_generics` compares code **and** span over 42
programs; the memory test builds five generic programs with both compilers and compares the
reclamation reports; the bootstrap compiles a generic corpus and refuses a refusal corpus
with C2, C3 and the reference.

**The dogfood.** The diagnostics were four parallel arrays — code, start, end, message —
threaded through every stage as four parameters and kept the same length by care. They are
a `Vec[Diag]` now: 19 parameter lists and 127 argument lists lost three parameters each, and
a stage that pushed a code without a message would not compile. Measured on the compiler
compiling itself: the same result, 26 KB less IR, about 3 MB less peak RSS and no
measurable change in time (`performance.md`).

### What this still does not do

- **No traits, constraints, methods or `impl`**, so a generic body can do only what every
  type can, and no `Option`/`Result` in the standard sense — `enum Maybe[T]` is a user
  declaration. No higher kinds, const generics, defaults, variance, specialisation or
  overloading. *(Superseded in part by §7.13: `Result` and `Option` now exist, as ordinary
  generic enums of the core prelude, and N12 needed no generic machinery N11 did not have.)*
- **No spawn of a generic function**, and no generic `main`.
- **No indexing syntax**; `vec_get`/`vec_set` are calls.
- **Monomorphisation only.** Code size grows with the number of instances; `performance.md`
  measures 1, 10 and 100.
- **No space promise for an enum in a `Vec`**: an enum is still one slot per variant, and a
  `Vec[Enum]` multiplies it by the length.

## 7.13 Typed errors: a prelude module and one operator — settled by N12, 2026-09-24

`docs/spec.md`, *Typed error values*, is the language. This is where it lives. The design
question was not how to represent `Result` — N9, N10 and N11 already could — but how one
definition becomes **the** `Result` in every module without a package system, a structural
test or a name test. The answer is one module whose identity the toolchain fixes, imported
by every other.

### The core source boundary

`library/core/prelude.nz` is ordinary `.nz` source declaring `pub enum Result[T, E]` and
`pub enum Option[T]`. Nothing in any crate describes those enums: `nazm-sema/src/prelude.rs`
holds the file's **bytes** (`include_str!`, so a compilation needs no installation
directory — a packaging decision) and the name it is shown under, and the checker reads them
as source like any other file.

- **Loading.** `nazm-service/src/load.rs` (in `nazm-cli` until N16) appends the prelude after every project file; the
  `&str` entry points (`nazm_core::one_file`) do the same. It is therefore the **last**
  module and the last `FileId`: no project module's id and no span in one moves.
  `ModuleGraph::attach_prelude` adds it and an ordinary import edge from every other module,
  with a synthetic span because nothing was written.
- **Identity.** `ModuleKey::prelude()` is `@core/prelude`. `ModuleKey::relative` refuses a
  first component `@core`, so the key is disjoint from every project key by construction,
  and `ModuleKey::parse` accepts it back from a persisted interface.
  `ModuleGraph::prelude()` finds the module **by key** — never by position — so a
  compilation assembled by hand, or from persisted interfaces, has one exactly when it
  contains that key.
- **Visibility and collision.** Because the prelude is an import, `type_environment` puts
  its exported types in every module's type namespace by the existing rule. The one special
  case is the collision: a project type named like a prelude type is `N0363` at the
  declaration rather than `N0208` at an import line that does not exist.

### `?`: resolved once, lowered as a `match`

- **Syntax.** `TokenKind::Question`, `Expr::Propagate { operand, op_span, span }`, parsed in
  the postfix loop beside projection.
- **Checking.** `Checker::check_propagate`. The canonical `Result` is found once per unit —
  `Enums::declared(prelude, "Result")`, a `DefKey` lookup — and a type is a `Result` iff
  `definition_of(t)` is that enum. The operand's and the function's `Result`s must share
  their second argument by `Type` equality, which interning makes nominal. The decision is
  recorded as a `Propagation { operand, ok, err, into, into_err }` keyed by the `?`'s span;
  nothing downstream asks whether a type is called `Result`.
- **Completion.** None of its own: the operand's `Value(T)` is the result, and the `Err` arm
  is a `return`, which N10.1 already made a transfer from any value position.
- **Interpreter.** `Interpreter::propagate`: the operand once, then `Flow::Fell(payload)` or
  `Flow::Return(Err)` in the function's own `Result` — the same `Flow::Return` a written
  `return` produces, so it leaves loops and scopes, joining tasks, exactly as one does.
- **Native.** `lower_propagate` builds the LIR `Expr::Match` the operator means: the operand
  into a hidden local the frame releases, an `Ok` arm binding the payload with `Stmt::Let`
  and yielding it, and an `Err` arm binding the error and `Stmt::Return`ing a `Variant` of
  the function's `Result`. **There is no new LIR node**, and the emitter did not change: the
  scope joins, temporaries and frame cleanup on the error path are `Stmt::Return`'s, and the
  retain-before-release order is `Stmt::Let`'s. No landing pad, no `invoke`, no personality
  function — there is nothing to unwind, because this is a branch.

### Interfaces and caches

- **`nazm.interface/4` is sufficient.** A signature mentioning `Result[Int, E]` persists as
  an application of `@core/prelude::enum Result`, which `/4` already represented; the
  prelude's own interface is an ordinary `/4` interface; and every module's `dependencies`
  now lists `@core/prelude`, which is data `/4` already had a field for. Nothing changed
  shape, so the schema did not move.
- **One identity across interfaces.** An interface carries every type its shapes mention,
  so B's carries the prelude's `Result`. N3–N11 rehydrated each interface's types
  separately, which made two rehydrated interfaces disagree about a shared definition —
  harmless until a third module's type mattered, and decisive for `?`. N12 adds
  `PersistentUnitInterface::rehydrate_into` with a `Rehydrated` table keyed by `DefKey`: a
  definition met in two interfaces is one definition, and one first met as a mention is
  adopted by its own module when that module's interface arrives, in either order.
- **`CheckKey`.** Unchanged in shape: the prelude is an import, so its `(ModuleKey,
  InterfaceHash)` is in every module's dependency list already. It is not also hashed into
  `CheckerIdentity`, which describes the checker, not the program. `SEMANTIC_EPOCH` moved
  4 → 5 for the rule changes the code set cannot show (`nazm-cache/src/identity.rs`).
- **Objects.** `ObjectKey` is untouched — any difference `?` makes is in the LLVM bytes.
  A module that declares no function now has **no unit**, so the prelude never produces an
  object; its enums' helpers are emitted in the units that use them, as every enum's are.

### The compiler written in Nazm

- **Loading.** `emit FILE CORE` and `check FILE CORE`: the prelude is named on the command
  line, because it is the toolchain's source and not the program's. `attach_core` appends
  it as the last module and gives every module an edge to it. This compiler's modules are
  session indices, so "the last module" was its identity for the prelude — *replaced by N12.1,
  §7.14: the module keyed `@core/prelude`, found by key*. Every stage of the bootstrap reads the same file,
  and `bootstrap.sh` records its digest.
- **Checking.** `n_propagate`, finished in `check_all`: the canonical `Result` is the entry
  the prelude module declares under that name; the node records the operand's `Ok` variant
  entry in `nrec` — substituted per instance like an arm's — and a hidden slot in `nslot`.
- **Emission.** `emit_propagate`, at the node: store the operand into its slot, one test of
  the discriminant, and on the error path retain the error, build the function's `Result`,
  and leave by `n_return`'s exact sequence; on the success path retain the payload and
  release the operand.

### The dogfood

The front end — lexing, parsing and twelve checking passes over one shared `Vec[Diag]` —
was written out twice, in `check.nz` and in `emit.nz`'s `main`, and each copy ended by
asking `vec_len(diags) > 0`. It is now **one stage**, `check_program(…) ->
Result[Checked, Vec[Diag]]`, where `Checked` is a record of the tables emission reads.
Inside it every pass still collects into the one `Vec[Diag]` and keeps going — recovery is
untouched, and a program with three mistakes still reports three. `check.nz` lost its copy
(171 → 64 lines), and `emit.nz` became `emit_program(…) -> Result[Str, Vec[Diag]]`, whose
first line is `check_program(…)?`, with `main` taking the result apart once. Measured in
`performance.md`.

### What this still does not do

- **No `?` on `Option`**, no conversion between error types, no `Error` trait, no methods
  on `Result`: each needs a protocol, and traits do not exist.
- **No `main` returning `Result`**: mapping a typed error to a process status is a separate
  contract.
- **No niche layout**: `Option[Str]` and `Result[A, B]` are enums, one slot per variant.
- **Every module depends on the whole prelude**, not on the names it uses. With two
  declarations that over-invalidates nothing.

## 7.14 One block in every position, and the prelude by its key — settled by N12.1, 2026-09-24

N12.1 adds nothing to the language. It closes two places where an implementation said less
than the language does: the native backends refused a block used as a value anywhere but a
function body and an `if` branch, and the compiler written in Nazm recognised the core
prelude by its position.

### The block law, unchanged

`docs/spec.md`, *One block, and completion decided by checking*: a block is statements and an optional tail, and it
completes `Value(T)`, `Unit` or `Diverges`. A `match` folds its arms as an `if` folds its
branches — an arm that diverges defers to the others, a `Unit` arm makes the whole `match`
`Unit`, and two `Value` arms must agree. The parser has always accepted `{` as an operand,
the checker has always checked it with `check_block`, and the interpreter has always run
it. What changed is that `nazm build` and the Nazm-written compiler compile it.

### Reference backend

- **One lowering for a value block.** `lower_block` — statements, then the tail, `result:
  None` when the statements transfer — is now reached from a function body, an `if` branch,
  a `match` arm written as a block, and a bare block in any value position. A block arm is
  the pattern's bindings followed by the block's own statements, in one `Block`. There is
  no second block lowering and no arm-specific completion rule.
- **`Expr::Block(Box<Block>)`** is the one LIR generalisation a value position needed: a
  block inside an argument or an initialiser has nowhere else to go, and hoisting its
  statements out would move their effects before the arguments to their left. It is
  emitted by the same `Emitter::block` the branches use. Its bindings need no scope: slots
  are unique per function, so a block local is frame storage exactly as an `if` branch's is.
- **`Stmt::Match`**, the statement form, pairs with `Stmt::If` for the same reason: an arm
  that only assigns completes `Unit`, which a value `Block` cannot represent — its `None`
  means *never arrives*. `lower_effect` is the one place an expression in statement
  position is lowered: an `if`, a `match` or a block keeps its statement form, anything else
  is a `Stmt::Discard`. The emitter shares the scrutinee dispatch (`dispatch`) and the
  release at the join (`release_matched`) between the value and the statement `match`.
- **Joins.** Unchanged, and now reached by block arms: an arm whose block transfers is
  emitted into its own terminator and is not a predecessor of the join; the `phi` lists only
  the arms that arrive; a `match` no arm reaches opens no join and has no value.

### Ownership at a block's end

A block does not release its locals when it ends. They are frame slots, released when the
function leaves or when their `let` runs again, which is what N7 already specified for a
branch. So the take-before-release question is the consumer's, and it is the one every
value already answers: a tail read from a local is **borrowed**, and whatever takes it over
— a `let`, an argument, a field, the `phi` of a `match` in each arm's own block before its
jump — takes its own reference. A `Vec` tail is the same handle, not a copy. A pattern
binding took its own reference from the scrutinee when it was bound, so the scrutinee's
release at the join cannot reach a payload an arm handed on.

### The compiler written in Nazm

- **Parsing.** `{` in operand position suspends the expression exactly as an `if` does and
  opens a `frame_blockexpr`; when it closes, the block is the operand, wrapped in a
  `blockexpr` node — the reference AST's `Expr::Block`, and the encoding `selfhost.rs`
  already fixed for it. A statement that starts with `{` is block-like and needs no `;`.
- **Checking.** `blockexpr` completes and types exactly as its block does. The block's
  `Value`/`Unit`/`Diverges` is the existing `c_value`/`c_unit`/`c_diverges`; nothing is
  inferred from which node came last.
- **Emission.** Structure only, like `block`: the tail's value is already on the stack, and
  the joins, `phi` incomings and `live` state are the existing ones.
- **The prelude's identity.** A module's entry in `mpath` is its **key**: a project file's
  normalised path, or `@core/prelude`, which `attach_core` gives the prelude. `core_module`
  finds the module with that key, once, in `check_program`, and both consumers — the
  collision rule in `check_type_imports` and `?`'s `Result` in `check_all` — take that one
  answer. A project path whose first component is `@core` is re-keyed out of the namespace,
  so no project module can hold the key; like the reference, such a file still compiles.
  The loader still places the prelude last, and nothing reads that.

### The dogfood

`record_owned` — which declaration a module owns under a name — answered `-1` for none. The
same `-1` is what `definition_of` answers for any type that is not a record, so each of its
four consumers carried an `if found >= 0` guard, one of them in front of the comparison that
decides whether a type is the core `Result`. It returns `Option[Int]` now, and each consumer
takes it apart with `match`: the type lookup falls through to imports on `None`, the
duplicate and collision checks report from the `Some` arm, and `applies_core_result` answers
`false` for a compilation with no prelude rather than comparing with anything.

### What this still does not do

- **No equality on enums**, so the compiler's integer tags stay integers: `kind ==
  n_block()` cannot become a `match` without a comparison the language does not have.
- **A block does not end a lifetime.** Locals are released with the frame; nothing here
  shortens that.
- **The self-hosted compiler still emits one LLVM module**, and still has no durable keys
  beyond the prelude's.

*N13 answered the first of these: enums compare now, and `analyse.nz`'s completion states
are one (§7.15).*

## 7.15 Equality: a derived capability, and a helper per compared type — settled by N13, 2026-09-25

`docs/spec.md`, *Equality is derived*, is the law. This is how the two compilers implement
it, and the decisions that are implementation rather than language.

### One derivation, beside the others, and independent of them

`UserTypes::equality(t) -> Result<(), NoEquality>` in `crates/nazm-sema/src/udt.rs` is the
whole rule. It sits beside `needs_cleanup` and `task_safety` because it is the same kind of
answer — a property of a type derived from what it contains — and walks the same
containment graph through `contained`, which for a record is its fields and for an enum every
payload field of every variant. What it does **not** do is consult either of them: a `Str`
needs cleanup and compares; a `Chan` may cross into a task and does not compare; `Vec[Int]`
does neither. Leaves: `Int`, `Bool` and `Str` have equality; `Ints`, `Strs`, `Vec`, `Chan`
and `Param` do not. The walk is iterative, depth-first over declaration order so the
reported path is the one a reader finds first, and keeps a visited set, so it ends even on a
program whose containment cycle is being refused in the same run. It is not memoised:
answers are asked once per `==`, and each is linear in the definitions reachable from the
type, which no measurement has shown to matter.

The checker's `check_binary` is the one consumer: `lt == rt && types.equality(lt).is_ok()`.
Type equality is id equality, so the nominal rule is the representation's — two records of
one shape are two `RecordId`s — and an applied type is its interned instance, whose fields
are already substituted: `Box[Int]` is answered over `value: Int`, `Box[T]` over
`value: T`, and nothing in the rule knows generics exist. `Option` and `Result` are
instances of prelude enums and are answered the same way; no code names them.

The refusal stays `N0304`. The derived path (`NoEquality::path`) makes its message say which
component is responsible, `inner.values` or `work.Held.items`, in the task-safety path's
spelling.

### Persisted interfaces carry nothing new

An interface already carries each exported record's and enum's fields and variants, with
their types, under durable identity — which is exactly the input the derivation reads. So an
importer that has only the interface derives equality for the imported type exactly as the
declaring module does (`a_dependent_derives_equality_from_a_deserialised_interface_with_the_dependency_deleted`),
and no `equality_capable` fact is written that could disagree with the shape it summarises.
The schema stays `nazm.interface/4`, and an unchanged public type has the same bytes and the
same `InterfaceHash` it had before N13; the pinned-interface tests are unchanged.

The checker's own identity moves instead: `SEMANTIC_EPOCH` 5 → 6, because the conditions
under which `N0304` is raised changed and the code set did not. `CheckerIdentity::at_epoch`
lets a test stand in for the epoch-5 checker, and an entry that build would have filed is a
miss (`an_entry_from_the_checker_before_derived_equality_is_never_read`).

### The interpreter: the language's relation, not the host's

`eval.rs`'s `equal(a, b) -> Option<bool>` is `==`, written out: `Int` and `Bool` by value,
`Str` by `bytes()`, a record over its fields, an enum by variant and then over that variant's
fields. An enum value carries only its active payload, so there is nothing inactive to read.
No definition id is compared — checking already guaranteed one type — and `Value`'s own
`PartialEq`, which compares sequences by identity so the rest of `Value` can derive things,
is not what `==` evaluates. `!=` is `!equal`, computed once.

### Native code: a helper per compared type

A record or enum comparison lowers to `Expr::Compare` exactly as a `Str` one does, and the
emitter calls `@nz.eq.<type>(T %a, T %b) -> i1`, generated on first use in the unit:

- **A record** extracts each field in **layout order** — canonical, by field name — compares
  it (`icmp`, the `Str` length-then-`memcmp` branch, or the field type's own helper), and
  branches to `false` at the first difference. Comparing is pure, so the order cannot be
  observed; canonical order makes the helper's bytes independent of how the declaration was
  written, and a program whose fields, variants and payload fields are written in another
  order emits byte-identical IR (`declaration_order_changes_no_byte_of_the_program`).
- **An enum** compares discriminants and branches to `false` if they differ, before any slot
  is read. Equal ones `switch` to the variant's arm, which extracts that variant's slot — tag
  plus one — from both operands and compares its payload fields; a zero-payload variant has
  no arm and reaches `true` through the default. An inactive slot is never extracted.
- **`!=`** is the helper's answer and an `xor i1 …, true`. There is no second helper.

Helpers are `internal`, named from the type's `NativeTypeName` — the durable identity with
its canonical type arguments, so `Box[Int]` and `Box[Str]` get two and a comparison written
twice gets one — and duplicated in each unit that compares the type. No symbol is exported,
so no ABI is promised and the object cache needs nothing new: a changed comparison is changed
LLVM, which is a changed `ObjectKey`. The recursion over a nested type is calls between
helpers, in the emitted program, as retain and release already are.

**Ownership** is the `Str` comparison's, generalised: both operands are evaluated, left then
right; the helper borrows them and retains nothing; after the answer exists each operand that
is a temporary is released once (`drop_temporary`). A bound operand is untouched. An operand
that transfers leaves before the helper is called, and what the other side built is released
by the unwinding that already exists.

### The compiler written in Nazm

`analyse.nz` has the same derivation, `equality_blocker`, over the same table: an enum's
fields are its variants and a variant's are its payload, so one walk covers both kinds, as
`task_unsafe_path` already did. `emit.nz` emits the same helper shape, `@nz.eq<entry>`,
generated after every function for each entry a comparison reached, each adding the entries
its own fields compare. The two compilers' helpers are not textually identical — the
self-hosted one delegates a variant's payload to the variant entry's helper — and nothing
requires them to be: both are compared on answers and on reclamation, not on text.

### The dogfood

`analyse.nz`'s completion was `c_value() = 0`, `c_unit() = 1`, `c_diverges() = 2` stored in
an `Ints`, with a fourth value, `-1`, meaning "no arm yet" in a `match`'s fold. It is
`enum Completion { Value, Unit, Diverges }` in a `Vec[Completion]` now, the fold is an
`Option[Completion]`, and its twenty comparisons are `Completion == Completion`. An integer
that is not a completion can no longer be stored as one — which is also why nine historical
mutations that compared a completion with `99` had to be rewritten: the state they
simulated is no longer expressible.

### What this still does not do

- **No abstraction over equality.** `fn same[T](a: T, b: T)` is refused and has to be: there
  is no way to say "`T` has equality", and N13 added none.
- **No sequence or channel equality**, and so none for anything that contains one.
- **No ordering, hashing or custom equality** on a user type.
- **An enum is still one slot per variant**, so an equality helper for a wide enum extracts
  from an aggregate larger than a tagged union would be; the helper reads only one slot.

## 7.16 A requirement on a type parameter: one set, read by one derivation — settled by N14, 2026-09-25

`docs/spec.md`, *A type parameter may require equality*, is the law. N13 found the wall:
a concrete `Box[Int]` compares and a generic body over `Box[T]` cannot say it wants one
that does. N14 is the smallest mechanism that lets it say so.

### Why a built-in requirement and not a trait

Three shapes were compared. A **trait-shaped bound** (`T: Eq`) reads as the first half of a
trait system — it invites `impl Eq for`, which would contradict N13's rule that equality is
derived and cannot be written — and would need a namespace in which user traits could later
live, decided now for no program. **Another spelling** (`T: ==`, `requires equality(T)`)
avoids a name but is either an operator in a type position or a second clause form. A
**built-in capability requirement**, `T: Equality`, is one name in one position: it reuses
the `:` a field already uses for "has this type", names the capability rather than a
declaration, and commits to nothing a future constraint system would have to undo — a trait
system could later make `Equality` one of its members, or not.

### Representation: an assumption environment of one set

`Generics::equality: BTreeSet<TypeParam>` holds every parameter whose declaration requires
equality, keyed by the parameter's identity — its owner and position — never by its name.
That set *is* the assumption environment. A parameter only appears inside its own
declaration, so one set for the compilation answers "what may this body assume" for every
body at once, and no environment has to be threaded through checking.

`UserTypes::equality` — N13's single derivation — reads it in exactly one place: the leaf
rule for `Type::Param(p)` is `requires_equality(p)`. Everything else follows from the walk
that already existed: `Box[T]` is an interned instance whose field is `T`, so it derives
equality exactly when `T` does; `Result[T, E]` needs both; `Vec[T]` is a leaf with none. No
bounded-parameter case is written in the operator, in record or enum logic, or in the core
prelude, and termination is the walk's (a visited set over definitions).

### Checking

* **Declaration** (`declare_unit`): `declared_requirements` records `T: Equality` for a
  function's parameter, refuses an unknown name (`N0365`) and a requirement on a record's or
  enum's parameter (`N0101`, not supported yet: every instance of such a type would have to
  be checked wherever it is written).
* **The body**: unchanged code. `check_binary` already asks `equality(lt)`; a required `T`
  now answers yes. The body is checked once.
* **A call** (`check_generic_call`): after the type arguments are settled — written or
  inferred, by the unchanged N11 rules — each argument of a required parameter is asked
  `equality(arg)`. A concrete argument answers by what it contains; a caller's own
  parameter by *its* requirement, which is what makes forwarding work exactly when the
  caller requires the same. `N0364` is reported at the callee for a written argument and at
  the argument an inferred one came from, with N13's blocking path.

### Interfaces and caches

`nazm.interface/5` adds `requires_equality` — ascending positions, absent when empty — to
each exported function; the reader validates it and rehydration re-records it, so an
importer checked against the interface alone is held to the requirement. A `/4` file could
not say it, and a `/4` reader handed a `/5` file could not honour it, so the major moved.
Because the field is by position, renaming a parameter changes no byte; because it is in the
interface, adding or removing a public requirement moves its `InterfaceHash`, which reaches
every importer's `CheckKey` through its dependencies — the existing invalidation path, with
nothing added. A private function's requirement is not in any interface and rechecks only
its own module. `SEMANTIC_EPOCH` moved 6 → 7 for the module's own body: `==` on a required
parameter moved from refused to accepted under the existing `N0304`.

### Native code: nothing

Native generics are monomorphised, so by the time a body is lowered every parameter is a
concrete type and `==` on it is N13's comparison — an `icmp`, the `Str` comparison, or a
concrete helper. The requirement is not in the IR, the object key or the program: the
instance of `same[T: Equality]` at `Int` is instruction for instruction a hand-written
`same_int` (`a_requirement_leaves_no_trace_in_the_program`). The interpreter is parametric
and compares values, so it has nothing to do either.

### The compiler written in Nazm

The parser writes a requirement into the parameter list's text after the parameter's
position — `[T@5=Equality@8]` — where every existing reader of that text stops, so only the
checker's `requirement_of` sees it. The table gains a `req` column, `1` for a parameter
whose declaration requires equality; `has_no_equality` reads it for a parameter, which puts
the assumption environment in the same one place as the reference's; `check_generic_call`
checks each argument by `equality_blocker`, at the reference's spans. The emitter needed no
change.

### What this still does not do

- **Only equality.** No other capability can be required, and nothing lets a program declare
  one.
- **Only functions.** A record's or enum's parameter cannot carry a requirement.
- **No user-defined behaviour behind a bound.** `==` is N13's everywhere; a bound only proves
  an operand is in its domain.

## 7.17 The lossless concrete syntax tree — settled by N15, 2026-09-25

**Why it exists.** The abstract tree is what a program means, and it forgets exactly what
an editing tool needs: whitespace, comments, parentheses, punctuation, and the whole body of
a function with a syntax error in it. Until N15 comments reached nothing but the formatter,
as a side list of spans, and one malformed statement discarded the rest of its function. The
concrete tree is the file as written. It owns what no other layer owns — the source
structure, every byte of trivia, the structure of malformed regions, and stable node
boundaries — and nothing else: no name is resolved in it and no type is in it, and nothing
downstream of the parser reads it.

**One scan, one parser, two trees.**

```text
UTF-8 bytes ─ lex_lossless_in ─▶ [Lexeme]  (every byte: token | whitespace | comment | invalid)
                                    │
                         tokens, borrowed in order
                                    │
                           parser.rs (one grammar)
                            ├──▶ Program (AST)      → checker, evaluator, backend
                            └──▶ events ─▶ Cst      → tools; nothing in the compiler
```

The lexer produces one lossless stream; the token list the parser reads, and the comment
list the formatter reads, are views of it, so nothing scans a file twice
(`crates/nazm-syntax/src/lexer.rs`). The parser (`parser.rs`) builds both trees from the
same decisions: every consumed token passes through one `bump`, which records it for the
concrete tree, and every construct records where it began and wraps its concrete node at the
point it builds its abstract one. **`parser.rs` is where concrete syntax becomes abstract
syntax** — not a pass over the concrete tree. This is the transitional shape the directive
allowed, stated as such: a later milestone may make the abstract tree a projection of the
concrete one, and nothing here depends on it not being. What makes it one grammar rather
than two is that neither tree can be built without the other's decisions, and
`the_abstract_and_concrete_trees_agree_on_every_construct` checks, over every clean file in
the repository, that each construct the checker sees has a concrete node over the same bytes.

**The tree** (`crates/nazm-syntax/src/cst.rs`) is a `cstree` 0.14 green tree with a red
layer, wrapped: the workspace sees `Cst`, `CstNode`, `Leaf` and `SyntaxKind`, never a
`cstree` type. `SyntaxKind` is one vocabulary — a leaf kind per lexer token (by an
exhaustive mapping, so a new token does not compile without one), three trivia kinds, a node
kind per grammar construct, and `Error`. **No kind has a fixed spelling**: `cstree` would
store such a token without its text and print it back from the kind, which is reconstruction
from canonical spelling; every leaf is interned from the bytes it was scanned from instead.
The end of the file is not a leaf.

**Trivia placement** is decided once, after parsing, from an event log: trivia before a token
is emitted immediately before it, after every node that closed before it and before every
node that opens at it. So no node begins or ends with trivia, and a comment between two
statements belongs to the block around them. Deciding it during parsing put trivia inside
whatever construct was being attempted when it was seen — including ones that then failed —
which the invariant tests caught on the first run.

**Error structure.** A malformed region is an `Error` node over the bytes it covers: the
leaves of the construct that failed and the tokens skipped after it. A character the lexer
refuses is an `Invalid` leaf owning its bytes. There is no synthetic token; a missing `;` is
a diagnostic, not a zero-width leaf.

**Recovery boundaries.** Inside a block, a statement that fails is skipped to the nearest
place a statement can start — past a `;`, before the block's own `}`, or before a keyword
that can only begin a statement (`let`, `while`, `return`, `break`, `continue`, `scope`,
`spawn`) — counting braces from those the failed statement left open, so a `match` it
opened does not end the block. `if` and `match` are not boundaries: they begin expressions,
and the tail of a malformed statement is full of them. Reaching `fn`, `pub` or the end of
the file abandons the block, which is the missing `}` already reported. Every path consumes
a token or ends the block. At the top level, recovery is what it was: skip to the next item.
**The abstract tree keeps its contract**: a body that recovered is complete in the concrete
tree and absent from the abstract one, exactly as a failed body was, so the checker never
sees a statement list with a hole in it and the signature still serves every caller.

**Cascades.** A later independent mistake in the same function is its own diagnostic. When
the item's delimiters do not balance, which block a later statement belongs to is what the
mistake made ambiguous, so after its first recovery the item's further syntax diagnostics
are withheld until the next item begins. Measured against the pre-N15 parser on 4,240
single-token deletions and duplications of real programs: the first diagnostic and the
abstract tree identical in every case, no diagnostic lost, one case with one more.

**What it costs**, measured in process (`performance.md`): the whole compiler written in Nazm
(606 KB) parses in 28.9 ms against 14.3 ms, with 29.2 MB peak heap against 15.8 MB, and a
retained tree of 2.7 MB. Every parse builds the tree, the compiler's included; it is built
and dropped when only the abstract tree is wanted. Removing that cost would mean a parse
path that builds only one tree, which is a second path; it has not been taken.

**Still future work.** Full-file parsing only: no incremental relexing, no subtree reuse, no
persisted green nodes. Recovery is local at statements inside blocks; errors inside an
item's signature, a record, an enum or a `use` still skip to the next item. The formatter
still lays out from tokens — it reads comments from the same scan the tree is built from,
but not through the tree. No language server, no structured-edit command. The compiler
written in Nazm has no concrete tree and does not recover; bootstrap parity is about the
language it accepts, not about this.

## 7.18 The language service and `nazm lsp` — settled by N16, 2026-09-25

**Why `nazm-service` exists.** An editor asks the same questions `nazm check` answers —
which files, what is wrong, what a name refers to — about text that is not on disk yet, in a
process that stays alive. Two things needed a home that was not the CLI binary: the loader
and root discovery, so that the command line and the editor share one module graph rather
than each having one; and a long-lived owner of open buffers. `crates/nazm-service` holds
both. It depends on `nazm-span`, `nazm-diag` and `nazm-core` only (`xtask/src/rules.rs`), so
it can reach the compiler's answers and cannot reach the parser or `nazm-sema` to compute
its own.

```text
editor ──stdio── nazm lsp (nazm-cli/src/lsp.rs)       framing, message types, UTF-16
                     │  byte offsets and paths only
                     ▼
              LanguageService (nazm-service)           buffers, versions, one analysis each
                     │
        load::load_from(…, Overlaid) ── the loader every command uses
        nazm_core::analyse           ── the parse and check every command runs
        Resolution                   ── what each name was resolved to, by identity
```

**Who owns what.**

| Concern | Owner |
| --- | --- |
| open buffers and their versions | `LanguageService` |
| which files a compilation has, identity of a path, cycles, prelude | `nazm-service/src/load.rs`, reading through a `Sources` — `Disk` for commands, `Overlaid` (buffer first, then disk) for the service |
| the source root | `nazm-service/src/root.rs`, `decide`, called exactly as `nazm check` calls it; `nazm lsp --source-root` is `nazm check --source-root` |
| what a program means | `nazm_core::analyse`, which `on_checked_map` is now a wrapper over |
| where a position is | the document's concrete tree (N15), by the rule below |
| what a name refers to | `Resolution`: calls by `FnId`, locals by slot of a function, constructions, variants, fields, payload fields |
| which occurrences an entity has | `ReferenceIndex` (N17, §7.19), derived from `Resolution` and nothing else |
| byte offset ↔ LSP position | `LineIndex` in `nazm-cli/src/lsp.rs`, and nowhere else |

**Where the concrete tree stops.** It answers exactly one question: which token a byte
offset is on. The rule is fixed — the identifier whose bytes contain the offset, or whose
last byte is just before it; anything else, including whitespace, comments, punctuation,
keywords, literals and the end of the file, is on no name. What that identifier *means* is
read from the resolution, keyed by its span. A name the resolution has no entry for — inside
a body that did not parse, a type annotation, a built-in — has no definition. **The service
never resolves a name itself:** no table from names to definitions exists in it, and a unit
test and the `one resolver` gate keep it that way. N16 needed one addition to the resolution
for this — each local binding records where its name is written (`LocalDef::span`), so a use
goes to its definition by slot rather than by spelling.

**Current versions.** Every open, change and close bumps a generation and re-analyses every
open document before returning. A query is answered only from an analysis whose generation
is current and whose version is the one asked about; diagnostics are published with the
version they were computed for, and every document's complete set is published every time —
an empty set included, which is how an editor clears errors. A broken buffer is analysed like
any other: its diagnostics are current, and a query into its broken body has no answer. The
previous, clean analysis is not kept.

**What is deliberately not incremental.** Every open document is loaded, lexed, parsed and
checked in full as the root of its own compilation on every change. No reparse of a changed
region, no maintained module graph, no reuse of semantic work between versions, no
persistent cache read or written: an unsaved buffer is not state `nazm-cache` may record. What
is reused is the process and the buffers. Measured on the whole compiler written in Nazm, a
warm edit is about 80 ms on one CPU (`performance.md`) — the evidence that incremental
reparse is not yet needed.

**Protocol surface.** Full-document synchronisation, `publishDiagnostics`, definition and
hover, positions in UTF-16 — and, since N17, references (§7.19), since N18 prepare-rename
and rename (§7.20), since N19 completion (§7.21), since N20 signature help (§7.22), since
N21 structure completion and constructor signature help (§7.23), since N22 document and
workspace symbols (§7.24), since N23 `semanticTokens/full` (§7.25), since N24 quick
fixes over `textDocument/codeAction` (§7.26), since N25 call hierarchy (§7.27), and since
N26 completion at a structured hole with `.` as the one trigger character (§7.28). Nothing
else is advertised. The core prelude has no file, so a
definition in it has no location. Diagnostics keep their Nazm codes and messages; the
protocol message is an adapter's rendering of a `nazm.diagnostic/1` object's primary span,
not a new schema.

## 7.19 The reference index and `textDocument/references` — settled by N17, 2026-09-25

**What a reference is.** A source occurrence that the current analysis resolved to a
specific entity — exactly the spans `Resolution` recorded an answer for. Not an identifier
with a matching spelling, not a mention in a comment or a string, not a name the checker
could not resolve, and not a token the concrete tree merely knows is a name.

```text
CST             → which token a position is on        (N15, the service's cursor rule)
Resolution      → which entity that occurrence is      (the checker, and only the checker)
ReferenceIndex  → which occurrences an entity has      (N17: Resolution read backwards)
LSP adapter     → UTF-16 ranges and `includeDeclaration` (nazm-cli/src/lsp.rs)
```

**What the index owns.** `nazm_sema::ReferenceIndex` (`crates/nazm-sema/src/references.rs`)
is built once per analysis by `ReferenceIndex::of(&Resolution)` and dropped with it. It holds
every indexed occurrence — declaration or use — with its entity, and for each entity its
declaration and its uses, apart. It is derived from two things the resolution already has:
the definition tables (functions and their locals, records and their fields, enums, their
variants and their payload fields), read by position; and the use answers the checker
recorded, read by span (`Resolution::each_use`, crate-private). **It resolves nothing**: it
never reads a name to decide what anything means, holds no scope, and a unit test keeps every
spelling-keyed search out of the file. A declaration is not a use of itself: the span where a
`let` introduces its binding, which the checker records for the backend, is the binding's
declaration and is not in its uses.

**Identity.** `ReferenceTarget` is resolution's own identity and nothing stronger:
`Function(FnId)`, `Local(LocalRef)`, `Record(RecordId)`, `Variant(VariantRef)`,
`Field(FieldRef)`, `PayloadField(PayloadRef)`. Every one is **session-local** — valid in one
compilation — and none is persisted: `nazm.interface/5` and `SEMANTIC_EPOCH` 7 are unchanged,
because an interface describes what an importer needs, not where anybody uses it. A durable
identity exists for top-level definitions (`DefKey`) and is not needed here; a local has only
its slot, and stable local identity across edits is not claimed. One adjustment is made, and
it is not a resolution: a use through an applied type (`Box[Int]`) is taken to the definition
it applies, by the type table's own `Instance` record, so a generic record's field has one set
of uses however many instances reach it.

**What `Resolution` gained.** Two narrow things, both recorded where the checker already knew
them. A variable use now records **which function's** slot it is (`LocalRef { function, slot
}`; `Resolution::var` still returns the slot the backend reads, and `Resolution::local` the
pair), so a use is not placed by working out which body a span is in. And a record
construction's initialiser label records **which field** it names — the checker looked the
field up there and discarded the answer — so a field's uses include the constructions that set
it. Neither changes what any program means, and the backend and interpreter read neither.

**Definition and references are one lookup.** The service takes the occurrence under a
position to its entity through the index; definition is that entity's declaration, references
are its uses. They cannot disagree about what a name means, because neither decides it. One
consequence, deliberate: definition on a *declaration* now answers with that declaration (N16
answered it for a `let` and not for a function), and definition on a construction label now
answers with the field.

**Which program a query covers.** The current snapshot: every compilation the service holds —
one rooted at each open document, loaded exactly as `nazm check <that document>` would load it
— that contains the file declaring the entity, with the same text. Identity does not cross
compilations, so an entity is carried from one to another by **where it is declared**: the
same bytes of the same file, analysed from the same text, are the same declaration in each,
and each compilation's own index answers what its uses are. Uses are merged by file and span,
so one found by two compilations is listed once, and ordered by file path, then start, then
end. A file that no open document's compilation loads is in no program being analysed and is
not searched: there is no scan of the disk and no persistent symbol index.

**Current means current.** The index is part of an analysis snapshot: replaced on every open,
change and close with the analysis it was read from, never kept across generations, and
answered only for the version asked about. A body that did not parse has no resolution, so its
names are no one's references — the concrete tree knowing a name is there does not make it
one. `retained()` and `indexed()` are functions of what is open now.

**What has no references.** A built-in (it declares nothing), a type parameter, and anything
in a body that did not parse. Until N18 a type name in an annotation or a generic argument, and
the enum name in `E.V(…)`, had none either; they are occurrences now (§7.20). A core-prelude
entity has uses in files and no declaration an editor can open, and the prelude's own uses are
not navigable.

**What is deliberately not here.** Rename, prepare-rename, any edit, call hierarchy, a
general semantic graph, a persistent index, incremental invalidation (the index is rebuilt
with its analysis), and MCP. The only edges are *occurrence → entity* and its reverse.

## 7.20 Type-name identity, safe rename and stale-safe edit plans — settled by N18, 2026-09-25

Four layers, each owning one thing, and the rule that binds them:

```text
Resolution      the entity each occurrence is                  (the checker)
ReferenceIndex  entity → occurrences                            (§7.19, Resolution read backwards)
RenamePlan      a complete, validated edit set + preconditions  (nazm-service/src/rename.rs)
LSP adapter     prepareRename, rename, versioned WorkspaceEdit  (nazm-cli/src/lsp.rs)
```

> **Rename never resolves by spelling and never uses a text search to complete a semantic
> occurrence set.** An entity whose occurrences are not all recorded is not renamed.

**Type names now have identity (N18A).** Every user-defined type's name is resolved at one of
two places in the checker — `resolve_written_inner`, for a type written in full (a parameter,
a return, a record field's or a payload field's type, a type argument at any depth, a call's
explicit type argument), and `Checker::resolve_type`, for a construction's head and the enum
before the `.` of a variant construction or a pattern. Both now record the answer they already
reached (`Resolution::record_type_name`, `TypeNode` by span), so a written type is an
occurrence of the record or enum it names, and `ReferenceTarget` gains `Enum(EnumId)`. In
`State.Ready()` the qualifier is the enum and `Ready` the variant — two occurrences, two
entities. A built-in type and a type parameter are not recorded: neither is a definition a
program wrote. Definition and references answer on written types and qualifiers through the
same lookup as everything else — N16's type-annotation gap, closed as a consequence.

**Coverage is explicit, and gated.** `ReferenceTarget::rename_coverage` states, per kind, that
every position the grammar gives it is recorded; it is an exhaustive `match`, so a new kind
cannot be added without answering. A new *position* for an existing kind is held by a corpus
gate (`every_name_token_of_the_corpus_is_indexed_or_is_not_an_entity`): every identifier token
of every clean compilation of the repository's own source is either indexed or one of a short,
syntactically recognised list of non-entities — a built-in type or call, a type parameter, `_`.

| Kind | Declaration | Uses recorded | Rename |
| --- | --- | --- | --- |
| function | its name | every call, `spawn` included | complete |
| local | parameter, `let`, pattern binding | every mention and assignment | complete |
| record | its name | constructions; every written type (parameter, return, field, payload, type argument at any depth) | complete |
| enum | its name | every written type; the qualifier of every variant construction and pattern | complete |
| variant | its name | the name after `.` in every construction and pattern | complete |
| field | its name | projections, assignment paths, construction labels | complete |
| payload field | its name | construction labels, pattern labels | complete |

**The safe scope.** A plan's occurrence universe must be provably complete, and two scopes are,
from the language's own rules: a **local**, written only inside its function's body; and a
**private** top-level entity with what it owns (a private record's fields, a private enum's
variants and payload fields), which only its own module can name — resolution binds another
module's names only to an import's exports, and `N0337` refuses a public item that would expose
a private type, so no value of one reaches another module. Both put every occurrence in the
declaring file, which a clean analysis resolved in full. An **exported** entity is refused: any
file under any source root may import its module, the service knows only the programs rooted at
open documents, and a directory an editor opened is not a project. No manifest is invented to
make it one. A **core-prelude** entity is refused as toolchain-owned; a built-in or a type
parameter is not an entity.

**The plan.** `LanguageService::rename(path, version, offset, new_name)` returns a `RenamePlan` —
the entity's kind, old and new names, the service generation, and for each file edited its
path, the open document's version (or none), the **exact text** the plan was computed from, and
the edits, each `start..end` with the bytes it expects and its replacement, ordered and
disjoint. The exact text rather than `nazm-cache`'s `SourceFingerprint`: the service may not
depend on the cache (`rules.rs`), and the bytes themselves, which it already holds, are a
stronger precondition than a digest of them. `plan_is_current` holds only while the generation,
every version and every text are unchanged. The service writes nothing.

**Validation.** The new name must be one identifier by the lexer every compilation uses (not a
keyword, not `_`, not empty, not punctuation). The program must be clean before: a resolution
with errors is not a complete account of uses. Then the candidate — the buffers with the edits
applied in memory — is loaded and checked by the same loader and checker, once for every open
compilation containing the edited file, and must have no diagnostics, which is where every
collision (two definitions, a built-in's name, one scope's two bindings) is found — the rename
engine decides none of them. And its occurrences must group into entities exactly as the old
program's did, with the old offsets moved past the edits: same groups, same kinds, ids ignored.
So a rename that captures a use into another binding, merges or splits entities, or loses a
resolution is refused though it checks. A top-level rename changes the entity's `DefKey` and
may change its module's `InterfaceHash`; that is what renaming means, and neither is compared.

**The protocol cannot carry the plan's precondition, so the adapter refuses what it cannot
protect.** LSP can tie an edit to an open document's **version**; a `null` version means "the
disk is the master", with no check of what the disk holds, and there is no content precondition
for any file. So `rename` answers only with versioned `documentChanges` (never `changes`), only
to a client that declares `workspaceEdit.documentChanges`, and only when every file the plan
edits is an open document; otherwise it is refused with `RequestFailed` and the reason. Every
rename N18 allows edits only its declaring file, which is the document asked about, so the
closed-file refusal is the rule's guarantee rather than a case that arises today.
`prepareRename` returns the token's range and spelling, or `null`.

**Not here.** Rename of an exported or prelude entity, any other code action, an edit command
(`nazm rename --apply` was not needed and does not exist), a general edit engine, and any
schema: rename is standard protocol traffic.

## 7.21 Scope at a position, and completion — settled by N19, 2026-09-25

> **Completion does not resolve names. It enumerates the semantic candidates the checker
> recorded as available.**

```text
Checker          decides what is visible where — its frames, its lookups, its environments
ScopeTrace       records those decisions as the checker makes them   (nazm-sema/src/scope.rs)
CST              says which kind of name a position expects           (the node around the token)
LanguageService  intersects the two, for the current document only    (nazm-service/src/completion.rs)
LSP adapter      renders ordered CompletionItems, UTF-16 at the edge  (nazm-cli/src/lsp.rs)
```

**What the checker records.** `Resolution::scopes()` is a `ScopeTrace`, written at the points
the checker already acts: every scope it pushes — a function's parameters (`f.span`), every
block (`b.span`: a body, an `if` branch, a `while` body, `scope { … }`, a block expression) and
every match arm (`arm.span`) — with its parent; every binding it defines, with **the byte it
becomes visible from** and **the binding its own lookup found under that name** at that
moment, which it now hides; each definition's type parameters over the definition, with the
type name each hides (again the checker's lookup); and each module's environment as the checker
built it — built-ins (`Intrinsic::ALL`), own and directly imported functions, own and directly
imported types, the built-in types (`Type::ALL`, `Vec`) — **less every name it refused** as an
ambiguous import or a collision with a local definition.

| Scope | Opens | Closes | What enters it, visible from |
| --- | --- | --- | --- |
| function | its declaration | its end | each parameter, after its name |
| block | `{` | `}` | each `let`, after the end of its statement — never in its own initializer |
| match arm | the pattern | the arm's end | each pattern binding, after its name; never a sibling arm |
| module | — | — | every function and type in the module's environment, in any order (declarations are collected before bodies) |
| type parameters | a function, record or enum | its end | its type parameters, in type positions only |

**Reading it back resolves nothing.** `ScopeTrace::visible_at(module, file, offset)` finds the
innermost frame around the position and walks its parents; a binding is visible if it was
introduced at or before the position; one is hidden if a visible binding recorded that it
hides it; a binding the checker refused as a same-scope duplicate names nothing, and neither
does the one it collided with. No name is compared with another anywhere in the file, which a
unit test holds. The answer is three lists by namespace — values (locals), callables (functions
and built-ins) and types — because the language keeps them apart: a record and a function may
share a spelling, and there are no function values.

**When the scope is complete.** The checker saw a recovered tree wherever N15 recovered, and a
recovered statement may have been a declaration it never saw. So a position's **locals** are
offered only when the function around it parsed without recovery, and a module's **functions
and types** only when no file of the compilation needed recovery (`Analysis::syntax_errors`).
Otherwise there is no answer (`NoCompletion::RecoveredSyntax`) rather than a list that could
be missing a name. A type error, an undefined name or a refused duplicate is not a recovery:
the checker's scope is still its own, and completion answers from it. A stale version is never
answered, and the last clean analysis is never consulted.

**The service.** `LanguageService::scope_at(path, version, offset)` answers at any position, not
only on a name. `LanguageService::completion(path, version, offset)` completes the identifier
under the cursor: the node around it chooses the context — `NameRef` or an assignment's root
is a **value**, `CallExpr`'s head a **call**, `Type` a **type** — and the candidates are that
namespace's, filtered by exact byte prefix of what is written up to the cursor, ordered by
name then identity, each with the hover-style detail the resolution already has and its
declaration's location. An accepted completion replaces the whole identifier. It is one
document's compilation: another open program's names never enter it.

**Deliberately not here.** Completion of a field after `.`, a variant after `E.`, a construction
or payload label, a pattern, an import path; auto-import; trigger characters; ranking; fuzzy
matching; signature help. Nothing is persisted: the trace lives and dies with its analysis.

## 7.22 The call at a position, and signature help — settled by N20, 2026-09-26

> **N20 does not perform call resolution. It reports call resolution that already happened.**

```text
CST              which call's argument list holds the position, which argument   (the tree's nesting)
Checker          which callee the call resolved to, the signature its arguments   (nazm-core/src/check/)
                 were checked against, what a generic call settled
CheckedCall      records those as the checker decides them                         (nazm-sema/src/resolve.rs)
LanguageService  joins the two, for the current document only                     (nazm-service/src/signature.rs)
LSP adapter      renders one SignatureHelp, UTF-16 at the edge                     (nazm-cli/src/lsp.rs)
```

**What the checker records.** `Resolution::checked_call(callee_span)` is a `CheckedCall`, written
wherever the checker checks a call or a `spawn` against a callee it resolved — whether or not
the arguments then fitted: the `Callee` (`Function(FnId)` or `Builtin(Intrinsic)`), the call's
and each parsed argument's span, and the parameter and return types **exactly as the module's
environment gave them** — a function's own signature, an import's from the interface it was
checked against, a built-in's from `Intrinsic::ALL`. For a generic callee it adds an
`Instantiation`: whether the type arguments were written, each one the checker settled (`None`
where it settled none, which it has reported), and the parameters and result with the settled
arguments in place, computed by `UserTypes::substituted` — which reads the type table and adds
nothing to it, so recording changes no type id and nothing a backend emits. A call whose
callee did not resolve is not recorded. Keyed by the callee's span, like `Resolution::callee`,
and session-local; it is not a call graph and nothing about compilation reads it.

**Which call, which argument.** The service walks down the one path of tree nodes containing
the position and keeps the last call or `spawn` whose argument list holds it — strictly after
its `(` and no later than its `)`. On the callee, before the `(`, and after the `)` the
position is in no call. The active argument is the number of that `ArgList`'s **own** `,`
tokens ending at or before the position: a comma of a nested call, a construction, a
parenthesised expression or a type argument list belongs to another node, and one in a string
or a comment is not a comma token. Then the call is looked up by its callee's span; no record
means no answer (`NoCall::NotChecked`) — a callee that did not resolve, whatever it is spelled.

| Position | Answer |
| --- | --- |
| on the callee, or before `(` | none |
| just after `(`, before the first argument | argument 0 |
| inside an argument, or just before the `,` after it | that argument |
| just after a `,` | the next argument |
| inside a nested call's argument list | the nested call |
| just before `)` | the last argument (or 0 when there are none) |
| after `)` | none — not the nearest call |

**When there is an answer.** One document at its current version, from its own compilation;
never an earlier clean one. None when any file of the compilation needed syntax recovery
(`NoCall::RecoveredSyntax`, the rule N19 applies to module names): a lost declaration could
have changed what the call resolved to. Too few arguments, too many and wrong argument types
do not stop it — the checker still resolved the callee. The active *parameter* is the active
argument when the callee has a parameter there, and none otherwise: for a callee with no
parameters, and for an argument past the last one.

**One signature, one spelling.** `Signature` is structured — callee, name, type parameters with
whether each requires `Equality`, parameters with their declared names and types, result — and
`Signature::render` is the only place a signature becomes text: hover, completion's detail and
signature help all call it, so they cannot disagree. A function's parameter names are its first
slots, which the checker bound; a built-in's are not named by the inventory, so it shows types
only (`fn str_len(Str) -> Int`). Rendered with a call's instantiation, the settled arguments
replace the parameters: `fn identity[T](x: T) -> T` is shown at `identity(1)` as
`fn identity[Int](x: Int) -> Int`; the declared form, with `[T: Equality]`, is what hover shows.

**The protocol.** One `SignatureInformation`, parameters as offsets into the label in UTF-16,
`activeSignature` 0; triggered by `(` and `,`, with no retrigger characters. Past the last
parameter of an over-long call the answer is `null`, because the protocol reads an absent active
parameter as the first; a callee with no parameters has its signature and no active parameter.

**Deliberately not here.** Signature help for a record construction or a variant construction
(neither is a call; each would be a label contract of its own); documentation; overloads, which
the language does not have; call hierarchy. Nothing is persisted: the records live and die with
their analysis.

## 7.23 Structure at a position — settled by N21, 2026-09-26

> **N21's language-service and LSP layers never resolve fields, variants, records, enums or
> types by spelling.** Structure queries consume semantic identities recorded by the checker.
> The one incomplete-source exception is `E.name` before `(` exists: after `E` fails as a value,
> the checker itself consults its existing module type environment and may record a non-generic
> enum as a tooling anchor. This is the canonical checker environment, not a second resolver.

```text
CST              which structured site the position is in:                (the node around the name)
                 a field after `.`, a variant after `E.`, a label
Checker          which record, enum or variant that site belongs to,      (nazm-core/src/check/)
                 and every field's and variant's identity
StructureSite    joins the two, for the current document                  (nazm-service/src/structure.rs)
Completion       enumerates that record's fields, enum's variants or
                 variant's payload fields
Constructor help renders the construction's record or variant shape
LSP adapter      byte spans to UTF-16                                     (nazm-cli/src/lsp.rs)
```

**What was already there.** Every name the checker resolved inside a structure is already in the
resolution, keyed by its span: a projection's or an initialiser's field (`FieldRef`), a
construction's record (`construction`), a variant construction's or pattern's variant
(`VariantRef`), a payload label (`PayloadRef`). A record or enum identity of an applied generic
is its **instance**, whose fields and payloads the type table substituted once, when the
instance was made. So a site whose name resolved needs nothing new: `Pair[Int, Str]`'s `first`
is `first: Int` because the instance says so.

**What was added to the checker.** One narrow fact, `Resolution::looked_up_on(span)`: the type
a member name **that did not resolve** was looked for on — the record an unknown field was
looked for on (`check_projection`), the enum an unknown variant was looked for in
(`find_variant`, for constructions and patterns alike), and, for `E.name` written before its
`(` exists, the enum `E` names in the module's types when `E` is no value and the enum is not
generic. It is written only where a lookup failed, so a program that checks holds **none** —
there is no per-expression type table, no copy of any definition, and no index. The
diagnostics are unchanged: the last case records what the checker's own environment says and
reports exactly what it reported before.

**The sites.** `LanguageService::structure_at(path, version, offset)` finds the node around the
identifier and answers only where the name is a member position:

| Site | Anchor | Candidates |
| --- | --- | --- |
| a projection's or an assignment place's field, after `.` | the field's record, or the record it was looked for on | the record's fields |
| a variant construction's or pattern's variant, after `E.` | the variant's enum, or the enum it was looked for in | the enum's variants |
| `E.name` with no `(` yet | the enum `E` names, when not generic | the enum's variants |
| a record construction's label | the construction's record | its fields, less those given elsewhere |
| a variant construction's or pattern's payload label | the variant | its payload fields, less those given elsewhere |

A label given elsewhere is excluded **by the identity the checker recorded for it**; a label
that did not resolve excludes nothing, and the label under the cursor is never counted as
given — it is the one being replaced. A pattern's binding (`code: c`'s `c`) is a local, not a
label, and is N19's. Candidates keep their identities (`Candidate::Entity(Field | Variant |
PayloadField)`) and come in the definition's **canonical** order — fields and payloads by
`in_layout_order`, variants by `in_tag_order`, both sorted by name, the order the backend
already uses — because declaration order means nothing in this language, and a tool that showed
it would make an edit with no meaning observable. Details are the field's type (`value: Int`)
or the variant's payload shape (`Done(code: Int)`), from the instance.

**Constructor signature help** is not function signature help: a construction has no callee
and runs no function, so it is its own `Constructor::Record | Variant`, never a `Callee` or a
`CheckedCall`. `LanguageService::help_at` finds the innermost call and the innermost
construction by the same rule — strictly after the `(`, no later than the `)` — and answers the
one whose parentheses open later: a call written in an initialiser is answered as the call, a
construction written as an argument as the construction. The label is the instance's name and
its fields in canonical order — `Point(label: Str, x: Int)`, `Box[Int](value: Int)`,
`Maybe[Int].Some(value: Int)` — rendered by the same list renderer as a function's parameters.
The active parameter is **the field whose initialiser holds the position, by its label's
identity**, at its canonical position; initialisers are named and in any order, so how many
precede it says nothing. Between initialisers, and on a label that did not resolve, no field is
active; the protocol then gets the label without parameters, because an absent index would
mark the first. `call_at` and `signature_help` are N20's, unchanged: they answer the innermost
call.

**When there is an answer.** One document at its current version, from its own compilation;
never an earlier one. A candidate list comes from a definition and its anchor from the
checker's reading of the site's function and of the module's names, and both are complete when
every other file of the compilation parsed without recovery and every recovery in the document
lies inside the body of a function other than the one the position is in — a recovered
statement there can neither lose a declaration the module's names need nor change a local this
site reads. A recovery at the top level, in a declaration, in the site's own function or in
another file refuses (`RecoveredSyntax`); a site the checker anchored nowhere refuses
(`Unanchored`) whatever it is spelled.

**Deliberately not here.** Completion with nothing written after `.` or `(` (the source does not
parse, so nothing anchors it) and therefore no `.` trigger character; methods, which the
language does not have; import paths, auto-import, ranking; documentation. Nothing is
persisted.

## 7.24 Document and workspace symbols — settled by N22, 2026-09-26

> **A symbol is a declaration the current analysis declared. Symbols are never reconstructed
> by scanning identifier spellings, and workspace symbols do not establish a complete Nazm
> project boundary and therefore do not close N18's exported-rename limitation.**

```text
Checker          which declarations exist, and what each is:          (nazm-sema definition tables)
                 FnId, RecordId + field, EnumId + variant + payload field
Parsed items     where each is written: the item the table's `decl`   (nazm-syntax Program, via Analysis)
                 index names, and the child whose name span matches
Symbol view      source-backed declarations, nested, derived on demand (nazm-service/src/symbols.rs)
LanguageService  which files are analysed now: every file of every
                 compilation rooted at an open document
DocumentSymbol   one file's declarations, in source order
WorkspaceSymbol  every analysed file's, once each, filtered by a substring
LSP adapter      protocol kinds, nesting or its flat form, UTF-16        (nazm-cli/src/lsp.rs)
```

**What was already there.** Everything. The checker's definition tables hold every declared
function, record, enum, variant, field and payload field with its identity (the same
`ReferenceTarget` references and rename use) and its `name_span`; each function, record and
enum also carries `decl`, its index in the parsed `Program` the resolution was produced from, so
its item — and the item's span, the whole declaration — is one index away. A field, variant or
payload field is found inside its parent's item by its name span: a position, never a spelling.
N22 added no checker fact, no table and no index. An applied generic's instance is not a
declaration and is skipped, as the reference index skips it.

**What a symbol is, exactly.** It exists when the checker declared something:

| Source | Symbol? |
| --- | --- |
| a function, record or enum the checker declared, private or `pub` | yes, with its visibility kept as metadata |
| a function whose body did not parse (the parser keeps its signature) | yes; its range ends where the parser's item does |
| a body with type errors | yes: existence is not a clean body |
| an item the parser could not read | no — nothing was declared, and an outline of text is not this contract |
| a second function, type, variant or field the checker refused as a duplicate | no; the first keeps its symbol |
| a function that collides with an import | yes, in its own file; the import's is in the dependency's |
| locals, parameters, pattern bindings, type parameters | no: an outline is structure, and scope is N19's |
| built-ins, intrinsics, the core prelude's `Option` and `Result` | no: there is no file an editor can open |
| a definition rehydrated from a persisted interface | no: its spans are synthetic and its `decl` is none |

The language service never checks against a persisted interface — it reads source or reports
the module missing — so the last row is guarded twice rather than reachable: a synthetic span
is never a symbol, and an item must sit at `decl` with the same name span.

**Range and selection.** A symbol's range is the parsed item's span (from `pub` or the keyword
to the closing brace; a field's `name: Type`; a variant's name to its `)`); its selection is the
table's `name_span` — the span definition goes to from any use, and the span the reference
index records the declaration at, so the two cannot disagree. A symbol's range holds its
selection and a parent's range holds each child's.

**Order.** A document's symbols are in **source order** — the order the author wrote them —
at every level. That is deliberately not N21's canonical order: declaration order means nothing
to what a record or enum *is*, which is why candidates and constructor help are canonical, but
an outline presents the file, and moving two fields does move them in it.

**The workspace universe.** `LanguageService::workspace_symbols(query)` covers every file of
every compilation the service holds at the current generation: each open document's
compilation, loaded exactly as `nazm check <it>` would load it, with everything it imports.
That is the union of the open roots and their loaded dependency graphs — **not** a directory:
there is no filesystem walk, no manifest, no persistent index and no watcher, and a `.nz` file no
open document's compilation loads is never read. Unlike completion, definition and signature
help, which belong to the one requesting compilation, it deliberately spans every open root,
and it does not apply import visibility: the question is what exists, not what one file may
name.

**One declaration, however many compilations load it.** Identity does not cross
compilations, so a declaration is carried across them as N17 carries an entity: the same file
(by identity path), the same kind, the same name span. `DefKey` is not used: its module key is
measured from each compilation's source root, so one file can have two keys in two
compilations, and one reached through a symbolic link has none. Deduplication is by that
position and never by spelling: two modules' `helper`, a record and a function both called
`Box`, two records' `value` and two enums' `Ready` are all distinct.

**The query.** A case-sensitive substring of the declared name; the empty query is everything.
It filters a universe already built — it loads, resolves and discovers nothing — and there is
no fuzzy matching, case folding, ranking or truncation. Results are ordered by name, then file,
then where the name is, then kind. Fields, variants and payload fields are included, each with
the name of what it is declared in (`Point`, `State`, `State.Done`), read from its parent.

**When there is an answer.** Document symbols are of one document at its current version,
from its own compilation, and a superseded version has none (`NotCurrent`); workspace symbols
are of the current generation only. Both see unsaved buffers at once, and closing a buffer makes
the disk authoritative for it again. Neither refuses because of a recovery: a symbol is what
the checker declared, and a recovery can only remove declarations, never invent one.

**Over the protocol.** `textDocument/documentSymbol` returns nested `DocumentSymbol`s to a client
that declares `hierarchicalDocumentSymbolSupport` and flat `SymbolInformation`s, each naming its
container, to one that does not. `workspace/symbol` returns `WorkspaceSymbol`s located at the
declared name; `resolveProvider` is `false` and there is no `workspaceSymbol/resolve`. Kinds:
function → Function, record → Struct, enum → Enum, variant → EnumMember, field and payload field
→ Field.

**Deliberately not here.** Locals in an outline, a syntax outline of text that did not declare
anything, qualified-name queries, fuzzy search, a project index, symbol tags, semantic tokens,
call hierarchy and any agent-facing export. Nothing is persisted.

## 7.25 Semantic identifiers and semantic tokens — settled by N23, 2026-09-26

> **N23 never classifies an unresolved identifier by syntactic position or spelling. N23
> semantic tokens are a semantic-identifier layer, not a replacement for lexical syntax
> highlighting.**

```text
Checker / Resolution  what each name at each span is                    (nazm-core/src/check/)
ReferenceIndex        entity occurrences, declarations apart (N17)      (nazm-sema/src/references.rs)
Resolution            built-in calls; written built-in and type-
                      parameter names (new, N23)                        (nazm-sema/src/resolve.rs)
SemanticIdentifier    identity + class + role + exact span, per span     (nazm-service/src/semantic.rs)
LanguageService       one current document's identifiers, source order
LSP adapter           static legend, UTF-16, relative encoding           (nazm-cli/src/lsp.rs)
```

**The rule.** The document's concrete tree lists its identifier tokens; each is classified by
what the compiler recorded at exactly its span, and by nothing else:

| Fact at the span | Class | Role |
| --- | --- | --- |
| an entity in the reference index | its kind; a local is a **parameter** when its slot is one of its function's parameter slots, which the checker allocates first | declaration when the index records the entity declared at this span |
| a call to a built-in (`Callee::Builtin`) | intrinsic | reference |
| `WrittenType::Builtin` | built-in type | reference |
| `WrittenType::Param` / `ParamDeclaration` | type parameter | reference / declaration |
| nothing | not an identifier | — |

So an unknown call head is not a function, an unknown member after `.` is not a field, and a
word in a comment or a string, a keyword or a name in a body that did not parse is nothing.
Keywords, comments, strings, numbers, operators and punctuation are an editor grammar's.

**What was already there.** Every program entity: functions, locals (parameters among them),
records, enums, variants, fields and payload fields, declarations and uses, each by the
identity references and rename use — N17's index, read by span. Built-in calls, in the
resolution's call map. Parameterhood needed nothing new: `LocalDef` slots are allocated
parameters first, and a slot below the function's parameter count is a parameter.

**What was added to the checker.** One narrow record, `Resolution::written_type(span)`: the
written type names the checker resolved to **something no program declares** — a built-in type
(as the `Type` resolution decided, `Int` as `Type::Int`, the `Vec` of `Vec[Int]` as its vector
type) or a type parameter (as its `TypeParam`: owner and position) — and each type parameter's
declaration in its `[…]`. It is written at the two points that already decided these names
(`resolve_written_inner` and the declaration of a definition's parameters), kept apart from the
reference index because neither is an entity, and measured in `performance.md`. A function the
checker refuses for redefining a built-in never has its body checked under an id, so the
parameter uses resolved in its signature are forgotten rather than left under the id the next
function receives; a refused *duplicate* is checked as the first definition, as it always has
been, and its names are classified as the checker made them — its parameters as bindings of the
first function after that function's own.

**Roles and modifiers.** A declaration is marked `declaration`; a use is not; nothing is both.
`defaultLibrary` marks exactly what is compiler-owned — intrinsics and built-in types — and
never a source declaration, the core prelude's included: `Option` is an enum like any other.
There is no `definition`, `readonly` or `modification`: an assignment is a reference.

**The legend**, fixed at compile time and pinned by a test: token types `type`, `struct`,
`enum`, `typeParameter`, `parameter`, `variable`, `property`, `enumMember`, `function`, in that
order; modifiers `declaration`, `defaultLibrary`. Function → function, intrinsic → function +
defaultLibrary, parameter → parameter, local → variable, record → struct, enum → enum, variant →
enumMember, field and payload field → property, type parameter → typeParameter, built-in type →
type + defaultLibrary. Two display kinds are one LSP type; their identities are not merged.

**Encoding.** One identifier token is one span, so no span is classified twice, and tokens come
in source order. The adapter converts each span to a line and UTF-16 columns with the one
`LineIndex`, and encodes relatively — lines since the previous token, start column relative to
the previous token on the same line and absolute on a new one, length in UTF-16 units. It
**refuses** (an error response, never a token array) an identifier list that is out of order,
overlapping, empty or spanning lines.

**When there is an answer.** One document at its current version, from its own compilation;
another open program never changes it, and a superseded version has none (`NotCurrent`).
Unsaved buffers take effect at once and closing one restores the disk. Unlike completion, a
recovery does not refuse the whole view: what the checker resolved is classified and what it
did not is omitted, and nothing from an earlier version is ever served.

**Deliberately not here.** `semanticTokens/range` and `semanticTokens/full/delta` (no
`resultId`, no previous token array, no cache); lexical tokens; read/write distinctions; a
workspace-wide token query. Nothing is persisted.

## 7.26 Quick fixes from current diagnostics — settled by N24, 2026-09-26

> **N24 does not derive repairs from diagnostic codes, messages, spelling or syntax. It only
> presents fixes already attached to current compiler diagnostics. Returning a
> `WorkspaceEdit` is not applying it; the client must send the resulting `didChange` before
> server state changes.**

```text
Compiler diagnostic   owns its fixes                                     (nazm-diag Fix)
Fix                   one span (one file), replacement, applicability,
                      precondition, description
LanguageService       current diagnostic · current version and generation ·
                      open document · exact bytes at the span           (nazm-service/src/fixes.rs)
FixPlan               the fix as one validated, versioned edit
LSP adapter           quick fix · versioned documentChanges · UTF-16     (nazm-cli/src/lsp.rs)
```

**What a fix is.** `nazm_diag::Fix { span, replacement, description, applicability,
precondition }`: one span, so one edit to one file. `Applicability` is `Automatic` or
`NeedsReview`, and the precondition is always present. It carries no expected text; the bytes
at its span in the analysed source are what it expects, and both consumers record them when
they take the fix.

**The plan.** `LanguageService::fix_plans(path, version, start, end)` reads the diagnostics of
the document's current analysis, keeps those in the document whose primary span the range
touches (half-open overlap; a cursor touches a span it is inside or at either end of; an empty
span, a range containing it), and makes one `FixPlan` per fix: the diagnostic, which fix of how
many, the applicability, description and precondition, the generation, and one N18
`FileEdit` — path, open version, exact text, and the edit with its expected bytes. The fix's
span decides the edit, the diagnostic's span only relevance: `mut` goes on the `let`, not on
the assignment. A fix whose file is not an open document has no plan — there is no version to
protect it — and neither does one whose span is not a range of the text. One fix has one edit,
so a plan cannot hold overlapping edits; two fixes of one diagnostic are two plans, which may
overlap as alternatives, and nothing combines them.

**Freshness.** `fix_plan_is_current` holds a plan to its generation, to its file being open at
its version with exactly its text, and to its bytes still being the ones it replaces — through
`file_is_current`, the one freshness law a rename's plan answers to as well. Version and bytes
protect different boundaries and both are kept: the bytes, what the server analysed; the
version, what the client applies to.

**Over the protocol.** `codeAction` returns `CodeAction`s of kind `quickfix`, one per plan, each
with the current diagnostic converted exactly as published and its edit as versioned
`documentChanges` — the same conversion, and the same refusal, a rename uses: a client that does
not accept versioned changes, or a file not open, gets no action rather than a weaker edit. An
automatic fix is `isPreferred` when it is its diagnostic's only fix; a needs-review fix never
is, and its title carries its precondition. The diagnostics a client sends are not consulted:
a stale copy cannot resurrect a fix and a fabricated one cannot produce one. `only` asking for
anything but quick fixes gets an empty list. No refactors, source actions, fix-all, commands,
`data` or `codeAction/resolve`; nothing is cached between requests.

**`nazm fix` beside it.** The command collects the same fixes and applies the automatic ones to
disk right to left, skipping overlaps, stale bytes, spans that name no range, and a result that
would not parse, with each of its seven skip reasons now reached by its own test. It writes and
can roll back; the server writes nothing and cannot — it relies on each fix being tested to
parse and to remove its diagnostic, on the bytes being checked, and on the version.

**Deliberately not here.** Edits to files that are not open (N18's gap stands), fix-all,
refactors, and any fix the compiler does not attach.

## 7.27 Call hierarchy from checked calls — settled by N25, 2026-09-26

> **N25 call hierarchy is a view of the current bound compilation. It does not establish a
> complete project boundary and does not prove that all possible callers are loaded. An edge
> exists only because the checker recorded a call from one source function to another; none
> is inferred from names, references, syntax or text.**

```text
N17  occurrence -> function identity                      (ReferenceIndex)
N20  checked call -> callee identity                      (CheckedCall, by callee name span)
     the body being checked -> the calls it made          (FnDef::call_sites, the same point)
N22  function identity -> source declaration and name     (symbols::function_symbol)
N25  current source function
       -> direct checked-call edges, grouped by the function at the other end
       -> source-backed hierarchy items                    (nazm-service/src/hierarchy.rs)
LanguageService   binds an item to one current compilation and generation (Locator)
LSP adapter       opaque locator in item data · UTF-16 · the three standard methods
```

**Items.** A hierarchy item is a function the checker declared from an item this compilation
parsed, with the declaration and name ranges of its outline symbol: the outline and the
hierarchy read both from one function, `function_symbol`, so there is no second location
table. A built-in or intrinsic has no declaration, the core prelude declares no function, and
the service loads every module from source, so a function known only from a persisted
interface does not occur in its compilations; the item builder refuses anything without a
parsed item in a file an editor can open, and an edge to it is left out rather than given a
place that does not exist. Records, variants, fields, locals and parameters are never items.
Generic instantiations are one item: instantiation is call metadata, not identity.

**Preparing.** `prepare_call_hierarchy(path, version, offset)` reads the entity the reference
index records at the name under the cursor — a function's declaration or any call of it — and
nothing else. It never looks a function up by its spelling; a record called `Box` is not the
function `Box`. Nazm has no function values, so every non-declaration occurrence of a function
is a call or a spawn; preparation answers *which function*, and edges answer *which checked
calls*, separately.

**Edges.** Each is read from two facts the checker already keeps, both recorded at the same
point under the same resolved callee: a `CheckedCall` at the callee's name span (N20), and the
same span in the `call_sites` of the body the checker was checking. So the caller is read, not
derived from where a span sits; no per-call state was added. Incoming calls scan every body's
call sites and keep those whose checked call resolved to the item — O(calls in the
compilation), with a request-local map from caller to spans that is dropped with the answer.
Outgoing calls read the item's own call sites — O(its calls). A call site with no checked call
is no edge (a refused generic `spawn` resolves its name and checks no call), a reference is not
a call, and a construction is not a call to the checker and is not one here. Calls nested in
another call's arguments, or inside `if`, `while`, `match` or a block, belong to the function
whose body holds them. A `spawn f(…)` is a checked call of `f`. A recursive call is an edge to
itself; mutual recursion is an edge each way; nothing is transitive — one level per request.

**Grouping and order.** Edges are grouped by the function at the other end, by identity, never
by name: two modules' `helper` are two callers. Each edge's ranges are the callee's name at each
call — the span N17 and N20 key the call by, without type arguments — in source order, each
once. Edges are ordered by the other end's declaring file, then where its name is, then name.

**Which program.** An item belongs to the compilation rooted at the document it was prepared
from, at the generation it was prepared at: callers are the calls known in that compilation —
the root and everything it imports — and no other. A shared file prepared through two open
roots gives two items at the same place with different callers; neither is a claim about the
project, and nothing reads a directory. So N25 does not lift N18's refusal to rename an
exported definition.

**Across requests.** An item carries a `Locator`: the root document's identity path, its
version, the service generation, and the declaring file and name bytes — a position, never a
session-local `FnId` or a spelling. Incoming and outgoing calls are answered only while the
generation, the root's presence and version all still hold, and then from that root's own
current analysis, where the function is found at exactly those bytes. After any change —
unrelated ones included — or once the root is closed, the item has no answer and is never
rebound to another root; it must be prepared again. Nothing is kept between requests: no item
table and no graph.

**Over the protocol.** `callHierarchyProvider` is advertised. An item's `range` is the whole
declaration and `selectionRange` its name, kind `Function`, detail the shared signature
renderer's, and `data` the locator's six fields and nothing more — opaque, and not a Nazm
machine schema. Missing, forged or stale data is answered `null`. Byte ranges become UTF-16
only in the adapter, through one line index per file text per response.

**Deliberately not here.** A persistent or whole-program call graph, transitive closure,
filesystem discovery of callers, and any claim that every caller in a project is known.

## 7.28 Completion at a structured hole — settled by N26, 2026-09-27

> **N26 does not infer a receiver, record, enum, variant or construction from spelling.
> Incomplete-source completion is available only where the canonical compiler can establish
> one semantic anchor.** `point.`, `State.`, `Point(` and `State.Done(` remain invalid
> programs; `nazm check` reports them exactly as before.

```text
Lossless CST / parser     the exact hole: a `.` or `(` token ending at the cursor
                          (a probe parse: nazm-syntax parse_both_at / parse_map_at)
Canonical checker         the anchor, at decision points it already has   (nazm-core probe)
N21 candidate machinery   real FieldRef / VariantRef / PayloadRef candidates (structure::candidates)
LanguageService           current version · the probe · zero-width completion (incomplete.rs)
LSP                       "." trigger · UTF-16
```

**Why a probe, not a record in every analysis.** An unfinished statement makes the parser
drop its function's body, so the ordinary analysis has no receiver, local or scope there — and
which `.` or `(` is being completed is decided by the editor's cursor, which only the request
knows; a `.` elsewhere is just broken code. So a completion request at a hole re-parses and
re-checks the compilation's current sources — the same `SourceMap` and module graph the
document's analysis loaded, overlays included, never the disk — with the parser told the
cursor's byte offset (`nazm_core::probe`). Nothing is inserted into any text. At exactly that
offset, right after the `.` or `(`, the parser reads a zero-width, empty name, which no source
can spell, and takes the `;` or `)` the unfinished statement leaves unwritten as written, until
that statement ends; a pattern's hole with no `=>` gets a zero-width body. Everything else is
parsed and reported as ever. The ordinary parse never takes this path: a parse with no hole
offset is unchanged, and a probe parse at an offset that is no hole is the ordinary parse
(`crates/nazm-syntax/tests/holes.rs`, over the corpus).

**The anchor.** The checker decides, at points it already had:

| Hole | Anchor |
| --- | --- |
| `receiver.` | the type an unknown member was looked for on (N21's record) — a record's fields |
| `E.`, `E` naming an enum and no value | the enum, when not generic (N21's rule; a value named `E` wins, as for any `E.x`) |
| `E[T, …].` | the applied enum an unknown variant was looked for in |
| `Name(` | only when the module has no function `Name`: the record its types name, resolved and applied as a construction's head is |
| `E.V(`, construction or pattern | the variant |

The one new checker behaviour is the `Name(` row, and it runs only in a probe: the probe's
`Resolution` carries the hole's callee, and only when that call finds no function does the
checker resolve the name as a construction head and record the construction. In a probe, a
type name the module refused as an ambiguous import anchors nothing. An ordinary compilation
records nothing new: **clean-program N26 state is zero**, and an incomplete program's ordinary
analysis is unchanged.

**A bare `(`.** Without a label, `Name(` is a call to the grammar, and the probe reads it as
one. A function `Name` makes it that call — signature help's — whether or not a record is also
called `Name`; only where no function is does a record anchor its labels. A generic record needs
its type arguments written, as a construction does.

**Completeness.** N21's rule, applied unchanged to the probe's own parse, in which the hole is
not a recovery: no other file recovered, and every other recovery in the document is inside the
body of another function. Another mistake in the hole's function, at the top level or in a
dependency, leaves no answer.

**Candidates.** `structure::candidates`, the function N21's completion uses: the anchor's
members in canonical order, identities, substituted details. Only the range differs: empty, at
the cursor — the `.` or `(` is not replaced — where a name being written is replaced whole.
Declarations are the definitions' name spans in the same files. The probe's diagnostics are never
published and its analysis is dropped with the response; nothing is cached.

**Over the protocol.** `completionProvider.triggerCharacters` is `["."]`: not `(`, which
signature help owns, nor `,` or `:`. The client's trigger is never read: whether a position is a
hole is decided from the current source, so a `.` in a string or a comment, or a trigger reported
where there is no `.`, completes nothing, and invoked and triggered requests answer alike.

**Deliberately not here.** Completion after a comma or at an empty label slot later in a list,
methods, ordinary call arguments, `(` as a trigger, ranking, snippets, and any reading of source
that would make an unfinished construct valid.

## 7.29 Semantic context packet v1 and read-only MCP — settled by N27, 2026-09-27

> **MCP does not create semantic facts. It serialises a protocol-independent packet derived
> from the compiler's current root compilation.** A packet reports what the compiler already
> knows — never effects, capabilities, tests or changes it does not have, which are marked
> unsupported rather than empty.

```text
N17  references, and every use inside a declaration      (ReferenceIndex)
N18  written names of program records and enums          (Resolution::type_name)
N20  the one function signature renderer                 (Signature)
N21  canonical fields, variants and payloads             (structure::members)
N22  declaration and name ranges, exact source           (symbols::declared_in)
N25  direct callers and callees                          (incoming_calls / outgoing_calls)
        ↓
ContextPacket, nazm.context/1                            (nazm-service/src/context.rs)
        ↓
nazm context --json                                      (nazm-cli/src/context.rs)
        ↓
nazm-mcp: one read-only stdio tool                       (nazm-mcp, rmcp)
```

**The target.** A function, record or enum the checker declared from source, found through the
entity the reference index records at a byte offset — its declaration or any use. A local, a
parameter, a field, a variant, an intrinsic or a built-in type is not a target; neither is a
position on no name. Nothing is looked up by spelling, so a record and a function called `Box`, or
two modules' `helper`, are always distinct.

**What a packet holds.** The target's durable identity (`module::kind name`, the `DefKey` the
interface already persists; null where a module has no durable key — never a session-local id),
kind, name, and a function's signature or a record's fields or an enum's variants and payloads,
by the existing renderers; its exact source, sliced from the declaration range N22 uses —
comments and all. Then compact links, each an identity, kind, name and declaration place:
**dependencies**, every entity used inside the declaration other than its own locals and itself,
grouped by identity with every occurrence; **related types**, the records and enums whose names
the checker recorded as written there — as a type, a construction's head or a variant's enum —
directly, not a closure; **references**, every use of the target, the declaration excluded;
**callers** and **callees**, call hierarchy's direct edges for a function and `not_applicable` for a
record or enum; and **diagnostics** whose primary span is inside the declaration, by code, span and
fix count. `availability` marks effects, capabilities, tests, semantic changes and semantic deltas
`unsupported`.

**Source links only.** Context packet v1 exposes source-linked program dependencies only: every
dependency, related type, caller and callee carries a declaration in a file of the compilation, and
`link.declaration` is never null. Compiler-owned, built-in, interface-only or toolchain definitions
without a navigable declaration — the core prelude's `Option` and `Result`, an intrinsic, `Int` —
may affect checking but are omitted from the link sections, rather than linked with no place. (A
link's `identity.definition` may still be null where its module has no durable key; its place is
given all the same.) Related types come from N18's record of the written names of program records
and enums; N23's `Resolution::written_type`, which records built-in types and type parameters, is
not read, since neither is a definition with a source.

**Minimum context.** The target's source is the only source text in a packet; everything else is a
place to ask about next. That is a strategy for compact context — not a claim that a packet is
sufficient for every task, or minimal in any proven sense.

**Which program.** The compilation rooted at one chosen file — its imports, and nothing else; no
directory is read and no project boundary is claimed. A compilation that needed syntax recovery, or
could not load a module, has no packet: what was lost could be a use, a caller or a declaration.
Type errors do not stop one.

**Determinism.** Paths are relative to the source root, every list has an explicit order (links by
kind, declaring file and place, then identity; references and diagnostics by position), and
nothing session-local — generation, numeric id, host path, time — is serialised; one compact
serialisation. The same sources and question give the same bytes, from any checkout.

**The CLI and the MCP server.** `nazm context` and `nazm-mcp` each open the root from disk, build
the packet and drop everything: no packet, analysis or graph outlives a request. `nazm-mcp` is its
own crate and binary on the official Rust SDK (`rmcp`), stdio only, with one tool,
`nazm.semantic_context` (`file`, `byte_offset`), whose output schema is `schema/nazm.context-3.json`
and whose result carries the packet once, as structured content, with an empty `content`;
it advertises tools and nothing else, and answers resources, prompts and completions as methods it
does not have. The root is fixed at startup; each call reads the disk as it is then, so an agent's
edit is seen at once and a broken file never gets an earlier packet. The `file` argument is only
compared with the compilation's own files by identity, so no other path — `/etc/passwd`, a file
nothing imports — is ever opened. It does not see an editor's unsaved buffers.

**Deliberately not here.** Semantic deltas, test mapping, effects and capabilities, history,
edit tools, HTTP transports, a persistent index or daemon, and any claim that every caller or
importer in a project is known.

## 7.30 Semantic snapshot manifest and semantic delta v1 — settled by N28, 2026-09-27

> **N28 reports changes in the semantic surfaces Nazm currently records. It is not a proof of
> behavioural equivalence.** Effects, capabilities and tests are unsupported, not unchanged.

```text
N17  every use of an entity, by where it is             (ReferenceIndex)
N18  written names of program records and enums         (Resolution::type_name)
N20  the one function signature renderer                (Signature)
N21  canonical fields, variants and payloads            (structure::members)
N22  top-level declarations and their exact ranges      (symbols::declared_in)
N25  the one checked-call edge rule                     (hierarchy::callee)
N27  identity, shape and source-link rules              (context::{identity_of, shape, Paths})
        ↓
SemanticSnapshot, nazm.snapshot/1                       (nazm-service/src/snapshot.rs)
        +  a baseline snapshot, as data
        ↓
SemanticDelta, nazm.delta/1                             (nazm-service/src/delta.rs)
        ↓
nazm snapshot --json · nazm delta --baseline --json     (nazm-cli/src/snapshot.rs)
nazm-mcp: nazm.semantic_snapshot · nazm.semantic_delta  (nazm-mcp)
```

**Which definitions.** Every function, record and enum the checker declared from a file of the
root's compilation — the root and what it imports, no directory read — that has a durable
definition key. `DefKey` is the only identity: no `FnId`, `RecordId` or `EnumId` is serialised, and
nothing is matched by path or name. A definition in a module outside the source root has no key; it
is left out and counted in `untracked`. A key the compilation gave two declarations would be no
identity for either and is counted the same way (the checker refuses a second declaration, so this
is a guard, not a case that occurs today). Built-ins and the core prelude's definitions are not the
root's definitions.

**What a snapshot holds.** The root's identity — its module key and the compiler (`nazm <version>`)
— then per definition its key, kind, declaration place (for navigation, not compared) and one BLAKE3
digest per section, over a length-prefixed framing that names schema and section:

| Section | Canonical data digested |
| --- | --- |
| `source` | the declaration's exact bytes |
| `shape` | kind, visibility, and the rendered signature, or the fields, or the variants and payloads |
| `dependencies` | every source-linked entity used inside (N27's rule), by kind and identity, with a count |
| `related_types` | the records and enums written by name inside, by identity, with a count |
| `references` | every use of it, by the identity of the definition the use is in, with a count |
| `callers`, `callees` | a function's direct checked calls in and out, by the identity at the other end, with a count; null for a record or an enum |
| `diagnostics` | code, severity, fix count and declaration-relative range of each diagnostic inside it — never its message |

Relationship digests take identities and counts, never byte offsets, so a line added above a
definition changes nothing below it; a reference or call is attributed to the declaration whose
range contains it, found by binary search over each file's declarations. No complete packet is
stored per definition — a snapshot is a manifest. `unsupported` lists effects, capabilities, tests,
semantic history and behavioural equivalence.

**What a digest proves.** Equal digests mean the canonical bytes above were equal, and nothing
more. `fn answer() -> Int { 1 }` and `{ 2 }` agree in every section but `source`. A digest is not
the program's meaning.

**Root identity and determinism.** The root is its module key, relative to the source root, so
the same project in another directory has the same identity and the same bytes; no absolute path,
generation, numeric id, time or random value is digested or written. Definitions are ordered by
identity; every map is ordered. The same sources and compiler give byte-identical snapshots across
calls, processes and checkouts. Two different projects whose root modules have the same key share
a root identity; a delta between them is still exact about which keys differ.

**The delta.** A baseline is data: parsed as JSON and validated before anything is compared. It is
refused — never compared as well as it can be — for another schema, an unavailable snapshot, any
field `nazm.snapshot/1` does not define or a missing one, an identity not in canonical `DefKey`
spelling or not of its entry's kind, a duplicate identity, a digest that is not 64 lowercase
hexadecimal digits, callers or callees present on a record or absent on a function, another root
module, or another compiler. Otherwise the current snapshot is derived and compared by key:
`added`, `removed` (with its baseline place), `changed` with a record of which sections differ —
`callers` and `callees` null for a type, not false — and an `unchanged` count. A rename is its old
key removed and its new key added; nothing is inferred. A comment, a reformatting or a different
literal is `source` alone.

**No state.** Snapshot and delta are derived per request from one analysis and dropped with it:
no snapshot cache, dependency graph, call graph or history outlives a call. Nothing needs, or asks,
git.

**The CLI and the MCP server.** `nazm snapshot --root FILE --json` prints a snapshot; `nazm delta
--root FILE --baseline FILE --json` reads a snapshot file the caller kept and prints the delta — a
file that is not JSON is a malformed baseline. `nazm-mcp` adds `nazm.semantic_snapshot` (no
arguments) and `nazm.semantic_delta` (`baseline`, a `nazm.snapshot/1` object), with the two schemas
as output schemas and each result carried once, as structured content. The server keeps no
previous snapshot: the client keeps the baseline and hands it back. A baseline is never a path,
so no file a client names is read. Each call reads the disk as it is then, so an edit is seen at
once and a broken one gets no snapshot or delta, never an earlier one. `nazm.semantic_context` is
unchanged.

**Deliberately not here.** Rename inference, test impact, effect and capability deltas, semantic
history, a persistent snapshot cache or index, git integration, patch application, and any claim
that equal digests mean equal behaviour.

## 7.31 Minimal semantic patch plan v1 — settled by N29, 2026-09-27

> **N29 plans edits but never applies them.** A patch is a proposal bound to exact semantic and
> source state, not permission to mutate files.

```text
N18  rename: entity at a position, declaration + uses, scope rule,      (rename.rs)
     new-name lexing, candidate re-parsed, re-checked, same partition
N24  the fix a current diagnostic carries, as one validated edit          (fixes.rs)
N28  the root's snapshot and identity                                     (snapshot.rs)
        ↓
SemanticPatch, nazm.patch/1                                               (nazm-service/src/patch.rs)
        ↓
nazm patch rename|fix --json                                              (nazm-cli/src/patch.rs)
nazm-mcp: nazm.semantic_patch, read-only                                  (nazm-mcp)
```

**Two operations, and no others.** `rename` is N18's validated plan: the entity the reference
index records at the position, its declaration and every use, nothing found by spelling, the new
name lexed as one identifier, and the candidate program re-parsed and re-checked — clean, its
occurrences grouped into entities exactly as before. Every category N18 renames: function,
local, record, enum, variant, field, payload field. `diagnostic_fix` is N24's projection of a
`Fix` a current diagnostic owns — selected by the file, a byte its primary span touches, and the
fix's index among those at that byte, in the order the compiler attached them — never by a code, a
message or a spelling, so a position with no diagnostic has no fix whatever a client believes.
Its applicability (`automatic` or `needs_review`) and precondition are carried as structured
fields, exactly as the compiler wrote them.

**Exported rename remains refused because one root compilation is not a complete importer
universe.** A snapshot of that compilation describes it; it does not make every module that may
import an exported name known. N18's refusal is unchanged and reported as `exported`.

**N29 does not alter LSP closed-file safety.** The disk plan reads the root and analyses it;
when the edit is in another file of that compilation, the request's own service opens that file
from the text the analysis read — no second read — and N18 or N24 plan against it as against an
open document. The plan may therefore describe a file no editor has open, protected by the
freshness below. `textDocument/rename` and `textDocument/codeAction` still edit open documents
only.

**Three freshness layers.** `baseline.snapshot` is BLAKE3 of the root's canonical
`nazm.snapshot/1` bytes (N28) — the semantic state of the whole compilation; `files[].digest` is
BLAKE3 of each edited file's exact bytes; `edits[].expected` is the exact bytes each edit replaces.
None implies another — a comment in another definition of the file moves the file and the
snapshot but not the edit's bytes, an edit in another file moves only the snapshot, and a line
above the target moves the expected bytes out from under the range — so all three are kept, and a
future applier refuses unless all three hold. `root` is N28's `RootIdentity`: the durable
compilation namespace, not proof of project lineage.

**Minimal and canonical.** One edit per renamed occurrence, each the name's bytes; a fix's one
span. No file text is carried. Files are ordered by root-relative path, edits by start then end;
a duplicate, an overlap, two insertions at one point, or an edit that changes nothing refuse the
whole patch. Serialisation order is not application order — right-to-left application is the
applier's concern. No session id, generation, host path or time is written: the same sources and
request give the same bytes from any checkout.

**Targets.** A top-level function, record or enum, and a member of one, carries its durable key
(`module::kind name`, with the member); `durable` says so. A local has no durable key and is not
given one: it is named by its declaration's place and the durable key of the function it is in.

**What a patch is not.** Not applied, by any command or tool; not cached — each request analyses
the disk and drops everything; not a refactoring framework; not a text replacement a client
supplies. A fix of a syntax diagnostic has no patch: the compilation that carries it needed
recovery, so N28 has no snapshot to bind it to, and it is refused as `incomplete_compilation`.

**The CLI and the MCP server.** `nazm patch rename --root --file --byte --new-name --json` and
`nazm patch fix --root --file --byte --fix --json` print exactly `nazm.patch/1`. `nazm-mcp` adds
`nazm.semantic_patch`, whose input is a typed request tagged by `operation` (`rename` or
`diagnostic_fix`), its schema the SDK's derivation of that type — no root field and no unknown
argument is accepted — and whose result is the patch once, as structured content.

**Deliberately not here.** Patch application or any write, MCP write tools, exported rename,
project-wide importer discovery, multi-file renames (N18's scope rule keeps a private entity's
occurrences in its declaring file), fix-all, refactors, git.

## 7.32 Compact diagnostics and progressive disclosure — settled by N30, 2026-09-28

> **Compact diagnostics never infer structured semantics from diagnostic prose.** A compiler
> `Fix` and an N29 semantic patch are distinct capabilities.

```text
compiler Diagnostic                                         (nazm-diag)
        ↓
nazm.diagnostic/1, unchanged                                (Diagnostic::published)
        ↓
compact summary index, nazm.diagnostic-index/1              (nazm-service/src/diagnostics.rs)
        ↓
one selected detail, nazm.diagnostic-detail/1
        ↓
optional N29 patch planning                                 (a nazm.semantic_patch selector)
```

**What is indexed.** Every diagnostic of the root's compilation as the service analyses it —
the analysis N27–N29 read, from disk, the root and what it imports and no other file. It is a
module's analysis, as the language server's is, so a missing `main` (`N0204`) is not among them.
Syntax diagnostics are: the index needs no semantic snapshot.

**The compact entry.** `id`, `code`, `severity`, `primary` (root-relative file and UTF-8 byte
range, translated from the compiler's span by the same source map every answer uses — never the
published concatenation offset), `owner`, and `fixes` counted by applicability. No message, help,
label text, fix description or precondition. The index adds counts — total, `error` (the one
severity the compiler has), diagnostics with fixes, automatic and needs-review fixes — and builds
no patch.

**Facts.** The compiler's `Diagnostic` carries no typed expected/actual/entity facts today, so
none are exposed: detail says `facts: "unavailable"`, and no field is reconstructed from a
sentence. A structural test keeps the module from reading a diagnostic's message, help,
description or precondition.

**Identity.** Entries are ordered by primary file, start, end and code, then by their secondary
and fix places, then by the compiler's own deterministic order. An id is `code@file:start-end#n`,
`n` counting earlier entries with the same code and primary place; two diagnostics the compiler's
structure cannot tell apart are told apart by that ordinal and nothing else. Message and help
never reach an id.

**State.** Two fields, each with one meaning. `state.source`, always present: BLAKE3 over the
root's identity (module key and compiler) and every loaded source file's root-relative path and
exact bytes, in load order, length-prefixed — no absolute path, generation or time. `state.snapshot`:
BLAKE3 of the root's `nazm.snapshot/1` (N28), or null where the compilation has none — after
syntax recovery or a missing module; N28's completeness rule is not weakened. A detail request
names the index's `state.source`; any other value is `stale_state`, and an id is resolved only
within the state it came from, so an old id never selects another diagnostic now at its place.

**Owners.** The durable function, record or enum whose declaration, by N22's outline, contains the
primary span — only in a compilation that needed no syntax recovery and loaded every module.
Otherwise, and for anything outside every declaration (a refused duplicate, an import), none.

**Detail.** The diagnostic exactly as `nazm.diagnostic/1` publishes it — canonical prose,
unchanged, spans as that schema defines them — with the same spans in their own files, the owner,
`facts`, `compiler_fixes`, `semantic_patches`, and for each fix its applicability and the
`diagnostic_fix` selector (`file`, `byte_offset`, `fix`) N29 plans it from, or null. A fix is a
semantic patch only where N29 would plan it: a compilation with a snapshot, and N24 planning the
edit. The check reads N24's plans for that one diagnostic and builds no patch; a syntax
diagnostic's fix is a compiler fix and no patch.

**The CLI and the MCP server.** `nazm diagnostics --root FILE --json` prints the index; `--detail ID
--state DIGEST` one detail. `nazm-mcp` adds `nazm.diagnostics` (no arguments) and
`nazm.diagnostic_detail` (`id`, `state`, typed). Each result once, as structured content. Every call
reads the disk; nothing — index, detail, history — is kept. They are disk tools: an editor's
unsaved buffers are the language server's, whose diagnostics are unchanged. `nazm check --json`
is unchanged.

**Deliberately not here.** Typed facts for any diagnostic, other verbosity levels, diagnostic
history, AI-written explanations, message parsing, patch application, and build or test output.

## 7.33 Token-efficient command and test summaries — settled by N31, 2026-09-28

> **Summary is an additive view, not a replacement for existing detailed machine output.**
> **Absence of diagnostics does not imply command success.**

```text
nazm check / build / test, as they run today               (nazm-cli)
        ↓  the command's own outcome, and its diagnostics or case verdicts
compact summary                                             (nazm-service/src/summary.rs, nazm-cli/src/test.rs)
        ↓  counts, and only the diagnostics and cases that matter
nazm.command-summary/1 · nazm.test-summary/1
        ↓  on demand
existing detail: --json, nazm.test/1, nazm diagnostics --detail
```

**What a command summary is.** `--summary-json` on `nazm check` and `nazm build` prints one
`nazm.command-summary/1` object on stdout and no diagnostic on stderr. It carries the command's
status — `success`, `rejected`, `unreadable`, `refused`, `toolchain_failed`, one per exit path the
command has, set where the command returns — and its exit status, unchanged; counts; and a
reference to every diagnostic the command reported: code, severity, place, fix counts, no prose.
A build adds the artifact path as it prints it. No timing is included, so the same sources give the
same summary.

**References and N30.** The command's diagnostics are its own — `nazm check`'s program check,
`nazm build`'s check and lowering — not N30's module analysis, so a missing `main` (`N0204`) and a
backend's refusal of a construct (`N0101`) are kept. Each is matched to N30's index by structure
alone (code, primary, secondary and fix places, the n-th of equals to the n-th): a match carries
N30's `id` with `scope: "index"`, and the summary carries N30's `state.source`, so `nazm diagnostics
--detail` answers it; anything else is `scope: "command"` with no id — never a fake one. N30's
shared structure orders them. If a file changed between the command's read and the summary's, no
reference is linked and `state` is null. No N28 snapshot is needed, so syntax diagnostics are
references like any other.

**What a test summary is.** `nazm test --summary-json` runs the cases exactly as `nazm test` does
and prints one `nazm.test-summary/1`: its status (`passed`, `failed`, `no_cases`, `unreadable`,
`toolchain_failed`, one per exit path), exit status, whether the native leg ran, counts — cases,
passed and failed each counted from the case's own verdict, and files without an expectation —
and every case that did not pass, by `nazm.test/1`'s name and file, with how each leg ended:
`matched`, `mismatched`, `not_run`, `build_failed`, `could_not_build`, `could_not_run`. Each is read
from what the runner did — which process it started, whether it could, whether it succeeded —
never from output text, so a runtime error or a compile error in a case is a `mismatched` leg and no
diagnostic is made of it. No passed name, no output, no truncation; cases in the order they ran.

**Every outcome is exercised.** Each status and each leg has a regression test that reaches its
production branch through the real binary. The runner starts each leg with this executable, or with
the one `NAZM_TEST_DRIVER` names — a test seam, not an interface — so a test can hand it a driver
that does not exist, one that removes itself after the program has run, or one whose build succeeds
and writes nothing; `nazm build` finds `clang` on the process's own PATH, so a test gives that one
process a `clang` that refuses, one that will not link, or none. Nothing machine-wide is changed.

**Flags and channels.** `--summary-json` conflicts with `--json`, and with `--cache-report` and
`--build-report`: an ambiguous combination is refused by the argument parser (exit 2; 5 since Q1-C1 froze the exit
contract). Human output,
`--json`, `nazm.test/1`, `nazm.check-report/1` and `nazm.build-report/1` are unchanged. A build's
toolchain failure may still write its own message to stderr.

**MCP.** One tool, `nazm.command_summary`, with a typed `operation` whose only value is `check`:
the root's check from the disk as it is now, run without the check cache, so nothing is written.
Building writes an executable and testing runs programs, which may write files; neither is
read-only, so neither is offered.

**Deliberately not here.** Test-detail retrieval, test impact, failure explanations, a build-log
store, timing, remote CI, and any MCP tool that builds, tests or runs a command.

## 7.34 Machine-addressable documentation and selective retrieval — settled by N32, 2026-09-28

```text
canonical documents under docs/                       (the authorities)
        ↓
structural section index                              (crates/nazm-docs)
        ↓
stable ids + authority classes
        ↓
selective exact retrieval                             nazm.docs-index/1 · nazm.docs-section/1
        ↓
nazm docs  ·  nazm-mcp nazm.docs_index / nazm.docs_section  (read-only)
```

**The index is not a second source of documentation truth.** The documents are. `nazm-docs`
reads them, as they are on disk at each request, into sections, and answers with their bytes;
it holds no copy of any rule, title or example. Its manifest is fourteen rows — a document's id,
its path, its authority and which reader it takes — in the order of `master-architecture.md`
§5's authority table, with `mcp.md` beside `diagnostics.md` and the grammar beside the guide.
Nothing is found by walking a directory: `README.md`, `releases/` (frozen evidence) and
`research/` (a draft protocol) are outside it, and so is every user project's documentation.

**Goals, roadmaps, architecture, evidence and normative specification retain distinct
authority classes.** Every document, and so every section, carries one of eleven: `goal`
(`NAZM_LANGUAGE_GOALS.md` — aspiration, never evidence of what exists), `law`
(`master-architecture.md`), `architecture`, `normative` (`spec.md`), `grammar`, `guide`,
`interface` (`diagnostics.md`, `mcp.md`), `evidence` (`capability-matrix.md`, `performance.md`,
`bootstrap.md`), `planning` (`roadmap.md`), `research` and `operational` (`runbook.md`). The
class is the document's, from §5; nothing classifies a section by its words, and a section's
`kind` is structural only — `document`, `heading` or `production`.

**Retrieval never paraphrases canonical text.** A section's body is exact bytes of its file:
a heading's line through the byte before the next heading of any level. That is its **direct**
text — a parent never carries its subsections, which are listed as children — so a document's
sections, in order, are the document. A grammar production's body is its attached comments
and the production through `;`, with the production's text and its `@example`s beside it,
exactly.

**Identity.** `document:local`, and a document's root — its title and the text before its
first subheading — is `document`. The local part is an explicit anchor, `<!-- id: name -->` on
the line under a heading, which survives rewording and moving; else the heading's own label
where the document numbers its sections (`architecture:7.33`, `capabilities:2a`, `goals:G78`,
`roadmap:N31`, `research:R1`), which survives rewording; else the heading's text as a segment
under its parent (`spec:generics/identity`), which does not. Never a line, an offset, an
ordinal, a hash, a random value or a commit. `spec.md` gained forty-three anchors, one under each
of its `##` and `###` headings, and no other change; no other document was edited for this. A
production is `grammar:name`. Two sections with one id are refused, never numbered apart — which
is also the one mechanical G79 audit this milestone makes: no anchor, and so no rule id an anchor
carries, is defined twice.

**Structure is read, not guessed.** ATX headings outside fenced code (backticks or tildes, at any
indentation) and outside HTML comments; a `#` line inside a fence is code. Setext and HTML
headings, which the corpus does not use, are refused rather than mis-sectioned. The grammar is
read as productions — a comment separated from every rule by a blank line, after the header, is
refused — and a production's `references` are the nonterminals it names: a grammar reference,
not a semantic dependency.

**Links are explicit or absent.** A Markdown link into the corpus resolves to a section id — a
`#fragment` by the anchor a reader's browser gives the heading — and one that resolves to nothing
makes the corpus unavailable, never quietly partial. A link outside the corpus or to the web is
kept as written. A guide section between the generator's markers titled `` `name` `` is
`generated_from` `grammar:name`. Nothing links sections because their words are alike.

**Freshness.** The corpus `state` is BLAKE3 over a domain tag and, in manifest order, each
document's id, repository-relative path and bytes; a section's digest is BLAKE3 over another tag,
its document, its id and its body — so moving an anchored section changes its parent and not its
digest. No absolute path, time, process or commit enters either, and a copy of the repository
elsewhere gives the same bytes. A section asked for with a state that is not the current one is
`stale_state`; an unknown id is `not_found` — never the nearest one. The MCP tool requires the
state, as N30's detail does; the command line takes it optionally.

**Surfaces.** `nazm docs --index [--document ID]` and `nazm docs --section ID [--state S]`, with
`--json` one object on stdout and nothing on stderr, and `--root` for another checkout; without a
root, the repository the binary was built from, used to find files and never written into an
answer. `nazm-mcp` adds `nazm.docs_index` and `nazm.docs_section`, whose arguments are a document
id, or a section id and a state — never a path — over the repository given as `--docs`, fixed at
startup. Both read at every call and keep nothing. `cargo xtask check` has a `docs corpus` gate
that loads the corpus with the same reader.

**Deliberately not here.** Semantic, fuzzy or full-text search, embeddings, summaries,
documentation history, Git, fine-grained normative rule ids below section level, an installed
copy of the documentation, and any user project's documents.

## 7.35 Repository context map and minimum task context — settled by N33, 2026-09-29

```text
nazm-cli (nazm repo)  ·  nazm-mcp (nazm.repository_map, nazm.task_context)
        ↓
nazm-repo — the repository map and the task planner          nazm.repository-map/1 · nazm.task-context/1
        ↓                  ↓                     ↓                          ↓
nazm-service           nazm-docs            Cargo manifests          the mutation catalogue
(N17 · N22 · N25 ·     (N32 ids, authority,  (packages, targets,      (profiles, killers,
 N27 · N28 · N30)       sections, links)      dependencies, docs)      where a mutant lands)
        ↓
nazm-core
```

**Not another source of truth: an index and a planner over the ones that exist.** `nazm-repo`
decides no meaning, reads no Markdown and scans no import: every entity and relationship is read
from the authority that owns it — Cargo manifests for packages, targets and their dependencies;
the compiler, through the language service, for which files a compilation loads, its durable
definitions (N28's, with N22's name ranges, a new service accessor and no new checker state),
their links, callers and diagnostics (N17, N25, N27, N30); `nazm-docs` for documents, sections,
authority and links (N32); the mutation catalogue for which tests observe which paths and which
test kills which mutant. The layering is the diagram: nothing below `nazm-repo` knows it exists.

**Repository entity law.** An entity is something with a durable identity in one of those
authorities, named `kind:authority-id`: `package:nazm-docs`, `target:nazm-docs:test:corpus`,
`sources:compiler`, `root:compiler:emit.nz`, `module:compiler:analyse.nz`,
`def:compiler:analyse.nz::fn check_program` (N3's durable key under a source root),
`doc:spec:generics` (N32's id), `schema:nazm.context/1` (the constant the file declares),
`profile:checker`, and — in a task context — `mutation:NAME`, `test:PKG:KIND:TARGET::PATH` (a
catalogue killer) and `diagnostic:SOURCES:ROOT:ID` (N30's id within its compilation). Never an
absolute path, a line, an offset, a random value, a time or a commit; a path appears only as a
location. What has no durable identity is named as omitted, not invented: Rust items (the
compiler owns no Rust semantics — packages and targets are what is mapped), locals, the core
prelude's definitions, untracked definitions, the definitions of a compilation that needed syntax
recovery, and tests the catalogue does not name. The one place the Nazm side is written down is
`nazm-repo`'s manifest: three source roots, `compiler` (its four drivers are roots, its other
files modules), `examples` and `examples-bad`, each with the sections that are its authority. A
package declares its own, as `[package.metadata.nazm] docs = [...]` in its manifest.

**The map** (`nazm.repository-map/1`) says what exists and how it relates, never what it says:
each entity's kind, parent, location, a compilation's completeness and diagnostic count,
structural relationships — `depends_on` and `dev_depends_on` (Cargo), `documented_by` (a
package's metadata or a source root's row), `includes` (a module a compilation loaded),
`observed_by` and `covers` (a profile's targets and owned package or source root) — and where to
retrieve more, as the schema and arguments of an existing answer: `nazm.snapshot/1` and
`nazm.diagnostic-index/1` of a root, `nazm.docs-index/1` of a document. Sections, mutations,
catalogue tests and diagnostics are counted as delegated to the answer that enumerates them. A
class table gives each kind's authority; a document carries its own. Canonical order is kind,
then id in byte order; relationships by kind and target.

**Validation.** A map is published only if every id is unique and well-formed, every parent and
relationship target exists (an N32 section by the corpus), every kind, relationship, retrieval
and authority is known, and the order is canonical — `nazm_repo::validate`, run on every map
built. Anything else, or any unreadable input, broken documentation link, unresolved catalogue
target, duplicate package or schema, missing root, or compilation that loads a file outside its
source root, is `unavailable` with every problem: never a partial map.

**Minimum-context closure.** A task (`nazm.task-context/1`) takes one to eight seeds — entity
ids, never paths or queries — and an intent. One relationship step from each seed, never more:
nothing is followed from an item that is not a seed, so a cycle is one visit, and each
relationship is cut at sixteen members, counted in `bounded`. From a definition: `understand` —
its exact source and shape, the records and enums it names (`signature_type`), every other
durable definition it uses by shape (`direct_dependency`), its source root's sections; `edit`,
the default because it is the widest — that, plus what uses it in **every** compilation of the
manifest that loads its module (`direct_caller` for a function, from N25; `direct_reference` for
a type, each use placed in its durable owner), and its tests; `diagnose` — the seed, the
diagnostics it owns, what it names; `test` — the seed and its tests. Tests are related only by
the catalogue: the verified killers of each mutation whose `find` lands inside the seed's
declaration (`killer_test`), and the targets of the profile owning its file (`profile_target`) —
never by a name that looks related, and a killed mutant is evidence the test observes that
defect, not that the code is right. A section brings the sections its text links to, the
production a guide rule is generated from and the productions a production names; an edit adds
the sections that link to it, pointed at without their text. A diagnostic brings its owner with
its exact source. A package brings its declared sections and dependencies, and for an edit its
dependents, owned targets, killers and profile targets.

**Why each item is there.** Every item carries its `reason`, the seed it came `via`, its priority
`class`, its `authority` and a `retrieval` — the arguments of `nazm context`, `nazm docs
--section` or `nazm diagnostics --detail` that give it again. What was known and not selected is
counted in `excluded` (callers for an `understand`, dependencies with no durable key). No score:
the classes are `seed`, `correctness`, `authority` (normative, grammar, law, interface,
architecture, evidence sections, in that order), `edit_safety`, `test`, `supporting` (guide and
operational sections, children), `context` (planning, research, goal) — a goal or a plan never
outranks the specification or the evidence. Within a class, by that authority order and then id.

**Budget.** `max_entities`, `max_source_bytes` (definition source included) and `max_doc_bytes`
(section text included) — structural, not tokens. The seeds are mandatory: a budget they do not
fit is `insufficient_budget` with what they need, never a context without its seed. Then items in
priority order until one does not fit: it (with the dimension it exceeded) and everything after
it (`priority`) are listed in `omitted`, and the answer is `partial`. A fan-out cut also makes it
`partial`. The same request and state give the same bytes, so the same budget chooses the same
items.

**Source and documentation inclusion.** Source only as a definition's exact declaration (N27's
range) — for a seed, a diagnostic's owner and a mutated definition; every other definition by its
shape, and never a whole file. Documentation only as N32 sections, never a whole document.

**Freshness and partial compilation.** The repository `state` is BLAKE3 over a tag, the compiler,
the manifest, N32's corpus state, every Cargo manifest and the targets Cargo would find, the
catalogue, every schema and every `.nz` file directly in a source root — paths and bytes; a task
with another state is `stale_state`. A compilation that needed syntax recovery keeps its modules
and loses its definitions: a definition only such a compilation loads is `incomplete_compilation`,
never last-clean facts.

**Security boundary.** A seed is compared with the repository's own identities and never opened:
`/etc/passwd`, `../x.nz` and a part with a `/` are `invalid_request`; an unknown well-formed id is
`not_found`, never the nearest. There is no listing, globbing, search or read of a caller-named
path. The CLI takes `--root`; the MCP tools read the repository fixed at startup (`--docs`).

**Surfaces.** `nazm repo --map` and `nazm repo --task [--intent I] --seed ID… [--state S]
[--max-entities N] [--max-source-bytes N] [--max-doc-bytes N]`; with `--json`, one object on
stdout, nothing on stderr, exit 0 with a map or a context and 1 with a refusal; conflicting modes
refused by the parser. `nazm-mcp` adds `nazm.repository_map` and `nazm.task_context`, whose
`state` is required. Both derive every answer from the disk at the call and keep nothing.

**Deliberately not here.** Rust semantics, search of any kind, embeddings, relevance scores,
transitive closure, Git, history, a persistent index, a daemon, network access, package
resolution, tokens, snapshot or delta machinery of its own (N28's), a second documentation
reader (N32's), a second context packet (N27's) — and any claim of project-wide completeness:
callers are those of the manifest's compilations, and an exported definition's users elsewhere
are not known.

## 7.36 Tokenizer-independent context measurement — settled by N34, 2026-09-29

```text
nazm repo --task … --json            (selection: entities, source bytes, doc bytes — N33)
        │  exact bytes, piped
        ▼
nazm-tokens measure                  nazm.token-cost/1        (a tool beside the compiler)
        │
        ├── tiktoken-rs 0.12.1       openai-bpe/cl100k_base, openai-bpe/o200k_base
        └── tokenizers 0.23.1        sentencepiece-bpe/mistral-7b-v0.1, sentencepiece-unigram/t5-small
                                     tools/tokenizers/*.tokenizer.json, pinned by BLAKE3
```

**Tokenizer independence.** No tokenizer reaches the language. `nazm-tokens` depends on no
internal crate, and no crate depends on it: the layering table lists it nowhere, and the
`tokenizer reach` gate refuses any other crate that depends on `tiktoken-rs` or `tokenizers`.
Parsing, checking, `DefKey` and `ModuleKey`, diagnostics, code generation, the bootstrap, the
documentation index and the planner's selection are what they were: `nazm` and `nazm-mcp` are
built without a tokenizer anywhere in their graphs.

**The byte-budget law.** N33's budgets — entities, bytes of definition source, bytes of section
text — stay the canonical ones, because they are the same under every tokenizer. A context is
selected first, from bytes and structure; it is counted afterwards, by any number of tokenizers,
and no count, identity or tokenizer-specific rank changes what was selected. N34's benchmark shows
why: one selection of `check_program`'s context is 1.63–1.66× as many tokens under one tokenizer
as under another, so a token
budget would be a budget for one tokenizer.

**Tokenizer identity.** A count is evidence only under the exact implementation and vocabulary.
Each tokenizer's identity is its id (`family-slug/name`), family, algorithm, library and version,
source and revision, licence, normalisation, special-token policy, and a BLAKE3 digest of its asset
— for the Hugging Face files the file's bytes, for the OpenAI vocabularies every rank's bytes as
the library decodes them — pinned in the crate and checked at every load. A missing, altered or
malformed asset is refused by name; an id no tokenizer has is `unsupported`, never another
tokenizer. Changing a version or an asset changes the identity.

**Three families.** `openai_bpe` (byte-level BPE over a regex pre-split, no normalisation;
`cl100k_base` and `o200k_base` are one family, two vocabularies), `sentencepiece_bpe` (Mistral
7B v0.1: merges over metaspace pieces with byte fallback) and `sentencepiece_unigram` (T5-small:
the most probable segmentation under per-piece scores, after the model's own NFKC-style
normalisation and whitespace split). The two SentencePiece models are different algorithms, not
two vocabularies of one: the files say `BPE` with 58,980 merges and a list of scored pieces, and the
tests hold both.

**The special-token law.** A count is of content: the exact text encoded as ordinary text.
OpenAI's special-token strings count as text; the SentencePiece templates' `<s>` and `</s>` are
not added, and are reported beside the count as `framing_tokens`. Chat roles, system framing and
vendor protocol overhead are not measured.

**The normalisation law.** Nothing is normalised before counting: the bytes measured are the
bytes that would be sent, and a composed and a decomposed accent are two inputs. T5 normalises;
that is its behaviour, recorded in its identity, and its unknown pieces are counted as
`unknown_tokens` — a smaller count that lost the text is reported as such.

**Offline and pinned.** The assets are in `tools/tokenizers/` with their provenance; the
libraries are pinned exactly, with no HTTP, hub or download feature, and the crate has no network
code. The container the gate runs in has no network.

**The multi-tokenizer benchmark law.** The seven N33 tasks, with N33's naive baselines unchanged
— they are defined once, in `crates/nazm-repo/tests/scenarios/`, for both benchmarks — are counted
under every tokenizer, each separately; reductions are integer basis points; the spread is the
largest minus the smallest reduction across tokenizers; the per-tokenizer median, worst and best
exclude only the fixed-overhead stress case, which is reported in full beside them. Every N33
mechanical check runs in the same pass.

**Surfaces.** `nazm-tokens list` and `nazm-tokens measure [--tokenizer ID]…`, the text on
standard input and never a path argument, one `nazm.token-cost/1` object on stdout. To measure a
task context: `nazm repo --task … --json | nazm-tokens measure`. There is no MCP tool: a server
that answers from the disk has no text of its own to measure, and would have to link a tokenizer.

**Deliberately not here.** Token budgets, token-ranked selection, a compact transport,
tokenizer-driven identity changes, chat-template accounting, model calls, and a persistent count
cache.

## 7.37 Agent task token, cost and correctness benchmark — settled by N35, 2026-09-29

```text
suite::build()        the N33 scenarios' baselines and `nazm repo --task` contexts, digested;
        │             truths read from the authorities; witnesses in both contexts
        ▼
run::plan()           task × arm × trial, arms alternating by trial, one independent request each
        │  budget check before every attempt, ≤ 2 retries of transport failures only
        ▼
Provider              openai (Chat Completions) · anthropic (Messages) · fake (tests)
        │  curl, credential on standard input — the only network access in the workspace
        ▼
journal.jsonl         nazm.agent-benchmark/1 records, raw usage beside normalised usage
        │  oracle::score() offline, fact by fact; price list applied, never in identity
        ▼
report::build()       per task, category and provider: correctness first, then tokens and cost
```

**The evaluation boundary.** `nazm-agent-bench` is evaluation tooling. No crate depends on it —
the layering table lists it nowhere — and it is the only crate allowed to depend on
`nazm-tokens`; the `tokenizer reach` gate refuses any other. It reads the planner and the
authorities through `nazm-repo`, `nazm-service` and `nazm-docs`; nothing it learns flows back. A
model's answer is never compiler truth: it enters no specification, no semantic database and no
capability evidence other than this benchmark's own results.

**The context-arm law.** Each task has two contexts and the requests differ only in them. The
baseline is the N33 scenario's naive texts — the whole files the facts are in, the whole
documents, the catalogue entries a search would show — each under a `==> label <==` line naming
what it is. The N33 context is `nazm repo --task`'s output for the scenario's request, byte for
byte, never trimmed. The system text, the layout, the task's wording, the answer contract, the
model, its parameters and the output budget are one value for both arms. Every context is
addressed by the BLAKE3 of its bytes, and a task whose two contexts have one digest is refused.

**The oracle law.** Every truth is read from an authority — the service's packets, a
documentation section, the mutation catalogue parsed independently — never from either context,
and a truth that rests on a section's statement is checked against the section's text, so a
reworded section stops the suite. Answers are JSON; each field is scored by an explicit,
mechanical normalisation: a definition as `file::name`, a test by its function name, a target by
its name, a citation as `document:id`, a scalar against a listed set of spellings. A fact is
correct or missing. A claim the truths do not contain is *hallucinated* when the supplied context
does not support it either, and a *wrong repository fact* when it does; a real section cited
beyond the required ones is recorded and neither credited nor counted. Solved is every fact
correct, no false claim, a valid object and no refusal. Every task's witnesses occur in both
contexts, so a refusal is always a failure here. No reasoning is requested or scored.

**The usage and cost law.** Usage is the provider's report, kept raw and normalised into total
input (cached parts included), uncached input, cache reads, cache writes, output and, where
reported, the reasoning part of output; `total_tokens` is input plus output and nothing else.
Prices are dated entries of `tools/agent-bench/pricing.toml`, with their source, applied to
usage and never part of an identity: a result can be priced again. Money is integer pico-USD — a
price in micro-USD per million tokens is pico-USD per token — so every cost is an exact product.
Local `o200k_base` counts of the context and of the fixed rest are recorded beside the
provider's; where the provider can count without generating, its counts of the request with and
without the context are recorded too, so the fixed prompt is never credited to a context.

**Benchmark identity.** A request's identity is its task, arm, trial, provider, requested model,
every request parameter, the prompt version and template digest, the suite version and the
context digest. A benchmark's identity is the suite digest, the prompt, the mode, the trial
count, the provider, the model and the parameters. A reply from a model other than the one
requested is recorded as `model_drift` and stops the run; it is never compared.

**The budget and retry law.** A run needs an explicit cap. Before every attempt, retries
included, the attempt's worst case — every input token uncached at the provider's bound over the
local count, and the whole output budget — is added to everything the evidence directory's
ledger has charged, and the attempt is refused if the sum passes the cap. A transport failure is
retried at most twice; a reply never is. A request with no reply is a `runner_error`: not scored,
never incorrect. A dry run computes the plan, the context sizes and the worst case with no
provider, credential or network.

**The resume law.** The journal is appended and synced one record at a time. A resumed run skips
every request whose identity has a record, and repeats a runner error only when asked to. A pilot
is its own benchmark identity and never evidence; the report reads authoritative records only.

**The network boundary.** Only `nazm-agent-bench run` reaches a network, through `curl`, with the
headers — the credential among them — on `curl`'s standard input and the body in a temporary
file, never on a command line. The credential comes from the environment or an owner-only file,
is never serialised, and is removed from any error text kept. Every contained workload, and so
every test and mutation killer, still runs with no network; the tests drive the runner with a
fake provider and the adapters with recorded responses.

**Deliberately not here.** Tool-using agents, multi-turn repair, automatic routing between raw
source and structured context, prompt tuning per arm, and any provider framework beyond the two
adapters the benchmark uses.

## 7.38 Typed effects — settled by N36, 2026-09-29

```text
parse        fn f() -> Int ! { io } { … }        EffectDecl: names and spans, as written
  │
declare      crate::effects::declared            N0367 unknown · N0368 duplicate (fix) → FnDef.declared_effects
  │                                               Export.effects → nazm.interface/6 (by name, id order; absent = undeclared)
check        record_site per call and spawn      Callee::Builtin | Callee::Function, spawned
  │
settle       crate::effects::analyse             direct effects + callees', Jacobi rounds to the least fixed point
  │                                               FnDef.inferred_effects, FnDef.effect_reasons
contract     inferred ⊆ declared?                N0366 at the first missing effect's site, shortest witness, fix for review
  │
tools        signature · hover · nazm.context/2 · nazm.snapshot/2 · nazm.delta/2
```

**Effect authority.** `nazm_sema::Effect` is the compiler's: two variants, each with a stable
`id` that is never reused or reordered, a name, and nothing else. A built-in's effect is
`Effect::of_intrinsic`, an exhaustive `match` on the built-in's identity, so a built-in added
without being classified does not compile; the call graph the checker already records — each
call site's resolved callee, and whether it is a `spawn` — is the only input. Nothing reads a
spelling: a name resolves to an effect only inside an effect set, in a namespace of its own.

**Canonical sets.** `EffectSet` is a bit per id: order-free and duplicate-free by construction,
iterated, printed (`{ io, spawn }`, the formatter's spelling) and persisted in id order, so no
map iteration can reach a set, a diagnostic or a digest. Purity is the empty set; no other flag
exists. The only relation between sets is `is_within`; there is no effect subtyping beyond it.

**Propagation.** After `check_unit` has checked every body of a module, `effects::analyse` builds,
per function, its direct effects — each built-in's, each `spawn`'s — and its edges to the module's
undeclared functions. A declared callee contributes its set, a constant; an imported callee
contributes the importer's view of its export — the declared set, or **every** effect when it
declares none; a callee whose body did not parse contributes nothing, since it is already an
error. Then rounds: each reads only the previous round's sets, so visiting order cannot change a
result, and the lattice is finite, so at most functions × effects rounds settle it — cycles
included. The first reason each effect was added is kept; the callee had it a round earlier, so a
chain of reasons is acyclic, and rounds being breadth-first it is a shortest one. No provenance
graph is kept: one reason per effect per function.

**The contract and its witness.** A declared set whose body requires more is `N0366`: one
diagnostic per function, at the first site requiring the first missing effect in id order, with a
secondary label per hop of the witness through the module's inferred functions to the built-in, the
`spawn`, the contract or the undeclared import where it ends, and a fix for review that declares
the union. A declaration naming an unknown effect is treated as unwritten after its `N0367`, so a
single mistake is a single error.

**The module boundary.** `Export.effects` is the declared set, and only it is persisted:
`nazm.interface/6` writes it by name in id order, `[]` for `! {}`, absent for no declaration, and
reads it back validated — an unknown name or a non-canonical order is refused. This is forced by
the cache design and is the right rule anyway: a module's interface hash is taken over its
**declarations** before any body is checked (N4), so a body-inferred set could not be in it
without a body change reaching importers unnoticed. A contract in the interface means an effect
change is an interface change, so every importer's key moves; a body change within its contract
moves nothing an importer depends on. `SEMANTIC_EPOCH` is 8: the checker refuses what it accepted
before.

**Tooling.** The signature renderer appends a declared set (and a built-in's own effect), so hover,
completion, signature help and packets show `fn f() -> Int ! { io }`; hover adds an undeclared
function's inferred set on a line of its own, since it is not a declaration. `nazm.context/2`
gives a function's `effects` — declared, required, published and one reason per effect, with the
call's range and the built-in or callee's identity — where `/1` had the section `unsupported`.
`nazm.snapshot/2` digests an `effects` section per function (declared or not, and required), and
`nazm.delta/2` reports it, so an effect-only change is never reported as unchanged. `DefKey` is
untouched: an effect change changes a definition's shape, not which definition it is, so rename,
references, call hierarchy and semantic tokens are unaffected. `nazm.diagnostic-detail/1`'s typed
facts stay `unavailable`: the effect facts are in the packet.

**No runtime.** Nothing reaches lowering. `nazm build` emits the same IR for a program with its
effects declared and without them; the interpreter never reads a set. When Core IR and MIR exist,
a function's checked set is on its `FnDef` for lowering to carry, which is where a later profile
(no `io`, no `spawn`) or an optimisation that needs to know a call is effect-free would read it.

**Deliberately not here.** Handlers, effect variables and polymorphism, user-defined effects,
`alloc`, `native`/`unsafe`/`foreign` (no FFI exists to exercise them), divergence and panic as
effects, capabilities and authority (§7.39 since N37), profiles that forbid effects, completion of effect names,
and the Nazm-written compiler reading effect sets — its lexer and parser do not, and no source it
reads writes one (`examples/effects/` is outside the folders its agreement tests read).

## 7.39 Capabilities and authority — settled by N37, 2026-09-30

```text
types        Type::Cap(CapabilityKind)            IoCap, SpawnCap: built-in names, Type::ALL, no equality leaf
  │
check        record_site per call and spawn      CallSite.held = kinds of the capability bindings a name reaches
  │           construction of a capability        N0370 (record form and call form)
  │           main's parameters                   N0371 unless every one is a capability
settle       crate::effects::analyse             effects to the least fixed point (§7.38), unchanged
  │
authority    crate::effects::authority           per function with a declared set, per site, in body order:
  │                                               built-in → CapabilityKind::of_intrinsic
  │                                               spawn → to_start_a_task
  │                                               undeclared callee → for_effects(its effects; every effect if imported)
  │                                               required ∖ held → N0369 once per kind, witness, no fix
contract     inferred ⊆ declared?                N0366, after the function's N0369s
  │
runtime      interpreter: Value::Cap(kind) for main's parameters · native: an i64 0 per root in the entry wrapper
```

**Effect versus authority.** An effect (§7.38) is what a function does; a capability is what allows
it to. `nazm_sema::CapabilityKind` is the compiler's: two variants with stable ids, each the
authority for one effect today (`CapabilityKind::for_effect`, exhaustive, returning a
`CapabilitySet` so a finer mapping is a table change). A built-in's required authority is
`CapabilityKind::of_intrinsic`, a second exhaustive table on the built-in's identity — written out
rather than derived from `Effect::of_intrinsic`, since what a built-in does and what allows it are
two questions, and a unit test holds the two consistent. `spawn`'s is `to_start_a_task`: authority
to **create** a task, kept apart from whatever the task itself does.

**Identity and namespace.** A capability's type is `Type::Cap(kind)`: a built-in type, in
`Type::ALL`, parsed by `Type::from_name` and refused as a declaration by the existing
`BUILTIN_TYPE_REDEFINED`. Kind and resource are separate by construction: the kind is the whole of a
v1 capability, and a resource-specific one would be `Cap` with an argument, not a new mechanism.

**Possession is lexical.** The checker's scope stack already holds every binding's type, so
`record_site` records, on each `CallSite`, the kinds of the capability-typed bindings a name reaches
there — innermost first, a shadowed binding reaching nothing. That set is the only authority input:
no function carries a flag, and nothing about a function's name, module or effect declaration adds
to it. A record containing a capability holds nothing until it is bound; a type parameter holds
nothing whatever instantiates it, which is parametricity rather than a special case.

**The check.** After the effect fixed point, `effects::authority` runs for each function with a
declared set — the ones that have left the bridge — over its call sites in body order. What each
site needs is decided from resolution only: a built-in's table entry, a `spawn`'s `SpawnCap`, and for
a callee with no declared set the authority for its effects — its inferred set in this module,
every effect across one. A declared callee needs nothing at the call: its authority is its
parameters, and ordinary type checking has already made the caller supply them. What is missing is
`N0369`, once per kind per function at the first site needing it, with the chain through the
module's undeclared functions (the §7.38 reasons, followed for the effect the kind authorises) and
no fix. Authority diagnostics precede the function's `N0366`, so a body wrong in both is reported
in one order every time.

**Unforgeability and the root.** No expression produces a capability: the record form and the call
form are `N0370`, there is no literal, conversion or default, and no built-in returns one. The root
is `main`, whose parameters must all be capabilities (`N0371`) and are supplied by the runtime —
`Interpreter::run_main` binds `Value::Cap(kind)` to each, and `emit_entry` passes an `i64 0` per
root. `lower` accepts a `main` whose parameters are all capabilities and still refuses any other.

**Spawn.** Starting a task needs a `SpawnCap` at the `spawn`. The task's authority is whatever the
`spawn` passes as arguments — capabilities cross into a task because they are values, not sequences
— and, for a task function with no declared set, what its effects need, which the spawner must
hold. A task never inherits its spawner's scope.

**The interface.** A capability parameter is a parameter: `nazm.interface/6` writes it as
`"IoCap"`, the built-in form every built-in type has, so authority crosses a module with no body
inspected and no new field. `/6` is kept deliberately: no field was added and none changed meaning,
an absent `effects` still means *declares none* — the rule that an importer with a contract must
then hold every authority is the checker's, covered by `SEMANTIC_EPOCH` 9 — and an older reader
refuses `IoCap` as an unknown type rather than misreading it. A capability-only change changes the
export's parameter list, so the interface fingerprint, every importer's key, the snapshot's `shape`
digest and the delta move; `DefKey` does not, so the definition is changed, not replaced.

**The compatibility bridge.** A function with no declared set is ambient: it has no authority of
its own and exercises its caller's. Its only root is a `main` with no set, which is handed all of it.
The bound is enforced at the one place the bridge meets explicit code — a call or `spawn` of an
ambient function from a function with a declared set needs the callee's authority held — so explicit
code cannot reach ambient authority, and an undeclared import is every authority. Before N37 every
source in the repository was ambient; each still is, unchanged.

**The enforcement boundary.** Static only. `Ty::of(Type::Cap(_))` is `Ty::Int`: a word that is
always 0 and that nothing reads — a word rather than a zero-sized `{}` because every storage, copy
and sequence path already handles a word, where a zero-byte element would reach a zero-byte
allocation. A capability parameter's function lowers to exactly the IR of the same function taking
an `Int`. There is no runtime check, no sandbox and no operating-system boundary; erasure permits
no forgery because forgery is refused before anything runs.

**Tooling.** Nothing new: a capability is a built-in type to hover, signature help, completion
(offered in type positions as a built-in type, never in a call head as a constructor), semantic
tokens and packets. `nazm.context/2`, `nazm.snapshot/2` and `nazm.delta/2` are unchanged — a
function's authority is its parameter list, which they already carry, and whether it is ambient is
whether `effects.declared` is null. No machine schema was added.

**Deliberately not here.** Attenuation and resource-specific kinds (nothing narrower exists to
derive), revocation, linear or affine capabilities, lifetimes, capability polymorphism,
user-defined capabilities, channels as authority, restriction profiles, foreign and unsafe
authority, runtime enforcement, retiring the bridge, and the Nazm-written compiler reading
capabilities — no source it reads declares one (`examples/capabilities/` is outside its
agreement tests' folders).

## 7.40 Provenance and information flow — settled by N38, 2026-09-30

```text
check_unit      each module's bodies, as before
  │
facts           crate::provenance::facts          one walk per body over resolution (vars, callees):
  │                                                 FunctionFacts { calls, result, sinks } as Flows
  │                                                 Flow = origins + why spans + params + call indices
  │             Reuse::checked_clean(m, facts)    kept in nazm.check/2 beside the interface
  │             Reuse::facts(m)                   a reused module's, rehydrated by durable name
solve           crate::provenance::solve          every module's facts, every run:
  │                                                 worklist 1: origins + passed-on params (least fixed point)
  │                                                 worklist 2: params reaching write_file's path
  │                                                 fixed point 3: origins each param is handed
restrict        explicit functions only           N0372 at the sink or the call, one witness chain
  │
tools           FnDef.provenance → nazm.context/3 · nazm.snapshot/3 · nazm.delta/3
```

**Provenance authority.** `nazm_sema::Origin` is the compiler's: four variants with stable ids —
`argument`, `file`, `authority`, `unknown` — and the absence of all four is local. A built-in's flow
is `Origin::of_intrinsic`, an exhaustive table on its identity: a source of one origin, a function
of its arguments, a read out of shared storage (`unknown`), a fresh handle, or the one sink. No
source syntax names an origin, so none can be forged.

**Summary representation.** A body is reduced once to `FunctionFacts`: its call sites (callee,
place, each argument's `Flow`), its result's `Flow` and its sinks. A `Flow` is a set of origins with
the first span each was met at, the enclosing function's parameters and indices of its own calls —
bounded by the body, whatever joins it. Solving turns each into a `Summary`: result origins,
passed-on parameters, parameters reaching a sink, and the origins each parameter is handed. There
is no symbolic execution and no per-operation identity; a function returning a constant has an
empty summary.

**The join law and the walk.** `Flow::join` is a set union and reports whether it grew. Bindings
are flow-insensitive slots: a walk joins every assignment into its slot and re-walks only if a slot
grew after the same walk read it, which is exactly when another walk could differ — so a body
settles in one walk unless a loop or a later assignment feeds an earlier read. Conditions are walked
for their calls and contribute nothing to values.

**Fixed points.** Three, over the settled facts of every module. Summaries grow from empty; a
worklist re-queues the callers of a function whose summary grew, seeded callees-last-first, and
empties because origins and parameter positions are finite — recursion, mutual recursion and import
cycles need no special case, and source order changes how soon, never what. Reaching a sink is a
second worklist over the settled values; what parameters are handed is a third. Call values inside
one function are their own small fixed point, since a loop can feed a call's result into its own
argument.

**Module persistence.** Facts name callees by module key and name, and places by byte range in the
module's own file, as `nazm_iface::PersistentFacts`. They are a function of exactly what a module's
`CheckKey` covers — its source and its imports' declarations — so they sit in the check entry, whose
schema moved to `nazm.check/2`, and no key changed. A reused module's facts are rehydrated strictly;
anything that does not fit this compilation is no facts, and a module without facts is solved as the
worst case — every origin `unknown`, every parameter passed on and reaching the sink — never as
local. `nazm.interface/6` did not move: the interface is declarations, and facts are not.

**Incremental invalidation.** None is needed, and that is the design: conclusions are never cached.
The whole program is solved every run from every module's facts, fresh or kept, so a callee's body
edit that changes its summary reaches a caller whose own entry was reused, in the same run. What a
cache hit saves is the body check and the walk; the solve is a few milliseconds over the facts.

**Diagnostic witnesses.** `N0372` is raised only in functions with a declared effect set, at the
`write_file` or at the call that hands a restricted value to a parameter that reaches one. Its
labels are one deterministic chain: through callee results (`g returns data from …`) and
pass-through calls to the span where the origin entered, and, for a call, through the callee's
calls to its `write_file`. Diagnostics are sorted by span within a function, after its N0369s and
N0366.

**Runtime erasure.** Nothing reaches lowering: `nazm-lir` reads no provenance, and the IR of every
example and of `compiler/emit.nz` is byte-identical under N37 and N38. The interpreter never reads a
summary. There is no runtime tag, table or object.

**Security limitations.** Explicit data flow only: implicit flow through conditions, timing and
every side channel are outside it. Records, variants and `Result` are tracked whole; sequences,
`Vec`s and channels are `unknown` on every read, because aliasing hides their writers. One sink.
Ambient functions' own writes are not checked. No declassification, sanitiser, user label or
policy exists. This is explicit information-flow provenance and a single restricted flow — not
non-interference, confidentiality enforcement or a sandbox.

## 7.41 Core IR: the constitution — N39, 2026-09-30

Written before the representation, so that the structs follow the law and not the other way
round. `crates/nazm-cir` holds the representation; this section is what it is allowed to be.

```text
source ─ parse ─ AST ─ check ─ Resolution ─┐
                                           ├─ nazm-core::lower (only after checking succeeded)
                                           ▼
                                  Core IR  (crates/nazm-cir, nazm.core-ir/1)
                                   │     │
                interpreter ───────┘     └─── nazm-lir::lower ─ LIR ─ LLVM text ─ clang
                (nazm-core::eval)              (reads no syntax tree and no position map)
```

**What Core IR represents.** The checked meaning of each function body, once: every value typed,
every name replaced by what it resolved to, every control transfer explicit, every piece of
surface sugar gone. It is the one lowering from a checked program; the interpreter and the native
backend both start from it, and neither reads the syntax tree or the checker's position maps
(the span-keyed answers: callee, binding, field, variant, payload, propagation, construction,
type arguments, checked call).

**What it deliberately omits.** Syntax: spellings, parentheses, statement-versus-expression
position, `?`, the shape an `if` or a block was written in. Layout: field offsets, discriminants,
aggregate shapes, symbols, linkage — the backend's, below the layout boundary of §2. Ownership
operations: no retain, release, move or drop instruction exists; what Core IR carries is the
semantic fact they are derived from, which locals a region owns until its exit (below).
Provenance: it stays a checker fact (below). Level 3 of §2 is also described as ANF with traits
as dictionaries; v1 is neither — it is a typed tree, and the language has no traits.

**When it is constructed.** Only from a program that checked with no diagnostics, by
`nazm-core`'s lowering, which is private to that crate and reached only through its checked entry
points. A parse or check failure produces no Core IR. The language service, completion, the
repository map and every other tool that reads incomplete source never construct it. A failure
inside lowering after a clean check is a compiler defect, reported as one; there is no fallback
to any other lowering.

**Invariants every consumer may assume.**

1. *Typed.* Every expression carries the type the checker established for it, read from the
   checker's facts — a binding's type, a checked call's result, a record field's or payload's
   declared type, the enum a variant constructs — and composed structurally, never inferred.
   The one other state is *never*: an `if`, `match` or block every path of which transfers
   control produces no value, and says so.
2. *Resolved.* A call names a function by its definition identity and a built-in by
   `Intrinsic`; a record, enum, field, variant and payload field are identities (declaration
   indices within a definition, as resolution spells them). No spelling is identity anywhere; a
   name kept for display is labelled display.
3. *Explicit control.* Structured regions, not basic blocks (why: below). A loop has a
   function-unique `LoopId`, and every `break` and `continue` names the loop it leaves or
   restarts — a `break` in a loop's condition names the enclosing loop, as the language has
   always meant. `return` carries its value. Each region states whether it falls through or
   transfers, and a value region has a tail exactly when it falls through. `if` without `else`
   exists only as a statement; `?` is a `match` whose error arm returns.
4. *Deterministic.* Functions in declaration order, locals in slot order, constants in first-use
   order, loops in source order, arms and initialisers in written order. The printed form of a
   program is a function of the checked program; no map iteration order reaches it.
5. *Source-linked, not source-identified.* Each node carries the span a diagnostic about it
   points at; a compiler-made node (the `match` a `?` becomes, its two bindings) is marked as
   such and carries the span of the source it came from. Spans are metadata: no digest includes
   one.
6. *Backend-neutral.* Nothing in it names LLVM, a slot index the emitter chose, a native type or
   a runtime entry point.

**IDs.** A function is its `FnId`, the checker's session-local definition identity — a Core IR
function exists for each and at the same index. A local is a `LocalId`: the checker's slots,
parameters first, then Core IR's own temporaries. A loop is a `LoopId`, a constant a `ConstId`,
both per function. None is a `DefKey`: those are durable names for definitions, and an IR entity
that lives for one compilation does not get one. The printed form names definitions and types by
their durable keys, so it reads the same from any checkout.

**Why regions and not basic blocks.** Both consumers are structured: the interpreter walks a tree,
and LIR is a structured tree the emitter turns into LLVM blocks, placing releases by region. A CFG
here would force a re-structuring pass downstream — a second analysis of control flow, which is
what this milestone removes — and would lose the region-ownership facts. Explicit loop targets and
explicit completion give every consumer what blocks would, without it. There is no SSA and no phi:
locals are mutable slots, `let` binds one, assignment writes one. A MIR may choose blocks.

**Generics.** Core IR is generic, as checking is: a generic function's body is lowered once, its
types mentioning its own parameters, and each call records its type arguments. The interpreter
runs it parametrically; the native backend instantiates it by substitution, as before. There is
no second generic system and no monomorphisation above the backend.

**Runtime primitives.** `Intrinsic`, the built-in table the checker reads — explicit, typed,
enumerated. `Vec[T]` operations carry their element type. Channels, `spawn` and `scope` are Core IR
operations; nothing is lowered to a thread or a runtime call here.

**Equality.** A comparison states its operand type and which law applies — integer, boolean,
bytes, or the derived structural equality of a named record or enum — or, in a generic body,
that the parameter's instance decides. No consumer discovers it.

**Effects and authority.** Each function carries its declared effect set, the set its body
requires, and whether it is pure; its parameters' types say which capabilities it holds, since
a capability is an ordinary typed value. Nothing re-infers either, and neither becomes a global
permission flag.

**The provenance boundary.** Provenance is decided before Core IR and stays in the checker's
summaries (`FnDef::provenance`, `nazm.check/2`). Core IR carries none — nothing below it reads
provenance, and a per-node origin set would be the bulk §7.40 avoided. No Core IR digest covers it;
the check artefact owns it.

**Memory management.** No retain or release is in Core IR. What survives is the semantic ownership
level: every region lists the locals bound in it, which the language releases at its exit, and a
`match` owns its scrutinee. The interpreter drops at exactly those points; LIR places native
retains and releases from the same regions and from each type's ownership law. That boundary is
the one a MIR will lower across.

**Digests.** Two per function, BLAKE3 over a canonical encoding that names every definition and
type durably and omits every span, display name and session-local number:

- *semantic*: the signature, the effect contract (declared and required), purity, and the body;
- *body*: what code generation reads — parameter, local and result types and the body — and no
  effect metadata.

An effect-only change moves the first and not the second; a capability-parameter change moves
both, because Core IR is typed and a capability is a type, even though the native backend erases
it (machine-code identity is the object cache's key, the emitted IR's bytes, §7.9); a
provenance-only change moves neither. A digest is not an identity: two functions with equal
digests are two definitions.

**Incremental boundary.** Core IR is built per module from the current run's full check and is
never persisted in v1; each function's digests exist for the caches that will key on them. Nothing
is reused across runs at this level yet, so nothing can be reused wrongly.

**Backend boundary.** Below Core IR a consumer may read the checker's semantic tables —
definitions, signatures, visibility, the call graph, the user-type table — and nothing that answers
a question about a source position. `cargo xtask check` enforces it.

## 7.42 MIR: the constitution — N40, 2026-09-30

Written before the representation, like §7.41. `crates/nazm-mir` holds MIR; this section is what
it is allowed to be.

```text
Core IR (nazm-cir) ─ nazm-mir::lower (instances, CFG, cleanup) ─ MIR (nazm.mir/1) ─ verify
                                                                    │
                          interpreter stays on Core IR              └─ nazm-lir: layout, symbols,
                          (a tree walker; §7.41)                       LLVM text ─ clang
```

**What MIR represents.** Each *concrete* function the native program contains — every
non-generic definition and every generic instance a program needs — as an explicit control-flow
graph whose ownership and cleanup obligations are written out. A backend reading MIR makes no
language decision: which loop a `break` leaves, what a `?` does, which values die on an edge,
which scopes must be joined before a `return`, whether a construction that failed half-way must
release what it took — all are statements and terminators in MIR.

**What it deliberately omits.** Syntax and everything Core IR already omits. Layout (field
positions, discriminants, sizes), symbols, mangling, linkage, object files, registers, stack
slots, calling conventions, relocations: the backend's, below MIR. Provenance: none (below).
Effects are function-level metadata only; nothing in MIR re-infers or checks them.

**When Core IR becomes MIR.** Only for `nazm build`, after the program checked cleanly and Core IR
was built and verified. MIR lowering plans the generic instances (moved here from `nazm-lir`,
unchanged: the same planner and bounds), then lowers each concrete function once. The
interpreter does not use MIR (below). A MIR the validator rejects after a clean check is a
compiler defect, `N0900`, and nothing is emitted.

**Front-end, MIR and backend concerns.** Front end: names, types, exhaustiveness, effects,
capabilities, provenance, which region owns which binding (§7.41). MIR: instantiation, the CFG,
evaluation into temporaries in the language's order, the move/copy/drop of every managed value,
scope joins and failure edges. Backend: layout, symbols and linkage, how a primitive is
implemented, how a drop of a given type is expanded into runtime calls, instruction selection.

**Invariants later passes may assume.**

1. *Typed.* Every local has a concrete type — a checker type with no parameter left, or the
   internal task-list type a `scope` holds. Every rvalue's result type is the destination's; no
   pass infers one.
2. *Resolved.* A call names a MIR function (`FuncId`); a primitive is an `Intrinsic`, with its
   element type for `Vec[T]`; records, fields, enums, variants and payload fields are the
   checker's identities (definition id plus declaration index). No string is an identity; names
   are display.
3. *Blocks and terminators.* Basic blocks, `BlockId(0)` the entry. Each block has exactly one
   terminator and nothing after it; statements do not transfer control except along an explicit
   *failure edge* (below). Terminators: `goto`, `branch` on a `Bool`, `switch` over an enum's
   variants (one arm per variant, no default), `return`, `unwind` (the failure exit), `exit` (the
   process ends: `exit_with`). No fall-through between blocks exists.
4. *Locals.* `LocalId`s are per function: parameters first, then the checker's bindings, then
   MIR's temporaries in creation order. They are ephemeral — never a `DefKey`, never persisted.
5. *Deterministic.* Functions: non-generic definitions in `FnId` order, then instances in
   canonical-key order. Blocks, locals and temporaries are numbered in creation order by a
   deterministic walk of Core IR; no map iteration reaches MIR.
6. *Source-linked.* Each statement and terminator carries the span a failure or diagnostic about
   it names. Compiler-made operations — drops, joins, the shared unwind block — carry a synthetic
   span and say so; no source position is fabricated.

**Ownership law.** A value of a *managed* type (`Str`, `Ints`, `Strs`, `Chan`, `Vec[T]`, and a
record or enum any field of which is managed, transitively) is owned by exactly one local at a
time. `copy l` takes a new reference (retain); `move l` transfers the one `l` holds and leaves `l`
empty; `drop l` gives it back and leaves `l` empty. An operand that reads a local *borrows* it:
calls, primitives and comparisons borrow their arguments, and a callee never releases a
parameter. Every result an rvalue produces is owned by its destination — a call's result, a
primitive's (a string literal and `arg` are owned too: their storage is immortal and their
release is a no-op), a projected field or payload (retained). A record or enum is built only
complete, from moved temporaries: there is no partially initialised aggregate in MIR, so
cleanup can never reach an uninitialised field. The empty state is the representation's zero,
and releasing it is a no-op, which is what makes `drop` of an already-moved local safe.

**Cleanup law.** Every edge that leaves a region drops what the region owns: the locals bound in
it so far, in reverse binding order, and every temporary still live. `return` drops everything
but the returned temporary, which it moves out; `break` and `continue` drop only what lies inside
the loop they leave, and join only the scopes opened inside it; `?` is a `match` whose error arm
returns (Core IR), and is cleaned like any `return`; a region's normal end drops its own. A
`scope` is opened by an allocation and closed by a join on **every** exit, innermost first; the
closing brace's join is followed by a failure check, as the language requires. Destruction order
is not observable (Nazm has no destructors) and is fixed anyway: reverse binding order.

**Failure edges and traps.** An operation that can fail — checked arithmetic, a call (the callee
may have failed), a primitive, opening a scope, spawning, the failure check after a join —
carries an explicit edge to a cleanup block. That block joins every open scope, innermost first,
and goes to the function's shared unwind block, which drops every managed local (null-safe) and
ends in `unwind`. A trap is this edge and nothing else; a `Result` is ordinary data and ordinary
control flow, and never an unwind. `exit_with` ends the process with no cleanup, as it always
has.

**Validator.** `nazm_mir::verify` checks, for every function: block targets and switch arms
exist; exactly one terminator per block; locals and constants exist and their types agree with
every operation (call signatures, primitive signatures, field and payload identities, record and
variant completeness, branch conditions); failure edges exist exactly on fallible operations;
payload reads occur only in the arm of a `switch` on the same local and variant; and, by a
forward dataflow over managed locals (`empty`, `owned`, `maybe`): no read of an empty local, no
overwrite of an owned one, no parameter dropped or moved, and at every `return` and `unwind`
nothing managed is still owned (no leak) and every task list has been joined. It never mutates
MIR, and running it twice gives the same answer.

**Generics.** MIR is post-instantiation. The planner is the one `nazm-lir` used since N11, moved,
with its bounds (`N0357`, `N0358`); an instance is identified by its definition and its type
arguments' canonical keys, which is also what its backend symbol is derived from. There is no
second generic system.

**The provenance boundary.** None. Provenance stopped above Core IR (§7.40, §7.41); MIR carries
no origin set and no summary. **Effects and capabilities:** each function records its published
effect set as metadata; a capability is an ordinary typed parameter; nothing below MIR is a
permission check.

**Printed form and digest.** `nazm mir FILE` prints `nazm.mir/1`: deterministic JSON, internal and
unstable, debug-only and not read back. Each function has an *executable digest*: BLAKE3 over its
signature types, locals' types, blocks and constants, naming definitions and types by durable
key, with no span, display name or effect metadata. A source-only or effect-only change leaves it
unchanged; a capability-parameter change moves it (the parameter's type is part of the
signature); a provenance-only change cannot reach it. It is not yet a cache key: objects are still
keyed on the emitted unit's bytes, which are a function of MIR (§7.9).

**The interpreter decision.** It stays on Core IR. It is a tree walker whose semantics are
Core IR's structured regions, it runs generic code parametrically, and it needs none of what MIR
adds — instantiation, explicit reference counting, failure edges. Moving it would be symmetry,
not a responsibility taken away from anywhere.

**Backend boundary.** After N40 the native backend reads MIR and a source-position table for
failure messages, and nothing of the checker. Remaining exceptions: `nazm-lir` still takes
`nazm-sema`'s *type and key definitions* (`Type`, `Intrinsic`, `DefKey`) as the vocabulary MIR is
written in — never a `Resolution`.

## 7.43 The backend boundary and the Cranelift backend — N41, 2026-10-01

Written before the code. `crates/nazm-lir/src/backend.rs` is the interface; `crates/nazm-codegen-clif`
is the second implementation of it.

```text
MIR (validated) ─ nazm-lir::lower: layout, symbols, linkage, units ─ Native
                                                                       │
                     ┌─────────────────── Backend ─────────────────────┤
                     ▼                                                 ▼
          LLVM text (nazm-lir::emit)                      Cranelift (nazm-codegen-clif)
          → clang -c → object                             → object bytes (cranelift-object)
                     └──────────── clang links, with the runtime ─────┘
```

**Backend input.** A `Native`: validated MIR, and what `nazm-lir::lower` settled from it for every
backend alike — the units (one per module, one per generic instance), each function's symbol and
linkage, and the refusals of the native subset. A backend reads MIR and nothing of the checker,
and decides nothing about the language.

**The interface.** `Backend` names itself (`id`, a version string that enters every cache key),
states its target, and turns a `Native` into artefacts: LLVM text for clang to compile, or object
bytes. Nothing from Cranelift appears in its signature, or in any crate above `nazm-codegen-clif`.

**Codegen unit.** The unit is `nazm-lir`'s: one per module and one per instance, so object reuse is
per unit for both backends. The runtime and the platform entry are LLVM text in every build — they
are the runtime, not code generated from a program's MIR, and N43 owns them.

**Runtime symbol boundary.** Every runtime entry point a backend calls is in one typed registry
(`crates/nazm-runtime/src/lib.rs`; in `nazm-lir` from N43 to N52): its symbol, its parameters and result in machine words and
pointers only, and what it does with ownership. A revision number names the registry; it enters
every object's cache key. Cranelift code calls the runtime only through it. Where the runtime took
or returned a first-class aggregate, the registry has a pointer-taking entry (`nz.str_join_to`,
`nz.read_file_to`, `nz.scope_join_to`) and a scalar accessor for thread-local state
(`nz.failed_now`), so no aggregate crosses between code two compilers generated.

**Internal calling convention (Cranelift).** The target's default convention. `Int` and a
capability are an `i64`, `Bool` an `i8`, a handle (`Ints`, `Strs`, `Chan`, `Vec`) a pointer; a `Str`,
record or enum is passed as a pointer to the caller's memory, which the callee borrows and never
writes, and returned through a pointer the caller supplies as the first argument. This is the
Cranelift backend's own convention and says nothing about C (N42).

**Layout (Cranelift).** Every type has a size and an alignment: 8 for a word, 1 for a `Bool`, 24
for a `Str` (pointer, length, owner); a record is its fields in the canonical order with natural
alignment; an enum is an 8-byte discriminant followed by the largest payload, each payload laid
out as a record. Both backends take field positions and discriminants from `nazm-lir`'s canonical
tables; the byte layouts differ and never meet, because no value crosses between the two in one
program.

**Object emission and linking.** A Cranelift unit is one object module, written by
`cranelift-object`. `nazm build` owns nothing past that: clang compiles the LLVM-text artefacts and
links every object with the runtime, with arguments built as a vector, never through a shell.

**Targets.** The v1 matrix is the host: `aarch64-apple-darwin` (verified on this machine) and
`aarch64-unknown-linux-gnu` (the contained, authoritative evidence); `x86_64` hosts are accepted by
the ISA but carry no evidence. A requested target that is not the host is refused: an object for a
foreign target needs a foreign linker and runtime, and building one here would be a claim with
nothing behind it. ISA flags are Cranelift's baseline for the triple; no host feature is detected.

**Settings.** `opt_level` none for `--opt-level 0`, speed otherwise; the Cranelift verifier on for
every function; position-independent code. All of them are in the backend's version string, so a
change to any of them is a new cache key.

**Choice and fallback.** `nazm build --backend llvm` (the default) or `--backend cranelift`. The
LLVM backend stays as the default and as the differential reference. A program Cranelift cannot
compile is refused by name with `N0101` and nothing is written; nothing falls back to the other
backend without being asked.

**Determinism.** The same MIR, target and settings produce the same object bytes; functions, data
and relocations are defined in MIR order.

**Failures.** An unsupported target or operation is a diagnostic (`N0101`); a Cranelift verifier
rejection of generated code is a compiler defect (`N0900`); a link failure is the toolchain's and
reported as one — none is a program error.

## 7.44 The ABI and the foreign-function interface — N42, 2026-10-01

Written before the code.

**Two ABIs, never one.** The *internal* ABI is how code this compiler generated calls code this
compiler generated, and it is each backend's own: the LLVM backend passes and returns LLVM
first-class values (an `Int` an `i64`, a `Bool` an `i1`, a `Str` the `{ ptr, i64, ptr }` it is, a
record or enum its named struct), and the Cranelift backend follows §7.43 (in-memory values by
pointer, an in-memory result through a pointer passed first). Neither is C's and neither is
promised to anything outside one build: the backend's identity and settings are in every object's
cache key, the runtime's calling contract is `RUNTIME_ABI_REVISION` (`nazm-runtime/src/lib.rs`),
and no program mixes the two backends' objects. The *external*
ABI is the C calling convention of the host target, and it is the only one a foreign function is
called with.

**v1 scope: import only, C only, scalars only.** A Nazm program may call a C function; C may not
call Nazm (no export, no callbacks, no runtime entry from a foreign thread). Only two types cross:

| Nazm | C | LLVM | Cranelift |
| --- | --- | --- | --- |
| `Int` | `int64_t` | `i64` | `i64` |
| `Bool` | `bool` (`_Bool`, one byte, 0 or 1) | `zeroext i1` | `i8`, zero-extended |

Everything else is refused by name (`N0381`): a `Str` is not a C string and has no NUL; a sequence,
channel, record or enum has a layout no C declaration states; a capability is authority and must
never become a forgeable word; generics have no single C signature. No pointer or opaque handle
exists in v1, so no ownership crosses the boundary and there is nothing to borrow, transfer or free.

**Declaration.** `[pub] extern "C" fn name(p: T, …) -> R = "symbol";` at the top level. `extern` is
contextual: it is an identifier everywhere except at the start of an item, so no program that
names something `extern` changes meaning. The ABI string is exactly `"C"` (`N0380` otherwise); the
symbol is written, never derived, and must be a C identifier — ASCII letters, digits and `_`, not
starting with a digit, at most 255 bytes — which cannot collide with a Nazm symbol (those contain
`.`) or the runtime's (`nz.`); anything else is `N0382`, so no malformed name reaches a linker
command. A foreign declaration has no body, no type parameters and no written effect set (`N0384`).
Two declarations of one symbol with different signatures are refused (`N0383`); the same signature
twice is one import.

**Authority and effects.** A foreign function can do anything a C function can, so calling one is
the effect `foreign` and needs the capability `ForeignCap`, held at the call — in every function,
with or without a declared effect set: N37's compatibility bridge does not extend to it, because
there is no older program to stay compatible with. `main` may take a `ForeignCap`, minted by the
runtime at start-up like `IoCap`; nothing else creates one. A wrapper that takes a `ForeignCap` and
calls the foreign function is the safe pattern; its callers then see `foreign` in its effects and
must pass it the capability. Importing a module that declares a foreign function grants nothing.

**Interpreter.** `nazm run` cannot call C. A program that calls a foreign function is refused before
it runs (`N0385`), not stopped part way; `nazm check` accepts it.

**Linking.** `nazm build --link FILE` adds an object or archive to the link, each as its own
argument, never through a shell; the path must exist. A symbol no linked file defines is the
linker's failure, reported with its output as a toolchain failure, never as a program error. No
search path, no dynamic loading and no library name resolution exist in v1.

**Guarantees kept and lost.** Nazm's checks stop at the boundary. The compiler guarantees the
declared types and that authority was held; it cannot know what the C function does, and a call
into C may corrupt memory, block, never return, or longjmp — none of which Nazm's cleanup or failure
model survives. A foreign call cannot fail the Nazm way, so no failure edge is taken after one.

**Amended during implementation.** Three rules the first draft did not state. *Reserved symbols*:
a foreign declaration may not name a C symbol the runtime or the platform entry declares itself
(`main`, `malloc`, `write`, the `pthread_*` family; `nazm_core::check::RESERVED_C_SYMBOLS`, kept
equal to the runtime's text by a test), because two declarations of one symbol in one link could
disagree — `N0383`, like any other conflict, which is also checked across modules when the program
is built, where every unit's declarations are visible at once. *Provenance*: what C returns carries
the origin `unknown` and every argument's origins, so a restricted `write_file` path computed from
it is `N0372`; what C does with an argument is not a Nazm sink. *Threads*: a task may call C — it
is handed the `ForeignCap` like any value — and C then runs on that task's OS thread, so a C
function called from two tasks must itself be thread-safe; Nazm adds no lock around it. A failed
link names the program's foreign symbols and `--link`, never "a bug in `nazm build`".

**Interface and versions.** A `pub extern` function is exported with its symbol, so another module
calls it without seeing the declaration's source: `nazm.interface/7`. The language gained syntax, an
effect and a capability kind, so the semantic epoch moves from 10 to 11.

## 7.45 The runtime constitution — N43, 2026-10-01

Written before the code. The native runtime is compiler-owned LLVM text linked into every
executable (`crates/nazm-runtime/src/`); this section is the contract it keeps with the code
both backends generate, with the operating system, and with the language.

**What the runtime is responsible for, and what it is not.** At run time: the process's entry and
end; the thread's failure state and the one place a failure is reported; string backings,
sequences and channels — their allocation, sharing and reclamation; files and the process's
arguments; starting and joining tasks; the memory accounting. At compile time only, and never in
the runtime: every check the language makes (types, effects, capabilities, provenance, ownership
— MIR decides every retain and release, §7.42), layout, and which runtime parts a program links.
The runtime makes no decision the language specifies: it carries out MIR's.

**One inventory.** Every symbol the runtime defines is listed once in
`nazm_lir::runtime::INVENTORY` — its exact LLVM signature, the runtime part that defines it, its
responsibility, what it does with ownership, its failure behaviour, its thread safety, and who
calls it (generated code of either backend, the entry, or the runtime itself) — and every global
in `GLOBALS`, with who writes it and how it is synchronised. The declarations every unit sees are
generated from the inventory, not written beside it; the word-level view Cranelift calls through
(`IMPORTS`, §7.43) must agree with it entry by entry; and tests hold the runtime text to both, so
a symbol defined and not inventoried, inventoried and not defined, or declared with a signature
it is not defined with, fails the build.

**Runtime ABI revision.** `RUNTIME_ABI_REVISION` names this contract: the inventory's signatures,
the sequence header (`len` at 0, `cap` at 8, `data` at 16, the reference count at 24), a string's
`(pointer, length, owner)` and a string backing's count at 0. It enters every object key of both
backends — Cranelift's directly, and clang's through the code generation configuration — so an
object built against another revision is never reused. N43 moves it to 2 because the inventory is
now the contract: the numbering says a revision-1 object was built against a contract nobody wrote
down.

**Startup.** The platform's `main(argc, argv)` is the entry unit's and nothing else is an entry.
In order: if the program reads its arguments, `nz.argc` (one fewer than `argc`: `argv[0]` is not
visible) and `nz.argv` are stored — before any task exists, never written again; each root
capability `main` takes is minted as the word `0` (authority was checked when the program was);
the program's `main` is called. There is no other initialisation and no lazy one: every other
global is statically initialised, and every helper set is stateless until called.

**Shutdown.** Three ways to end, distinguished. *Normal return*: every scope `main` opened has
been joined, the memory report is written to stderr if `NAZM_MEMORY_REPORT` is set, and the
process exits with `main`'s result as its status — one outside `0..=255` writes `N0416`'s message
and exits 2 — or, when `NAZM_RESULT_REPORT` is set, prints that result to stdout in decimal with
a newline and exits 0, as every program did before R1 (5b.3). *Failure*: the
memory report as above (the counts are final: nothing is still running), then `nz.report` writes
the thread's first failure message to stderr and exits 2. *Explicit exit*: `exit_with(code)` ends
the process at once with that status — no scope is joined, nothing is reclaimed, no report is
written — as the spec says. Nothing is freed at exit that the program had not already released:
the operating system reclaims the process.

**Memory.** `malloc`, `realloc` and `free` are the allocator; the runtime owns every block it
allocates and frees it on the last release. A string backing is a 16-byte header (the reference
count at 0) and its bytes; a literal and `arg`'s strings are immortal (their owner is null, and
retaining or releasing null is a no-op). A sequence is a 32-byte header and a data block that grows
by `realloc`; `nz.vec_release` releases each element with the element type's release function at
the element's stride, then the sequence. A channel is a 64-byte block (capacity, length, head,
closed flag, ring, lock, condition, reference count) and three allocations beside it — the ring,
the lock and the condition — taken in order and released together, with a failure part-way
through freeing exactly what was taken. Allocation failure is never undefined behaviour: every allocating entry
returns null or a status, and the call site records `N0406`. Each release decrements and frees on
zero; a count never underflows because MIR's validator proves every owned value is released
exactly once (§7.42).

**Thread safety, before N44.** String backings' and channels' counts are atomic (`monotonic` up,
`release` down with an `acquire` fence before freeing), because both may be shared between
tasks. Sequence counts are not atomic and need not be: a sequence may not cross into a task
(`N0321`), so one is only ever touched by one thread. The failure state is thread-local; the
accounting counters are atomic; `nz.argc`/`nz.argv` are written once before any task starts.
A channel's operations take its mutex. Nothing else is shared.

**Tasks and channels: the primitive boundary.** The runtime provides `nz.spawn` (start an OS
thread for a task and record it in its scope's list, record-first), `nz.scope_join` (join every
task in the list and hand back the first failure), and the channel operations (bounded buffer,
blocking send and receive, close). *Which* thread runs a task, when, and in what order is not a
runtime contract before N44: v1 starts one OS thread per task.

**The failure model.** Four classes, never confused:

| Class | Codes | What happens |
| --- | --- | --- |
| A language failure the spec defines | `N0400` overflow, `N0401` division by zero, `N0405` index or capacity | recorded (the thread's first), unwound by MIR's failure edges, joined at the scope, reported by the entry, exit 2 |
| The host refused a resource | `N0406` allocation, `N0404` a thread (in a native build `N0404` means the operating system would not start one; in `nazm run`, that the interpreter could not) | recorded and reported the same way: the program cannot continue, and says why |
| An I/O failure | `N0407` | the same, for `read_file` and `write_file`; `file_exists` answers instead of failing |
| Explicit exit | — | `exit_with`: the process ends with the program's status |

A `Result` is data and never any of these: nothing in the runtime turns an `Err` into a failure or
a failure into an `Err`. A runtime invariant the compiler guarantees (a count underflow, a join of
a list that was never opened) has no code, because MIR's validator makes it unreachable; a
foreign function that crashes is outside every class (§7.44).

**Capability roots.** Only the entry mints capabilities, and only for `main`'s parameters. No
runtime symbol returns one, no built-in constructs one (`N0370`), and none crosses to C (`N0381`).

**Foreign code.** A foreign call is made on the calling task's thread with no runtime state
touched. C cannot call into the runtime: its symbols are `hidden` and none is a C entry point.

**The operating system.** The runtime reaches it only through the C library and POSIX-threads
entry points the adapter (`runtime/os.rs`) declares — the same list on every v1 target — and a
test holds the runtime text to that list. A target outside the v1 matrix is refused before
generation (§7.43); there is no per-OS code path in the runtime, so there is no per-OS behaviour.

**Not here.** A scheduler, work stealing, task priorities (N44); runtime provenance tags (N38
provenance is static and erased); a runtime for `nazm run` (the interpreter is its own).

## 7.46 Structured concurrency and the scheduler — N44, 2026-10-01

Written before the code. `docs/spec.md` *Concurrency* is the language rule; this is what both
implementations — the interpreter and the native runtime (§7.45) — owe it, and what the scheduler
is and is not.

**Task and scope.** A *task* is a call started by `spawn f(args)` inside a `scope { … }` block. A
scope owns every task started in it, and **no task outlives its scope**: every exit from the
block — its closing brace, a `return`, a `break` or `continue` leaving it, a `?`, and a failure
unwinding through it — joins every task it started, innermost scope first (§7.42's cleanup law).
There are no detached tasks, no task handles a program can hold, and no way to name a task.

**Task states.** *recorded* (in its scope's list, before its thread exists) → *running* (its
thread started) → *finished* (its body returned; a failure, if any, published into its block) →
*joined* (its thread joined by the scope, its block freed). The order is fixed by the runtime:
`nz.spawn` records first and undoes the record if the thread cannot be started, so no task is
running and unrecorded; `nz.scope_join` joins in the order tasks were started and frees each block
after its thread is joined. A task that cannot be started is `N0404` at the `spawn`, and the
scope still joins every task started before it.

**Failure.** A failed task stops at its failure and publishes it. Its siblings are **not**
cancelled: they run to completion. At the scope's join, the first failure *in start order* is
the scope's, and it continues in the parent as the parent's own failure — at the closing brace or
the exit that joined. Later failures are dropped. A failure in the parent while tasks run is held
until they are joined, then unwinds — and it, not any task's, is the one reported, because a
thread keeps only its first failure and the parent's came first on its own thread. So a scope's result is a function of its tasks' outcomes, not
of the order they happened to finish in.

**Cancellation: none in v1** — amended by N54 (§7.56): cooperative cancellation by closing a channel a task selects on, and a select deadline; no cancellation on a sibling's failure still. In v1 there is no cancel operation, no cancellation on sibling failure,
and no timeout. A blocked `chan_recv` waits until a value arrives or the channel is closed. This
is a decision, not an omission to fill quietly: cooperative cancellation needs observation
points in blocking operations, and both implementations would have to agree on every one of them.

**The scheduler is the operating system's.** Each task runs on its own OS thread, started at the
`spawn` and joined by its scope. There is no worker pool, no run queue and no work stealing in
Nazm, and the compiler emits no scheduling policy — MIR says `spawn` and `join` (§7.42), the
backends call `nz.spawn` and `nz.scope_join`, and the runtime starts and joins threads. This is
the simplest architecture that keeps the language's progress guarantee: channel operations block
the calling thread, so a fixed pool of *N* workers would deadlock any program with more than *N*
tasks blocked at once — a program that is correct today. A pool becomes possible only with
suspendable tasks (stack switching or compiled continuations), which is research, not v1.

**Fairness and determinism.** Nothing is promised about the order tasks run in, interleave, or
finish, beyond what the OS scheduler does for threads. Program output that depends on task order
is the program's nondeterminism. What *is* deterministic: which failure a scope reports (first in
start order), when a scope ends (after every task), and what is reclaimed (everything).

**Blocking.** File I/O, `print` and channel operations block the calling task's thread and no other.

**Authority.** `spawn` is the effect `spawn` and needs a `SpawnCap` in scope (N37); holding one is
not the effect. A child has exactly the authority its arguments carry: a declared child needs its
capabilities passed to it (`N0369` otherwise), and an undeclared one exercises the spawner's under
N37's compatibility bridge, which the spawner must therefore hold (`N0369` at the `spawn`). A task
inherits nothing else.

**Provenance.** A task's arguments carry their origins into it: N38's summaries treat a spawn as a
call, so a value from a file handed to a task that writes to a path derived from it is `N0372`
exactly as a direct call is.

**What may cross into a task.** Scalars, strings (immutable, atomically counted), channels (shared,
locked), capabilities, and records and enums of those. Not a sequence, nor anything holding one
(`N0321`): a sequence is shared and mutable, and nothing synchronises it. So **Nazm has no data
races on its own values**: every value two tasks can reach is immutable or a channel. It does not
prevent deadlock (two tasks each waiting on a channel the other would fill), livelock or
starvation, and no diagnostic claims to.

**Channels.** `chan_new(n)` is a bounded FIFO of `Int`s with capacity *n*; `chan_send` blocks while
it is full (backpressure) and returns `false` once the channel is closed; `chan_recv` blocks while
it is empty, then appends the oldest value to a sequence and returns `true`, or returns `false`
once the channel is closed **and** drained — a close never discards what was sent. Every state
change wakes every waiter (a broadcast under the channel's lock), so no wakeup is lost. Only `Int`s
are queued, so no managed value is owned by a channel; the channel itself is atomically counted
and freed by its last holder.

**Shutdown.** `main` returns only after every scope in it has joined, so no task is running when
the process ends normally or reports a failure. `exit_with` inside a task ends the whole process at
once, with its siblings still running: that is what `exit_with` means.

**Not here.** Cancellation, timeouts, select over several channels, detached tasks, a worker pool,
task priorities, async I/O.

## 7.47 The standard library — N45, 2026-10-01

Written before the code. A foundation, not a catalogue: the smallest set of source-backed modules
real programs were writing for themselves, with every boundary stated.

**Three layers.** *Built-ins* are the compiler's: the operations the language cannot express in
itself — string bytes and slices, the sequences, channels, the process and file primitives,
`print` — each an `Intrinsic` with a fixed signature, effect and authority (§7.38–7.40). *The
prelude* (`@core/prelude`, §7.13) is two enums every module sees without importing them, and stays
two: `Result` and `Option`. *The standard library* is new: toolchain-owned Nazm modules a program
imports by name — `use "@std/text";` — and that no module sees unless it writes the `use`. A
standard module is ordinary source (`library/std/*.nz`), checked by the same checker, compiled by
the same backends, cached by the same keys; nothing in the compiler knows what any of its
functions does. So the standard library adds no compiler magic, and the list of true primitives
is the built-in list, unchanged.

**Naming and resolution.** An import path beginning with `@std/` belongs to the toolchain and is
never looked for on disk. `@std/NAME` is the standard module `NAME`, keyed `@std/NAME` — a key no
project path can produce, as `@core` is not (`ModuleKey::relative` refuses both prefixes) — and any
other `@std/` path is `N0205`, naming the modules there are. *Narrowed during implementation:* the
first draft reserved every path beginning with `@`, which broke an existing contract — a project
may keep and import its own files under a directory named `@core` (`prelude.rs`) — so only
`@std/` is reserved, and every other path resolves on disk as it always has. A standard module may import only standard
modules. The compiler carries each file's bytes, so a compilation needs no installation directory;
what is checked is those bytes, parsed as source.

**The v1 modules.** `@std/text` (prefix and suffix tests, find, contains, repeat, split, trim,
`parse_int`), `@std/option` and `@std/result` (the tests and fallbacks every use of them wrote
again), `@std/seq` (search, sum, maximum and minimum over `Ints`, `Strs` and `Vec[T]`),
`@std/io` (`println`, `eprintln`, `read` as a `Result`), `@std/process` (the arguments as one
sequence), `@std/chan` (one value as an `Option`). Function names carry their module's name —
`text_split`, `option_unwrap_or` — because Nazm has no qualified names and a module's exports
enter the importer's namespace: a prefix is the only thing that keeps two modules' names apart.

**Contracts.** Every standard function declares its effect set: `! {}` for everything pure,
`! { io }` for `@std/io` and `@std/process`, each of which takes the `IoCap` it needs — so a
wrapper never has authority its signature does not show, and calling one is as visible as calling
the built-in. Provenance flows through them because they are ordinary calls N38 follows: what
`io_read` returns is `file`, exactly as `read_file`'s is.

**Errors.** Ordinary failure is a value. `text_parse_int` returns `Err` for no digits, a
non-digit or a value outside `Int` — never an overflow; `text_find` and `ints_max` return `None`
rather than a sentinel; `io_read` returns `Err` for a missing file. A standard function traps only
where the built-in it calls would, and says so: `ints_sum` overflows as `+` does; `io_read`'s read
of a file that exists and still cannot be read is `N0407`, because the check and the read are two
steps and the language has no way to make them one.

**What is not here, deliberately.** No `map` or `filter` (Nazm has no function values), no
formatting framework, no HTTP, JSON, crypto or time. `exit_with`, `print`, `str_concat` and the
rest are not wrapped: a second name for one thing is a second way to say it.

**Stability.** Pre-1.0: a standard module may change between milestones, and its interface hash
says when. Its identity is its key and its bytes; no semantic epoch moves for a library change,
because the checker's rules did not change.

**The compiler written in Nazm** does not resolve `@std` imports: it is a second implementation of
the front end for the programs in this tree, none of which imports a standard module, and it
resolves every import against the importing file's directory, so it reports `@std/text` as a
module it cannot find.

## 7.48 Packages, locking and reproducible builds — N46, 2026-10-01

Written before the code. `crates/nazm-package` holds the model; the loader and the command line
consume it. No registry, no network, no build scripts.

**A package** is a directory holding `nazm.toml`, its manifest, `nazm.package/1`:

```toml
schema = "nazm.package/1"

[package]
name = "geometry"        # [a-z][a-z0-9-]*, at most 64 bytes
version = "1.2.0"        # MAJOR.MINOR.PATCH, decimal, no pre-release in v1
root = "src"             # the source root, relative to the manifest; default "src"
main = "main.nz"         # the entry module, relative to the root; only a binary has one

[dependencies]
shapes = { path = "../shapes", version = "0.3.1" }
```

Nothing else is accepted: an unknown key is refused by name, so a manifest cannot carry
configuration the compiler silently ignores. Descriptive metadata (authors, licence) is out of v1.

**Identity.** A package is its name, its exact version and its source — in v1 always a path,
relative to the manifest that names it. Two packages of one name in one build are a conflict and
refused, whatever their versions: a module's durable key is `NAME@VERSION/path`, and a program
with two `shapes` would have two answers to which one a `use` means. A dependency's declared
version must equal the version its own manifest states — exactly; there are no ranges in v1, so
nothing is ever chosen for a program.

**Imports.** `use "shapes:polygon.nz";` imports the module `polygon.nz` of the package the
importing package declares as `shapes`, resolved against that package's source root. Only a
package's own dependencies are visible to it: its dependency's dependencies are not. A path with
no `NAME:` stays relative to the importing file and may not leave its package's source root.

**Resolution** starts at the root package and visits dependencies in name order, depth first,
reading each manifest once by its canonical directory; a package reached twice by one identity is
one package (a diamond), by two is a conflict, and a package reached from itself is a cycle,
reported with its path. The result is a list sorted by name and a graph whose edges are sorted:
no hash-map iteration, directory listing order or filesystem timestamp enters it.

**Digest.** A package's digest is BLAKE3 over its manifest's bytes and every `.nz` file under its
source root — each as its path relative to the root with `/` separators, its length and its bytes,
in path order — under a domain-separation tag. No timestamp, permission, absolute path or
directory order is an input.

**The lockfile**, `nazm.lock` beside the root manifest, is `nazm.lock/1`: one entry per package in
name order — name, version, source (a path relative to the root package, `/`-separated) and
digest — and each package's dependency names. `nazm lock DIR` writes it, and writes it only when
its bytes would change: resolving the same packages twice gives the same bytes. Nothing else
writes it. `--locked` makes it authoritative: the build resolves as usual and refuses — before
compiling anything — if the lockfile is missing, if any package's identity, source or
dependencies differ from it, or if any package's bytes no longer match its digest (tampering).

**Offline, always.** Every v1 source is a path, so resolution never reaches a network; a missing
dependency directory is refused by name. There is no download step to disable.

**Builds and caches.** `nazm check|build|run DIR` builds the package in `DIR` from its `main`.
Module keys of dependency modules carry `NAME@VERSION`, and every cache key already covers a
module's source bytes and its dependencies' interfaces (§7.6), so a changed dependency invalidates
its dependents and nothing else, and an unchanged one is reused. The build's configuration — the
backend, its version and settings, the optimisation level, the target and the runtime ABI — is in
every object key (§7.43, §7.45): that is the profile, and there is no second profile mechanism.

**Environment.** A build reads no environment variable that changes its output, except the ones
the C toolchain is handed by name (`nazm-cli/src/backend.rs`, `PASSED`), which the toolchain's own
identity then covers. There are no build scripts.

**Reproducibility, stated exactly.** The same package sources, lockfile, toolchain, target,
backend and configuration give byte-identical executables — tested on one host by building two
copies of a package in two directories from empty caches. Nothing is claimed across operating
systems or toolchain versions.

**Amended during implementation.** Three facts the first draft did not know. *The linker*: the
Apple linker gives every link a new UUID unless `ZERO_AR_DATE=1` is in its environment, and
derives the UUID and the ad-hoc signature from the output's file name — so the link is made with
that variable set, under the final file name, inside the build's working directory, and renamed
into place (`two_copies_built_from_empty_caches_are_byte_identical` failed until both were done).
*Stored keys*: `ModuleKey::parse` read back only the prelude's key and project paths, so neither a
dependency's module (`NAME@VERSION/…`) nor a standard one (`@std/…`, N45's — a gap N45's tests did
not cover) was ever reused from the cache; both now read back as themselves. *The first
component*: only a key's first component may not hold `@`, because `lib/@core/x.nz` was already a
project's own key. The source-root decision (`nazm-service/src/root.rs`) learned a fourth answer,
`package`, reported by `--cache-report` and `--build-report` as a new value of `root`.

## 7.49 Restriction profiles — N47, 2026-10-01

Written before the code. One language, and profiles that refuse some of its programs.

**What a profile is.** A named set of rules, each with a stable identity, each reading a fact the
compiler already settled and passing or failing with witnesses. A profile may reject a program;
it never changes what an accepted program means, compiles to or does. Every program a profile
accepts, `general` accepts, and runs identically. There is no profile-specific syntax, built-in or
code path, and nothing in a profile reads a name: an effect is an `Effect`, a contract is a
function's `declared_effects`, a build's lock is `nazm-package`'s verification.

**The rules.**

| Rule | Fact | Refuses |
| --- | --- | --- |
| `declared-effects` | each function's declared contract | any function of the program's own modules (not the prelude's or a standard module's, all of which declare theirs) with no `! { … }` — the ambient bridge of N37 |
| `no-io` | the effect set `main` requires | `io` anywhere in the program, through any call |
| `no-spawn` | the same | `spawn`: tasks, and so the scheduler |
| `no-foreign` | the same | `foreign`: any call into C |
| `locked-build` | the build | a build that is not a package build whose lockfile verifies |

`main`'s required effect set is transitive by construction (N36): through every call of every
module, standard wrappers included — `io_println` carries `io` because it calls `print` — and an
undeclared function of another module is assumed to have every bridged effect, so an effect rule
over-refuses rather than misses.

**The profiles.** `general`: no rules. `embedded`: `no-io`, `no-spawn` — no operating-system
services and no scheduler; C (hardware access) is allowed. `critical`: `declared-effects`,
`no-spawn`, `no-foreign`, `locked-build`. `cyber`: `declared-effects`, `no-foreign`,
`locked-build`; because every function then declares its effects, N38's restricted flow (`N0372`)
applies to every function of the program. None is a certification, a sandbox or a proof of
anything beyond its rules.

**What is not a rule, and why.** No allocation rule: allocation is not tracked. No determinism
claim: forbidding `spawn` removes one source of nondeterminism, not every one. No provenance rule
beyond N38's, which `declared-effects` extends to the whole program.

**Selection and propagation.** `--profile NAME` on `check`, `build` and `run`, and `profile =
"NAME"` in any package manifest of the build. The profiles in force are all of them; their rules
are the union, evaluated over the whole program — so a dependency that requires `embedded` holds
every program it is part of to it. An unknown name is `N0511`. No environment variable selects one.

**When, and caching.** After a full check, before the command does its own work: `nazm build`
under a profile that refuses writes nothing. The rules read the facts of an uncached check, and
their verdicts are never cached, so a check cached under one profile can never carry its
acceptance into another — the cost is one check more whenever a profile other than `general` is in
force. What is compiled does not depend on the profile, so no object key needs it.

**Diagnostics and the report.** Each failed rule is one `N0510` naming the profiles in force and
the rule, pointing at its first witness — for an effect, the chain of calls from `main` to the
built-in, `spawn` or contract where it enters — with the rest as secondary labels. `nazm check
--profile-report` prints `nazm.profile-report/1`: the profiles, the outcome, and every rule's
verdict and witnesses in rule order, byte-identical for the same program and build, and marked as
compiler evidence rather than certification.

## 7.50 Inspection, debugging and profiling — N48, 2026-10-01

Written before the code. Three surfaces over facts the compiler already has, none of which
changes what a program does.

**The inspector.** `nazm inspect FILE|DIR [--def NAME]` prints `nazm.inspect/1`, one line of JSON
with sorted keys: the toolchain's identities (compiler version, semantic epoch, interface schema,
runtime ABI revision, Cranelift version, host target), the package and the state of its lock
(`verified`, `absent`, `mismatch`) and the profiles its manifests require, whether MIR validated,
and for each definition in identity order — its durable identity, parameter and result types,
whether it is generic, declared and required effects, the capabilities it takes, its provenance
summary (the origins its result carries, the parameters it passes on and those reaching a sink),
its foreign symbol, its Core IR semantic and body digests, and for each concrete function MIR has
for it (an instance per type for a generic one) the executable digest and its block, statement
and local counts. Excluded by construction: absolute paths, times, addresses, process ids,
iteration order. The same program, toolchain and build give the same bytes.

**Debug information.** `nazm build --debug` emits DWARF through the LLVM backend: a compile unit
per object, a subprogram per function — the Nazm name, the native symbol as its linkage name, its
file and the line of its declaration — and a source position on every instruction, taken from the
span MIR recorded for the statement or terminator it was emitted for. Compiler-made code carries
the position of the statement it belongs to, or its function's line; nothing is given a position
the source does not have, and runtime and entry objects carry none. On macOS the executable's
debug information is gathered into `OUTPUT.dSYM` with `dsymutil` before the objects are removed.
What is claimed: a debugger resolves a breakpoint by Nazm function name or by file and line, and
the line table has a row per statement, which is what stepping by statement reads. *Narrowed during
implementation:* the first draft also claimed frames and stepping in a live session; launching a
process under lldb waited on macOS's debugger authorisation in the non-interactive environment N48
ran in, so the live half was not exercised and is not claimed. What is not: local variables (no
`DILocalVariable`), types, the Cranelift backend (`--debug` with it is refused, never silently
ignored), and stepping into the runtime. Without `--debug` nothing changes: the emitter writes the
same text it always did.

**Timings.** `nazm build --timings` writes `nazm.timings/1` to stderr: the wall time of each phase
in the order it ran — load, front end (parse, check, effects, capabilities, provenance, Core IR),
MIR, LIR, code generation, native compilation, link — and the total. The front end is one phase
here because the CLI runs it as one step; the per-pass split of the front end is the benchmark
`what_each_phase_costs` in-process. The runtime's own counters stay `NAZM_MEMORY_REPORT`.

**The v1 release audit.** N48 also audits what exists against what is claimed: every capability
area's status, the security and safety wording across the documents, a centralised list of known
limitations, release-candidate notes and a human release procedure — and freezes the v1
performance baseline. Nothing is pushed, tagged or published by it.

## 7.51 Usability and native completeness — N49, 2026-10-01

Written before the code. Four small mechanisms, each closing a line of `limitations.md`, and an
audit of every construct the interpreter runs and a native backend does not. None is a new
semantic layer: each is owned by a layer that already owns its neighbours.

**String escapes — the lexer's.** A string literal may contain `\n` (10), `\t` (9), `\r` (13),
`\0` (0), `\\` (92), `\"` (34) and `\xHH` for `HH` in `00`–`7F` — two hexadecimal digits, either
case. Each escape **is one byte**; nothing else in a literal changes, so a non-ASCII character
written in UTF-8 source is its UTF-8 bytes as before. Any other backslash sequence, a `\x` with
fewer than two hex digits, or a `\x` above `7F` is refused (`N0004`) at the escape's own bytes,
and the literal is still one token so recovery is unchanged. `\x80`–`\xFF` are refused rather than
defined because the reference representation of a literal is UTF-8 text in every layer from the
token to MIR, and widening it to arbitrary bytes is a representation change with no user it would
serve that `str_from_byte` does not; no `\u{…}`, because `Str` is bytes and a Unicode escape is an
encoding decision. Decoding happens once, in the lexer; the token carries the decoded value and
the span of the source bytes, so every diagnostic about a literal still points at what was
written. The formatter prints a literal's source bytes, never its value, so escapes are kept
exactly as written: the canonical form is the written form. **Semantic epoch moves (11 → 12):**
a literal containing a backslash meant its bytes literally before N49 and means an escape after
it, so a cached check of such a source is not a check of the new language. The compiler written
in Nazm does not decode escapes and **refuses** a literal containing a backslash rather than
reading it the old way.

**Logical operators — the parser's and the checker's, lowered away in Core IR.** `!e` (prefix,
beside unary `-`), `a && b` and `a || b`; `||` binds loosest, then `&&`, then the comparisons —
`a == b && c < d || e` is `((a == b) && (c < d)) || e`. Every operand is `Bool` and so is the
result (`N0304` otherwise, as for any operator). `&&` and `||` are left-associative and
**short-circuit**: the right operand is evaluated only when the left does not decide the result,
so its effects, its failures and its non-termination happen only then. Core IR lowers them away
— `a && b` to `if a { b } else { false }`, `a || b` to `if a { true } else { b }`, `!e` to
`if e { false } else { true }` — so MIR, both backends and the interpreter run them with no new
case, and short-circuiting is a fact about control flow they already share rather than three
implementations of it. The effect, capability and provenance analyses see the operands as the
`if` they become: the right operand's effects are the expression's, conditionally, as any
branch's are. No new code; the epoch already moves for escapes. The compiler written in Nazm
refuses the three operators by name.

**Native recursion — MIR's stack checks and the runtime's guard.** Recursion, direct and mutual,
across modules and packages, compiles under both backends. The contract: **a native call that
would exhaust its thread's stack fails with `N0408` at the function it entered, after which the
failure unwinds like any other** (cleanup runs, a scope joins its tasks, `main`'s failure is
reported and the process exits 2). The mechanism: MIR lowering places a `StackCheck` statement
with a failure edge first in every function that is in a call cycle — the cycles of the call
graph without `spawn`, which starts a task on its own stack — and each backend emits it as a call
to the runtime's `nz.stack_check` and a branch to the failure edge. `nz.stack_check` compares the
frame address with the thread's limit, which it computes once per thread on first use: the
current frame minus a budget of three quarters of the smaller of the process's `RLIMIT_STACK` and
8 MiB. Task threads are created with an 8 MiB stack (`pthread_attr_setstacksize`; Linux's default
already, and raised from macOS's 512 KiB), so every thread's budget is within its stack. **What
is guaranteed, and what is not:** the check is made on entry to every function in a cycle, so
unbounded recursion meets it; a chain of non-recursive calls between two checks is bounded by the
call graph but its stack is not measured, so the guarantee holds when no such chain plus the frames
it builds exceeds the quarter left as headroom (2 MiB at 8 MiB). A program whose non-recursive
frames are that large can still exhaust the stack and be killed by the operating system; no
measured program comes close. There is no tail-call optimisation and none is promised; the
interpreter's limit stays its own (`--max-depth`, `N0403`), so the two implementations agree on
what a terminating program produces and each reports its own limit. **Runtime ABI revision moves
(2 → 3):** generated code calls a runtime entry it did not before. The compiler written in Nazm
still refuses recursion by name: its emitter has no stack check, and the reason it refused is the
reason it still does.

**Qualified names — the loader's, the checker's and the service's.** `use "PATH" as NAME;` imports
a module **under a name**: its exports are not brought into scope unqualified, so they collide with
nothing, and each is written `NAME::item` wherever a definition is named — a call `t::split(s)`, a
construction `g::Point(x: 1)`, a variant `g::Shape.Circle(r: 2)`, a type `g::Point`. `as` is
recognised only after a `use` path, so no identifier is taken from programs. The alias is the
module's within the importing module only; two aliases may name one module; an alias that is
also a definition's name, or a second alias of the same name, is refused (`N0209`), and a
qualified name naming an alias that does not exist or an item the module does not export is
refused (`N0210`) with the module's exports as candidates. **One resolver:** the checker resolves
`NAME::item` against exactly the interface entry the unqualified import would have used, so a
qualified call has the same `FnId`, durable identity, effects, capabilities and provenance as an
unqualified one, and everything downstream — Core IR, MIR, both backends, the interpreter, the
cache — sees no difference; the service's references, rename, semantic tokens and completion key on
the item's name span, which is the same span. A plain `use "PATH";` is unchanged. The compiler
written in Nazm refuses `as` and `::` by name.

**The construct audit.** `nazm capabilities --json` is read row by row against both backends; every
construct the interpreter runs and a backend refuses is classified as deliberate, a limitation, or
a bug, and only the gaps above are closed here (`limitations.md` says which remain).

## 7.52 Function values and closures — N50, 2026-10-01

Written before the code. One callable model: a **function value** is a value of a **function type**,
made from a named function or from a closure expression, and called the way a function is. Traits
and methods are not part of it, for the reason under *What is not here*.

**The type.** `fn(T1, …, Tn) -> R`, optionally `! { e… }`, is a type wherever a type is written:
parameters, results, bindings, fields, payloads, `Vec[…]` and type arguments. It is
**structural** — two function types are one type exactly when their parameter types, result type
and effect set are equal — and **invariant**: no subtyping, so a pure function is not a
`fn(Int) -> Int ! {io}` (effect polymorphism and subsumption are N51's). Unwritten effects mean
the empty set. A function type has **no equality**, as a capability has none: equality of code is
not decidable, and identity of objects is not a meaning the language has chosen.

**The values.** A non-generic, non-foreign function written as a value (`let f = double;`) is a
function value of its signature's type. A **closure** is `fn(x: Int) -> Int ! {e} { body }` in
expression position: parameters and result written in full, as a declaration's are (no inference
across a function boundary), its effects declared or empty. Its body may name bindings of the
functions and closures that enclose it; each one it names is **captured by value** when the
closure is created — a copy, so a handle (`Ints`, `Vec`, `Chan`) is shared and a value is not.
A `let mut` binding may not be captured (`N0386`): a copy of it would silently stop tracking the
binding, and sharing state is what handles and channels are for. A capability may not be captured
either (`N0386`): authority is passed as a parameter, so a closure has no authority its type does
not show. A generic function, a built-in and a foreign function are not values (`N0387`); a
closure over them is.

**Calls.** `f(args)` where `f` is a parameter or binding of function type is an **indirect call**:
arguments are checked against the type's parameters, the result is its result, and the call
exercises the type's effects exactly as a call to a declared function exercises its declaration's.
A local of function type hides a function of the same name for calls in its scope, as any binding
hides an outer one. A function value stored in a field or a payload is called by binding it first
(`let g = r.f; g(1)`): `r.f(1)` is the shape of a variant construction and keeps that meaning.
Function values may not be passed to a task (`N0321`): their type does not say whether what they
captured may cross into one.

**Effects, authority, provenance.** A closure's body is checked against its own declared effects,
and creating a closure performs none of them; calling a function value performs its type's. A closure's authority is its capability parameters and nothing else. Provenance: the result of an
indirect call is of unknown origin, joined with what its arguments and the function value carry —
which function runs is not known where the call is. A closure's body is walked as a function of
its own: its parameters are of unknown origin (its callers cannot be named), and a capture carries
the origins of the binding it copies, or unknown when that binding depended on its maker's
parameters or calls. A restricted sink reached from either is refused, conservatively.

**One lowering.** The checker registers each closure as a definition of its own — a private
function of the enclosing function's module, identified by the enclosing definition and its
position among that body's closures — with its captures recorded in order of first mention. Core IR
lifts it: a closure expression is `Closure { func, captures }`, a named function used as a value is
`FnValue(func)`, an indirect call is `CallIndirect { callee, args }`, and the closure's body is a Core
IR function marked as one, whose local 0 is the closure itself, a parameter, followed by its declared
parameters; its captures are locals its body region owns, listed in order. MIR keeps the same three, adds the **environment**:
every function a function value can reach — a closure's body, and a **thunk** MIR makes for each
named function used as a value — takes the environment as its first parameter and reads its
captures from it at entry (`Rvalue::Capture`). Both backends and the interpreter consume exactly
that; none rediscovers a closure.

**The calling convention.** A function value is one pointer to a **closure object**: a reference
count, the code pointer, a release function, then the captures in order, each at its type's slot
size. Each backend lays the captures out itself, and an object never crosses from one backend's code to
the other's. Calling it passes the object as the first argument and the arguments after. Copying a
function value retains the object and dropping one releases it; the last release calls the release
function, which releases every managed capture, and frees the object. A named function's value is
an object like any other, made where the value is written: its code is the thunk, it carries
nothing, and its release function is null. *(Amended with the code: the constitution first had a
static immortal object here, which would have given the two kinds of value two ownership laws; one
law, at the cost of an allocation per use, is the simpler truth.)* Making an object can fail for
want of memory (`N0406`), so MIR gives `Closure` a failure edge. The runtime owns allocation and the
reference count (`nz.closure_new`, `nz.closure_retain`, `nz.closure_release`), atomically as a
string's, and counts objects for the memory report, whose line gains a fourth class, `closures`;
the runtime ABI revision moves (3 → 4). A function that makes an indirect call checks its stack
first, as a function in a call cycle does (N49): which function the call reaches is not known, so
any cycle through one contains such a function.

**Durable interfaces.** A function type in an exported signature, field or payload is persisted as
`{"fn":[…],"ret":…,"effects":[…]}`. *(Amended with the code: the schema does not move.)* A `/7` reader
that meets that shape cannot read it as any of the four it knows, refuses the file and treats it
as no entry — never wrong, only unable — so the major is kept, by the rule that a schema moves only
when an old reader would otherwise be wrong. A closure is never exported. Captured bindings are one
binding to a reader: `Resolution::binding_of` maps a closure's copy back to what it copies, so
references and rename see every use.

**The higher-order library.** `@std/seq` gains `vec_map`, `vec_filter` and `vec_fold`, written in
Nazm over `Vec[T]` and function values, with no compiler knowledge of them.

**The compiler written in Nazm** has neither: its parser stops at a closure or a function type with
its ordinary syntax error. That is the recorded parity gap; nothing in it needs a function value.
Its emitted programs print the memory report's `closures` class, always zero, so the two compilers'
reports still compare line for line.

**What is not here, and why.** *Traits and methods.* N50's prompt makes them conditional on a need
generics and higher-order APIs demonstrate. With function values, the needs in hand — mapping,
filtering, folding, ordering or rendering by a supplied function — are met by passing the function;
none requires a constraint the caller cannot supply as an argument. A trait would add a second
abstraction (dictionaries or instances, coherence, orphan rules, method lookup) for no program the
language cannot already express, so they are **designed and deferred**: the design questions —
trait identity as a durable definition, coherence as one impl per trait and type in the trait's or
the type's module, method syntax as resolution sugar over the same functions, monomorphisation
before dictionaries — are recorded here and settled when a program needs them. *Function values
across tasks*, *generic function values*, *closures that call themselves*, *calling a field
directly*, and *a closure or a named function value inside a generic function* (`N0387`: each
would need an instance or a thunk per type argument) are refused rather than half-supported.

## 7.53 Effects and capabilities v2 — N51, 2026-10-02

Written before the code. N50 made function values, and with them the gap N36 recorded: a function
type's effects are fixed, so `vec_map` could take only a pure function, and a closure could not
hold a capability, so even an effectful one had no authority to use. This section closes that gap
with the smallest mechanism that does, bounds the N37 bridge at the one edge N50 opened, and
settles the capability questions N51 asks — implementing what a program in hand needs and
deferring, with the cost stated, what none does.

**Effect parameters.** A function may declare **one** effect parameter, in its type parameter
list after its type parameters: `fn vec_map[T, U, effects E](xs: Vec[T], f: fn(T) -> U ! { E })
-> Vec[U] ! { E }`. `effects` is a word only there. The parameter's name may be written in any
effect set of the function's own signature — its parameters' and result's function types and its
declared set — and in the effect sets of closures in its body; nowhere else. It stands for a set of
effects the caller chooses. In the body it is opaque: calling a parameter whose type has `E`
performs `E`, which the declared set must therefore include, and `E` needs no capability — the
function value that performs it carries its own authority (below). One parameter per function is
a decision, not an omission: every higher-order function in hand (map, filter, fold, apply,
compose over one effect) needs one, and a second would bring row unification with no program to
test it. Effect parameters do not make a function generic for code generation: effects are
metadata from Core IR down, so no instance is made per binding. `main` may not declare one.

**Binding at a call.** At a call to a function with an effect parameter, the parameter is bound by
matching each argument's type against its parameter's, structurally, as type parameters are
(N50's unification extended to effect sets): where the parameter's function type writes `! { k…, E }`
and the argument's is `! { a… }`, `E` binds to `a… − k…`, and `k…` must be within `a…`. Bound twice
to different sets is `N0389`; bound nowhere, it is the empty set. The call then performs the
callee's declared set with `E` replaced by its binding, and its argument types are checked against
the parameter types with `E` replaced, so a mismatch is the ordinary `N0300`. Inside a function
with its own effect parameter `F`, an argument's type may carry `F` (it is a parameter of the
caller), and a binding of `E` may contain `F` — which the caller's own declared set then needs, by
the same rule. So `vec_map(xs, f)` inside `run[effects F](f: fn(Int) -> Int ! { F })` performs `F`.

**Capabilities a closure captures.** A closure may now capture a capability (`N0386` is narrowed to
`let mut`): the capability is copied into the closure, as any value is, and the authority the
closure's body holds is its own capability parameters **and** its captured capabilities.
*(Amended with the code: "captured" is lexical — a closure holds the capabilities visible where
it is written, as a block does, whether or not its body names them. A capability has no run-time
representation to copy, so the two readings differ only in which programs a reader must study,
and the lexical one is the rule every other body already follows.)* The
closure's type still shows what it does — its declared effects — so a function value's type is
an honest statement of what calling it may do, and the value carries the authority for it.
Copying a capability confers nothing new: it is a token that already exists. **Delegation** is
passing one as an argument or capturing it; **duplication** is copying it; **storage** is as any
value's (a field may hold one, as before); **task transfer** is unchanged — a capability may be
passed to a task where its kind allows, and a function value may not be passed to one (`N0321`).
**Narrowing** has no form: there are no attenuated kinds (below).

**The bridge, bounded.** N37's compatibility bridge stays as it is for code that declares nothing:
an undeclared function exercises its caller's authority, and a declared caller must hold what the
callee's effects need (`N0369`). N50 opened one more edge: a *value* of an undeclared function.
Its type assumes the empty set if it is this module's (and the function is held to that, `N0366`)
or the bridged `{ io, spawn }` if it is another module's — so calling the value performs those
effects, and since a call through a value needs no authority, declared code could reach the
ambient bridge through it. N51 closes the edge where the value is **made**: making a value of an
undeclared function requires, at that point, the capabilities its type's effects need, exactly as
calling it would (`N0369`). Strict code therefore cannot acquire authority through an undeclared
import or callback. The policy is stated rather than migrated: no accepted program changes
meaning; a program that made such a value without holding the authority was unsound and is now
refused, which the semantic epoch records.

**Attenuated capabilities: deferred.** No program in the tree — the compiler written in Nazm, the
examples, the standard library — needs authority narrower than a kind. The first candidate, an
`IoCap` scoped to a path prefix, needs a semantics for path containment and the provenance of the
path (N52); building it without a program would be inventing the use case.

**Revocation: deferred, with its cost.** A capability is a value with no identity: two copies are
indistinguishable, and nothing records that one was handed out. Revocation needs identity — an
allocated cell per granted capability, a check at every use, and an operation that flips it —
which is runtime state and a runtime cost at every authorised call. A static flag would be a
claim the representation cannot back, so there is none.

**The trust root** is unchanged: `main`'s parameters are the only source of a capability (`N0370`
refuses building one; `N0371` a `main` parameter that is not one), an interface carries capability
*types* and never values, and a closure's capture copies a capability that already exists.

**Representation.** `Effect::Param` is a fourth effect: *the effect parameter*. In a function's
declared set it is that function's; in a function type it is the parameter of the function type's
`row` owner (`FnSignature::row`, a `ParamOwner`), so a type that mentions one function's parameter
is never confused with another's. It is never a built-in's effect, needs no capability, is not in
the bridged or the assumed set, and is not a name a set may write except as the declared
parameter's. Substitution replaces it as a type parameter is replaced; the interface writes it as
`"*"` in an effect list, a value a `/7` reader refuses as an unknown effect rather than misreads, so
the schema does not move. Effects analysis treats it as one more effect in the fixed point — what a
body requires may include it — and a call's contribution is the callee's set with it bound.
Below the checker effects mean nothing and a binding makes no instance, so Core IR's and MIR's
verifiers compare the types crossing a call **up to effects** (`same_shape`): the callee's
parameter `fn(…) ! { E }` takes the caller's `fn(…) ! { io }`. A call's result type is the bound
one, recorded on the call. That is also why the parameter may not be named inside a type's
arguments (`N0388`): `Vec[fn() ! { E }]` and `Vec[fn() ! { io }]` would be two `Vec` types, with
two element tables, for one value.

**The standard library** takes the parameter: `vec_map`, `vec_filter` and `vec_fold` are
`effects E` over their function argument, so a printing closure is accepted and its caller is held
to `io`, through the wrapper, not around it. **Profiles** read the effect set `main` requires,
which now includes what a bound parameter contributes, so `embedded`'s `no-io` refuses a program
whose only `io` is a closure handed to `vec_map`.

## 7.54 Resource facts and information flow v2 — N52, 2026-10-02

Written before the code. Two gaps, one each side of the language. The capability matrix lists
allocation as untracked: a reader, an agent or a profile cannot ask where a function allocates,
retains, blocks or starts a task. And provenance has one source rule and one sink — a path
`write_file` writes to — so a program that echoes a file's contents to standard output is
invisible to every policy, and a program that has checked a value has no way to say so.

**Resource facts are read off MIR**, where every operation that allocates, retains, blocks, talks
to a channel, calls C or starts a task is already an explicit statement — no layer below has to
rediscover them and none above can see them. The classes, each from its MIR form:

| class | MIR |
| --- | --- |
| `allocation` | a built-in that makes storage (`ints_new`, `strs_new`, `vec_new`, `chan_new`, `str_concat`, `int_to_str`, `str_from_byte`, `str_join`, `read_file`, `arg`), a closure object (`Rvalue::Closure`), and a push that may grow its sequence |
| `retain` | a copy of a managed value (`Rvalue::Copy`, a field or payload read, a capture) |
| `block` | `chan_send` and `chan_recv` on a bounded channel, and a `scope`'s join |
| `channel` | every channel operation |
| `foreign` | a call to a foreign function |
| `task` | a `spawn` |

Each site carries a **count per call of its function**: `at-most-once` when its block is in no
cycle of the function's control-flow graph, and `unknown` when it is — inside a loop the count
depends on values the compiler does not know. `unknown` is a value of its own and is never
written as a number; there is no "zero" in the vocabulary, because a site that exists may run. A
count is per call: what a caller's loop makes of it is the caller's site. Nothing here is a
measurement, and nothing claims an upper bound on bytes. `nazm explain-cost` prints the sites,
by function, for a reader and as `nazm.cost/1` for a tool.

**A profile reads them.** `embedded` gains `bounded-allocation`: no allocation site whose count is
`unknown`, so an accepted embedded program allocates a number of times bounded by its call
structure (recursion is already a cycle the rule sees through its calls' sites; an indirect call
is reported as such).

**Provenance v2: a registry of sinks.** A sink is an argument of a built-in that takes data out of
the program, by class: `path` (the path `write_file` writes to, N38's), `output` (what `print`
and `eprint` write) and `file-data` (what `write_file` writes). The sources are N38's four origins.
Every sink is reduced to flows and solved through summaries exactly as the path sink is — through
helpers, higher-order calls (N50's conservative indirect-call rule) and modules — and every solved
sink is recorded with its origins. The language's own restriction is unchanged: `N0372` for a
restricted origin at a `path` sink in a declared function. What other classes may receive is
**profile policy**: `cyber` gains `contents-stay-in-files`, refusing a file's contents (or an unknown
origin) at an `output` sink. **Containers** stay conservative, and say so: a value read out of a
sequence or a channel is `unknown`, the whole container standing for every element, because a
local analysis cannot see every alias that wrote to it.

**Declassification is authority, not a name.** `str_vouch(s: Str) -> Str` returns its argument with
no origin. It needs a `VouchCap` held where it is written — in every function, declared or not, as a
foreign call does — and `VouchCap` is a new capability kind, minted only for `main`'s parameters like
every other. Nothing is inferred from a function's name, and nothing else clears an origin. Each
vouch is recorded with the origins it cleared, which `nazm explain-flow` (`nazm.flow/1`) prints
beside every sink, so the evidence is machine-readable; `critical` gains `no-declassification`,
refusing any vouch at all.

**Bounded and deterministic.** The provenance fixed point is over finite lattices (origins ×
parameters) and its worklists re-queue callers only on growth; a stress test holds a 2,000-function
call chain to a deadline and to identical output on two runs. Resource facts are one pass over each
MIR function's blocks.

**Versions.** The semantic epoch moves: `str_vouch` and `VouchCap` are new names a program may use,
and a program that declared a type or function of either name moves from accepted to refused. The
check artefact's sink facts gain a class; an entry from an older checker is unreachable by the epoch.
`nazm.cost/1` and `nazm.flow/1` are new schemas.

## 7.55 The runtime component and generic channels — N53, 2026-10-02

Written before the code. Two things the runtime could not do: be a component of its own, and carry
anything through a channel but an `Int`.

**The boundary.** The runtime — every function and global generated code calls, its typed
inventory, its OS adapter, the platform entry and its identity — moves out of the LLVM backend's
crate into **`nazm-runtime`**, which both backends depend on and neither owns. What stays
compiler-generated is what depends on a program: per-type retain, release and equality helpers,
closure release functions, task trampolines. The runtime is still LLVM IR text compiled by clang,
for both backends: rewriting it as Rust compiled to an object would need a `no_std` runtime with
no unwinding, built per target at `nazm build` time or shipped prebuilt per target, and a linker
step neither backend has today — a research item (`docs/research-register.md`), not this
milestone's. The Cranelift backend therefore still needs clang for the runtime object, and both
need the system C library and linker; that is stated, not hidden.

**Identity.** The runtime ABI revision stays a number a reader can talk about, and beside it the
runtime gains a **digest** — of every part's text, the entry, the inventory and the globals — that
enters every object's cache key. A change to the runtime that forgot to bump the revision still
changes every key, so an object built against another runtime is never reused. Revision 5 is this
milestone's: generic channels.

**`Chan[T]`.** A channel of any type that may cross into a task (`Int`, `Bool`, `Str`, `Chan`,
`Chan[U]`, and records and enums of those) — the same rule `spawn` applies to an argument, now
also the rule for being a channel's element (`N0390` otherwise). `Chan` alone is unchanged: the
`Int` channel, its four built-ins and its runtime. `Chan[T]` has its own: `chan_new_of[T](capacity)`,
`chan_send_of(c, v)`, `chan_recv_of(c, out: Vec[T])` and `chan_close_of(c)`, with exactly `Chan`'s
blocking, closing and draining semantics. **Ownership:** a send copies its value into the channel
(the sender's binding is untouched, as every argument is); a value refused by a closed channel is
released at once; a receive moves the value out of the channel into the vector; a channel destroyed
with values still queued releases each of them, by a release function generated for `T` — the
helper a `Vec[T]` already has. Equality, ordering and a task's use are as `Chan`'s. The memory
report counts generic channels with the others.

**Runtime representation.** A generic channel is a ring of fixed-size slots, the slot size and the
element release function given at creation; send and receive copy one slot under the channel's
lock. The `Int` channel keeps its own ring, so nothing about existing programs changes.

## 7.56 Select, deadlines, cancellation and the scheduler's counters — N54, 2026-10-02

Written before the code. §7.46 left five things out: a worker pool, cancellation, timeouts, select
and any count of what the scheduler did. This section settles what N54 adds, and — as carefully —
what it decides not to build yet and why.

**The model stays one OS thread per task.** An M:N scheduler needs tasks that can be suspended off
an OS thread: either a stack per task that a worker switches to (stackful), or compiled
continuations (stackless: every function that may block becomes a state machine). Stackless is
ruled out for Nazm: it colours every function that can reach a channel operation, doubles every
backend's lowering, and breaks the debugger's frame model (§7.50). Stackful is the model a pool
would use — a guarded `mmap`'d stack per task and a small per-target switch routine — and it is not
built here, because three things the runtime already promises are thread-local and would silently
break when a task migrated between workers: the first-failure slot (`nz.failed`, §7.45), the stack
limit `nz.stack_check` budgets from (N49), and the C library's own per-thread state any foreign call
(N42) may rely on. Each needs a task-local home first, and a foreign call that blocks needs a
hand-off so it cannot pin a worker. Until then a fixed pool would deadlock correct programs (§7.46),
so **bounded workers are DESIGNED, not implemented**: the stack model is chosen (stackful, guarded,
per-target switch), its prerequisites are named, and nothing in the language below depends on it —
every rule here holds unchanged under a pool.

**Suspension points.** The operations that may wait are exactly: `chan_send`/`chan_send_of` on a full
channel, `chan_recv`/`chan_recv_of` on an empty one, `chan_select_of`/`chan_select_until` with no
channel ready, and a `scope`'s join. Nothing else waits. These are also the only places a future pool
would park a task, and the only places cancellation is observed.

**Select.** `chan_select_of(cs: Vec[Chan[T]], out: Vec[T]) -> Int` waits until some channel in `cs`
is *ready* — it holds a value, or it is closed and drained — and returns the **lowest index** among
the channels ready when it looks. A ready channel holding a value gives its oldest value, moved onto
`out` as `chan_recv_of` moves it; a closed and drained one gives nothing, and `out` is unchanged.
`-1` means `cs` is empty. Tie-breaking is deterministic — lowest index — but *which* channels are
ready when select looks depends on how tasks were scheduled, so a select's result is classified
**schedule-dependent**, as any receive from a channel two tasks send into already is. A closed channel
is always ready: closing is a broadcast, which is what makes it a cancellation signal.

**Deadlines.** `chan_select_until(cs, out, ms: Int) -> Int` is `chan_select_of` that gives
up after `ms` milliseconds and returns `-2`; `ms <= 0` looks once and does not wait. Reading the clock
is reading the host, so it needs authority: a new capability, **`TimeCap`**, held where it is written,
in every function (as `VouchCap` is, N52). Like a vouch it performs no effect — reading a clock changes
nothing outside the program, and N51 already says *pure* means *no effect*, not *referentially
transparent* — so the authority is the one thing that gates it, and the thing a profile refuses.
`time_now_ms() -> Int` reads a monotonic clock in milliseconds, a fresh `Int` with no origin. So a function that may time
out says so in its signature, and a profile can refuse time outright: `critical` gains
**`no-ambient-time`**, refusing any use of `TimeCap`, because a program whose behaviour depends on a
clock is not reproducible. The interpreter and both native backends read the same host clock; a
timeout is a lower bound — at least `ms` elapse before `-2` — never a promise of promptness.

**Cancellation is cooperative and explicit.** A canceller closes a channel; every task selecting on it
sees it ready and decides what to do. Cancellation is observed only at the suspension points above, and
only by a task that looks — a task in a loop that never selects runs to completion. There is still no
cancellation on a sibling's failure: N44's rule that a scope reports its first failure in start order
and joins every task is kept unchanged, because cancelling siblings on failure would make which failure
is reported depend on timing. Cleanup is unchanged too: a cancelled task returns like any other, its
scope joins it, and everything it held is released on that path (§7.42).

**Ownership.** Select moves at most one value, from one channel, onto `out`; nothing else changes hands.
A value queued in a channel select did not choose stays queued. A timed-out select moves nothing.

**Counters.** With `NAZM_SCHED_REPORT=1` a program writes one line to standard error at exit,
`nazm-sched: spawned=… joined=… selects=… timeouts=…`: tasks started, tasks joined, select calls
completed, and selects that timed out. They are counts of what happened, not samples; the interpreter
and both native backends count the same events, and for a program whose selects cannot race they agree.
`queued`, `running`, `suspended`, wake-ups and steals are counts only a pool has, and are not invented
for a scheduler that has none.

**Implementation.** Select over typed channels only (N53's `nz.chanv_*`), so the `Int` channel's runtime
under the self-hosted compiler's parity gate is untouched. A select cannot wait on several condition
variables, so every typed channel's state change also advances one runtime-wide *event count* and
broadcasts its condition variable; a selector reads the count, scans the channels in index order under
each one's lock, and waits only if the count has not moved since it read it — the classic event-count
protocol, which cannot lose a wake-up. The cost is one extra lock per typed send, receive or close,
measured below (`docs/performance.md`, N54). The counters live in the runtime (`nz.sched_*`), called by
generated code after a task starts and before a scope joins.

**Not here.** A worker pool, task migration, work stealing, cancellation on failure, `select` over send
operations or over the `Int` channel, timers other than a select's deadline, async I/O, deadlock
detection.

## 7.57 FFI v2: C strings in, exported functions out — N55, 2026-10-02

Written before the code. §7.44 let a program call C with `Int` and `Bool`. N55 widens the boundary
in the two directions programs most need, and settles — without building — the shapes that need a
memory model Nazm does not have yet.

**A `Str` argument is a borrowed C string.** A foreign parameter may be `Str`. At the call the
caller makes a NUL-terminated copy of the string's bytes, passes C a `const char *` to it, and frees
the copy when the call returns: C borrows it for the call and must not keep it. A string containing
a NUL byte cannot be a C string, so such a call fails before C runs (`N0405`, at the call) — the
copy is never truncated silently. The cost is one allocation and one copy per string argument,
stated rather than hidden; zero-copy would need every Nazm string NUL-terminated, which a slice
cannot be. A `Str` **result** stays refused (`N0381`): who frees a `char *` C returns is a fact no
declaration states.

**An exported function is a Nazm function C may call.** `pub extern "C" fn name(p: Int, …) -> R ! {}
= "symbol" { body }` — an `extern` declaration *with* a body — defines `symbol` with the C calling
convention, its parameters and result the v1 scalars (`Int` as `int64_t`, `Bool` as `bool`). Its
body is ordinary Nazm, with three rules, all checked (`N0391`):

- **It is public and declares `! {}`.** C cannot hand Nazm a capability, so an export takes none and
  performs no effect: it computes, calling pure functions.
- **It is not generic and takes no `Str`.** A C caller has one signature and owns its own memory; a
  borrowed string *into* Nazm needs the lifetime rule a callback would, and is not here.
- **No failure unwinds into C.** A Nazm failure inside an export — an overflow, an out-of-bounds
  index, out of memory — cannot propagate through C frames, which have no cleanup Nazm knows of. The
  export's C entry reports it on standard error exactly as `main`'s entry does, and ends the process
  with status 2. An export either returns its declared value or the process ends with a Nazm
  diagnostic; it never returns a stand-in.

**Libraries.** `nazm build --lib FILE -o OUT.a` builds the program's modules without requiring a `main`,
writes a static archive of every unit's object and the runtime's, and `OUT.h`, a C header declaring
each export with `<stdint.h>` and `<stdbool.h>` types, in a fixed order. The archive's other symbols
stay hidden; only the exports are visible to a C link. One Nazm library per C program: two would
both carry the runtime. Either backend builds a library, and the header is the same text for both.

**Identity.** An export's symbol enters its function's Core IR and MIR digests and so its unit's
object key. It is not persisted in the interface: to another Nazm module an export is an ordinary
function, called by the internal ABI, and its C entry lives in the unit that defines it. Two exports
of one symbol, or one also imported from C, are refused when the program is built (`N0383`). Accepting syntax that was refused, and a
`Str` where `N0381` was, moves the semantic epoch (17 → 18).

**Not here, and why.** *Opaque handles* need a nominal type whose values Nazm never dereferences or
frees — designed (`extern "C" type T;`, a word at run time, no equality, not crossing into a task,
freed only by a C function the program calls) and not built. *C structs* need a layout guarantee
distinct from Nazm's records, which §7.43 deliberately leaves free. *Callbacks* — C calling a Nazm
function value — need a trampoline that finds its closure from a C `void *` and a rule for a
callback called after its environment is gone. *`errno`* is a per-thread location whose symbol
differs by platform (`__error` on macOS, `__errno_location` on Linux) and belongs with N57's target
model. Dynamic libraries and exporting to anything but C are also not here.

## 7.58 Packages v2: a local registry and version requirements — N56, 2026-10-02

Written before the code. §7.48 made every dependency an exact path. N56 adds the second source a
package ecosystem needs — a registry — and the version requirements that make it useful, while
keeping every N46 guarantee: no network, deterministic resolution, digests, a lockfile that is
authoritative under `--locked`.

**A registry is a directory**, named by the root manifest's `[registry] path = "…"` (relative to
it; a dependency's own `[registry]` is ignored, so one build has one registry):

```text
REGISTRY/index/NAME.toml              nazm.registry-index/1: name, and every version with its digest
REGISTRY/packages/NAME/VERSION/…      the published copy: its manifest and its sources, nothing else
```

There is no remote registry and no download: a registry somewhere else is a directory someone
copied here, and the copy is checked, not trusted. A correctness test never needs a public service.

**Coordinates and requirements.** A package is still its name and exact version. A registry
dependency names a requirement instead of a path: `shapes = { version = "^1.2.0" }`. Four forms:
`1.2.3` and `=1.2.3` (exactly), `^1.2.3` (the same left-most non-zero component: `>=1.2.3,<2.0.0`;
`^0.2.3` is `<0.3.0`; `^0.0.3` is exact), `~1.2.3` (`>=1.2.3,<1.3.0`). No pre-releases, no
arbitrary comparators, no features. A path dependency is unchanged: a path and an exact version.

**Resolution** visits the path graph as N46 does, then solves the registry requirements in rounds:
each round collects every requirement on every registry name — from the path packages and from the
registry packages chosen so far — and chooses, per name, the version the lockfile records if it
still satisfies them all, else the **highest non-yanked** published version that does. It stops
when a round changes nothing. **One version per name** stays the rule (§7.48), and there is **no
backtracking**: if no version satisfies every requirement, resolution is refused naming each
requirement and who made it (`N0513`). This can refuse a graph a backtracking solver would find;
it never chooses a surprising version, and its answer is a function of the manifests, the index and
the lockfile alone — not of their order on disk.

**The store refuses what a store must.** A published version is immutable: publishing it again is
refused (`N0512`). Every package is fetched through its index entry and checked before use: its
manifest names that package and version, it holds only regular files and directories (a symbolic
link is `N0514`: it could point anywhere), and its bytes match the digest the index records
(`N0507`: tampered after publishing). An index that is not `nazm.registry-index/1`, names another
package, records a version twice or a malformed digest is `N0512`; a version is parsed, so it can
never be a path. A published package depends only on the registry: a path means nothing in another
checkout. A registry package and a path package of one name in one build conflict (`N0503`); a
cycle through registry edges is `N0504`.

**Identity.** A registry package's source in the lockfile is `registry+NAME@VERSION`, and its
modules' durable keys are `NAME@VERSION/…` as every dependency's: where the registry is on disk
enters neither. The digest is N46's, over the published copy.

**Lock, frozen, yank.** `nazm lock` records the chosen versions; a later resolution prefers them
while they still satisfy the manifests, so publishing a newer version changes nothing until the
lockfile is rewritten. `--locked` refuses any difference, a stale lockfile included. `nazm yank
NAME@VERSION --registry DIR` withdraws a version: never chosen anew, still used where a lockfile
names it, and `--undo` restores it. `nazm publish DIR --registry REG` copies a package's manifest
and sources into the registry and records its digest; it writes the index by rename, so a reader
never sees half of one.

**The self-hosted compiler** reads no manifest: its resolver knows relative imports only. Teaching
it packages needs a TOML reader and a package graph written in Nazm — designed (the graph is
`nazm.lock/1`, which a later milestone can hand it already resolved) and not built here.

**Not here.** A network registry, signatures (a digest is integrity, not authorship), features,
pre-releases, version ranges beyond the four forms, multiple versions of one package, workspaces
with several roots, build scripts.

## 7.59 Targets and cross-compilation — N57, 2026-10-02

Written before the code. Since N41 a build named its target and refused every one but the host.
N57 makes a target a real input: four triples, each built by both backends from any host in the
matrix, linked where a linker for it is present and handed back as objects where none is.

**The target is a value, not the host.** `--target TRIPLE` is one of `aarch64-apple-darwin`,
`x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`: an architecture
(64-bit ARM or x86, little-endian, 64-bit pointers), an operating system (macOS or Linux) and an
ABI (Darwin's, or glibc's System V). No CPU feature beyond the triple's baseline is used by either
backend, so an object does not depend on the machine that built it. Anything else is refused by
name before anything is compiled.

**What changes per target, and what does not.** The runtime text is the same for all four: it
names only what both C libraries share under the same values (`RLIMIT_STACK` 3,
`CLOCK_REALTIME` 0, `CLOCK_MONOTONIC_RAW` 4, `struct timespec` two 64-bit words, an attribute
object of at most 128 bytes) — a fact §7.45 already relied on, now stated as the portability rule
the runtime keeps. What changes is the code generator's: clang is told the triple (`-target`), and
Cranelift is built for it (its `x86` and `arm64` backends both compiled in). The object format —
Mach-O or ELF — follows.

**Host tools and target artefacts are separate.** A build runs the compiler, clang and the linker
— host tools — and never runs what it produced: there is no build script, and nothing in a build
executes a target binary. Whether a produced program can run on the host is the test harness's
question, answered separately: an x86_64 macOS program on an ARM Mac runs under Rosetta when it is
installed; a Linux program runs in a Linux container. The two are reported apart, as
**run-verified** and **compile-only**.

**Linking.** A target linked by the host's own toolchain — the host's triple, or the other macOS
architecture, whose SDK the Apple toolchain carries — produces an executable. A target the host has
no linker for (Linux from macOS, macOS from Linux) produces **objects**: `nazm build --objects DIR`
writes every unit's object, the runtime's and the entry's, and `DIR/link.txt` listing them in link
order with the system libraries the target needs (`-lpthread`), for the target's own toolchain to
link. Asking for an executable for such a target is refused by name, not attempted with a host
linker that would produce the wrong thing. No linker is found on `PATH` by accident: the one used
is the driver's own, and the driver's resolved command line for the target is in every key.

**Identity.** The target triple is in every object's key under both backends — LLVM's through the
driver's resolved configuration, Cranelift's directly — so an object built for one target is never
reused for another. `nazm inspect` and the build report name the target. A package's lockfile is
target-independent: a package's sources do not depend on the target in v1, and none may yet.

**Reproducibility across hosts.** Same-target builds from different hosts are a separate claim from
same-host reproducibility. N57 records what it measured — for which targets two hosts' objects
agree — and claims nothing it did not measure.

**Not here.** Windows, 32-bit or big-endian targets, a target-specific runtime part, CPU feature
selection, a bundled linker or sysroot, target-conditional source, and emulation inside the
compiler.

## 7.60 Debugger and profiler v2 — N58, 2026-10-02

Written before the code. §7.50 gave a `--debug` build functions and lines, and left two things
unclaimed: a live session and variables. N58 claims both, for what it can show, and says where the
evidence comes from.

**Where a live session is verified.** On the development Mac, launching a process under lldb waits
on the operating system's developer-mode authorisation, which a non-interactive agent cannot give
and must not change; that remains unexercised there. The live session is verified instead in the
Linux container — a separate image, `nazm-debug`, that adds gdb to the gate's image without
changing it — on objects built for `aarch64-unknown-linux-gnu` (§7.59) and linked there: a
breakpoint by file and line and by function, a backtrace naming Nazm functions with their files
and lines through several calls, `next` and `step` by statement, and variables read while live.

**Variables.** A `--debug` build describes every parameter and every source binding of type `Int`,
`Bool` or `Str` with a `DILocalVariable` on its stack slot — a parameter with its position — and the
types as DWARF: `Int` a signed 64-bit integer, `Bool` an 8-bit boolean, `Str` a structure of its
three words (`data`, a pointer to bytes; `len`; `owner`). Temporaries are not described: they have
no name a reader wrote. Records, enums, sequences, channels and closures are not described yet.
**No stale value is shown as live**: a managed local's slot is zeroed when it is dropped and before
its storage is released, so after its last use a `Str` reads as `{data = 0x0, len = 0}`, never as
the bytes of freed memory; and a slot is zeroed at entry, so a binding read before its statement
reads `0`. Both are the slot's real contents, not a claim about the variable's value at that point:
scope ranges (`DILexicalBlock`) are not emitted, so every described variable is visible for its
whole function.

**Amended during implementation.** Two facts the live session taught. *Names*: with the native
symbol as the subprogram's `linkageName`, gdb named every frame and resolved every breakpoint by
the symbol (`nz.m.v_2Enz.greet`) and not by `greet`; the subprogram now carries the Nazm name alone,
and the symbol stays in the object's symbol table. *The prologue*: LLVM marks the end of a
function's prologue at the entry block's first instruction with a line, and when none has one it
uses the function's line at its first instruction — so a breakpoint on a function stopped before
its parameters were stored, and gdb showed `times=0`. The prologue now carries no line and the
entry block's branch takes the first statement's position, so the breakpoint lands after every
slot is written.

**Optimised builds.** At `-O2` LLVM may delete or move a slot; a variable it removed reads as
"optimized out" in the debugger, which is the debugger's word for it and the honest answer. Nothing
is claimed about values at `-O2`.

**Cranelift.** `--debug` with Cranelift stays refused by name: Cranelift 0.136 exposes no DWARF
emission for an object module through the API this backend uses, so there is no line table to
write. A blocker with its reason, not a silent difference.

**Scheduler and runtime counters in the profiler.** `nazm profile FILE` builds the program,
runs it once with `NAZM_MEMORY_REPORT` and `NAZM_SCHED_REPORT` set, and prints `nazm.profile/1`: the
exit status, the wall time, and the memory and scheduler counts parsed from those lines — counts of
events, not samples. The compiler's phase timings stay `nazm.timings/1`; the two are separate
documents and are never added together.

**Not here.** Sampling profilers, timelines, an event trace, closure environments and generic
instances by name in the debugger, foreign frames' variables, DWARF from Cranelift, and the Mac's
live session.

## 7.61 Backend performance and what the native build still needs — N59, 2026-10-02

Written before the code. N40 measured the cost of lowering through MIR: at `-O0` the LLVM emitter
gives every MIR local a stack slot, and clang at `-O0` promotes none of them, so the compiler
written in Nazm emitted 13,836 `alloca`s where N39's tree walk emitted 3,275, and clang's compile
grew 17 %. N59 removes the part of that cost that needs no new analysis, and states what the native
build still depends on.

**Scalar temporaries become SSA values.** MIR keeps its slot-per-local model — it is the authority
for ownership and control flow, and nothing here changes it. The LLVM emitter, deciding how to
represent a local, gives no slot to a local that MIR lowering made (`Temp`), of type `Int` or
`Bool`, assigned exactly once, and read only later in the block that assigns it: such a value is
already in a register, the store and load around it are pure overhead, and SSA's dominance rule is
satisfied by construction — a use in the same block after the definition. Everything else keeps its
slot: parameters, source bindings (a debugger reads them, §7.60), managed values (a failure edge's
cleanup reads and zeroes them), records and enums, and any local read in another block, including a
cleanup block. The rule is decided per function from MIR alone, so the emitted text is a
deterministic function of MIR, and its digest of what was emitted (the object key) is unchanged in
kind.

**Measured, not assumed.** The gain is counted — slots and instructions in the compiler's own
text — and timed as an interleaved A/B on one host, with the variance stated; it is claimed for the
workloads measured and nothing else. Cranelift needed no counterpart: it already keeps every scalar
local in an SSA variable (§7.43) and optimises at `-O1` and above; its remaining gap on sequence
loops is its register allocator and lack of loop-invariant motion over memory, which N59 measures
and does not claim to close.

**What the native build still needs.** The runtime is LLVM text (§7.55), so both backends still run
clang to compile it, and both run the system C compiler driver to link. N59 does not remove either:
a runtime precompiled per target, or a linker driven directly, is the next step, and the build
reports both dependencies by name (`nazm inspect`'s toolchain) rather than hiding them.

**Not here.** Promoting locals across blocks (phi construction), promoting managed values, an
optimisation pipeline of Nazm's own, a precompiled runtime, a built-in linker.

## 7.62 Restriction profiles v2: stack, determinism, unknowns — N60, 2026-10-02

Written before the code. §7.49's profiles refuse effects, the ambient bridge and unlocked builds;
N52 and N54 added allocation, flow, declassification and time. N60 adds the rules those facts
make possible next, and gives a verdict a third value.

**A verdict is `pass`, `fail` or `unknown`.** A rule whose facts were not established — MIR did not
lower, or a call target cannot be known — is `unknown`, and a profile treats `unknown` as a refusal
(`N0510`, with the reason): a restriction that cannot be checked is not met. The report carries the
three values, so a reader sees which refusals are proofs of a violation and which are absences of a
proof. `general` has no rules and is unchanged.

**Stack: `no-recursion` (embedded).** A program whose call graph has a cycle has no stack bound the
compiler can state. The facts are MIR's: a function in a call cycle is exactly one MIR gives a
`StackCheck` (N49), and the witness names it. A call through a function value (`CallIndirect`) has a
target the compiler does not know, so where one exists the rule is `unknown` rather than `pass`. A
frame-size estimate per function is not computed here: the bound on depth is the claim, not bytes.

**Determinism: `no-select` (critical).** `critical` already refuses spawning a task, reading a clock and
declassifying; a select's result depends on which channels are ready when it looks (§7.56), so it is
refused too, by its call site. What is left inside `critical` — no tasks, no clock, no select — is a
program whose observable behaviour is a function of its inputs, its files and its arguments; those
stay allowed under `io`, and refusing them is `embedded`'s `no-io`. Random numbers and the network
do not exist in the language.

**Composition.** A profile only adds rules, and every package's required profile is in force for any
program it is part of (§7.49): a dependency cannot weaken what the root requires, because nothing a
dependency declares removes a rule. This was true since N47 and is now tested from the root's side.

**Not here, and why.** *Constant-time* rules — no branch or index depending on a secret — need a
secret-origin lattice that N52's origins (file, argument, unknown) do not express; a research item,
not a partial claim. *Bounded loops, queues and tasks* need a declared, checked bound (§7.63's
subject, N63). *Byte bounds* on allocation stay N52's `at-most-once`/`unknown` counts. Nothing here is
a certification.

## 7.63 A formal core and bounded model checking — N61, 2026-10-02

Written before the code. Nazm's meaning is its specification and its two implementations' agreement;
nothing about it was machine-checked against a definition independent of both. N61 states a small
core formally — `docs/formal-core.md`: syntax, typing rules, big-step evaluation rules with traps —
and checks five properties of it, mechanically, over every program of the core up to a size.

**Why bounded model checking and not a proof assistant.** No proof assistant is part of the
toolchain, and adding one is a dependency decision the project has not made. A checker that
enumerates every program of the core up to `BOUND` nodes and checks each property on each is
mechanical, deterministic, runs in the ordinary test suite, and says exactly what it covers: every
program up to the bound, and nothing beyond it. That is weaker than a proof and stronger than a
sample, and it is stated as such.

**Correspondence.** The semantics is transcribed into `crates/nazm-formal`, one arm per rule, with no
dependency on the compiler: the compiler cannot be its own oracle. Each core program is printed as a
Nazm program, and the properties hold the real checker and the real interpreter to the formal
definition: the checker accepts exactly what the typing rules type, and the interpreter's outcome
— value or trap code — is the evaluation rules'. A change to either side that the other does not
share is found by the enumeration.

**What the critical profile may cite.** A property of the core is evidence about the core's
constructs — integer and boolean arithmetic with traps, comparison, short-circuit `&&`, `if`,
`let` — and only those. The profile report names no proof; the matrix records the core's scope.

**Not here.** Functions, recursion, loops, mutation, strings, sequences, records, enums, effects,
tasks, closures, memory, and the native backends' code generation; a proof of any property beyond
the bound.

## 7.64 A freestanding target: no OS, no libc, no heap — N62, 2026-10-02

Written before the code. Every target so far runs on an operating system through its C library.
N62 adds one that runs on neither: `aarch64-unknown-none`, a 64-bit ARM machine with no OS, verified
on QEMU's `virt` board.

**What a freestanding program is.** A Nazm program that needs nothing of the hosted runtime: no
allocation (so no strings built at run time, sequences, channels or closures), no tasks, no files,
arguments or standard streams, no C, and no call cycle — a stack bound is what a machine without
an OS needs, and the hosted stack check reads a limit from an OS that is not there. The build reads
which runtime parts a program reaches (the same `Needs` every build computes) and refuses any beyond
the freestanding set by name, before anything is linked (`N0101`). `embedded` (§7.49, §7.62) is the
profile that states the same restrictions as rules; a freestanding build applies them as facts of
what can be built.

**The freestanding runtime.** A failure is recorded in plain globals — there is one thread, and no
thread-local storage without an OS — and reported by writing its message to the board's UART and
stopping the machine with status 2. The platform entry is `_start`: it sets the stack pointer to the
top of a stack the linker reserves, calls `main`, writes its result to the UART in decimal and a
newline, and stops the machine with status 0. Stopping is ARM semihosting's `SYS_EXIT_EXTENDED`,
which QEMU honours with `-semihosting`; on a board without a debugger attached it would trap, which
is stated as the target's contract, not hidden. There are no hidden constructors: every global is a
constant the linker places, and the entry runs nothing before `main` but setting the stack.

**MMIO.** `mmio_read32(addr: Int) -> Int` and `mmio_write32(addr: Int, value: Int) -> Int` read and
write a 32-bit device register — a volatile access, never merged, removed or reordered with another
volatile access — and need an `MmioCap` held where they are written (as `TimeCap` and `VouchCap` are):
reaching a device is authority, and it performs no effect a Nazm function's contract names, so an
`embedded` program can use it. They are refused where there is no device: by `nazm run` and by any
hosted build (`N0392`). Ordinary loads and stores never become volatile; only these two are.

**Linking.** The build writes objects (§7.59) and, beside `link.txt`, the linker script `link.ld`: the
image at `0x40080000` (where QEMU's `virt` loads a kernel), `_start` first, and a 64 KiB stack above
the image. The host has no ELF linker, so the image is linked by `ld.lld` in the QEMU image and run
there; the build itself never runs it.

**Deferred, and why.** *Atomics*: one thread and no interrupts make them unobservable; their memory
orders need the model §7.46 does not yet state for shared memory. *Interrupt handlers*: an ABI, a
reentrancy rule and a stack requirement are designed here in outline — a handler is a function of no
parameters and no result, `! {}`, holding at most an `MmioCap`, run on its own stack — and not built.
*A heap*: a startup-only arena is the next policy; none is built, so a freestanding program allocates
nothing. *A standard library subset*: every `@std` module allocates, so none is available.

**As built (N62), where it differs from the above.** *Recursion builds*: the board runtime carries
its own `nz.stack_check`, against the linker script's `__stack_bottom` with a quarter of the stack
in reserve, so a call cycle is checked rather than refused and exhaustion is `N0408` on the UART.
*The refusal reads symbols, not `Needs`*: `Needs` is coarse — `print` of a static string sets no
flag yet calls the C library's `write` — so the build refuses what the generated units name and
nothing defines, by runtime part, with exit status 4 (1 since Q1-C1) rather than `N0101`. *`_start` enables
floating point and SIMD* (`CPACR_EL1`), trapped at reset, since code generation may use them.
*Cranelift is refused for the board by name*: its object writer has no format for it.

## 7.65 Real-time bounds: what is bounded, what is measured, what is unknown — N63, 2026-10-02

Written before the code. "Fast" is not "real-time": a real-time claim is an upper bound that holds
on every run, and Nazm states one only where the compiler or the code generator establishes it. N63
targets an **analysable subset** — a program on the board (§7.64) whose loops, calls, allocation,
tasks, queues and blocking the compiler can bound — not hard-real-time certification of anything.

**Three kinds of number, never mixed.** A *static bound* is established from the program or its
generated code and holds on every run of that build. A *measurement* is what one run observed; it is
evidence that a bound is not violated there, never a bound. An *unknown* is a quantity the compiler
does not establish, reported as `null` with the reason. Every report below names which each number is.

**The `realtime` profile.** `embedded`'s rules (no `io`, no `spawn`, bounded allocation, no
recursion) and three more, each an existing fact or a new one read the same way:

- `no-blocking`: no channel send, receive or select — the operations whose latency depends on
  another task or the host scheduler (§7.56). Without tasks they either complete at once or wait
  forever; either way their latency is not the program's.
- `no-ambient-time`: as `critical` (§7.56) — a deadline makes behaviour depend on when it runs.
- `bounded-loops`: every `while` has a trip bound the compiler establishes, or the rule is
  `unknown` and refused. The bounded form, read off Core IR: a counter `i`, set by `let mut i = A`
  or `i = A` (`A` an `Int` literal) as the last write to `i` before the loop in its block; the
  condition `i < B`, `i <= B`, `i > B` or `i >= B` (`B` an `Int` literal, or an immutable local bound
  to one); and the body's one write to `i`, a top-level `i = i + C` (or `- C` for `>`, `>=`), `C` a
  positive literal, with no `continue` to the loop. The bound per entry to the loop is the exact
  trip count of that arithmetic, computed in 128 bits; a `break` or `return` only ends it sooner, and
  an overflow on the last step traps, which is failure, not another turn. Anything else — a
  condition on data, a counter written twice — is `unknown`, by loop, with its span.

**The bounds report.** `nazm check --profile-report` gains `bounds` whenever a rule needing MIR or
Core IR facts is in force: each loop's bound or `null`; the longest chain of direct calls from
`main` (or `null` with a cycle or an indirect call); the allocation, task and queue *sites*, counted
statically (a count of sites, not of bytes or of executions); and `wcet: null` — no execution-time
bound is computed, and none is implied.

**The stack bound (board only).** A board build (§7.64) writes `bounds.json` (`nazm.bounds/1`) beside
`link.ld`: the code generator's frame size for every function of the image — clang's own
`prologepilog` figures, read at compile time, so the board build does not reuse cached objects — and
the deepest path through the image's call graph, read from the generated code from `nz_board_main`.
The sum along that path is a static upper bound on stack bytes for that build, stated with its path,
beside the 64 KiB the linker script reserves. A call cycle, a call through a value, or a function
without a reported frame makes it `null` with the reason. Interrupts do not exist (§7.64), so no
nesting is added.

**Measured beside it.** `--stack-watermark` (board only) paints the stack in `_start` and, after
`main`, writes `nazm-stack: used=N` to the UART: the deepest byte the run touched. A test holds
measured ≤ bound on QEMU; that is evidence about the bound, and the bound is the claim.

**Exhaustion is deterministic.** On the board a stack past the reserve fails with `N0408` and status
2 (§7.64); no allocation exists to exhaust. That is behaviour, not a bound.

**Not here, and why.** *WCET*: no cycle model of any core is built, and an emulator's timing is not
one; `null`, always. *Scheduler latency and jitter*: the hosted scheduler's wake-up is measured in
`performance.md` as a distribution, never a bound, and `realtime` refuses tasks. *Byte bounds on the
heap*: the board has no heap; a hosted bound is N52's site counts. *Resource annotations*: none are
needed for the subset; none are added. Nothing here is a certification.

**As built (N63), where it differs from the above.** *A function the generated text defines but the
code generator reports no frame for* was inlined into every caller and removed — at `-O2` most are —
so it adds nothing and the search continues through what it called, now called from its callers'
frames; only a callee outside the image makes the bound `null`. *Negated literals* count as
literals. *`embedded` reports `bounds` too*, since it computes the same MIR facts.

## 7.66 WebAssembly: one more freestanding target, and authority as imports — N64, 2026-10-02

Written before the code. N64 adds `wasm32-unknown-unknown`: a WebAssembly module, compiled by the
LLVM backend from the same MIR and LIR as every other target. Nothing in the language changes for
it — no browser semantics, no source construct of its own.

**The baseline.** WebAssembly 1.0 (MVP) with the features clang 21's `wasm32` default enables
(mutable globals, sign extension, multivalue off); `i64` is a WebAssembly `i64`, a pointer is 32
bits, memory is one linear memory the module defines and exports. The verified toolchain: the host's
clang for the objects, `wasm-ld` (LLD 19) to link, Node (V8) as the engine — all recorded in
`docker/wasm.Dockerfile`; another engine is unverified, not unsupported.

**Semantics carried, not reinterpreted.** A WebAssembly module has no operating system, so it is a
freestanding target in §7.64's sense and shares its law: what the program reaches beyond the
module's runtime is refused by part, before anything is linked. Arithmetic is checked exactly as
on every target (overflow is LLVM's intrinsic and a branch, not WebAssembly's wrapping `i64.add`),
and a failure is recorded in plain globals — the module has one thread. Recursion builds under a
stack check against the linker's `__stack_low`, so exhaustion is `N0408`, not the engine's own
trap. Static strings are bytes in the module's data. Allocation, sequences, strings built at run
time, channels, tasks and closures are refused, as on the board: a heap is designed (a bump
allocator over `memory.grow`, freeing nothing until §7.41's release is ported) and not built.

**Authority is an import.** A module imports from one module, `nazm_host`, and only what it uses:
`write(fd: i32, ptr: i32, len: i32)` exactly where the program writes to standard output or error
(which needs an `IoCap`, as everywhere), and `report(ptr: i32, len: i32)`, always — the entry
checks for a failure after `main`, so every module can report one. A program that holds no `IoCap` therefore has no `write` import at all — absent authority
is an absent import, which a test reads from the module itself. Files, arguments, the clock, the
network and randomness have no import: the built-ins that need them are refused for this target by
part, so the host is never asked. Every import is named in the module's import section, in a fixed
order, so the module's bytes say what it can ask of its host.

**The host interface.** The module exports `memory` and `nazm_main() -> i64`: `main`'s result, or —
after calling `report` with the failure's text — `0`, with the host told by `report` that it
failed. A string crosses as a pointer into `memory` and a byte length; the host copies, never keeps.
`--objects DIR` writes the objects, `link.txt` (the `wasm-ld` line) and `host.mjs`, a reference host
for Node that provides exactly the imports the module declares and refuses to instantiate a module
asking for any other. It prints `main`'s result and a newline, and exits with status 0, or 2 after a
failure's text on standard error — the board's contract, carried.

**WebAssembly is not the boundary here.** The engine isolates memory and control; what the module
can do outside is what its host's imports do. The reference host's `write` writes to its own
process's streams; nothing else is provided. A different host is a different trust boundary, and the
module's import section is the contract it is checked against.

**Identity and source mapping.** The module is a function of its objects and the link line, which
are a function of the source and the toolchain; two builds give the same bytes. `--debug` carries
DWARF in the objects' custom sections, which `wasm-ld` keeps, so a WebAssembly debugger maps code to
source as a native one does.

**Not here, and why.** *WASI*: its interfaces are a host this milestone does not choose; the
`nazm_host` imports are the contract. *A heap*: designed above. *Exported Nazm functions*
(`pub extern "C"`, N55) over the module boundary: the same ABI applies to `i64`/`i32`, and they are
not yet exported — `nazm_main` alone is. *The browser*: no DOM, no event loop. *Cranelift*: its
WebAssembly support is as a source, not a target.

**As built (N64), where it differs from the above.** *`__multi3`*: LLVM's checked `i64` multiply on
wasm32 calls compiler-rt's 128-bit multiply, and there is no compiler-rt; the runtime defines it from
64-bit pieces. *The stack bound* of §7.65 is stated for a module too, rooted at `nazm_main`; a host
import uses the host's stack, not the module's, and adds nothing. *The freestanding refusal names
each part's symbols*, on the board as here.

## 7.67 The interactive tier: a REPL, comptime and reload checks — and a JIT, blocked — N65, 2026-10-02

Written before the code is committed. Native AOT stays the foundation (Goals G02): nothing here
changes what `nazm build` produces or what a program means. The interactive tier is the
interpreter and the checker — the settled Core IR every build starts from, never a second meaning.

**The JIT is BLOCKED, by a law of this repository.** An in-process JIT writes machine code to memory
and calls it: that is `unsafe` Rust, and the workspace forbids `unsafe` code (`unsafe_code =
"forbid"`, `Cargo.toml`). Lifting the law for one audited crate is a decision about the project's
safety rules, not a milestone's implementation detail, and it is not made here. Designed for when it
is: Cranelift (already the second backend) compiles MIR to code in memory through `cranelift-jit`;
the runtime's LLVM text is compiled once by clang into a shared library keyed by the runtime's
digest and loaded beside it; the entry's call to `main` goes through a pointer the JIT sets; the
JIT's cache key is the object key Cranelift already uses (MIR, settings, triple, runtime digest), so
no code survives a semantic, runtime or version change. Until then the fast interactive tier is the
interpreter, whose agreement with native code the existing differential suites hold.

**The REPL.** `nazm repl` reads definitions and expressions a line at a time. The session is one
program — its definitions in the order first given — checked whole on every edit, so a definition's
identity is its name in the session's module (§7.10) and a redefinition is checked against every
definition that uses it. One that breaks another is refused by the other's name and leaves the
session as it was: no definition is ever checked against a predecessor that has gone. An expression
is the body of a `main` holding an `IoCap`, evaluated by the interpreter as an `Int`, a `Bool` or a
`Str`; `main` itself is the REPL's. Nothing is cached between lines, so nothing can be stale.

**Comptime.** `nazm comptime FILE FUNCTION` evaluates a function of no parameters that is pure —
declared `! {}` or inferred so — under the interpreter's iteration and depth budgets. That is the
whole of its isolation — not an operating-system sandbox — and it is the language's own: authority is a value (§7.39), so a function of no
parameters holds none, and a pure one cannot exercise its caller's through the ambient bridge, so
files, the clock, the network, tasks and C are unreachable without any check of their own. Anything
else is refused, by name, with `N0393`; a runaway evaluation stops at its budget with `N0402`.
In-language comptime (a constant evaluated during checking) is designed on this rule and not built.

**Reload checks.** `nazm reload-check OLD NEW` compares two versions of a program's root module, per
definition: a function whose body changed and whose signature — parameter and result types, and
effects — did not is `reloadable`, as is anything added; a changed signature, a removed function, or
a record or enum whose layout changed (its fields by name and type; order is not layout, §7.9) needs
a `restart`, named with what changed. One `nazm.reload-check/1` line, with the runtime ABI revision;
a non-zero exit when anything needs a restart. Nothing is reloaded: the check is the rule a hot
reloader would be held to, stated and tested before any reloader exists.

## 7.68 Vectorisation under checked arithmetic — N66, 2026-10-02

Written before the code. Nazm's `Int` arithmetic is checked: every `+` is LLVM's overflow
intrinsic and a branch (§7.43), so a loop over a sequence carries a failure edge per element, and no
code generator vectorises it. N66 makes one idiom vectorise without changing what any program means,
and says for every other loop why it did not.

**No vector types in the language.** A source-level vector would be a new kind of value with its own
overflow, bounds and layout rules, and nothing needs one yet. The semantic addition is one built-in,
`ints_sum_from(start: Int, v: Ints) -> Int`: `start` plus every element of `v`, left to right, failing
with `N0400` exactly when one of those running sums would overflow — what the loop computing it
means, as a function. It is also the manual escape: a program may call it.

**Legality, exactly.** A vectorised sum reorders additions, and a reordered checked sum can succeed
where the ordered one fails (`[MAX, 1, -1]`). The runtime keeps the ordered meaning by bounding, not
reordering, the checks: a block of 64 elements whose magnitudes are all at most 2^56, added to a
running sum whose magnitude is below 2^62, has every partial sum inside `Int` — 64 · 2^56 = 2^62 —
so the block's total is exactly what the 64 checked additions give, and it is added without checks,
in a loop the code generator vectorises. Any other block is added one checked element at a time,
and the first overflow is the failure. A tail shorter than a block is checked. Bounds need no check:
the sum reads `0..len`.

**Automatic, for one idiom, in native builds.** The counted loop `while i < ints_len(v) { s = s +
ints_get(v, i); i = i + 1; }`, with `i` started at `0` in the loop's block and nothing else in the
body, is replaced in Core IR before MIR by `s = ints_sum_from(s, v); i = ints_len(v);`, the call at the
addition's span — so an overflow is the same diagnostic at the same place, and the loop's bounds
checks could never fail. The interpreter keeps the loop, so its iteration budget still counts each
turn. Every other loop is left alone, and `nazm explain-cost` names the reason for each.

**Targets and identity.** The vectorised code is what clang produces for each target's baseline —
NEON on `aarch64`, SSE2 on `x86_64` — at `--opt-level 2` or above; nothing asks for a host's own CPU
features, so no object depends on the machine that built it, and the object key's triple and
optimisation level already separate what differs. At `-O0` the same code runs unvectorised, with
the same results. Choosing wider features (AVX2, SVE) per build is designed — a target feature list
in the object key and in `link.txt` — and not built.

**Not here, and why.** Element-wise maps (`out[i] = a[i] + b[i]`) need the store's failure to stop the
stores after it, which the same block bound gives, but the idiom's recogniser is not written. Floating
point does not exist. GPU offload is N67.

## 7.69 Accelerators: kernels, transfers and one GPU — N67, 2026-10-02

Written before the code. No ordinary Nazm function becomes GPU code by itself. A **kernel** is a
function that meets a stated eligibility, run over a sequence by an explicit command, with every
movement of data and every synchronisation named in its report.

**Eligibility, statically.** A kernel is pure, maps one `Int` to an `Int`, and reaches only
functions that are themselves pure functions of `Int`s and `Bool`s, without recursion (a work item
has no stack to recurse on), built-ins (none has a device form), records, enums, closures, tasks or
C. Anything else is refused before a device is touched, by name and with the span that makes it
ineligible (`N0394`).

**Meaning, carried.** Arithmetic is checked on the device exactly as on the host: addition and
subtraction by the sign rule on wrapping two's-complement results, multiplication by comparing the
high word with the low word's sign, division and remainder by zero and `i64::MIN / -1`, and
`i64::MIN % -1` is 0 (the spec's, where C's is undefined). A map is a function of each element alone,
so its sequential meaning — results in order, or the first element's failure — is recovered from a
parallel run by recording each failure as `index · 256 + site` with `atomic_min`: the lowest index
wins, and its site names the operation. Sites and indices are bounded by that one 32-bit word,
which caps a launch at 8,000,000 elements.

**Memory and transfers.** The input is copied host to device once, the results device to host
once, and a failure word each way; the device never sees host memory, and nothing persists between
runs. One synchronisation — every work item finished — precedes every read. The report states the
bytes and the time of each transfer, the kernel's build, and the run.

**The first backend: OpenCL on Apple's GPU.** The tooling that exists here is macOS's OpenCL 1.2
on the M1 Pro's GPU; the kernel is OpenCL C, generated from Core IR. OpenCL is deprecated on macOS
and absent elsewhere, so this backend is a first, scoped one: SPIR-V (Vulkan) and Metal are its
portable and native successors, designed, not built. The host side is a C program compiled against
the system framework and run as a process: the compiler links no GPU API and keeps its `unsafe_code
= "forbid"` law.

**Identity.** A kernel's identity is its OpenCL C source, the device's name and driver version, and
the host program — hashed together (`nazm.kernel/1`); the driver compiles the source on every run,
and the identity is what a cache of its binaries would be keyed on.

**The reference, and no silent fallback.** The command holds the device's results to the
interpreter's, element by element, or the same failure at the same index, and says which. A host
without the device refuses; nothing runs elsewhere instead.

**Not here, and why.** *Layout specialisation* (coalescing, structure-of-arrays) needs a value
richer than `Int`; *kernels in the language* (a `kernel` modifier, launched from Nazm) need a device
handle as a capability, designed on `MmioCap`'s pattern and not built; *occupancy* is not reported by
the API version here.

## 7.70 Layout: a `Vec` of records, one array per field — N68, 2026-10-02

Written before the code is committed (the code was prototyped first, for N66's lesson that a
measurement must precede a claim; this text, and the register's prior art, precede the commit).
A layout choice is sound only where no program can observe it. In Nazm a `Vec` element has no
address, cannot cross the FFI (`Int` and `Bool` only, N55), is never serialised, and a record's
layout is already by field name (§7.9) — so for an element that owns nothing, the layout of a
`Vec`'s storage is unobservable by construction.

**Eligibility.** A `Vec[R]` where `R` is a record with at least one field and every field an `Int`
or a `Bool`. An element that owns storage (a `Str`, a sequence, a `Vec`, a closure) is excluded,
not because it is observable but because its release walks elements, and splitting that walk is
not done here; an enum or a non-record element is excluded likewise. Each excluded type says why.

**The layout.** `--layout soa` stores an eligible `Vec[R]` as one 8-byte array per field in one
allocation, field `k` of element `i` at `data + (k · cap + i) · 8`; `nz.soa_grow` relays the arrays
out when the vector grows. Get gathers, set scatters, push and pop do both; bounds and failures are
exactly the element-by-element layout's, checked before any field is touched. The header — `len`,
`cap`, `data`, the count — is unchanged, so every sequence operation that does not touch elements
(length, release, sharing) is the same code.

**Choice and identity.** Opt-in, by name, LLVM backend only (Cranelift refuses it), and applied to
every eligible type: the access pattern decides whether it wins, and the compiler has no cost model
for that yet, so it does not choose. The layout changes the emitted text of every unit that touches
such a `Vec`, and an object is keyed by its text, so the layout is in every affected object's
identity and in the emitter's; it is not in the semantic identity, because no program's meaning
changes. `nazm explain-cost` reports, per `Vec` type, whether `--layout soa` lays it out field by
field or why not.

**Not here, and why.** Automatic selection (a cost model over access patterns); elements that own
storage; enums; hybrid (AoSoA) blocking; layouts for a single record value, which LLVM already
scalarises.

## 7.71 Contracts: a chain-neutral model over ordinary Nazm — N69, 2026-10-02

Written before the code. A smart contract is an ordinary Nazm module held to a profile and run by a
simulator that owns its persistent state. No Web3 dialect: the same pure function means the same
under every profile, and nothing below is syntax.

**The `web3` profile.** `declared-effects`, `no-io`, `no-spawn`, `no-foreign`, `no-ambient-time`,
`no-blocking` and `no-recursion` (§7.49, §7.62, §7.65): no ambient authority, no clock, no tasks or
channels, no C, every contract declared, and a call graph whose depth the compiler can state.
Arithmetic is already deterministic and checked everywhere; nothing changes for it.

**A contract.** A module with a record `State` whose fields are `Int`s and `Bool`s, a `fn init() ->
State`, and entrypoints: every `pub fn` of the form `fn name(s: State, caller: Int, a: Int, …) ->
Result[State, Int]`, pure (`! {}`). Persistent state is therefore not a global: it is the first
argument and the `Ok` result, and an entrypoint's authority over it is exactly what its body does with
`s`. `caller` is the signer, handed by the host. `Err(code)` reverts with `code`; a runtime failure
(`N0400`, `N0401`, `N0405`) reverts with its diagnostic code; a revert leaves the state as it was. An
optional `fn conserved(s: State) -> Int` names the quantity every successful transaction must leave
unchanged — the contract's asset.

**Machine-readable facts.** `nazm contract FILE --interface` writes `nazm.contract/1`: the state's
fields, each entrypoint's arguments, and its **read and write sets** — a field it projects from `s`
is read; a field of an `Ok(State(…))` it builds other than as `s.field` itself is written; anything it
cannot see through (a helper given `s`, a state built elsewhere) makes the whole state "may read" and
"may write", never omitted. External calls do not exist in this model: an entrypoint calls only Nazm
functions, so there is no reentrancy to analyse, and that is stated rather than claimed as safety.

**The simulator.** `nazm contract FILE --txs FILE` runs transactions in order — `CALLER ENTRY ARGS…`
per line — on the interpreter, threading the state, each from the state the last successful one left.
`nazm.contract-run/1` reports, per transaction: the outcome (`ok`, `revert` with the code, or `fail`
with the diagnostic code), the **dynamic meter** (the interpreter's steps, a count, not gas), the
fields it changed, and — where `conserved` exists — whether the conserved quantity held. The run is a
pure function of the module and the transactions: two runs give the same bytes.

**Assets, designed and checked dynamically.** A linear asset kind — a value that cannot be copied,
only moved, and cannot be dropped except by an explicit destruction — is a type-system change this
milestone does not make. In the implemented scope an asset is the declared conserved quantity, and the
simulator refuses every transaction that changes it (`N0395`); duplication or silent loss of an asset
is a conservation failure, found at the first transaction that commits it. Static linearity is
designed: a built-in `Asset[T]` with move-only semantics, its own drop rule, and the checker's existing
ownership facts (§7.41) — RESEARCH until a backend needs it.

**Not here, and why.** Chain host calls (storage, events, crypto) belong to each backend's adapter
(N70–N72); metering in gas is the EVM's (N70); signer and account constraints beyond `caller` are
N72's.

## 7.72 The EVM backend: the contract model as bytecode — N70, 2026-10-02

Written before the code. The contract model (§7.71) compiled to EVM bytecode, run in a local EVM,
and held to the simulator. Nothing of the EVM reaches the language: words, selectors, storage slots
and gas are the backend's.

**Route: direct, from Core IR.** No Yul and no `solc`: neither is in this toolchain, and a contract's
body is the small, already-checked Core IR of §7.71 — `Int`/`Bool` arithmetic, `if`, `let`,
`while`, calls to pure helpers (which `web3` makes acyclic, so the backend inlines them), and the
entrypoint's `Result[State, Int]`. The backend emits opcodes with labels and resolves them. What it
does not support — the state used other than through `s.field` and as the returned `Ok(value: s)`
or `Ok(value: State(…))`, a record or enum other than those, any built-in — is refused before
emission, by name.

**Integers.** An `Int` is a 256-bit word holding a sign-extended `i64`. Every arithmetic result is
held to `i64` exactly as the language holds it: addition, subtraction, multiplication and negation
are exact in 256 bits for `i64` operands, then checked with `SIGNEXTEND(7, r) == r`; division and
remainder are `SDIV`/`SMOD` — truncating, the remainder taking the dividend's sign, as `docs/spec.md`
says — after the zero and `i64::MIN / -1` checks. A failure reverts with the five bytes of its
diagnostic code (`N0400`); an `Err(code)` reverts with the 32-byte word `code`; the two are told apart
by length. Every revert discards the transaction's writes: the EVM's own rule, and the simulator's.

**The ABI.** Each entrypoint is a function `name(int64,…)` — its arguments after `caller`, which is the
EVM's `CALLER`, not an argument anyone can choose. The selector is the first four bytes of
`keccak256` of that signature, computed by the compiler (its own keccak, tested on known vectors). An
argument whose 32-byte word is not a sign-extended `i64` reverts before the body runs.

**Storage.** Field `f` of `State` lives at slot `keccak256("nazm.state." + f)`: by name, so reordering
fields moves nothing (§7.9's rule, kept), and adding one never moves another. `storage.json`
(`nazm.evm-storage/1`) records each field's name, type and slot; `nazm contract --upgrade-check OLD NEW`
refuses a change that would misread live storage — a field removed, or its type changed — and allows
one that adds a field. `init()`'s state is written by the deployment's constructor.

**Gas.** For an entrypoint without a loop, the backend states a static upper bound on its execution
gas — the costliest path's opcodes, a cold `SLOAD` per field read, the worst `SSTORE` per field written,
and its memory — beside the intrinsic cost; a loop makes it `null`. The EVM's measured gas is reported
beside it, and a test holds measured ≤ bound.

**Held to the simulator.** A test harness (`docker/evm.Dockerfile`, py-evm) deploys the bytecode, runs the
same transactions from the same senders, and the test compares each outcome, revert code and the final
storage with `nazm contract --txs`.

**Not here.** Events, value transfer and external calls do not exist in the model (§7.71); an
optimiser; Yul output for audit.

## 7.73 Contracts on WebAssembly: the ordinary backend and a host adapter — N71, 2026-10-02

Written before the code. A contract (§7.71) compiled by the ordinary WebAssembly backend (§7.66), with
nothing chain-specific in the compiler but one generated adapter: the contract's own unit, its code
unchanged, gains exported entry functions that move the state across typed host imports.

**The adapter.** For each entrypoint `name`, an export `nazm_call_name(caller: i64, a: i64, …) -> i64`
that reads every state field with `nazm_host.state_get(index) -> i64`, builds the `State`, calls the
entrypoint, and on `Ok` — after comparing `conserved` before and after, where declared — writes every
field back with `nazm_host.state_set(index, value)` and returns `0`; on `Err(code)` it calls
`nazm_host.revert(code)` and returns `1`; a conservation change returns `2` with nothing written; a
runtime failure is the module's ordinary `report` (§7.66) and returns `3`. `nazm_init()` writes
`init()`'s state. A field's `index` is its declaration index, and its value an `i64` (`Bool` as 0 or 1):
the serialisation is the state's declared shape, deterministic and versioned in the metadata.

**The host.** The module imports exactly `nazm_host.state_get`, `state_set`, `revert` and `report` —
read from its import section, and nothing else is granted. The reference host (`contract_host.mjs`)
keeps the state, instantiates the module afresh for each transaction (a failure leaves nothing behind),
and applies the result only on `0`. Its run is compared with the simulator's: the same outcomes, codes
and final state.

**Shared, not forked.** The adapter is text appended to a unit the ordinary backend emits; the backend
itself is unchanged, and the same contract built for the ordinary `wasm32-unknown-unknown` target without
the adapter is the same unit without it — which a test holds.

**Not here.** A chain's own host interface (storage keys, events, crypto) is a binding of these four
imports to it; none is bound.

## 7.74 Account-oriented contracts: signer constraints and metadata; execution blocked — N72, 2026-10-02

Written before the code is committed. An account-oriented chain hands a program its accounts —
state it may read, state it may write, keys that signed — and refuses a transaction whose accounts do
not meet the program's declared constraints. The chain-neutral model (§7.71) already knows most of
what those constraints must be; this milestone states them and checks the one the model can decide.

**Signed writes, statically.** The `accounts` profile is `web3` with one more rule, `signed-writes`:
an entrypoint whose write set is not empty must decide on its `caller` — `caller` must appear in a
condition of its own body — or it is refused (`N0510`), naming the entrypoint. A helper handed
`caller` is not looked into, so a check made only there does not count: the rule refuses rather than
trusts what it cannot see. A read-only entrypoint needs no signer. This is a missing-signer check in the
target-neutral model; it is not proof that the decision is the right one.

**Account metadata.** `nazm contract FILE --accounts DIR` writes `accounts.json`
(`nazm.sbpf-accounts/1`): the state account, owned by the program, its data laid out as a version byte
and then each field's 8 bytes by declaration index; and for each instruction its discriminator (the first
8 bytes of `keccak256("nazm.instruction." + name)`), its arguments' offsets, and its accounts — the state,
writable exactly when its write set is not empty, and the caller, a signer exactly when the
entrypoint decides on it. Cross-program calls do not exist in the model and are reported as none.

**Execution is BLOCKED.** There is no sBPF toolchain, validator or emulator on this host. Upstream LLVM's
`bpfel` target exists, but Solana's sBPF differs from it in its program ABI, syscalls and verifier, so an
upstream eBPF object would not be evidence of an sBPF program. The backend — the account input
deserialised into the `State`, the entrypoint called, the data written back, the verifier's constraints —
is designed by this metadata and not built.

## 7.75 Trust evidence: fuzzing, provenance and reproducibility — N73, 2026-10-02

Written before the code. Evidence that a build is what it says, and that the compiler survives
inputs nobody wrote by hand — never a proof, and labelled as what it is.

**Two fuzzers, deterministic.** *Front-end robustness*: every source in the repository's corpora,
mutated by a seeded generator (bytes deleted, duplicated, swapped, replaced with tokens of the
language), is parsed and checked in process; the property is that nothing panics — a diagnostic is
the right answer to any input. *Semantic differential*: a generator of well-typed programs (`Int` and
`Bool` arithmetic and comparison, `let`, `if`, bounded `while`, calls between generated functions,
records) — every one terminating by construction — run under the interpreter, the LLVM backend at
`-O0` and `-O2` and Cranelift; the property is one answer (value, or failure code and place). Both are
seeded: a seed names a run, and the same seed is the same inputs on every host. A failing input is
shrunk — statements and subexpressions removed while it still fails — and the shrunk input is printed to
be added to the regression corpus, which every run replays first. Resource bounds are the test
harness's and, contained, the gate's.

**Shown to find a fault.** A backend fault injected on purpose — a signed comparison emitted unsigned —
is found by the differential fuzzer without any test written for it; it is catalogued as a mutant whose
only killer is the fuzzer.

**Provenance.** `nazm build --provenance FILE` writes `nazm.provenance/1`: every source module with its
digest, every package with its version and digest, the target, the compiler's version, the C
compiler's identity, the runtime's ABI revision and digest, the backend and its options, the profiles in
force, and the output's digest. `--attest-with CMD` runs CMD with the provenance file's path and records
its standard output as the attestation — a hook for any local signer; nothing online is needed or used.

**Reproducibility, scoped.** Two builds of one program in two clean directories with one toolchain are
the same bytes (N46). Across toolchains — Apple clang 21 on macOS and clang 19 in the Linux image, for
the same target triple — objects are expected to differ, and are measured and classified rather than
assumed.

**An evidence bundle and the compiler's own bill of materials** (amended before their code, the same
day). `cargo xtask evidence DIR` adds two files to a directory of evidence — provenance records, gate
logs, mutation journals, measurements, artefacts. `sbom.json` (`nazm.sbom/1`): every crate the compiler
is built from, read from `Cargo.lock` — name, version, source and checksum — and the toolchain pin;
offline, so a crate the lockfile does not name is not in it. `index.json` (`nazm.evidence/1`): every
other file of DIR by relative path, with its size, BLAKE3 digest and a kind read from its name
(`provenance`, `sbom`, `tests`, `mutation`, `measurement`, `artefact`), sorted, with no time in it.
`cargo xtask evidence --verify DIR` recomputes each and refuses a changed, missing or unlisted file by
name. Who vouches for a bundle is an attestation over `index.json` (the `--attest-with` hook's
convention), never a field a bundle writes about itself.

**Not here, and why.** Sanitizers over generated code, Miri and Loom: the compiler has no `unsafe`
code (`unsafe_code = "forbid"`), so Miri's subject is absent; sanitizing the C-compiled runtime is a
separate experiment, not run. Coverage-guided fuzzing (`cargo fuzz`) needs a nightly toolchain and a
new dependency; the seeded generators stand in for it, and are said to.

## 7.76 Ecosystem tooling: init, API docs, C headers, an editor client — N74, 2026-10-02

Written before the code. Tooling over what the compiler already knows; no language rule moves, so the
semantic epoch, the interface schema and the runtime ABI stay.

**`nazm init DIR --template T`** writes a project a fresh user can run at once: `nazm.toml`, `src/`
and a `tests/` corpus `nazm test` replays. Six templates, each built only from what the toolchain
supports and each checked by the template's own commands in the tests: `cli` (a program with
`IoCap`), `library` (a package with no `main`, `pub` functions), `server` (a worker pool over
channels serving the requests in a file named on its command line — Nazm has neither sockets nor a
way to read standard input, and the template says so),
`embedded` (`aarch64-unknown-none`, `MmioCap`, built to objects), `wasm`
(`wasm32-unknown-unknown`, built to objects) and `contract` (the `web3` profile, run by
`nazm contract --txs`). An existing non-empty directory is refused; nothing is overwritten.

**`nazm doc FILE|DIR [-o OUT]`** documents a program's public API from its semantic definitions,
not from its text: each module's exported functions, records and enums, with the checker's declared
and required effects, the capabilities a function takes, its foreign symbol or C export, the
package's profiles, and a link back to the source — the path relative to the package and the line.
The only text read is the signature as written and the comment lines directly above a definition,
shown as its description (a convention of the tool; the language has no doc comments). One
`nazm.api-doc/1` JSON document, and with `-o` that and `api.md`, whose anchors are the definitions'
identities, so a link survives a rebuild and moves only when the definition does.

**`nazm bindgen HEADER`** turns the supported subset of a C header into `extern "C"` declarations: a
prototype whose parameters are `int64_t` or `bool` (or `const char *`, as N55's borrowed `Str`) and
whose result is `int64_t` or `bool`. Anything else — `int`, `long`, `void`, a pointer to anything
else, a struct, a variadic, a function pointer, a macro — is refused by name with the reason, never
widened or guessed; the output names every prototype as translated or refused
(`nazm.bindgen/1`), and the declarations it writes are checked by the checker before they are
printed. Wrappers, not emulation: nothing of C's semantics is reproduced in Nazm.

**`nazm publish --dry-run`** resolves, checks and digests exactly as a publish would, and writes
nothing to the registry.

**A library is checked as modules** (amended before its code: the `library` template found that a
package with no `main` could be tested and locked but not checked). `nazm check DIR` on such a package
checks every module of its source root as a module — the root need not have `main`, everything it
imports is checked with it — and reports each diagnostic once. `run` and `build` still refuse it
with `N0509`: there is nothing to run.

**An editor client, standard protocols only.** `editors/vscode` is a client for `nazm lsp` (the
Language Server Protocol) and a launch configuration for N58's debugger through an existing LLDB
debug adapter; all language logic stays in the compiler's service. The language service is already
package-aware — definition, references, hover and workspace symbols reach a dependency's sources, and
renaming a dependency's public function is refused (`Exported`: its other importers are programs no
open document loads) — and is tested so.

**Not here, and why.** A registry over the network, `nazm update` (versions are exact and the lock
is rewritten by `nazm lock`), translating C semantics or another language into Nazm, rename across a
package boundary, and the agent benchmark: no claim of improved agent efficiency is made, so it is
not re-run.

## 7.77 Syntax completion: incremental reparse, finer recovery, `\u{…}` — N76, 2026-10-03

Written before the code. Area 1 stayed PARTIAL for four named gaps: whole-file parsing only, recovery
coarser than a statement outside blocks, no editor path that edits a parse, and an undecided Unicode
escape. This section settles each one.

**One owner.** `nazm-syntax` owns lexing and parsing, and stays the only place either happens.
Incremental reparse is not a second parser. It is the same lexer and the same parser, restarted at a
boundary where their state is provably the same as it was in the previous revision. Everything the
reparse does not touch is copied from the previous revision and moved by the edit's length delta.

**The revision model.** A `Revision` is the result of parsing one file's text. It holds the text,
the lossless lexemes, the abstract items, the concrete tree, the diagnostics, and a *chunk table*:
one row per top-level item, recording the bytes and tokens the item owns, how far its parse looked
ahead, the parser's cascade state on entering it, and the diagnostics it raised. `Revision::edit`
takes a `TextEdit` (a byte range of the old text and its replacement) and returns the next
revision. The contract, which is the falsifier: **for every text and every edit, `edit` returns a
revision equal to `Revision::new` of the edited text.** That means the same lexemes, the same
concrete tree (kinds, bytes and spans of every node and leaf), the same abstract tree (spans
included), and the same diagnostics in the same order. Nothing is "close enough". A sequence of
edits converges because each step does.

**Where the parse may restart.** The lexer is context-free from any lexeme boundary, and it reads at
most one byte past a lexeme's end. So relexing begins at the first lexeme of the first affected
chunk. It ends at the first boundary at or past the edit's end that is also a boundary of the old
scan, shifted. From that point the old lexemes are exactly what a fresh scan would produce. The
parser's only state across top-level items is the cascade mute (§7.17), and an item keyword resets
it. Its only forward reach past an item is the delimiter-balance scan (§7.17), which is recorded per
chunk as the furthest token the chunk examined. So a chunk is reused if and only if:

- every token it examined lies wholly before the first changed token, or wholly after the relexed
  region;
- and it starts at the same token, shifted, with the same entry mute state.

Every other chunk is reparsed. The reparse stops at the first old chunk boundary past the edit
that it lands on exactly. If the edit unbalances a delimiter so that no later boundary matches, the
reparse runs to the end of the file. That is correct, and only slower.

**Green-node reuse.** A reused chunk's root-level green children are the previous tree's, by
reference. The new root is assembled from them and from the reparsed region's children. The token
text resolver becomes an append-only table that the next revision clones and extends, so an old
leaf's key still resolves.

**What a reuse may not do.** Reuse a chunk whose lookahead crossed the edit. Reuse after a lexing
boundary mismatch. Carry a span unshifted. Reuse a completion-probe parse: a hole parse (§7.28) is
always full. Fall back silently: every revision reports `ParseStats` (lexemes relexed, tokens
reparsed, chunks reused and reparsed, and whether the revision was full), so a test can see which
path ran.

**Recovery, finer.** Inside a declaration, a malformed member no longer costs the item:

- a record field or enum variant (or payload field) recovers at its `,` or at the list's closing
  delimiter;
- a parameter recovers at its `,` or at `)`;
- a call's argument, a construction's field initialiser or a `match` arm recovers at its `,` or at
  its closing delimiter.

The malformed member is an `Error` node in the concrete tree. Its siblings stay typed nodes, and a
later independent mistake in the same item is its own diagnostic. A broken `use` recovers at its
`;`. Item recovery now also stops at `use`, so one bad import no longer swallows the next.

**The abstract tree's contract is unchanged, deliberately.** An item whose header or member list
needed recovery is absent from the abstract tree. A function whose body needed recovery keeps its
signature and has no body. The checker never sees a declaration with a hole in it, so recovery adds
syntax diagnostics and never semantic cascades. The usable abstract tree around a malformed region
is every other item, and the signature of the item it is in. A hole-tolerant checker is not
attempted here.

**Unicode escapes, decided.** `Str` is bytes, and a literal's bytes are the UTF-8 encoding of its
source text: that decision was made when `"é"` was specified as two bytes. `\u{H…}` (one to six
hexadecimal digits, a Unicode scalar value) is therefore those same bytes, spelled in ASCII. It
denotes nothing a literal could not already contain. It exists so that an invisible or
bidirectional character can be written visibly. A surrogate (`D800`–`DFFF`), a value above
`10FFFF`, no digits, more than six digits, a non-hex digit or a missing brace is refused (`N0004`)
at the escape. `\xHH` still stops at `7F`. Above that, a lone byte is what `str_from_byte` makes,
and a `\x80` meaning one byte would contradict `\u{80}` meaning two.

**Unsaved buffers.** `nazm lsp` moves from full to incremental document sync. The service keeps one
`Revision` per open document and applies each ranged change to it. Analysis takes the root file's
parse from that revision instead of parsing the text again (`nazm_core::analyse_parsed`). Because
of the contract above, the two are interchangeable, and a test holds them equal after every change.
Imported files are still parsed in full on every analysis.

**Versions.** Semantic epoch `20 → 21`: `\u{…}` moved from refused to accepted. Interface schema,
runtime ABI, Core/MIR/LIR print schemas and package, lock and registry schemas: unchanged. No
semantic fact, lowering or layout moves; only which source texts parse. Machine schemas: unchanged.
The new diagnostics are existing codes at new positions. Check cache: an unparsable module is never
written (§7.4), so recovery cannot reach a cache. The epoch bump invalidates every check cached under
20. Selfhost: unchanged. The compiler written in Nazm still refuses every escape, still has no
concrete tree, and still does not recover. Parity is N98's.

**Negative example.** `fn f() -> Int { 1 }` + edit inserting `{` after the body opens a delimiter
that no later boundary closes. The reparse runs to the end of the file, and the result equals a
fresh parse. **Boundary example.** An edit at the exact first byte of an item touches the token
that ends at that byte, so the previous chunk is reparsed too (`"ab"` + `"c"` at the boundary of an
identifier makes one token, not two).

**What would prove this unsuitable.** Any edit sequence over any text for which `edit` differs from
`Revision::new`, checked over every `.nz` file in the repository with seeded random edit sequences.
Also: a common one-item edit on the compiler's own sources that reparses most of the file.

**Not here.** A hole-tolerant checker, incremental semantic analysis of an unsaved buffer (area 2a
covers saved modules), incremental parsing of imported files, a concrete tree or recovery in the
compiler written in Nazm, and `\x` above `7F`.

**As built (amended after the code).** Relexing starts at the first lexeme whose end reaches the
edit, not at the affected chunk's first lexeme. The lexer is context-free from every boundary, so
that is exact, and a one-word edit relexes two lexemes instead of its whole item. The reparse still
starts at the chunk. The one cross-item read the convergence tests found is not the balance scan
but recovery: `fn ,` skips to the next `fn NAME`, reading the next item's name. The recorded
lookahead covers both, and each has a test that fails if lookahead goes unrecorded. Reuse of green
nodes is by construction (`Revision::edit` clones the old children). cstree exposes no node
identity without `unsafe`, so the tests observe reuse as chunk counts and equality, not as
pointers. A declaration dropped from the abstract tree keeps its own concrete node (`RecordItem`,
`FnItem`) with the malformed member inside it as an `Error`, rather than becoming one `Error` node.

## 7.78 Resolution, persisted: one resolved unit per module, one resolver — N77, 2026-10-03

Written before the code. Area 2 is PARTIAL, and area 2a names the gap: no written form of name
resolution exists, so nothing a check decides about a body outlives the process that checked it.
The answers are the checker's (`Resolution`), addressed by session ids no other process can read.

**One resolver, restated.** `nazm_core::check` decides what every name means. `load.rs` decides which
module every `use` names: a relative path, `@std/NAME`, `NAME:path` into a declared package, or the
core prelude. Nothing else decides either. `compiler/module.nz` is the bootstrap subset's own loader,
and it is not a second answer: it resolves relative paths only. From now on it **refuses `@std/` and
`NAME:` imports by name**, where until now it looked for a file of that name and reported it missing.
It still keys modules by a textually normalised path, not a canonical one (`bootstrap.md`). No
semantic rule is copied into the resolved unit: it is written from the checker's answers and read
back as data.

**The product, `nazm.resolved/1`.** One per module that checked cleanly, holding only durable
facts:

- the module's durable key (which carries its package, `ModuleKey::in_package`);
- its source fingerprint, and the checker identity (semantic epoch included);
- its imports in source order: the path as written, the alias if any, the target's durable key, and
  the target's interface hash;
- its definitions, each a `DefKey` with the byte range of its declaration and its name;
- every resolved occurrence of a declared entity in its text: the byte range, and the target as a
  durable identity. A function, record or enum is its `DefKey`. A variant, field or payload field is
  its owner's `DefKey` and its member's name. A built-in is its name.

Locals are absent: their identity is a slot in one body, and they are not durable. Session ids,
absolute paths, timestamps and load order are absent. Occurrences sort by range, then target.
Ranges are byte offsets into the fingerprinted source, so they mean something exactly as long as
the fingerprint does.

**Where it lives, and why its invalidation is already sound.** The resolved unit is a function of
exactly what a `CheckKey` covers: the source, the checker, and each direct dependency's interface.
A body change in a dependency leaves the `DefKey`s it offers, and so every importer's targets,
unchanged. So the unit is stored in the check entry, under the same key. The check entry becomes
`nazm.check/3`: a `/2` entry has no unit and is a miss. The unit is validated against the entry's
own fingerprint and module on read. By change:

| Change | Invalidated |
| --- | --- |
| a private body | its own module |
| an exported signature | its module and every direct importer |
| an alias or a `use` line | the importing module, through its source |
| a package's version or name | every module of it, through `ModuleKey` |
| a semantic epoch | every entry |
| a profile, a target, an optimisation level | nothing: no checking rule reads them (`key.rs`), and profiles run after checking and are never cached |

**Consumers.** `nazm check` writes the unit and reads it back for every module it reuses. `nazm
references DEF [FILE]` is new. It lists every occurrence of the entity `DEF` (a durable key,
`module::kind name`) in the compilation as `nazm.references/1`, built from the modules' resolved
units. A module whose check entry is reused contributes its stored unit, and its body is **not
checked again**: the output reports how many modules were checked and how many reused. `nazm
resolve FILE --json` prints the compilation's units as they would be stored.

**What does not consume it, deliberately.** `nazm build` needs the typed Core IR of every body it
lowers, and a resolved unit carries no types or slots, so a build still checks every module
(area 2a's limitation, unchanged). The language service never persists or reads a cache: an unsaved
buffer is not state a cache may record (§7.18).

**Versions.** Semantic epoch: unchanged, because no program moves between accepted and refused. The
check entry goes `nazm.check/2 → /3`. New machine schemas: `nazm.resolved/1`,
`nazm.references/1`. Interface schema, runtime ABI and Core/MIR/LIR schemas: unchanged, because
nothing they describe moved. Object and build identity: unchanged, because the build does not read
the unit.

**Negative example.** A `/2` entry from before N77 is a miss and the module is checked again. A
unit whose fingerprint differs from the entry's is a miss. **Boundary example.** A dependency whose
private body changes is checked again, and its importer's unit is reused byte for byte: its
occurrences of the dependency's functions name the same `DefKey`s.

**What would prove this unsuitable.** Two processes, or two checkouts, writing different bytes for
one module. A reused unit that names a target a fresh check would not. `nazm references` disagreeing
with the language service's references over the same saved compilation, which is the independent
oracle: the service's index is computed in-process from a fresh check.

**Not here.** Re-exporting a name and exporting it under another name, both language decisions with
no settled design. A build that skips bodies. A persisted resolution for locals. Making the
compiler written in Nazm resolve `@std` or packages (N98).

## 7.79 Traits and methods: named capabilities of a type, resolved before Core IR — N78, 2026-10-03

Written before the code. §7.52 designed traits and deferred them until a program needed one. The
need is now named. With `[T: Equality]` the only requirement a type parameter can carry, generic
code cannot ask anything else of `T`: it cannot render, order, hash or measure one. A program that
wants to print every element of a `Vec[T]` must take a rendering function as an argument at every
call, through every layer of generic code in between. That is the problem a nominal capability
solves, and no larger one is attempted. Coherence, orphan rules and method lookup are therefore
the smallest that make one answer per call.

**Syntax.** Three new forms, and `trait` and `impl` become keywords:

- `[pub] trait Name { fn m(self: Self, p: T) -> R [! {e}]; … }` declares a trait. Each method is a
  signature ending in `;`, and its first parameter is `self: Self`.
- `impl Name for Type { fn m(self: Type, p: T) -> R { … } … }` implements one.
- `recv.m(a, …)` calls a method on a value. `Name.m(recv, a, …)` calls a trait's method by its
  trait (UFCS).

A bound names a trait the way it names `Equality`: `fn f[T: Show](x: T)`, at most one requirement
per parameter, as before.

**Identity.** A trait is a durable definition, `module::trait Name`, a new `DefKind`. A trait
method is its trait's key and its name. An impl is identified by its trait and its type. Each
method an impl defines is an ordinary function of the impl's module, keyed
`module::fn Trait/Type/m`: `/` is in no identifier, so the key can never be an ordinary function's.

**What an impl is for.** An impl's type is a non-generic record or enum, a numeric type, `Bool` or `Str`.
A generic type, a `Vec`, a channel, a function type, a capability or a type parameter is refused
(`N0604`). That keeps overlap decidable by equality: there are no impls over a pattern of types.
An impl defines every method of its trait exactly once, with the trait's signature with `Self`
replaced by its type, effects included (`N0602` missing, `N0603` extra or mismatched).

**Coherence.** At most one impl of a trait for a type in a compilation (`N0600`, with the other
impl as a secondary label). The orphan rule: an impl must be in the module that declares the trait
or the module that declares the type, and for `Int`, `Bool` and `Str` only the trait's module
(`N0601`). Both rules are checked over the whole compilation, so a module's impls are part of its
interface.

**Method lookup.** `recv.m(args)`: let `R` be `recv`'s type. Candidates are the traits **in
scope** — declared in this module or imported — that declare `m` and have an impl for `R`. When
`R` is a type parameter, the candidates are the traits its bound names. Exactly one candidate is
the call. None is `N0605`, naming the traits that would have one if imported. More than one is
`N0606`, and UFCS chooses. `Name.m(recv, args)` takes the trait from the written name and the type
from `recv`. An enum's variant still wins where `Name` is an enum (`E.A()`), so no program that
checked before changes meaning: `Name.m(…)` with `Name` a value, or with unlabelled arguments, was
refused before this milestone. A field of function type is not a method: `r.f(1)` with no method
`f` is refused as before, with the existing help to bind it first.

**Not here, deliberately.** A trait is not a type (`N0607`): no trait objects, no dynamic dispatch,
no `dyn`. Default method bodies, associated types and constants, generic traits, generic methods,
supertraits, impls with bounds, inherent impls without a trait, and methods as values (`x.m`
without a call, `N0608`) are all out. Each is a language decision this milestone does not need, and
each is refused by name rather than half-supported. Generic function values and closures inside
generic functions (`N0387`) stay **outside the supported model**. Each would need a thunk per
instance, and a bound now covers the programs that wanted them (pass `T: Show` instead of a
rendering closure). `N0387` is retained with that reason, not retired.

**Lowering, one representation per layer.** The checker resolves every method call before Core IR.
A call on a concrete type becomes an ordinary call of the impl's function, with the receiver first:
Core IR, MIR, both backends and the interpreter see a call they already understand, and `calls`
records the impl function at the method's span, so definition, hover and references work unchanged.
A call on a type parameter becomes the one new Core IR form, `CallTrait { trait, method, self_ty,
args }`. MIR resolves it per instance: `self_ty` substituted, the impl looked up, a direct call
emitted. MIR never contains it. The interpreter resolves it per frame, from the frame's type
arguments, in the same table. Dictionaries are not used: monomorphisation already exists, and it
makes every trait call a direct call in native code. The impl table (trait, type) → functions is
the checker's, in `Resolution`, and nothing downstream builds another.

**Effects, authority, provenance.** A method's effects are its declaration's, and a call exercises
them as any call does. An impl must declare exactly the trait's effects, so a call through a bound
cannot exercise more than the bound says. Authority is passed as parameters, as everywhere. For
provenance, a call on a concrete type is a call of a known function. A call through a bound is of
unknown origin joined with its arguments, conservatively, as an indirect call's is.

**Persisted.** An exported trait, and every impl a module declares, are in its interface, because
an importer's method lookup and coherence check need them: `nazm.interface/7 → /8`. The resolved
unit (§7.78) gains the kinds `trait` and `trait method`; its schema stays `/1` because the kinds
are values of an existing field, and a reader that does not know them is incomplete, not wrong.
Core IR's printed form gains `CallTrait`: `nazm.core-ir/1 → /2`, since an old reader would misread
a call it does not know. MIR's printed form is unchanged, because it never contains one.

**Versions.** Semantic epoch `21 → 22`: `trait` and `impl` declarations, method calls and trait
bounds moved from refused to accepted, and a program using `trait` as a name moved from accepted to
refused. Interface `/7 → /8`. Core IR print `/1 → /2`. Runtime ABI unchanged, since no runtime
entry point moves. MIR, LIR, objects and package schemas unchanged. An object's key covers its LLVM
input, which changes exactly when the code does.

**Libraries.** `@std/show` gains `pub trait Show { fn show(self: Self) -> Str; }`, impls for `Int`,
`Bool` and `Str`, and `show_all[T: Show](v: Vec[T], sep: Str) -> Str`, all written in Nazm. It is
the higher-order library use this milestone is held to.

**Tooling.** A trait's name, its methods' declarations, and a method call through a bound are
indexed: `ReferenceTarget::Trait`, `ReferenceTarget::TraitMethod`. A concrete call refers to its
impl's function. The language service's definition, hover and references, and MCP's context
packets, read those as they read every other target.

**The compiler written in Nazm** refuses `trait` and `impl` by its ordinary syntax error. That is
the recorded parity gap (N98).

**Negative examples.** Two impls of `Show` for `Point` (`N0600`). An impl of `Show` for `Int` in a
third module (`N0601`). `x.show()` where no trait in scope has an impl for `x`'s type (`N0605`).
`fn f(s: Show)` (`N0607`). **Boundary examples.** `Show.show(1)` with `Show` imported under an
alias, `s::Show.show(1)`. An impl in the type's module of a trait from `@std`. A generic call
`show_all[Point]` that reaches `CallTrait` through two levels of generic functions.

**What would prove this unsuitable.** A program whose method call resolves differently in the
checker, the interpreter and a native build. A coherence violation accepted because its impls are
in different modules. A generic body that checks under its bound but cannot be instantiated for a
type that satisfies it.

**As built (amended after the code).** An impl is *usable* only from its own module and from modules
that import it directly. The constitution's lookup said "an impl for `R`". That alone would have let a
module's method resolution depend on an impl in a module outside its check key, so a cached check
could silently go stale. Coherence is still checked over the whole compilation. A trait's method list
does not recover member by member: a malformed method gives up the trait, as a record's field list
did before N76. A persisted interface carries traits, impls and bounds for identity and invalidation,
but `rehydrate` does not rebuild them, so a module checked against a persisted interface cannot use
its traits. Each impl method is hidden-linked, not internal, because a generic instance in an object
of its own may call it through a bound. The interpreter keeps no type arguments per frame, so it
dispatches a `CallTrait` on the receiver value's runtime type. That type is the one the bound was
instantiated at, because impls exist only for types whose values carry their type.

## 7.80 Effects and capabilities v3: the bridge made a contract, attenuation, subsumption — N79, 2026-10-03

Written before the code. Areas 5 and 6 are PARTIAL for four named reasons. First, **the ambient
bridge**: a function that declares no effect set exercises its caller's authority, and nothing says
so. Second, **no attenuation**: the only authority is a whole kind. Third, **no effect
subsumption**: a pure function value is refused where an effectful one is allowed. Fourth, **no
revocation**. This section settles each, and states plainly what stays.

**The bridge, made a representable contract.** Deleting inheritance outright would refuse every
undeclared function in the repository that prints (more than a thousand checks, the compiler written
in Nazm among them, whose checker reads no capabilities). That is a migration of the language's
whole corpus and of the second compiler, which this milestone does not attempt. What it does
instead:

- Every function's contract now has three parts. Its effects, declared or inferred. Its capability
  parameters. And its **inherited authority**: the capability kinds its body exercises without
  holding them lexically, computed by the same rule a declared function's authority is checked by.
  A declared function's inherited authority is always empty, because missing authority is
  `N0369`.
- The inherited authority is recorded in the resolution. It is published in the interface (each
  export's `inherits`, `nazm.interface/8 → /9`, since a `/8` reader would take an inheriting
  function for one that inherits nothing). `nazm inspect` reports it. So the bridge is no longer
  ambient: it is a named part of a callable's type-level contract, and tools and reviewers see
  exactly which functions use it.
- **Strict authority is available and enforceable.** The restriction-profile rule
  `explicit-authority` refuses any function in its package whose inherited authority is not
  empty. Under it, authority is only what a function's scope holds. Undeclared inheritance is gone
  for that code, and `main`'s parameters are the only root.
- **The default does not change.** A program not under the profile still inherits as before, so
  area 6 stays PARTIAL. Migrating the corpus, and teaching the compiler written in Nazm
  capabilities, are named as the remaining work.

**Attenuation.** One narrower kind, `OutCap`, authorises the standard streams only: `print` and
`eprint`. It is derived, never constructed: `cap_out(io: IoCap) -> OutCap` is the only way to make
one (`N0370` still refuses construction), and `main` may take one as a root. Authority is a
preorder, monotone by construction. Holding an `IoCap` is holding the authority an `OutCap` gives,
because `print` asks for `OutCap` and an `IoCap` implies it. Nothing implies `IoCap`, and there is
no built-in from `OutCap` to `IoCap`. A function given only an `OutCap` cannot read a file, write
one, read the arguments or exit, and each of those is `N0369`. The effect stays `io`: attenuation
narrows authority, not the effect vocabulary. Its scope is lexical, like every capability's.

**Effect subsumption.** Where a value of function type is passed or returned, a function type whose
effect set is a subset of the expected one is accepted: `fn() -> Int ! {}` where `fn() -> Int !
{io}` is expected. Parameters and results stay invariant, and so does an effect parameter in either
type, because a row is matched by binding and not by inclusion. Below the checker, effects are
metadata and every verifier compares types up to effects (§7.53), so nothing downstream moves.

**Callable polymorphism and traits.** One effect parameter per function, as N51 settled. A trait
method's declared effects are its contract, and an impl's must equal them (N78). Capabilities cross
traits, closures and methods as ordinary parameters and captures, and the profile rule applies to
an impl's methods like any function.

**Revocation: outside the model, deliberately.** A capability has no identity: copies are
indistinguishable, and nothing records a grant. Revocation would need an allocated cell per grant, a
check at every authorised call, and runtime state. Nazm's capabilities are static, and the language
does not offer revocation. A program that needs a revocable grant passes a channel or a value it can
invalidate. That is ordinary data, not authority.

**Versions.** Semantic epoch `22 → 23`: `cap_out` and `OutCap` became built-in names (a program
defining either moved from accepted to refused), and a subsumed function value moved from refused to
accepted. Interface `/8 → /9` for `inherits`. Runtime ABI unchanged: a capability erases to a word,
and `cap_out` returns one. Core IR, MIR and LIR schemas unchanged: `cap_out` is a built-in, and
built-ins are named, not schematised. Profile rule ids gain `explicit-authority`.

**Negative example.** `fn log(o: OutCap) -> Int ! {io} { write_file(…) }` is `N0369`: `OutCap`
does not authorise a file. **Boundary example.** Under `explicit-authority`, `fn helper() -> Int {
print("x"); 0 }` called from `main(io: IoCap)` is refused, with `helper`'s inherited authority named.
Outside the profile it is accepted exactly as before.

**What would prove this unsuitable.** A function that exercises authority it does not hold
lexically, with an empty recorded `inherits`. Any path from `OutCap` to `IoCap`'s authority. A
subsumed value whose call performs an effect its expected type does not allow.

**As built (N79, 2026-10-03).** Four amendments, each narrower or simpler than the text above:

- **No `cap_out`.** Attenuation is by passing: the checker's one relation `assignable` accepts an
  `IoCap` where an `OutCap` is expected (`CapabilitySet::implied`), at every argument, `spawn`
  argument and result. A built-in would have been a second spelling of the same fact. `OutCap`
  is still never constructed (`N0370`), and `main` may take one as a root.
- **The interface stays `/8`.** Inherited authority depends on a body, and an interface publishes
  declarations. A declared export inherits nothing by construction, and an undeclared import is
  already every effect to its importer (§7.38), so a `/8` reader loses nothing it relied on. The
  record is the resolution, and `nazm inspect` reports it as each definition's `inherits`.
- **The profile is `authority`.** `explicit-authority` is enforced by a profile of that name with
  that one rule. It is not added to `critical` or `cyber`, whose `declared-effects` already
  implies it for the program's own modules. It applies to the program's own modules, not the
  prelude and the standard library, which declare every effect.
- **Core IR's verifier compares results up to effects**, as it already compared arguments
  (§7.53). The checker's acceptance of a subsumed result reached a verifier that compared results
  exactly, and lowering refused it (`N0900`). Nothing else below the checker moved.

The epoch is 23 for `OutCap` becoming a built-in type name and for the two new acceptances.

## 7.81 Provenance v3: container cells, indirect targets, local shapes, one policy engine — N80, 2026-10-03

Written before the code. Area 7 is PARTIAL for named reasons, and this section settles the ones
that are precision rather than kind. **Containers**: every read of a sequence, `Vec` or channel is
`unknown`, so a path built from a `Strs` the program filled with its own arguments is refused.
**Indirect calls**: a call through a function value or a trait bound returns `unknown`, and a
closure's parameters are `unknown`. **Records and variants** are tracked whole. **The one
restriction** is code, not policy: `N0372`'s rule and `cyber`'s `contents-stay-in-files` are two
implementations of one idea. **No traces**: `nazm explain-flow` names the origins at a sink, never
how they got there. What stays out is stated at the end.

**The domain.** Unchanged: four compiler-owned origins (`argument`, `file`, `authority`,
`unknown`), explicit data flow only, conditions contributing nothing, joins as set unions, and
summaries as the least fixed point over the whole program. No origin is added, so no label a
program writes exists.

**Container cells (type-keyed, alias-free).** Every container has a **cell**, named by its type:
`Ints`, `Strs`, `Vec[T]`, `Chan` and `Chan[T]`, with `T` written as the checker shows it. A cell's
origins are the join of every value written into **any** container of that type, anywhere in the
program: `ints_push`, `ints_set`, `strs_push`, `strs_set`, `vec_push`, `vec_set`, `chan_send`,
`chan_send_of`, and the value `chan_select_of` and `chan_select_until` move from `Chan[T]` onto
`Vec[T]` (a write of the `Chan[T]` cell into the `Vec[T]` cell). Every read of one — `ints_get`,
`ints_pop`, `ints_len`, `ints_sum`, the `strs_` and `vec_` reads, `str_join`, `chan_recv`,
`chan_recv_of` — carries its cell's origins, and its other arguments'. This is sound without alias
analysis, because a handle of type `T` can only ever hold what was written into some handle of
type `T`, and every write is one of these built-ins: no foreign function takes a handle (§7.44), and
the runtime fills none. It is imprecise exactly where two containers share a type, and that is
stated, not hidden. A container whose type mentions a type parameter, in a generic body, is the
**wild** cell: a write to it reaches every cell, and a read of it reads every cell. Same-named
records from two modules share a cell: an over-approximation, never a miss. A select's index is
control, not data, and carries nothing: implicit flow, outside the model as a condition is.

**Indirect targets (type-based).** A call through a function value may reach any function value
the program makes of a matching type: each named function written as a value, and each closure.
Types match **up to effects** (N79's subsumption lets a pure value stand where an effectful one is
expected) and exactly otherwise; a function type that mentions a type parameter matches every
function value. The call's result is the join of every candidate's summary applied to its
arguments; each candidate's parameters receive its arguments; a candidate whose parameter reaches
a restricted sink carries the rule back to the argument. A call through a trait bound (N78) reaches
every impl's method of that trait method. With no candidate, the call can never run and carries
nothing. A **closure's parameters** are therefore ordinary parameters, received from the indirect
calls that can reach it, and its **captures** are parameters received from its maker, at the point
it is made — so a captured path that comes from the maker's file read is still refused, and one from
its arguments is not.

**Local shapes (field and variant sensitive within a body).** Inside a body, a binding assigned
only constructions — a record, a variant, or another binding with a shape — keeps each field's
flow apart, keyed by variant and field **name**. A field read, a pattern's binding and `?` on a
`Result.Ok` read that field's flow; a variant never assigned to the binding contributes nothing. At
a **call boundary** — an argument, a result, a store into a container — a value is whole again: the
join of its fields. Flow-insensitive like everything else: a binding assigned anything without a
shape is whole from then on.

**One policy engine.** A **policy** is a list of rules `(sink class, forbidden origins, scope)`,
where the scope is *declared functions* or *every function*. The language's policy is one rule,
`(path, {file, unknown}, declared)`, and it is `N0372`, unchanged in meaning and message. A profile
adds rules: `cyber`'s `contents-stay-in-files` is `(output, {file, unknown}, every)`, and a new
rule, `checked-paths`, is the language's rule over every function — the provenance half of N37's
bridge, closed by profile — added to `cyber`. One function decides a violation for both, and
gives the same witness.

**Traces.** Every sink, as solved, carries for each origin that reaches it one deterministic
chain: where the origin entered, every call, return, container cell, capture and caller it
crossed, in order. `nazm explain-flow --json` prints them as `nazm.flow/2`; `/1` had no `traces`.
`N0372` and `N0510` show the same chain as secondary labels.

**What tooling and the programmer observe.** A program accepted before stays accepted except where
`cyber` gains `checked-paths`. A program refused before for a container read, an indirect call or a
closure parameter that the cells and targets now prove carries no file contents is accepted. Nothing
runs differently: provenance is erased before Core IR.

**Versions.** Semantic epoch `23 → 24`: `N0372` moved from refused to accepted for such programs.
Check entry `nazm.check/3 → /4`: a module's persisted facts gain cell writes and reads, indirect call
sites and closure creations; a `/3` entry is a miss. Interface `/8` unchanged: no fact is in it.
`nazm.flow/1 → /2` for traces. Core IR, MIR, LIR, runtime ABI and every object key unchanged:
provenance is erased before Core IR. Profile ids gain `checked-paths`. `nazm.context`,
`nazm.snapshot` and `nazm.delta` unchanged in shape: a summary's fields are the same three.

**Negative example.** `let xs = strs_new(); strs_push(xs, read_file("cfg")); write_file(strs_get(xs,
0), "x")` in a declared function is `N0372`, with a trace through the `Strs` cell to the
`read_file`. **Boundary example.** The same with `strs_push(xs, arg(1))` is accepted — unless any
other `strs_push` in the program writes a file's contents into any `Strs`, which the trace names.

**Not solved, deliberately.** Implicit flow and non-interference; timing and side channels; per-handle
or per-index precision (two containers of one type share a cell); shapes across calls; user labels,
sanitisers and a policy language a program writes; runtime tracking; foreign code, whose results
stay `unknown` and whose arguments are not sinks (the `ForeignCap` authorised them).

**What would prove this unsuitable.** A program where a value written into a container reaches a
read of another container of the same type with that origin missing from the read. An indirect call
that runs a function value outside its candidate set. A field read whose flow omits an origin that
was written to that field. A policy verdict for `N0372` that differs from the language rule it
replaced, on any program in the repository.

**As built (N80, 2026-10-03).** The design held; five additions and one measurement:

- **What C hands an export is `unknown`.** An exported function (N55) is called by C, so each of
  its parameters receives `unknown`. It is pure and has no sink of its own, but it can write a
  container, and through the cell that reaches the program. The constitution did not say so; the
  model needed it to be sound.
- **A device register is a source of `unknown`.** `mmio_read32` was a read of a shared handle; with
  containers followed it is the one built-in whose result is `unknown`.
- **A missing module's facts make everything indirect `unknown`.** When a reused module's facts do
  not rehydrate, its writes, values and callers cannot be seen, so every cell, every indirect call's
  result and every closure's parameters also carry `unknown`, and its functions reach every sink.
- **`?` yields every part but `Err` and `None`.** That is the payload of the prelude's `Result`, the
  only type `?` applies to.
- **Reach is per sink class.** Which parameters reach a sink is solved for every class, so a rule
  of the policy over any class and scope uses one solver. The published summary's `reaches` stays
  the path's, as `nazm.context/3` documents it.
- **The falsifier was run.** N79 (`8daf107`) and N80 checked all 237 `.nz` files in the repository
  with identical codes on every one. A cold check of `compiler/emit.nz` costs 1.125×, measured in
  `performance.md`; later rounds re-solve only the functions that read a grown cell.

## 7.82 The backend contract v2, stage one: bytes, ownership and a printed LIR — N81, 2026-10-03

Written before the code. Area 10 is PARTIAL because LIR is not a level of its own. `nazm-lir` settles
units, symbols, linkage and canonical field order from validated MIR, and then **each backend
translates MIR's operations itself**: `emit.rs` for LLVM, `func.rs`, `prims.rs` and `helpers.rs`
for Cranelift. Three responsibilities are duplicated or orphaned:

- **Bytes.** Cranelift decides every record's and enum's byte layout in its own `layout.rs` (an
  enum a tagged union at a fixed payload offset), with an 8-byte pointer assumed. LLVM never
  decides bytes: it writes named aggregates and lets LLVM lay them out, an enum as the product of
  every variant's payload. No value ever crosses between the two, so neither is wrong, but no one
  owns "what are the bytes of this type on this target", and `wasm32`'s 4-byte pointer is known
  only to LLVM.
- **Ownership.** Whether a value of a type owns anything a drop must give back is computed twice,
  as a fixed point in each backend.
- **Visibility.** Nothing prints what LIR decided, and nothing validates it.

**What N81 settles (stage one).** `nazm-lir` becomes the one owner of the target's **data layout**
(pointer width, word, `Bool`, the three-word `Str`), of every record's and enum's **byte layout**
on that target (offsets, size, alignment; an enum a tag word then a payload area aligned for its
widest variant), and of the **ownership table**. Both backends read them: Cranelift's `layout.rs`
keeps only how a byte layout becomes Cranelift values, and both backends' ownership fixed points
are deleted. A **validator** checks every layout on every native build — fields ascending, inside
their structure, aligned, sizes multiples of alignment, every variant inside its enum's payload
area, ownership closed under containment — and a broken table is a compiler bug (`N0900`), never a
miscompilation. `nazm lir FILE [--target T]` prints `nazm.lir/1`: the target's data layout, every
unit with its symbols and linkage, every layout and the ownership table, deterministic, with a
schema checked against real output. The differential fuzzer (N73) gains records with `Bool` and
`Str` fields and an enum with payloads, so layouts are exercised on the interpreter, LLVM at
`-O0` and `-O2`, and Cranelift against each other.

**What N81 does not settle, stated before the code.** The prompt's acceptance is one MIR→LIR
lowering into an instruction level that both backends consume, with no semantic path from MIR into
either backend, an LIR oracle interpreter, and LIR-backed mutations. That is a rewrite of both code
generators (about 7,400 lines) and of everything they carry — DWARF positions, the struct-of-arrays
layout, the vectoriser's patterns, exports, the freestanding and WebAssembly targets — and this
stage does not attempt it: risking the reference compiler's correctness is not a trade this
programme makes for a dashboard. **N81 is therefore not accepted under its own criteria.** Area 10
stays PARTIAL, named: operations are still translated per backend, guarded by differential testing
rather than by construction; LLVM's enum is still the product of its variants. The instruction
level is recorded as the remaining work.

**Observable and versioned.** Nothing a program does changes. `nazm lir` and `nazm.lir/1` are new
(there was no `/1` before: the prompt's `/2` assumed one). Semantic epoch, interface, Core IR, MIR
and runtime ABI unchanged: no language rule, interface shape, IR or runtime moved. Cranelift's
objects are byte-identical, because the layouts are the same numbers computed in one place; the
object cache keys on bytes, so no key moves. LLVM text is byte-identical.

**Negative example.** A record whose field offset the validator finds outside its size is refused
as `N0900` before any code is generated. **Boundary example.** On `wasm32-unknown-unknown` a `Str`
is `{ptr: 0, len: 8, owner: 16}` in 24 bytes, its pointers four bytes each and the length still
eight; on every 64-bit target the same three words are 0, 8, 16 of 24.

**What would prove this unsuitable.** Any Cranelift object byte that differs from before, on any
program in the repository; any program both backends compile correctly that the validator refuses;
any layout whose printed offsets disagree with the bytes Cranelift writes.

**As built (N81, 2026-10-03).** As constituted, with three notes. The printed form is `nazm.lir/1`,
there having been no earlier one. The validator also checks ownership against containment, which
the constitution listed and the code needed a table for: ownership is computed once by
`abi::owns`, which the LLVM backend's helper now calls. And the falsifier ran: every `.nz` program in
the repository built by N79 and N81 with each backend gave byte-identical objects, and the extended
fuzzer agreed on every tier.

## 7.83 Runtime v2: the runtime as a built, versioned, verified artifact — N82, 2026-10-03

Written before the code. Area 13 is PARTIAL for one named reason this milestone does not touch: one
OS thread per task (N83's). What N82 settles is the runtime's **form**. Today it is LLVM text the
compiler generates into every build, specialised to the services the program reaches, and compiled by
clang on every build whose project store does not already hold that exact text. Nothing can build a
runtime once, ship it, check that it is the one a compiler expects, and link it from another build.
Both native backends already link the same runtime implementation — Cranelift's units call the same
text, through the typed registry — so that half of the prompt's acceptance holds before the code, and
this section keeps it.

**What is settled.**

- **A runtime artifact.** `nazm runtime build [--target T] [--profile hosted|board|wasm] [-O N] -o
  DIR` compiles the whole runtime for one target and profile — every service — once, into
  `DIR/libnazmrt.a`, beside a manifest `DIR/nazm-runtime.json`, `nazm.runtime/1`: the runtime ABI
  revision, the runtime digest that already enters every object key, the target, the profile, the
  services, the optimisation level, the toolchain that compiled it, and BLAKE3 digests of the text and
  of the archive. The profile is the target's: `hosted` on the four operating-system targets, `board`
  on `aarch64-unknown-none`, `wasm` on `wasm32-unknown-unknown`; another is refused.
- **Linking it.** `nazm build --runtime DIR` links that archive and generates no runtime. The
  platform entry is still the program's own. Before anything is linked, the build refuses — exit 4 (3 or 1 since Q1-C1),
  the reason named, nothing produced, never a silent fallback to generating the runtime — a missing or
  unreadable manifest; another schema; another ABI revision or runtime digest (a compiler whose
  runtime differs); another target or profile; a service the program reaches that the archive does
  not provide; and an archive whose bytes are not the ones the manifest describes. Both backends take
  `--runtime`, and link the same archive.
- **Checking it.** `nazm runtime verify DIR` performs the same checks without a program, and says
  whether the artifact fits this compiler.
- **Nothing a program does changes.** The archive holds the same runtime text with every service, so a
  program linked against it prints the same, ends the same, and reports the same memory counters as
  one whose runtime was generated. Tested over programs reaching every service, on both backends.

**What is not settled, stated before the code.** A runtime written in Rust: only the host's Rust
target is installed and builds stay offline, so it could exist only for one target beside the text
for every other — two implementations where there is one. An allocator of its own and a scheduler
beyond one thread per task (N83). Every build without `--runtime` still generates its runtime, as
before, and is what the tests and the default keep using; the artifact is the path for building a
runtime once and checking what is linked, not yet the default.

**Versions.** Runtime ABI revision unchanged: no runtime symbol, signature or global moved.
`nazm.runtime/1` is new. `nazm.provenance/1` gains an optional `runtime_archive`, the archive's digest,
present only when one was linked: additive, as its schema allows. Object keys unchanged: an archive
is a link input, and nothing about linking is keyed (area 10b). Semantic epoch and interface
unchanged.

**Negative example.** An archive built by a compiler whose runtime digest differs is refused, naming
both digests. **Boundary example.** An archive built with `--profile board` for `aarch64-unknown-none`
links into a freestanding image, and a hosted build given it is refused for the profile.

**What would prove this unsuitable.** A program whose output, ending or memory report differs between
a generated runtime and the artifact; a mismatched or altered artifact that links.

**As built (N82, 2026-10-03).** As constituted. Three details the code settled: a board's and
WebAssembly's manifests list only `stack`, since what those runtimes lack is refused by the
freestanding build itself (N62), and the service check applies to the hosted profile; the archive is
written with `ZERO_AR_DATE`, so the same runtime archives to the same bytes; an unknown target is
exit 4, as a missing toolchain is (5 since Q1-C1, a usage error). The falsifier ran: every program in the repository built with a
generated runtime and with the artifact, on both backends — 228 builds — printed, ended and reported
memory identically.

## 7.84 The task pool: M:N tasks on bounded workers — N83, 2026-10-04

Written before the code. Area 15 is RESEARCH: §7.56 chose the model — stackful tasks on guarded
`mmap`'d stacks with a small per-target switch — and named what must have a task-local home before a
pool exists. This section settles the pool, as an **opt-in mode of the native runtime**, beside the
default of one OS thread per task, which does not change.

**Selecting it.** A native program whose environment has `NAZM_SCHEDULER=pool` runs its tasks on a
pool of `NAZM_WORKERS` OS threads (default: the online processors, at most 64), started at the first
`spawn`. Anything else, or no variable, is today's model. The choice is the runtime's, made once per
process; the compiler, both backends and the interpreter are unaffected, and so is every program's
meaning.

**Tasks.** A task is a record and a guarded stack: `NAZM_TASK_STACK` bytes (default 256 KiB, rounded
to pages) above one inaccessible page, `mmap`'d at `spawn` and released when the task ends. A task is
**pinned to one worker** — the one `spawn` assigned it, round robin — for its whole life. Pinning is
what makes the runtime's thread-local state sound without rewriting how compiled code reads it: a
compiler may keep a thread-local's address across a call, so a task must never resume on another
thread. The four thread-local words the runtime promises — the first-failure flag, message and
length, and the stack limit — are saved into the task when it leaves its worker and restored when it
returns, so each task has its own. A task's stack limit is set from its own stack, so `nz.stack_check`
reports recursion that would reach the guard page as it does on a thread (`N0408`).

**States and suspension.** A task is runnable, running, parking, parked or done. The operations that
wait are exactly §7.56's suspension points: a full channel's send, an empty channel's receive, a select
with no channel ready, and a scope's join. In a task, each **parks the task and frees its worker**
instead of blocking it: the task records itself as a waiter on what it waits for, under that thing's
own lock, then switches to its worker. Whoever changes that state wakes every task waiting on it, as
the condition variable is broadcast today. A wake that arrives while the task is still switching out is
not lost: the worker finishes the park and sees it. A select with a deadline parks with that deadline,
and its worker wakes it when it passes. A select that does not wait — `ms <= 0` — yields its worker
before returning, so a task polling for another on the same worker cannot starve it. The program's
`main` is not a task: its waits block its own thread, as now.

**Scheduling.** Each worker runs its own tasks in first-in, first-out order until each parks, yields
or ends. There is no preemption and no work stealing: a task that computes without waiting holds its
worker until it does, and only its own worker's tasks wait for it. That is the fairness this pool
promises — first in, first out among a worker's runnable tasks — and no more.

**Failure, cancellation and shutdown.** Unchanged in meaning: a failure is the task's own, its scope
joins every task and reports the first by start order; cancellation is a closed channel. Workers never
exit; the process ends as it does today, when `main` returns.

**What is refused or not covered.** A foreign (C) call that blocks holds its worker: there is no
hand-off, and it is stated, not hidden. A task's stack is fixed: recursion past it is `N0408`, and a
program that recursed deeply in a task under the thread model may need a larger `NAZM_TASK_STACK`.
The interpreter keeps a thread per task. Only aarch64 and x86-64 are covered; the switch is verified on
`aarch64-apple-darwin`, and on `x86_64-apple-darwin` under Rosetta, and compiled but not run for the
two Linux targets. **Deadlock freedom is not claimed**: a program that deadlocks on threads deadlocks
on the pool.

**Versions.** Runtime ABI revision `10 → 11`: the waits and wakes go through three new runtime
functions, a task record exists, and the per-target switch is a new artefact `switch` that every
native build reaching tasks links (and every runtime artifact carries). Semantic epoch, interface,
Core IR, MIR and LIR unchanged: no language rule moved. `nazm.runtime/1` gains nothing: the archive
holds the switch for its target. `NAZM_SCHED_REPORT` gains `workers=` and `peak=`, the most tasks alive
at once.

**Negative example.** Ten thousand tasks each waiting on a channel `main` fills: under the thread model
that is ten thousand OS threads; under the pool, `NAZM_WORKERS` threads, every waiting task parked.
**Boundary example.** A task in `while true { chan_select_until(cs, out, 0) }` waiting for a sibling on
the same worker still sees the sibling run, because a non-waiting select yields.

**Accepted when** (§7.56's, kept): many more logical tasks than OS threads with no supported waiting
operation holding a worker, and spawn latency, switch cost and task memory measured as resident set
size, with the page size, against the thread model.

**What would prove this unsuitable.** A program whose output, ending or memory report differs between
the two models; a waiting task that holds its worker; a lost wake-up; a task observing another's
failure or stack limit.

**As built (N83, 2026-10-04).** Three amendments. **The default worker count is 4**, not the online
processors: the call that reports them takes a constant that differs between macOS and Linux, and
the runtime text is target-independent. **The pool is joined to the thread model's texts as they are
emitted**, not written into them: `compiler/emit.nz` carries the task and channel texts line by line
(`runtime parity`), so those constants are unchanged and the Rust runtime rewrites their waits, wakes,
join and spawn when it emits them; the compiler written in Nazm keeps one thread per task. **The
switch is its own artefact**, `switch`, linked wherever the runtime has tasks or channels — always
under Cranelift, whose runtime is its whole baseline — and carried by a hosted runtime artifact
(N82) as a second archive member. A test program that waits inside a scope for a task that fails
first deadlocks on both models, as §7.56 says it must; the pool's tests receive after the scope.

## 7.85 The standard library 1.0 — N84, 2026-10-04

Written before the code. Area 16 is PARTIAL: N45's foundation is 34 functions in eight modules, enough
to show how the library works, not enough to write a real program against. A program that counts
words, reads a table, serves requests over channels or edits a package manifest today reimplements
sorting, a map, number formatting, path handling and data encoding itself. This milestone fixes the
**1.0 surface**: what a program may rely on, what is deliberately absent, and how the surface may
change.

**What 1.0 contains.** Ordinary Nazm in `library/std/`, each module imported by name, each function
documented, total where it can be, and explicit about its effects and authority:

- `@std/text` grows: case (ASCII), trimming either end, replacing, padding, lines, comparison, and
  byte classes. `@std/seq` grows: a **stable sort by a caller's ordering**, polymorphic in that
  function's effects; reversal; slices; concatenation; `any` and `all`; and the same for `Ints` and
  `Strs` where they need their own.
- New: `@std/fmt` (integers padded to a width, hexadecimal, lists with separators and brackets),
  `@std/num` (absolute value, sign, clamp, checked power, greatest common divisor), `@std/map` (a map
  from `Str` keys, ordered by key, found by binary search: `map_new`, `map_get`, `map_set`,
  `map_remove`, `map_keys`, `map_len`), `@std/path` (join, base name, directory, extension, stem,
  normalising `.` and `..` lexically), `@std/fs` (reading, writing and testing a file under an
  `IoCap`, each failure a `Result` value), `@std/time` (a clock reading and elapsed time under a
  `TimeCap`), `@std/json` (parsing and encoding JSON into an **arena document** — nodes in one
  `Vec`, children as indices, because a type that owns itself through a `Vec` is refused (`N0359`) —
  with accessors by key and index), and `@std/test` (expectations that return a `Result` naming what
  differed).
- **Concurrency utilities stay channel helpers.** A function value may not cross into a task
  (`N0321`), so a library cannot spawn a caller's function; what it can do — collect, drain and send
  many — it does, in `@std/chan`.

**What 1.0 does not contain.** Networking: there is no network authority or runtime, and none is
designed here. Cryptography: only an audited external implementation would do, and there is none to
bind. Floating point: the language has none. Selective adoption by the compiler written in Nazm: it
refuses every `@std` import by name (N77), and stays so.

**Stability.** `library/std/API-1.0` lists every public function of 1.0 with its signature, one per
line, sorted; a test regenerates it from the modules and requires equality, so the surface changes
only by editing that file in the same commit. The policy, written into `spec.md`: within 1.x a function
may be added, never removed or changed in its signature, effects or documented behaviour; a removal or
change is 2.0. An edition, if the language ever needs one, would be how 2.0 is opted into.

**Evidence that it is enough.** Four representative programs under `examples/std/` — a word counter (a
CLI over a file: split, count in a map, sort, format), a table summer (data: lines, fields, parse,
fold), a channel service (concurrency: workers over channels, results collected), and a package tool
(paths and a JSON manifest read, changed and written) — each written only against the library for its
plumbing, run on the interpreter and both native backends with the same output. The library's own
tests check every function at its edges on every tier, and the core operations are measured.

**Versions.** Semantic epoch `24 → 25`: names a program could not import before, it now can, and a
standard module's bytes are part of every importer's check. Interface, IR, runtime ABI unchanged:
the library is source.

**Negative example.** `json_parse("[1, 2")` is an `Err` naming the offset where the input ended, never
a trap. **Boundary example.** `vec_sort_by` on a vector of records with equal keys keeps their order:
stable, as the documentation says.

**What would prove this unsuitable.** A representative program that still needs a helper the library
should have provided; a 1.x change to a listed signature that the manifest test does not stop.

**As built (N84, 2026-10-04).** Four amendments. **Sorting is `@std/sort`**, not part of `@std/seq`: a
restriction profile checks every function of every module a program imports, and a sort allocates in
a loop and calls through a function value, so in `@std/seq` it made a program that only sums a
sequence fail `embedded`. Seventeen modules, then; the surface is the same 126 items. **No epoch moves**: `spec.md` already says a
standard module's changes are carried by its interface hash, as N45 settled, and that holds; the
constitution's epoch 25 was not needed. **Channel helpers are written for `Int` and `Str`**: a
channel cannot carry a type parameter (`N0390`), so `chan_send_all`, `chan_collect` and `chan_drain`
are each two functions. **`num_pow` returns `None` for `Int`'s minimum**: its overflow check is on
magnitudes, and the documentation says so rather than the check growing a special case. A standard
module may import another (`@std/fs` imports `@std/text`).

## 7.86 FFI v3: handles, C-layout records, C-string results, errno, callbacks and shared libraries — N85, 2026-10-04

Written before the code. Area 17 is PARTIAL: C is called with `Int`, `Bool` and borrowed `Str`
arguments, Nazm is exported to C with scalars, and a static library is built. A real C API —
`sqlite3 *`, a `struct timespec`, `strerror`, `qsort`'s comparator, `errno` — cannot be bound. §7.57
designed four of the shapes; this milestone settles them, builds a practical subset of the C ABI,
and names everything outside it as refused.

**The sole owner.** The checker (`nazm-core/src/check/foreign.rs`) owns *which* types may cross and in which
position, by one table: the FFI position rule. Core IR lowering (`nazm-core/src/lower.rs`) owns
*how* a crossing value changes representation, and writes it as ordinary Core IR — a field read, a
record or variant built, a test against zero — so MIR, LIR and both backends see a foreign function
whose parameters and result are `Int`, `Bool`, `Str` (borrowed, as N55) or a C-layout record. The
backends own only two things they always owned: the C calling convention, and the borrowed copy of an
argument for the duration of the call (N55's C string, now also a C-layout record). Nothing else
learns about C.

**Opaque handles.** `extern "C" struct Db;` — C's own spelling of an incomplete struct — declares a nominal type whose values are C pointers Nazm
never reads, writes, compares or frees. A handle comes into existence only as a foreign function's
result; it is passed back to C as an argument, stored in a local, a record field or a `Vec`, and
returned from Nazm functions — ordinary values otherwise. Refused at the operation:
constructing one and reading a field of one (`N0611`: it has no fields to name); `==` or `!=` on one
(`N0304`, as for any type without equality: a pointer's identity is C's to define); and a handle
crossing into a task, by capture (`N0321`) or by channel (`N0390`), exactly as a sequence is refused —
C objects are rarely thread-safe, and nothing in a declaration says which are. Each is the existing
derived rule (§7.13, *Task safety is derived*) answering for a new type, not a new rule. Freeing is C's: the program calls the
C function that frees it, and using a handle after that is C's undefined behaviour, outside every Nazm
guarantee — the same trust boundary as everything else C does.

**Pointer width (v1, Q1-A1-05).** Below the checker a crossing pointer — a handle, a C string
result, a callback — is an `Int`, so it crosses as 64 bits. That is C's pointer on every 64-bit
target and on no 32-bit one: there a C function takes and returns 32-bit pointers and lays a
pointer field out in four bytes. A build for a 32-bit target (`wasm32-*`, `thumbv7m-none-eabi`)
refuses each such crossing by name, `N0381` at the foreign declaration (`build/lower.rs`,
`narrow_pointer_refusal`), rather than importing a signature C does not have; crossing pointers at
the target's width is not built.

**Nullability is in the type.** A result `-> Db` promises a non-null pointer: the call checks it, and
a null result fails the call with `N0409` *after* C returns — never a handle that is null. A result
`-> Option[Db]` maps null to `None` and anything else to `Some`. An argument is always a handle: no
null pointer can be written in Nazm, and a C function that accepts null is called with a handle or
not at all. `Option[Db]` as an argument is refused (`N0381`): it would be the one place a Nazm value
means "no pointer", and nothing needs it yet.

**C strings returned.** A result `-> Str` is a `const char *` that C still owns: Nazm copies its bytes
up to the NUL into a Nazm string immediately after the call and never frees C's. A null result fails
with `N0409`; `-> Option[Str]` makes it `None`. A C function returning a string the *caller* must free
is bound as a handle and its free function — the ownership is then written in the program, not
guessed. The copy is one allocation and one `strlen` per call, stated.

**C-layout records.** `extern "C" struct Point { x: Int, y: Int }` declares a record whose fields are
laid out **in declaration order** with C's alignment rules — `int64_t` for `Int`, `bool` for `Bool`, a
pointer for a handle, the nested struct for a nested C-layout record — so declaration order *is*
meaning for this record, unlike any other (§7.43). Its fields may be only those. A C-layout record
crosses as an argument by **borrowed pointer**: C receives a `const struct Point *` to a copy the
caller made for the call and frees after it, as a borrowed C string is. Passing a struct *by value*,
a struct result, a union, a bit-field, an array field and a field of any other type are refused
(`N0381`): each has per-target register rules the compiler does not implement. Otherwise a C-layout
record is an ordinary record.

**`errno`.** `c_errno()` returns the `errno` value the most recent foreign call on this thread left,
captured by generated code immediately after the call returns and before anything else runs. It
performs the effect `foreign`, as calling C does. It is defined on the hosted targets only (`__error`
on macOS, `__errno_location` on Linux); a freestanding or WebAssembly build that calls it is refused
(`N0613`). Under the task pool (§7.84) a task is pinned to its worker, so the value is the task's
unless the task blocked between the call and the read — written down rather than prevented.

**Callbacks.** A foreign parameter may have a function type whose parameters and result are `Int` and
`Bool`: a C function pointer. Its argument must be the name of an **export** (`pub extern "C" fn`,
§7.57) — never a closure or any other function value (`N0612`) — so the pointer C receives is the
export's C entry, which needs no environment, cannot be freed, and is valid for the life of the
process. Reentrancy is the export rules' consequence: an export performs no effect, holds no Nazm lock
and takes no capability, so C may call it from inside the foreign call, again after it, and from any
thread. **Thread attach**: an export uses no per-thread runtime state except its failure flag, which is
per-thread; a C thread the runtime never started may call one. **Unwinding**: as N55, a Nazm failure in
an export ends the process with status 2; it never unwinds through C frames, and a C `longjmp` across
Nazm frames is outside every guarantee.

**Shared libraries.** `nazm build --lib --shared -o libx.dylib` (or `.so`) links the library's objects
and the runtime into one shared object whose only exported symbols are the program's exports, with the
same header `--lib` writes. Two Nazm shared libraries in one process each carry a runtime; neither
sees the other's state. Linking *against* a shared library is `--link libfoo.dylib`, as any link
input. **Dynamic-library lifecycle**: Nazm never `dlopen`s or `dlclose`s — a library is mapped by
the loader before `main` and unmapped at process exit, so no handle or callback outlives its code.
Loading at run time is refused by absence: there is no construct for it.

**Bindings.** `nazm bindgen` widens with the language: an incomplete `struct S *` becomes `extern "C"
struct S;`, a complete struct of `int64_t`/`bool` fields passed as `const struct S *` becomes `extern
"C" struct S { … }`, and a `const char *` result becomes `Option[Str]` (a header never says non-null).
Everything else is a line naming its reason: by-value structs, unions, arrays, variadics, floating
point, non-`int64_t` integers, function pointers whose argument a header cannot name an export for.

**What the programmer observes, and when.** Compile time: every refusal above, by code and span. Run
time: `N0409` after a call that broke its declaration's null promise; nothing else is checked after
C returns. Target time: `N0613`. The interpreter refuses any program calling C or `c_errno` before
running it (`N0385`, unchanged). **Deterministic**: every compile-time decision, every layout, the
header. **Target-dependent**: `errno`'s location; a C-layout record's layout follows the target's C
ABI, which on every target of the matrix is natural alignment for these field types.

**Authority and provenance.** Unchanged in kind: every foreign call and `c_errno()` needs a
`ForeignCap`; what C returns — a handle, a copied string, an `errno` — carries the `unknown` origin
(N42), and a handle's origin flows through records and `Vec`s as any value's (N80).

**Versions.** Semantic epoch `24 → 25` (new declarations, new accepted types). Interface schema
`nazm.interface/8 → 9`: a record's C-layout and opaque flags are part of what an importer may do with
it. Runtime ABI `11 → 12`: `nz.cstr_len`, `nz.errno_save`, `nz.errno_last`. Core IR, MIR and LIR
schemas unchanged: the crossing is written in existing nodes, plus two internal built-ins
(`foreign_nonnull`, `cstr_copy`) no program can name. The check cache follows the interface.

**Negative example.** `extern "C" struct Db; fn same(a: Db, b: Db) -> Bool ! {} { a == b }` is `N0304`
at the `==`, and `Db {}` anywhere is `N0611`. **Boundary example.** `extern "C" fn find(k: Int) -> Option[Db] = "find";` called when C
returns null is `None`, and the same declaration written `-> Db` fails that call with `N0409` and
status 2, not a crash in the caller's next use.

**Not here.** By-value structs and struct results; unions, bit-fields, arrays; floating point;
integer types other than `int64_t` and `bool`; variadic functions; closures as callbacks (a C
`void *` environment rule); `dlopen`; ABIs other than C; exports taking `Str` or records; out-buffers
C writes into. Each is a refusal with a code, not a silent fallback.

**What would prove this unsuitable.** A C-layout record whose bytes as C reads them differ from its
fields in declaration order on any target of the matrix; a null handle reaching Nazm code; an `errno`
read that sees a value set by the runtime rather than by the foreign call; an export called from a C
thread that crashes for want of runtime state.

**As built (N85, 2026-10-04).** Five amendments. **The syntax is `extern "C" struct`**, for a handle
and a C struct alike — C's own spelling of an incomplete struct, and no new keyword: `extern` stays
contextual, read before a string and `struct`. **A handle is a record of one hidden field**
(`#pointer`, a name no program can write), so it is an ordinary value to every layer below the
checker; `==` on one, and one reaching a task, are refused by the existing derived rules (`N0304`,
`N0321`, `N0390`) answering for a new type rather than by a new code. **A callback's address is
written by Core IR lowering** as an internal built-in on the export's symbol, because a named
function value is a thunk by MIR, which no longer knows the export it wraps. **`errno`'s location is
a per-target unit** (`nz_errno_location`, beside the task switch), so the runtime text stays the same
on every target; the capture is the runtime's `foreign` part, and a freestanding runtime defines a
capture that does nothing. **Cranelift links the `errno` unit always**, its runtime being its whole
baseline. Versions as stated: epoch 25, `nazm.interface/9`, runtime ABI 12; Core IR, MIR and LIR
print schemas unchanged — the crossing is existing nodes and three internal built-ins.

## 7.87 Embedded v2: boards as data, a second architecture, device registers by width — N86, 2026-10-04

Written before the code. Area 18 is PARTIAL because everything it shows is one board: N62 built a
freestanding image for QEMU's AArch64 `virt` machine, N63 bounded its stack, and the board's facts —
where the image loads, where the UART is, how the machine stops — are constants in one runtime file.
A second board would be a second copy of that file. This milestone makes a **board a description**
and adds a second architecture family through it, so that what is specific to a board is data and
what is the language's is not.

**The sole owner.** `nazm_runtime::board` owns the board table: for each freestanding target, its
architecture, load address, stack size, UART (address and access width), how the machine is stopped
(ARM semihosting, or SiFive's test device), the startup's per-architecture instructions, and the
emulator command a person runs it with. The linker script, the freestanding runtime and the entry are
generated from a description and from nothing else; `nazm inspect` prints the table. The language
gains no board knowledge: a program names device addresses as integers under an `MmioCap`, as in
N62, and a board's description never changes what a program means.

**The second architecture.** `riscv64gc-unknown-none-elf` on QEMU's RISC-V `virt` machine: RV64GC,
machine mode, no firmware (`-bios none`), the image loaded at `0x8000_0000`, the NS16550A UART at
`0x1000_0000`, the SiFive test device at `0x10_0000` stopping the machine with a status. Its startup
sets the stack, enables the floating-point unit (code generation may use it) and calls `main` as
AArch64's does; its stack check, stack painting and stack bound are N49's and N63's, unchanged. Only
the LLVM backend builds it, as only it builds AArch64's board; Cranelift is refused by name.

**Device registers by width.** `mmio_read8`, `mmio_write8`, `mmio_read16` and `mmio_write16` join the
32-bit pair: one volatile access of exactly that width, zero-extended on read and truncated on write,
under an `MmioCap`, freestanding only (`N0392` on a hosted build, unchanged). A 16550's registers are
bytes; a 32-bit store to one writes its three neighbours, so a byte-wide device needs the byte access,
and the width is the program's to state. No access is reordered against another (volatile), and none
is merged or split.

**Settled and not built.** *Atomics*: an ordering model needs a memory model the language has not
written (N61's formal core has one thread), and a litmus test on one emulated core shows nothing about
orderings; refused by absence — there is no atomic built-in — and recorded as DESIGNED. *Interrupts*:
a handler is code the hardware calls between any two instructions; it needs the effect and capability
story for preemption and a vector table per board; DESIGNED. *A heap*: the boards have none, and a
program that allocates is refused as N62 refuses it (what only a hosted runtime has). *Static
initialisation* is the linker's: `.data` is loaded with the image and `.bss` is zero in QEMU's loaded
memory; on hardware a loader that does not zero `.bss` is outside this guarantee, written down.
*Panic path*: unchanged — a failure is reported on the UART and the machine stopped with status 2.

**Evidence, and what is claimed.** Emulator runs only: each board boots its image under QEMU, in a
container, and a program's output, failures and measured stack are compared with independent expected
values. No hardware is claimed; a board on the table is "emulated", and the published support matrix
says which of build, link, boot and measured stack each target has.

**Versions.** Semantic epoch `25 → 26` (four names a program could not call before). Runtime ABI
`12 → 13` (four MMIO entries; the board table enters the runtime digest). Interface, Core IR, MIR and
LIR schemas unchanged. A board image's objects are keyed by target as N57 keyed them, so a RISC-V
object is never an AArch64 one.

**Negative example.** `mmio_write8(268435456, 72)` in a program built for the host is `N0392`; a
program allocating a `Vec` built for either board is refused before linking, naming the sequences
runtime. **Boundary example.** On RISC-V, `mmio_write8` of `0x1_48` writes the byte `0x48` (`H`) and
nothing else — the truncation is the width's.

**What would prove this unsuitable.** A board fact needing a change outside its description; a
program whose output differs between the two boards for anything but its device addresses; a stack
measured above its stated bound on either.

**As built (N86, 2026-10-04).** Three amendments. **RISC-V is built where `clang` can build it**:
Apple's `clang` has no RISC-V code generator, so the host refuses the target by name before
compiling (`-print-targets` asked), and the evidence is built and run in `nazm-qemu:n86`, whose
Debian `clang` has one. **The linker script names more than it did**: RISC-V's small-data sections,
and `/DISCARD/` for unwind tables — at `-O2` an orphaned `.eh_frame` was placed at the load address
ahead of `_start`; AArch64's images were unaffected and its script gains the same lines. **The
hosted refusal (`N0392`) names both boards** in its help. Epoch 26 and runtime ABI 13 as stated;
`nazm inspect` gains `toolchain.boards`, additive.

## 7.88 Assurance profiles v3: contracts, obligations and what each one's status is — N87, 2026-10-04

Written before the code. Areas 19 and 20 are PARTIAL: `critical` and `cyber` are restriction rules
over effects, spawning, foreign calls, recursion, allocation and locked builds, and a profile report
names the rule a program broke. Neither can say what a function *promises*, and neither can say, of
the places a program may fail, which are proved not to, which are checked as the program runs, and
which nobody knows. This milestone adds **contracts** to the language and an **obligation report** to
the tooling, and states, for each obligation, which of the three it is. It is profile enforcement and
evidence for a reviewer; it is not certification, and §7.49's disclaimer stands in full.

**Contracts.** A function may state preconditions and postconditions after its effect set:

```nazm
fn isqrt(n: Int) -> Int ! {}
    requires n >= 0
    ensures result >= 0 && result * result <= n
{ … }
```

`requires E` and `ensures E` may each be written any number of times, in that order; each `E` is a
`Bool` expression over the parameters (and, in `ensures`, `result`, the value being returned),
literals, arithmetic, comparison, `&&`, `||`, `!`, and calls of functions that declare `! {}`. Nothing
else: no effect, no capability, no allocation (a contract that could fail for want of memory would
be a contract about memory) — each refused at the clause (`N0614`). `result` outside an `ensures` is an
ordinary name.

**What a contract means.** Every `requires` is evaluated, in order, when the function is entered,
after its arguments; every `ensures` when it returns, on every path, with `result` the returned
value. A clause that is false fails the call — `N0410` for a precondition, `N0411` for a
postcondition — at the clause, through the failure path every other failure takes: status 2 natively,
the same diagnostic from `nazm run`. A clause that itself fails (an overflow inside it) fails the call
with that failure. Contracts are never compiled out: a program with contracts means the same under
every backend, optimisation level and profile.

**What the compiler proves.** One thing, exactly: a call whose arguments are all integer or boolean
literals is checked against the callee's `requires` at compile time by evaluating each clause. False
is `N0615` at the call — it would fail every time it ran; true makes that call's obligation **proved**.
No other call is reasoned about.

**Obligations, and their three statuses.** `nazm check --obligations` prints `nazm.obligations/1`: for
each function, each obligation with its kind, its span and its status —

- **proved**: discharged at compile time, with the witness (a literal call's evaluated clause; an
  operation on two literals whose result the checker computed);
- **checked**: not proved, and checked when the program runs — every contract clause, every
  arithmetic operation that can overflow or divide by zero, every index and every allocation, since
  Nazm checks all of them;
- **unknown**: nothing about it is decided here — a call through a function value, whose callee and
  so whose contract are not known (N50), and every foreign call (C's behaviour, N42).

The kinds: `precondition`, `postcondition`, `call-precondition` (one per call of a function with a
`requires`), `overflow`, `division`, `index`, `allocation`, `indirect-call`, `foreign-call`. The report
is deterministic and states, as its first field, that it is compiler evidence and not certification.

**Profiles.** `critical` gains `contracts-hold`: no `call-precondition` may be refuted (already
`N0615`), and every `unknown` obligation is a witness the profile refuses (`N0510`), so a `critical`
program's obligations are each proved or checked. `cyber` gains `no-unknown-calls` — the same refusal
of indirect and foreign calls — and keeps N47's locked build; constant-time checking stays RESEARCH:
the one subset precise enough to state (no branch, index or division whose operand is derived from a
named secret) needs secrets in the type system, which N80's provenance tracks only to sinks.

**Who owns what.** The parser owns the clauses' syntax; the checker owns their typing and the literal
proof; Core IR lowering owns their meaning, as ordinary Core IR — a clause is a `Bool` expression and a
call of an internal `contract_failed` built-in on its negation — so the interpreter, MIR and both
backends run them with no new node; `nazm-core` owns the obligation census, read off Core IR.

**Versions.** Semantic epoch `26 → 27` (two clause forms, three codes). Interface schema
`nazm.interface/9 → 10`: an export's contracts are part of what a caller's literal proof and
obligation report read. Runtime ABI unchanged: a contract failure is the existing failure path. Core
IR and MIR print schemas unchanged (an internal built-in). `nazm.obligations/1` new, with its schema.

**Negative example.** `isqrt(-4)` is `N0615` at compile time; `isqrt(x)` with `x` negative at run time
is `N0410` at the `requires`. **Boundary example.** `isqrt(0)` is proved and returns `0`, its `ensures`
holding as `0 * 0 <= 0`.

**Not here.** Loop invariants and ranges as declarations, initialisation (every Nazm binding is
initialised by construction), a solver or any proof beyond literal evaluation, separation logic,
constant time, and any claim of absence of runtime error beyond the census's own `proved` entries.

**What would prove this unsuitable.** A contract a backend or the interpreter does not run; a
`proved` obligation that fails at run time; an obligation the census omits — a trap site not in it.

**As built (N87, 2026-10-04).** Four amendments. **The census is its own command**, `nazm
obligations FILE [--json]`, beside `nazm core-ir`, rather than a flag of `nazm check`: it reads Core
IR, which only a program that checks has. **The interface carries a count**, `requires`, not the
clauses: the literal proof stays within a module, and an importer needs only to know a call is an
obligation. **A contract call is `pure` by its signature** — a declared `! {}`, not foreign — so a
callee whose interface came from the cache answers as the same callee checked from source. **The
formatter prints the clauses as the header's**, one level in, the body's `{` back at the function's
level. The census's kinds and statuses are as stated; a `proved` overflow or division is one on
literals the census computed, the only proof it makes beyond the checker's.

## 7.89 Accelerators v2: maps of one or two sequences, reductions, and a numeric oracle — N88, 2026-10-04

Written before the code. Area 21 is PARTIAL: N67's `nazm accel` runs one idiom — a pure
`(Int) -> Int` map over one sequence — on one GPU through one deprecated API. A user who needs two
sequences combined, or a total of what was mapped, cannot say it. This milestone widens what a user
can express to a small compute subset with one meaning everywhere, and holds the device to an
oracle on generated inputs rather than on chosen ones.

**What a kernel is.** As N67, plus one shape: a kernel is a pure function of **one or two** `Int`s
returning an `Int` — `(Int) -> Int`, a *map*, or `(Int, Int) -> Int`, a *zip* — reaching only
functions of the same kind. A zip takes two input sequences of the same length (`--input A --input
B`); different lengths are refused before any device (`N0394`). Eligibility, the checked arithmetic,
the lowest-failing-index rule, the 8,000,000-element cap and the explicit transfers are N67's,
unchanged; a zip's report names both inputs' transfers.

**Reductions.** `--reduce add|min|max` folds the kernel's results, in index order, into one `Int`:
`add` is `ints_sum`'s fold — checked, failing with `N0400` where a running sum would overflow, even
if the total would fit — and `min` and `max` cannot fail (an empty sequence reduces to `None`,
printed as `null`). The fold runs on the host, after the one device-to-host transfer, and the report
says so and times it: its meaning is a sequential fold's, which a device's tree of partial sums would
change for `add`; for `min` and `max` it is a choice of where, stated.

**One meaning, three ways.** Every run is held, unless `--no-check`, to the interpreter applying the
same function to the same elements and folding the same way; a disagreement is a failure of the run,
never a result. **The oracle on generated inputs:** a fuzzer generates pure kernels over `+ - * / %`,
comparisons, `if` and literals at the edges of `Int`, and elements across the range, and compares
the device's results and failure (index and code) with the interpreter's and with an independent
computation in Rust's 128-bit arithmetic.

**Providers.** OpenCL on the Apple M1 Pro's GPU, as N67. A second provider was sought and is not
available on this host — no Metal compiler (`xcrun metal` absent without Xcode), no Vulkan loader —
so Metal and SPIR-V stay designed, and the row says BLOCKED for want of the toolchain, not of a
design. **Vector operations in LIR** — a vector type and operations the backends lower — stay
DESIGNED: the one vectorised loop (N66) is a pattern, and is reported as one.

**Versions.** No language change: semantic epoch, interface, runtime ABI and IR schemas unchanged.
`nazm.accel/1 → /2`: `inputs` (one or two, each with its bytes and time), `reduce` (the operation,
where and how long, its value). `nazm.kernel/1` keeps its form; a zip's kernel has two parameters.

**Negative example.** A zip over inputs of 3 and 4 elements is refused before a device is touched.
**Boundary example.** `--reduce add` over `[i64::MAX, 1, -1]` fails with `N0400` at the second
element, as `ints_sum` does, though the total fits.

**What would prove this unsuitable.** A generated kernel whose device result or failure differs from
the interpreter's; a reduction whose value differs from the interpreter's fold.

**As built (N88, 2026-10-04).** As stated, with two notes. The host program now takes `kernel.cl
output.bin input.bin [input2.bin]`, one buffer per input, and its JSON's `to_device_bytes` counts
both. The numeric oracle is an ignored test of 40 generated kernels of 200 elements each, seeded and
deterministic, run on the M1 Pro: every one agreed with the interpreter and with 128-bit arithmetic,
values and failures alike. No schema file existed for `nazm.accel/1`; `/2` is documented in
`spec.md` and the matrix, not given one.

## 7.90 Workspace editing: renaming what a project exports, and applying a plan stale-safely — N89, 2026-10-04

Written before the code. Area 25 is PARTIAL: rename covers locals and private definitions, and an
exported entity is refused because "a module no open program loads may use it" (§7.20) — so the
one rename a maintainer most needs, of a function a project's other modules call, is the one the
tools cannot do. And a `nazm.patch/1` plan is a proposal nothing applies. This milestone makes a
**workspace** the unit of completeness, renames exported entities within one, and applies a plan
only to the exact bytes it was computed from.

**What a workspace is.** A source root that is *declared* — a `nazm.root` marker, or `--source-root`
— and every `.nz` file under it, found by walking the directory (hidden directories and `target/`
excepted, as the CLI's other walks except them). Every one of those files is analysed as a root, as
`nazm check` would analyse it, over the editor's unsaved buffers where it has them. A file outside
the root, and a package's dependency, are not in it. Without a declared root there is no workspace,
and an exported rename is refused as before.

**Which exported renames are complete.** Within a workspace, every module that can name an exported
entity is a file under the root that imports its module — and every such file is analysed. So an
exported function, record, enum, field, variant or payload field may be renamed when: its declaring
file is under the root; every compilation of the workspace that contains it is clean; and the root is
not a **library package** — a `nazm.toml` with no `main` — whose exported API is what *other*
packages import, which no walk of this root finds. That last case is refused by name
(`Refusal::PackageApi`): a library's API changes by its author's decision, never by a rename.

**The plan.** The entity is found in each compilation by its declaration's place (file and bytes) —
its session-local id means nothing outside one compilation — and its occurrences, across every file
of every compilation that contains it, are the edits: the declaration and every use, each spelled as
the old name, merged across compilations, one `FileEdit` per file, each bound to that file's exact
text and its editor version (or, for a file not open, its disk bytes). The candidate workspace —
every edited file replaced in memory, nothing written — is re-analysed in full: every compilation
that contains an edited file must be clean and partition its occurrences into entities exactly as
before, the edits accounted for. Any difference refuses the whole plan.

**Applying.** `nazm patch apply PLAN.json` applies a `nazm.patch/1` plan to disk, all or nothing: every
file's bytes must hash to the plan's `digest` and hold each edit's `expected` bytes at its place; if
any file differs the command refuses, naming it, and writes nothing. The edits are written through a
temporary file and a rename per file. The language server applies nothing: an editor applies a
rename's edits, and the protocol's edit is offered only when every file it touches is open at the
version the plan was computed from — a closed file's edit goes through `nazm patch`, whose plan binds
its bytes.

**What does not change.** The language, the compiler, every schema but one addition: `nazm.patch/1`
already carries several files; `apply` reads it. No semantic epoch, interface, runtime or IR change.

**Negative example.** In a library package, renaming its `pub fn parse` is refused — other packages'
uses cannot be found. **Boundary example.** Renaming a `pub fn` used by three modules of a workspace
edits all four files; a fifth file edited after planning makes `apply` refuse and write nothing.

**What would prove this unsuitable.** A workspace file that uses the entity and is not edited; a plan
`apply` writes to a file whose bytes differ from the plan's; a rename that changes what any
compilation of the workspace means.

**As built (N89, 2026-10-04).** Two amendments. **A plan names a file outside the requesting
root's compilation by its path from the workspace root** — `nazm.patch/1`'s `path` was always
root-relative, and a workspace rename's other files are under the same root. **`apply` takes
`--base`**, the directory the plan's paths are relative to, the current directory by default, and
refuses any path that leaves it. `nazm.patch/1` gains the reason `package_api`.

## 7.91 Packages v3: a backtracking resolver that explains itself, and `nazm update` — N90, 2026-10-04

Written before the code. Area 26 is PARTIAL. N56's registry chooses each package's highest version
satisfying the requirements it has seen, round by round, and refuses (`N0513`) the first time a
choice leaves some package with no version — even when an older version of the package that made the
demand would have satisfied everyone. "No backtracking" was its stated limit. And a lockfile can only
be written afresh (`nazm lock`); nothing updates one deliberately.

**Resolution, settled.** The registry's packages are chosen by a **backtracking search**: packages in
name order; for each, its published, non-yanked versions highest first — a version the lockfile
records first, while it satisfies — and each candidate kept only if it satisfies every requirement
the current choice places on it; a candidate's own requirements then join the set, and the search
goes on. A dead end undoes the last choice and tries the next candidate. The answer is the first
complete assignment in that order: deterministic, the same for the same registry and manifests, and
the newest a lockfile allows. One version per package name, as before: the language has one module
per path, and two versions of one package would be two modules a program cannot tell apart.

**When there is none.** The search is bounded — at most 10,000 candidates tried — and a failure is
`N0513` with an **explanation**: the package that could not be chosen, every requirement on it at the
deepest point the search reached, and which package (at which version) placed each. A search that
runs out of its bound says so, rather than claiming the registry unsatisfiable.

**`nazm update [NAME…]`.** Re-resolves with the lockfile's preferences dropped — for every registry
package, or only for those named — writes the lockfile, and prints each change, `name old → new`.
Without names it is `nazm lock` with no preference; `nazm build --locked` is unchanged.

**Not here.** Signatures and trust roots (no signing implementation is available offline here, and a
digest is not a signature), a remote registry, pre-release versions and features, multi-root
workspaces, build scripts (prohibited by absence: there is no construct for one). Each is a named
limit, not a silent gap.

**Versions.** No language change. `nazm.lock/1` unchanged: what it records is the same, only how its
versions were found changes. The registry layout is unchanged.

**Negative example.** `a` requires `b ^1` and `c ^1`; every `b 1.x` but `1.0.0` requires `c ^2`: N56
refused; N90 chooses `b 1.0.0`. **Boundary example.** `a` requires `b =2.0.0` and `c` requires
`b ^1`: refused, the explanation naming both requirements and who placed them.

**What would prove this unsuitable.** Two runs over the same inputs choosing differently; a refusal
where an assignment exists within the bound; an `update` that changes a package not named.

**As built (N90, 2026-10-04).** Three precisions. **A requirement placed on a package already chosen
is checked again** at every step, since a later choice can constrain an earlier one; such a dead
end, like a package with no fitting version, is a candidate for the explanation. **`update NAME`
keeps every other package's lockfile preference**: another package changes only when the named one's
new version requires it — the exception the last paragraph's "not named" admits. **An unknown name
is `N0501`**, the code for a package that is not there. A path package's requirements are placed
by `` `name` ``, a registry package's by `` `name` version ``.

## 7.92 Debugger and profiler v3: lexical scopes, Cranelift's DWARF, and sampling — N91, 2026-10-04

Written before the records. Area 27 is PARTIAL: an LLVM `--debug` build has line tables and
`Int`/`Bool`/`Str` variables, but every variable is visible for its whole function; a Cranelift
build refuses `--debug` (BLOCKED in N48 on "no DWARF through the API this backend uses"); and
`nazm profile` counts events, never samples. This milestone gives bindings their blocks, gives
Cranelift line tables, and samples a run — each claim exercised by a real debugger or sampler.

**Lexical scopes (LLVM).** MIR records, for each `let`, the source block it is in
(`LocalDecl.scope`; `None` for a parameter or a binding of the function's own body). The emitter's
variable marker carries that block's first and last positions; `crate::debug` makes one
`DILexicalBlock` per distinct block, nested by containment, gives each variable its block, and gives
each instruction the innermost block containing its position. A debugger shows a branch's binding
inside the branch and not before it. MIR's digest and printed form do not include the scope — it is
display, like a local's name.

**Cranelift's DWARF.** No Cranelift API writes DWARF, but Cranelift carries a source location on
each instruction it is given one for and reports the code ranges each covers. The backend now sets
one per MIR statement and terminator (the LLVM emitter's positions), and writes DWARF 4 with `gimli`
— the version Cranelift itself depends on — into the object: a compile unit, a subprogram per MIR
function, and a line program whose prologue carries the declaration's line and ends
(`prologue_end`) where the first statement's code begins, a gap at line 0. Addresses are
relocations: against the function's symbol in ELF; in Mach-O against its section, with the object
address in place, as `dsymutil` maps addresses through the debug map. **Variables are not
described** under Cranelift: where a value lives is reported only through value labels this backend
does not track. A Cranelift debug object is never taken from the cache: its lines are a function of
the source's text, which no MIR digest is.

**Sampling.** `nazm profile --sample [--interval-ms N] FILE` builds with `--debug`, runs the program
under the host's sampler, and attributes each sample to its innermost frame: a Nazm function and
`.nz` line (line 0 for code no statement made); the runtime (`nz.*` code without a line); foreign
code linked into the program; or the system — each system frame named, and those in a fixed list of
waits (locks, condition variables, sleeps, `kevent`, Mach messages, reads) counted as **blocked**.
The sampler is macOS's `sample`, the one available here; a host without it is refused before anything
is built — never a profile without the samples asked for. `nazm.profile/1 → /2`: the same fields,
and `samples`, `null` unless sampled. Allocations and scheduler events stay the runtime's own counts.

**Owners.** MIR lowering owns where a binding is visible; `nazm-lir::debug` owns LLVM's metadata;
`nazm-codegen-clif::dwarf` owns Cranelift's; the CLI owns running the sampler and attributing its
report. No language, runtime, ABI or schema change but the profile's.

**Versions.** Semantic epoch, interface, runtime ABI, Core IR/MIR/LIR print schemas unchanged (the
scope is display only). `nazm.profile/2`. The Cranelift backend's version string gains ` debug` for
a debug build, and a debug build has no object key.

**Negative example.** Under Cranelift, `info locals` says `No locals.` — not a wrong value. **Boundary
example.** At a function's first statement, a binding of a later branch is not in scope; at the
branch's statement, it is, with its value.

**Not here.** Cranelift variables; records, enums, sequences and closures described; DAP; a sampler
on Linux (no `perf` in the images); timelines and event traces. Area 27 stays PARTIAL.

**What would prove this unsuitable.** A breakpoint by line landing on another line; a variable shown
outside its block; a sample attributed to a function that was not on the stack.

## 7.93 Performance evidence v2: a register of every claim, and a benchmark that says what it measured — N92, 2026-10-04

Written before the code. Area 28 is PARTIAL. `docs/performance.md` holds some seventy measured
sections; a few are regenerated by a command (`cargo xtask bench`, an ignored measurement test), most
were measured once by a method described in prose, and nothing says which is which. A claim nobody
can regenerate is an anecdote with a date. `nazm.bench/1` records a median and a minimum, the host's
name and its CPU count (empty on Linux), and one C and one Python reference.

**The register.** `bench/claims.toml` holds one entry per measured section of `performance.md`, by
its exact heading, with a status:

- **reproducible** — `how` names the command that regenerates the numbers: a `cargo xtask` command
  or a test the tree contains (`cargo test … -- --ignored NAME`), and `metric` what it prints;
- **dated** — a measurement of the implementation as it was on that date, its method in the prose,
  superseded or kept as history: not a current claim about the tree, and said so;
- **unreproduced** — a current claim whose method no command reproduces.

A gate in `cargo xtask check` (**performance claims**) refuses a measured heading with no entry, an
entry whose heading is gone, an unknown status, and a reproducible entry whose test or command does
not exist. The gate checks that a command *exists*, not that its numbers still hold: running every
measurement is the benchmark's job, contained, not a check's.

**The benchmark, v2.** `nazm.bench/2` adds, to every field of `/1`: a machine fingerprint (CPU model,
logical CPUs — read on Linux as on macOS — memory, kernel); for each measurement every run, and p90,
maximum and the median absolute deviation beside the minimum and median; each native executable's
size in bytes; the noise floor — the `floor` program's native median — named as the floor under
which no difference is claimed; and the references that were not measured, each with its reason. A
reference must be the Nazm program statement for statement and say whether its arithmetic is checked:
`sieve.c` (no), `sieve.py` (unbounded), and new — `sieve.rs`, built with `-O` and
`-C overflow-checks=on`, growable and bounds-checked like `Ints` (checked: yes); `sieve.go`, bounds-
checked, wrapping (checked: no), measured where Go is installed. Zig is not installed here and is
recorded as absent.

**Owners.** `xtask` owns the register's gate and the benchmark; `docs/performance.md` owns the
claims' prose; nothing in the compiler changes.

**Versions.** `nazm.bench/1 → /2` (additive). No language, IR, runtime or schema change elsewhere.

**Negative example.** A new `### … — N93, measured …` section without a register entry fails `cargo
xtask check`. **Boundary example.** A dated entry stays valid when the code it measured is gone: it
says it is history.

**Not here.** Running every measurement on every change; energy (no counter is readable here without
privileges); a cross-host comparison, which `--check` still refuses. Area 28 is VERIFIED only if every
current claim — every entry not dated — is reproducible.

**What would prove this unsuitable.** A reproducible entry whose command no longer prints its metric;
a current claim filed as dated to escape the gate.

## 7.94 Coverage-guided fuzzing: a fuzzer that keeps what reaches new code, contained, replayed by the suite — N93, 2026-10-04

Written before the code. Area 30's fuzzing half is PARTIAL: N73's two fuzzers are seeded and blind —
a mutated input is run and thrown away whether or not it reached anything new — and "not
coverage-guided" is the named limit, `cargo fuzz` needing `libfuzzer-sys`, which is not in this
machine's offline crate cache.

**The fuzzer.** `fuzz/` is a crate of its own workspace, as cargo-fuzz's are. Its code under test —
the compiler's crates — is built with LLVM's SanitizerCoverage in `trace-pc-guard` mode, a stable
`rustc` option (`-C passes=sancov-module` with its `llvm-args`); the callbacks that count edges are
C (`fuzz/cov.c`), compiled without instrumentation. An input is **kept** when it reaches an edge no
earlier input reached, and inputs are mutated from what is kept: byte edits, tokens of the language
inserted whole, lines duplicated, deleted, swapped or spliced in from another kept input, and names
renamed everywhere — structure a byte flip rarely makes. Its one `unsafe` is reading the C counters,
outside the workspace whose lint forbids it.

**Targets.** The compiler's entry points, in process: `front` (parse and check: a diagnostic is the
answer to any input, a panic a defect); `lower` (Core IR and MIR for every program the checker
accepts, and **MIR's validator must accept what lowering produced** — validator fuzzing); `run` (the
interpreter under its step and depth budgets); `manifest` (a package manifest and every version
requirement in it). A panic is shrunk by lines and recorded; the campaign fails.

**Budgets and replay.** `cargo xtask contained fuzz [SECONDS] [TARGET…]` runs each target for
SECONDS (300 by default) in the contained runner, seeded from its persistent corpus and the
repository's programs, and exports each target's **minimised** corpus — the smallest set of kept
inputs, chosen greedily, that reaches every edge reached — and a report, one `nazm.fuzz/1` line per
target. The minimised inputs are committed to `fuzz/corpus/TARGET/`, and the ordinary suite replays
every one through the same entry point (`crates/nazm-cli/tests/fuzz_replay.rs`): the evidence a
release carries is that replay, and the campaign that grew it is rerun by one command.

**Who owns what.** `fuzz/` owns the engine and the coverage; `xtask` owns the contained campaign;
the suite owns the replay. The compiler is unchanged; a defect found is fixed where it lives, with
a regression input in the corpus and a mutant.

**Versions.** None change: no language, IR, runtime or schema. `nazm.fuzz/1` is a report line.

**Negative example.** An input that panics the checker fails the campaign and is written, shrunk, to
the report. **Boundary example.** An input the checker refuses is a kept input when its diagnostic
path is new — refusals are coverage too.

**Not here.** Sanitizers (no `unsafe` in the compiler to run them on; ASan on a nightly is not this
toolchain), Miri over the whole suite, Loom (not in the offline cache), backend and FFI differential
fuzzing beyond N73's generator, registry archives, and a contract VM. Area 30's fuzzing half is
VERIFIED for the four targets at the stated budget, not for those.

**What would prove this unsuitable.** A corpus input the replay passes and the campaign's target
panics on; a "minimised" corpus that reaches fewer edges than the kept set.

## 7.95 Reproducibility and provenance v2: a stated model, signed in-toto statements, and an SBOM — N94, 2026-10-04

Written before the code. Area 32 is PARTIAL: N73 writes `nazm.provenance/1` and runs a local command
as an "attestation" whose output Nazm neither signs nor checks; reproducibility was measured, not
stated as a model; there is no SBOM.

**The reproducibility model.** Five claims, each with its own evidence, none implied by another:
1. *Identical input* — the same source, toolchain, target, options and environment give the same
   executable, object, provenance and SBOM bytes, under either backend.
2. *Across directories* — the same, built from two checkouts in different places: no absolute path,
   time or host name enters an artefact Nazm writes.
3. *Archive* — a release archive (N75) is the same bytes for the same revision.
4. *Offline* — a locked package build needs nothing but the local registry and the toolchain.
5. *Across toolchains* — **not** byte identity. Two C compilers are compared and every difference
   classified (N73's measurement: the program's objects equal, the runtime's differing by a symbol
   type at `-O0` and by layout at `-O2`).

**Signed statements.** `nazm attest sign --key KEY PROVENANCE ARTIFACT…` wraps a provenance record in
an **in-toto Statement v1** — `_type` `https://in-toto.io/Statement/v1`, one `subject` per artefact
with its `blake3` digest, `predicateType` `urn:nazm:provenance:1`, the record as `predicate` — after
checking every artefact's digest is one the record names; then signs it with OpenSSH's `ssh-keygen -Y
sign` (the SSHSIG format git signs commits with), namespace `nazm-attestation`. `nazm attest verify
--allowed-signers FILE --identity ID STATEMENT ARTIFACT…` succeeds only when the signature verifies
for that identity, the statement is well formed, and every artefact's digest is a subject's. Keys
and trust are the user's: Nazm holds none, publishes nothing, and logs nothing to a transparency log —
this is Sigstore's statement shape signed offline, not Sigstore.

**SBOM.** `nazm build --sbom FILE` writes a **CycloneDX 1.5** JSON document, deterministic: the
output as the metadata component with its BLAKE3 hash; one component per package (with its version,
`pkg:generic/NAME@VERSION` and BLAKE3 digest), per source module (a `file` component with its hash),
and for the runtime and its ABI revision; the tools — `nazm`, its Cranelift, the C compiler as it
reports itself. Every package of the build is in it, and nothing else.

**Owners.** `nazm-cli`'s `provenance` owns the record and the SBOM; a new `attest` module owns
statements and their verification, through `ssh-keygen`. No compiler semantics change.

**Versions.** No language, IR, runtime or interface change. New output formats: in-toto Statement v1
with predicate `urn:nazm:provenance:1`; CycloneDX 1.5. `nazm.provenance/1` unchanged.

**Negative example.** Verifying with the artefact edited after signing fails, naming the digest; a
statement signed by a key the allowed-signers file does not list for the identity fails.
**Boundary example.** Signing refuses an artefact the provenance does not name, before any key is used.

**Not here.** A transparency log, keyless signing, SLSA levels claimed, rebuilds on several
independent machines (one host and one Linux image are what exist here), and dependency-origin
verification beyond N56's registry digests.

**What would prove this unsuitable.** An artefact that verifies after a byte of it changed; two
builds of identical input with different bytes; an SBOM that omits a package the build linked.

## 7.96 The platform matrix v2: every cell one of three answers — N95, 2026-10-04

Written before the code. Area 33 is PARTIAL: seven targets are accepted, their evidence spread over
N57, N62, N64 and N86 in prose, and nothing holds the documented matrix to the compiler or says what
CPU an object is for.

**The matrix.** One table in `nazm-lir` (`backend::SUPPORT`) gives, for every accepted triple and
each backend, exactly one of **run-verified** (programs built for it ran here — host, Rosetta, the
Linux image, QEMU, Node — and agreed with the interpreter; the evidence named: a test, or a contained
command), **compile-only** (objects built and read from their headers; nothing ran) or
**unsupported** (refused by name, with why). It records each target's CPU identity — the C compiler
driver's default CPU for the triple under LLVM, the baseline ISA under Cranelift, which detects
nothing of the building host — and what a debugger has been shown to do. `nazm inspect` prints it;
the capability matrix's table is held to it cell for cell by a test, which also requires every
accepted target to have a row, every non-refusal to name evidence the tree holds, and each LLVM CPU
to be what the driver resolves. No cell may say "supported".

**Non-goals, decided.** Windows (no linker, SDK or machine here; no runtime port), 32-bit hosted
targets (`Int` is 64-bit; hosted layouts assume 64-bit pointers — wasm32 is the one 32-bit target,
freestanding) and big-endian targets (nothing here runs one; layouts and the DWARF writer assume
little-endian), each with its reason in the same table.

**Versions.** None: no language, IR, ABI or object change; the CPU is recorded and checked, not pinned
by a new flag, so no object's bytes move. `nazm.inspect/1` gains `toolchain.support` and
`toolchain.non_goals`, additive.

**Negative example.** A row claiming x86_64 Linux run-verified fails the test — no x86_64 Linux
machine has run anything. **Boundary example.** A clang whose default CPU for a triple changes fails
the CPU test: the identity is then stale, and the table must change with it.

**Not here.** An x86_64 Linux runner, Windows, CPU-feature selection, sysroots, a bundled linker.

**What would prove this unsuitable.** A run-verified cell whose named test does not run a program for
that target; a target the compiler accepts with no row.

## 7.97 Smart contracts v2: generated sequences on the reference VMs, and what stays blocked — N96, 2026-10-04

Written before the code. Area 34 is PARTIAL: the EVM (N70) and WebAssembly (N71) backends were each held
to the simulator on one written sequence; sBPF execution is BLOCKED (N72); the model has no events,
external calls, assets as types, or gas beyond a bound.

**What this adds.** Generated evidence, not new semantics. Transaction sequences are generated from
the contract's own facts (`nazm.contract/1`: each entrypoint and its arguments) — seeded, the same on
every host — with callers among the owner and two others and arguments from the edges of `Int`. Each
sequence is run by the simulator and by both reference VMs: py-evm (Shanghai, `nazm-evm:n70`) and the
reference WebAssembly host under Node (`nazm-wasm:n64`). Every transaction's outcome and revert code
and the final state must agree; every committed state must keep the conserved quantity; every EVM
transaction must use no more gas than its entrypoint's stated bound.

**What stays as it is, and why.** sBPF: no sBPF toolchain, validator or emulator exists on this
machine — this clang has no BPF target at all, and no Solana tool or crate is installed — so no object
or run is claimed, and the blocker is external and named. Events, external calls, reentrancy, assets
as linear types, metering beyond N70's bound, a chain's own WebAssembly host and storage migrations
beyond `--upgrade-check` are not in the model: adding them is a language change this milestone does
not make. Area 34 stays PARTIAL, and N100 says so.

**Versions.** None change.

**Negative example.** A backend whose division truncated differently from the simulator's would
disagree on a generated `arith` with negative operands. **Boundary example.** `i64::MIN / -1` fails
with `N0400` in all three.

**What would prove this unsuitable.** A generated sequence on which a VM and the simulator disagree
while the written sequences agree — which is the point of generating them.

## 7.98 Formal semantics v2: statements, loops and recursion, bounded and machine-checked — N97, 2026-10-04

Written before the code. N61's formal core is expressions over `Int` and `Bool`; functions, loops and
mutation are "not in the core", and nothing is proved.

**The proof assistant, chosen deliberately: none.** None is installed here — no Coq, Lean, Agda,
Isabelle, Dafny or SMT solver — and nothing can be installed offline. Proofs of progress and
preservation, effect soundness and explicit-flow properties are therefore **BLOCKED**, and nothing
here is called a proof. What extends is the executable model and its bounded, machine-checked
properties.

**Phase A, as a model.** `nazm.formal-core/2` (`docs/formal-core.md` §2, `nazm_formal::statements`):
`let mut`, assignment, `while`, `if` as a statement, `return`, and one recursive function `f(n: Int)`,
over the core's arithmetic and traps. Big-step with **fuel** — each loop turn and call spends one, 400
in all — and running out is no outcome: the semantics declines, since Nazm bounds neither iteration
nor recursion. Properties over **every** program of a stated bound of three families (loops of two
mutable bindings, branches with an early return, recursion built on `f(n − 1)`): S1 soundness —
never stuck; S2 determinism; S3 correspondence — Nazm's interpreter gives the same value or trap for
every program the semantics finishes; S4 the checker accepts every program of the bound.

**Not here.** Records, enums, strings and sequences in the model; Phase B (ownership and
reclamation, effects and capabilities, explicit flows) and Phase C (structured concurrency, Core/MIR
refinement); any property beyond the bound. The whole language is not formally verified, and nothing
says so.

**Versions.** `nazm.formal-core/2` is new and contains `/1`, unchanged. Nothing in the compiler changes.

**Negative example.** A loop that never ends within the fuel — `while 0 < x { x = x + 1; }` from 1 —
is out of fuel, and the interpreter is not asked. **Boundary example.** The same loop from `i64::MAX`
traps `N0400` on its first turn, in the model and the interpreter alike.

**What would prove this unsuitable.** A program of the bound the model and the interpreter answer
differently — which, as N61's first run did, may be an error in either.

## 7.99 The compiler written in Nazm: parity measured, not reached — N98, 2026-10-04

Written before the code. The bootstrap is VERIFIED (C2 = C3, area 31), but the compiler written in
Nazm (`compiler/*.nz`) compiles a subset of the language, and "the bootstrap subset" has been a
standing limitation stated in prose, with no measure of which features it builds.

**The parity record.** `compiler/parity/` holds one probe per feature, named by its dimension —
`syntax`, `types`, `failure`, `modules`, `closures`, `traits`, `effects`, `concurrency`, `contracts`,
`ffi` — each a program the reference compiler builds. A contained test builds every probe with the
reference (`nazm build`, run natively) and with `compiler/emit.nz` (built by the reference, its LLVM
IR linked by clang, run natively), and writes one verdict per probe: **equal** (the same exit status,
output and failure code), **refused** (the compiler written in Nazm does not build it: a gap, with the
code it gives), or **differs** (it builds something that behaves otherwise: a defect, which fails the
test whatever the record says). `compiler/parity.json` (`nazm.parity/1`) is that record, and the test
fails when it is not what the compilers do — so the record cannot go stale.

**What this milestone does not do.** Port the missing features. Closures, traits, effects and
capabilities, concurrency, contracts, foreign declarations and `@std` are each a body of work in a
12,800-line compiler, and the acceptance criterion — the selfhost subset equal to the reference's
native subset — is **not met**. N98 makes the gap measured and gated, which is what an honest N100
needs to read.

**Versions.** None. `nazm.parity/1` is a record.

**Negative example.** A probe the emitter refuses is a refusal, never counted as equal. **Boundary
example.** A failing probe — overflow, division by zero — is equal only when both exit with status 2
and name the same code.

**What would prove this unsuitable.** A probe recorded equal whose two executables behave differently
on another input: probes are single programs, and equality is of those programs only.

## 7.100 The interactive tier v2: the JIT question answered, and the tiers held to one answer — N99, 2026-10-04

Written before the code. N65 verified the REPL, `nazm comptime` (isolated by the authority model and
the interpreter's budgets — no ambient files, network or clock — not an operating-system sandbox) and the reload rule, and left the JIT BLOCKED on the workspace's
`unsafe_code = "forbid"`.

**Whether safe abstractions suffice: they do not.** Running generated machine code in process needs
memory that is writable, then executable (`mmap`/`mprotect`, or `MAP_JIT` with
`pthread_jit_write_protect_np` on this host), and a jump to an address as a function — a raw-pointer
cast to a function pointer. The standard library offers none of this safely, and no crate that wraps
it is in this machine's offline cache (`cranelift-jit`, `region`, `memmap2`: absent). A subprocess is
not a JIT. So the JIT stays **BLOCKED**, by two causes: the forbid lint, and the absent crate.

**What would lift it, designed and not built.** One crate, `nazm-jit-mem`, the only member allowed
`unsafe` (`xtask/src/rules.rs`'s allowlist, the workspace lint relaxed to `deny` in the same change,
§2's rule): it maps a buffer read-write, copies relocated code in, flips it to read-execute (W^X —
never both), and returns a handle whose call goes through one audited `unsafe` cast. Threat model:
the code is the compiler's own output from validated LIR, never input bytes; the handle owns the
mapping and unmaps on drop, so no call outlives its code; a stale-code test after recompilation and
a W^X test are its acceptance. Lifting the lint is the project's decision, not a milestone's.

**What this milestone adds.** The tiers that exist held to one answer: a REPL session's every value
and failure code equal to `nazm run`'s and to native builds' by LLVM and by Cranelift, on the same
definitions — recursion, loops, both trap codes, `i64::MIN / -1` and `% -1`.

**Versions.** None.

**Negative example.** An overflow inside a function the REPL defined fails with `N0400` in all four
tiers. **Boundary example.** `i64::MIN % -1` is 0 in all four.

**What would prove this unsuitable.** A REPL answer no other tier gives.

## 7.101 The grand audit: every area against its evidence, and one candidate through every gate — N100, 2026-10-04

Written before the gate runs. N76–N99 each moved or narrowed one area. This milestone adds no
feature. It audits every row of the capability matrix — `area | status | remaining limitation |
owner milestone | executed evidence | proposed final status` (`docs/audit-n100.md`) — and runs one
candidate, the head of N99's records, through the gate the programme deferred to its end: the full
workspace suite, the selfhost suite and the bootstrap, contained; the mutation campaign over every
mutant N76–N99 added or repointed (215, every verdict class reported); and the container-backed runs
— the live debugger sessions, both boards under QEMU, WebAssembly, the EVM and the contract
properties.

**The objective, and its answer stated in advance.** The ideal is no row PARTIAL, RESEARCH, BLOCKED
or MISSING. It is not met, and the audit will not force it: fifteen rows are PARTIAL with named,
executed limits — among them sBPF and the JIT BLOCKED by what this machine lacks, proofs BLOCKED by
the absence of a proof assistant, the selfhost compiler at 13 of 21 parity probes, eight performance
claims unreproduced. N100 therefore **fails the zero-partial objective** and says so; the candidate is
a release candidate for what the matrix states, with a human procedure, and nothing is tagged,
pushed or published.

**What would prove the audit wrong.** A row whose stated evidence a reader cannot find or rerun; a
gate step reported passed that did not run.

## 7.102 Evidence closure: the N100 survivor, the four without a verdict, and a catalogue with no silent tier — N101, 2026-10-04

Written before the code. N100 ended with one mutant SURVIVED (`n79-a-held-io-cap-is-read-alone`,
called "possibly equivalent") and four with no verdict (`every-definition-is-exported`,
`a-record-drop-does-not-recurse`, `task-safety-stops-at-the-first-level`,
`lowered-propagation-falls-through-on-error`): suite-tier entries, no declared killer, each needing a
whole workspace suite per run. Owner: the mutation catalogue (`xtask/mutations/mutations.toml`) and
its validation (`xtask/src/plan.rs`).

**The survivor is reachable.** The mutant reads a held `IoCap` without what it implies, so a
declared function holding only an `IoCap` that calls `print` is refused (`N0369`, no `OutCap`). N100's
probe of exactly that program reported success because it ran in a directory holding a warm check
cache: the cache's key is the checker identity — semantic epoch, package version, schemas, the
built-in table — and a mutant changes the checker without changing any of these, so a warm entry
answered for the unmutated checker. That key is right for a released build, whose rules change only
with the epoch, a counter bumped by hand (§7.4); it is not evidence about a mutated build. Mutation probes and killers run in
fresh directories. The focused killer is `authority::a_held_io_cap_authorises_its_own_print`.

**The four get focused killers**, each a test whose failure under the mutant was observed:
`build::the_same_name_defined_privately_in_two_files_is_not_a_conflict`,
`memory::a_nested_record_releases_through_both_levels`,
`scheduler::nothing_mutable_crosses_into_a_task`,
`propagation::locals_are_released_once_when_an_error_propagates`.

**No silent tier.** The other 175 catalogue entries without a declared killer are the same problem
waiting for N108, whose full-catalogue campaign would spend a whole suite on each (about 50 hours).
Each gets a killer found by applying it and running its crate's tests, then `nazm-cli`'s, in a
dedicated worktree, and a declared killer is verified by the contained runner. An entry no test
catches is recorded with a `no_killer` reason and its classification — a known survivor, or not
injectable — never left unclassified. The catalogue's validation then refuses an entry with neither
killers nor a `no_killer` reason.

**Versions.** None: no semantic epoch, schema, ABI or cache key changes; the catalogue gains killer
metadata only.

**Negative example.** An entry with no `killers` and no `no_killer` fails `cargo xtask check`.
**Boundary example.** An entry whose `find` text no longer occurs is NOT INJECTED: its definition is
repaired to the code that now owns the rule, never dropped.

**What would prove this wrong.** A declared killer that passes with its mutant applied, in a fresh
directory, under the contained runner.

## 7.103 The compiler written in Nazm at parity: 21 of 21, and what each dimension means — N102, 2026-10-04

Written with the port rather than before it: the constitution below states what the work
established, and it was settled probe by probe as each refusal was replaced. N98 measured the
compiler written in Nazm (`compiler/*.nz`) against the reference on 21 probes: 13 equal, 8 refused.
Owner: the compiler written in Nazm, held by `crates/nazm-cli/tests/selfhost.rs` and
`compiler/parity.json`.

**What was ported, each as the reference has it and none for a probe.**

| Feature | Lexer / parser | Checker | Emitter |
| --- | --- | --- | --- |
| Effect sets `! { … }` | `!`, the set on the function's text | `N0366`–`N0368`, inferred in rounds as `effects.rs` does; the bridge's assumption for another module's undeclared function | nothing |
| Capabilities | the seven type names | types; `IoCap` accepted where `OutCap` is expected; `N0371` for a parameter of `main` that is none; no equality | an `i64` nothing reads; `main` given one per parameter |
| Recursion | — | — | `nz.stack_check` on entry to every function in a call cycle or calling through a value, failing `N0408` at the function's name — the reference runtime's, derived and gated (`check-runtime`) |
| Contracts | `requires`/`ensures` as children of the function: parameters, `requires`, body, `ensures` | a `Bool` (`N0300`); `N0614` for what a clause may not contain; `result` in a slot of the function's own | `N0410` on entry; every exit stores into `result`'s slot and leaves through one block, `nz.ens`, where `N0411` is checked before the frame goes |
| Foreign declarations | `extern "C" fn … = "symbol";` | no body; contract `foreign` | the C symbol declared, and a function under the definition's own symbol calling it; `Int` only |
| Standard modules | `@std/NAME` keyed as itself, read from the library beside the core prelude | — | — |
| Traits | `trait`, `impl`, `recv.m(…)`, `Trait.m(x)`; a method named `#Trait.Type.m`, in no namespace a call by name searches | `N0600`–`N0607`, `N0609`, `N0364` for a bound; a concrete call resolved to the impl's definition | a call through a bound resolved per instance |
| Closures and function values | `fn(T) -> R ! {…}` types; closure literals | captures through every enclosing closure; `N0386`, `N0387`; effects checked per closure; effect subsumption where a value is passed | the reference's objects (`nz.closure_new`, counted), captures retained into them, code taking the object first, thunks for named functions, the closure runtime derived and gated |
| `&&`, `\|\|`, `!` | the reference's binding powers | `N0304` | short-circuit branches and a `phi` |

**Parity, dimension by dimension.** Parse: the selfhost parser's tree is compared with the reference's on
every source in the tree, as before. Resolution: unchanged rules, plus trait and method resolution
compared in `the_nazm_checker_agrees_with_the_reference_on_the_n102_features` (39 sources, every code
and span). Checking: the same test, plus every earlier one; **authority is not checked** by the
compiler written in Nazm — no `N0369` — so it accepts a program whose function exercises authority
it does not hold, which the reference refuses (N104 removes the bridge that makes that rule
complicated; the port is its natural successor). Effects: as the reference, including the bridge's
assumption. Code generation and runtime ABI: the same failure codes, the same memory report, closure
counters included, on every probe and on
`the_nazm_written_compiler_compiles_the_n102_features_with_the_same_memory_semantics` (16 programs and
4 failures, on stated answers). Packages: refused by name (`NAME:path`). Profiles: none, as before.
Diagnostics: codes and primary spans compared; message text is not.

**What still differs, outside the parity record's subset, each refused by name or recorded:**
string escapes, `::`, `as`, effect parameters, a function type inside another type, foreign types
other than `Int` and foreign exports, `Chan[T]`, `select`, the clock, device registers, `N0615`
(a literal call proved against a precondition at compile time — checked at run time instead),
reserved words, and authority. The release-native subset the parity record defines is its 21 probes'
features.

**Bootstrap.** C2 = C3 (`8cc750c6…`) at 16,030 source lines, 34 conformance cases (14 multi-module)
and 29 refusals agreeing across C2, C3 and the reference — the new features among them. Self-reproduction,
not correctness.

**Cost.** The source grew 27% (12,767 → 16,200 lines across `compiler/*.nz`); `nazm check` of it cold
1.05 → 1.36 s and 50 → 75 MB; `nazm build` 2.39 → 2.66 s and 82 → 106 MB.

**Versions.** Semantic epoch, interface schema, runtime ABI, Core/MIR/LIR and machine schemas:
unchanged — no rule of the language changed; the compiler written in Nazm caught up with it. `nazm.parity/1`
unchanged; its record moved to 21 equal.

**Negative example.** A trait method whose impl declares `! { io }` where the trait declares `! {}` is
`N0603` in both checkers. **Boundary example.** `false && boom()` never calls `boom` in either
compiler's program.

**What would prove this wrong.** A program both compilers accept whose two executables differ in
status, output, failure code or memory report.


## 7.104 Re-exports: one definition under another module's names — N103, 2026-10-04

Written with the code. Area 2's standing limitation was that a module could neither re-export
a name nor export one under another name. Owner: `nazm-syntax` (the grammar), `nazm-sema` (the
graph's edge and the interface), `nazm-core` (applying re-exports), `nazm-iface` (persisting
them), `nazm-service` (rename) and the compiler written in Nazm.

**The syntax.** `pub use "PATH";` re-exports every name the module at `PATH` offers;
`pub use "PATH" { a, b as c };` the names chosen, `c` the name `b` is offered under. A
`pub use` is also an ordinary `use`: the module sees what it re-exports. `pub use "PATH" as m;`
is refused — a re-export is not imported under a name.

**The semantics: one definition, several names.** A re-export adds no definition. The export it
adds carries the defining module's id, so a call, a type, a trait bound, a reference, a rename and
a persisted interface all answer with the definition — `shapes.nz::fn double`, whichever module's
name was written. What a module offers is its own `pub` definitions and what it re-exports;
what an import brings is what its module offers, unchanged in every other respect: not
transitive beyond what is re-exported, collisions refused as before (`N0207`, `N0208`). A trait
is re-exported under its own name only; its name is how bounds and calls find it. An impl in a
module re-exported from is usable wherever the re-exporting module is imported, and is carried in
its interface.

**Refusals.** A chosen name the module does not offer: `N0211`, at the name. A name the module
already offers — its own, or another re-export's: `N0212`. Modules re-exporting from each other
in a cycle: `N0213`, at each `pub` — a cycle of re-exports offers nothing in particular, and
applying re-exports in dependency order is what keeps the answer deterministic.

**Durability.** `nazm.interface/11`: a re-export's export carries `reexport` and the defining
module's `DefKey`; a type offered under another name, or by another module, is listed in
`reexported_types`. An importer's check key covers the re-exporting module's interface, and so
every signature it re-exports: a re-exported signature's change re-checks the importer; a
private body's change does not. Resolved units name the definition. `nazm.api-doc/2` documents
each re-export with the identity it offers.

**One resolution model.** `nazm_core::declared_interfaces` is the stages a compilation runs before
any body, re-exports included; `nazm interface` publishes from it rather than from a shorter
second path, which until N103 it did.

**The compiler written in Nazm.** Its parser accepts `pub use`; its checker reads the re-exports
from the tokens and the module graph, computes what each module offers — functions, types,
traits — in the same order, and reports `N0211`–`N0213` at the reference's positions; impls
follow re-exports as the reference's do.

**Versions.** Semantic epoch 27 → 28 (programs that did not parse are accepted).
`nazm.interface/10` → `/11`; `nazm.api-doc/1` → `/2`. Runtime ABI, Core/MIR/LIR: unchanged — a
re-export is resolved before any of them exists. Check and object keys: through the interface
schema and the epoch, both moved; no other key input.

**Negative example.** `pub use "shapes.nz" { hidden };` where `hidden` is private: `N0211`.
**Boundary example.** A rename of `double` re-exported as `twice`: the list's `double` and
every use written `double` are renamed; every use written `twice` is not, and still means it.

**What would prove this wrong.** Two identities for one definition in any output: a reference,
a resolved unit, a persisted interface or a symbol.


## 7.105 Authority is a parameter: no inheritance — N104, 2026-10-05

Written before the code. N37's compatibility bridge let a function with no declared effect set
exercise its **caller's** authority; N79 recorded what each such function inherits (its
`inherits`), published it, and offered a profile rule, `explicit-authority`, that refused it — but
the default stayed: `fn main() -> Int { print("x"); 0 }` was accepted, and so was a helper that
prints without holding anything. Area 6 is PARTIAL for that reason. Owner: the authority check
(`crates/nazm-core/src/effects.rs`), with every program in the repository and the compiler
written in Nazm as its migration.

**The rule.** A capability is a value, and authority is what a function's scope holds lexically:
its capability parameters, a binding of capability type, a capture of one. Every call that needs
authority — a built-in whose identity needs a kind, a `spawn`, a foreign call — needs it held
**there**, in every function, declared or not. A call to a function needs nothing at the call:
whatever its body exercises it holds itself, so its authority is in its parameter list and type
checking has already made the caller supply it. Missing authority is `N0369`, as it was for a
declared function, with the function and the kind named. `main`'s parameters are the root: the
runtime hands `main` one capability per parameter, and nothing else is a source.

**What goes.** The bridge's authority half, entirely: `Needs::Ambient`, the inherited-authority
record in the resolution, and its report in `nazm inspect`. A
function value of an undeclared function no longer needs its maker to hold anything: the
function holds what it needs. The profile rule `explicit-authority` is now the language's rule;
it stays named, so a manifest that lists it still parses, and can no longer fire.

**What stays.** The bridge's **effect** half: a function in another module that declares no
effect set is assumed to have the bridged effects (`io`, `spawn`), because its interface publishes
no set. That is an effect contract, conservative and representable, and it grants nothing:
authority is now always a parameter. Attenuation (`OutCap`), subsumption and the capability kinds
are unchanged; no kind is added.

**Migration.** Every program in the repository — examples, the standard library, the corpora,
every program inside a test, the compiler written in Nazm — gains the capability parameters its
functions exercise, threaded from `main`, by a migrator that reads the pre-N104 checker's
`inherits` (the exact set each function exercised without holding) and adds `io: IoCap`,
`out: OutCap` or `tasks: SpawnCap` where needed, and the caller's own at each call. The
compiler written in Nazm also gains the check itself: `N0369` at the reference's spans, for a
built-in or a `spawn` with no capability of the kind in scope.

**Versions.** Semantic epoch 28 → 29: programs accepted before are refused. `nazm.interface/11`:
unchanged — an interface never carried inherited authority (an importer assumed the bridged
effects of an undeclared export, and still does), and a function's authority is its parameters'
types, which it already publishes. `nazm.inspect/1 → /2`: a definition no longer reports
`inherits`. Runtime ABI,
Core/MIR/LIR: unchanged — authority is a check, and a capability's representation is untouched.
Check and object keys: through the epoch and the interface schema.

**Negative example.** `fn shout() -> Int { print("!"); 0 } fn main(io: IoCap) -> Int { shout() }`:
`N0369` in `shout`, which holds no `OutCap`; `main`'s `io` is not reachable from it.
**Boundary example.** `fn shout(out: OutCap) -> Int { print("!"); 0 } fn main(io: IoCap) -> Int
{ shout(io) }` is accepted: an `IoCap` is given where an `OutCap` is expected.

**What would prove this wrong.** A program in which a function exercises a capability kind that
no binding in its scope holds, accepted by either compiler.

## 8. Toolchain, and one real incompatibility

Availability is not compatibility. Declared MSRVs checked 2026-09-19:

| Crate | Version | MSRV | vs local Rust 1.93.1 |
| --- | --- | --- | --- |
| `cranelift` | 0.135.2 | **1.95.0** | **fails** |
| `cranelift` | 0.132.3 | 1.93.0 | ok |
| `redb` | 4.3.0 | 1.90 | ok |
| `salsa` | 0.28.4 | 1.88 | ok |
| `cstree` | 0.14.0 | 1.85 | ok — added in N15 against 1.98.1, resolved and built (§7.17) |

The machine's `stable` was **1.93.1 (2026-02-11)** while current stable was **1.98.1
(2026-09-01)** — seven months behind. Resolved by pinning 1.98.1 rather than holding
Cranelift at 0.132. **This establishes declared MSRVs, not that the set resolves and builds
together**; that check happens when each dependency is actually added.

Choices for the compiler, with the corrections an external review forced:

| Need | Pick | Reasoning |
| --- | --- | --- |
| Lexer | Handwritten | Incremental relexing, trivia attachment, interpolation, recovery |
| Parser | Handwritten recursive descent + Pratt | Justified by *Nazm's* requirements — incremental reparse of a changed subtree, per-construct diagnostics, control of trivia — **not** by disparaging alternatives. An earlier draft dismissed `chumsky` for "poor recovery"; that was wrong, `chumsky` supports recovery and partial ASTs via `recover_with`. rust-analyzer, Roslyn, Swift, and TypeScript chose handwritten RD for the incremental-IDE reasons above |
| CST | `cstree` over `rowan` | An earlier draft claimed `rowan` blocks parallelism. Also wrong: `rowan`'s `GreenNode` **is** `Send + Sync`; only the red `SyntaxNode` wrapper is not, and that is architectable around. The real reasons are interned strings in the green tree and a thread-safe red layer out of the box. A preference, not a blocker |
| Incremental | `salsa` in memory + own content-addressed store on disk | rust-analyzer is the existence proof; salsa's persistence is weak and its API churns |
| Dev backend / JIT / comptime | `cranelift` | One component, four jobs |
| Release backend | LLVM IR as text → `clang`/`opt` | No build-time LLVM dependency |
| Hashing / serialisation / store | `blake3` · `postcard` (disk) + `serde_json` (agent/LSP) · `redb` | Keeps the bootstrap chain free of C dependencies |
| Diagnostics | `ariadne` to render; **own structured schema** | The renderer is cosmetic. Stable codes, JSON, machine-applicable fixits are the artifact |
| LSP / MCP | `lsp-server` (not `tower-lsp` — cancellation control) · `rmcp` | |
| Testing | `insta` · `tests/ui/` golden tests · `proptest` + `arbitrary` + `libfuzzer-sys` | Golden UI tests are the highest-value compiler test investment |

## 7.106 One LIR both backends consume — N105, 2026-10-05

Written before the code. Area 10 is PARTIAL because LIR owns bytes and nothing else: since N40 each
native backend translates MIR itself. An inventory at N105's start found three decisions made once
— byte layouts and the ownership predicate (`abi.rs`), symbols and linkage (`symbol.rs`), the
runtime's functions — and everything else made twice, in `nazm-lir/src/emit.rs` (LLVM text) and in
`nazm-codegen-clif` (Cranelift): every guard, its order, failure code and message; 33 of the 57
built-ins' semantics; which runtime function retains or releases each type; the record, enum,
closure and task-block layouts the code actually uses (LLVM's enum was one slot per variant, the
tables' a tagged union); the internal calling convention; literals. The two agreed because tests
compared them, not because they were one. Owner: `nazm-lir`.

**The level.** LIR is an instruction set below MIR and above either backend, target-shaped but
backend-neutral, in `nazm-lir/src/op/`:

- *Values* are typed by a machine class — `I8`, `I32`, `I64`, `Ptr` — and are defined once and used
  only in the block that defines them; a value that crosses blocks lives in a *variable* (a mutable
  scalar) or a *slot* (stack memory of a stated size and alignment).
- *Instructions*: constants and symbol addresses; slot addresses; loads and stores at a byte offset;
  wrapping and overflow-checked arithmetic; comparisons; selects; extensions and truncations;
  pointer offsets; variable reads and writes; direct, indirect and runtime calls; memory copy and
  zero of a constant size; a source position for debug information.
- *Terminators*: jump, branch, switch on an integer, return, and `unreachable`.
- A *module* is one unit: its functions (MIR's, and every helper, trampoline, closure release and C
  entry they need), the data they reference (literals, failure texts), what they import, and which
  parts of the runtime they reach.

**What the lowering from MIR decides, once.** Every failure: code, message and its
`file:line:col` text. Every guard and its order (division by zero before overflow; `MIN % -1` is 0).
Comparisons, string equality and the equality helpers. Which runtime entry each operation calls —
only the runtime's word-level view, the same symbol for both backends. Every retain and release, and
the helpers that do them. Every offset — records, the enum's tag word and one payload area, closure
objects, task blocks, `--layout soa` — from `abi.rs`, which gains the closure and task layouts. The
calling convention: a value held in memory by pointer, an in-memory result through a pointer passed
first; a foreign function's C classes and extensions.

**What a backend decides.** Instruction selection, registers, the object or text format, and how a
position becomes debug information. Nothing it decides is observable to a program. The LLVM backend
becomes a printer of LIR as LLVM text; the Cranelift backend a translator of LIR into Cranelift IR.
Neither reads MIR: a layering rule refuses the dependency.

**Validation and an oracle.** A validator holds every module to the rules above — classes agree,
a value is defined before it is used in its block, every symbol and slot and block exists, every
call matches its callee's signature — before either backend sees it, and a broken module is
`N0900`. An interpreter of LIR executes it against a model of the runtime's services, for the
sequential subset: tasks, channels, foreign calls, the clock and device registers are refused by
name. It is the oracle that the three executions are held to — LLVM `-O0` and `-O2` and Cranelift
against the LIR interpreter — over the N73 generator's programs and a new fuzz target.

**Keys.** A Cranelift object is a function of its unit's LIR (which carries every failure text, and
so every position), the translator's revision, Cranelift's version and settings, the target and the
runtime; until N105 it omitted positions and reused an object reporting a moved failure at its old
line. An LLVM object stays keyed by its text and clang.

**Versions.** Semantic epoch and interface schema: unchanged — the language does not move. Runtime
ABI: unchanged unless a word-level entry is added, and then a revision. `nazm.lir/1 → /2`: the
document gains each unit's instructions. Every object key moves, since the code does.

**Non-goals.** No optimisation in LIR: clang's and Cranelift's. The GPU kernels (N88) and contract
bytecode (N70) are other targets with their own generators, not native backends.

**What would prove this wrong.** A failure code or message, a guard, an offset, a runtime entry or a
retain/release decision found in a backend rather than in LIR; or a program whose observable
behaviour differs between the LIR interpreter, LLVM and Cranelift.

**As built (N105, 2026-10-05).** The level is as written above, with three differences, each now the
record:

- *Values cross blocks under dominance.* A value is used where its definition dominates the use, not
  only in its own block: a checked operation's result is used in the block after the guard that
  tests its overflow bit (`v3, v4 = mul.ovf v1, v2; branch v4, b3, b4` and `b4: return v3`), and
  a promoted temporary (N59) is read past the guards its expression made. The validator checks
  dominance (Cooper, Harvey and Kennedy's immediate dominators over reverse postorder). Its first
  form was the cubic bit-vector iteration, which made the debug build of `compiler/emit.nz` spend
  21 s of its 21 s validating; the contained selfhost suite went from 84 s (N104) to 236 s, and to 83 s
  once replaced. A release build of `compiler/emit.nz` takes the same time as N104's (1.47 s and
  1.50 s); the executable it makes runs the same (20.7 s and 21.0 s compiling the compiler, the
  same bytes out) and is 17% smaller.
- *`nazm.lir/2` carries text.* Each unit's `lir` is the module as `nazm lir --ops` prints it and its
  BLAKE3 digest, not a structured form: the text is the stable, reviewable statement of the
  instructions, and the digest is what keys a Cranelift object.
- *`--layout soa` is both backends'.* N68's layout was LLVM's and refused on Cranelift; it is LIR's
  now (`op/soa.rs`), so Cranelift takes it, and its object for one layout is never reused for the
  other because the LIR differs.

What the inventory deleted: `nazm-lir/src/emit.rs` (4,405 lines) and Cranelift's `func.rs`,
`prims.rs`, `helpers.rs`, `unit.rs` and `layout.rs` (2,963; their deletion was committed by mistake
in the N104 gate fix `388d11b`, which does not build on its own). What replaced them: `op/` (5,982
lines, the interpreter's 960 and the validator's tests included) and `translate.rs` (645). Of the
mutation catalogue's 93 entries on the deleted files, 76 were repointed at the LIR line that now
carries their rule, 16 retired as one of an LLVM/Cranelift pair that is now one rule, one
(`n91-a-debug-object-is-reused`) retired as equivalent, and N68's `n68-cranelift-takes-the-layout-silently` retired with the refusal it attacked: a debug build's LIR carries every position,
so the key moves when a line does. Eleven of the 76 survived their old killers, which compared the
two backends — and two backends translating one LIR agree on its defects. Each has a killer with an
independent expectation now. Eight new mutants attack what N105 added (`n105-*`).

**Versions, decided.** Semantic epoch 29 and `nazm.interface/11`: unchanged, the language did not
move. Runtime ABI: unchanged — no runtime entry was added or changed; generated code reaches only the
word-level registry it reached before. `nazm.lir/1 → /2`. Core IR and MIR: unchanged. Cranelift
object key: the unit's LIR digest, `TRANSLATOR_REVISION` 1, Cranelift's version and settings, the
target and the runtime digest (was MIR without positions). LLVM object key: unchanged in form (its
text and clang), every key moved because the text did. Package, lock and registry: unchanged.

**The failure bit (R1, 5b.8, 2026-10-14).** A caller used to learn that a Nazm callee failed by
reading the thread's failure flag after every call — a load, on the path of every recursive call.
Now a Nazm function (`Conv::Nazm`: MIR's functions, and a large frame's entry) returns one more
thing than its signature says, a bit beside its result: false on every `return`, true on its
failure exit, the new terminator `fail` (MIR's `Unwind`). A call of one takes the bit
(`v1, failed v2 = call @f(…)`) and branches on it to the statement's failure edge; a call through
a function value does the same, since every function value's code is a Nazm function. The flag is
untouched: `nz.fail` sets it and the message as before, it stays the runtime's record of the
failure, and everything that is not a Nazm caller reads it — a runtime entry's caller
(`nz.chan_recv`), a task's trampoline, an export's C entry, the platform entry. The runtime's
entries and the generated helpers take the new `Conv::Plain`: the internal convention without the
bit. A function the platform calls by its symbol — `main`, an interrupt handler, a board's task or
failure hook (`Sig::entered`) — keeps that symbol as a plain entry with its old signature, which
calls the function under `<symbol>.body` and leaves the bit; every Nazm caller, in any unit, calls
the `.body`. The LLVM backend returns `{ T, i1 }` (`i1` alone for no result), which arm64 and x86_64
return in two registers; Cranelift a second result (`enable_multi_ret_implicit_sret` for a pair that
needs more registers than a target returns in); the oracle a pair. The WebAssembly contract adapter,
which calls a contract's functions by symbol, takes the pair and reads the flag. Both backends also
lay out the blocks only a failure reaches as cold (`Function::cold_blocks`: LLVM branch weights,
Cranelift cold blocks) — a layout hint, nothing a program observes. Measured on `fib(35)` (Apple M1
Pro, hyperfine median of 20): LLVM `-O2` 66.8 → 55.4 ms, Cranelift 106.3 → 83.8 ms. The bit alone
was slower under LLVM (82 ms): clang merges the two returns, so the path that did not fail set the
bit and jumped to the shared return; the weights put that return in line, and with them the bit
measured 56.2 ms against the flag's 57.6 on this program (hand-built from the two IR texts) — most
of the gain is the layout, not the register. Versions: semantic epoch, interface and
runtime ABI unchanged — no runtime entry or signature changed; `TRANSLATOR_REVISION` 4; every
object key moves because the LIR text does.

## 7.107 The pool by default, and two regressions attributed — N106, 2026-10-05

Written before the code. Area 13 is PARTIAL with *one OS thread per task by default*: N83 built an
M:N pool — stackful tasks on guarded stacks, pinned to bounded workers, parked at the four
suspension points (`nz.cwait`, `nz.ctimedwait`, `nz.cwake`, `nz.join_one`) — and verified it, but
only behind `NAZM_SCHEDULER=pool`. Area 28 names two regressions since N75 without causes inside
the milestones that caused them. Owner: `nazm-runtime` (the scheduler's decision), `nazm-cli` (the
build's policy and its report).

**The default.** On every hosted target the pool has a switch for — AArch64 and x86-64, macOS and
Linux — a native program's tasks run on the pool unless the environment says
`NAZM_SCHEDULER=threads`. `NAZM_SCHEDULER=pool` asks for it explicitly, as before. Any other value
is refused rather than read as one or the other: no task starts, and the first `spawn` fails as a
pool that could not start does (`N0404`, *this task could not be started*). A pool that cannot start
— its tables or workers not allocated — is the same refusal, never a quiet return to threads.
Freestanding and WebAssembly targets have no tasks and are unchanged.

**The one policy exception: C.** A program that declares a foreign function runs a thread per task
unless `NAZM_SCHEDULER=pool` asks otherwise. A task blocked in C holds its worker and every task
pinned to it, and there is no hand-off of a blocking foreign call (§7.84's non-goal); a program
whose tasks wait on each other through C would deadlock on the pool and does not on threads. The
build decides it from the program, the entry records it, and `nazm explain-cost` says which
scheduler a program gets and why (`nazm.cost/1 → /2`, a `scheduler` object).

**What is kept.** Scopes, joins, cancellation, failure adoption, channels, `select`, deadlines and
cleanup are the pool's as N83 verified them, now on the default path; the thread model stays, by
name. **A deterministic test mode:** `NAZM_WORKERS=1` runs every task on one worker, first in, first
out, each until it suspends — the same interleaving on every run of a program that reads no clock.
**The stack:** a pool task's stack is `NAZM_TASK_STACK` (default 256 KiB, at least four pages, a
guard page below), where a task thread's is 8 MiB; the N49 stack check budgets each from its own
stack, so a deep recursion in a task meets `N0408` sooner on the pool and is reported the same way.
`docs/spec.md`'s N49 table says so. The interpreter keeps a thread per task (it is Rust); the
compiler written in Nazm emits the thread model's runtime text (the `runtime parity` gate holds it
to the Rust text), so its programs keep threads — recorded as a gap, not a semantic difference.

**The regressions.** Attributed before anything is optimised, by builds of the milestones' own
commits on one host and by sampling, then confirmed contained. Only a cause that is measured may be
changed, and each change is measured against the same baseline; a cost that is the price of a
feature is stated and accepted, not hidden. A benchmark claim is added for each cause found
(`bench/claims.toml`), so it is held from now on.

**Measured, and published in `performance.md`:** logical tasks against OS threads (the pool's
`peak` against its workers), resident bytes per task with the page size, spawn and join, a channel
round trip, and a blocked receive woken — on the pool and on threads, from the same programs.

**Versions.** Semantic epoch 29: unchanged — no program means anything else. Runtime ABI revision
5 → 6: the default and the policy global the entry defines. `nazm.cost/1 → /2`. Interface, MIR and
LIR schemas unchanged. Every object key moves with the runtime's revision and digest.

**Non-goals.** Work stealing, preemption, growable stacks, an event reactor, the blocking-C hand-off,
the pool in the compiler written in Nazm. **What would prove this wrong:** a program that is correct
on threads and deadlocks, fails or answers differently on the default; the suite not passing on the
default; a `NAZM_SCHEDULER` value accepted as something it does not say.

**As built (N106, 2026-10-05).** As written, with three corrections, each now the record:

- *The revision was 13, not 5:* the constitution read an old count; the runtime ABI moves 13 → 14.
- *The policy is a runtime global, not a weak one.* The first form declared `nz.pool_policy`
  `extern_weak` for the entry to define; on macOS a weak reference resolves only against a dylib,
  so a library and the C harness that links the runtime alone failed to link. The runtime defines it
  (2, the pool) and the entry of a program that calls C stores 1 before `main` runs; a library, which
  has no entry, keeps the pool.
- *"Calls C" is the program's declaration, not the linked runtime.* Cranelift links every part of the
  runtime, C's included, so a policy read from the runtime's parts ran every Cranelift program on
  threads; the build asks whether any unit declares a foreign function.

The regressions, as measured (`performance.md`, N106): the `channels` build's is N83's pool text,
accepted; the check's is N76's lossless tree and N80's provenance, a third and a fifth of the
analysis, with a comparison sort and SipHash inside them removed — a counting pass orders node
openings, and `nazm_span::fast` (multiply-rotate, FxHash's) keys the interner and the resolver's
span tables. Contained, `compiler/check` 213.7 → 180.2 ms. The bench baseline is re-saved at this
tree. Mutants: seven new (`n106-*`), two repointed (`n15-nodes-at-one-place-nest-inside-out`,
`n53-the-runtime-abi-stays`), and N83's nine re-verified against the tests that now ask for threads
by name.

## 7.108 The core's scope, audited — N107, 2026-10-05

Written before the audit's results. Five language-core rows are PARTIAL — 2 (resolution and modules),
3 (types), 5 (effects), 6 (capabilities), 7 (provenance) — each for a list of features another
language has. A row that is PARTIAL until it has every mechanism anyone has built is a checklist,
and a checklist inflates a kernel law 12 says stays small. Owner: `docs/capability-matrix.md`, with
this section as its reason.

**The method, for every candidate gap.** The root problem the row exists for; what Nazm already
does about it, with its evidence; whether the candidate is *required* — a program the repository or
a stated goal needs and cannot write, or an unsound answer — or merely another language's
mechanism; whether it conflicts with the small kernel; and whether a real Nazm use case asks for it.
Required gaps stay, as named blockers or future work. Gaps that are not required leave the row, and
the row's name and limitation say exactly what VERIFIED promises, with every absent construct still
refused by name. **What may not happen:** a defect scoped away, a refusal turned into a silent
absence, or a row renamed because the work is hard. Each candidate is first checked for being a
defect — a wrong answer on a path a program takes — and a defect stays a defect.

**Not in scope:** the ecosystem and domain rows (21 AI/HPC, 25 the language service, 26 packages,
27 the debugger, 28 performance, 30's fuzzing half, 32 reproducibility, 33 platforms, 34 contracts),
which N108 classifies; no code changes, so no epoch, schema, ABI or key moves.

**What would prove the audit wrong:** a program in the repository, or one a stated goal describes,
that the narrowed promise says Nazm handles and it does not; or a dropped candidate that turns out
to be an unsound answer rather than a missing feature.

## 7.109 The core release gate — N108, 2026-10-05

Written before the gate runs. N108 adds nothing: it decides whether the core N101–N107 closed is
a release candidate, on one tree, by evidence gathered on that tree. Owner: the release record
(`docs/releases/`), with this section as its method.

**Classification.** *A — core*, required for the language, compiler, runtime, backends and the
compiler written in Nazm to be mature: areas 1, 2, 2a, 3–9, 10, 10a, 10b, 11–15, 22–24, 29, 30's
mutation half, 31, and the selfhost parity record. *B — ecosystem and domain*, a release limitation
and never core debt: 16 (stdlib breadth), 17 (FFI breadth), 18–20 (profiles and boards), 21
(AI/HPC), 25 (LSP/MCP), 26 (packages, registry scale, build check reuse), 27 (debugger), 28
(performance claims), 30's fuzzing half, 32 (transparency log), 33 (platform breadth), 34
(contracts). *C — external blockers*, stated with their cause: the in-process JIT (area 12:
`unsafe` forbidden, no executable-memory crate offline), proofs of the formal core (no proof
assistant or solver in the image), sBPF execution (area 34: no toolchain offline), a second GPU
provider (area 21).

**The gate, contained, serial, from a clean worktree of one candidate.** The entire mutation
catalogue, targeted with every killer verified, resumed until every entry has a verdict, each entry
accounted for (CAUGHT, SURVIVED, UNUSABLE, NOT INJECTED, MUTANT CRASH, TIMED OUT, RUNNER ERROR or
UNEXECUTED, never converted); the workspace suite; the full lifecycle suite; the selfhost suite;
the bootstrap; fuzzing of every target from its corpus; the LIR oracle and backend differential (in
the suite, and the N73 generator at a larger budget); scheduler and channel stress (the pool's ten
thousand tasks, in the suite); supported platforms and emulators that run offline here; the FFI,
embedded, WebAssembly and contract smoke tests the suite and its images hold; the performance
baseline checked; `cargo xtask contained release` — assembly, provenance, evidence bundle — and
the manifest redigested independently of the tool that wrote it. Anything that cannot run here is
recorded as not run, with why.

**Accepted only if** no class-A row is PARTIAL (N107), N102's parity holds, the authority bridge is
gone (N104), the single-LIR contract holds (N105), the default runtime is the pool (N106), every
catalogue entry is accounted for with no unresolved SURVIVED, and the artifact and its evidence
verify independently. A survivor is fixed — a killer or the code — and re-run, or the gate is NOT
ACCEPTED and says so. Nothing is tagged, pushed or published; the release procedure stays manual.

**Versions frozen:** whatever the candidate carries — semantic epoch 29, `nazm.interface/11`,
runtime ABI 14, `nazm.lir/2`, `nazm.cost/2` — recorded, not moved.

**As run (N108, 2026-10-05/06).** The gate ran in the order written, and found four things, each fixed
in a commit of its own and gated again where it reached (`releases/920a2a5.md`).

- *Thirty-four catalogue entries no contained run could decide.* A targeted session defers a mutant
  every declared killer passed on, and retries it in every later session, so these consumed most of
  each session and would have looped. Sixteen killers saw their defect on macOS only — glibc keeps a
  freed block's bytes; AArch64 does not fault on `MIN % -1`; one oracle sum let two wrong remainders
  cancel — and the killers that see it on Linux were found by running each mutant in a container of
  its own. Nine survived on macOS too: N90's resolver had left `Index::choose` without a caller (deleted,
  its mutants repointed); N105 decided a foreign function's exclusion twice, each copy masking the other
  (decided once); N104 left a capability table read only by its consistency test; a restriction's
  `Unknown` origin, a requirement filter and a statement's debug line had no test that saw them (three
  added). One was equivalent and is retired. Eight are observable only with macOS tools or Apple's clang
  and are verified on the host. **What this says about the catalogue:** a killer verified on one
  platform is evidence on that platform; `architecture.md` keeps N101's no-silent-tier law, and N108
  adds that the release gate's platform is where its killers are verified.
- *A timing-dependent killer:* the polling-select test, fixed to fix who waits first and to run five
  times.
- *A debugger regression from N105,* found by the live gdb test the suite ignores (it needs an image):
  the prologue's `llvm.memset` carried a line and a guard's continuation took the text's last line.
  Fixed in the lowering and the debug pass, held by two debugger-free tests.
- *The release assembly no longer fits one CPU in the runner's 2,400 s;* it ran with four.

Versions frozen as written. Classes: no class-A row PARTIAL; class B's eight PARTIAL rows and class C's
four blockers are the release's limitations. **ACCEPTED** as a candidate; tagging and publishing stay
a person's.

## 7.110 A function written as a value is a reference to it — R1-C1, 2026-10-07

Found by R1's precheck, when the tour's `examples/tour/closures.nz` entered the corpus the
reference audits read: `apply_twice(double, 5)` checked, ran and compiled, and `double` there was
in no entity's set. A corrective step before v0.3.0, not a language change. Owner: the reference
index (`crates/nazm-sema/src/references.rs`).

**The contract, stated first.** Every occurrence the checker resolved to an entity the reference
index models is a use of that entity, whatever the occurrence's syntactic role: a function's name
is a use whether it is called, spawned or written as a value. It is therefore in the index, in
`nazm references` and `nazm resolve`, in the LSP's references, in a snapshot's `references` and
`dependencies`, and in every plan read from the index (rename, patch). There is one way a use is
found — the checker's recorded answer — and no second one: nothing reads the source again.

**What was wrong.** N50 records each named function written as a value
(`Resolution::record_fn_value`, keyed by the name's span, with the `FnId` the checker resolved).
Lowering, MIR and effects read it; `Resolution::each_use`, the one list the index is built from,
did not, so the index — and everything over it — held the call `double(1)` and not the value
`double`. N50 had taught the index that a capture is the binding it copies (`binding_of`) and left
this out. Rename was never unsafe: its plan missed the value, the re-check found `double` undefined
in the candidate, and it refused (`Rejected`, `N0200`); the comment that justified
`Coverage::Complete` for a function ("there are no function values") predated N50.

**The fix.** `each_use` reports every recorded function value as a use of the function it names,
at the name's span, between calls and variables — the kinds are recorded in disjoint positions, so
no occurrence has two answers and none is listed twice. Every form the language accepts is one
path in the checker and so is covered by it: passed as an argument, bound by `let`, returned,
stored in a field, passed to a generic parameter, imported, and imported under a re-export's alias
(where rename keeps the alias's spelling, as for a call). A qualified name (`m::f`) and a method
(`x.m`) are refused as values (`N0100`, `N0608`), so neither has a use to record. A closure is not a
named function and refers to none.

**What did not move.** No checking rule, diagnostic, interface, lowering or runtime behaviour:
semantic epoch 29, `nazm.interface/11`, `nazm.diagnostic/1`, runtime ABI 14. `nazm.resolved/1` and
`nazm.snapshot/3` keep their shapes and now carry the occurrences they always promised — a
correctness fix within the schema, so no major moves; a snapshot of a program with a function
value has different `references` and `dependencies` digests than before, which is the fix. A check
cache entry stores its module's resolved unit under the checker's identity; the crate version in
that identity moved to 0.3.0 in R1A, so only unreleased builds since then could have stored a unit
without these uses, and the repository's own `.nazm` caches were deleted.

**Two audits, stale since N50 and N78, fixed with it.** The corpus audit that asks the resolution
one accessor at a time took a closure's capture slot for a declaration of its own (a capture is
`binding_of`'s binding, N50) and knew no call through a bound (N78). The structure audit took every
name after `x.` in a variant's shape for a variant; `x.m()` with no arguments has that shape and is
a method call when the checker read `x` as a value (N78, *Unchanged meanings*). Structure has no
member to offer at a method's name, which is now stated by a test rather than assumed.

**Evidence.** `crates/nazm-service/tests/references.rs` (every accepted position, exact spans, no
duplicate, imported and aliased); `crates/nazm-cli/tests/resolved_units.rs` (`nazm references`,
cold and from a reused unit); `crates/nazm-cli/tests/lsp.rs` (the real process, UTF-16, local and
imported); `crates/nazm-service/tests/snapshot.rs` (a value use changes the function's
`references` and no call edge); `crates/nazm-service/tests/rename.rs` (a complete plan, from every
occurrence); `crates/nazm-service/tests/closures.rs` (no capture slot is a source definition);
`crates/nazm-service/tests/structure.rs` (a field and a method of one name told apart); two
catalogue entries, `r1c1-a-function-value-is-no-use` and `r1c1-a-function-value-is-a-use-of-its-maker`.

## 7.111 A closure's environment is a node of the ownership graph — Gate 1-C1, 2026-10-07

**What Gate 1 found.** N11 refused every ownership cycle a *definition* could describe (`N0359`),
and the argument that counting reclaims everything rested on the graph of definitions being the
whole graph. N50 added a node the graph of definitions does not show: a closure's environment,
which owns what it captured, behind a function type that says nothing of it. A closure capturing
a `Vec[fn() -> Int]` and stored into that vector owned itself; the checker accepted it and the
memory report ended with it live.

**The decision: refuse, by type, at the capture.** Three facts carry it (`spec.md`, *Cycles*): an
environment is made once and never changes, so its edges point at older values; an edge into an
existing value is made only by storing into a counted handle; and a counted handle that cannot
hold a function value holds nothing that leads back to an environment. So the only way to close a
cycle through an environment is for it to capture a container that can hold one, and that is
what `N0616` refuses. `nazm_sema::UserTypes::function_holder` answers the question over a captured
binding's type — a `Vec` or channel element that is, or reaches through record and enum fields or
further handles, a function type — and `check.rs`'s capture lookup asks it once per capture, where
it already refuses a captured `let mut` (`N0386`). It reads the same instance fields every derived
property reads (`task_safety`, `equality`), so it is not a second type walker, and a future owning
container is covered by being asked the same way.

**What was considered and not done.** A rule at the store — `vec_push(fs, f)` refused when `f`
might reach `fs` — needs to know what an opaque function value captured, which its type does not
say, and alias analysis of handles; either a new capture-carrying component of function types or
refusing every store of an unknown function value, including inside every generic container
function. Refusing at the capture needs neither: every function value, wherever it came from,
was held to the rule where it was written, so storing one is always safe and generic code is
untouched. A cycle collector was the constitution's third option; it would be a second
reclamation mechanism with its own timing, pauses, task-safety and profile questions, beside a
counting scheme that is otherwise complete. Not introduced.

**What it costs.** Some closures that would never close a cycle are refused — one that only
reads a vector of handlers. The remedy is to pass the vector as an argument. The rule is stated
by type so that a reader can apply it without running the program.

**The compiler written in Nazm** refuses the same capture with the same code and span
(`compiler/analyse.nz`, `holds_functions`). Its type arguments are never function types, so in
its subset the container is a `Vec` of records with a function field; that form is in
`compiler/conformance/refused/` and `crates/nazm-cli/tests/refusals/`.

**Identity.** The semantic epoch moved 29 → 30: programs accepted before are refused. `Type::ALL`,
hashed into the checker's identity, gained the five capability types it lacked since before they
existed (so completion offers all seven); it moved in the same epoch. The interface schema and the
runtime ABI are unchanged.

**Evidence.** `crates/nazm-cli/tests/closures.rs`: every shape refused once, with nothing else
(direct, alias, mutual, nested closure, a record, a `Vec` of records, an enum, an `Option`, a `Vec`
of `Vec`s, a closure made in a helper); an acyclic program — named functions, closures capturing
`Int`, `Str`, `Vec[Int]` and another closure, a record with a function field, a closure through a
generic function and one stored by a function that cannot know what it captured — agreeing under
the interpreter, LLVM at `-O0` and `-O2` and Cranelift with every count `live=0`; and a closure made
in another module. `tests/compat/v1/`: five refusals and one run case. Catalogue entries `g1c1-*`.

## 7.112 The general-purpose foundation — Gate 2, 2026-10-07

Gate 2's decisions are recorded in [`general-purpose.md`](general-purpose.md), written before its
code: one section per mechanism, each with the problem, the mechanism Nazm already had, why it was
not enough, the semantic cost, its placement and its evidence plan. The constraints it states are
architectural and are restated here so that they are found where the laws are:

- **No second mechanism where one exists.** Floats and fixed-width integers are built-in types with
  the existing operator and call rules; wrapping is a function, not an operator. A view is a counted
  reference, as `Str`'s slices already were — no lifetime syntax. A socket wait parks through the
  pool's existing hooked wait, woken by a reactor, not through a second suspension mechanism or an
  `async` syntax. Maps are library code over one new derived requirement, `Hash`, the twin of
  `Equality`.
- **Authority stays a capability.** `NetCap`, `RandomCap` and `ProcessCap` join the seven kinds;
  the set widens to sixteen bits and no id changes.
- **Every new value kind joins the ownership graph.** `Bytes` and `OsHandle` hold no values and
  close no cycle; arrays are inline and own what their elements own.
- **Conditions are the build's, never the environment's.** `@cfg` is decided from target, features
  and profiles, which are part of every key that could reuse a result.

## 7.113 The user cache: the runtime and the standard library compiled once per user — R1.5 D12, 2026-10-15

Written before the code (`openspec/changes/r1-5-performance-and-dx/design.md`, D12). Until now every
project compiled its own runtime and its own copy of every `@std` module it imports, once per
`.nazm/`: a new project, a second checkout or a scratch directory paid for clang on the runtime
before it paid for anything of its own. The objects of those units do not depend on the project, so
they are kept where every project finds them.

**What is settled.**

- **Where.** The user cache is `NAZM_USER_CACHE_DIR` when that is set (empty turns the user cache
  off, and its units return to the project store); otherwise `NAZM_CACHE_DIR` when that is set, so
  one variable still moves everything; otherwise `~/Library/Caches/nazm` on macOS and
  `$XDG_CACHE_HOME/nazm` (an absolute `XDG_CACHE_HOME`), or `~/.cache/nazm`, elsewhere. With no home
  directory there is no user cache. Its objects are a `native/` store of `nazm.object/1` entries,
  exactly the project store's format.
- **What.** The runtime's units — the runtime, the context switch, the platform's initialisation,
  its stacks, `errno`, its files and the counted heap — and each `@std` or prelude module's own
  unit. A generic instance is the program's (its type arguments are), and so are the entry and every
  module of the project's own: those stay in `.nazm/native/`.
- **The key.** The project store's, unchanged: a digest of the exact bytes handed to the object
  compiler, the compiler's identity and the configuration it resolves (`-###`), for LLVM text; for a
  Cranelift object, its backend's key over the unit's LIR, settings, target and runtime ABI. So
  everything that changes an object's bytes is in its key by construction — the target and its
  CPU, the optimisation level, the counted heap and its limit (the runtime's text is rewritten, and
  the heap unit holds the limit), debug information, the services the program reached (the
  runtime's text is specialised to them, N82), the runtime ABI and the toolchain — and nothing
  enumerates them. One runtime object per distinct runtime text, target and profile, not one
  archive: `nazm runtime build`'s archive holds every service, with thread-local failure state, and
  a build linked to it is a different executable from a build without the cache.
- **Publication.** As the project store: a uniquely named temporary file, then a rename, so two
  builds at once both write and one rename lands second with identical bytes. An entry is checked
  as every entry is before it is a hit; a damaged one is a miss.
- **Linking.** Copied out of the entry into the build's own working directory, as a project hit
  is, and linked from there: an entry is a header and then an object, never an object file.
- **Off.** `--no-cache` uses neither store. A board build (N63) uses neither, as before. Deleting the
  user cache changes no program: it is derived data, and the next build compiles what it lacks.

**Evidence.** `crates/nazm-cli/tests/user_cache.rs`: a second project reuses the first one's runtime
and `@std` objects and compiles only its own, its `.nazm/` holds no runtime object, and its
executable is byte-identical to a `--no-cache` build's; another optimisation level, heap limit or
target reuses nothing of the other's; deleting the user cache changes nothing; `NAZM_CACHE_DIR`
alone still moves everything. Every test names a temporary user cache, and `.cargo/config.toml`
turns the user cache off for every other test, so no test writes to a real one.

**Not settled.** Eviction: like the project store, the user cache grows until it is deleted.

## 7.114 Dead-stripping: what nothing calls is not linked — R1.5 D14, 2026-10-15

Written before the code (`openspec/changes/r1-5-performance-and-dx/design.md`, D14). An executable
carried every function of every unit it linked: each `@std` module's whole unit, and under
Cranelift every part of the runtime, whether the program called it or not.

**What is settled.**

- **Mach-O** (macOS, iOS): the link passes `-dead_strip`. `ld64` strips by symbol, since clang's
  objects (and Cranelift's) are divided at each symbol already; no compile argument changes, so no
  Mach-O object key changes.
- **ELF** (Linux, Android): each object is compiled with `-ffunction-sections -fdata-sections` and
  the link passes `--gc-sections`. The compile arguments are in every object's resolved
  configuration (`-###`), so every ELF key changes once, by construction.
- **Executables only.** A static or shared library keeps every function, for its C callers, and
  Windows, WebAssembly and the boards link as before. `crates/nazm-cli/src/backend.rs` decides, from
  the target, in one function the link and the probe share.
- **What a program does is unchanged.** The linker leaves out only what nothing reachable from the
  entry refers to; a function a task, an export, an interrupt table or a pointer reaches is
  referred to, and kept.

**Evidence.** `crates/nazm-cli/tests/dead_strip.rs`: a program calling one `@std/text` function
carries none of its others under either backend at `-O0` and `-O2` and prints what it did before; the
same objects linked whole from `--objects` are larger and hold the unused function. Measured in
`docs/performance.md`, *R1.5 D14*. Two mutants.
