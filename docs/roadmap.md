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

### N1 — Source identity, and one resolution result · **complete, 2026-09-21**

**Done, in four slices** (`0045ed9`, `26a52bd`, `ea016c3`):

| | |
| --- | --- |
| **N1A** | `Span` is `{ file, start, end }` with file-local offsets; `SourceMap` replaces the merged buffer; each file is parsed on its own text. The published diagnostic shape is unchanged |
| **N1C** | One built-in inventory. `nazm-lir`'s 31-variant enum and its `by_name`/`name` tables are gone; `Builtin` is `nazm_syntax::Intrinsic`. `Type` and `Intrinsic` moved to `nazm-syntax` so the backend can see them without reaching the interpreter |
| **N1B** | `nazm_core::check` produces a `nazm_syntax::Resolution`; the backend reads it. Deleted from `nazm-lir`: `resolve`, `bind`, `scoped`, the scope stack, the name-keyed signature table, the declared-names set, `native_ty`, and the built-in-first rule at the call and spawn sites |
| **N1D** | A `cargo xtask check` rule fails if `nazm-lir` declares a name-keyed table; eleven tests run resolution-sensitive programs both ways and require one answer |

**The acceptance test was met:** one of the two resolvers no longer exists.
`crates/nazm-lir/src` declares no symbol table keyed by a name and performs no name lookup.

**No new crate.** The shared vocabulary went into `nazm-syntax`, which both consumers
already depend on; the resolver did not move, the duplicate was deleted. `architecture.md`
§7.1 records why a `nazm-hir` holding a copy of the checker's answer would have been the
pass-through the ordering rule exists to prevent.

**What N1 deliberately did not do:** no HIR, no Core IR, no MIR — a resolution is not an IR
— no ownership, effects, capabilities, generics, visibility, separate or incremental
compilation, and no content-addressed identity. Every id it produces is session-local.

### N2 — Compilation units, visibility and module interfaces · **complete, 2026-09-22**

**Done, in two commits** (`ae77034` the semantics, `871f09e` the implementation), after a
separate correction (`72019e2`) that removed a lossy `Serialize` from `Span`.

| | |
| --- | --- |
| **N2A** | `spec.md` settles it: one module is one file; top-level definitions are private and `pub` exports; `use` makes a module's *exported interface* nameable, unqualified; imports are **not transitive**; every collision is a diagnostic naming both sides; cycles stay legal; the root module owns `main`. Grammar gains `[ visibility ]`, one bit, one spelling |
| **N2B** | `ModuleId` and a `ModuleGraph` the driver builds while loading — a graph, not an ordered bag of files. `nazm-sema` is created, and earns it: `nazm-syntax`'s lexer, parser, formatter and AST referred to `types`, `intrinsic` and `resolve` exactly zero times |
| **N2C/D/E** | `declare_unit` reads one module's declarations and returns its `UnitInterface`; `check_unit` checks its bodies against its own definitions and its dependencies' **interfaces**. `check` is a loop over the two and holds no resolution of its own |
| **N2F** | Deleted: the checker's program-wide name table, the evaluator's `HashMap<String, usize>` and its private copy of the built-in-first rule, and the backend's last two name lookups. 80 of the self-hosted compiler's 215 functions became `pub`; 135 stayed private |

**The acceptance test was met.** `crates/nazm-core/tests/unit_checking.rs` checks a module
against an interface for a module whose **source does not exist** — no tree, no body,
nothing to parse — and refuses the same module when that interface is empty. A dependency's
bodies are not a parameter of `check_unit` and are not reachable from one.

**What N2 deliberately did not do:** nothing is separately *compiled*. Lowering still takes
the whole program and emits one LLVM module; there are no per-module artefacts, no cached
interfaces and no linker symbols. Every id remains session-local. No packages, no
re-export, no aliasing, no qualified paths, no HIR.

### N3 — Durable identity and persisted interfaces · **complete, 2026-09-22**

**Done, in one commit after a separate constitution commit** (`40b1c9a` the semantics).

| | |
| --- | --- |
| **N3A** | `spec.md` settles what makes this the same module and the same definition, before any representation. A **source root** — the root module's directory, derived not declared. `ModuleKey` is the normalised path relative to it; `DefKey` is that plus a kind and a name. A body edit, a signature edit, a visibility change and a move all keep the key; a rename does not |
| **N3B** | `ModuleKey`/`DefKey` in `nazm-sema`, beside the session ids rather than replacing them. The loader computes them and **withdraws** one where the two compiler implementations could disagree |
| **N3C** | `nazm-iface`: `nazm.interface/1`, deterministic, validated on read. serde lives there and not in `nazm-sema`, so a session-local id cannot be serialised by accident |
| **N3D** | `InterfaceHash` — BLAKE3 over the canonical bytes. Twelve invalidation rows measured rather than asserted |
| **N3E** | `nazm interface FILE`, so the boundary is real: two processes, two directories, and a dependent checked with its dependency's source **deleted** |
| **N3F** | A `sema purity` gate, verified to fire both ways; five identity mutations, all caught |

**The symlink policy is the part that was settled rather than coded around.** The reference
loader keys by canonical path and the Nazm-written one by normalised text, so an aliased
file is one module to the first and two to the second. Durable identity **declines** where
they could disagree, and so does a path that escapes the source root. Both compile exactly
as before; they simply get no key, and everything built on one refuses rather than guesses.

**What N3 is not.** No cache, no store, no query engine, no scheduler, no artefact reuse,
and no claim that anything is faster. `InterfaceHash` answers *must this module's dependents
be re-checked* and excludes bodies by design — using it as a codegen cache key would hand
back an object file compiled from a body that has since changed, which is why
`architecture.md` §7.3 names an implementation fingerprint and a `BuildKey` it is not.

### N4 — Reuse, and the inputs an artefact actually depends on

**The next dependency, and not started.** Everything N3 built is an input to something that
does not exist. The next milestone is the first one that *reuses* work, and it needs two
things N3 deliberately did not build:

* a **`BuildKey`** — compiler identity, target, flags, dependency fingerprints and a
  module's own implementation fingerprint. `InterfaceHash` is not it and must not become
  it;
* somewhere to put the result, and a rule for when reading it back is sound. Anything
  omitted from the key is a silent miscompilation waiting to happen, which
  `architecture.md` §3 has said since before there was a compiler.

**A decision that comes first, and is a build-system decision rather than a compiler one:**
a **declared** source root. N3's is derived from the file named on the command line, so a
module's key depends on which file that was. A project file would fix it, and it is the
same decision as a manifest — `capability-matrix.md` area 26.

### After N3, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| A persistable unit interface | durable identity ✓ | done — N3 |
| Artefact reuse | a `BuildKey`, and somewhere to put results | `InterfaceHash` excludes bodies, so it cannot key an artefact |
| Incremental / content-addressed compilation | reuse + a normalised form of a definition | nothing normalises or hashes a definition — `research-register.md` R2 |
| A key independent of which file was named | a **declared** source root | today's is derived from it; the same decision as a manifest |
| Separate *code generation* | units ✓ + a linking model | checking is per unit; emission is still whole-program |
| A language server | units ✓ + durable identity ✓ + a lossless CST | it would re-implement the semantic engine otherwise |
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
