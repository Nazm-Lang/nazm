# Architecture

**Status: design record. Partly implemented — see below.** This documents decisions made
before writing the compiler, and the reasoning that would otherwise be lost. Where a
decision rests on a claim that has not been checked, it says so.

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

| Phase | Crates | State |
| --- | --- | --- |
| 2 | `nazm-bench` · `xtask` | **exists** |
| 4 | `nazm-span` · `nazm-diag` · `nazm-syntax` · `nazm-sema` · `nazm-iface` · `nazm-core` · `nazm-cli` | **exists** (`nazm-core` is checker + evaluator, not the Core IR below; `nazm-sema` and `nazm-iface` are not on this diagram at all — see §7.2 and §7.3) |
| 4 | `nazm-hir` · `nazm-ty` · `nazm-mcp` | planned |
| 6 | `nazm-db` · `nazm-mir` · `nazm-lsp` | planned |
| 7 | **`nazm-lir`** | **exists** — named for this level, but a structured typed tree over numbered slots rather than SSA over a CFG of places. See the note below |
| 7 | `nazm-codegen-clif` · `nazm-link` · `nazm-driver` | planned |
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
(Core IR, normalisation, Merkle hashing, reference interpreter) · `db` (salsa queries +
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
assumed: the target triple, CPU features and ABI (no rule in `crates/nazm-core/src/check.rs`
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
to hashing.

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
`channel_runtime` is a line of `crates/nazm-lir/src/emit.rs`'s runtime constants with
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
and rename (§7.20), since N19 completion (§7.21), and since N20 signature help (§7.22).
Nothing else is advertised. The core prelude has no file, so a
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
Checker          which callee the call resolved to, the signature its arguments   (nazm-core/src/check.rs)
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
