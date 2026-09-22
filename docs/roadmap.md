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

### N4 — Sound incremental semantic reuse · **complete, 2026-09-22**

**Done, in two commits** (`2565419` the keys and the store, `7b44b4f` the root and the
reuse).

| | |
| --- | --- |
| **N4A** | A **declared** source root, which had to come first: under a derived one a module's key depends on which file the compilation began at, so two compilations of one project never share anything. `nazm.root` is an empty marker — enforced empty, because the first field it carried would make it a manifest — and `--source-root DIR` names one without writing a file. With neither, nothing changes |
| **N4B** | `nazm-cache`: `SourceFingerprint` over the exact source bytes with no normalisation at all, `CheckerIdentity` deriving the built-in table, the type set and both schemas with an explicit `SEMANTIC_EPOCH` for what cannot be derived, and `CheckKey` over both plus each direct import's `(ModuleKey, InterfaceHash)`. Every input tested by removing it |
| **N4C** | `nazm.check/1` and a local store. Content-addressed, so entries are immutable and two compilers need no lock; published by `rename`; validated on read by recomputing the key from the inputs the entry records. Sixteen ways of damaging one, every one a miss |
| **N4D** | One hook in `nazm-core`, `Reuse`. `check_unit` is still the only thing that decides what a body means; the driver only says which bodies it needs decided. Only a clean check is written, so no entry is ever a cached refusal |
| **N4E** | `--no-cache`, `--cache-report` (one `nazm.check-report/1` object, off by default), `NAZM_CACHE_DIR`. The store's location is not part of any identity |
| **N4F** | A **cache soundness** gate, proved by breaking it three ways; six mutations; twenty-nine end-to-end tests, all of them running the binary |

**The acceptance test was met, three times.** A private body change re-checks its own module
and leaves its importer alone. An exported signature change re-checks the importer and finds
the error it now has. In `A → B → C`, a change to C that forces B to re-check but leaves B's
interface unchanged **stops at B** — which is the difference between interface-driven
invalidation and a reachability sweep, and the reason the design is worth the keys.

**What N4 is not, and does not claim.** Incremental *native* compilation. What is reused is
the second stage of checking one module — its bodies. Every module is still read, parsed and
declared from source on every run, which is exactly what makes a skipped module invisible.
`nazm run` and `nazm build` reuse nothing, because both need the resolution that only
checking a body produces, and lowering is still whole-program. There is no object, IR or LIR
cache, no query database, no scheduler and no eviction policy.

### N5 — Separate code generation and internal linking · **complete, 2026-09-22**

**Done, in three commits** (`0bf58be` a preflight correction, `41e0536` the symbol
constitution, `d303571` the split).

| | |
| --- | --- |
| **N5A** | A native symbol is a spelling of the definition's `DefKey`, not a position in a whole-program numbering. The encoding escapes every byte outside `[A-Za-z0-9]` including `_`, so it is injective and collisions are impossible **by construction** — no digest, no truncation, no probability. `pub` chooses linkage and never spelling |
| **N5B** | Lowering splits into a global `plan` — signature admissibility and the recursion refusal, both of which span modules — and `lower_module`, which reads one module's bodies and every function's *signature*. No new IR |
| **N5C** | `emit_unit` per module. A call across a boundary is a `declare` and a linker reference; a unit contains no other module's body and has no route to one |
| **N5D** | Runtime and platform entry in artefacts of their own, defined once. Constants needed no qualification: `private` linkage makes them local labels, established by linking two objects that both define `@.s0` |
| **N5E** | `clang -c` per artefact, then a link. Deterministic input order, a working directory this build created and removes, and nothing else touched |
| **N5F** | A `native identity` gate, proved by breaking it three ways; five mutations; sixteen end-to-end cases; clean-build cost measured against the previous commit |

**The acceptance tests were met.** Three modules each define a private `helper` and all
three compile; a diamond generates the shared module once; a legal import cycle links; a
cross-module *call* cycle is still refused; an imported `main` is not the process entry;
two checkouts and two entry points give one library identical symbols.

**A preflight found a live defect and fixed it first.** Every native runtime failure named
the file `nazm build` was invoked on, so a division by zero in `lib.nz` printed
`main.nz` with `lib.nz`'s line and column — a location that points somewhere real and
wrong. The interpreter had been right all along and no assertion compared the two.

**What N5 answered that N4 had assumed.** A's object names B in one `declare` and one
`call`. Changing B's body, adding a private definition, or adding an export A does not call
leave A's artefact byte-identical — so an object depends on its dependencies' **symbol
identity and ABI signature**, not their implementations. N4's report said otherwise; it was
reasoning ahead of an architecture that did not exist yet.

**What N5 is not.** A cache. Every module is regenerated on every build, and the clean build
is roughly twice as slow for a small program — almost entirely `clang` process startup,
which `performance.md` measures rather than hides.

### After N5, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| A persistable unit interface | durable identity ✓ | done — N3 |
| A key independent of which file was named | a declared source root ✓ | done — N4 |
| Semantic check reuse | that, plus a key covering every checking input ✓ | done — N4 |
| Separate *code generation* | units ✓ + a linking model ✓ | done — N5 |
| Native artefact reuse | separate code generation ✓ + a key over the backend's input ✓ | done — N6 |
| Content-addressed compilation | reuse ✓ + a normalised form of a definition | nothing normalises or hashes a definition — `research-register.md` R2 |
| A language server | units ✓ + durable identity ✓ + reuse ✓ + a lossless CST | it would re-implement the semantic engine otherwise |
| Ownership / the surface memory model | — | the largest open design question; `research-register.md` R1 |
| Effects | a typed core IR | and *"is divergence an effect?"* is unanswered — `spec.md` *Open* |
| Capabilities, information flow | effects | `architecture.md` §5 keeps the three separate |

### N6 — Content-addressed native object reuse · **complete, 2026-09-22**

An emitted LLVM unit whose object was compiled before is not compiled again.
`architecture.md` §7.6 owns the design; this is what shipped and in what order.

| | |
| --- | --- |
| **preflight** | A generated artefact names a module by its durable identity. Two strings used to carry the path the command line spelled — the `source_filename` and every runtime failure's location — so one project built from two directories was two programs. Without this there is no reuse between checkouts and no honest failure message |
| **N6A** | `ObjectKey` over three things and nothing else: a digest of the exact bytes handed to the object compiler, that compiler's identity, and the configuration it resolves. A store beside the semantic one under a shared `.nazm/` root, one file per entry, validated six ways before it is believed |
| **N6B–D** | One `clang -###` per build reads what the driver says it will actually run, so the triple, the CPU features, the relocation model and the ABI are in the key without anyone enumerating them. The invocation is hermetic: an environment built from nothing plus ten named variables. Module, runtime and entry units all go through one key and one store. `--no-cache`, `--build-report`, `nazm.build-report/1` |
| **N6E** | An `object cache` gate, proved by breaking each of its rules; six mutations, three of which attack the key and three the validation; forty end-to-end and store-level cases |
| **N6F** | Measurement, and the authorities |

**What the key is, and what it is not.** Not a second dependency graph — not the module's
source, not each dependency's exported signatures, not an emitter epoch. Every one of those
is an approximation of what `clang -c` reads, and every approximation leaves something out.
The emitted IR **is** the input, so N5's measured precision falls out rather than being
encoded: a dependency's body change and an export the caller never calls each leave the
caller's object alone, and a called signature change does not, because it is written into
the caller's own `declare`.

**What a warm store cannot do.** Bypass anything. A key is a digest of an artefact, so the
artefact must exist before the key does, so the reading, parsing, checking, backend
admissibility, lowering and emission have all already happened — including the whole-program
recursion refusal, which a fully warm store never reaches.

**Measured.** On the five-module compiler: `-O0` 665.4 ms with `--no-cache`, 730.7 ms cold,
**157.6 ms** warm; `-O2` 1693.0 ms, 1755.4 ms, **157.6 ms**. A one-module edit costs
198–218 ms whether it is a private body, a called export's body or an export nobody calls —
the three are indistinguishable, which is the whole point. A warm build runs two external
processes where a cache-disabled one runs eight.

**What N6 is not.** Incremental native compilation. Lowering and emission run every time and
so does the link; there is no executable cache, no `LinkKey`, no incremental linker, no
persistent LIR, no LTO. Nothing is evicted, so the store grows until it is deleted.

### N7 — the next milestone, and not started

Two candidates, and the ordering between them is the first decision.

**The remaining per-build processes.** A warm build's cost is now two external processes: a
`clang -###` probe at about 60 ms and the link at about 89 ms. The probe is a fact about a
toolchain rather than about a program, so it is memoisable — and doing so means a second
cache whose invalidation question is exactly the one the backend identity already struggles
with, which is why it was not done here. The link is the larger number and reusing it needs a
`LinkKey`: the linker, its version, its options, and the identity of every object in order.
That is executable reuse, and it is a different soundness argument from this one.

**Or the front end.** `nazm build` cannot reuse a semantic check at all, because lowering
reads the resolution that checking a body produces and nothing persists one. Persisting it
means designing a written form for name resolution — which is `research-register.md` R2's
neighbourhood and a larger piece of work than either cache above.

Neither is urgent. A 158 ms warm build of the whole self-hosted compiler is not the
bottleneck in this project.

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
