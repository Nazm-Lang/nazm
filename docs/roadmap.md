# Roadmap

**Status: Era 1 (M1–M3) is complete. This document answers one question — what comes next,
and why in this order.**

It stopped being a build diary on 2026-09-21. Roughly 900 lines of completed M1a–M1f
increment narrative were removed: every accepted/rejected table, every status section, every
derivation of a rule that had already landed. Nothing was lost that is not preserved better
elsewhere — the semantics went to [`spec.md`](spec.md), the evidence to
[`bootstrap.md`](bootstrap.md) and [`releases/4fb1554.md`](releases/4fb1554.md), the current
status to [`capability-matrix.md`](capability-matrix.md), and the narrative to Git. Two
rules that other files were citing *this* file for — where a loop target binds, and why
native recursion is refused rather than counted — were normative, so they moved to
`spec.md` where a reader would look for them.

Ambition beyond the next few milestones is [`NAZM_LANGUAGE_GOALS.md`](NAZM_LANGUAGE_GOALS.md).
The invariants that constrain any ordering are [`master-architecture.md`](master-architecture.md).

---

## Where the line is today

| | |
| --- | --- |
| `nazm run` | The interpreter. The widest subset |
| `nazm build` | Native, for the bootstrap subset. **Narrower than `nazm run`** — notably it refuses recursion |
| `compiler/*.nz` | A compiler for Nazm written in Nazm, reaching a C2 ≡ C3 fixpoint |

Ask `nazm capabilities --json` for construct-level truth; it is read from the compiler's own
tables. Ask `capability-matrix.md` for architectural truth, area by area.

## What Era 1 delivered

| Milestone | Outcome | Evidence |
| --- | --- | --- |
| **M1** — native vertical slice | `Int`/`Bool`/`Str`/`Ints`/`Strs`, all control flow, calls, one IR level, LLVM text → `clang` | `capability-matrix.md` areas 10–11; 127 two-oracle tests |
| **M2** — facilities for a compiler | Text, sequences, I/O, modules | `bootstrap.md` §1 |
| **M3** — bootstrap | C1 → C2 → C3, byte-identical in IR and executable | `bootstrap.md` §3 |
| **Concurrency C-1…C-3** | Specified first, then interpreted, then compiled, then compiled *by* the Nazm compiler | `capability-matrix.md` area 14 |
| **AI-native (a)** | Versioned schemas, a checked grammar, a generated guide, a canonical formatter, machine-applicable fixes | `capability-matrix.md` areas 22–24 |
| **Release** | One experimental candidate, contained, manifest-verified twice | `releases/4fb1554.md` |

### What M2 bypassed rather than delivered

This matters more than what it delivered, because it is where the next decisions come from.
M2's first, third and fourth rows — **value/copy/move semantics with cleanup, records and
tagged unions with exhaustive matching, and generics** — were never built. The compiler was
written around them, with parallel `Ints`/`Strs` arrays addressed by index. That was the
shortest sound path to a bootstrap and it worked; it also means the type system has never
been asked the questions that make type systems hard, and that the memory model is still
the one `bootstrap.md` §1 states: allocate, never free.

---

## What comes next, and why in this order

The ordering rule is [`master-architecture.md`](master-architecture.md) §3: **a layer earns
its existence by owning a semantic responsibility no existing layer owns.** Matching the
aspirational IR diagram is not such a reason. By that test the next step is not the next box
in the diagram — it is the one place where a responsibility is currently owned *twice*.

### N1 — Source identity, and one resolution result

**The problem, from the tree rather than from the diagram.**

Name and signature resolution happens twice, independently:

- `crates/nazm-core/src/check.rs` resolves names and returns
  `(HashMap<String, Signature>, Vec<Diagnostic>)`.
- `crates/nazm-core/src/lib.rs:160` then **discards that map** — `let (_, check_errors) =
  check(&program);` — and hands the bare AST to the backend.
- `crates/nazm-lir/src/lower.rs` rebuilds it: its own `sigs: HashMap<String, Sig>`, its own
  `resolve`, its own slot allocation.

And the two built-in tables are transcribed separately — `Intrinsic::ALL` in
`crates/nazm-core/src/intrinsic.rs` and `Builtin` in `crates/nazm-lir/src/ir.rs`, 31 entries
each, kept in agreement **by a test rather than by construction.**

Separately, a span has no file identity. `nazm_span::Span` is `{ start: u32, end: u32 }`,
there is no `FileId` type, and `crates/nazm-cli/src/load.rs` concatenates every reachable
file into one buffer, recovering the filename from the offset. That is sound for a
whole-program compiler and it is the hard stop under separate compilation, incremental
compilation, content-addressed definitions and any language server.

**The shape of the answer:**

```text
source files
    ↓
real source identity (a file a span can name)
    ↓
one name/signature resolution result
    ├── the checker/evaluator consumes it
    └── lowering/backend consumes it
```

**The questions to answer before writing any of it** — a crate is not created to match a
diagram, and this milestone fails if it adds a pass-through:

1. Does the artefact belong in an existing crate? `xtask/src/rules.rs` forbids
   `nazm-lir → nazm-core`, so a shared artefact lands in `nazm-syntax`, or in a new crate
   both depend on. That constraint is the reason the duplication exists, so it is the first
   thing to settle rather than the last.
2. Would a new crate own a real semantic responsibility, or would it only hold a struct?
3. What exact representation does the resolution result need — enough for the evaluator's
   environment *and* the lowerer's slots, without becoming a second AST?
4. What is the stable identity of a definition, and does it survive an unrelated edit
   elsewhere in the file?
5. How do built-ins enter the same resolution model **without a third table**? One table,
   two consumers, is the point; two tables plus a resolver is worse than today.
6. How does a span retain file identity without touching every diagnostic, every syntax
   node and every test? The merged-buffer design was chosen precisely to avoid that cost,
   so the replacement has to state what it pays.
7. Which existing tests prove behaviour is unchanged? The 114 language tests, the 127
   two-oracle native tests and the 20 selfhost differentials are the floor.
8. **Which duplicate resolver disappears?** If the answer is "neither, but now they share a
   type", the milestone has not been done.

**Acceptance:** one resolver, one built-in table, a span that names its file — and the
selfhost differentials and two-oracle suite unchanged. **Not started.** Nothing here
authorises ownership, effects, capabilities, generics, user-defined types, a second
backend, a scheduler or a package manager.

### After N1, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| Separate compilation units | N1 | there is no unit until a span knows its file |
| Incremental / content-addressed compilation | units | nothing of definition granularity to hash — `research-register.md` R2 |
| A language server | units + a lossless CST | it would re-implement the semantic engine otherwise |
| Ownership / the surface memory model | — | the largest open design question; `research-register.md` R1 |
| Effects | a typed core IR | and *"is divergence an effect?"* is unanswered — `spec.md` *Open* |
| Capabilities, information flow | effects | `architecture.md` §5 keeps the three separate |

---

## Long-running tracks

**Concurrency — C-4 and C-5, still not now.** What shipped is one POSIX thread per task and
nothing more. C-4 is `nazm-rt`: a scheduler, stacks, channels and a reactor, and **the one
crate allowed `unsafe`** — which means the workspace's `unsafe_code = "forbid"` becomes
`deny` with a narrow allowance that `xtask check` enforces. It depends on the memory model.
C-5 is measurement: spawn-to-first-instruction latency, context-switch cost, and task memory
as **RSS, not virtual reservation, with page size recorded**. No scheduler work starts before
C-4's prerequisite lands; a single-threaded compiler is a perfectly good compiler.

**AI-native — one item left.** A lossless CST (`cstree` is the recorded choice) and the
~3k-token cheatsheet. The formatter, grammar artefact, schemas and fixes all shipped. **No
language keyword will be added to support the label**; if a construct earns its place on
semantics it earns it, and "AI-native" is not an argument for syntax.

**Backends.** LLVM IR as text stays the only backend. Cranelift is an ordering decision, not
a rejection — it becomes worth having when one of its four jobs (JIT, comptime evaluator,
REPL, hot reload) exists. None does.

## Deliberately not next

Ownership · effects · capabilities · provenance · generics · user-defined types · pattern
matching · M:N scheduling · a second backend · GPU or accelerator work · a package manager ·
a standard library beyond the built-in table.

Each has a home — `capability-matrix.md` for status, `research-register.md` for the ones
that are hypotheses, `NAZM_LANGUAGE_GOALS.md` for why they are wanted eventually. None is
blocked by the others in a way that forces it now, and **all of them are easier after N1**,
which is the argument for doing the small thing first.
