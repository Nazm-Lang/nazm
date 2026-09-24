# Capability matrix

**Status: this file is the current status authority for the tree it is committed in.**

Two things are kept apart here. **`4fb15542dfbf9af0bf34c42fe52783637b3daed4` is a frozen
historical candidate**: the commit a retained release candidate was built and tested at.
Its evidence is [`releases/4fb1554.md`](releases/4fb1554.md), which is immutable and is not
updated by anything that happened since. **The claims below are current-tree claims**, and
they include behaviour added after that candidate — the milestones N7 through N13, each of
which changed the compiler and ran its own evidence. They are not grounded in `4fb1554`,
and nothing here says the tree has changed only in comments since then.

Each row carries the evidence for the milestone that changed it — the tests, the contained
runs and the dates — so a claim is checked against its row, not against a single commit. A
row that rests on nothing newer than the candidate says so or cites only what the candidate
already had.

This is one table serving two purposes. The directive asked for a capability matrix and a
gap map; they are the same information at the same granularity, so the five fields of a
gap map are the columns of the matrix and there is one document instead of two.

**Vocabulary:** VERIFIED · PARTIAL · DESIGNED · RESEARCH · BLOCKED · MISSING, defined in
[`master-architecture.md`](master-architecture.md) §1. A VERIFIED row cites a path; a
`cargo xtask check` rule fails if it does not, or if a cited path stops existing.

**What this document does not cover.** Construct-level support — whether `while` or
`spawn` or a given built-in runs interpreted, compiles natively, or is refused — is
answered by the compiler itself:

```sh
cargo run -p nazm-cli --bin nazm -- capabilities --json
```

That output is read from the compiler's own dispatch tables (`crates/nazm-cli/src/capabilities.rs`),
so it cannot drift from the implementation the way a written table can. Ask it, not this
file. The schema is `schema/nazm.capabilities-1.json`.

---

## Summary

**Current suite: 1,291 passed, 0 failed, 13 ignored, across 65 suites**, run contained at
N13 (2026-09-25); the thirteen ignored are the xtask lifecycle tests, run on their own (13 of
13 passed, contained). N12.2 ended at 1,255 across 63, N12.1 at 1,211 across
63, N12 at 1,186 across 62, N11 at 1,125 across 58. The frozen count at the tested release commit is in
[`releases/4fb1554.md`](releases/4fb1554.md) and does not move.


| # | Area | Status |
| --- | --- | --- |
| 1 | Syntax, parser, CST | PARTIAL |
| 2 | Name resolution and modules | PARTIAL |
| 2a | Incremental semantic checking | VERIFIED |
| 3 | Type system | PARTIAL |
| 4 | Memory model and reclamation | VERIFIED for the current type universe |
| 5 | Effects | MISSING |
| 6 | Capabilities (language) | MISSING |
| 7 | Provenance and information flow (language) | MISSING |
| 8 | Core IR | MISSING |
| 9 | MIR | MISSING |
| 10 | LIR and the backend contract | PARTIAL |
| 10a | Per-module code generation and linking | VERIFIED |
| 10b | Native object reuse | VERIFIED |
| 11 | LLVM backend | VERIFIED |
| 12 | Cranelift backend | DESIGNED |
| 13 | Runtime | PARTIAL |
| 14 | Structured concurrency | VERIFIED |
| 15 | Advanced scheduler | RESEARCH |
| 16 | Standard library | MISSING |
| 17 | FFI and ABI | MISSING |
| 18 | Embedded support | MISSING |
| 19 | Critical profile | DESIGNED |
| 20 | Cybersecurity profile | DESIGNED |
| 21 | AI/HPC | RESEARCH |
| 22 | Diagnostics | VERIFIED |
| 23 | Formatter | VERIFIED |
| 24 | Machine-applicable fixes | PARTIAL |
| 25 | LSP and MCP | MISSING |
| 26 | Package and build tooling | PARTIAL |
| 27 | Debugger and profiler | MISSING |
| 28 | Performance measurement | PARTIAL |
| 29 | Differential testing | VERIFIED |
| 30 | Mutation testing and fuzzing | VERIFIED / MISSING |
| 31 | Bootstrap | VERIFIED |
| 32 | Reproducibility and provenance | PARTIAL |
| 33 | Platform and target matrix | PARTIAL |

Nine VERIFIED, twelve PARTIAL, three DESIGNED, two RESEARCH, nine MISSING. The shape is
worth reading directly: **everything verified is either a front-end capability, a tooling
interface, or an evidence mechanism.** Nothing in the semantic core — ownership, effects,
capabilities, provenance — exists at all, and that is the accurate summary of where the
project stands after the first era.

---

## Front end

### 1. Syntax, parser, CST — PARTIAL

**Evidence** · Hand-written lexer and recursive-descent/Pratt parser,
`crates/nazm-syntax/src/lexer.rs` and `crates/nazm-syntax/src/parser.rs`. The grammar is a
tested artefact, not prose: `crates/nazm-syntax/tests/grammar.rs` requires every lexer
token to appear in `docs/grammar.ebnf`, every nonterminal to be reachable from `program`,
and every `(* @example *)` in it to parse with zero diagnostics. `docs/guide.md`'s syntax
section is generated from the same file by `xtask/src/guide.rs`.

*Since N1, each file is parsed on its own text through `nazm_syntax::parse_map`, so the
parser no longer requires a concatenated buffer. Since N2 the grammar has one visibility
bit — `function = [ visibility ] , "fn" , ...` — and the AST records `pub_span`, which is
where the word was written and nothing about what it means.*

*Since N11, `[` and `]` are tokens and mean one thing: type parameters after a declared name
(`type_parameters`) and type arguments after a type or an expression's name
(`type_arguments`). A `TypeName` is recursive, carries its name's span and its whole span,
and counts against the parse budget; `[]` parses and is the checker's to refuse. The
Nazm-written parser produces the same tree for every generic shape in
`the_nazm_parser_produces_the_same_tree_as_the_reference`, spans included.*

*Since N12, `?` is a token and one postfix operator, parsed beside projection in any order —
`primary = atom , { "." , IDENT | "?" }` — so `-r?` is `-(r?)` and `r??` is two layers. The
formatter glues it to its operand; the Nazm-written parser produces the same trees.*

**Limitation** · There is **no CST**. `crates/nazm-syntax/src/ast.rs` is a plain owned
AST; comments are returned as a parallel span list consumed only by the formatter, and
never reach the tree. Error recovery is one strategy at one granularity — skip to the next
`fn` — so a malformed statement loses its whole function body. Parse nesting is bounded at
512 with a 24 MiB parse stack.

**Next dependency** · A lossless green tree (`cstree` is the recorded choice,
`architecture.md:344`), which is also what safe structured edits and any language server
need.

**Accepted when** · A round trip through the tree preserves every byte including trivia,
and the parser produces a usable tree for a file with an error in the middle of a function.

### 2. Name resolution and modules — PARTIAL

**Evidence** · Lexical scoping with shadowing rules and a separate function namespace,
`crates/nazm-core/src/check.rs`. `use "relative/path.nz";` resolved by the driver,
`crates/nazm-cli/src/load.rs` — canonical-path keyed, read-once, depth-first, cycles
terminate rather than erroring. A second resolver written in Nazm, `compiler/module.nz`,
serves the bootstrap stages. Codes `N0200`–`N0208` in `crates/nazm-diag/src/lib.rs`.

**Resolution is singular as of N1 (2026-09-21).** `nazm_core::check` produces a
`nazm_sema::Resolution` — function definitions, call targets, and the slot every name
mention refers to — and both the evaluator and `crates/nazm-lir/src/lower.rs` read it.
Since N2 that is true of the **evaluator** as well: its `HashMap<String, usize>` over every
function, and its own copy of the built-in-first rule, are gone. The backend's last two
name lookups went with them — `crates/nazm-lir/src/lower.rs` pairs definitions with
declarations through `FnDef::decl` and finds the entry point through `Resolution::entry`,
so neither consumer knows the spelling `main`.

**Source identity is real as of N1.** `nazm_span::Span` is `{ file, start, end }` with
file-local offsets, and `crates/nazm-cli/src/load.rs` produces a `SourceMap` whose files
are parsed separately. The merged buffer, and the binary search that recovered a file from
an offset, are gone.

**There is a compilation unit as of N2 (2026-09-22).** One module is one file.
`crates/nazm-sema/src/module.rs` holds `ModuleId` and the `ModuleGraph` the driver builds
while loading; `crates/nazm-sema/src/interface.rs` holds the `UnitInterface` a module
offers. Checking is two stages, `declare_unit` then `check_unit`
(`crates/nazm-core/src/check.rs`), and the second takes its dependencies as **interfaces**.
`crates/nazm-core/tests/unit_checking.rs` checks a module against an interface for a module
whose source does not exist, and refuses the same module when that interface is empty.

**Visibility exists as of N2.** Top-level definitions are private to their module; `pub`
exports; imports are not transitive; every collision is a diagnostic naming both sides
(`N0206`–`N0208`). Twenty cases in `crates/nazm-cli/tests/visibility.rs`, most of them run
interpreted and compiled and required to agree. The self-hosted compiler was migrated
rather than exempted: 84 of its 224 functions are `pub`, 140 are private, and since N2.1 it enforces the rule as well as expressing it.

**A toolchain-owned module exists as of N12 (2026-09-24).** The core prelude,
`library/core/prelude.nz`, is loaded into every compilation after its project modules and
imported by every one of them with no `use` (`ModuleGraph::attach_prelude`). Its key,
`@core/prelude`, is fixed by the toolchain and disjoint from every project key; its two type
names are reserved (`N0363`). It is a dependency like any import, so it is in every module's
check key. It is **not** a package system.

**Durable identity and persisted interfaces exist as of N3 (2026-09-22).** A module's
`ModuleKey` is its normalised path relative to the compilation's source root, and a
definition's `DefKey` is that key plus its kind and name
(`crates/nazm-sema/src/key.rs`). `nazm interface FILE` writes a module's interface as
`nazm.interface/3` — durable keys, canonical type names, exported **record and enum
definitions** closed over their field and payload types, no spans, no session ids, no
discriminants (`crates/nazm-iface/`) — and `--hash` fingerprints it with BLAKE3. It was
`/1` until N9 and `/2` until N10; a `/1` file describes a language with no records and a
`/2` one a language with no enums, so reading either as a `/3` would be believing a module
offers less than it does, and every direction is refused rather than reinterpreted. The session ids are
unchanged and were not renamed; both layers coexist.

`crates/nazm-cli/tests/durable_identity.rs` establishes it across real boundaries: the same
project gives identical bytes in two processes and in two different directories, a module
whose private body changed gives the same fingerprint, and **a dependent checks against a
deserialised interface with the dependency's source deleted.** A module outside the source
root, or one a symbolic link gave two names, is refused a durable key rather than given a
guess.

**A project can declare where it begins, as of N4 (2026-09-22).** An empty `nazm.root`
marker, or `--source-root DIR`, fixes the root a module's key is relative to, so
`lib/common.nz` is the same module whichever entry file named it
(`crates/nazm-cli/src/root.rs`, ten cases in `crates/nazm-cli/tests/source_root.rs`). With
neither, the root is derived from the named file exactly as before. The marker must be
empty and is refused otherwise: it says where a project begins and owns nothing else.

**Limitation** · A module still cannot export a name under another one, re-export, or be
named by anything but a relative path. *The sentence that stood here — that nothing is
separately compiled — was true until N5 and is now area 10a.*

**Next dependency** · Aliasing and re-export are language decisions; separate code
generation needs a linking model (area 10).

**Accepted when** · Met for identity, interfaces and a declared root.

### 2a. Incremental semantic checking — VERIFIED

**Evidence** · `nazm check` reuses the result of checking a module whose semantic inputs
are unchanged, as of N4 (2026-09-22). `crates/nazm-cache/` computes what those inputs are —
`SourceFingerprint` over the exact source bytes, `CheckerIdentity` over the checker's own
tables plus an explicit `SEMANTIC_EPOCH`, and `CheckKey` over both plus each direct
import's `(ModuleKey, InterfaceHash)` — and stores a `nazm.check/1` entry per module in
`.nazm/check/`, published by `rename` and validated on read.
`crates/nazm-core/src/check.rs` gained one hook, `Reuse`; `check_unit` is still the only
thing that decides what a body means.

Twenty-nine tests in `crates/nazm-cli/tests/incremental.rs`, each running the binary
repeatedly, establish the model: a private body change re-checks its own module and leaves
its importer alone; an exported signature change re-checks the importer and finds the error
it now has; in `A → B → C`, a change to C that leaves B's interface unchanged stops at B.
Sixteen more in `crates/nazm-cache/tests/store.rs` break one thing about a stored entry —
truncation, a future schema, an edited fingerprint, an edited interface, a valid entry
under the wrong name — and every one is a miss.

**What is reused, exactly** · The second stage of checking a module: its bodies. Every
module is still read, parsed and declared from source on every run, which is why a skipped
module is invisible — the interface other modules' keys depend on, and whether the
compilation has a `main`, are recomputed rather than recalled.

**Limitation** · **This is not incremental native compilation, and no such claim is made.**
`nazm run` and `nazm build` reuse *this* cache not at all: both need the resolution that
only checking a body produces, and nothing persists one. (`nazm build` reuses objects as of
N6 — a different cache, a different key, area 10b — and a build that reused every object
still checked every module.) There is no IR, LIR or resolution cache, no query database, no
scheduler and no eviction policy. A module with no durable identity is checked every time.

**And the measured gain is about a millisecond.** On the compiler's own five modules,
`performance.md` records 40.5 ms with the cache disabled against 38.9 ms warm — 18% of the
work above a 31.7 ms process floor, and 3.9% of the wall clock. The first run is **24 ms
slower**, because publishing five entries costs an `fsync` each. The decisions are right;
the saving is small because parsing and declaring happen every run and are most of the
cost. Recorded rather than averaged away: a reader deciding whether to depend on this
should know it buys correctness of invalidation, not speed.

**Next dependency** · A written form for name resolution, which is what would let a build
skip a module's bodies the way a check can. Not a `CodegenKey`: N6 found that native reuse
needed no semantic key at all (area 10b), so the dependency this row once named is gone
rather than met.

**Accepted when** · Met for semantic checking. Native reuse is a separate row that does not
exist.

### 3. Type system — PARTIAL

**Evidence** · Six built-in types, `crates/nazm-sema/src/types.rs`: `Int`, `Bool`, `Str`,
`Ints`, `Strs`, `Chan` — **and, since N9, user-defined records; since N10, user-defined
enums**. Local inference for `let`; parameters and returns always written. Completion
analysis decides whether a block produces a value, which is what lets one `if` serve both
statement and value position. Codes `N0300`–`N0314` and `N0330`–`N0345`. 114 behavioural
tests in `crates/nazm-core/tests/language.rs`, 33 in `crates/nazm-core/tests/records.rs`,
47 in `crates/nazm-core/tests/enums.rs`.

*N9, 2026-09-23.* `struct Point { x: Int, y: Int, }` — nominal value records with named
fields, named construction, projection, field replacement through a projection path,
nesting, and separate type and function namespaces. The type enum stopped being closed:
`Type::Record(RecordId)` names a definition rather than a shape, because two records with
identical fields are different types. Every property a record has — cleanup, task safety,
copy, destroy — is **derived from its fields**, recursively, and that composition rather
than the syntax is what the milestone was for. `docs/spec.md`, *Records*, is the law;
`architecture.md` §7.10 is the implementation.

*N10, 2026-09-23.* `enum State { Ready, Done(code: Int), }` with `match` — closed nominal
sum types with named payload fields, qualified construction, exhaustive variant matching
with named payload bindings, and composition with records in both directions. The
milestone's question was not whether enums compile but whether the ownership model holds
when **which fields exist is a runtime fact**: copy and release act on the active variant
alone, derived and dispatched in the emitted program, while task safety stays a property of
the **type** and quantifies over every variant. `docs/spec.md`, *Enums*, is the law;
`architecture.md` §7.11 is the implementation.

*N11, 2026-09-23.* First-order parametric generics — `struct Pair[A, B]`, `enum Maybe[T]`,
`fn identity[T]` — checked **once, parametrically**, with call-site type arguments written
or inferred from the arguments and never from the expected result, and one generic
container, `Vec[T]`. An applied type is an interned table entry with substituted fields, so
every derivation above holds after substitution unchanged; parameters are positions, so
renaming one changes no interface byte. Codes `N0350`–`N0359`. 40 tests in
`crates/nazm-core/tests/generics.rs` with code and span, 7 native ones in
`crates/nazm-cli/tests/generics.rs`, and `the_nazm_checker_agrees_with_the_reference_on_generics`
comparing both checkers on 42 programs by code **and** span. `docs/spec.md`, *Generics*, is
the law; `architecture.md` §7.12 is the implementation.

*N12, 2026-09-24.* **Typed error values.** `Result[T, E]` and `Option[T]` are ordinary
generic enums declared by the core prelude, and `?` propagates the core `Result` — recognised
by definition, never by name or shape — with the exact rule `expr: Result[T, E]` in a function
returning `Result[U, E]` gives `expr?: T`, the identical `E` required. No exceptions, no
unwinding, no conversion between error types, no `?` on `Option`. Codes `N0360`–`N0363`. 33
tests in `crates/nazm-core/tests/results.rs` with code and span, 12 native memory cases in
`crates/nazm-cli/tests/propagation.rs`, 10 module, interface and cache cases in
`crates/nazm-cli/tests/prelude.rs`, and
`the_nazm_checker_agrees_with_the_reference_on_propagation` comparing both checkers on 22
programs by code **and** span. `docs/spec.md`, *Typed error values*, is the law;
`architecture.md` §7.13 is the implementation.

*N12.1, 2026-09-24, changed no rule here.* A block used as a value — a `match` arm, a `let`
initialiser, an argument, a field, an operand — was always part of the checked language; it
now also compiles natively in both compilers, which is area 9's and area 10's row rather
than this one's. What this row gains is the first `Option` doing real work inside the
compiler: `record_owned` returns `Option[Int]` instead of a `-1`.

*N13, 2026-09-25.* **Derived structural equality.** `==` and `!=` apply to two operands of
one type when that type has equality, and for a user type it is derived, never declared:
a record has it exactly when every field does, an enum exactly when every payload field of
**every** variant does, and a concrete generic instance after substitution — so
`Option[Int]` and `Result[Int, Str]` have it as ordinary enums, with no rule naming them.
One derivation, `UserTypes::equality` in `crates/nazm-sema/src/udt.rs`, beside cleanup and
task safety and independent of both; the checker asks it and nothing else, and its refusal
names the blocking component (`Outer`'s `inner.values`). No code was added — the rule moved
under `N0304` — so `SEMANTIC_EPOCH` went 5 → 6; the interface schema is unchanged at
`nazm.interface/4`. 20 tests in `crates/nazm-core/tests/equality.rs` with code and span, 11
native ones in `crates/nazm-cli/tests/equality.rs` at `-O0` and `-O2` with every memory
counter, `the_nazm_checker_agrees_with_the_reference_on_equality` comparing both checkers on
30 programs by code **and** span, and the interface-only case with the dependency deleted.
`docs/spec.md`, *Equality is derived*, is the law; `architecture.md` §7.15 is the
implementation.

| | equality |
| --- | --- |
| `Int`, `Bool`, `Str` (primitive) | VERIFIED, unchanged — `Str` by its bytes |
| record, derived | VERIFIED |
| enum, derived over every variant | VERIFIED |
| concrete generic instance, after substitution | VERIFIED |
| `Option[T]` / `Result[T, E]`, inherited as ordinary enums | VERIFIED |
| `Vec[T]`, `Ints`, `Strs` — sequence equality | absent, deliberately |
| `Chan` | absent, deliberately |
| equality over a constrained type parameter | absent — no constraint mechanism |
| custom (user-defined) equality | absent |

Six constructs are verified rather than partial: **records**, **closed enums**,
**exhaustive variant match**, **first-order generics with `Vec[T]`**, **typed `Result`
propagation** and **derived structural equality**. What surrounds them is not. Typed errors are **not** effects: a `Result` in a
signature is a return type, and area 5 is unchanged.

**Limitation** · Generics are unconstrained: no traits, no constraints, no methods or
`impl`, so a generic body can do only what every type can. `Option` has no `?`, `Result` no
methods and no conversion between error types, and `main` cannot return one; no higher kinds, const generics, defaults, variance or specialisation; no
indexing syntax on a `Vec`; no `spawn` of a generic function. No closures or function
values, no unit type, no `Never`.
Equality cannot be abstracted over: `fn same[T](a: T, b: T)` is refused, and so is
`fn same_box[T](a: Box[T], b: Box[T])`, while a concrete `Box[Int]` compares — nothing can
say "`T` has equality". No ordering, hashing or custom equality on any user type, and no
equality at all on a sequence, a `Vec` or a `Chan`. No field-level or per-variant visibility, no
default field values, no record update syntax. The pattern language is variant selection
with named payload bindings and `_` — no top-level wildcard arm, no guards, no or-patterns,
no literal, record, tuple or range patterns, and no `let` destructuring. Five keywords
remain reserved and refused by name rather than silently absent
(`crates/nazm-syntax/src/parser.rs`); `struct` left that list in N9 and `enum` and `match`
in N10, for the same reason `print` did in B3 — a reserved word earns its place by naming
something absent. *Until N11 this said neither a record nor an enum could be put in a
sequence; `Vec[T]` holds either.*

**Next dependency** · A way for a generic definition to require a capability of its
parameter — constrained generics — which is the one thing N13 found equality cannot do
without: a concrete `Box[Int]` compares and a generic body over `Box[T]` cannot say it
wants one that does (`research-register.md`, *Derived equality composes; abstraction over
it does not*). *Until N13 this named equality on user types, which N13 added and the
compiler's completion states now use; until N12 it named typed errors; until N12.1 blocks
as values in the native backends.*

**Accepted when** · Chosen by the milestone that needs it, not by completeness.

---

## Semantic core

### 4. Memory model and reclamation — VERIFIED for the current type universe

*This row was "Ownership and resource semantics — MISSING" until N7 (2026-09-22), and
PARTIAL between N7 and N8 (the same day). The paragraph it used to carry is kept below,
because what it said was true and the numbers in it are what the work was measured against.*

**Read the status carefully.** VERIFIED here means: **every heap-backed type the language
currently has is reclaimed, in both compiler implementations and in the reference
interpreter, and that is tested rather than asserted.** It does **not** mean whole-language
leak freedom, and it says nothing about types that do not exist.

*N9, 2026-09-23, extended the type universe and the claim with it.* Records are covered:
a record owns exactly what its fields own, recursively, and copy, destroy, cleanup and
task safety are all **derived** rather than declared. The cycle argument survives because
`docs/spec.md`'s *Cycles* was satisfied the first way it offered — the type system refuses
containment cycles (`N0336`), because a record holds its fields by value and a cycle has
no finite layout.

*N10, 2026-09-23, extended it to variants.* An enum owns exactly what its **active**
variant's payload owns: copy, destroy and cleanup dispatch on the discriminant in the
emitted program, and an inactive variant is ownership-inert. A payload is held inline like
a field, so the same refusal (`N0336`) covers a containment cycle through a variant, across
modules too.

*N11, 2026-09-23, extended it to generics and `Vec[T]`.* An applied record or enum owns
exactly what its substituted fields own; a `Vec[T]` is a reference-counted handle whose
elements are copied in by `vec_push` and `vec_set`, copied out by `vec_get`, transferred
out by `vec_pop`, and released by `T`'s law when the last reference dies. A `Vec` is the
first way a definition can reach itself with a finite layout, so the argument needed its
second satisfaction, and got it the first way again: every definition whose values could
own themselves through a counted handle is refused (`N0359`), over an ownership graph that
keeps acyclic nesting and phantom parameters legal.

*N12, 2026-09-24, needed no extension.* `Result` and `Option` are ordinary enums, so they
reclaim by the enum law, and `?` is a `match` with a `return`: its success payload takes its
own reference before the temporary `Result` is released, and its error payload is owned by
the returned `Result` before anything it came out of is. Every shape — `Ok` and `Err` of
`Int`, `Str`, a record, an enum, a `Vec` and a `Chan`; `None` and `Some` of four kinds — and
every way out — a half-evaluated call, a half-built record or variant, a scope with a running
task, three functions deep — reports `live=0` in both implementations, under allocator churn,
in `crates/nazm-cli/tests/propagation.rs`.

**Evidence** · `docs/spec.md`'s *memory constitution* settles assignment, parameters,
returns, aliasing, mutation, reclamation, cross-task transfer, cycles and cost visibility
as four kinds of value and five rules. `docs/architecture.md` §7.7 carries the audit it was
derived from, §7.8 the string and channel model, §7.9 the one classification cleanup
dispatches on. No program that was legal before changed its meaning.

| Type | Storage | Status |
| --- | --- | --- |
| `Int`, `Bool` | none | — |
| `Str` | a backing allocation, named by the value's third field; null for a literal or an `arg` | **VERIFIED** |
| `Ints`, `Strs` | a reference-counted header; a `Strs` releases every element's backing first | **VERIFIED** |
| `Chan` | a reference-counted header, with its ring, mutex and condition variable | **VERIFIED** |
| a **record** | none of its own — an inline aggregate whose fields own what they own | **VERIFIED**, composed: one generated retain and release helper per record that owns anything, and none at all for one that does not |
| an **enum** | none of its own — an inline discriminant and payload slots; only the active variant's payload owns anything | **VERIFIED** since N10: one generated helper pair per enum that owns anything, dispatching on the discriminant |
| an applied **record** or **enum** | as its definition's, with the substituted field types | **VERIFIED** since N11: its own helpers, generated from the substituted fields |
| `Vec[T]` | the sequence header an `Ints` has, holding elements of `T`'s layout | **VERIFIED** since N11: one release helper per owning element type, releasing each element before the storage; a `Vec` of non-owning elements is released as an `Ints` is |
| a `spawn`'s argument block, a file path's terminated copy | runtime-internal | **VERIFIED** — freed since N8; each was one `malloc` per operation before |
| a literal's constant, the process's `argv` | not this program's | **not reclaimed, correctly.** Nothing allocated them and nothing may free them |

**How.** Reference counting, which `docs/spec.md` deliberately does **not** make the law —
the rule is that storage stays valid while a reference exists, and an arena, an interning
pass or an escape analysis would satisfy it. The count is non-atomic on a sequence because
`N0321` refuses to let one cross into a task, and atomic on a string backing and a channel
because both do cross. Cleanup is on the three exit edges of a generated function — the
tail, each `return`, and the shared unwind block a failure takes — with parameter slots
excluded, which *is* rule 2.

**Tested.** 68 cases in `crates/nazm-cli/tests/memory.rs`, each run twice and each requiring
the interpreter and the compiled program to agree on the value **and** on what they
allocated and reclaimed, by class: slices outliving their buffers, slices of slices, empty
strings, borrowed strings returned on both edges, strings stored in and read out of
sequences, a string read out of a **temporary** sequence, self-assignment of an element,
embedded NUL bytes, a process argument, the failure path, channel scaling, a returned
channel, blocked senders and receivers woken by a close, a task that fails while holding a
channel, and a string and a channel crossing into two tasks at once.

*The 27 record cases added by N9* cover: a scalar record allocating nothing, a heap-string
field, a sliced-string field, `Ints` and `Strs` and `Chan` fields, a nested owning record,
a copy taking one reference per owning field, whole-record replacement including
self-assignment, field replacement including one backed by the same allocation, tail and
explicit return, a projection out of a temporary, a failed constructor at one and two
levels, a failing replacement expression, a task-safe record crossing into two siblings, a
record built in one branch of an `if`, four hundred short-lived records against a constant
live set, and three cases where a failure leaves while a temporary is still in hand.

*The 14 enum cases added by N10* cover: an inactive owning variant owning nothing, a
zero-payload variant beside owning ones, a copy retaining the active payload once, a
payload returned out of an arm (and still valid after the allocator reused the block), a
discarded payload, replacement across variants, a failed payload initialiser, a failure
inside an arm, a matched sequence handle, a constant live set of short-lived enums, an enum
inside a record and a record inside an enum, a nested enum, and an enum crossing into two
tasks. The Nazm-written compiler is held to the same counts by four enum programs in
`the_nazm_written_compiler_emits_the_same_memory_semantics`.

*The 12 generic cases added by N11* cover: `Vec[Int]` under the `Ints` law, `Vec[Str]`
owning each string, a record read out of a `Vec` surviving its slot being overwritten, a
`Vec` of enums releasing only each element's active payload, a nested `Vec` releasing each
inner vector and what it holds, a `Vec[Chan]` holding a reference to each channel, a `Vec`
returned by tail and by `return`, a generic function returning each kind of value by its own
law, a generic record releasing each field by its substituted type, an element replaced by a
copy of itself surviving the allocator, a failed `Vec` operation giving back what the frame
held, and many short-lived vectors keeping a constant live set. The Nazm-written compiler is
held to the same counts by five generic programs in the same test.

**Two of those were written because a mutation survived**, and the pair is the lesson.
Removing `strs_get`'s own reference broke nothing the suite could see, because a binding
retains what it stores whether or not the built-in already did — the shape that finds it is
a sequence with **no** binding. And releasing a replaced field before taking the new value
broke nothing the *counters* could see, because both orders end with the same two numbers —
the shape that finds it is `h.name = h.name` followed by enough small allocations that the
block the wrong order freed is handed back out, after which the program stops with a
signal. A compensating mechanism makes a defect invisible without making it absent, and
the compensation is sometimes the allocator.

**Measured, N11** (contained, Linux aarch64). 5,000 / 20,000 / 80,000 short-lived `Vec[Str]`,
`Vec[Row]`, `Vec[Tok]` and `Vec[Vec[Str]]` against a constant live set all peak at **1.17 MiB**,
the empty program's floor, with nothing live at exit. `Vec[Int]` and `Vec[Str]` cost what
`Ints` and `Strs` cost. `performance.md` has the rows, and the enum-in-a-`Vec` pressure: 48
bytes a token against 40 for parallel arrays, +18% peak.

**Measured, N9.** 80,000 short-lived two-string records against a constant live set:
**1.72 MiB**, against 1.73 MiB for the same two strings without a record, and 1.69 MiB for
an empty program. The N8 controls are unmoved — 80,000 temporary strings 1.77 MiB, 80,000
short-lived sequences 1.78 MiB, 2,000 short-lived channels 1.75 MiB. In time, at `-O2`, a
record is **not measurable**: three million two-field constructions and reads take 34 ms
against 34 ms for the same two integers through bindings, and a million record copies with
an owning field take 54 ms against 55 ms for copying that field directly. `performance.md`
carries the `-O0` figures, where the helpers are real calls, and the executable-size delta
(**+96 bytes** for a record that owns something, zero for one that does not).

**Measured, N8.** 5,000 / 20,000 / 80,000 temporary heap strings against a constant live set:
1.88 / 2.33 / 4.17 MiB → **1.73 / 1.77 / 1.77 MiB**. The accumulator shape from the
2026-09-21 incident, at 8,000 iterations: 70.66 MiB → **2.23 MiB**. 2,000 short-lived
channels: 2.38 MiB → **1.80 MiB**. Sequences, a genuinely live 2,000,000-element sequence
and a sieve are unchanged, which is the control. `performance.md` has the runtime cost:
about 13–16 ns per string operation, and nothing measurable anywhere else.

**Self-hosted parity — VERIFIED**, records included. `compiler/emit.nz` emits the same model. Its runtime
text is **derived** from `crates/nazm-lir/src/emit.rs` rather than transcribed, and
`cargo xtask check`'s `runtime parity` gate re-derives it and refuses a difference; its
*decisions* are held by `the_nazm_written_compiler_emits_the_same_memory_semantics`, which
builds eight programs with both compilers and requires the same reclamation report from
each. Between N7 and N8 the two disagreed for a whole milestone and every behavioural test
passed throughout, which is what the gate exists to prevent.

**Completion is part of it since N10.1.** A `break`, `continue` or `return` inside a value
position — an argument, an operand, an initialiser, a condition, a scrutinee, an arm, the
operand of `return` — produces no value on that path in either compiler, and
`a_transfer_in_a_value_position_is_never_a_value` holds it: 32 programs in
`crates/nazm-cli/tests/transfers/`, each run by the interpreter, the reference backend, and
the Nazm-written compiler compiled **and** interpreted, whose IR must be byte-identical,
against a value written by hand and a reclamation report that must match and be zero live.
Before it the compiler written in Nazm passed `void` operands for the shape
`pick(int_to_str(i), if i == 1 { continue; } else { i })` and failed inside itself for a
construct whose branches all left. `architecture.md` §7.11 has the mechanism.

**And one leak the reference shared.** `==` on two strings never gave its operands back, in
both native backends, with no transfer involved — `int_to_str(i) == "5"` allocated a string
per evaluation and reclaimed none. The interpreter was right. Fixed in both, held by
`a_string_comparison_gives_back_its_operands`.

**And for refused programs, since N10.2.** The Nazm checker's completion is the
reference's three states — a value, no value, never finishing — so a value position given
nothing is `N0300`, an `if` whose branches disagree is `N0302`, and a body or a `return` is
held to its signature. `the_nazm_checker_agrees_with_the_reference_on_what_produces_a_value`
compares **code and span** with the reference over 40 programs in
`crates/nazm-cli/tests/refusals/`, with the Nazm checker both compiled and interpreted, and
requires the Nazm compiler to refuse each one before emission; one more pair crosses a
module boundary with two records laid out alike. What is claimed is diagnostic *codes and
primary spans*, not message text, and not the diagnostics that follow an earlier error in
the same expression, where the two checkers recover differently (`bootstrap.md`).

**Not claimed.** Whole-language leak freedom as a permanent property; anything about a
kind of value the language does not have — a weak reference, a closure, a trait object;
deadlock freedom; user-visible destruction, RAII or any ordering a program can
observe; a clean sanitizer run. `exit_with` reclaims nothing and reports nothing, which is
correct and is tested so nobody later treats it as a bug.

**Limitation, in the words this row carried before N7** · *"Compiled programs allocate and
never release: `@free` appears zero times in `crates/nazm-lir/src/emit.rs` and zero times
in `compiler/emit.nz`, against 11 `@malloc` and 2 `@realloc` sites in each."* Both halves
are false now, and the second was still true between N7 and N8. **Whole-language leak
freedom is still NOT VERIFIED and is still not claimed** — the sentence above about types
that do not exist is the reason, and it is not a formality.

**Next dependency** · None for the current type universe. *Until N11 this named a generic
container, which would make a cycle expressible again; N11 added one and refused the cycles
(`N0359`).* The next value kind that could form a cycle — a closure capturing its own
handle, a weak or shared reference — has to satisfy `docs/spec.md`'s *Cycles* a third time.

**Accepted when** · A compiled program's peak RSS is bounded by live data rather than by
total allocation, demonstrated on the self-hosting compiler compiling itself. **Met since
N8**, and unmoved by N9: contained on Linux aarch64, the compiler written in Nazm compiling
`compiler/emit.nz` peaks at 32.75 MiB against a 3,756-line input, and 80,000 short-lived
records with heap fields peak at the empty-program floor.

### 5. Effects — MISSING

*N12 did not change this row.* A function's error type is visible because it is inside its
return type, `Result[T, E]`; a caller may store, pass or ignore that value, and nothing about
it is tracked as an effect.

**Evidence of absence** · No effect row, annotation, inference or purity analysis. The
only occurrences of the word in the language crates are the English phrase "evaluated for
its effects" and `crates/nazm-lir/src/lib.rs`'s statement that there are *"no effects to
track"* — given as a reason there is one IR level rather than eight.

**Limitation** · The blocking question is open and named: **is divergence an effect?**
Without an answer, "drop an unused pure call" is unsound, so the optimiser cannot be given
that transformation at all. `docs/spec.md` *Open* carries it, along with whether allocation
failure and panic are effects.

**Next dependency** · A typed core IR to carry a row on a function type.

**Accepted when** · A function's row is inferred, checked, and a compile-fail test shows an
effectful call refused where a pure one is accepted.

### 6. Capabilities, as a language feature — MISSING

**Evidence of absence** · Authority is **ambient and labelled as such** in the source:
`crates/nazm-sema/src/intrinsic.rs` — *"These are callable from anywhere, and nothing in
the language restricts which functions may reach the file system or the process arguments.
That is a real limitation, not a design."*

**Do not confuse this with `nazm capabilities`.** `schema/nazm.capabilities-1.json` is a
*toolchain descriptor* — what one build of the compiler supports. It is unrelated to
object capabilities or capability-safety, and it is VERIFIED under area 22.

**Limitation** · `fn main(io: IO)` was written down as the plan of record and then **not**
adopted: I/O shipped as ambient authority, labelled as a limitation in `docs/spec.md`. It
remains the most likely growth path, and it remains unchosen.

**Next dependency** · Effects, or an independent capability-parameter design — the two are
separable and `architecture.md` §5 insists they stay separate concerns.

**Accepted when** · A function without the authority cannot reach the filesystem, and the
refusal is a compile error with a span.

### 7. Provenance and information flow, as a language feature — MISSING

**Evidence of absence** · No labels, lattice, sanitizers or declassification anywhere in
the language crates. `architecture.md` §5 designs a lattice-valued qualifier
(`Untrusted`, `Secret`, `PII`) and records that an earlier draft calling provenance
*"nearly free once effects exist"* was overclaiming.

**Limitation** · Every question that makes information flow usable is open in
`docs/spec.md`: implicit flows, sanitizer declaration, label propagation through a
projection, declassification authority, and what label a value from C carries.

**Next dependency** · Effects and a MIR that tracks places.

**Accepted when** · A `Secret` reaching an output is refused, with a stated and documented
approximation of what the analysis misses.

---

## IR and backend

### 8. Core IR — MISSING · ### 9. MIR — MISSING

**Evidence of absence** · Neither exists; no HIR either. `crates/nazm-lir/src/lib.rs`
states the reason directly: the levels in between *"exist to carry information M1a has
none of"* — no generics to monomorphise, no traits to lower, no regions to infer, no
effects to track. Monomorphisation is absent and cannot be otherwise, since nothing is
polymorphic.

**Limitation** · Not the absence of the levels — that is correct restraint.

**Still MISSING after N1 and N2, and deliberately.** N1 produced a `Resolution`, and a
resolution is **not an IR**: it holds no expressions and no control flow, only the answers
to "what does this name refer to", keyed by where the name was written. N2 added a
`ModuleGraph` and a `UnitInterface`, and neither holds a body — an interface is a module's
signatures, which is less than an AST rather than more. Lowering still lowers from the AST.
`nazm-sema` is not `nazm-hir`: the crate on §7's table is a graph of definitions *with*
bodies. Reading either new type as an HIR would be exactly the progress-bar mistake
`master-architecture.md` §3 warns about.

**Next dependency** · A responsibility no existing level owns. The duplicated one is gone;
the next candidate has to be argued for on its own evidence.

**Accepted when** · Introducing the level **removes** code rather than adding a
pass-through.

### 10. LIR and the backend contract — PARTIAL

**Evidence** · `crates/nazm-lir/src/ir.rs` and `crates/nazm-lir/src/lower.rs`. The
distinguishing information it carries over the AST is which operations can fail and where
the source said so; every fallible node carries a span. Signatures are lowered before
bodies, so forward references resolve without a second walk, and all refusals are
collected rather than the first. Layering is gated: `xtask/src/rules.rs` forbids
`nazm-lir` depending on `nazm-core`.

**Limitation** · It is not level 7 as `architecture.md` §2 defines it — a structured typed
tree over numbered slots, not SSA over a CFG of places. No optimisation, inlining or
constant folding happens here.

*The duplicate built-in table and the second resolver this row used to name were removed by
N1; the forbidden dependency on `nazm-core` still holds, and is satisfied by both crates
depending on `nazm-sema`, which owns the shared vocabulary since N2.*

*N12 added no node.* `?` is lowered to the LIR `match` it means, with a `Stmt::Return` in its
error arm (`lower_propagate`), so its cleanup is `return`'s and the emitter did not change.

*N12.1 added two, and removed a refusal.* A block used as a value compiles natively: every
value block goes through one `lower_block`, a `match` arm written as a block is its bindings
followed by that block, `Expr::Block` carries a block in any other value position, and
`Stmt::Match` is the statement `match` whose arms may produce nothing — the pairing
`Stmt::If` already had. Until then *a block used as an expression* was refused as `N0101`;
`nazm capabilities` now reports it compiled. Evidence: `crates/nazm-cli/tests/value_blocks.rs`,
19 cases each interpreted and native and required to agree on value and on every memory
counter. `architecture.md` §7.14.

*The paragraph that stood here until N5 said the backend was whole-program: one LLVM
module, functions numbered `@nz.fN`, modules existing in the front end and nowhere below
it. That was accurate, and it is no longer true — see area 10a.*

**Next dependency** · Only whatever a second backend or a real optimisation pass demands.

**Accepted when** · A second consumer exists that the contract serves unchanged.

### 10a. Per-module code generation and linking — VERIFIED

**Evidence** · As of N5 (2026-09-22) each Nazm module is lowered, emitted and assembled on
its own, and the objects are linked. `crates/nazm-lir/src/lower.rs` splits into a global
`plan` — signature admissibility and the recursion refusal, both of which span modules —
and `lower_module`, which reads one module's bodies and every function's *signature*;
`crates/nazm-lir/src/emit.rs` has `emit_unit`, `emit_runtime` and `emit_entry`;
`crates/nazm-cli/src/build.rs` runs `clang -c` per artefact and links.

A symbol is a spelling of the definition's `DefKey` (`crates/nazm-lir/src/symbol.rs`), not
a position: `@nz.m.lib_2Futil_2Enz.parse`. The encoding escapes every byte outside
`[A-Za-z0-9]` including `_`, so it is injective and collisions are impossible **by
construction** rather than improbable.

Sixteen cases in `crates/nazm-cli/tests/native_units.rs`, each running the compiler and the
program: a cross-module call resolved by the linker, three modules with the same private
`helper`, a diamond generated once, a legal import cycle, a cross-module call cycle still
refused, an imported `main` that is not the process entry, per-unit constants that do not
collide, a runtime defined exactly once, identical symbols across two checkouts and from
two entry points, and `pub` changing linkage without changing spelling. A `native identity`
gate covers what a test cannot: that no session id, no `pub` and no positional number
reaches a symbol, and no LLVM spelling reaches `nazm-sema`.

**What an object depends on, measured** · A's artefact names B in one `declare` and one
`call`. Changing B's body, adding a private definition to B, or adding an export A does not
call each leave A's artefact **byte-identical** — so an object depends on its dependencies'
symbol identity and ABI signature, and not on their implementations.

*Since N11* a generic function instance is a unit and an object of its own — artefacts
`g0`, `g1`, … — named from its definition's `DefKey` and its type arguments' canonical
keys, never folded into a caller's unit: `one_instance_requested_by_two_modules_is_one_unit_and_one_object`,
`a_callers_unit_never_carries_a_generic_body`, and a caller that declares an instance
carries every type the declaration names (`a_call_carries_the_types_its_callee_is_declared_with`,
found by the conformance corpus).

**Limitation** · The clean build is roughly **twice as slow** for a small program, almost
entirely because there are now several `clang` processes where there was one — measured in
`performance.md` together with a 10% smaller optimised executable, which is cross-module
inlining no longer happening. No LTO. `nazm build` still compiles a narrower subset than
`nazm run`, and the recursion refusal is unchanged and still whole-program.

*Until N6 this row also read "nothing is reused, and no native cache exists". That was
accurate and is no longer true — see area 10b.*

**Next dependency** · None for this row.

**Accepted when** · Met for separate generation and linking. Native reuse is area 10b.

### 10b. Native object reuse — VERIFIED

**Evidence** · As of N6 (2026-09-22) an emitted LLVM unit whose object was compiled before
is not compiled again. `crates/nazm-cache/src/object.rs` defines `ObjectKey` over three
things and nothing else — a digest of the exact bytes handed to the object compiler, the
identity of that compiler, and the configuration it resolves for the run — and
`crates/nazm-cache/src/objects.rs` is the store. `crates/nazm-cli/src/backend.rs` is the
single place the object compiler is invoked: one `clang -###` per build reads the resolved
command line, so the triple, the CPU features, the relocation model and the ABI are in the
key without anyone having enumerated them, and the invocation runs with an environment
built from nothing plus ten named variables, because `CCC_OVERRIDE_OPTIONS`, `SDKROOT` and
`MACOSX_DEPLOYMENT_TARGET` were each measured to change the object.

Thirty-three cases in `crates/nazm-cli/tests/native_cache.rs` and eighteen in
`crates/nazm-cache/tests/objects.rs`. The precision ones: a dependency's body change and an
export the caller never calls each recompile one unit of seven; a comment that moves no
emitted position recompiles nothing, and one above a fallible operation recompiles the unit
it moved. The safety ones: two optimisation levels share nothing, a backend that reports a
different version shares nothing, a truncated, edited, mis-filed or wrongly-headed entry is
a miss rather than a link failure, eight concurrent writers of one key leave one valid
entry, deleting the whole store changes only the time, and cold, warm and `--no-cache`
builds produce the same program. An `object cache` gate covers what a test cannot: that the
key still has all three fields, that a lookup still recomputes the key and the object's
digest, that nothing semantic is imported into the key, that an entry carries no field
nothing validates, and that the arguments hashed and the arguments executed come from one
function.

*Since N11, for generic instances* (`crates/nazm-cli/tests/native_cache.rs`, seven cases):
a new instantiation compiles the caller, the new instance and any runtime it newly reaches;
a generic **body** edit recompiles exactly its instances and no caller — not even its own
module's unit, which never carries the body; renaming a type parameter recompiles nothing;
a warm build compiles nothing. The key needed no change: an instance's key is its bytes.

**Measured** · On the five-module compiler, `-O0`: 665.4 ms with `--no-cache`, 730.7 ms
cold, **157.6 ms** warm; `-O2`: 1693.0 ms, 1755.4 ms, **157.6 ms**. A one-module edit costs
198–218 ms whether the edit is a private body, a called export's body, or an export nobody
calls — the three are indistinguishable, which is the point of the key. A warm build runs
**two** external processes where a cache-disabled one runs eight.

**Limitation** · This is object reuse, not incremental native compilation. Reading,
parsing, checking, backend admissibility, lowering, emission and fingerprinting run on every
build, and so does the final link. No executable cache, no `LinkKey`, no incremental linker,
no persistent LIR or LLVM, no remote cache, no LTO. Cold builds pay about 65 ms more than a
cache-disabled build for hashing, `fsync` and publication. Nothing is evicted: the store
grows until it is deleted, 852 KiB for this workload's seven units at `-O0`. A backend that
reports one version and behaves differently is indistinguishable, which is a stated limit
rather than a missing check.

**Next dependency** · A `LinkKey` if executable reuse is ever justified, and a decision
about whether the per-build `clang -###` probe — 60 ms of a 158 ms warm build — is worth
memoising, which is a cache of a toolchain fact with an invalidation question of its own.

**Accepted when** · Met for per-unit object reuse. Executable and link-result reuse are not
claimed anywhere.

### 11. LLVM backend — VERIFIED

**Evidence** · `crates/nazm-lir/src/emit.rs` emits LLVM IR as text; `crates/nazm-cli/src/build.rs`
hands it to `clang`. No LLVM is linked, so there is no build-time LLVM dependency. 127
end-to-end tests in `crates/nazm-cli/tests/build.rs`, each at `-O0` **and** `-O2`, against
a written expectation and against the interpreter.

**Limitation** · The arithmetic contract is the load-bearing part and is deliberately
conservative: `nsw` is not used, because it would promise overflow cannot happen and
license an optimiser to delete the trap. No debug info is emitted and `clang` is invoked
without `-g`, so a compiled program cannot be debugged symbolically.

**Next dependency** · None for correctness.

**Accepted when** · Already met for the current subset.

### 12. Cranelift backend — DESIGNED

**Evidence** · `architecture.md` §1 and `roadmap.md`, *Backends* — one component doing four jobs
(JIT, comptime evaluator, REPL, hot reload), and an explicit ordering decision that it
comes after, not instead. `xtask/src/main.rs` gates against a codegen dependency appearing
unannounced.

**Limitation** · None of the four jobs exists. A second backend now would be a second set
of semantics bugs to keep in agreement for no present benefit.

**Next dependency** · One of the four jobs becoming real.

**Accepted when** · `nazm test --backend=all` means something — which first requires
`docs/spec.md`'s backend-semantics questions to be settled.

---

## Runtime and concurrency

### 13. Runtime — PARTIAL

**Evidence** · There is no runtime crate; the runtime is LLVM IR generated into each
program, emitted lazily only when reached — `crates/nazm-lir/src/emit.rs` carries the
join, I/O, sequence, string, task and channel runtimes. `compiler/emit.nz` does not
*mirror* them since N8: its copy is **derived** from the Rust one line by line, and
`cargo xtask check`'s `runtime parity` gate re-derives it and refuses a difference. The
external surface is 26 libc/pthread symbols plus five overflow intrinsics and `memcpy`.

**Limitation** · No `nazm-rt` crate, so no scheduler, no allocator, no reactor. *This
read "No deallocation (area 4)" until N8; the emitted runtime reclaims every heap-backed
value now, and the remaining gap is the crate rather than the model.*

**Next dependency** · A reason to have a runtime crate — stack switching or a custom
allocator — since the memory model no longer waits on one.

**Accepted when** · `nazm-rt` exists with a narrow `unsafe` allowance that `xtask check`
enforces, per `architecture.md` §7.

### 14. Structured concurrency — VERIFIED

**Evidence** · Specified in `docs/spec.md` before implementation, then interpreted
(`crates/nazm-core/src/eval.rs`, 18 tests in `crates/nazm-core/tests/concurrency.rs`), then
compiled (17 tests in `crates/nazm-cli/tests/concurrent.rs` at `-O0` and `-O2`, each
against a written expectation *and* the interpreter), **then compiled by the compiler
written in Nazm** — `compiler/emit.nz` carries the task and channel runtime. Every test
carries a deadline, so a hang fails by name. Static rules are real: `N0320` for `spawn`
outside a `scope`, `N0321` for a sequence crossing into a task.

*Narrowed in N10.1:* the compiler written in Nazm joined a scope when control fell off its
end or failed, and not when a `break`, `continue` or `return` left it — so a task that failed
inside a scope a `break` left was never waited for and its failure never taken on. The
reference and the interpreter were right. Held by the `scope_*_joins` cases in
`crates/nazm-cli/tests/transfers/`.

**Limitation** · One OS thread per task and nothing more. No M:N, no work stealing, no
fairness, no task density, no timeouts, and **no cancellation beyond closing a channel**.
`Chan` carries `Int` only. Ownership prevents data races; **that is not deadlock freedom**
and is not claimed as such (`architecture.md` §5).

**Next dependency** · None for the current model; area 15 for anything beyond it.

**Accepted when** · Met for the model as specified.

### 15. Advanced scheduler — RESEARCH

**Evidence** · `architecture.md` §5 and `roadmap.md`'s C-4/C-5 rows: M:N tasks, work
stealing, growable stacks, an event reactor, blocking-FFI handoff.

**Limitation** · None of it exists and none of it is measured. `architecture.md` records
that the "1M live tasks under 2GB" target was **withdrawn** as arithmetically unsupported —
the right precedent for anything proposed here.

**Next dependency** · `nazm-rt`, which needs the `unsafe` allowance, which needs the memory
model.

**Accepted when** · Spawn-to-first-instruction latency, context-switch cost and task memory
as **RSS, not virtual reservation, with page size recorded**, measured against a named
implementation.

---

## Libraries and interop

### 16. Standard library — MISSING

*Since N12 there is one `.nz` library file, and it is not a standard library:*
`library/core/prelude.nz` declares `Result` and `Option`, is checked as ordinary source, and
is special only in its identity and in being imported everywhere (area 2). Nothing can add a
second declaration to it or a second such module.

**Evidence of absence** · No `.nz` library beyond that prelude. Thirty-one compiler built-ins in
one table, `crates/nazm-sema/src/intrinsic.rs`, mirrored by the backend in
`crates/nazm-lir/src/ir.rs`. Every built-in the front end has, the backend lowers — there
is no coverage gap today, and a test enforces that.

**Limitation** · The table is the library. Names live in the ordinary function namespace
and cannot be redefined. There are no string escapes, so `str_from_byte` is the only way to
write a newline.

**Next dependency** · User-defined types, or the library stays a table.

**Accepted when** · A `.nz` library ships, is versioned, and is not special-cased by the
compiler.

### 17. FFI and ABI — MISSING

**Evidence of absence** · No `extern`, no ABI annotation, no such token in
`crates/nazm-syntax/src/lexer.rs`. The 18 libc symbols are emitted *by* the compiler to
implement built-ins; a user program cannot declare or call anything else. `docs/spec.md`
lists the FFI boundary under *Open*, so it is not designed either.

**Limitation** · Total. This is the single largest gap between the language and any
systems, embedded or cybersecurity use.

**Next dependency** · A settled ABI and layout story, which `docs/bootstrap.md`
deliberately left unfrozen.

**Accepted when** · A C function is declared and called, with the safety boundary stated
and the provenance question (what label a returned value carries) answered.

---

## Domain profiles

`NAZM_LANGUAGE_GOALS.md` §10 owns what each profile is *for*. `master-architecture.md`
§4 owns the invariant that makes them profiles rather than dialects, and the ladder a
profile has to climb to become real. **This section owns what exists**, which is: one
profile, unnamed, and five ambitions.

**General — PARTIAL.** What `nazm run` and `nazm build` accept today: ambient authority,
checked arithmetic that traps, structured concurrency over OS threads, and automatic
reclamation of every heap-backed value with no annotation to write. *That last clause read
"no reclamation" until N8.* It
is the widest profile and every other is a restriction of it. PARTIAL rather than VERIFIED
for one reason: **the interpreter's subset and the native subset are not the same** —
recursion runs under `nazm run` and is refused by both compilers. A profile whose two
implementations accept different programs is not yet a profile. *Accepted when* the two
subsets coincide, or the difference is itself declared as a profile boundary rather than
left as a gap.

**Systems — MISSING.** Would need explicit control over allocation and layout, no hidden
allocation where none is declared, and a stated ABI; would grant raw memory access and
foreign calls inside a named, narrow boundary. Nothing profile-specific exists. Blocked on
area 17 (FFI and ABI, MISSING) and on area 4, which is now VERIFIED for the current type
universe but says nothing about *explicit* control over allocation and layout — a profile
that must declare where memory comes from needs more than automatic reclamation. The architectural
commitment that keeps it reachable — field access stays symbolic until the lowest level, so
layout selection remains possible — is already honoured and must be preserved rather than
undone when this profile arrives.

### 18. Embedded support — MISSING

**What it would restrict** · No allocation after startup, or none at all; a stack bound
that is computed; no I/O except through declared capabilities; a freestanding target with
no libc.

**Evidence of absence** · No cross-compilation, no target triple handling, no freestanding
target, no `no_std` equivalent, no control over allocation. `crates/nazm-cli/src/build.rs`
emits a module with no triple and lets `clang` supply the host's. The emitted runtime
declares 18 libc and pthread symbols and cannot currently be built without them.

**Next dependency** · Cross-compilation (area 33), then a freestanding runtime — which
means area 13. *This clause added "and therefore area 4" with the sentence "A language that
never frees cannot run on a device with kilobytes." The first half of that is closed since
N8; the second is not, because the profile's real requirement is "no allocation after
startup, or none at all", and reclaiming what you allocate is not the same as not
allocating.*

**Accepted when** · A program runs on a target with no libc, within a stack bound that was
computed rather than hoped for. Native frame counts do not bound stack bytes, and that
obligation is inherited here.

### 19. Critical profile — DESIGNED

**Read this before the rest.** This profile is an architectural intention. It is **not** a
claim that Nazm is suitable for any safety-related, airborne, medical, automotive or
otherwise regulated use; it is not a certification path; and no artefact in this repository
has been produced under a safety standard or assessed against one. **The architecture
anticipating a Critical profile is not evidence for one.**

**What it would restrict** · No dynamic allocation, no recursion, bounded loops with an
established bound, total functions, and every failure mode enumerated rather than trapped;
contracts and invariants checked rather than asserted.

**What exists that is genuinely relevant** · Three things, and they are real. Arithmetic is
**specified rather than inherited from the hardware** — overflow traps, and
`crates/nazm-lir/src/emit.rs` refuses `nsw` so no optimiser may delete the check. Recursion
is already **refused by both compilers**, which is the restriction this profile would want,
arrived at for an unrelated reason. Everything outside a backend's coverage is **refused by
name with a span**, never silently approximated or partially compiled.

**Missing prerequisites** · A memory model; effects; contracts; a bounded-stack story; and
a qualified toolchain, which the bootstrap explicitly does **not** provide — `docs/bootstrap.md`
§4 puts compiler trustworthiness on the list of things a fixpoint does not establish.

**Accepted when** · A proof obligation is discharged on a real Nazm program and assessed
against a named standard by someone qualified to make that assessment. Not a test count.

### 20. Cybersecurity profile — DESIGNED

**What it would restrict** · Untrusted input tracked to its uses; declassification
explicit and audited; no ambient authority; constant-time obligations honoured where they
are declared.

**Evidence** · The one real ingredient present is refuse-by-name-with-a-span rather than
silent degradation, and failures that are diagnostics rather than undefined behaviour.

**Limitation** · Against that: authority is **ambient and labelled as such in the source**
— `crates/nazm-sema/src/intrinsic.rs` says nothing in the language restricts which
functions may reach the filesystem.

**Next dependency** · Areas 6 and 7 — capabilities and information flow — both MISSING,
with every design question still open in `docs/spec.md`.

**Accepted when** · Untrusted input is tracked to its uses and declassification is explicit
and audited, with the approximation's blind spots stated.

### 21. AI/HPC — RESEARCH

**Evidence of absence** · No tensors, no GPU path, no vectorisation work, no measurement.

**Next dependency** · A reason. `architecture.md` §1 says MLIR should be revisited *only
if* Nazm pivots toward tensor/accelerator codegen, and nothing proposes that pivot.
Treating this as a profile rather than a declined direction is itself the open question,
tracked as R7 in `research-register.md`.

**Accepted when** · A workload the project actually has, that a CPU backend cannot serve.

---

## Tooling

### 22. Diagnostics — VERIFIED

**Evidence** · `crates/nazm-diag/src/lib.rs` — 33 stable codes, byte-offset spans, a
versioned JSON schema `schema/nazm.diagnostic-1.json` emitted per object. Conformance is
tested against real binary output in `crates/nazm-cli/tests/schemas.rs`, and
`crates/nazm-cli/tests/codes.rs` requires every declared code to be provoked by a test or
listed as unprovokable with a written reason. The capability inventory
(`crates/nazm-cli/src/capabilities.rs`) is read from the compiler's own tables, and
`docs/diagnostics.md` states the compatibility contract.

**Limitation** · Two codes are unprovokable by construction. Spans are offsets into a
merged buffer, so file identity is recovered rather than carried (area 2).

**Next dependency** · None.

**Accepted when** · Met.

### 23. Formatter — VERIFIED

**Evidence** · `crates/nazm-syntax/src/format.rs`, token-driven so comments survive, with
no options by design. `crates/nazm-syntax/tests/format.rs` asserts on every input that
tokens are unchanged, that `format(format(x)) == format(x)`, that comments are identical,
and that output ends in exactly one newline — applied to every `.nz` in the tree and to 15
hand-written ugly inputs.

**Limitation** · No reflow, no reordering, no insertion. Per file, not per merged program.
Canonical in the sense of "one output", not yet in the sense of a lossless tree round trip.

**Next dependency** · A CST, for edits rather than formatting.

**Accepted when** · Met for formatting.

### 24. Machine-applicable fixes — PARTIAL

**Evidence** · `crates/nazm-cli/src/fix.rs`, schema `schema/nazm.fix-1.json`. Only
`Automatic` fixes apply; nothing is written without `--apply`; edits apply right-to-left;
overlaps are skipped; and a result that no longer parses is rolled back wholesale. Ten
workflow tests in `crates/nazm-cli/tests/workflow.rs`.

**Limitation** · Stale-edit protection exists but is **by expected-bytes comparison, not a
file hash or document version**, and it is **untested** — none of its four skip paths is
asserted anywhere. The named test for it exercises a different path.

**Next dependency** · None; this is a test to write.

**Accepted when** · Each of the four skip reasons has a test that reaches it.

### 25. LSP and MCP — MISSING

**Evidence of absence** · No crate, no dependency, no protocol code. `architecture.md`
lists `nazm-lsp` and `nazm-mcp` as planned and records the intended libraries.

**Next dependency** · A CST and incremental resolution — both area 1 and area 2.

**Accepted when** · An adapter shares the compiler's semantic engine rather than
re-implementing it.

### 26. Package and build tooling — PARTIAL

**Evidence** · Multi-file builds work through path-relative `use`, two independent
implementations: `crates/nazm-cli/src/load.rs` and `compiler/module.nz`.

**Limitation** · There is no project manifest, no dependency resolution, no lockfile and no
registry. `nazm build` takes one root file, and since N3 the **source root is derived from
it** — so a module's durable identity is relative to whichever file was named. A declared
root is what would make a key independent of that, and it is a build-system feature this
project does not have.

The two resolvers still differ deliberately — the Nazm one normalises paths textually
because the emitted runtime has no `realpath`, so a symlinked file is read twice there and
once here. N3 settled what that means for identity rather than closing it: a module reached
under two names gets **no** durable key, in either implementation, so the two cannot
disagree about one (`spec.md`, *Durable identity*).

**Next dependency** · A declared source root, which is the same decision as a project
manifest.

**Accepted when** · A project builds from a manifest and the two resolvers agree on
identity.

### 27. Debugger and profiler — MISSING

**Evidence of absence** · No DWARF, no `!dbg` metadata, no `-g`; `--emit-ir` writing the
`.ll` beside the executable is the only debugging affordance
(`crates/nazm-cli/src/build.rs`). Source positions in compiled programs are baked in as
string constants at compile time. No profiler integration exists.

**Next dependency** · Line tables in the emitted module.

**Accepted when** · A native backtrace names a `.nz` line.

---

## Evidence and infrastructure

### 28. Performance measurement — PARTIAL

**Evidence** · `xtask/src/bench.rs` times five stages per program plus the compiler's own
source, with a correctness gate that refuses to time anything not producing its `.expected`
output first. One discarded warm-up, then N runs, median compared. Peak RSS captured where
readable. A regression must clear **both** 25% and 5 ms. External references are built and
timed from `bench/reference/sieve.c` and `bench/reference/sieve.py`, and a reference whose
answer differs from the Nazm program's is not timed.

**Limitation** · `bench/baseline.json` (Darwin) is **stale** and not a valid comparison;
re-recording it means running generated programs on the host, which the resource contract
forbids. `bench/baseline-linux-aarch64.json` is current. Cross-host comparison is refused
rather than attempted. Every measurement under ~15 ms is below the method's noise floor.

**Next dependency** · A Darwin machine on which running generated programs is contained.

**Accepted when** · Both baselines are current and recorded under containment.

### 29. Differential testing — VERIFIED

**Evidence** · Five independent mechanisms. `crates/nazm-cli/tests/build.rs` — two oracles
per case, both opt levels. `crates/nazm-cli/tests/concurrent.rs` — the same for
concurrency. `crates/nazm-cli/src/test.rs` — `nazm test` runs every case interpreted and
native and requires agreement. `crates/nazm-cli/tests/selfhost.rs` — the Nazm-written
lexer, parser and checker against the Rust ones, token-for-token and span-for-span, over
every `.nz` in the tree including their own sources, and eleven programs built by **both**
compilers and required to report the same reclamation — plus, since N10.1, the 32-case
transfer corpus run five ways. `compiler/bootstrap.sh` — thirteen conformance cases, six of
them the N10.1 transfer shapes and one the original reproducer byte for byte, and seven
multi-module ones, under C2, C3 and the interpreter. *Since N12, propagation: 22 programs
through both checkers by code and span, four `?` programs built by both compilers with equal
reclamation reports, and a `results.nz` and a `Result`-library module case in the corpus.*
*Since N12.1, value blocks: 26 programs through both checkers by code and span, 10 parser
trees, six block programs built by both compilers with equal reclamation reports, the core
prelude found by key with the prelude last, between two project modules and absent, and a
`blocks.nz`, a value-block module case and three refusals in the corpus.*

**Limitation** · The interpreter is the oracle, so a shared specification error is
invisible to all five. `docs/spec.md` names what must be settled before
`nazm test --backend=all` would mean anything.

**Next dependency** · None.

**Accepted when** · Met.

### 30. Mutation testing — VERIFIED · Fuzzing — MISSING

**Evidence** · `xtask/mutations/mutations.toml` holds **266** catalogued defects (158 when
this sentence was first written, 233 at N12.2), **77** of them with a verified killer and all
of them owned by one of nine focused profiles;
`xtask/src/mutate.rs` injects each under a kernel advisory lock with a write-ahead backup,
requires the pattern to match exactly once, rebuilds, runs the suite and restores. It
distinguishes seven verdicts and refuses to fold a non-result into "survived" — a
distinction added after an incident that produced 19 false survivors. Lifecycle behaviour
is tested in `xtask/tests/lifecycle.rs`.

*N4 added six on 2026-09-22 and ran all six contained: four remove one input each from a
check key — the module's own source, its dependencies' interfaces, the checker's identity,
and which module it is — one stops an entry's recorded inputs being checked against the key
it is filed under, and one publishes an entry for a module that did not check cleanly. Each
is a wrong answer rather than a slow build, which is why they were worth the run.*

*N2 added two and ran four contained on 2026-09-22 — `every-definition-is-exported` and
`imports-become-transitive`, plus the two existing entries in the files N2 rewrote most, to
confirm their patches still apply. All four were caught, by 8 to 18 tests each. N2.1 added
four more against the **self-hosted** path — the `pub` bit ignored, resolution across every
module, the first ambiguous import winning, and any `main` accepted as the entry point —
and all four were caught by the parity suite. Both are targeted runs, not catalogue runs;
the row below still stands.*

*N9 added fifteen on 2026-09-23 and ran all fifteen contained and serial, in four
batches.* Each attacks the composition rather than the syntax: a copy that forgets a field,
a drop that omits one, a drop that does not recurse, a replacement that releases before it
takes, a projection out of a temporary that does not preserve ownership, a construction
that fails and leaks what it built, a task-safety derivation that stops at the first level,
an interface that omits its records, a published field that loses its type, a published
record named by its spelling, a layout that follows the declaration, a field read from the
wrong position, a recursive record accepted, a private type leaked through a public
signature, and the **self-hosted** emitter forgetting a record field. All fifteen are
caught; two runs were needed, because one survived and one did not compile.

The survivor is the interesting one and it is written up in `architecture.md` §7.10: a
replacement that releases before it takes is invisible to the reclamation counters, because
both orders end with the same two numbers. The one that did not compile is a different
verdict and was reported as one rather than folded into a pass — which is what the
seven-verdict distinction exists for.

*N10 added twenty-two on 2026-09-23 and ran all twenty-two contained and serial, in five
batches, with nothing else running.* Eighteen attack the composition becoming dynamic: an
enum's properties taken from one variant, a discriminant from the parser rather than from
the names, a payload laid out as written, a published enum in source order, an exported
enum missing from the interface, a helper branching on the wrong tag, a copy that takes no
reference, a `match` that does not take over what it matches and one that never lets go, a
partly built variant left uncleaned, a task that does not give back what it borrowed, the
five checker rules — coverage, a repeated arm, another enum's variant, a missing payload
field, a private payload type — and the **self-hosted** checker skipping exhaustiveness and
the self-hosted emitter not taking over a payload.

Four more came out of the one that survived, and they are the interesting ones.
`a-variant-slot-keeps-what-the-last-round-left-in-it` was injected, compiled, and the suite
still passed. Asking why found that a `break`, a `continue` or a `return` out of a
half-evaluated expression gave back nothing it was holding — a defect of **N8's**, in both
backends and in the compiler written in Nazm — and then that the mutation itself no longer
described a defect, because the slot it zeroes is zero on every path that reaches it.
`architecture.md` §7.11 writes it up. It was retired and replaced by four that do describe
one: a transfer that keeps the temporaries, one that keeps the values being built, one that
releases the temporaries of the loop *around* it — a premature free rather than a leak —
and the same omission in the self-hosted emitter.

*N10.1 added thirteen on 2026-09-23, repointed two whose text the correction moved, and
ran all fifteen contained and serial in five batches of three, with nothing else running:
**fifteen caught, none survived**, none unapplied, none that did not build, no timeout.* Each
puts back one half of the missing representation or one thing the audit found: an `if`
typed by the branch that left — the root cause — a join opened that no branch reached, for
an `if` and for a `match`; a node emitted though control never reaches it, and the same for
assignment alone; a `phi` read from a fixed two incomings; an unreachable node's operands
left on the stack; a `break` and a `return` that leave a `scope` without joining it; the
Nazm checker silent after a transfer; a value that never arrives given no type; and `==`
on strings keeping its operands, in each backend. The two repointed —
`the-self-hosted-emitter-skips-one-release-path` and
`the-self-hosted-compiler-keeps-what-a-continue-abandons` — were caught as before. Targeted
batches, not a catalogue run.

*N10.2 added fifteen on 2026-09-23, repointed one, and retired one it superseded, and ran
the sixteen contained and serial in six batches with nothing else running: **sixteen
caught, none survived**, none unapplied, none that did not build, no timeout.* They are the
ways the three completions can collapse into two — no value read as a value, a value that
never arrives refused as none, an `if` with no `else` taken to leave, a `continue` taken to
complete — each half of the `if` join taking the wrong branch's type, two types accepted, a
value with nothing accepted, a `Unit` arm leaving its `match` a value, a body and a
`return` not held to the signature, the arms of a refused `match` still checked, and the
emitter's discarded arm value kept, or discarded without being given back. The repointed one
is N10.1's `N0313` entry, whose line the block's rewrite moved;
`an-if-is-typed-by-the-branch-that-left` is retired because its rule is now one line of the
three-state join, which `a-then-branch-that-leaves-still-gives-the-type` mutates exactly.
Targeted batches, not a catalogue run.

*N11 added twenty-six on 2026-09-23 and repointed four the diagnostics migration moved, and
ran them contained and serial in five batches with nothing else running.* By category —
**type system** (six: parameter order forgotten, wrong arity accepted, inference keeping the
first conflicting type, `==` on a parameter, two definitions of one shape unified, parameter
names persisted in the interface), **native instances** (four: a symbol built from the first
argument alone, an instance planned twice, an instance's own calls never planned, polymorphic
recursion not counted as expanding), **`Vec` ownership** (five: push without a reference of
its own, get handing out the vector's, set releasing before taking, pop copying instead of
transferring, destruction without the elements), **cycles** (two: a `Vec` hiding what it
owns, every parameter counted as used) and **the compiler written in Nazm** (nine, across
all four). First run: **twenty caught, six survived**, none unapplied, none that did not
build, no timeout. The six were two holes and one equivalence:

* `two-generic-definitions-of-one-shape-unify` and `an-instance-symbol-keeps-only-its-first-argument`
  had no test that could see them — no call passed one generic record where another of the
  same arity was declared, and no two instances differed only after their first argument.
  Now `inference_does_not_unify_two_generic_definitions_of_one_shape` and
  `two_instances_that_differ_only_in_a_later_argument_are_two_functions`;
* `vec-get-returns-a-borrowed-element`, in both compilers, is the N8 lesson a third time:
  every shape bound what it read, and a binding takes a reference of its own. Now
  `an_unbound_vec_read_outlives_the_element_being_replaced`, in both, reads an element into
  an argument and replaces it in the next argument, and churns the allocator in the callee;
* `vec-set-releases-before-it-takes`, in both, survived a second run and is **equivalent**:
  the order is observable only when the new value's one reference is the slot it replaces,
  and `vec_get` always gives out a reference of its own, so no program can build that. Both
  are retired, with the reason in the catalogue.

Rerun: **all four remaining caught**. Final: **24 of 24 live mutations caught**, 2 retired as
equivalent. Targeted batches, not a catalogue run; the catalogue is 182 entries.

*N12 added twenty-six on 2026-09-24 and ran all twenty-six contained and serial, in six
batches, with nothing else running.* Result identity — `?` recognising `Result` by name, by
an `Ok`/`Err` shape, or as the first enum of that name; a project path taking the `@core`
key; the prelude left out of every check key; rehydration giving each interface its own
types. The rule — `?` outside a `Result` function, the error type ignored, the success type
demanded equal, a project type taking a prelude name. Control flow — the lowered `match`
testing the error tag, returning the operand's `Result`, falling through on error, and the
interpreter carrying on with the error. And in the compiler written in Nazm, the same checks
plus ownership and the scope join: the error or the success value not retained, the operand
leaked on success, the join skipped, earlier temporaries or the frame left unreleased, the
error path falling into the success path, the prelude never imported. **All twenty-six
caught on the first run** — none survived, none unusable, none unapplied, none without a
verdict, and none retired. Targeted batches, not a catalogue run; the catalogue is 208
entries.

*N12.1 added twenty-five on 2026-09-24 and re-pointed two of N12's,* whose patterns the
`Option` dogfood had moved; their defects were unchanged, so they were rerun rather than
retired. Run contained and serial, with nothing else running. Prelude identity in the
compiler written in Nazm: the last module taken as the prelude, any module called `prelude`,
a project path keeping the `@core` key, the prelude losing its key, the root module's
`Result`, the reserved-name rule following position — and in the reference graph, the last
module named the prelude. The reference backend: an arm block dropping its statements, a bare
block emitted as its tail alone, a block's value released as it leaves, a statement block
skipped, a statement `match` claiming every arm leaves, an arm that never rejoins, a
scrutinee never let go, a tail emitted after a transfer. The compiler written in Nazm: a block
operand refused or given an empty span, a statement block needing a `;`, every block
expression a value, a diverging one `Unit`, a block expression emitted as something. The
dogfood: `None` confused with `Some`, the `-1` brought back for a compilation with no
prelude, a check's arms reversed, a found declaration discarded. **All twenty-seven caught.**
None survived, none unusable, none unapplied, none retired.

One needed three runs, and the reason is the suite's rather than the compiler's. *A
statement `match` claiming every arm leaves* drops a loop's increment, so the compiled
programs it produces never finish — and three places ran compiled programs with no deadline
(`value_blocks.rs`, the corpus run in `schemas.rs` and `workflow.rs`), while `run_bounded`
killed a stage but not what the stage had started. The first run ended at the batch's 3000 s
deadline with no verdict, and the second was stopped when it hung in the next such place; a
timeout was not counted as a catch. Each now fails its case under a deadline and kills its
process group, and the third run caught the mutant with 25 failing tests. The catalogue is
233 entries.

*N12.2 (2026-09-24/25) replaced the runner's evidence shape without changing what a verdict
means, and then ran the whole catalogue for the first time.* The harness is
`xtask/src/campaign.rs`, `xtask/src/plan.rs` and `xtask/src/procs.rs`, operated as
`docs/runbook.md` "Mutation campaigns" describes: per mutant, a verified **killer** (tier 1),
then a **focused profile** (tier 2), then the **workspace suite** (tier 3), stopping at the
first failure. A tier-1 or tier-2 catch is the same evidence as a tier-3 one — the test is a
member of the suite, passed in the baseline and failed under the mutant — and only tier 3 can
report SURVIVED. A timeout is its own no-verdict. `--strategy full` keeps the legacy shape
as the oracle. Lifecycle is in `xtask/tests/lifecycle.rs`: every tier transition, a deadline
at each tier, stale killer metadata, build reuse across four mutants, interruption and
resume, resume refused for another tree or catalogue, and the watchdog under SIGKILL and
SIGINT.

Measured, contained, serial, nothing else running:

| run | mutants | wall | per mutant, median |
| --- | --- | --- | --- |
| N12.1's catalogue batches, legacy, productive time | 27 | 10,782 s (13,870 s with the no-verdict reruns) | ≈ 240 s |
| the same 27, v2, profiles only | 27 | 1,807 s | 52 s |
| the same 27, v2, verified killers | 27 | **523 s** | **1.4 s** |
| differential sample, `--strategy full` | 10 | 3,058 s | 246 s |
| the same 10, `--strategy targeted`, profiles only | 10 | 945 s | 23 s |
| **whole catalogue, targeted** | **233** | **10,686 s**, five resumable sessions | 22 s |

The differential sample spans the checker, native symbols, memory, the interface, the cache
key, the compiler written in Nazm, the agent harness and the parser: **identical verdicts**
(ten caught under both) and identical original, mutated and restored digests. Two mutants
that crash a binary gave MUTANT CRASH under both strategies too.

**The whole catalogue, on `f444d0d`:** 233 selected, 233 finalized. **228 caught** — 38 at
tier 1, 185 at tier 2, 5 at tier 3. Six workspace-suite runs in all; 227 avoided. And five
problems, each accounted for rather than folded into a pass:

* **2 UNUSABLE** — `a-sequence-does-not-retain-the-string-it-stores` and
  `a-published-field-loses-its-type`: their *replacements* had drifted from the code (a
  renamed field; a field type that is no longer a string), so the mutants did not compile.
  Repaired, rerun, **caught**.
* **1 SURVIVED** — `a-record-that-contains-itself-is-accepted` disabled
  `UserTypes::layout_cycle`, which has had no caller since N11 moved the checker to
  `ownership_cycles`. An equivalent mutation of dead code; **retired**, and
  `an-inline-containment-cycle-is-not-refused` attacks the rule where it lives: **caught**.
* **2 MUTANT CRASH** — `nesting-unbounded` and `stack-not-sized-from-the-budget` overflow the
  stack inside the `nazm-syntax` and `nazm-core` test binaries. The legacy strategy gives the
  same verdict. The effect is observed; no test *asserts* it. Open — the cases must move
  out of process, which is a change to those crates' tests and not N12.2's to make.

*N13 (2026-09-25) added thirty-three and repointed thirteen,* each new one written with the
semantic test that observes it and its killer confirmed by `--verify-killer` before it was
committed. They break the capability derivation (a `Vec`, a sequence, a `Chan` given
equality; only the first component, only the first variant, no nested type walked), the
checker (two types of one shape compared, a refusal that names nothing), the interpreter
(first field only, variants not compared, `!=` not the negation, `Str` by length), the
native helpers (first field only, tags not compared, a zero-payload variant unequal to
itself, arms in the wrong order, `!=` not negated, each temporary leaked, helper names
without their type arguments, operands released before they are read), the Nazm-written
checker and emitter (the same families), and the dogfooded completion fold. Nine of the
thirteen repointed entries compared a completion with `99`; the enum made that
inexpressible, so they became typed (`x != x`) — the invalid state they simulated no longer
exists. **Campaign `418a5e5921b866cc`, targeted, one session, 1,997 s:** the 33 new, the 13
repointed and 12 existing entries whose target code N13's diff touched or sits beside — 58
selected, 58 finalized. **57 caught**, 45 at tier 1 and 12 at tier 2, none at tier 3; no
timeout, no runner error, nothing unapplied or unusable, no crash; the workspace suite ran
once. **1 SURVIVED**: `n13-native-operands-are-released-before-they-are-compared`, a real
use-after-free — the helper reads operands whose storage was just released. It is **not
equivalent** and is **not counted as caught**. It survives because nothing allocates between
the release and the read, so the freed bytes are unchanged and every answer and counter is
right; glibc 2.41 in the image ignores `MALLOC_PERTURB_` and `glibc.malloc.perturb`
(measured), so **no deterministic direct killer exists under the supported allocator and
harness**. It stays **live** in the catalogue, with the `native` profile and a `no_killer`
reason, as a known surviving mutation: a mutation-sensitivity limitation of this suite, not
a verdict about the defect. Three
candidate killers were refuted by verification and not used. The whole catalogue was not
re-run: N13 did not change the harness, and the two open MUTANT CRASH entries were not
reached by it.

The run also found three harness defects, all repaired before the counted run:
`memory.rs` ran compiled programs with no deadline (the loop mutant waited 1500 s for a
verdict it could not get); N12.1's process-group kills used `kill -KILL -<pgid>`, which
procps rejects, so inside the container they signalled nothing; and a session killed at its
deadline was reported as a memory limit.

**Limitation** · Two catalogued defects are MUTANT CRASH, not caught (above), and one N13
defect is a known survivor (above): the suite cannot observe a read of freed storage nothing
has reused. **VERIFIED here means the harness runs every catalogued mutant and accounts for
every verdict — not that every meaningful mutation is killed.** A campaign is
five sessions, each paying a cold build and a baseline (about 355 s); journal lines reach the
host when a container ends, so a supervisor killed mid-session loses that session's verdicts,
which then simply run again. The 71 entries in `compiler/` cost 45 % of per-mutant time,
almost all at the selfhost profile, because 51 of them have no killer yet. **Fuzzing does not
exist** — no fuzz directory at the repository root, no targets, no corpus.

**Next dependency** · For mutation, killers for the older `compiler/` entries, and the two
crashing cases moved out of process. For fuzzing, a decision to start.

**Accepted when** · A full catalogue run completes with every verdict accounted for. **Met on
2026-09-25**: 233 of 233, as above. Fuzzing's status is separate and unchanged.

### 31. Bootstrap — VERIFIED

**Evidence** · `compiler/bootstrap.sh` runs C1 → C2 → C3 from one source and compares two
things: the emitted IR byte for byte, and the linker-normalised executables byte for byte.
Five conformance cases under `compiler/conformance/` must agree across C2, C3 and the
interpreter. The script refuses to run without an external memory ceiling. `docs/bootstrap.md`
states what this does and does not establish, and the answer is narrower than it sounds.

**Limitation** · A fixpoint establishes self-consistency, not correctness — a compiler with
a bug that reproduces itself reaches a fixpoint too.

**The reference compiler was stale until 2026-09-22.** `compiler/bootstrap.sh` requires
`$nazm` to exist and does not build it, and inside the contained runner `CARGO_TARGET_DIR`
is `/work/target` — which `docker/contained.Dockerfile` warms with a binary at *image build
time*. Every contained bootstrap between the image's build and this date therefore used
that binary for the reference check and for stage 1, whatever the tree said, and reported
nothing about the discrepancy. The workload now runs `cargo build -p nazm-cli` first. The
fixpoint claim itself is unaffected — C1 → C2 → C3 is about the compiler written in Nazm,
and the reference check is explicitly not part of the chain — but *which front end built
C1* was not what the record said it was.

**Since N2** the compiler source states its own module boundaries: 84 of its 224 functions
are `pub`, 140 are private, and `emit.nz`, `check.nz` and `parser.nz` gained the `use` lines
for modules they were reaching through someone else's import.

**Since N2.1 the two implementations accept the same language.** For one day they did not:
the Nazm-written compiler parsed `pub` and resolved every name across the whole program, so
it accepted eight kinds of program the reference refuses and emitted an invalid module for a
ninth. It now carries a module graph, a per-definition owner and visibility, per-module
interfaces collected before any body, and a recorded resolution that emission consumes.
Fifteen rules are compared through both compilers
(`the_two_compilers_agree_on_every_module_and_visibility_rule`), and five multi-module
conformance cases agree across the reference, C2 and C3 — which is a **different** claim
from C2 = C3 and is recorded separately in `bootstrap.md`.

**Since N11** the compiler written in Nazm implements generics and `Vec[T]` and uses one — its
diagnostics are a `Vec[Diag]` — so every stage compiles a `Vec` of records. The corpus has 24
cases, 9 of them multi-module, including a generic library instantiated only by its importer
and one instance requested from two modules, and a **refusal corpus** of 4 programs that C2,
C3 and the reference must each refuse with the named code. The fixpoint moved to
`c58b91dd…` at 11,877 source lines. `bootstrap.md` has the record.

**Since N12** every stage loads the same core prelude — named on its command line, its digest
in the record — and the compiler uses `?` itself: its front end returns
`Result[Checked, Vec[Diag]]`. The corpus has 26 cases, 10 of them multi-module, and 9
refusals. The fixpoint moved to `08895900…` at 12,046 source lines, with the runnable
executables byte-identical as well.

**Since N12.1** the compiler written in Nazm compiles a block used as a value and finds the
prelude by the key `@core/prelude` rather than as the last module, and its checker takes an
`Option[Int]` apart where it used to test for `-1`. The corpus has 28 cases, 11 of them
multi-module, and 12 refusals. The fixpoint moved to `a9ead258…` at 12,164 source lines,
runnable executables byte-identical too.

**Since N13** the compiler written in Nazm derives equality and emits it — a helper per
compared record and enum, a tag comparison and then the active slot alone — and uses it on
itself: a node's completion is an `enum Completion` in a `Vec[Completion]` rather than `0`,
`1` or `2` in an `Ints`, compared with `==`. The corpus has 30 cases, 12 of them
multi-module (one compares an imported record, enum and `Option[Point]`), and 19 refusals,
seven of them equality's. The fixpoint moved to `76598c43…` at 12,412 source lines,
runnable executables byte-identical too.

**Next dependency** · None.

**Accepted when** · Met.

### 32. Reproducibility and provenance — PARTIAL

**Evidence** · `scripts/release-candidate.sh` runs every gate, writes `PROVENANCE.txt` and
`MANIFEST.txt`, and validates the package against its own manifest with counts compared
both ways so a file copied in but not listed fails.

**Limitation** · Three claims, kept apart. Archive **entry metadata is normalised** — tar
sorted with zeroed owner and mtime, gzip with no timestamp. Whole-archive
**bit-reproducibility is not established and cannot be** while the provenance and bootstrap
records embed their own run date. **C2 and C3 are byte-identical within one run** after the
build-id is stripped. Separately, the source digest covers a **declared file set**, not the
tree — it omits every Cargo manifest, `Cargo.lock`, `rust-toolchain.toml`,
`docker/contained.Dockerfile`, the mutation catalogue and every `.expected` fixture, so it
cannot detect a changed dependency pin.

**Next dependency** · Moving the run date out of the digested set, if bit-reproducibility
is wanted.

**Accepted when** · Two runs at one commit produce identical archives, or the claim is
permanently retired.

### 33. Platform and target matrix — PARTIAL

**Evidence** · Two verified targets: `aarch64-apple-darwin` and `aarch64-unknown-linux-gnu`
(the latter inside `docker/contained.Dockerfile`). The emitter states its own scope, and
`crates/nazm-cli/tests/docs.rs` asserts the documented target and the emitter agree.

**Limitation** · **No cross-compilation and no target handling** — `crates/nazm-cli/src/build.rs`
has no `--target`, and the emitted module carries no triple, so `clang` supplies the host's.
Other targets are unverified rather than unsupported.

**BLOCKED, and it is a real finding** · `.github/workflows/ci.yml` runs
`cargo test --workspace` and sets neither `NAZM_CONTAINED` nor `NAZM_ALLOW_UNCONTAINED`,
while `crates/nazm-cli/tests/common/mod.rs` turns that into a panic for the four selfhost
stage builders. The workflow was last changed before the containment gate existed, so by
reading, those tests cannot pass in CI. CI also runs on `ubuntu-latest` x86_64, which is
**neither** verified target. Two candidate fixes — run the selfhost stages through
`cargo xtask contained`, or exclude them from the CI test step and say so — and choosing
between them is outside this task.

**Next dependency** · Deciding what CI is for: a gate on the verified targets, or a smoke
test on a third.

**Accepted when** · CI runs green on a stated target and the stated target is one the
project verifies.
