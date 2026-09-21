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
| 4 | `nazm-span` · `nazm-diag` · `nazm-syntax` · `nazm-core` · `nazm-cli` | **exists** (as an interpreter; `nazm-core` is checker + evaluator, not the Core IR below) |
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

Roles, in dependency order: `span` (files, byte spans, interned identifiers — depends on
nothing; the `FileId` this list used to promise **does not exist**, and
`crates/nazm-cli/src/load.rs` merges every reachable file into one buffer instead, which is
what currently blocks separate and incremental compilation) · `diag` (stable error codes, fixits, JSON schema, renderer) · `syntax`
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
