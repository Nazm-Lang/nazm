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
| the module graph's **construction** | `crates/nazm-cli/src/load.rs` | resolving a path reads the filesystem, which is an effect, and the front end performs none |
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

- **`Str` and `Chan` storage is not reclaimed.** The reasons are above and in the
  specification, in those words.
- **`exit_with` reclaims nothing and reports nothing.** Found by measuring: the
  self-hosted compiler ends that way, so the entry wrapper is never reached. It is correct
  — a process that has ended returned everything — and there is a test so that nobody
  later spends real time on the largest workload here "fixing" it.
- **A failure still leaks the temporary it was holding.** A `Val::Transferred` on a guard's
  failing path cannot be followed by a release, and that path ends the program. Stated
  rather than papered over.
- No destructors, no RAII, no user-visible ordering.

## 8. Toolchain, and one real incompatibility

Availability is not compatibility. Declared MSRVs checked 2026-09-19:

| Crate | Version | MSRV | vs local Rust 1.93.1 |
| --- | --- | --- | --- |
| `cranelift` | 0.135.2 | **1.95.0** | **fails** |
| `cranelift` | 0.132.3 | 1.93.0 | ok |
| `redb` | 4.3.0 | 1.90 | ok |
| `salsa` | 0.28.4 | 1.88 | ok |
| `cstree` | 0.14.0 | 1.85 | ok |

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
