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

### N7 — Memory constitution and the first reclamation — **complete, 2026-09-22**

The cache ladder stopped here on purpose. `architecture.md` §7.7 owns the design and the
evidence; `spec.md`'s *memory constitution* owns the law.

| | |
| --- | --- |
| **preflight** | A mutation verdict is measured the way the suite is authoritative — one job, one test thread. `nproc` reports 10 inside a container limited to one CPU, and the suite has wall-clock deadlines, so verdicts were a function of scheduling. The dangerous direction was never the one that was noticed: a spurious failure while measuring a **mutant** counts as CAUGHT and is invisible |
| **N7A** | The audit. Every type across both implementations, every allocation site, the leak measured, and `compiler/*.nz` counted |
| **N7B–C** | Five models compared against real Nazm code; the constitution settled in `spec.md` before anything was implemented |
| **N7D** | Reference counting on the sequence header. Reclamation on the three exit edges of a generated function, and the same rule modelled in the interpreter rather than inherited from the host |
| **N7E** | Sixteen cases, each run twice and compared; six mutations |
| **N7F** | Measurement, and the authorities |

**What the audit changed.** Two findings moved the design before a line was written. A
`Str` has four provenances and does not say which — a literal's constant, `argv`, a heap
buffer, or the **interior** of any of them, because `str_slice` is specified not to
allocate — so `free` is wrong for three of the four. And the specification's `Str` value
semantics are currently paid for *with the leak*. `Str` is therefore out of the slice, in
the specification's own words.

**What did not change.** Any program's meaning. `let b = a;` still aliases a sequence, a
push through one is still visible through the other, a parameter is still borrowed. A model
that had to break that to be implementable would have been the wrong model.

**Measured.** 5,000 / 20,000 / 80,000 short-lived sequences: 4.38 / 12.28 / 43.52 MiB
before, **1.78 / 1.88 / 1.86 MiB** after. Flat in work where it was linear. A genuinely
live sequence and a string-heavy loop are both unchanged.

### After N7, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| Reclaiming `Ints` and `Strs` | a memory constitution ✓ | done — N7 |
| Reclaiming `Str` | a representation that records provenance | done — N8. Four provenances, one of them an interior pointer, answered by a third field naming the owner |
| Reclaiming `Chan` | a rule for a value whose lifetime is a `scope`'s and whose holders are tasks | done — N8. The count is atomic, because it does cross; a waiter implies a live reference, which is what makes `rc == 0` sufficient |
| Reclamation in the self-hosted backend | the above, and a migration | done — N8. No source change was needed; the runtime is derived from the Rust emitter's and a gate refuses drift |
| User-defined records | a decision this milestone deliberately did not make | `research-register.md` R1, re-scoped: value semantics are a live question for a kind of value with no existing meaning to break |
| Ownership cycles | a recursive type existing at all | impossible today: no value can refer to another that can refer back |

### N8 — Complete current heap reclamation — **complete, 2026-09-22**

Every heap-backed type the language has is reclaimed, in the reference interpreter, in the
Rust compiler's output, and in the output of the compiler written in Nazm.

| | |
| --- | --- |
| **N8A–C** | The `Str` backing model — `{ pointer, length, owner }`, four origins classified rather than guessed at — string and channel reclamation, container element ownership, task-boundary ownership, and four runtime leaks that were nobody's feature |
| **N8D** | `compiler/emit.nz` migrated. Its runtime is **derived** from the Rust emitter's rather than transcribed, with a gate; its decisions are held by a differential memory test |
| **N8E** | Nine mutations, and the scaling measurements |
| **N8F** | The authorities |

The number that says what it was for: the accumulator shape from the 2026-09-21 incident,
at 8,000 iterations, fell from **70.66 MiB to 2.23 MiB**. 80,000 temporary strings against a
constant live set: 4.17 MiB → 1.77 MiB. 2,000 short-lived channels: 2.38 MiB → 1.80 MiB.
Sequences, a genuinely live sequence and a sieve are unchanged, which is the control. The
cost is about 13–16 ns per string operation and nothing measurable elsewhere.

`compiler/*.nz` needed **no source change**, which was the test of whether the constitution
is as ergonomic as it claims: memory semantics are transparent, and a milestone that had
required manual lifetime management in the compiler's own source would have been evidence
against the design rather than for it.

### After N8, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **User-defined records** | a decision about value semantics for a kind of value with no existing meaning to break | done — N9. Value semantics, composed field by field; cycles refused as an impossible layout rather than admitted with a policy |
| Variants, pattern matching, `Option`, `Result` | records ✓ | the shape of a payload is a record's question first, and it has been answered |
| Generics | records ✓ and variants | nothing to be generic over yet |
| A lossless CST | nothing in this list | the last AI-native item, and independent of all of it |
| `nazm explain-cost` | the cost table in `spec.md` ✓ | the model is documented; the tool is not written, and is not urgent |

### After N9, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **Variants and pattern matching** | records ✓ | done — N10. A discriminant, an exhaustiveness rule and a binding form, and copy and release that act on the active variant alone |
| Generic containers | records ✓ and variants | a `Vec<Record>` is where value semantics usually start to hurt, and it is also what would make a reference cycle expressible again — at which point `spec.md`'s *Cycles* has to be satisfied a second time |
| Copy elision, specified | records ✓ | `research-register.md` R1's original sharp question, still unanswered: a record copy costs one reference adjustment per owning field, and nothing says when the compiler may remove the pair |
| A lossless CST | nothing above | unchanged; still independent, still the last AI-native item |

### After N10, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **Generic containers** | records ✓ and variants ✓ | `Vec[T]` is now named by two findings rather than argued for: it is what the self-hosted compiler needs before an enum can replace a tagged integer, and it is where value semantics usually start to hurt. It is also what would make a reference cycle expressible again, at which point `spec.md`'s *Cycles* has to be satisfied a second time |
| `Option`, `Result`, typed errors | generics | expressible monomorphically today — `enum MaybeInt { None, Some(value: Int), }` — which is the evidence the kernel is ready and also the reason not to bless a built-in one before it can be written once |
| Copy elision, specified | records ✓ and variants ✓ | R1's original sharp question, still unanswered, and now slightly larger: a copy costs one reference adjustment per owning field and, for an enum, a branch as well. Nothing says when either may be removed |
| A wildcard arm and `non_exhaustive` | variants ✓ | deliberately absent. Adding a variant should break importers while the language is young; when it should stop doing that is a design decision, not an omission to fix |
| A lossless CST | nothing above | unchanged; still independent, still the last AI-native item |

### After N11, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **`Result`, `Option` and typed error propagation** | generics ✓ | expressible now as ordinary generic enums, which is the evidence the kernel is ready. What is missing is not the type but the decisions around it: where a standard definition lives when there is no standard library, whether `?`-style propagation is syntax, and what a failure *is* when `N0405` already stops the program. N11 deliberately left all three alone |
| Traits, or any constraint on `T` | generics ✓ | a generic body can do only what every type can, which is the honest limit of N11. A constraint system is a large decision about dispatch, coherence and orphan rules and should be forced by a program that needs `==` or ordering on `T`, not by symmetry |
| Copy elision, specified | records ✓, variants ✓, generics ✓ | R1's sharp question is larger again: a `Vec` of records copies each element on `vec_get`, by `T`'s law, and nothing says when the pair may be removed |
| A compact enum representation | variants ✓, `Vec[T]` ✓ | an enum is one slot per variant, and a `Vec[Enum]` multiplies that by its length. `performance.md` measures the pressure; nothing here claims it is acceptable at scale |
| A lossless CST | nothing above | unchanged; still independent, still the last AI-native item |

### After N12, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **Blocks as values in `nazm build`, and derived equality on records and enums** | records ✓, variants ✓ | the two walls both dogfoods hit. N11 could not turn the compiler's integer tags into an enum because an enum has no `==`; N12 could not turn a single `-1` sentinel into an `Option` because a `match` arm cannot be a block in the native subset, so the only spelling converts straight back to the sentinel. The interpreter already has blocks as values; the native backends refuse them, which is also why an explicit early return from an arm is written `if true { return … } else { return … }` there |
| `?` on `Option`, and conversion between error types | traits | both need a protocol — a carrier `?` can dispatch on, a conversion it can call — and a protocol is a trait. N12 refused both rather than special-case them |
| Traits, or any constraint on `T` | generics ✓ | unchanged: a constraint system is a large decision and should be forced by a program that needs `==` or ordering on `T`. Derived equality on *concrete* types is the smaller step that does not need one |
| Mapping a typed error to a process status | `Result` ✓ | `main` still returns `Int`. Letting it return a `Result` is a contract with the operating system, not a type rule, and nothing has needed it yet |
| A compact enum representation | variants ✓, `Vec[T]` ✓ | `Option[Str]` and every `Result` are now common, and each is one slot per variant; `performance.md` measures them |
| Copy elision, specified | records ✓, variants ✓, generics ✓ | unchanged, and `?` adds one retain/release pair per success it passes through |
| A lossless CST | nothing above | unchanged; still independent |

*N12.1 took the first half of the first row.* Blocks as values compile natively in both
compilers, and the `-1` sentinel became an `Option`. Derived equality is what is left of it.

### After N12.1, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **Derived equality on records and enums** | records ✓, variants ✓ | the wall N11's dogfood hit and N12.1's did not remove: the compiler's node kinds, token kinds and frame kinds are integers compared with `==` or `!=` at 463 sites in `compiler/*.nz` (`kind == n_block()`), and an enum cannot replace them while it has no `==`. Rewriting every comparison as a `match` would be the workaround this milestone refused. Needs no trait: equality on a *concrete* nominal type, derived from its fields as copy and release already are |
| `?` on `Option`, and conversion between error types | traits | unchanged |
| Traits, or any constraint on `T` | generics ✓ | unchanged |
| Mapping a typed error to a process status | `Result` ✓ | unchanged |
| A compact enum representation | variants ✓, `Vec[T]` ✓ | unchanged; `Option[Int]` now appears inside the compiler too |
| Copy elision, specified | records ✓, variants ✓, generics ✓ | unchanged |
| A lossless CST | nothing above | unchanged; still independent |

### After N21, in dependency order

No item is chosen here; N21 left these standing.

| | Depends on | Why not before |
| --- | --- | --- |
| Document and workspace symbols | a language service ✓, structure ✓ | not started |
| Semantic tokens, code actions | a language service ✓ | not started |
| Call hierarchy | checked calls ✓, references ✓ | not asked for |
| Completion after a bare `.` or `(`, and a `.` trigger | structure ✓ | needs an anchor for source that does not parse |
| Rename of exported definitions; edits of files not open | rename ✓ | unchanged since N18 |
| MCP, semantic context packets | a language service ✓, scope, call and structure at a position ✓ | not started |
| Incremental reparse | a lossless CST ✓ | N21 added nothing measurable to a warm edit; still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N21 — Typed structure at a position, structure completion, constructor signature help — **complete, 2026-09-26**

Which record, enum or variant a field, variant or label position belongs to is now answered
from identities the checker already recorded, plus one fact it recorded nowhere before — the
type an unresolved member name was looked for on, written only where a lookup fails. Completion
of fields, variants and labels, and signature help for record and variant constructions, are
that answer. No language, interface, cache-key or bootstrap change: `SEMANTIC_EPOCH` 7,
`nazm.interface/5`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| `Resolution::looked_up_on`, written where a field or variant lookup fails | `crates/nazm-sema/src/resolve.rs`, `crates/nazm-core/src/check.rs`, `architecture.md` §7.23 |
| `structure_at`, structure completion, `constructor_help`, `help_at` | `crates/nazm-service/src/structure.rs`, `crates/nazm-service/src/signature.rs` |
| fields as Field, variants as EnumMember, constructor help over `textDocument/signatureHelp` | `crates/nazm-cli/src/lsp.rs` |
| what it costs: nothing measurable to checking, editing or memory; queries in microseconds, whole-compiler queries dominated by finding the token | `performance.md` |

Mutation evidence: 15 new, all 15 direct killers verified on the final source; one targeted
campaign of 34, all caught at tier 1, over three sessions (`capability-matrix.md` §30).

**N22 has not started.**

### After N20, in dependency order

No item is chosen here; N20 left these standing. *Superseded by "After N21" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Member, variant-dot, construction-label and payload-label completion; signature help for constructions | completion ✓, signature help ✓, field identity ✓ | each is a structure-aware contract — the type before `.`, the record being built — not a call |
| Document and workspace symbols | a language service ✓ | not started |
| Semantic tokens, code actions | a language service ✓ | not started |
| Call hierarchy | checked calls ✓, references ✓ | a consumer of the records N20 added, and not asked for |
| Rename of exported definitions; edits of files not open | rename ✓ | unchanged since N18 |
| MCP, semantic context packets | a language service ✓, scope and call at a position ✓ | not started |
| Incremental reparse | a lossless CST ✓ | N20 measured a warm edit of the whole compiler at 84.7–86.4 ms on one CPU, 3–5 ms over N19; still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N20 — Call-site semantic query and signature help — **complete, 2026-09-26**

Which call a position is in, and with which signature, is now answered from what the checker
recorded when it checked the call — the resolved callee, the signature its arguments were
checked against, what a generic call settled — with the tree deciding only which argument list
and which argument. Signature help is that answer rendered by the one signature renderer hover
and completion also use. No language, interface, cache-key or bootstrap change:
`SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| `CheckedCall` and `Instantiation`, written by the checker | `crates/nazm-sema/src/resolve.rs`, `crates/nazm-core/src/check.rs`, `architecture.md` §7.22 |
| `call_at`, `signature_help`, the structured `Signature` and its one renderer | `crates/nazm-service/src/signature.rs` |
| `textDocument/signatureHelp`, triggered by `(` and `,`, UTF-16 label offsets | `crates/nazm-cli/src/lsp.rs` |
| what it costs: 6 µs a query on the compiler; 2–5 ms more `analyse`, 3–5 ms more per warm edit, 9 MB more resident | `performance.md` |

Mutation evidence: 14 new, 1 renamed, 1 candidate found equivalent and left uncatalogued; all 16
direct killers verified on the final source; one targeted campaign of 23, all caught at tier 1
(`capability-matrix.md` §30).


### After N19, in dependency order

No item is chosen here; N19 left these standing. *Superseded by "After N20" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Signature help | scope at a position ✓ | needs the call being written and which argument, which the scope does not say |
| Member, variant-dot, construction-label and payload-label completion | completion ✓, field identity ✓ | each is a structure-aware contract — the type of what precedes `.`, the record being built — not lexical scope |
| Document and workspace symbols | a language service ✓ | not started |
| Semantic tokens, code actions | a language service ✓ | not started |
| Rename of exported definitions; edits of files not open | rename ✓ | unchanged since N18 |
| MCP, semantic context packets | a language service ✓, scope at a position ✓ | not started |
| Incremental reparse | a lossless CST ✓ | N19 measured a warm edit of the whole compiler at about 81 ms on one CPU; the evidence that would justify it still does not exist |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N19 — Semantic scope-at-position and context-aware completion — **complete, 2026-09-25**

What may be written at a position is now recorded by the checker as it decides it — frames,
visibility, the binding each hides, type parameters, module environments less the names it
refused — and read back without resolving anything; completion of a value, a call head or a
type is that answer, filtered by what is written, for the current document's compilation only.
No language, interface, cache-key or bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`,
fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| the scope trace, written by the checker; `visible_at` | `crates/nazm-sema/src/scope.rs`, `crates/nazm-core/src/check.rs`, `architecture.md` §7.21 |
| which files needed recovery | `Analysis::syntax_errors` in `crates/nazm-core/src/lib.rs` |
| `scope_at`, `completion`, contexts, completeness | `crates/nazm-service/src/completion.rs` |
| `textDocument/completion`, invoked, whole-identifier replacement in UTF-16 | `crates/nazm-cli/src/lsp.rs` |
| what it costs: 0.7 ms a scope query and 2.1 ms a completion on the compiler | `performance.md` |

Mutation evidence: 14 new, 1 repointed, 1 renamed; all 16 direct killers verified on the final
source; one targeted campaign of 17 — 16 caught at tier 1 and 1 at tier 2, no survivor
(`capability-matrix.md` §30).


### After N18, in dependency order

No item is chosen here; N18 left these standing. *Superseded by "After N19" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Rename of exported definitions | rename ✓ | needs a provably complete set of importers — a finite project boundary the service does not have; N18 refuses rather than guess |
| Edits of files that are not open | rename ✓ | the protocol cannot make a closed file's content a precondition; needs an application path that can, or a client contract that does |
| Other code actions, structured edits beyond rename | a lossless CST ✓, a plan model ✓ | each needs its own semantic contract |
| Incremental reparse | a lossless CST ✓ | N18 measured a warm edit of the whole compiler at about 83 ms and a validated rename at about 93 ms on one CPU; the evidence that would justify it still does not exist |
| Completion, signature help | a language service ✓ | each needs its own semantic contract — what is in scope at a position — which the resolution does not record |
| Symbols (document, workspace) | a language service ✓ | not started |
| Semantic tokens | a language service ✓ | unchanged |
| MCP | a language service ✓ | not started |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N18 — Safe semantic rename and stale-safe edit plans — **complete, 2026-09-25**

Every written user-type name and enum qualifier is now an occurrence of the definition it
names, which makes the occurrence set complete for every kind of entity; on that, a rename is a
validated plan — the entity's recorded occurrences only, re-checked in memory, partition-
preserving, tied to the exact text and version it was made from — for locals and private
definitions, served as a versioned `WorkspaceEdit`. No language, interface, cache-key or
bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| the checker records every written type name and qualifier; `Enum` is an entity | `crates/nazm-core/src/check.rs`, `crates/nazm-sema/src/{resolve,references}.rs`, `architecture.md` §7.20 |
| coverage per kind, exhaustive; a corpus gate for new positions | `ReferenceTarget::rename_coverage`, `crates/nazm-service/tests/references.rs` |
| the plan, its scope rule, its validation and its preconditions | `crates/nazm-service/src/rename.rs` |
| `prepareRename`, `rename`, versioned `documentChanges` only | `crates/nazm-cli/src/lsp.rs` |
| what it costs: a rename of the compiler's private function in about 93 ms | `performance.md` |

Mutation evidence: 14 new, all 14 killers verified; one targeted campaign of 18 — 18 caught at
tier 1, no survivor (`capability-matrix.md` §30).

**N19 has not started.**

### After N17, in dependency order

No item is chosen here; N17 left these standing. *Superseded by "After N18" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Safe structured edits, rename | a lossless CST ✓, references ✓ for the entities whose coverage is complete | N17's occurrence set is complete for functions, locals, variants, fields and payload fields, and turning it into validated edits — and deciding what a rename across files not open means — is its own contract. It is **not** complete for type names: a record's name written in an annotation, and the enum name in `E.V(…)`, have no reference identity, so a record or enum rename would miss them. Safe rename is possible only for an entity whose coverage is complete; record and enum rename depends on the checker recording type-name occurrences first. The milestone that adds rename must either add that prerequisite first or refuse rename for every category whose occurrence set is incomplete |
| Incremental reparse | a lossless CST ✓ | N17 measured a warm edit of the whole compiler at about 81 ms on one CPU, index included; the evidence that would justify it still does not exist |
| Completion, signature help | a language service ✓ | each needs its own semantic contract — what is in scope at a position — which the resolution does not record |
| Symbols (document, workspace) | a language service ✓ | not started |
| Type names in annotations as references | references ✓ | the resolution records no entity at a written type name; the checker would have to record one |
| Semantic tokens, code actions | a language service ✓ | unchanged |
| MCP | a language service ✓ | not started |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N17 — Semantic reference index and references query — **complete, 2026-09-25**

Given an occurrence that resolves to an entity, where every use of that same entity is in the
current program snapshot: one reference index per analysis, derived from the resolution alone,
behind `LanguageService::references` and `textDocument/references`. No language, interface,
cache-key or bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| what a reference is, and the index that reads the resolution backwards | `crates/nazm-sema/src/references.rs`, `architecture.md` §7.19 |
| a variable use records which function's slot it is; a construction label, which field | `LocalRef` in `crates/nazm-sema/src/resolve.rs`, `crates/nazm-core/src/check.rs` |
| definition and references as one lookup; the query over every open compilation | `crates/nazm-service/src/service.rs` |
| `textDocument/references`, `includeDeclaration`, UTF-16 at the boundary only | `crates/nazm-cli/src/lsp.rs` |
| what it costs: 3.8 ms of index per compiler analysis, 1.8 ms a query | `performance.md` |

Mutation evidence: 16 new and 1 repointed, all 17 killers verified uncontended; one targeted
campaign of 19 — 19 caught at tier 1, no survivor (`capability-matrix.md` §30).

**N18 has not started.**

### After N16, in dependency order

No item is chosen here; N16 left these standing. *Superseded by "After N17" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Incremental reparse | a lossless CST ✓ | N16 measured a warm edit of the whole compiler at about 80 ms on one CPU; the evidence that would justify it does not exist yet |
| Structured edits | a lossless CST ✓, a language service ✓ | no edit command exists |
| Completion, signature help | a language service ✓ | each needs its own semantic contract — what is in scope at a position — which the resolution does not record |
| References, rename | a language service ✓ | need every use of an identity, across modules not open |
| Semantic tokens, code actions | a language service ✓ | unchanged |
| MCP | a language service ✓ | not started |
| `nazm explain-cost` | nothing above | independent |

### N16 — Language service core and minimal LSP — **complete, 2026-09-25**

`nazm lsp` serves diagnostics, definition and hover over an editor's unsaved buffers, as an
adapter over a protocol-independent language service that answers from the loader, parser,
checker and resolution every command uses. No language, interface, cache-key or bootstrap
change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| one loader for the CLI and the editor, reading bytes through a `Sources` | `crates/nazm-service/src/load.rs` (moved from `nazm-cli`) |
| open buffers, versions, one current analysis per document | `crates/nazm-service/src/service.rs`, `architecture.md` §7.18 |
| one parse-and-check pipeline that also hands back its resolution | `nazm_core::analyse` |
| a local's definition by slot, not by spelling | `LocalDef::span` in `crates/nazm-sema/src/resolve.rs` |
| the protocol shell, UTF-16 at the boundary only | `crates/nazm-cli/src/lsp.rs` |
| what it costs: about 80 ms per warm edit of the whole compiler on one CPU | `performance.md` |

Mutation evidence: 14 new, all 14 killers verified uncontended; one targeted campaign of 15 —
15 caught (14 at tier 1, 1 at tier 3, then tier 2 once the moved loader was given back to the
`cli` profile), no survivor (`capability-matrix.md` §30).

**N17 has not started.**

### After N15, in dependency order

No item is chosen here; N15 left these standing.

| | Depends on | Why not before |
| --- | --- | --- |
| A language server | units ✓ + durable identity ✓ + reuse ✓ + a lossless CST ✓ | every listed blocker is met; it must read the checker's results rather than re-implement them |
| Incremental reparse | a lossless CST ✓ | N15 parses whole files; the tree is shaped for subtree reuse and does none |
| Structured edits | a lossless CST ✓ | the tree finds and reproduces a node by position (N15's test); no edit command exists |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| A requirement on a record's or an enum's parameter | a requirement ✓ | unchanged |
| Cross-unit inlining of generic instances | native units ✓ | unchanged |
| `?` on `Option`, and conversion between error types | traits | unchanged |
| A compact enum representation | variants ✓, `Vec[T]` ✓ | unchanged |
| Copy elision, specified | records ✓, variants ✓, generics ✓ | unchanged |

### N15 — Lossless CST and local error recovery — **complete, 2026-09-25**

One lossless scan feeds the one parser, which builds the abstract tree and a lossless
concrete tree from the same decisions. The tree reproduces every input byte — whitespace,
comments, punctuation, refused characters — and a malformed statement no longer costs the
rest of its function: it becomes an error node, and what follows it is still syntax. No
language, interface, cache-key or bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`,
fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| one lossless scan; the parser's tokens and the formatter's comments are views of it | `crates/nazm-syntax/src/lexer.rs` |
| the tree, its kinds, trivia placement, a wrapper that keeps `cstree` inside the crate | `crates/nazm-syntax/src/cst.rs`, `architecture.md` §7.17 |
| one parser for both trees; statement-level recovery; cascades withheld | `crates/nazm-syntax/src/parser.rs` |
| losslessness, recovery, the two trees agreeing, termination | `crates/nazm-syntax/tests/cst.rs`, `capability-matrix.md` area 1 |
| what it costs: 2.0× parse time on real code | `performance.md` |

Mutation evidence: 16 new and 3 repointed; the 17 of those 19 that list a killer (fourteen
distinct tests) verified, uncontended; one targeted campaign of the
26 entries on the syntax crate — 25 caught (19 at tier 1, 6 at tier 2), no survivor, and the
known `nesting-unbounded` MUTANT CRASH unchanged (`capability-matrix.md` §30).

**N16 has not started.**

### After N14, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **A lossless CST** | nothing above | still independent, still the last AI-native item, and now the largest item nothing blocks |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | N14 covered the one abstraction a program needed with a built-in requirement, and its dogfood found no code wanting another. The next capability should be forced by code that needs behaviour the language cannot derive — an ordering, a conversion between error types — not by the symmetry of having one bound |
| A requirement on a record's or an enum's parameter | a requirement ✓ | refused today; would need every instance checked wherever it is written, and no program needs it |
| Cross-unit inlining of generic instances | native units ✓ | measured by N14: an instance lives in its own unit, so `same[Int]` costs a call a hand-written `same_int` does not (`performance.md`) — with or without a requirement |
| `?` on `Option`, and conversion between error types | traits | unchanged |
| A compact enum representation | variants ✓, `Vec[T]` ✓ | unchanged |
| Copy elision, specified | records ✓, variants ✓, generics ✓ | unchanged |

**Recommended next: a lossless CST** — the item nothing blocks and nothing has displaced, now
that the equality wall is gone. *N15 took it (above).* Not traits: no program here yet needs user-defined behaviour
abstraction, and N14's evidence is that the built-in requirement met the need N13 exposed.

### N14.1 — Contained performance calibration — **complete, 2026-09-25**

Tooling, between N14 and whatever follows it; no language, compiler, interface, cache-key or
bootstrap change (`SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`). A contained
workload now runs under a named execution profile — CPU quota, Cargo jobs and test threads
as one value — inside the same memory, swap, pid, storage and deadline ceilings. The
workspace suite and the selfhost suite default to P4T4 (**3.03×** and **3.44×**, measured);
everything else stays P1, mutation because both parallel profiles tried crossed the frozen
total-memory gate with identical verdicts.

| Settled | Where |
| --- | --- |
| `ExecutionProfile`, P1 and P4T4, one owner for quota, jobs and threads; `--profile` | `xtask/src/contained.rs`, `xtask/src/main.rs` |
| the measurements, the gates frozen before selection, why P6 and parallel mutation were refused | `docs/performance.md`, *Development and evidence pipeline parallelism* |
| a campaign's identity includes its parallelism and CPU quota; a resume under another profile reuses nothing | `xtask/src/campaign.rs`, `xtask/tests/lifecycle.rs` |
| the serial rule, what was relaxed and what was not | `docs/runbook.md`, *Serial execution* |

*No language milestone has started since N14.*

### N14 — A type parameter may require equality — **complete, 2026-09-25**

`fn same[T: Equality](a: T, b: T) -> Bool { a == b }` is valid; without `: Equality` it is
still refused. Every argument, written or inferred, concrete or a caller's own parameter, is
checked against the requirement by N13's one derivation, so `Box[T]`, `Option[T]` and
`Result[T, E]` with both required compare in a bounded body and `Vec[Int]` never satisfies it.

| Settled | Where |
| --- | --- |
| the rule, one built-in capability, functions only, statically checked | `docs/spec.md`, *A type parameter may require equality* |
| one set of required parameters, read by N13's derivation and nowhere else | `crates/nazm-sema/src/udt.rs`, `generic.rs`, `architecture.md` §7.16 |
| call satisfaction, forwarding, `N0364`/`N0365` | `crates/nazm-core/src/check.rs` |
| `nazm.interface/5`, requirements persisted by position; `SEMANTIC_EPOCH` 6 → 7 | `crates/nazm-iface/src/wire.rs`, `crates/nazm-cache/src/identity.rs` |
| nothing at run time | `crates/nazm-cli/tests/constraints.rs` |
| the same rule in the compiler written in Nazm; a new fixpoint, `0e1a40e6…` | `compiler/`, `bootstrap.md` |

Mutation evidence: 22 new and 6 repointed, 27 killers verified; one targeted campaign of 40 —
40 caught (36 at tier 1, 4 at tier 2), no survivor (`capability-matrix.md` §30). No honest
dogfood candidate: the compiler has no generic helper that needs a capability.

**N15 has not started.**

### After N13, in dependency order

| | Depends on | Why not before |
| --- | --- | --- |
| **Constrained generics — a way for a definition to require a capability of `T`** | generics ✓, derived equality ✓ | N13's finding, and the first program that needs one: a concrete `Box[Int]` compares, and `fn same_box[T](a: Box[T], b: Box[T])` cannot, because nothing can say "`T` has equality". Equality is the one capability the language now derives for concrete types that a generic body cannot ask for. What the mechanism is — a trait, a built-in bound, something else — is the milestone's decision, not this table's |
| `?` on `Option`, and conversion between error types | traits | unchanged |
| Sequence and channel equality | a decision | still unmade on purpose: same storage or same contents. N13 refuses it and everything containing it |
| A compact enum representation | variants ✓, `Vec[T]` ✓ | unchanged; an equality helper reads one slot of the one-slot-per-variant aggregate |
| Copy elision, specified | records ✓, variants ✓, generics ✓ | unchanged |
| A lossless CST | nothing above | unchanged; still independent |

**Recommended next: constrained generics** — recommended from N13's evidence, not
committed to. *N14 took it (above).* The alternatives are unchanged and smaller: a lossless CST, and
`nazm explain-cost`.

### N13 — Derived structural equality — **complete, 2026-09-25**

`==` and `!=` apply to two operands of one type when that type has equality, and for a
record, an enum or a concrete generic instance it is **derived**: every field, every payload
field of every variant, after substitution. `Option` and `Result` get it as the ordinary
enums they are. `Vec`, `Ints`, `Strs`, `Chan` and an unconstrained `T` do not have it, and
neither does anything that contains one.

| Settled | Where |
| --- | --- |
| the rule, nominal and static over every variant | `docs/spec.md`, *Equality is derived* |
| one derivation beside cleanup and task safety, independent of both | `crates/nazm-sema/src/udt.rs`, `architecture.md` §7.15 |
| the interpreter's relation, and a native helper per compared type — tags first, then the active slot alone | `crates/nazm-core/src/eval.rs`, `crates/nazm-lir/src/emit.rs` |
| the same derivation and helpers in the compiler written in Nazm | `compiler/analyse.nz`, `compiler/emit.nz` |
| `SEMANTIC_EPOCH` 5 → 6; interface schema unchanged at `nazm.interface/4` | `crates/nazm-cache/src/identity.rs` |
| the compiler's completion states an enum, compared with `==` | `compiler/analyse.nz`, `performance.md` |
| a new fixpoint, `76598c43…` | `bootstrap.md` |

Mutation evidence: 33 new mutations and 13 repointed, each with its regression test; one
targeted campaign of 58 — 57 caught (45 at tier 1, 12 at tier 2, none at tier 3) and one
genuine survivor, a use-after-free no supported configuration can observe, kept live as a
known limitation (`capability-matrix.md` §30).

*N14 followed it (above).*

### N12.2 — Mutation harness v2 — **complete, 2026-09-25**

Evidence infrastructure between N12.1 and N13; no language, compiler, interface, cache-key
or bootstrap change. A mutant now stops at the first tier that sees it — a verified killer,
then a focused profile, then the workspace suite — and only the workspace suite can report
SURVIVED. The same 27 N12.1 mutants took **523 s** instead of 10,782 s, and the **whole
catalogue ran for the first time**: 233 of 233 accounted for in 10,686 s.

| Settled | Where |
| --- | --- |
| tiered escalation, the full strategy kept as the oracle, identical verdicts on a differential sample | `xtask/src/campaign.rs`, `docs/capability-matrix.md` §30 |
| typed killer and profile metadata, validated in `cargo xtask check`; `--verify-killer`, `--plan` | `xtask/src/plan.rs`, `xtask/mutations/mutations.toml` |
| one subprocess runner: own process group, tree kill at a deadline, a watchdog if the harness dies | `xtask/src/procs.rs` |
| a journal bound to a campaign identity, `--resume`, bounded sessions | `docs/runbook.md` "Mutation campaigns" |
| the N13 mutation-authoring workflow: regression test, entry, verified killer | `docs/runbook.md` |
| found on the way: N12.1's group kill did nothing in the container, `memory.rs` had no deadline, two catalogue entries had drifted, one was equivalent | `docs/capability-matrix.md` §30 |

*N13 followed it: its mutations arrived with their regression tests and killers verified by
`--verify-killer` (above).*

### N12.1 — Core prelude identity and native value-block parity — **complete, 2026-09-24**

No new source concept. The native backends and the compiler written in Nazm now compile
every block the language already accepted as a value, and that compiler finds the core
prelude by its key rather than by its position.

| Settled | Where |
| --- | --- |
| one lowering for a value block; `Expr::Block` and `Stmt::Match`; the join and ownership at a block's end | `architecture.md` §7.14, `crates/nazm-lir` |
| `{` as an operand, `blockexpr`, and its completion, in the compiler written in Nazm | `compiler/parse.nz`, `compiler/analyse.nz`, `compiler/emit.nz` |
| the prelude keyed `@core/prelude` and resolved once; a project path cannot hold the key | `compiler/module.nz`, `compiler/analyse.nz` |
| `SEMANTIC_EPOCH` 5 and `nazm.interface/4`, both unchanged: no accepted program and no persisted shape moved | `crates/nazm-cache/src/identity.rs`, `crates/nazm-iface` |
| the first `Option` inside the compiler, replacing a `-1` sentinel | `compiler/analyse.nz`, `record_owned`; `performance.md` |

### N12 — `Result`, `Option`, typed error values and `?` — **complete, 2026-09-24**

Typed failure as a value, in both compilers. `Result[T, E]` and `Option[T]` are ordinary
generic enums declared by a toolchain-owned **core prelude** that every module imports
implicitly; `?` propagates the core `Result` — recognised by definition, never by name or
shape — as a `match` with a `return` in one arm. No exceptions, no unwinding, no conversion.

| Settled | Where |
| --- | --- |
| the prelude, its identity and visibility, the reserved names, `?`'s rule, order, cleanup and ownership | `spec.md`, *Typed error values* |
| one module with a toolchain-owned key, attached last; `?` resolved once and lowered as a `match` | `architecture.md` §7.13 |
| `@core/prelude` is disjoint from every project key | `crates/nazm-sema/src/key.rs` |
| persisted interfaces share a definition's identity across one compilation | `crates/nazm-iface/src/wire.rs`, `rehydrate_into` |
| `nazm.interface/4` unchanged; `SEMANTIC_EPOCH` 5 | `crates/nazm-iface`, `crates/nazm-cache/src/identity.rs` |
| the compiler written in Nazm does all of it, loads the same prelude, and returns its front end as a `Result` | `compiler/*.nz`, `bootstrap.md` |

Two defects found on the way, both fixed and held by tests. **Rehydration gave every
interface its own copy of the types it mentioned**, so a `Result` carried by a dependency's
interface was not the prelude's: harmless while no two interfaces shared a type, decisive
for `?`. And a native build produced an empty unit and object for every module that
declared no function; it produces none now.

### N11 — Parametric generics and `Vec[T]` — **complete, 2026-09-23**

First-order parametric polymorphism, in both compilers. Records, enums and functions declare
type parameters; types are applied with `[…]`; a generic definition is checked **once**,
parametrically; call-site type arguments are written or inferred from the arguments, never
from the expected result. One generic container, `Vec[T]`, follows the sequence law: a
task-unsafe mutable handle whose elements are copied, replaced, appended and removed by `T`'s
own law.

| Settled | Where |
| --- | --- |
| syntax, identity, parametric checking, inference, `Vec[T]`, the ownership-cycle rule | `spec.md`, *Generics* |
| an applied type is an interned entry with substituted fields; completion is a worklist | `architecture.md` §7.12 |
| `nazm.interface/4`: parameters are positions, so renaming changes no byte | `crates/nazm-iface/src/wire.rs` |
| monomorphisation, one artefact per instance, symbols from canonical type keys, `N0357`/`N0358` | `crates/nazm-lir/src/instance.rs`, `symbol.rs` |
| an ownership cycle through a `Vec` is `N0359`; acyclic nesting and phantom parameters stay legal | `crates/nazm-sema/src/udt.rs` |
| the compiler written in Nazm does all of it, and holds its diagnostics in a `Vec[Diag]` | `compiler/*.nz`, `bootstrap.md` |

The finding that justified it came true in the dogfood: the self-hosted compiler's
diagnostics were four parallel arrays kept the same length by care, and are one `Vec` of
records now. The finding N11 leaves is the one above — a `Vec` of diagnostics still cannot be
returned as a failure.

The generic conformance corpus found one real defect in the reference backend on its first
run: a unit that declared an instance never carried the types in that declaration's
signature, so `or_else[Str]` taking a `Maybe[Str]` the caller never bound was declared with
`<enum>`. Fixed in `crates/nazm-lir/src/lower.rs`, held by
`a_call_carries_the_types_its_callee_is_declared_with`.

### N10.2 — Selfhost completion and diagnostic parity — **complete, 2026-09-23**

The two gaps N10.1 left, both in programs the reference refuses, closed before generics make
them expensive. No semantics changed; no accepted program changed meaning, and the Nazm
compiler's IR for every accepted program in the corpora is byte-identical to N10.1's.

The cause was the same one N10.1 named, one level on. The reference's `Completion` has three
states and the Nazm checker had two: N10.1 added *never completes*, and *completes with no
value* was still the type code `unknown`, which also meant "already reported". So a value
position given nothing was accepted — `let x = if c { 1 };` became `alloca void` — and an
`if` whose branches disagreed was typed by one of them and handed clang a mistyped `phi`.

| Settled | Where |
| --- | --- |
| one completion per node, `Value`, `Unit` or `Diverges`, never inferred from the type | `architecture.md` §7.11 |
| `N0300` for a value position given nothing, decided in one place, at the reference's span | `compiler/analyse.nz`, `needs_value` |
| the `if` join in the reference's order: a branch that leaves defers; two values must agree; a value with nothing is `N0302` | the same |
| a body and a `return` are held to the signature, which the Nazm checker had never done | the same |
| a `match` refused for its scrutinee checks no arm, as the reference's does not | the same |
| a `match` of no value discards the value its value arms produce, and gives it back | `compiler/emit.nz`, `discard_branch_value` |

The last is the one emitter change, and it is a consequence rather than a rule: the
reference accepts `match e { E.A() => int_to_str(i), E.B() => if c { … }, };` as a
statement, and once the checker stopped giving that `match` its value arm's type, that arm's
value had nowhere to go. The reference's *native* backend refuses the shape as outside its
subset; the Nazm compiler compiles it and agrees with the interpreter, strings reclaimed.

### N10.1 — Transfer expressions and selfhost parity — **complete, 2026-09-23**

A correction between N10 and N11, and no new language: `spec.md` is unchanged. N10's
evidence exposed one older divergence, and it had to close before the type system gets more
complex, because what it is about — whether an expression produced a value — is what every
generic container will ask of every element.

```nazm
let v = pick(int_to_str(i), if i == 1 { continue; } else { i });
```

The reference compiled it, and the compiler written in Nazm emitted `void %24` as the
second argument. The cause was **completion not being represented**: its checker kept one
integer per node, in which a statement, an error and a branch that leaves were all
`unknown`, and it typed an `if` by its then-branch; its emitter opened every join whether a
branch reached it or not. The repair is one bit per node in each — *never completes* in the
checker, *is the block live* in the emitter, asked in one place — and `architecture.md`
§7.11 says why that and not an enum.

The audit that followed found three more, and each is a transfer rule the reference already
had:

| Found | Since | Now |
| --- | --- | --- |
| a `break`, `continue` or `return` out of a `scope` did not join it, so a task's failure was lost and a value printed instead | N7, selfhost only | joined, innermost first, as the reference's `join_through` |
| a statement after one that leaves was compiled rather than refused (`N0313`) | always, selfhost only | refused, once per run, as the reference refuses it |
| a value that never arrives was `unknown` where no type is expected, so a `match` over one was accepted and a `let` of one had a slot of no type | always, selfhost only | `Int`, the reference's `value_type(e, None)` |

And one that was not the Nazm compiler's: `==` on two strings never gave its operands back,
in **both** native backends, with no transfer anywhere near it. The interpreter was right.

| Settled | Where |
| --- | --- |
| a transfer is never a value: no operand, no incoming, no store, no call is emitted for it | `architecture.md` §7.11 |
| interpreted and compiled, the compiler written in Nazm emits byte-identical IR for every case | `a_transfer_in_a_value_position_is_never_a_value` |
| the original reproducer is permanent, byte for byte, in the corpus and in the conformance set C2 and C3 compile | `crates/nazm-cli/tests/transfers/`, `compiler/conformance/transfer_argument.nz` |

What it does **not** settle is recorded in `bootstrap.md`: the self-hosted checker still has
no `Unit` rule (`N0300`, "this produces no value") and does not compare an `if`'s two branch
types (`N0302`). Both concern programs the reference refuses, so neither is a divergence in
the accepted language — but the compiler written in Nazm fails on them rather than refusing
them by name. *Both closed by N10.2, above.*

### N10 — Variants and exhaustive pattern matching — **complete, 2026-09-23**

`enum State { Ready, Done(code: Int), }` with `match`. Closed nominal sum types with named
payload fields, qualified construction, exhaustive variant matching with named payload
bindings, module visibility, a persisted shape, and native code — in the reference
compiler, the interpreter and the compiler written in Nazm.

The milestone asked whether the model N7–N9 built survives **the ownership question
becoming dynamic**. A record owns what all its fields own; an enum owns what the active
variant's fields own, which is a value the program carries rather than a fact about the
type. The answer is that `Owns` gained one variant and no consumer gained a case: copy and
release are still one call per site, and the branch lives in a generated helper — in the
emitted program, where the recursion already lived.

The asymmetry that fell out of it is worth keeping. *Needs cleanup* and *may cross into a
task* stayed static and quantify over **every** variant; copy and destroy became dynamic and
act on **one**. So `enum Work { None, Values(xs: Ints), }` may not cross into a task even
while it holds `None` — what a `spawn` is handed is a value of a type, and the guarantee is
about the type.

*Cycles* was satisfied the same way a second time, over one graph: records and enums form a
single inline-containment graph, and a cycle anywhere in it is refused (`N0336`). A choice
between leaves is a leaf.

| Settled | Where |
| --- | --- |
| `enum` and `match` as the keywords, `Token.Eof()` as the constructor, always parenthesised | `spec.md`, *Enums* |
| variant and payload order are not semantic — not in the type, the fingerprint, or the discriminant | `spec.md`; `architecture.md` §7.11 |
| every variant gets exactly one arm, and there is no wildcard — adding one breaks importers on purpose | `spec.md`, *Exhaustiveness* |
| the scrutinee is a value the `match` owns, released **after** the arm's result is established | §7.11 |
| the discriminant is written **first**, so cleanup after a failed initialiser dispatches on a valid tag | §7.11 |
| `nazm.interface/3`, closed over the payload types its public surface mentions | `architecture.md` §7.3, extended |

Evidence: the numbers are in the milestone report and in `bootstrap.md`; what they cover is
the contained suite, selfhost parity, a new C2 = C3 fixpoint, twenty-two new mutations, and
the interface and cache behaviour that variant addition is supposed to move. The compiler
written in Nazm uses an enum of its own — and making that one dispatch exhaustive found a
defect N9 had shipped, where a record passed to a task produced a wrong answer and leaked
its fields. A mutation that *survived* found a second one, older: leaving a half-evaluated
expression by `break`, `continue` or `return` gave back nothing it was holding, in both
backends, since N8. `spec.md`'s rule 4a now says it and `architecture.md` §7.11 says how.

### N9 — User-defined records — **complete, 2026-09-23**

`struct Point { x: Int, y: Int, }`. Nominal value records with named fields, named
construction, projection, field replacement through a projection path, nesting, module
visibility, a persisted shape, and native code — in the reference compiler, the interpreter
and the compiler written in Nazm.

The milestone was never about the syntax. It asked whether the memory constitution N7 and
N8 built could describe a type whose ownership is **composed** from arbitrary field types
rather than branched on, and the answer is in what did not change: `Owns` gained one
variant and no consumer gained a case. Cleanup, task safety, copy and destroy are all
derived from the fields, recursively.

The leak-freedom claim was tested rather than extended, and the constitution's *Cycles*
was satisfied the first way it offered: a record holds its fields by value, so a
containment cycle has no finite layout and is refused (`N0336`) before anything is lowered.
A composition of leaves is a leaf, so the argument survives — and a generic container able
to hold a record is what would end it.

| Settled | Where |
| --- | --- |
| `struct` as the keyword, *record* as the category, `Point(x: 1, y: 2)` as the constructor | `spec.md`, *Records* |
| field order is not semantic — not in the type, the fingerprint, or the layout | `spec.md`; `architecture.md` §7.10 |
| a projection borrows when its base does, and takes a reference when it does not | §7.10 |
| a failure releases the temporaries it was holding — §7.9's last open limitation | §7.10 |
| `nazm.interface/2`, closed over the field types its public surface mentions | `architecture.md` §7.3, extended |

Evidence: 936 tests over 55 suites contained, 22/22 selfhost, C2 = C3 over 12 conformance
cases with 6 multi-module, 15 new mutations all caught, and the compiler written in Nazm
now uses a record of its own.

---

## Long-running tracks

**Concurrency — C-4 and C-5, still not now.** What shipped is one POSIX thread per task and
nothing more. C-4 is `nazm-rt`: a scheduler, stacks, channels and a reactor, and **the one
crate allowed `unsafe`** — which means the workspace's `unsafe_code = "forbid"` becomes
`deny` with a narrow allowance that `xtask check` enforces. It depends on the memory model.
C-5 is measurement: spawn-to-first-instruction latency, context-switch cost, and task memory
as **RSS, not virtual reservation, with page size recorded**. No scheduler work starts before
C-4's prerequisite lands; a single-threaded compiler is a perfectly good compiler.

**AI-native — one item left.** The ~3k-token cheatsheet. The lossless CST shipped in N15;
the formatter, grammar artefact, schemas and fixes before it. **No
language keyword will be added to support the label**; if a construct earns its place on
semantics it earns it, and "AI-native" is not an argument for syntax.

**Backends.** LLVM IR as text stays the only backend. Cranelift is an ordering decision, not
a rejection — it becomes worth having when one of its four jobs (JIT, comptime evaluator,
REPL, hot reload) exists. None does.

## Deliberately not next

Effects · capabilities · provenance · traits · methods · closures · exceptions ·
M:N scheduling · a second backend · GPU or accelerator work · a package manager · a standard
library beyond the built-in table and the two-type prelude. Ownership, user-defined types,
pattern matching, generics and typed errors left this list as N7–N12 landed them. Typed
errors did not bring effects with them: a `Result` in a signature is a return type, not an
effect row, and effects remain MISSING.

Each has a home — `capability-matrix.md` for status, `research-register.md` for the ones
that are hypotheses, `NAZM_LANGUAGE_GOALS.md` for why they are wanted eventually. None is
blocked by the others in a way that forces it now, and **all of them are easier after N1**,
which is the argument for doing the small thing first.
