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

### After N48: the v1 foundation, and what is deliberately after it

N40–N48 complete the v1 foundation; `docs/limitations.md` is the one list of what it does not do.
What follows is **not started** and is bounded here so it is not mistaken for v1 scope:

| Post-v1 work | Depends on | Why not in v1 |
| --- | --- | --- |
| Function values, closures, traits | a calling convention for captures | changes the language; every layer below would move — N50 |
| Effect polymorphism, attenuation and revocation of capabilities | function values; a capability with a type argument | the coarse kinds are what N37 settled |
| Richer information flow: more sinks, declassification | N38's framework | one sink was enough to make the framework real |
| More targets and cross-compilation | a foreign linker and runtime per target | the host is the only verified one |
| A suspendable-task scheduler (M:N) | stack switching or compiled continuations | a worker pool would deadlock blocking programs (§7.46) |
| A JIT, SIMD, GPU/HPC | a reason; area 21 | none proposed |
| Pointers, strings and structs across the FFI | a layout promise and a borrowed-string type | scalars were enough to make the boundary real |
| Debug variables, a live-debugger workflow, Cranelift DWARF | `DILocalVariable` from MIR locals | function and statement positions were the declared scope |
| A package registry and version ranges | a resolver that chooses | exact path dependencies choose nothing |
| Formal methods | a semantics to state them against | area 19's acceptance is an assessment, not a test count |

### After N75: N76–N100, closing what is PARTIAL or RESEARCH

One milestone at a time, each constitution first, each ending with its own evidence and the
capability row decided against that evidence rather than its title. The order is
`docs/nazm_N76_N100_FULL_prompts/MASTER_EXECUTION_ORDER.md`'s: the front end (N76–N80); execution,
runtime and interoperability (N81–N85); embedded, assurance and AI/HPC (N86–N88); tooling,
ecosystem, evidence, platforms and contracts (N89–N96); formal trust, the compiler written in Nazm
and the interactive tier (N97–N99); and N100, an audit of the whole catalogue that may fail
honestly. A row stays PARTIAL, RESEARCH or BLOCKED when that is what its evidence says.

### N85 — FFI and ABI v3 — **complete as scoped, 2026-10-04; area 17 VERIFIED as scoped**

`architecture.md` §7.86 first. A practical C ABI subset: opaque handles (`extern "C" struct Db;`),
C-layout structs by borrowed pointer, nullability in the result type (`N0409` or `None`), `Str`
results copied, `c_errno()`, exports passed to C as callbacks, `nazm build --lib --shared`, and
`bindgen` reading all of it. The crossing is written as ordinary Core IR, so MIR, LIR and both
backends learned only the borrowed struct copy, the `errno` capture and an export's address. Semantic
epoch 25, `nazm.interface/9`, runtime ABI 12. Not here: floats, by-value structs, variadics, closures
as callbacks, `dlopen`, other ABIs, the compiler written in Nazm. Next: N86, embedded and bare metal
v2.

### N84 — the standard library 1.0 — **complete as scoped, 2026-10-04; area 16 VERIFIED as scoped**

`architecture.md` §7.85 first. Seventeen modules and 126 public items, frozen by `library/std/API-1.0`
and a semver policy in `spec.md`; four representative programs (a CLI tool, a data tool, a concurrent
service, a package utility) written against it alone and run on every tier. Not here: networking,
cryptography, floats, a faster map, `@std` in the compiler written in Nazm. Next: N85, FFI and ABI v3.

### N83 — the task pool — **complete as scoped, 2026-10-04; area 15 VERIFIED as scoped**

`architecture.md` §7.84 first. `NAZM_SCHEDULER=pool`: tasks on bounded workers, each a guarded stack
pinned to its worker, a per-target switch, every wait parking the task; ten thousand tasks alive at
once on two workers; the whole CLI suite passes on the pool. Faster spawn (7.4 µs against 25.4 µs).
Not here: the default stays a thread per task, no work stealing or preemption, no hand-off for a
blocking foreign call, Linux unverified, the interpreter and the compiler written in Nazm unchanged.
Runtime ABI 11. Next: N84, the standard library 1.0.

### N82 — runtime v2: the runtime as an artifact — **complete as scoped, 2026-10-03; area 13 stays PARTIAL**

`architecture.md` §7.83 first. `nazm runtime build` compiles the whole runtime for a target and
profile into `libnazmrt.a` with a `nazm.runtime/1` manifest; `nazm build --runtime` links it on both
backends after checking schema, ABI revision, runtime digest, target, profile, services and archive
bytes, and refuses by name otherwise; `nazm runtime verify`; provenance names the archive. Behaviour is
identical over the repository; a cold build is 34 ms faster with it. Not here: a runtime in Rust (one
implementation kept), an allocator, a scheduler (N83). Next: N83, the advanced scheduler.

### N81 — the backend contract v2, stage one — **not accepted, 2026-10-03; area 10 stays PARTIAL**

`architecture.md` §7.82 first, and it says why: LIR now owns a target's data layout, every byte
layout and the ownership table, both backends read them, every native build validates them, and
`nazm lir` prints them as `nazm.lir/1`; the fuzzer exercises layouts on every tier. Objects are
byte-identical on every program in the repository. Not done, by decision: one instruction-level LIR
that both backends consume, with an oracle interpreter — a rewrite of both code generators that this
programme does not risk the reference compiler for. Next: N82, runtime v2.

### N80 — provenance v3 — **complete as scoped, 2026-10-03; area 7 stays PARTIAL**

`architecture.md` §7.81 first. Containers by type-keyed cell, calls through values and bounds by
type-based targets, closures' parameters and captures received, fields and variants within a body,
one policy engine with `checked-paths` in `cyber`, and a chain per origin at every sink
(`nazm.flow/2`). Epoch 24, `nazm.check/4`. No program in the repository changed verdict. Not here:
implicit flow, per-handle precision, labels a program writes. Next: N81, LIR and the backend
contract v2.

### N79 — effects and capabilities v3 — **complete as scoped, 2026-10-03; areas 5 and 6 stay PARTIAL**

`architecture.md` §7.80 first, amended as built. The ambient bridge became a recorded contract: each
function's inherited authority is computed, recorded and shown by `nazm inspect`, and the
`authority` profile's `explicit-authority` refuses it by name. `OutCap` attenuates `IoCap` to the
standard streams, by passing; nothing widens it back. A function value with fewer effects than
expected is accepted where one is passed or returned. Revocation is declared outside the model.
Epoch 23. Not here: refusing the bridge by default (the corpus and the compiler written in Nazm
rely on it), per-resource capabilities, subsumption inside other types. Next: N80.

### N78 — traits and methods — **complete as scoped, 2026-10-03; area 3 stays PARTIAL**

`architecture.md` §7.79 first. Nominal traits, `impl Trait for Type`, `recv.m(…)`, `Trait.m(recv, …)`
and trait bounds; coherence over the compilation (one impl per trait and type, in the trait's or the
type's module); methods resolved before Core IR — a concrete call is the impl's function, a call
through a bound is `CallTrait`, resolved per instance by MIR and per value by the interpreter. The
same results in the interpreter, LLVM and Cranelift. `@std/show` is the first standard trait;
interfaces carry traits, impls and bounds (`/8`); epoch 22. Not here: trait objects, default
methods, associated items, generic traits, generic function values (`N0387` stays outside the
model), the compiler written in Nazm (N98). Next: N79, effects and capabilities v3.

### N77 — resolution, persisted — **complete as scoped, 2026-10-03; area 2 stays PARTIAL**

`architecture.md` §7.78 first. A module that checks cleanly leaves `nazm.resolved/1`, its names by
durable identity, stored with its check (`nazm.check/3`) so its invalidation is the check's; the
same bytes from two processes and two checkouts. `nazm references` and `nazm resolve` read reused
modules' units without checking their bodies, and agree with the language service. The compiler
written in Nazm refuses `@std/` and package imports by name. Not here: re-export and export under
another name (no design, so area 2 stays PARTIAL), a build that skips bodies, persisted locals. Next:
N78, traits and methods.

### N76 — syntax completion: incremental reparse, finer recovery, `\u{…}` — **complete as scoped, 2026-10-03**

`architecture.md` §7.77 first. `nazm_syntax::Revision` and `Revision::edit`: a file's next parse from
its previous one, relexing and reparsing only from the first item an edit can reach to the first old
boundary it lands on, held **equal to a fresh parse** over every `.nz` file in the repository under
seeded edit sequences. Recovery inside record, enum, parameter, argument, initialiser and arm lists,
and at a broken `use`'s `;`; the abstract tree's contract unchanged. `\u{…}` escapes, epoch 21.
`nazm lsp` synchronises incrementally through the service's per-document revision. Not here: a
hole-tolerant checker, incremental parsing outside an editor's open document, the compiler written
in Nazm (N98). Next: N77, resolution and modules.

### N75 — production-readiness audit and release gate — **complete: candidate `cf664fd`, not released, 2026-10-03**

An audit, not a feature: every matrix status against its evidence, `limitations.md` line by line, the
security wording of every current document, the schemas against their documentation — four documentation
defects fixed. The gate at one commit, contained: the workspace suite, lifecycle, selfhost, bootstrap,
the benchmark and the **full mutation catalogue, 1,089 of 1,089 caught** (one timeout given a killer); nine
platform runs and the fuzzers and formal core at release scale on the host. The release assembly found
that its script could not copy the conformance corpus's directories, fixed in `cf664fd`, which it then
assembled and verified; the artefact re-digested independently. `docs/releases/cf664fd.md` holds the
identities, the target table, the accounting, the evidence index and the human release procedure.
Nothing is tagged, pushed or published: that is a person's decision. N49–N75's programme ends here.

### N74 — ecosystem, IDE and migration tooling — **complete as scoped, 2026-10-02**

`architecture.md` §7.76 first. `nazm init` and six templates, each held by its own commands; a library
package checked as its modules; `nazm doc` from the checker's facts; `nazm bindgen` for the C subset
that crosses, refusing the rest; `nazm publish --dry-run`; the language service tested across packages;
an LSP client for one editor. Not here: a network registry, rename across packages, a debug adapter,
an agent-efficiency claim. Next: N75, the release-candidate audit.

### N73 — compiler trust and supply chain — **complete as scoped, 2026-10-02**

`architecture.md` §7.75 first. A front-end robustness fuzzer and a differential fuzzer across the
interpreter and three native tiers, both seeded, with a replayed regression corpus; three injected
faults found by them alone. `nazm build --provenance` writes a deterministic record of a build's inputs,
toolchain and outputs, and `--attest-with` hands it to a local signer; `cargo xtask evidence` indexes an
evidence bundle with checksums and writes the compiler's bill of materials from `Cargo.lock`.
Cross-toolchain objects measured and classified. Not here: coverage-guided fuzzing, sanitizers, Miri, Loom. Next: N74, ecosystem.

### N72 — sBPF account backend — **PARTIAL: analysis and metadata; execution BLOCKED, 2026-10-02**

`architecture.md` §7.74 first. A `signed-writes` rule refuses a state write no signer decision guards,
and account metadata states each instruction's discriminator, arguments and account constraints. No
sBPF toolchain, validator or emulator exists here, and upstream eBPF is not sBPF: the backend is
designed by the metadata and not built. Next: N73, compiler trust and supply chain.

### N71 — WASM contract adapter — **complete as scoped, 2026-10-02**

`architecture.md` §7.73 first. The ordinary WebAssembly backend, unchanged, plus a generated adapter
appended to the contract's own unit: exports per entrypoint, the state across two typed host imports,
conservation in the module. The reference host's run equals the simulator's. Next: N72, an
account-oriented BPF backend.

### N70 — EVM backend — **complete as scoped, 2026-10-02**

`architecture.md` §7.72 first. Contracts compile directly from Core IR to EVM bytecode — `i64` held
exactly in 256-bit words, conservation enforced on-chain, selectors from the compiler's own keccak,
storage slots by field name so order and additions move nothing — and run in py-evm with the
simulator's outcomes, codes and storage, inside their stated gas bounds. Next: N71, a WASM contract
adapter.

### N69 — The Web3 semantic contract model — **complete as scoped, 2026-10-02**

`architecture.md` §7.71 first. A contract is ordinary Nazm under a `web3` profile of existing
rules; its read and write sets are facts, conservative where unseen; a deterministic simulator runs
transactions, reverting by code and rejecting any that changes a declared conserved quantity. Static
asset linearity is designed. Next: N70, an EVM backend.

### N68 — Adaptive representation — **complete as scoped, 2026-10-02**

`architecture.md` §7.70 and research register R4 first, with the prior art. A `Vec` of a record of
`Int`s and `Bool`s has an unobservable layout, so `--layout soa` stores it one array per field: the
same results and failures, the layout in every affected object's identity, 2.8× on a one-field scan
and 2× slower on an all-field one — which is why the compiler does not choose yet. Next: N69, the
Web3 semantic contract model.

### N67 — GPU/HPC — **complete as scoped, 2026-10-02**

`architecture.md` §7.69 first. A kernel is a pure `(Int) -> Int` function meeting a stated
eligibility, refused by name otherwise; `nazm accel` generates OpenCL C from Core IR carrying checked
arithmetic, runs it on the M1 Pro's GPU with every transfer and the synchronisation in its report,
recovers the sequential map's first failure from a parallel run, and holds every result to the
interpreter's. About 6× one CPU core on a Collatz map. Next: N68, adaptive representation.

### N66 — Vectorisation — **complete as scoped, 2026-10-02**

`architecture.md` §7.68 first. Checked arithmetic kept every loop scalar; `ints_sum_from` keeps the
ordered checked sum's meaning while adding a block unchecked only where every partial sum is provably
in range, and native builds replace the counted summation loop with it — 1.9× on an M1 Pro at `-O2`,
the same values and failures everywhere. Every other loop's reason is in `nazm explain-cost`.
Next: N67, GPU/HPC.

### N65 — The interactive tier — **complete as scoped, JIT BLOCKED, 2026-10-02**

`architecture.md` §7.67 first. A REPL that checks its session as one program on every edit and
refuses a redefinition that breaks a caller, by name; comptime for parameterless pure functions,
sandboxed by the authority model itself; a reload check naming what a running image could take. The
JIT is blocked by the workspace's `unsafe_code = "forbid"`, which is the project's to lift, and is
designed. Next: N66, SIMD and vectorisation.

### N64 — WebAssembly — **complete as scoped, 2026-10-02**

`architecture.md` §7.66 first. `wasm32-unknown-unknown` through the LLVM backend, carrying checked
arithmetic and failure unchanged, with every capability of the outside a `nazm_host` import present
only where used — a module with no `IoCap` has no `write`. A reference host grants exactly those.
Fourteen corpus programs run as WebAssembly with the interpreter's output; fifty are refused by
part, for want of a heap, which is designed and not built. Next: N65, a JIT and REPL tier.

### N63 — Real-time bounds — **complete as scoped, 2026-10-02**

`architecture.md` §7.65 first. A `realtime` profile — `embedded` plus no blocking, no clock and
bounded loops — and a bounds report: exact trip bounds for the counted loop form, call depth, site
counts, `wcet` null. A board build states its stack bound from the code generator's frames along the
deepest path; a painted stack on QEMU stays below it at `-O0` and `-O2`. Measurement is never the
bound. Next: N64, WebAssembly.

### N62 — Embedded bare metal — **complete as scoped, 2026-10-02**

`architecture.md` §7.64 first. `aarch64-unknown-none`: a board runtime without thread-local storage
or a C library, failure on the UART and semihosting's exit, `_start` and `link.ld`, a stack check
against the image's own stack; volatile `mmio_read32`/`mmio_write32` under an `MmioCap`, refused
where there is no device (`N0392`). A Nazm program boots on QEMU's `virt` board and prints through
its UART. Atomics, interrupts, a heap and `@std` designed, not built. Next: N63, real-time bounds.

### N61 — Formal semantics — **complete as scoped, 2026-10-02**

`architecture.md` §7.63 and `docs/formal-core.md` first. A core's typing and evaluation rules,
transcribed with no compiler dependency, and five properties checked over all 94,352 programs up to
five nodes, holding Nazm's interpreter and checker to them. Its first run found the transcription
wrong against the spec (`i64::MIN % -1`). A model check, not a proof. Next: N62, embedded bare metal.

### N60 — Restriction profiles v2 — **complete as scoped, 2026-10-02**

`architecture.md` §7.62 first. `pass`/`fail`/`unknown` verdicts, `unknown` refused; `embedded`'s
`no-recursion` over MIR's direct call graph, `unknown` through a function value; `critical`'s
`no-select`; a dependency cannot weaken the root's profile. Constant-time RESEARCH; bounded loops,
queues and tasks to N63. Next: N61, formal semantics.

### N59 — Backend performance — **complete as scoped, 2026-10-02**

`architecture.md` §7.61 first. Scalar temporaries assigned once and read in their block have no
slot in the LLVM emitter: 54 % fewer `alloca`s in the compiler's own text, a 5 % faster `-O0`
build, a quarter faster `-O0` program, the same output. MIR's ownership model untouched; Cranelift
already SSA. The native build still needs clang for the runtime and the C driver for the link,
named by `nazm inspect`. Next: N60, advanced restriction profiles.

### N58 — Debugger and profiler v2 — **complete as scoped, 2026-10-02**

`architecture.md` §7.60 first. `Int`, `Bool` and `Str` parameters and bindings described in DWARF;
the prologue carries no line, so a breakpoint on a function stops after its slots are written; a
subprogram is named by its Nazm name. A live gdb session verified in the Linux container — names,
lines, backtraces, stepping and variables; lldb on macOS still blocked by developer mode. `nazm
profile` prints one run's memory and scheduler counts as `nazm.profile/1`. Cranelift's DWARF is
BLOCKED by name. Next: N59, backend performance.

### N57 — Targets and cross-compilation — **complete as scoped, 2026-10-02**

`architecture.md` §7.59 first. Four targets, both backends, from any host in the matrix; an
executable where the host links the target, objects and `link.txt` where it cannot; the triple in
every key; `nazm inspect` names each target's output. Run-verified on three (host, x86_64 macOS
under Rosetta, aarch64 Linux in the container), compile-only on x86_64 Linux. No language change:
semantic epoch unchanged. Next: N58, debugger and profiler v2.

### N56 — Packages v2: a local registry — **complete as scoped, 2026-10-02**

`architecture.md` §7.58 first. A registry is a directory: an index per package and an immutable,
digest-checked copy per version. Four requirement forms, solved per name to the lockfile's version
while it satisfies or the highest non-yanked, one version per name, no backtracking. `nazm publish`
and `nazm yank`; tampering, links, malformed or equivocating indexes, double publishing and name
conflicts refused by code. No network, no signatures, no multi-root workspaces; the self-hosted
compiler's package support is DESIGNED. Semantic epoch unchanged: no checker rule moved. Next: N57,
cross-compilation.

### N55 — FFI v2: C strings in, exports out — **complete as scoped, 2026-10-02**

`architecture.md` §7.57 first. A `Str` argument to C is a borrowed NUL-terminated copy, freed after
the call; a NUL byte is refused before C runs. `pub extern "C" fn … ! {} = "sym" { … }` exports a
scalar Nazm function, and a failure inside one ends the process instead of unwinding into C.
`nazm build --lib -o OUT.a` writes a static archive and `OUT.h`, the same bytes on every build,
under both backends. Opaque handles, C structs, callbacks and `errno` are DESIGNED with their
reasons. Runtime ABI 7; semantic epoch 18. Next: N56, packages v2.

### N54 — Select, deadlines, cooperative cancellation and counters — **complete as scoped, 2026-10-02**

`architecture.md` §7.56 first. One OS thread per task stays: the stackful pool is DESIGNED, its three
thread-local prerequisites named, and not built. Built: `chan_select_of` and `chan_select_until`
over `Vec[Chan[T]]` with the lowest ready index winning and a closed channel always ready — the
cancellation signal — on a runtime-wide event count that cannot lose a wake-up; `time_now_ms`;
`TimeCap`, held, and `critical`'s `no-ambient-time`; `NAZM_SCHED_REPORT` counters agreeing across the
interpreter and both backends. N44's failure rule unchanged. Runtime ABI 6; semantic epoch 17. Next:
N55, FFI v2.

### N53 — Runtime crate and generic channels — **complete, 2026-10-02**

`architecture.md` §7.55 first. The runtime is its own crate, `nazm-runtime` — text, inventory,
OS adapter and platform entry — depending on nothing of the compiler and consumed by both
backends; runtime ABI revision 5, and a digest of what the runtime emits enters every object key
beside it. `Chan[T]` for any `T` that may cross into a task (`N0390` otherwise), with
`chan_new_of`, `chan_send_of`, `chan_recv_of` and `chan_close_of`: one runtime (`nz.chanv_*`) whose
header extends the `Int` channel's with the element's stride and release, so retain and close are
shared; a queued value is released by the channel's last reference, a refused one by its sender.
The `Int`-only `Chan` and its built-ins are unchanged, and the Nazm-written compiler does not
carry the typed channel. Semantic epoch 16. Next: N54.

### N52 — Resource cost and information flow v2 — **complete, 2026-10-02**

`architecture.md` §7.54 first. Resource sites read off MIR with a count per call that is
`at-most-once` or `unknown` (`nazm explain-cost`, `nazm.cost/1`); a sink registry — `path`,
`output`, `file-data` — solved through helpers, closures and modules (`nazm explain-flow`,
`nazm.flow/1`); `str_vouch` under a new `VouchCap`, recorded; three profile rules over the new
facts. Containers stay whole-container conservative, by statement. Semantic epoch 15. Next: N53,
the runtime crate.

### N51 — Effects and capabilities v2 — **complete, 2026-10-02**

`architecture.md` §7.53 first. One effect parameter per function, `effects E`, opaque in its body
and bound at each call from its function-typed arguments (`N0388`, `N0389`); `vec_map`,
`vec_filter` and `vec_fold` take it, so a caller is held to what the function it passes does, and
a profile sees it. A closure holds the capabilities visible where it is written, as captured
(`N0386` narrowed to `let mut`). Making a value of an undeclared function needs the authority its
type's effects need, which bounds the N37 bridge at the edge N50 opened. Attenuation and
revocation deferred with their cost. Semantic epoch 14; interface `/7` with `*` in effect lists.
Next: N52, resource cost and information flow.

### N50 — First-class functions and closures — **complete, 2026-10-02**

`architecture.md` §7.52 first, amended with the code where the code was simpler. Function types
`fn(T…) -> R ! {e}`, named functions as values, closures capturing by copy (`N0386` for a `let mut`
binding or a capability), indirect calls through a binding, and `N0387` for what cannot be a value.
One lowering: the checker declares each closure, Core IR lifts it, MIR adds the environment and a
thunk per named function used as a value, and the interpreter and both backends consume that. One
closure object, counted in the memory report's new `closures` class. `@std/seq` gained `vec_map`,
`vec_filter` and `vec_fold`, plain Nazm. Semantic epoch 13, runtime ABI 4, interface schema kept at
`/7` with a fifth type shape. Traits and methods are designed and deferred: function values met
every need in hand. The compiler written in Nazm has none of it. Next: N51, effects and
capabilities v2 — effect polymorphism is what keeps the higher-order library pure-only.

### N49 — Language usability and native completeness — **complete, 2026-10-01**

`architecture.md` §7.51 first. String escapes (`N0004`), `&&`, `||` and `!` (lowered to `if` in
Core IR), native recursion under both backends with a measured per-thread stack guard (`N0408`,
runtime ABI 3), and `use "PATH" as NAME;` with `NAME::item` (`N0209`, `N0210`). Semantic epoch
12. `nazm capabilities` now separates the two implementations only by the interpreter's iteration
budget and the native-only foreign call. The compiler written in Nazm refuses all of it by name.
Found on the way: an instance that calls itself was declared and defined by one unit, which
recursion through a generic function exposed. Next: N50, function values.

### N48 — Inspector, debug information, timings, and the v1 release audit — **complete, 2026-10-01**

`architecture.md` §7.50 first. `nazm inspect` prints `nazm.inspect/1`; `nazm build --debug` writes
DWARF through the LLVM backend, verified statically with lldb; `nazm build --timings` prints
`nazm.timings/1`. The release audit added `docs/limitations.md`, release-candidate notes and the
human release procedure in `docs/releases/`, and this section.

### N47 — Restriction profiles — **complete, 2026-10-01**

`architecture.md` §7.49. `general`, `embedded`, `critical` and `cyber`: rules with stable ids over
the checker's facts, `N0510`/`N0511`, and `nazm.profile-report/1`. A profile only refuses.

### N46 — Packages, locking and reproducible builds — **complete, 2026-10-01**

`architecture.md` §7.48. `nazm.toml`, `use "NAME:path"`, `nazm lock`, `--locked`, `N0500`–`N0509`,
and byte-identical executables from two directories.

### N45 — The standard library — **complete, 2026-10-01**

`architecture.md` §7.47. Seven `@std/…` modules of Nazm source, every function's contract declared.

### N44 — Structured concurrency and the scheduler — **complete, 2026-10-01**

`architecture.md` §7.46. The OS as scheduler, stated and stress-tested; no cancellation, by
decision.

### N43 — The runtime constitution — **complete, 2026-10-01**

`architecture.md` §7.45. One typed inventory of the runtime, an OS adapter, runtime ABI revision 2
in every object key, and the runtime tested directly.

### N42 — The ABI and foreign functions — **complete, 2026-10-01**

`architecture.md` §7.44. `extern "C"` declarations of `Int` and `Bool`, the effect `foreign` and
`ForeignCap`, `--link`, `nazm.interface/7`, semantic epoch 11.

### N41 — The backend boundary and the Cranelift backend — **complete, 2026-10-01**

`architecture.md` §7.43. A `Backend` trait, a typed runtime registry, and Cranelift 0.136.1
compiling every MIR function, identical to LLVM on all 104 buildable programs.
### N40 — MIR, the one lowering for the native backend — **complete, 2026-10-01**

`architecture.md` §7.42 was written first. `nazm-mir` holds MIR: each concrete function — every
generic instance included — as basic blocks with exactly one terminator each, typed locals, and
every copy, move, drop, scope join and failure edge written out; a validator proves, by a forward
dataflow over managed locals, no read of an empty local, no overwrite of an owned one, nothing
owned at a return or unwind, and every task list joined. `nazm-lir` reads MIR only — it no longer
depends on `nazm-cir` and cannot name a `Resolution` (a `cargo xtask check` rule) — and its LLVM
emitter was rewritten over it. `nazm mir` prints `nazm.mir/1`. Each function has an executable
digest that formatting, names and effects do not move. The interpreter stays on Core IR, by
decision. All 205 sources check, run and build byte-identically to N39, and all 104 buildable
programs behave identically. No language change: `SEMANTIC_EPOCH` stays 10.

| Settled | Where |
| --- | --- |
| the constitution | `architecture.md` §7.42 |
| the representation, lowering, validator, printer, digest | `crates/nazm-mir/` |
| native lowering and emission from MIR | `crates/nazm-lir/src/lower.rs`, `emit.rs` |
| the boundary | `cargo xtask check` *core ir boundary*, `xtask/src/rules.rs` |

Mutation evidence: fifteen new mutations and 44 historical ones repointed to where their rule lives
now, each for a stated responsibility, seven retired; all 59 caught (`capability-matrix.md` §30).

### After N39, in dependency order

No item is chosen here; N39 left these standing, beside N38's below.

| | Depends on | Why not before |
| --- | --- | --- |
| MIR: places, moves, drops, exclusivity | Core IR ✓, its region ownership ✓ | Core IR says what a region owns; nothing yet needs a place |
| Digests as cache keys (Core IR, MIR, objects) | per-function digests ✓ | nothing reuses work at this level yet, so nothing could key on them |
| Persisting Core IR across runs | a cache key that covers what lowering reads | built from each run's full check; cheap next to checking |
| An optimiser | a level to optimise, and a reason | no pass exists and none was added for a benchmark |
| The Nazm-written compiler reading Core IR | its own checker growing a lowering | the bootstrap compares what it emits, which did not move |
| Checking cost in one function's bindings | a scope structure that is not scanned | quadratic in bindings per function (`performance.md`, N39) — the checker's, predating Core IR |

### N39 — Core IR constitution and typed lowering v1 — **complete, 2026-09-30**

One lowering of a checked program, and both backends start from it. `architecture.md` §7.41 was
written first and says what Core IR represents and omits, when it is built, and the invariants a
consumer may assume. `nazm-cir` holds it: typed values — the checker's types, or *never* for an
`if`, `match` or block every path of which transfers — definitions and built-ins by identity,
structured regions with explicit completion and a named loop on every `break` and `continue`,
`?` as a `match` whose error arm returns, comparisons with their law, and each function's effect
contract and authority. `nazm-core` lowers only after checking succeeded, privately, and a
disagreement with the checker is `N0900`, a compiler defect; the verifier runs on every lowering.
The interpreter runs Core IR with slot frames and reads neither the tree nor the resolution;
`nazm-lir` lowers Core IR and no longer depends on `nazm-syntax`; a `cargo xtask check` gate, run
by the suite too, refuses a position lookup below Core IR. `nazm core-ir` prints
`nazm.core-ir/1`, deterministic and durable. Each function has a semantic and a body digest: an
effect-only change moves only the first, a capability parameter both, provenance neither.
Provenance stays a checker fact. All 205 sources check, run and emit IR byte-identically to N38;
the interpreter is 16–52 % faster on the bench programs on the host, and mixed — −12 % to +10 % —
in the Linux container. No language change: `SEMANTIC_EPOCH`
stays 10, `nazm.interface/6` and every tooling schema unchanged.

| Settled | Where |
| --- | --- |
| the constitution | `architecture.md` §7.41 |
| the representation, verifier, printer, digests | `crates/nazm-cir/` |
| the one lowering | `crates/nazm-core/src/lower.rs` |
| the interpreter over Core IR | `crates/nazm-core/src/eval.rs` |
| native lowering from Core IR | `crates/nazm-lir/src/lower.rs` |
| the boundary | `cargo xtask check` *core ir boundary*, `xtask/src/rules.rs` |

Mutation evidence: nineteen new mutations, twelve repointed to where their rule lives now, one
retired; 37 selected with a stated link each, all caught (`capability-matrix.md` §30). The MILESTONE
gate ran offline and every stage passed; the campaign needed six sessions, 13,876 s. The interpreter
is faster on the host and mixed in the Linux container (`performance.md`, N39).

N40 followed; see above.

### After N38, in dependency order

No item is chosen here; N38 left these standing, beside N37's and N36's below.

| | Depends on | Why not before |
| --- | --- | --- |
| Implicit flow (conditions, loop bounds) | a control-dependence model | explicit data flow is what v1 can state exactly |
| Field-, element- and alias-precise provenance | a MIR that tracks places | records are tracked whole and shared storage is `unknown` |
| More sinks, user labels, declassification, sanitisers | a policy design | one sink is what current semantics justify without an escape hatch |
| Provenance summaries for foreign code | an FFI | there is none |

### N38 — Provenance and information flow v1 — **complete, 2026-09-30**

Where a value came from, statically. Four compiler-owned origins — `argument`, `file`, `authority`,
`unknown` — enter only through the built-ins that bring data in and `main`'s capabilities, and flow
by explicit data flow through bindings, operators, calls, records, variants, `Result` and `?`; a
condition contributes nothing, and a value read out of a sequence or channel is `unknown`. Each
module's bodies are reduced to facts kept in its check entry (`nazm.check/2`), and every run solves
the whole program's summaries from them to the least fixed point, so a callee's body change reaches
a caller the cache reused. One restricted flow: in a function with a declared effect set, the path
`write_file` writes to may not derive from a file's contents or from shared storage, directly or
through any function that writes to a path it is given (`N0372`). Tools carry it in
`nazm.context/3`, `nazm.snapshot/3` and `nazm.delta/3`. Nothing reaches what runs: the IR is
byte-identical under N37 and N38. `SEMANTIC_EPOCH` 9 → 10; `nazm.interface/6` unchanged. All 203
existing sources check exactly as before.

| Settled | Where |
| --- | --- |
| origins, flows, facts and summaries | `crates/nazm-sema/src/provenance.rs` |
| the walk, the solve and the restriction | `crates/nazm-core/src/provenance.rs` |
| facts across runs | `crates/nazm-iface/src/facts.rs`, `crates/nazm-cache/src/artifact.rs`, `crates/nazm-cli/src/checking.rs` |
| packets, snapshots, deltas | `crates/nazm-service/`, `schema/nazm.{context,snapshot,delta}-3.json` |
| the law | `spec.md` *Provenance: where a value came from*, `architecture.md` §7.40 |

Mutation evidence: twenty-five new mutations and one retired; 34 selected with a stated link each,
all caught (`capability-matrix.md` §30). The MILESTONE gate ran offline in 7,217 s. Every stage passed but the MCP benchmark's memory
check, which a one-time glibc arena step failed; its law now samples the measured window and allows
one such step only with a plateau after it, and the release-benchmark stage passed when run again
alone (`performance.md`).


### After N37, in dependency order

No item is chosen here; N37 left these standing, beside N36's below.

| | Depends on | Why not before |
| --- | --- | --- |
| Attenuation and resource-specific capabilities (a read-only `IoCap`, one file, one directory) | a kind with a resource argument | v1 has no narrower kind to derive one into |
| Retiring the ambient bridge | declared sets on the compiler's own sources, and a migration story | 1,034 of 1,054 function checks in the tree still declare no set |
| Runtime or operating-system enforcement | a design for what crosses into foreign code | the check is static; nothing at runtime consults a capability |
| Linear or affine authority, revocation | move semantics, lifetimes | a capability copies freely today |
| Restriction profiles (no `IoCap` in embedded, no `SpawnCap` in critical) | capabilities ✓, a profile mechanism | nothing selects a profile yet |
| Foreign and unsafe authority | an FFI | there is none to authorise |
| The Nazm-written compiler reading capabilities | the same lexer and parser work as effect sets | no source it reads declares one |

### N37 — Capabilities / authority v1 — **complete, 2026-09-30**

An effect says what a function does; a capability says what allows it. Two compiler-owned kinds,
`IoCap` and `SpawnCap`, are built-in types: a function that declares an effect set holds exactly
the capability values its scope reaches, and every outside-world built-in, `spawn` and call of an
undeclared function needs the authority for what it does (`N0369`), checked apart from the effect
(`N0366`) and reported before it. `main` is the root: the runtime hands it one of each capability
it declares, it may take nothing else (`N0371`), and nothing constructs one (`N0370`). A function
that declares no set is the **compatibility bridge** — it exercises its caller's authority — and is
bounded so that code with a contract cannot reach authority it does not hold through it, in its
module or another. Authority crosses a module as parameter types, in `nazm.interface/6` unchanged.
The checking is static and a capability erases to a word nothing reads; there is no sandbox.
`SEMANTIC_EPOCH` 8 → 9. Of the 200 `.nz` sources at N36, 197 check exactly as before; the three
that do not are N36's `examples/effects/`, which declared effects without authority and now hold it.
N36's wording was corrected so that `fn f() -> Int` and `fn f() -> Int ! {}` cannot be read as one.

| Settled | Where |
| --- | --- |
| capability kinds, sets and the built-in table | `crates/nazm-sema/src/capability.rs`, `types.rs` |
| possession, forging, `main`'s parameters | `crates/nazm-core/src/check.rs` |
| the authority check and its witness | `crates/nazm-core/src/effects.rs` |
| the root at runtime | `crates/nazm-core/src/eval.rs`, `crates/nazm-runtime/src/entry.rs` |
| the law | `spec.md` *Capabilities: what allows a function to act*, `architecture.md` §7.39 |

Mutation evidence, under the new responsibility law: twenty new mutations and one retired; 33
selected — the twenty and thirteen N36 entries each with a stated link — all caught, 32 at tier 1
and 1 at tier 2 whose killer was then strengthened and verified alone (`capability-matrix.md` §30).
The MILESTONE gate ran offline: 5,737 s of wall time across its stages (`performance.md`).


### After N36, in dependency order

No item is chosen here; N36 left these standing, beside N35's and N34's below.

| | Depends on | Why not before |
| --- | --- | --- |
| ~~Capabilities: who may exercise an effect~~ | the effect sets N36 settled | **done in N37** |
| Profiles that forbid effects (critical: no `io`, no `spawn`; embedded) | effect sets ✓, a profile mechanism | nothing selects a profile yet |
| The Nazm-written compiler reading effect sets | a lexer token and a parser rule in `compiler/*.nz`, and a bootstrap | N36 avoided migrating the compiler's sources; none of them declares a set |
| Declared sets on the compiler's own exports | the item above | an undeclared import is every effect to its importer, so 152 of 1,044 function checks in the tree read `{ io, spawn }` |
| Effect polymorphism and handlers | function values | nothing a type argument brings can call anything yet |
| Divergence, panic and allocation as effects | answers to `spec.md` *Open — Effects* | each is a design question, not an omission |
| Completion of effect names | a completion site inside an effect set | two names; not worth a new site kind in N36 |

### N36 — Typed effects and the effect system constitution v1 — **complete, 2026-09-30**

A function may declare the effects it exercises — `fn show(n: Int) -> Int ! { io }`, `! {}` for
pure — and every function's effects are inferred from its resolved calls to the least fixed point
over its module. Two effects, both the compiler's: `io`, the eight outside-world built-ins, and
`spawn`, a `spawn` statement and everything its task does. A declared set is a contract the body
is checked against (`N0366`, with a shortest witness); unknown and repeated names are refused
(`N0367`, `N0368`). Across modules only a declared set travels, persisted in `nazm.interface/6`, so
a caller never reads another module's body and an undeclared import is every effect. Tools show
it: signatures and hover, a packet's `effects` (`nazm.context/2`), an `effects` snapshot section
(`nazm.snapshot/2`, `nazm.delta/2`). Nothing reaches what runs: the IR is byte-identical with and
without declarations. `SEMANTIC_EPOCH` 7 → 8. Every one of the 116 existing sources checks exactly
as before; none needed a change.

| Settled | Where |
| --- | --- |
| the syntax, CST node and formatting | `crates/nazm-syntax/`, `grammar.ebnf`, `guide.md` |
| effect identity and sets | `crates/nazm-sema/src/effect.rs` |
| inference, the contract and the witness | `crates/nazm-core/src/effects.rs` |
| the interface, `/6` | `crates/nazm-iface/src/wire.rs` |
| signatures, hover, packets, snapshots, deltas | `crates/nazm-service/`, `schema/nazm.{context,snapshot,delta}-3.json` (`-2` until N38) |
| the law | `spec.md` *Effects: what a function may do*, `architecture.md` §7.38 |

Mutation evidence: seventeen new mutations, two retired, two repointed; 85 selected, all caught,
78 at tier 1 with their killers verified and 7 at tier 2 (`capability-matrix.md` §30). The
MILESTONE gate ran offline: 7,242 s of wall time across its stages (`performance.md`).


### After N35, in dependency order

No item is chosen here; N35 left these standing, beside N34's and N33's below.

| | Depends on | Why not before |
| --- | --- | --- |
| The same benchmark on a second model family, through the direct adapters | a key a developer brings | none was supplied; the OpenAI and Anthropic adapters are tested against recorded responses only |
| A deterministic run (temperature 0) | a provider that takes one | the Claude Code client sets none; the model's default applied |
| More trials, and intervals rather than ranges | a budget for them | two trials show a spread, not a significance |
| Routing a tiny, self-contained task to raw source | evidence across models, and a rule to test | N35 observed one crossover, for one model, between 744 and 6,720 input tokens; it routes nothing |
| A task wording that keeps builtins out of "definitions" (T2) | a new suite version, compared from the start | the frozen suite is not edited after its results |
| Tool-using and multi-turn agents | a protocol for them | N35 measures whether the context alone suffices |

### N35 — Agent task token, cost and correctness benchmark v1 — **complete, 2026-09-29**

`nazm-agent-bench` puts eight tasks over the seven N33 scenarios — understand, edit, diagnose,
documentation and test selection, the 123-byte diagnostic among them — to a real model, each with
the naive baseline and with `nazm repo --task`'s context, byte for byte and digest-addressed, one
prompt for both. Each answer is scored fact by fact by an offline oracle whose truths come from the
authorities; hallucinations are kept apart from misreadings; usage is kept raw and normalised, and
dated prices are applied in integer pico-USD, never in an identity. A spend cap is checked before
every attempt, transport failures are retried at most twice and never counted incorrect, the
journal resumes, and a dry run needs no provider. On `claude-haiku-4-5-20251001` through the Claude
Code client, two trials: the N33 context solved 12 of 16 requests to 6, with 112 of 116 facts to
95, no hallucination in either arm, 94.36 % fewer input tokens and 95.39 % less cost as billed
(92.86 % uncached); correctness was preserved on 7 of 8 tasks, T2 the exception by the
pre-declared rule, and the diagnostic the one task where raw source was cheaper. Evaluation tooling
only: no crate depends on it, and no grammar, semantics, `SEMANTIC_EPOCH` (7), `nazm.interface/5`
or existing schema changed; one new schema.

| Settled | Where |
| --- | --- |
| the suite, oracles, providers, runner and report | `crates/nazm-agent-bench/`, `architecture.md` §7.37 |
| the dated price list | `tools/agent-bench/pricing.toml` |
| the results | `tools/agent-bench/results/`, `performance.md` |
| how to run it, and bring a key | `runbook.md`, *The agent task benchmark* |
| nothing else on the network, nothing depending on it | `xtask/src/rules.rs`, the `network reach` and `tokenizer reach` gates |

Mutation evidence: sixteen new mutations and an `agent` profile, all caught at tier 1 with their
sixteen killers verified (`capability-matrix.md` §30). The MILESTONE gate ran offline after the
paid run, at `0419b25`: 2,977 s of wall time (`performance.md`). The benchmark spent $1.78 of a $2
cap, pilots included.


### After N34, in dependency order

No item is chosen here; N34 left these standing, beside N33's and N32's below.

| | Depends on | Why not before |
| --- | --- | --- |
| An agent task benchmark with a real model: tokens, cost, success (G83–G85) | task contexts ✓, multi-tokenizer counts ✓ | nothing has been run against a model, and a model run is its own protocol |
| Framing and protocol overhead of a model conversation | a chosen protocol and model | N34 measures content only, by law |
| A compact transport for tiny contexts | evidence that the fixed overhead matters in a real task | it is 400–700 tokens, constant, and carries the retrieval and reasons the planner's law requires |
| An audit of syntax decisions against several tokenizers (the rest of G81) | a syntax change to decide | no syntax was changed or proposed here |
| More families (e.g. WordPiece), a model's own chat template | a reason and a permissive asset | three families are what G82 asks |

### N34 — Tokenizer-independent context measurement and multi-tokenizer benchmark v1 — **complete, 2026-09-29**

`nazm-tokens measure` counts the exact text on standard input under four pinned tokenizers of
three families — OpenAI byte-level BPE (`cl100k_base`, `o200k_base`), SentencePiece BPE
(Mistral-7B-v0.1) and SentencePiece Unigram (T5-small) — offline, with every identity carrying its
library, revision, licence, normalisation, special-token policy and asset digest, as
`nazm.token-cost/1`. Content only; framing and unknown pieces beside the count; nothing normalised
first; an unknown tokenizer or an altered asset refused by name. The seven N33 tasks, with their
baselines unchanged, keep a 72.7–97.5 % reduction under every tokenizer, the one-function diagnostic
reported as a stress case. It is a tool beside the compiler: no crate depends on it, and `nazm` is
the same bytes as at N33. No grammar, semantics, `SEMANTIC_EPOCH` (7), `nazm.interface/5` or
existing schema changed; one new schema.

| Settled | Where |
| --- | --- |
| the tokenizers, their identities and the measurement | `crates/nazm-tokens/`, `architecture.md` §7.36 |
| the pinned assets and their provenance | `tools/tokenizers/` |
| one definition of the N33 tasks for both benchmarks | `crates/nazm-repo/tests/scenarios/` |
| the benchmark | `crates/nazm-tokens/tests/benchmark.rs`, `performance.md` |
| no tokenizer outside `nazm-tokens`, nothing depending on it | `xtask/src/rules.rs`, the `tokenizer reach` gate |
| workloads with no network | `xtask/src/contained.rs` |

Mutation evidence: eleven new mutations and a `tokens` profile, all caught at tier 1 with their
twelve killers verified (`capability-matrix.md` §30). The MILESTONE gate ran offline: 2,090 s of
stages, 40.6 minutes of wall time with one documentation fix and its re-run (`performance.md`).


### After N33, in dependency order

No item is chosen here; N33 left these standing, beside N32's below.

| | Depends on | Why not before |
| --- | --- | --- |
| An agent task benchmark with a real model: tokens, cost, success (G83–G85) | task contexts ✓, their byte measurements ✓ | nothing has been run against a model; bytes are not tokens |
| Multi-tokenizer measurement of contexts (G81, G82) | task contexts ✓ | tokenizer claims need several tokenizers |
| Rust semantic entities in the map | a compiler-owned Rust fact source | the compiler has no Rust semantics; packages and targets are all the map can own |
| Project-wide callers and importers of exported definitions | a project/importer boundary | callers are the manifest's compilations only, as rename's refusal already records |
| Transitive or ranked context | a justified bound | one step and sixteen members per relationship are the v1 law |
| A persistent or incremental repository index | measurement that forces one | a map takes about 0.4 s and a task 0.05–0.6 s in a release build |
| Documentation links from definitions and diagnostics | an authority that states them | no document names a definition or a code structurally today |

### N33 — Repository context map and minimum task context v1 — **complete, 2026-09-29**

`nazm repo --map` lists every entity of the repository with a durable identity — packages and
targets, source roots, compilation roots, modules and durable definitions, documents, schemas and
mutation profiles — with its authority, its structural relationships and where to retrieve more,
and never a body. `nazm repo --task` answers, for seed ids and an intent, the smallest context the
repository can justify: one step from each seed, each item with its reason, class, authority and
retrieval, the seed's exact source and the selected sections' exact text, under a structural budget
whose cut is `partial`. A goal or a plan never outranks the specification or the evidence; a stale
state is refused; a seed is an id and never a path. `nazm-mcp` adds `nazm.repository_map` and
`nazm.task_context`. It composes N27, N28, N30 and N32: no grammar, semantics, `SEMANTIC_EPOCH` (7),
`nazm.interface/5` or existing schema changed; two new machine schemas.

| Settled | Where |
| --- | --- |
| the map, the planner, the manifest, the Cargo and catalogue readers | `crates/nazm-repo/`, `architecture.md` §7.35 |
| a durable definition's name range, for asking its packet | `crates/nazm-service/src/durable.rs` |
| `nazm repo` | `crates/nazm-cli/src/repo.rs`, `schema/nazm.repository-map-1.json`, `schema/nazm.task-context-1.json` |
| `nazm-mcp`: `nazm.repository_map`, `nazm.task_context` | `crates/nazm-mcp/src/main.rs`, `docs/mcp.md` |
| each package's documentation | `[package.metadata.nazm] docs` in its `Cargo.toml` |
| what it costs and saves | `performance.md` |

Mutation evidence: nineteen new mutations and a `repo` profile; one session verified all 36 declared
killers of those nineteen and of the fifteen MCP entries N33's server change touches, and decided
all 34 — caught at tier 1 (`capability-matrix.md` §30). The MILESTONE gate took 40.5 minutes from
empty caches (`performance.md`).


### After N32, in dependency order

No item is chosen here; N32 left these standing. *Superseded by "After N33" above, except where
this table is the only record of an item.*

| | Depends on | Why not before |
| --- | --- | --- |
| Semantic or full-text documentation search (the rest of G78) | stable sections ✓ | ids are exact by design; a search is its own contract |
| Fine-grained normative rule ids and a rule graph (G79) | section anchors ✓ | ids stop at sections; rule-level identity is a spec-wide decision |
| Whole-spec deduplication, contradiction detection (G79) | a rule graph | no rule graph exists |
| Documentation history and version-change retrieval | a retention contract | nothing is kept between requests, and nothing reads Git |
| Documentation outside a source checkout | a packaging decision | the corpus is read from a repository's `docs/` |
| Progressive test-detail retrieval | test summaries ✓ | unchanged since N31 |
| Typed diagnostic facts (expected, actual, entity) | the checker recording them | unchanged since N30 |
| Patch application, a transactional write | patch plans ✓ | its own safety milestone |
| Rename of exported definitions | rename ✓, a project/importer boundary | unchanged since N18 |
| Effect and capability context | the language owning effects and capabilities | both are MISSING as language features |
| Incremental reparse | a lossless CST ✓ | still no evidence that forces it |

### N32 — Machine-addressable documentation and selective retrieval v1 — **complete, 2026-09-28**

The first executable step toward G78, and a narrow one toward G79. `nazm docs --index` lists Nazm's
own documentation — fourteen canonical documents, each with its authority — as sections with
stable ids, titles, parents and digests and no body; `nazm docs --section ID` returns one
section's own text byte for byte. Ids come from an explicit anchor, the document's own numbering,
or the heading path, never from a line or a hash; a state binds a request to the corpus an index
described. A goal is never evidence and a plan never a rule: each section carries its document's
authority. `nazm-mcp` adds `nazm.docs_index` and `nazm.docs_section`, by id and never by path.
Compiler semantics and every existing schema are unchanged: `SEMANTIC_EPOCH` 7,
`nazm.interface/5`; two new machine schemas; `spec.md` gained forty-three anchor comments and no
other change.

| Settled | Where |
| --- | --- |
| the corpus, its sections, ids, authorities and digests | `crates/nazm-docs/`, `architecture.md` §7.34 |
| `nazm docs` | `crates/nazm-cli/src/docs.rs`, `schema/nazm.docs-index-1.json`, `schema/nazm.docs-section-1.json` |
| `nazm-mcp`: `nazm.docs_index`, `nazm.docs_section` | `crates/nazm-mcp/src/main.rs`, `docs/mcp.md` |
| the corpus gate | `cargo xtask check-corpus` |
| what it costs | `performance.md` |

Mutation evidence: 17 new mutations and one re-pointed; the harness that verifies them was
rebuilt first (N32-H: one warm session per gate, `runbook.md`, *The warm-worker law*) and gained
two of its own. One session verified all 34 declared killers of the 19 new entries and of the 12
MCP entries N32's server changes touch, around the one injection each verdict came from, and
decided all 31 — caught at tier 1 (`capability-matrix.md` §30). The whole MILESTONE gate took
40.1 minutes (`performance.md`).


### After N31, in dependency order

No item is chosen here; N31 left these standing. *Superseded by "After N32" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Progressive test-detail retrieval | test summaries ✓ | `nazm test --json` is the detail today; no evidence yet needs a per-case query |
| Test impact, test-to-definition mapping | a relation the compiler owns | tests are not tied to definitions |
| Build and test summaries over MCP | a write and execution boundary | building writes an executable and tests run programs; the MCP server is read-only |
| Typed diagnostic facts (expected, actual, entity) | the checker recording them | unchanged since N30 |
| Further verbosity levels (G75) | compact and detail ✓ | not asked for |
| Diagnostic or build-log history | a retention contract | nothing is kept between requests |
| Patch application, a transactional write | patch plans ✓ | its own safety milestone |
| Rename of exported definitions | rename ✓, a project/importer boundary | unchanged since N18 |
| A project or importer boundary | a manifest, or a declared universe of roots | nothing says which files may import a module |
| Fixes of syntax diagnostics as patches | a binding for a recovered compilation | unchanged since N29 |
| Effect and capability context | the language owning effects and capabilities | both are MISSING as language features |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | no evidence justifies them |
| Refactors, source actions, fix-all | quick fixes ✓, patch plans ✓ | each is its own contract |
| Incremental reparse | a lossless CST ✓ | still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |

### N31 — Token-efficient build and test output v1 — **complete, 2026-09-28**

The first executable step toward G76. `--summary-json` on `nazm check` and `nazm build` prints one
`nazm.command-summary/1`: the command's own status — one per exit path — and exit status, and a
reference to every diagnostic it reported, with N30's id where N30 indexes it and an explicit
command reference where it does not (a missing `main`, a backend's refusal). On `nazm test` it
prints one `nazm.test-summary/1`: counts from each case's verdict, and only the cases that did not
pass, with how each leg ended, read from what the runner did. Summary is an additive view, not a
replacement for existing detailed machine output, and absence of diagnostics does not imply
command success. `nazm-mcp` adds `nazm.command_summary` for `check` only — building and testing are
not read-only. Human output, `--json`, `nazm.test/1`, `nazm.check-report/1`, `nazm.build-report/1`,
every exit status and every existing schema are unchanged: `SEMANTIC_EPOCH` 7, `nazm.interface/5`;
two new machine schemas.

| Settled | Where |
| --- | --- |
| command summaries, and their references to N30 | `crates/nazm-service/src/summary.rs`, `schema/nazm.command-summary-1.json`, `architecture.md` §7.33 |
| `nazm check|build --summary-json` | `crates/nazm-cli/src/checking.rs`, `build.rs`, `summary.rs` |
| `nazm test --summary-json` | `crates/nazm-cli/src/test.rs`, `schema/nazm.test-summary-1.json` |
| `nazm-mcp`: `nazm.command_summary`, check only | `crates/nazm-mcp/src/main.rs`, `docs/mcp.md` |
| what it costs | `performance.md` |

Mutation evidence: 15 new mutations; 41 killers verified on the final source — the fifteen, the
sixteen N30 entries whose order N31 shares, and the ten MCP entries of N27–N30; one targeted
campaign of 23, all caught — 21 at tier 1, one each at tiers 2 and 3 (`capability-matrix.md` §30).
Closure correction: every public N31 outcome variant — `could_not_build`, `could_not_run` and
`toolchain_failed` among them — has at least one regression test that executes its production
branch; the test runner's driver is a test seam, `NAZM_TEST_DRIVER`; seven more mutations.
24 killers verified on the final source; a targeted campaign of 14, all caught at tier 1.

### After N30, in dependency order

No item is chosen here; N30 left these standing. *Superseded by "After N31" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Typed diagnostic facts (expected, actual, entity) | diagnostics ✓, the checker recording them | no diagnostic carries them today; reading them from prose is refused |
| Further verbosity levels (explain, and the rest of G75) | compact and detail ✓ | not asked for; each is its own contract |
| Diagnostic history | a retention contract | unchanged: nothing is kept between requests |
| Build and test output redesign (G76) | diagnostics ✓ | done in part by N31: compact command and test summaries |
| Patch application, a transactional write | patch plans ✓ | its own safety milestone |
| Rename of exported definitions | rename ✓, a project/importer boundary | unchanged since N18 |
| A project or importer boundary | a manifest, or a declared universe of roots | nothing says which files may import a module |
| Fixes of syntax diagnostics as patches | a binding for a recovered compilation | N28 has no snapshot where syntax needed recovery |
| Semantic history, or a persistent snapshot store | snapshots ✓ | unchanged since N28 |
| Effect and capability context | the language owning effects and capabilities | both are MISSING as language features |
| Test-to-definition mapping | a relation the compiler owns | unchanged since N27 |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | no evidence justifies them |
| Refactors, source actions, fix-all | quick fixes ✓, patch plans ✓ | each is its own contract |
| Incremental reparse | a lossless CST ✓ | still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |

### N30 — Compact machine diagnostics and progressive disclosure v1 — **complete, 2026-09-28**

The first executable step toward G74, G75 and G77. `nazm.diagnostic-index/1` is every current
diagnostic of a root compilation as compiler-owned facts — id, code, severity, file and byte range,
owning definition, fix counts — with no prose; `nazm.diagnostic-detail/1` is one of them in full on
request, the diagnostic exactly as `nazm.diagnostic/1` publishes it, with its places in their files
and, for each fix, the N29 selector where a semantic patch is plannable. Compact diagnostics never
infer structured semantics from diagnostic prose, and a compiler fix and an N29 semantic patch are
distinct capabilities. Syntax diagnostics are indexed, bound to a digest of the loaded sources where
N28 has no snapshot. `nazm diagnostics` prints both; `nazm-mcp` adds `nazm.diagnostics` and
`nazm.diagnostic_detail`. `nazm.diagnostic/1`, `nazm check --json`, the language server and every
existing schema are unchanged: `SEMANTIC_EPOCH` 7, `nazm.interface/5`; two new machine schemas.

| Settled | Where |
| --- | --- |
| the index and detail, from `nazm-diag`, N22, N24, N28, N29 | `crates/nazm-service/src/diagnostics.rs`, `schema/nazm.diagnostic-index-1.json`, `schema/nazm.diagnostic-detail-1.json`, `architecture.md` §7.32 |
| `nazm diagnostics [--detail ID --state DIGEST]` | `crates/nazm-cli/src/diagnostics.rs` |
| `nazm-mcp`: `nazm.diagnostics`, `nazm.diagnostic_detail` | `crates/nazm-mcp/src/main.rs`, `docs/mcp.md` |
| what it costs | `performance.md` |

Mutation evidence: 18 new mutations; 26 killers verified on the final source — the eighteen and
the eight MCP entries of N27–N29; one targeted campaign of 25, all caught at tier 1
(`capability-matrix.md` §30).

### After N29, in dependency order

No item is chosen here; N29 left these standing. *Superseded by "After N30" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Patch application, a transactional write | patch plans ✓ | its own safety milestone: checking the three freshness layers, writing atomically, and reporting what was and was not written |
| Rename of exported definitions | rename ✓, a project/importer boundary | one root compilation is not every importer; unchanged since N18 |
| A project or importer boundary | a manifest, or a declared universe of roots | nothing today says which files may import a module |
| Fixes of syntax diagnostics as patches | a snapshot of a recovered compilation, or another binding | N28 has no snapshot where syntax needed recovery |
| Semantic history, or a persistent snapshot store | snapshots ✓ | unchanged since N28 |
| Effect and capability context | the language owning effects and capabilities | both are MISSING as language features |
| Test-to-definition mapping | a relation the compiler owns | unchanged since N27 |
| Machine-readable diagnostics beyond `nazm.diagnostic/1` | diagnostics ✓ | done in part by N30: a compact index and detail |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | no evidence justifies them |
| Refactors, source actions, fix-all | quick fixes ✓, patch plans ✓ | each is its own contract |
| Incremental reparse | a lossless CST ✓ | still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |

### N29 — Minimal semantic patch plan v1 — **complete, 2026-09-27**

The first executable step toward G72. `nazm.patch/1` is an edit the compiler already validates —
N18's rename, or the fix a current diagnostic carries (N24) — as minimal exact-byte edits, bound
to the root's N28 snapshot digest, each edited file's BLAKE3 digest and each edit's expected
bytes. N29 plans edits but never applies them: a patch is a proposal bound to exact semantic and
source state, not permission to mutate files. Exported rename remains refused, because one root
compilation is not a complete importer universe; the language server's closed-file rule is
unchanged. `nazm patch rename|fix` prints a plan; `nazm-mcp` adds one read-only planning tool,
`nazm.semantic_patch`, and keeps no plan. No language, interface, cache-key or bootstrap change:
`SEMANTIC_EPOCH` 7, `nazm.interface/5`, `nazm.context/1`, `nazm.snapshot/1`, `nazm.delta/1`
unchanged; one new machine schema.

| Settled | Where |
| --- | --- |
| the patch, `nazm.patch/1`, from N18/N24/N28 | `crates/nazm-service/src/patch.rs`, `schema/nazm.patch-1.json`, `architecture.md` §7.31 |
| `nazm patch rename`, `nazm patch fix` | `crates/nazm-cli/src/patch.rs` |
| `nazm-mcp`: `nazm.semantic_patch`, typed, read-only | `crates/nazm-mcp/src/main.rs`, `docs/mcp.md` |
| what it costs | `performance.md` |

Mutation evidence: 16 new mutations; 30 killers verified on the final source — the sixteen, the
nine N18 entries in `rename.rs` and the five MCP entries of N27 and N28; one targeted campaign of
27, all caught at tier 1 (`capability-matrix.md` §30).

### After N28, in dependency order

No item is chosen here; N28 left these standing. *Superseded by "After N29" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Semantic patch plans (G72) | rename ✓, quick fixes ✓, snapshots ✓ | done in part by N29: rename and diagnostic-fix plans, never applied |
| Rename inference, or definition lineage | snapshots ✓, deltas ✓ | a key's continuity is a name in a module; lineage needs a contract of its own |
| Behavioural or body fingerprints beyond exact source | a semantic IR | N28's `source` section is exact bytes; nothing finer is sound without one |
| Test impact, effect and capability deltas | a relation the compiler owns, effects and capabilities | tests are not tied to definitions; effects and capabilities are MISSING as language features |
| Semantic history, or a persistent snapshot store | snapshots ✓ | the client keeps its baseline; no evidence yet needs server-side history and its retention contract |
| Test-to-definition mapping | a relation the compiler owns | unchanged since N27 |
| Rename of exported definitions | rename ✓ | unchanged since N18; a snapshot is of one root, not every importer |
| Edits of files not open | rename ✓, quick fixes ✓ | unchanged since N18 |
| Incremental reparse, or a persistent analysis for MCP | a lossless CST ✓ | each MCP call re-analyses its root; still no evidence that forces a cache and its freshness contract |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |

### N28 — Semantic snapshot manifest and semantic delta v1 — **complete, 2026-09-27**

The first executable step toward G71. `nazm.snapshot/1` is one root compilation's durable
functions, records and enums, by `DefKey` alone, each with one BLAKE3 digest per section the
compiler records — exact source, shape, dependencies, related types, references, callers, callees,
diagnostics — taken over identities and counts, never offsets or session ids. `nazm.delta/1`
compares a baseline snapshot, validated strictly as data, with the current compilation: added,
removed, and changed with exactly which sections changed. A rename is removed plus added; a comment
or a literal is `source` alone. N28 reports changes in the semantic surfaces Nazm currently records;
it is not a proof of behavioural equivalence, and effects, capabilities, tests, history and
behavioural equivalence are unsupported, not unchanged. `nazm snapshot` and `nazm delta` print
them; `nazm-mcp` adds two read-only tools and keeps no snapshot — the client keeps its baseline. No
language, interface, cache-key or bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`,
`nazm.context/1` unchanged; two new machine schemas.

| Settled | Where |
| --- | --- |
| the snapshot, `nazm.snapshot/1`, from N17/N18/N20/N21/N22/N25/N27 | `crates/nazm-service/src/snapshot.rs`, `schema/nazm.snapshot-3.json`, `architecture.md` §7.30 |
| the delta, `nazm.delta/1`, and baseline validation | `crates/nazm-service/src/delta.rs`, `schema/nazm.delta-3.json` |
| `nazm snapshot --root --json`, `nazm delta --root --baseline --json` | `crates/nazm-cli/src/snapshot.rs` |
| `nazm-mcp`: `nazm.semantic_snapshot`, `nazm.semantic_delta`, stateless | `crates/nazm-mcp/src/main.rs`, `docs/mcp.md` |
| what it costs | `performance.md` |

Mutation evidence: 20 new mutations and five N25/N27 entries re-pointed onto code N28 now shares;
49 killers verified on the final source; one targeted campaign of 25, all caught at tier 1
(`capability-matrix.md` §30).

### After N27, in dependency order

No item is chosen here; N27 left these standing. *Superseded by "After N28" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| MCP expansion | the context packet ✓, one read-only tool ✓ | only where a question needs it; each tool is its own contract |
| Semantic delta (G71) | context packets ✓ | done in part by N28: snapshots and deltas of the sections the compiler records, not behaviour |
| Test-to-definition mapping | a relation the compiler owns | tests are not tied to definitions today |
| Effect and capability context | the language owning effects and capabilities | both are MISSING as language features |
| Rename of exported definitions | rename ✓ | unchanged since N18 |
| Edits of files not open | rename ✓, quick fixes ✓ | unchanged since N18 |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | no evidence justifies them |
| Refactors, source actions, fix-all | quick fixes ✓ | not asked for; each is its own contract |
| Incremental reparse, or a persistent analysis for MCP | a lossless CST ✓ | each MCP call re-analyses its root; no evidence yet forces a cache and its freshness contract |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |

### N27 — Semantic context packet v1 and read-only MCP — **complete, 2026-09-27**

The first AI-facing semantic unit. `nazm.context/1` is one function's, record's or enum's exact
source and compact links — identity, kind, name, place — to what it uses, the types it names, what
uses it, what it calls and what calls it, and its own diagnostics, all read from the layers that
already answer them; effects, capabilities, tests and semantic changes are marked unsupported.
Durable identities, source-root-relative paths, one deterministic serialisation. `nazm context`
prints it, and `nazm-mcp`, on the official Rust SDK, serves it as one read-only stdio tool that
reads the disk at every call. No language, interface, cache-key or bootstrap change:
`SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`; one new machine schema.

| Settled | Where |
| --- | --- |
| the packet, `nazm.context/1`, from N17/N18/N20/N21/N22/N25 | `crates/nazm-service/src/context.rs`, `schema/nazm.context-3.json`, `architecture.md` §7.29 |
| `nazm context --root --file --byte --json` | `crates/nazm-cli/src/context.rs` |
| `nazm-mcp`: one read-only stdio tool, disk per call, no other file readable | `crates/nazm-mcp/src/main.rs`, `docs/mcp.md` |
| what it costs | `performance.md` |

Mutation evidence: 13 new mutations, every killer verified; one targeted campaign of 24
(`capability-matrix.md` §30). Closure correction: the MCP result carries the packet once, and v1
links only entities with a declaration in a file of the compilation — three more mutations, all 16
killers re-verified on the corrected source, one campaign of 16.

### After N26, in dependency order

No item is chosen here; N26 left these standing. *Superseded by "After N27" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Rename of exported definitions | rename ✓ | unchanged since N18 |
| Edits of files not open | rename ✓, quick fixes ✓ | unchanged since N18 |
| MCP, semantic context packets | a language service ✓, scope, call, structure, symbols, identifiers, call hierarchy ✓ | done by N27: one packet, one read-only tool |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | no evidence justifies them |
| Refactors, source actions, fix-all | quick fixes ✓ | not asked for; each is its own contract |
| Completion after a comma, at an empty label slot later in a list | completion at a hole ✓ | not asked for; N26 answers an empty list only |
| Incremental reparse | a lossless CST ✓ | a hole's probe re-checks the compilation (about 43 ms on the compiler); warm edits show no delta; still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N26 — Incomplete-source semantic anchors and triggered structure completion — **complete, 2026-09-27**

Completion now answers right after a `.` or a `(` with nothing written yet: a record's fields
after `receiver.`, an enum's variants after `E.` or `E[T].`, a record's labels after `Name(` where
the module calls no function `Name`, and a variant's payload labels after `E.V(` in a construction
or a pattern. The request runs a completion probe — the same parser told the cursor's offset,
which reads a zero-width empty name there, and the same checker, which anchors the hole where it
already decides — and answers with N21's candidates and an empty range. `.` is the one trigger
character. Nothing is recorded by an ordinary analysis, published from the probe, or kept; the
unfinished programs stay invalid and `nazm check` reports them unchanged. No language, interface,
schema, cache-key or bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint
`0e1a40e6…`.

| Settled | Where |
| --- | --- |
| a probe parse with a hole at the cursor; ordinary parses unchanged | `crates/nazm-syntax/src/parser.rs`, `crates/nazm-syntax/src/lib.rs`, `architecture.md` §7.28 |
| `nazm_core::probe`; a bare `Name(`'s record, recorded only in a probe | `crates/nazm-core/src/lib.rs`, `crates/nazm-core/src/check.rs` |
| completion at a hole from `structure::candidates`, shared with N21 | `crates/nazm-service/src/incomplete.rs`, `crates/nazm-service/src/structure.rs` |
| `.` the one completion trigger | `crates/nazm-cli/src/lsp.rs` |
| what it costs: a hole's probe is one parse and check of the compilation; no ordinary-path delta once the hole code was moved out of line | `performance.md` |

Mutation evidence: 8 new mutations and 2 N21 mutations repointed; every killer verified, the four in the parser
again on the final source; one targeted campaign of 28, all caught at tier 1
(`capability-matrix.md` §30).

### After N25, in dependency order

No item is chosen here; N25 left these standing. *Superseded by "After N26" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Completion after a bare `.` or `(`, and a `.` trigger | structure ✓ | done by N26 |
| Rename of exported definitions | rename ✓ | unchanged since N18; call hierarchy is complete only for the compilation it was prepared in, so it proves nothing about other importers |
| Edits of files not open | rename ✓, quick fixes ✓ | unchanged since N18 |
| MCP, semantic context packets | a language service ✓, scope, call, structure, symbols, identifiers and call hierarchy ✓ | not started |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | no evidence justifies them |
| Refactors, source actions, fix-all | quick fixes ✓ | not asked for; each is its own contract |
| Incremental reparse | a lossless CST ✓ | the interleaved N24/N25 comparison detected no warm-edit delta; still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N25 — Semantic call hierarchy: prepare, incoming and outgoing calls — **complete, 2026-09-26**

An editor can now ask which source functions call a function and which it calls, one level at a
time. An item is a function the checker declared from source, prepared from its declaration or
a call through the reference index, at its outline symbol's ranges; an edge is a checked call to
such a function, attributed to the body the checker recorded it in, grouped by the function at
the other end with every callee name. No per-call state, no graph and no item table: each answer
is derived from the current analysis and dropped. An item is bound to the compilation it was
prepared in and the generation it was prepared at, and is unanswered after any change or once
its root closes. It is complete only for that compilation snapshot — not every caller in a
project — so exported rename stays refused. No language, interface, schema, cache-key or
bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| `prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`, `Locator`; `function_symbol` shared with the outline | `crates/nazm-service/src/hierarchy.rs`, `crates/nazm-service/src/symbols.rs`, `architecture.md` §7.27 |
| `textDocument/prepareCallHierarchy` and `callHierarchy/incomingCalls` / `outgoingCalls`, UTF-16, stale item `null` | `crates/nazm-cli/src/lsp.rs` |
| what it costs: no detected analyse/edit/RSS delta; no persistent state; incoming over the compiler about 0.5 ms; +65,536 bytes of binary | `performance.md` |

Mutation evidence: 14 new, 2 repointed, all 16 killers verified on the final source on their
first run; one targeted campaign of 24, all caught at tier 1, in one session
(`capability-matrix.md` §30).

### After N24, in dependency order

No item is chosen here; N24 left these standing. *Superseded by "After N25" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Call hierarchy | checked calls ✓, references ✓, symbols ✓ | done by N25 |
| Completion after a bare `.` or `(`, and a `.` trigger | structure ✓ | needs an anchor for source that does not parse |
| Rename of exported definitions | rename ✓ | unchanged since N18 |
| Edits of files not open | rename ✓, quick fixes ✓ | unchanged since N18: neither a rename nor a fix edits a closed file |
| MCP, semantic context packets | a language service ✓, scope, call, structure, symbols and identifiers ✓ | not started |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | no evidence justifies them |
| Refactors, source actions, fix-all | quick fixes ✓ | not asked for; each is its own contract |
| Incremental reparse | a lossless CST ✓ | the interleaved N23/N24 comparison detected no warm-edit delta; still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N24 — Diagnostic-backed code actions and versioned fix plans — **complete, 2026-09-26**

The fixes the compiler already attaches to its diagnostics now reach an editor as quick fixes:
each current diagnostic the requested range touches has one plan per fix — its applicability,
description and precondition, the generation, and one edit tied to the open document's version
with the bytes it replaces — held to the freshness law renames answer to, and sent as a
versioned `documentChanges` edit. Nothing is invented from a code or message, and the server
writes nothing. `nazm fix`'s seven skip reasons each have a test that reaches them, and area 24
is VERIFIED. No language, interface, schema, cache-key or bootstrap change: `SEMANTIC_EPOCH` 7,
`nazm.interface/5`, `nazm.fix/1`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| `fix_plans`, `FixPlan`, `fix_plan_is_current`; `file_is_current` shared with rename | `crates/nazm-service/src/fixes.rs`, `crates/nazm-service/src/rename.rs`, `architecture.md` §7.26 |
| `textDocument/codeAction` quick fixes, versioned, UTF-16; no resolve, refactor, source or fix-all | `crates/nazm-cli/src/lsp.rs` |
| the seven `nazm fix` skip reasons, each reached by a test | `crates/nazm-cli/src/fix.rs`, `diagnostics.md` |
| what it costs: no detected analyse/edit/RSS delta; no persistent state; a document's plans in microseconds, a request about 0.1 ms; +65,536 bytes of binary | `performance.md` |

Mutation evidence: 17 new, 2 repointed, all 20 affected killers verified on the final source
(one after its test was extended to reach the case it guards); one targeted campaign of 29, all
caught at tier 1, in one session (`capability-matrix.md` §30).

### After N23, in dependency order

No item is chosen here; N23 left these standing. *Superseded by "After N24" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Code actions | a language service ✓, `nazm.fix/1` ✓ | quick fixes done by N24 |
| Call hierarchy | checked calls ✓, references ✓, symbols ✓ | not asked for |
| Completion after a bare `.` or `(`, and a `.` trigger | structure ✓ | needs an anchor for source that does not parse |
| Rename of exported definitions; edits of files not open | rename ✓ | unchanged since N18 |
| MCP, semantic context packets | a language service ✓, scope, call, structure, symbols and identifiers ✓ | not started |
| `semanticTokens/range` and `/full/delta` | semantic tokens ✓ | a full request is 9 ms on the largest file; no evidence justifies a cache |
| Incremental reparse | a lossless CST ✓ | the interleaved N22/N23 comparison detected no warm-edit delta; still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N23 — Semantic identifier classification and LSP semantic tokens — **complete, 2026-09-26**

What each identifier of a document is — its class, whether it declares or refers, and the
compiler's identity for what it names — is now answered from what the checker recorded at its
exact span: the reference index's entities, built-in calls, and one narrow new record of the
written built-in and type-parameter names the checker resolved. An identifier the checker did
not resolve is not classified. `semanticTokens/full` is that answer under a fixed legend. No
language, interface, cache-key or bootstrap change: `SEMANTIC_EPOCH` 7, `nazm.interface/5`,
fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| `Resolution::written_type`, written where the checker resolves a built-in or type-parameter name | `crates/nazm-sema/src/resolve.rs`, `crates/nazm-core/src/check.rs`, `architecture.md` §7.25 |
| `semantic_identifiers`, `SemanticIdentifier`, `SemanticClass`, `Role`, `SemanticTarget` | `crates/nazm-service/src/semantic.rs` |
| `textDocument/semanticTokens/full` under a fixed legend, relative and in UTF-16; no range, no delta | `crates/nazm-cli/src/lsp.rs` |
| what it costs: +~180 kB resident with the compiler open (1,989 recorded names); no detected warm-edit delta; a possible sub-millisecond analyse cost not resolved by the measurement; a whole compiler file in 3 ms, 9 ms over the protocol; no binary growth | `performance.md` |

Mutation evidence: 14 new, all 14 direct killers verified on the final source, 2 re-verified;
one targeted campaign of 26, all caught at tier 1, in one session (`capability-matrix.md` §30).

### After N22, in dependency order

No item is chosen here; N22 left these standing. *Superseded by "After N23" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Semantic tokens, code actions | a language service ✓, symbols ✓ | semantic tokens done by N23 |
| Call hierarchy | checked calls ✓, references ✓, symbols ✓ | not asked for |
| Completion after a bare `.` or `(`, and a `.` trigger | structure ✓ | needs an anchor for source that does not parse |
| Rename of exported definitions; edits of files not open | rename ✓ | unchanged since N18; workspace symbols cover what is loaded, which is not every possible importer, so they do not lift it |
| MCP, semantic context packets | a language service ✓, scope, call, structure and symbols ✓ | not started |
| Incremental reparse | a lossless CST ✓ | N22 adds no analysis-time work, and the interleaved N21/N22 comparison detected no warm-edit delta; still no evidence that forces it |
| `nazm explain-cost` | nothing above | independent |
| User-defined traits, or any capability beyond equality | generics ✓, a requirement ✓ | unchanged since N14 |
| `?` on `Option`, and conversion between error types | traits | unchanged |

### N22 — Semantic document and workspace symbols — **complete, 2026-09-26**

Which declarations exist, where each is written and what contains it is now answered from the
checker's definition tables and the parsed items their `decl` indices name — derived on demand,
with nothing kept. One document's outline is its declarations, nested, in source order; the
workspace is every declaration of every file the open documents' compilations load, once each,
identified by file, kind and name span. It is not a project index and does not lift N18's
exported-rename refusal. No language, interface, cache-key or bootstrap change:
`SEMANTIC_EPOCH` 7, `nazm.interface/5`, fixpoint `0e1a40e6…`.

| Settled | Where |
| --- | --- |
| `document_symbols`, `workspace_symbols`, `Symbol`, `SymbolKind`, `WorkspaceSymbol` | `crates/nazm-service/src/symbols.rs`, `architecture.md` §7.24 |
| `textDocument/documentSymbol` (nested, or flat for a client without hierarchy) and `workspace/symbol` (no resolve) | `crates/nazm-cli/src/lsp.rs` |
| what it costs: no detected analyse/edit/RSS delta in the interleaved comparison; no persistent symbol table; an outline in microseconds, a workspace query 2.2 ms over the compiler as four programs; +65,536 bytes of binary | `performance.md` |

Mutation evidence: 13 new, all 13 direct killers verified on the final source (one after its
killer was moved to the test that observes it), 2 re-verified; one targeted campaign of 24, all
caught at tier 1, in one session (`capability-matrix.md` §30).

### After N21, in dependency order

No item is chosen here; N21 left these standing. *Superseded by "After N22" above.*

| | Depends on | Why not before |
| --- | --- | --- |
| Document and workspace symbols | a language service ✓, structure ✓ | done by N22 |
| Semantic tokens, code actions | a language service ✓ | not started |
| Call hierarchy | checked calls ✓, references ✓ | not asked for |
| Completion after a bare `.` or `(`, and a `.` trigger | structure ✓ | needs an anchor for source that does not parse |
| Rename of exported definitions; edits of files not open | rename ✓ | unchanged since N18 |
| MCP, semantic context packets | a language service ✓, scope, call and structure at a position ✓ | not started |
| Incremental reparse | a lossless CST ✓ | the contended interleaved N20/N21 comparison detected no N21-specific warm-edit delta; there is still no evidence that forces incremental reparse |
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
| what it costs: no detected analyse/edit/RSS delta in the contended interleaved comparison; no persistent clean-code structure table; structure queries measured separately (microseconds, whole-compiler queries dominated by finding the token) | `performance.md` |

Mutation evidence: 15 new, all 15 direct killers verified on the final source; one targeted
campaign of 34, all caught at tier 1, over three sessions (`capability-matrix.md` §30).

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
