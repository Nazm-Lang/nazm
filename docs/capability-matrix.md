# Capability matrix

**Status: this file is the current status authority for the tree it is committed in.**

Two things are kept apart here. **`4fb15542dfbf9af0bf34c42fe52783637b3daed4` is a frozen
historical candidate**: the commit a retained release candidate was built and tested at.
Its evidence is [`releases/4fb1554.md`](releases/4fb1554.md), which is immutable and is not
updated by anything that happened since. **The claims below are current-tree claims**, and
they include behaviour added after that candidate — the milestones N7 through N15, each of
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

**Current suite: 1,718 passed, 0 failed, 18 ignored, across 95 suites**, run contained at
N32-H2 (2026-09-29), at P4T4; the eighteen ignored are the xtask lifecycle tests,
run on their own (18 of 18 passed, contained). N32 ended at 1,706 across 95; N31 ended at 1,669 across 91, 1,663 before its closure correction, N30 ended at 1,647 across 89, N29 ended at 1,629 across 88, N28 ended at 1,609 across 87, N27 ended at 1,582 across 86 (1,580 before its closure correction), N26 ended at 1,555 across 83, N25 ended at 1,532 across 81, N24 ended at 1,514 across 80, N23 ended at 1,491 across 79, N22 ended at 1,476 across 78, N21 ended at 1,461 across 77, N20 ended at 1,448 across 76, N19 at 1,435 across 75, N18 at 1,417 across 74, N17 at 1,395 across 73, N16 at 1,376 across 72, N15 at 1,348 across 68, N14 at 1,317 across 67, N13 at 1,291 across 65, N12.2 at 1,255 across 63, N12.1 at 1,211 across
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
| 24 | Machine-applicable fixes | VERIFIED |
| 25 | LSP and MCP | PARTIAL / PARTIAL |
| 26 | Package and build tooling | PARTIAL |
| 27 | Debugger and profiler | MISSING |
| 28 | Performance measurement | PARTIAL |
| 29 | Differential testing | VERIFIED |
| 30 | Mutation testing and fuzzing | VERIFIED / MISSING |
| 31 | Bootstrap | VERIFIED |
| 32 | Reproducibility and provenance | PARTIAL |
| 33 | Platform and target matrix | PARTIAL |

Nine VERIFIED, thirteen PARTIAL, three DESIGNED, two RESEARCH, eight MISSING. The shape is
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

*Since N15 there is a lossless concrete syntax tree — **the lossless CST is VERIFIED; the
area stays PARTIAL** for the limitations below.* One lossless scan
(`lexer::lex_lossless_in`) gives every byte of a file to exactly one lexeme; the one parser
builds the abstract tree and a `cstree` 0.14 green tree (`crates/nazm-syntax/src/cst.rs`)
from the same decisions, and `parse_cst` returns it. Its leaves, concatenated, are the input
byte for byte — whitespace, comments, punctuation, refused characters, trailing trivia —
and every leaf is the bytes it was scanned from, never a spelling rebuilt from its kind.
Comments are leaves of the tree; the formatter's comment list is derived from the same scan.
Inside a block, a malformed statement becomes an `Error` node and parsing resumes at the next
statement boundary, so what follows it is still structured syntax and a later independent
mistake is its own diagnostic. Evidence in `crates/nazm-syntax/tests/cst.rs`:
`a_lossless_tree_preserves_every_byte_and_recovers_inside_a_function`; every `.nz` file in
the repository and every grammar example round-trips with its byte-ownership invariants;
13 malformed fixtures each keep the construct after the error; seeded random input (3,000
cases) and seeded token deletions of the compiler written in Nazm always round-trip and
terminate; the abstract and concrete trees agree on the span of every construct the checker
sees, over the whole clean corpus. Against the pre-N15 parser, all 196 files give an
identical abstract tree and identical diagnostics, and 4,240 single-token corruptions of
real programs give the same first diagnostic and abstract tree in every case and one more
diagnostic in one (`architecture.md` §7.17). Sixteen N15 mutations were caught with verified
killers; the campaign is in area 30.

**Limitation** · Full-file parsing only: no incremental relexing or reparse, no reuse of
green nodes. Recovery is local at statements inside blocks; a mistake in a signature, a
record, an enum or a `use` still skips to the next item, and one inside an expression costs
its whole statement. After an item's first recovery, if its delimiters do not balance, its
further syntax diagnostics are withheld as cascades. The abstract tree still has no body for
a function whose body had a syntax error. The concrete tree is built on every parse and
costs about 2× the parse time and 1.8× the peak parse heap (`performance.md`). The compiler
written in Nazm has no concrete tree and does not recover. Parse nesting is bounded at 512
with a 24 MiB parse stack. The language server that reads this tree to find the token under a
position is area 25.

**Next dependency** · None for the tree. Structured edits and incremental reparse are
separate items (`roadmap.md`).

**Accepted when** · Met, 2026-09-25 (N15): a round trip through the tree preserves every byte
including trivia, and the parser produces a usable tree for a file with an error in the
middle of a function.

### 2. Name resolution and modules — PARTIAL

**Evidence** · Lexical scoping with shadowing rules and a separate function namespace,
`crates/nazm-core/src/check.rs`. `use "relative/path.nz";` resolved by the driver,
`crates/nazm-service/src/load.rs` — canonical-path keyed, read-once, depth-first, cycles
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
file-local offsets, and `crates/nazm-service/src/load.rs` produces a `SourceMap` whose files
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
(`crates/nazm-service/src/root.rs`, ten cases in `crates/nazm-cli/tests/source_root.rs`). With
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
| equality over a constrained type parameter | absent at N13 — there was no constraint mechanism; N14 added one (below) |
| custom (user-defined) equality | absent |

*N14, 2026-09-25.* **A type parameter may require equality.** `fn same[T: Equality](a: T,
b: T) -> Bool { a == b }` is accepted, the same without `: Equality` is still refused, and
every argument — written or inferred, concrete or a caller's own parameter — is checked
against the requirement by N13's one derivation, which now reads a required parameter as
having equality. So `Box[T]`, `Option[T]` and `Result[T, E]` with both required compare
inside a bounded body, `Vec[Int]` and `Option[Vec[Int]]` never satisfy it, and an
unconstrained wrapper cannot forward to a bounded callee. One compiler-owned capability,
`Equality`, on function parameters only; nothing is emitted at run time. Codes `N0364`,
`N0365`; `SEMANTIC_EPOCH` 6 → 7 and the interface schema `/4` → `/5`, which persists each
export's required positions. 14 tests in `crates/nazm-core/tests/constraints.rs` with code
and span, 3 native ones in `crates/nazm-cli/tests/constraints.rs`, interface-only and cache
cases in `durable_identity.rs` and `incremental.rs`, and
`the_nazm_checker_agrees_with_the_reference_on_requirements` comparing both checkers on 23
programs by code **and** span. `docs/spec.md`, *A type parameter may require equality*, is
the law; `architecture.md` §7.16 is the implementation.

| | status |
| --- | --- |
| first-order generics | VERIFIED |
| a generic parameter that requires the built-in equality capability | VERIFIED |
| a requirement on a record's or an enum's parameter | refused (`N0101`), deferred |
| any other capability, user-defined traits, `impl`, methods | absent |
| custom equality, sequence or channel equality, ordering, hashing | absent |
| higher kinds, const generics, specialisation | absent |

Seven constructs are verified rather than partial: **records**, **closed enums**,
**exhaustive variant match**, **first-order generics with `Vec[T]`**, **typed `Result`
propagation**, **derived structural equality** and **equality-requiring type parameters**.
What surrounds them is not verified, and none of it is a trait system. Typed errors are **not** effects: a `Result` in a
signature is a return type, and area 5 is unchanged.

**Limitation** · Generics remain narrowly constrained: N14 adds exactly one compiler-owned
requirement, `Equality`, on a function's type parameters, and that is not a trait system.
There are still no user-defined traits, no `impl`, no methods, no other capability
requirement, and no higher kinds, const generics, defaults, variance or specialisation —
so apart from `==` on a parameter that requires it, a generic body can do only what every
type can. A requirement cannot be written on a record's or an enum's parameter (`N0101`).
`Option` has no `?`, `Result` no
methods and no conversion between error types, and `main` cannot return one; no
indexing syntax on a `Vec`; no `spawn` of a generic function. No closures or function
values, no unit type, no `Never`.
Equality is the only thing a generic definition can require of `T` (N14): nothing can say
"`T` can be ordered", "hashed" or "passed to a task", and no program can declare a
capability of its own. No ordering, hashing or custom equality on any user type, and no
equality at all on a sequence, a `Vec` or a `Chan`. No field-level or per-variant visibility, no
default field values, no record update syntax. The pattern language is variant selection
with named payload bindings and `_` — no top-level wildcard arm, no guards, no or-patterns,
no literal, record, tuple or range patterns, and no `let` destructuring. Five keywords
remain reserved and refused by name rather than silently absent
(`crates/nazm-syntax/src/parser.rs`); `struct` left that list in N9 and `enum` and `match`
in N10, for the same reason `print` did in B3 — a reserved word earns its place by naming
something absent. *Until N11 this said neither a record nor an enum could be put in a
sequence; `Vec[T]` holds either.*

**Next dependency** · None forced by a program yet. N14's requirement covered the need N13
exposed with one built-in capability, and its dogfood found no compiler code that wanted
another (`research-register.md`, N14). *Until N14 this named constrained generics, which N14
added for equality; until N13 equality on user types; until N12 typed errors; until N12.1
blocks as values in the native backends.*

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

**Evidence of absence** · Neither exists; no HIR either — no typed Core IR, no MIR, no
HIR. What changed is the reason. Since N11 the language has first-order generics and the
native backend compiles **monomorphised concrete instances**: the checker interns each
applied record and enum with its substituted fields (`crates/nazm-sema/src/generic.rs`),
and native lowering plans one instance per concrete generic function and lowers each from
the same syntax tree (`crates/nazm-lir/src/instance.rs`). So polymorphism and
monomorphisation exist, and **no dedicated Core or MIR level carries them** — the existing
semantic and native pipeline does the specialisation, and nothing has shown that a level
between them would own something neither owns. Traits, regions and effects are still
absent, so none of those gives a middle level work either.

*Historical, M1a:* `crates/nazm-lir/src/lib.rs` gave the original reason — the levels in
between *"exist to carry information M1a has none of"*, no generics to monomorphise, no
traits to lower, no regions to infer, no effects to track. The absence it justified still
stands; its "nothing is polymorphic" does not, since N11.

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

*N30, 2026-09-28: Compact Diagnostic Index v1, `nazm.diagnostic-index/1` — VERIFIED; Diagnostic
Detail v1, `nazm.diagnostic-detail/1` — VERIFIED; G74, G75, G77 — PARTIAL.* An index
(`crates/nazm-service/src/diagnostics.rs`, `architecture.md` §7.32) is every current diagnostic
of a root compilation as compiler-owned facts — `code@file:start-end#n` id, code, severity, file
and UTF-8 byte range, owning durable definition, fix counts by applicability — with no prose,
bound to a digest of the loaded sources and, where N28 has one, the snapshot; detail is one
diagnostic exactly as `nazm.diagnostic/1` publishes it, its places in their files, and for each
fix the N29 selector where a semantic patch is plannable. Compact diagnostics never infer
structured semantics from diagnostic prose; a compiler fix and an N29 semantic patch are distinct
capabilities. The goals are partial because no diagnostic carries typed expected/actual/entity
facts (`facts: "unavailable"`), only the compact and detail levels exist, and there is no
diagnostic history. Evidence: `crates/nazm-service/tests/diagnostics.rs` — an index with no
sentence the compiler wrote in it, ordered by place, owned by `main`; each detail the canonical
diagnostic with the index's code, place and owner; lexer and parser errors, a missing `;`, an
unterminated string and a mutable parameter indexed with no snapshot and no owner, their compiler
fixes no semantic patches; an automatic and a needs-review fix counted by applicability, each
detail's selector planning exactly that patch through N29; one code twice in one file and across
files as distinct ids, `lib.nz`'s places its own and its published offsets the concatenation's; a
`+` after `é😀` at its byte offset, not its character index; a fake id refused, and after a move, a
repair or another state every old id `stale_state`; byte-identical indexes from another directory,
an edit in an imported file a new state, a broken file nothing imports absent, and two roots over
one file each their own; unknown name, type mismatch, arity, visibility, a refused duplicate
(no owner), a generic mismatch, an unsatisfied requirement and a task capture each indexed with
its owner and detail; and 500 index and detail pairs leaving retained state unchanged. The unit
tests tell two structurally identical diagnostics apart by ordinal alone and show rewording a
diagnostic changes no index byte. `crates/nazm-cli/tests/schemas.rs` the real command's indexes
and details valid against their schemas, every detail's diagnostic equal to a line of `nazm check
--json` and valid as `nazm.diagnostic/1`. `crates/nazm-mcp/tests/protocol.rs` the real server:
six read-only tools, index and detail equal to the service's, the index carried once, no root or
file argument accepted, a stale id after an edit, syntax errors indexed, and 1,000 calls through
edits and repairs with flat resident memory. Eighteen N30 mutations (area 30).

**Limitation** · Two codes are unprovokable by construction. Spans are offsets into a
merged buffer, so file identity is recovered rather than carried (area 2); the N30 index and
detail recover it, by the same source map. No diagnostic carries typed facts, and the index is a
module's analysis — a missing `main` (`N0204`) is `nazm check`'s, not the index's.

**Next dependency** · None.

**Accepted when** · Met.

### 23. Formatter — VERIFIED

**Evidence** · `crates/nazm-syntax/src/format.rs`, token-driven so comments survive, with
no options by design. `crates/nazm-syntax/tests/format.rs` asserts on every input that
tokens are unchanged, that `format(format(x)) == format(x)`, that comments are identical,
and that output ends in exactly one newline — applied to every `.nz` in the tree and to 15
hand-written ugly inputs.

**Limitation** · No reflow, no reordering, no insertion. Per file, not per merged program.
Canonical in the sense of "one output". Since N15 a lossless tree exists (area 1), and the
formatter reads its comments from the same scan the tree is built from, but it still lays
out from tokens rather than from the tree.

**Next dependency** · None for formatting; edits are a separate item.

**Accepted when** · Met for formatting.

### 24. Machine-applicable fixes — VERIFIED

**Evidence** · `crates/nazm-cli/src/fix.rs`, schema `schema/nazm.fix-1.json`. Only
`Automatic` fixes apply; nothing is written without `--apply`; edits apply right-to-left;
overlaps are skipped; and a result that no longer parses is rolled back wholesale. Ten
workflow tests in `crates/nazm-cli/tests/workflow.rs`. *N24, 2026-09-26:* the code has
**seven** skip reasons, not the four this entry used to count, and each is now reached by its
own unit test in `fix.rs` — `write_back` reads and writes through injected functions, so an
unreadable and an unwritable file are arranged rather than hoped for: a fix that needs review
(`it needs review`), an unreadable file, two overlapping edits (the later applied, the earlier
skipped, and exactly the one written), a span past the end and one inside a character (`the
span does not name a range of this file`), bytes that are no longer the ones checked (nothing
written), a result that does not parse (nothing written), and a failed write (not reported
applied). Each asserts the exact `skipped` text. `docs/diagnostics.md` lists the seven. The
same fixes reach an editor as quick fixes (area 25), a distinct consumer of one fix contract.

**Limitation** · Stale-edit protection is by expected-bytes comparison, not a file hash or
document version — which is what a command that reads and writes a file in one run needs.

**Next dependency** · None.

**Accepted when** · Each skip reason has a test that reaches it: met, 2026-09-26 (N24), for
the seven the code has.

### 25. LSP — PARTIAL · MCP — PARTIAL

**Evidence** · *N16, 2026-09-25.* `nazm lsp` serves the Language Server Protocol over stdio
(`crates/nazm-cli/src/lsp.rs`) as an adapter over a protocol-independent language service
(`crates/nazm-service/src/service.rs`, `architecture.md` §7.18) that answers from the loader,
parser, checker and resolution every command uses — nothing is re-implemented, and the
layering and `one resolver` gates keep it so. Open buffers are the source: they override the
disk for every compilation, an unsaved dependency is what its importer sees, closing a buffer
gives the file back to the disk, and an unsaved `use` changes the graph. Diagnostics keep
their Nazm codes and are published per version, an empty set included. Definition follows the
resolution's identities — calls, locals (by slot), record constructions, variants, fields and
payload fields — across files; hover shows a function's declared signature or a local's type,
from the same resolution. Positions are converted between byte offsets and UTF-16 at the
adapter boundary only. Evidence: `crates/nazm-service/tests/service.rs` (overlays, close,
import edits, versions, a broken buffer never answering from the clean one, every definition
category, a spelling with three meanings, the prelude, the cursor rule, the service's
diagnostics equal to the command line's over every example and multi-module conformance
program, a thousand edits with constant retained state); `crates/nazm-cli/tests/lsp.rs` (the
real process over real framing — handshake, exact capability set, diagnostics and definition
in UTF-16 after `é→😀`, an unsaved dependency and its close, unimplemented methods refused,
malformed source and a malformed frame without a panic, stdout nothing but frames); the
position unit tests in `lsp.rs`. Fourteen N16 mutations were caught with verified killers
(area 30).

*N17, 2026-09-25: references — VERIFIED as a sub-capability.* `textDocument/references` and
`LanguageService::references` answer from one reference index per analysis
(`crates/nazm-sema/src/references.rs`, `architecture.md` §7.19): the resolution read backwards,
with no name resolved again. Functions, locals (parameters, `let`s, pattern bindings, by slot
of a function), records (their constructions), variants, fields (projections, assignments and
construction labels) and payload fields; across files and across every open compilation that
loads the declaring file; from unsaved buffers; declaration apart from uses, combined only for
`includeDeclaration`; ordered by file and position, each occurrence once. Evidence:
`crates/nazm-service/tests/references.rs` — one spelling given to a function, a parameter, two
shadowing `let`s, fields of two records, variants of two enums and private functions of two
modules, each its own set asked from every one of its occurrences; definition and references
agreeing at every byte of every file; shadowing; a generic record's field through two
instances; the unsaved program (a use added, removed, moved to another same-named entity, and
the disk back on close); a broken buffer; only analysed programs searched; unsupported
positions; the prelude; a thousand edits with constant retained state; and, over 40
compilations of the repository's own source (the compiler, the conformance programs, the
examples), every resolved name token in exactly one entity's set and the service's answer equal
to the command line's resolution read backwards. `crates/nazm-cli/tests/lsp.rs` adds the real
process: the capability, UTF-16 after `é→😀` on both the query and the answers, an unsaved
edit, and the same answer with a warm semantic cache beside the sources and without one.
Sixteen N17 mutations, all caught at tier 1 (area 30).

*N18, 2026-09-25: rename — VERIFIED for locals and private definitions only.*
`textDocument/prepareRename` and `textDocument/rename` over `LanguageService::rename`'s
validated plan (`crates/nazm-service/src/rename.rs`, `architecture.md` §7.20). Renameable:
parameters, `let`s and pattern bindings; and private functions, records, enums, and the fields,
variants and payload fields of private records and enums. The proof of completeness is the
language's own: a local is written only in its body, and a private entity only in its module
(`N0337` keeps private types out of public signatures), in a clean analysis whose every written
type name, qualifier, label and use the checker now records — type names and enum qualifiers
since N18. Every plan's candidate program is re-checked in memory in every open compilation
containing the edited file and must partition its occurrences into entities exactly as before.
Refused: exported entities (their importers cannot be known), core-prelude entities, built-ins
and type parameters, programs with errors, a name the lexer does not read as one identifier,
and every collision or capture the checker or the partition finds. The protocol edit is
versioned `documentChanges` only, only for a client that accepts them, and only when every file
the plan edits is open. Evidence: `crates/nazm-service/tests/rename.rs` — a type rename through
declaration, parameter, return, field, payload, nested generic argument, construction and
qualifiers, stated as the exact edited text; one rename from any occurrence; definition on a
type annotation and on a qualifier; same-spelled locals, fields, variants and a record and a
function of one name renamed apart; an enum, its variant, its payload field and the pattern
local bound to it, apart; capture and collision refused and legal shadowing accepted; built-in
names; invalid names; non-entities and the prelude; a program with errors; private accepted and
exported refused across modules, and a private rename validated in the importer's compilation;
stale plans by generation, version and text; a thousand plans with constant retained state.
`references.rs` adds the corpus gate — every name token of every clean compilation of the
repository indexed or explicitly a non-entity. `crates/nazm-cli/tests/lsp.rs` adds the real
process: the capability, prepare-rename's range and placeholder, refusals as `null`, a versioned
edit with every range in UTF-16 after `é→😀`, an unsaved edit's new version, refusals with
reasons, and a client without versioned edits refused. Fourteen N18 mutations, all caught at
tier 1 (area 30).

*N19, 2026-09-25: completion — VERIFIED for identifier completion in value, call-head and type
contexts only.* `LanguageService::scope_at` answers what may be written at any position, and
`completion` and `textDocument/completion` complete the identifier under the cursor, from a
`ScopeTrace` the checker writes as it pushes scopes, defines bindings and builds environments
(`crates/nazm-sema/src/scope.rs`, `architecture.md` §7.21) — with no lookup of its own. Values
are locals (parameters, `let`s, pattern bindings), visible from where the checker introduced
them and hidden where it recorded a nearer binding hiding them; calls are the module's
functions, its direct imports' exports and the built-ins (`Intrinsic::ALL`); types are type
parameters in their own definition, the module's and its direct imports' records and enums,
and the built-in types (`Type::ALL`, `Vec`). A name the checker refused (an ambiguous import, a
collision, a same-scope duplicate) is offered as nothing. A position whose function needed
syntax recovery has no locals, and a compilation with any recovery has no function or type
completion. Invoked only (no trigger characters), `resolveProvider` false, `isIncomplete`
false. Evidence: `crates/nazm-service/tests/completion.rs` — one spelling as a record, a
function, a parameter and a shadowing `let`, each offered only in its own context and scope;
later locals absent, a `let` absent from its own initializer, loop, branch, `scope` and arm
bindings not leaking, sibling arms apart; a later top-level function callable; type parameters
in their own definition only; built-ins from the compiler's inventories and none spelled in the
service; direct imports' exports only — no private, no transitive; an ambiguous import offering
neither; another open program never mixed in; unsaved locals, imports and dependencies, and the
disk back on close; recovery refused, type errors not; non-sites refused; a thousand edits with
constant retained state; and over the repository's own clean compilations every one of 23,821
resolved names offered, once, as exactly what it resolved to. `crates/nazm-cli/tests/lsp.rs`
adds the real process: the capability, the whole identifier replaced in UTF-16 after `é😀`, a
built-in call head by prefix, a string refused, an unsaved local. Fourteen N19 mutations (area
30).

*N20, 2026-09-26: signature help — VERIFIED for calls of user functions (the module's own and
imported ones), generic instantiations (written and inferred) and built-ins, and for `spawn`.*
`LanguageService::call_at` and `signature_help`, and `textDocument/signatureHelp`, report the
call whose argument list holds the position from a `CheckedCall` the checker records wherever
it checks a call against a resolved callee (`Resolution::checked_call`, `architecture.md`
§7.22): the callee, the argument spans, the parameter and return types the environment gave —
an import's from its interface — and what a generic call settled. The tree decides only which
argument list holds the position (innermost by nesting) and which argument (that list's own
commas). Too few or too many arguments and wrong argument types still answer; an unresolved
callee, a record or variant construction, a position outside any argument list and a
compilation with any syntax recovery do not. One `Signature` and one renderer serve hover,
completion and signature help. Triggered by `(` and `,`, no retrigger characters, one
signature, parameters as UTF-16 label offsets; `null` past the last parameter; no active
parameter for a callee with none. Evidence: `crates/nazm-service/tests/signature.rs` — a record,
a function, a local and another module's private function sharing a spelling, each call shown
as the function it resolved to (by declaration, through the reference index); a generic call
inferred and written, with its settled arguments; a built-in; `[T: Equality]`; a zero-parameter
call; a construction refused; twenty-two positions across nested calls, strings, comments,
parenthesised arguments, constructions and nested type arguments, each with its exact active
parameter; too few, too many, wrong types, an unresolved callee, a `spawn`; a stale version, a
broken and a repaired source, a recovery elsewhere in the file, an unsaved and then closed
dependency signature, another open program; a thousand edits with constant retained state and
checked-call count; hover and signature help rendering one signature; a structural test that
the module looks nothing up and substitutes nothing; and over the repository's own clean
compilations 9,030 calls (71 generic) whose record is the resolution's callee with the
declaring signature and the backend's type arguments, 4,814 of them asked of the service.
`crates/nazm-cli/tests/durable_identity.rs` adds a call checked against a deserialised interface
with the dependency deleted, recorded with the interface's signature. `crates/nazm-cli/tests/lsp.rs`
adds the real process: the capability, label offsets, a comma inside a string after `😀`, a
nested built-in, no active parameter, `null` past the end and outside the call, an unsaved edit.
Fourteen N20 mutations (area 30).

*N21, 2026-09-26: structure completion and constructor signature help — VERIFIED for member
fields after `.` (variables, temporaries, chains, generic records), enum variants after `E.`
(in a construction, a pattern, or before its `(` for a non-generic enum), record construction
labels, variant construction payload labels, pattern payload labels, and record and variant
constructor signature help.* `LanguageService::structure_at`, the structure contexts of
`completion`, and `help_at` answer from what the checker recorded — a field's `FieldRef`, a
variant's `VariantRef`, a construction's record, a payload's `PayloadRef` — and from one narrow
checker fact, `Resolution::looked_up_on`, the record or enum an unresolved member name was
looked for on, written only where a lookup failed (`architecture.md` §7.23). The service and
the LSP adapter perform no name lookup and no semantic re-resolution; normal structure sites
consume checker-recorded identities. The only spelling-based lookup N21 introduced is the
canonical checker's incomplete-source anchor for `E.name` written before its `(`: after `E`
fails as a value, the checker consults its existing module type environment and records a
non-generic enum (§7.23). No diagnostic and no language semantics changed. Candidates are that record's fields, that enum's variants or that variant's payload
fields, in canonical order, with substituted types; labels given elsewhere are excluded by
identity and the label being edited is kept. Constructor signature help is its own
`Constructor`, never a call; its active field is its label's identity at its canonical
position. Answered around a recovery only when it is inside another function's body.
Evidence: `crates/nazm-service/tests/structure.rs` — two records sharing `value`, two enums
sharing `Ready`, two variants sharing a payload `value`, a record and a function sharing `Box`,
a temporary receiver, a chain through `Holder[A]`, `Pair[Int, Str]` and `Opt[Int]` substituted,
each candidate identified by its declaration; a pattern binding refused as a label; labels given
elsewhere excluded, the edited label kept, an unknown label offered what is not given; an
unknown field, an unknown variant and `EA.Rea` with no `(` offered their anchor's members, and
unknown receivers, constructors and enums refused; constructor help out of canonical order,
between initialisers, on an unknown label, around a nested call, nested constructions, generics,
variants, a construction inside a call; reordered fields, payloads and variants changing no
answer; recoveries in another function answered and in the same function, the top level and
another file refused; an unsaved and closed dependency, a private dependency record refused,
another open program; a thousand edits with constant retained state; and over the repository's
own source 518 resolved sites — 278 members, 126 variants, 73 construction labels, 31 pattern
labels — each offered, once, as exactly the identity the checker gave it.
`crates/nazm-cli/tests/durable_identity.rs` adds the construction, fields, unknown member and
variant payload read from a deserialised interface with the dependency deleted;
`crates/nazm-cli/tests/lsp.rs` the real process with `é😀` before every site. Fifteen N21
mutations (area 30).

*N22, 2026-09-26: document symbols — VERIFIED; workspace symbols over the current analysed
LanguageService universe — VERIFIED.* `LanguageService::document_symbols` is one open
document's functions, records, enums, variants, fields and payload fields, nested — a record's
fields under it, an enum's variants under it, a variant's payload fields under the variant — in
source order, each with its whole declaration as its range and its declared name as its
selection. `LanguageService::workspace_symbols` is every such declaration of every file the
open documents' compilations load, each once however many compilations load it, identified by
file, kind and name span and never by spelling, filtered by a case-sensitive substring of the
name and ordered by name, file, position and kind. Both are derived on demand from the
checker's definition tables and the parsed items their `decl` indices name
(`architecture.md` §7.24); nothing is kept. Over the protocol: `textDocument/documentSymbol`
(nested, or flat with container names for a client without hierarchical support) and
`workspace/symbol` (`resolveProvider: false`). Evidence: `crates/nazm-service/tests/symbols.rs`
— two records' `value`, two enums' `Ready`, a record and a function both `Box`, two modules'
`helper`, each separate; an import absent from its importer's outline; a broken body kept, an
unreadable item and refused duplicates absent, a type error kept; two roots sharing a module
listing it once, then after one closes, then none; an unimported `orphan.nz` beside them never
listed; the prelude and a missing module never located; unsaved edits seen at once and close
restoring the disk; a stale version refused; the query rule pinned on `Point`, `Pointer`,
`Checkpoint`; one order over ten runs; 1,000 edits with retained state constant; every symbol
the entity the reference index records at its name and where definition goes from every use;
over the repository's own source, every one of 817 declarations in 70 clean documents one
symbol with the same kind, name, range and parent as its concrete tree, and the compiler opened
as four overlapping programs listing each of its 500 declarations in its 8 files exactly once.
`crates/nazm-cli/tests/lsp.rs` the real process, with `é😀` before every checked position.
Thirteen N22 mutations (area 30).

*N23, 2026-09-26: semantic identifiers and `textDocument/semanticTokens/full` — VERIFIED for
functions, intrinsics, parameters, locals and pattern bindings, records, enums, variants,
record fields, payload fields, type parameters and built-in types, each as a declaration or a
reference.* `LanguageService::semantic_identifiers` classifies each identifier of an open
document by what the checker recorded at exactly its span — an entity in the reference index,
a built-in call, or a written built-in or type-parameter name (`Resolution::written_type`, the
one checker record N23 added; `architecture.md` §7.25) — keeping the compiler's identity for
what it names; an identifier the checker resolved to nothing is not classified. Over the
protocol: `semanticTokens/full` only — no range, no delta, no `resultId`, no token cache —
under a fixed legend of nine types and two modifiers (`declaration`, and `defaultLibrary` on
intrinsics and built-in types only), relatively encoded in UTF-16. Semantic identifiers only:
no keywords, comments, strings, numbers or operators. Evidence:
`crates/nazm-service/tests/semantic.rs` — every identifier of a fixture pinned with its class
and role, nothing from a comment or string; a record and a function both `Box`, two records'
`value`, two enums' `Ready`, shadowed locals, a parameter named like a field, and `State.Done(code:
code) => code` each keeping its identity, each declaration and its uses one identity; unknown
call heads, members, types and qualifiers unclassified; a recovered body losing only what it
left unresolved and a renamed declaration's old use disappearing, not remembered; a refused
signature lending its type parameters to no one; an imported function, an unsaved edit,
another open program and close; 1,000 edits with retained state constant; over the
repository's own source, 24,870 entity occurrences in 70 clean documents each one identifier of
the same entity and role, 817 declarations matching their N22 symbols, 2,371 built-in type
names, 94 type-parameter names and 5,875 intrinsic calls, nothing unaccounted, and 2,679 type
names found from the concrete tree alone all classified. `crates/nazm-cli/tests/lsp.rs` the real
process with `é😀` before tokens across blank lines, the exact integer array computed
independently, and range and delta requests refused. Fourteen N23 mutations (area 30).

*N24, 2026-09-26: `textDocument/codeAction` for current compiler diagnostic fixes — VERIFIED;
quick fixes only, open versioned documents only, no resolve, no fix-all; both `automatic` and
`needs_review` fixes are surfaced.* `LanguageService::fix_plans` turns each fix the current
diagnostics touched by the requested range carry into a `FixPlan` — the diagnostic, the fix's
applicability, description and precondition, the generation, and one versioned edit with the
bytes it replaces — and `fix_plan_is_current` holds it to all of them with the freshness law
renames use (`architecture.md` §7.26). Nothing is invented from a code or a message; the
client's diagnostics are not consulted. An automatic fix is preferred when it is its
diagnostic's only fix; a needs-review fix is titled with its precondition and never preferred.
Evidence: `crates/nazm-service/tests/fixes.rs` — all five fixes the compiler attaches (`mut`
on a `let`, `mut` off a parameter, `;` after a loop's value, `;` after an expression, a closing
quote), each planned exactly from its diagnostic, applied in memory to text that parses, with
its diagnostic gone and, when automatic, nothing outside its span changed; the edit on the
fix's span while relevance is the diagnostic's; the range rule at a cursor inside, at either
end, across two diagnostics and adjacent; a plan refused for other bytes, another version, no
version, another generation, a change outside its edit and one inside it; unsaved buffers, an
unsaved dependency and its close; no fix from an earlier analysis; 1,000 edits with retained
state constant; and, at the plan layer, two overlapping alternatives kept apart and a fix in a
file not open never planned. `crates/nazm-cli/tests/lsp.rs` the real process: a quick fix after
`é😀` with its versioned edit's UTF-16 range computed independently, unchanged until the client
sends `didChange`, then gone with its diagnostic; a stale and a fabricated client diagnostic
producing nothing; a needs-review fix with its precondition; `only` honoured; a client without
versioned edits offered none. Seventeen N24 mutations (area 30).

*N25, 2026-09-26: `textDocument/prepareCallHierarchy`, `callHierarchy/incomingCalls` and
`callHierarchy/outgoingCalls` — VERIFIED; source-backed functions only, the bound current
compilation snapshot only, direct checked calls only, no filesystem or project-wide caller
discovery, and no item for an intrinsic or a function without source.* An item is a function
the checker declared from source, prepared from its declaration or a call through the reference
index, with its outline symbol's ranges; an edge is a `CheckedCall` to such a function,
attributed to the body whose call sites the checker recorded it in, grouped by the function at
the other end and located at each callee name (`architecture.md` §7.27). No per-call state was
added and nothing is kept between requests. An item's opaque locator binds it to the
compilation it was prepared in and the generation it was prepared at; any change, or closing
that root, leaves it unanswered. **This is complete only for the bound current compilation
snapshot: it is not evidence that every importer or caller in a project is loaded**, and it
does not lift the exported-rename refusal. Evidence: `crates/nazm-service/tests/hierarchy.rs` —
preparation from a declaration, a call and the position just after a name, and none from a
same-spelled record, a construction, a field, an enum, a variant, a payload label, a local, a
parameter, an intrinsic or a type; items at their N22 symbol's ranges; two callers and two
callees each grouped with every call in order, two calls on one line; calls nested in
arguments, `if`, `while` and `match` owned by their function; recursion and mutual recursion;
three instantiations of one generic function as one item, each range the callee name alone;
constructions, variants and intrinsics not edges, a `spawn` an edge and a refused generic
`spawn` — a reference with no checked call — none; four files through an import cycle, a
private function, and two `helper`s kept apart as callees and as callers; a call with a type
error kept, a misspelt call dropped beside a kept one, an unparsable body losing its calls with
nothing brought back; unsaved callers, an unsaved dependency and its close; a refused duplicate
declaration no item, its calls the checker's; two roots over one shared file each seeing only
its own callers, and a closed root's item unanswered, never rebound; an item unanswered at
another version, after an unrelated edit and after an identical one; 1,000 edits through six
shapes with every old item unanswered and retained state constant; and, over the repository's
own source, every checked call of 70 clean documents to a function with source exactly one edge
occurrence from the body that holds it — found independently by containment — to the function
the checker resolved, seen alike from both ends, at the entity the reference index records,
at items matching their N22 symbols, and classified by N23 as a function declared or
referenced. `crates/nazm-cli/tests/lsp.rs` the real process: prepare, incoming and outgoing
after `é😀` with every range computed independently in UTF-16, a cross-file callee, a stale item
and forged data answered `null`, and two roots over one file. Fourteen N25 mutations (area 30).

*N26, 2026-09-27: completion at a structured hole — a bare member after `.`, a bare enum variant
after `E.` or `E[T].`, a bare construction or pattern label after `(` — and `.` as the
completion trigger character — VERIFIED; anchored only where the canonical compiler establishes
one record, enum or variant.* A request with no name at the cursor, right after a `.` or `(`
token, runs a completion probe: the compilation's current sources parsed by the same parser told
the cursor's offset — which reads a zero-width empty name there and nothing else differently — and
checked by the same checker, which anchors the hole where it already decides: the type an unknown
member was looked for on, the enum an `E.` names when no value does, the variant a construction
or pattern names, and — only where the module calls no function by that name — the record a bare
`Name(` would construct (`architecture.md` §7.28). Its candidates are N21's, from one function:
identities, canonical order, substituted details; the range is empty at the cursor. Nothing is
recorded by an ordinary analysis, the probe's diagnostics are never published, and nothing is
kept. Evidence: `crates/nazm-service/tests/incomplete.rs` — a parameter, a local, a temporary, a
projection chain and a call argument each giving exactly the receiver's fields; an applied generic
record substituted; a non-generic and an applied generic enum's variants, and a generic enum with
no arguments refused; a record type, an unknown name, an enum value, an `Int` and `p..` refused; a
local named like an enum reading as the local; two records' `value`, two enums' `Ready` and two
variants' `value` kept apart; a record's labels after `Point(`, `Point()` and `Point();`, a generic
record's substituted, none for a generic record with no arguments, a function, an unknown name, a
record and a function of one spelling, or a generic record and function of one spelling; payload
labels in a construction, an applied generic's, and a pattern alone, before another arm and after
one; the same candidates as `p.|x`, differing only in the range, and `p.x` unchanged; declaration
order irrelevant; the hole alone answered, another error before or after it in its function, at the
top level or in a dependency refused, another function's allowed; strings, comments, whitespace
and a second dot no hole; a private dependency record and an ambiguous import never anchors;
unsaved receivers, an unsaved dependency and its close, no earlier answer, a stale version refused,
another open program lending nothing; the published diagnostics, the text and retained state
unchanged by asking; 1,000 edits through ten shapes; and, over the repository's own source, cut
member and variant sites each completing to a list holding exactly the member the checker had
resolved there. `crates/nazm-syntax/tests/holes.rs` — over every corpus file, broken test programs
included, a probe parse at an offset with no hole the ordinary parse exactly; the hole read only at
the exact byte and changing only its statement; a later missing `;`, two dots and a written label
still what they were. `crates/nazm-cli/tests/lsp.rs` the real process: `.` advertised alone,
invoked and triggered completion identical after `é😀`, each edit empty at the UTF-16 cursor, and a
trigger in a string, a comment or whitespace answered `null`. Eight N26 mutations (area 30).

*N27, 2026-09-27: Semantic Context Packet v1, `nazm.context/1` — VERIFIED for source-backed
functions, records and enums, in the current root compilation, with direct semantic links and
target-local diagnostics; and MCP — PARTIAL: one read-only stdio tool, `nazm.semantic_context`,
reading the disk at each call.* A packet (`crates/nazm-service/src/context.rs`, `architecture.md`
§7.29) is the target found through the reference index at a byte offset, its durable identity, its
signature or shape by the existing renderers, its exact source by N22's range, and compact links —
identity, kind, name, place — for its dependencies (every entity used inside it, grouped by
identity), related types (N18's written names of program records and enums, directly), references (uses only), callers and
callees (N25; `not_applicable` for a type) and diagnostics inside it; effects, capabilities, tests,
semantic changes and deltas are `unsupported`, never empty. Paths are source-root-relative and
nothing session-local is serialised. `nazm context --json` prints it; `nazm-mcp`
(`crates/nazm-mcp/src/main.rs`), on the official Rust SDK, serves it with
`schema/nazm.context-1.json` as its output schema. This establishes semantic context retrieval for
those three kinds and a compact-by-design contract for the fields it promises; it does not make a
packet sufficient for every task, and G68–G70 remain broader. Evidence:
`crates/nazm-service/tests/context.rs` — a function, record and enum each the same packet from its
declaration and from a use, a record and a function of one name and two modules' `helper` apart;
locals, parameters, fields, variants, labels, intrinsics, built-in types, whitespace, keywords and
comments refused, a file beside the root and `/etc/passwd` not in the compilation; the exact source
slice; dependencies grouped by identity with every occurrence, same-named fields of two records
apart, locals, intrinsics and the target itself excluded; related types direct; the core prelude's
`Option` and `Result` and a built-in type never linked, since v1 links only entities with a
declaration in a file of the compilation, and every link's place checked to be one; a private definition of an imported module still linked; references every use and not the
declaration; callers and callees equal to call
hierarchy's, a type's not applicable; diagnostics only the target's; availability unsupported; a
syntax error or a missing module refusing; two roots over one file each seeing its own; a hundred
serialisations and a copy in another directory byte-identical with no absolute path; each packet of
the disk as it is then; and, over the repository's own source, 649 packets of 70 clean documents
matching N22's ranges and signatures and N23's classification, every dependency occurrence the
reference index's use of the linked entity, and callers and callees N25's. `crates/nazm-cli/tests/schemas.rs`
the real command's packets and refusals valid against the schema. `crates/nazm-mcp/tests/protocol.rs`
the real server through the SDK's client: only tools advertised, one read-only tool with the
published output schema, packets equal to the service's and carried once — structured content, an
empty `content`, the packet's schema string exactly once on the wire — protocol errors for an unknown tool and bad
arguments, refusals for files outside the compilation, the disk re-read between calls with a broken
edit refused and a repair answered, two servers with two roots isolated, 1,000 calls with flat
resident memory, and nothing but JSON-RPC on standard output. Sixteen N27 mutations (area 30). Context
packet v1 exposes source-linked program dependencies only: compiler-owned, built-in, interface-only
or toolchain definitions with no navigable declaration may affect checking but are not linked.

*N28, 2026-09-27: Semantic snapshot v1, `nazm.snapshot/1` — VERIFIED; Semantic delta v1,
`nazm.delta/1` — VERIFIED; for durable source-backed functions, records and enums of the current
root compilation, and the sections the compiler records of them. G71 — PARTIAL; MCP — PARTIAL,
three read-only tools.* A snapshot (`crates/nazm-service/src/snapshot.rs`, `architecture.md` §7.30)
is every such definition by `DefKey`, with one BLAKE3 digest each for its exact source, shape
(kind, visibility, signature or fields or variants), source-linked dependencies, related types,
references, callers and callees (a function's; null for a type) and diagnostics — relationships
digested by identity and count, never offset — plus the root's module key and compiler, a count of
definitions with no durable key, and `unsupported` naming effects, capabilities, tests, semantic
history and behavioural equivalence. A delta (`crates/nazm-service/src/delta.rs`) validates a
baseline as data and reports added, removed and changed definitions by key, with a record of which
sections changed. N28 reports changes in the semantic surfaces Nazm currently records; it is not a
proof of behavioural equivalence: `{ 1 }` to `{ 2 }` is `source` alone, and effects, capabilities
and tests are unsupported, not unchanged. G71 is partial for that reason — a delta names what the
compiler records, and no behavioural, effect or test delta. Evidence:
`crates/nazm-service/tests/snapshot.rs` — every durable definition by identity, ordered, with two
modules' `helper` apart and no source text or packet inside; byte-identical from fresh services, at
any service generation, and from a copy in another directory with no host path and no git
repository; a file nothing imports absent and another root's compilation its own; a module outside
the source root counted untracked, never matched; recovery and a missing module refusing; no edit
and a moved definition no change; a comment and a changed literal `source` alone; a function added,
an enum and a function and a record removed, a rename removed plus added and never inferred; a
signature, a field, a variant and visibility `shape`; a new call changing the callee's references
and callers and the caller's dependencies and callees, and a second call counting; a use elsewhere
changing only a record's references; a new diagnostic changing only its definition; a broken edit
refusing and the repair answering; every malformed baseline refused with its reason — another
schema, an unavailable snapshot, an unknown or missing field, a record's callers, a non-canonical,
wrong-kind, absolute or prelude identity, a duplicate, a bad digest, another root, another
compiler; and 500 snapshot and delta pairs leaving the service's retained state unchanged.
`crates/nazm-cli/tests/schemas.rs` the real commands' snapshots, deltas and refusals valid against
both schemas. `crates/nazm-mcp/tests/protocol.rs` the real server: three read-only tools with the
published output schemas, a snapshot equal to the service's and a delta needing only it, each
carried once on the wire, an edit seen between calls, a broken edit refused for both tools and the
repair answered, a path or path-shaped object refused as a baseline without being read, protocol
errors for missing or extra arguments, and 1,000 snapshot and delta calls with flat resident
memory. Twenty N28 mutations (area 30).

*N29, 2026-09-27: Semantic Patch Plan v1, `nazm.patch/1` — VERIFIED, for two operations,
`rename` and `diagnostic_fix`; patch application — MISSING; MCP write or edit tools — MISSING;
G72 — PARTIAL.* A patch (`crates/nazm-service/src/patch.rs`, `architecture.md` §7.31) is N18's
validated rename or the fix a current diagnostic carries (N24), as minimal exact-byte edits, bound
to BLAKE3 of the root's `nazm.snapshot/1`, each edited file's BLAKE3 digest and each edit's
expected bytes; a fix's applicability and precondition are structured fields. N29 plans edits but
never applies them. Exported rename remains refused, because one root compilation is not a
complete importer universe; N29 does not alter LSP closed-file safety. G72 is partial because
minimal structured edit planning exists for these two operations, and generic semantic edits,
application and write automation do not. Evidence: `crates/nazm-service/tests/patch.rs` — a
private function renamed at exactly its declaration and call, with its durable key, the file's
digest, the snapshot's digest, each edit's bytes and no other text; every N18 category — local,
record, field, enum, variant, payload field — at exactly its occurrences, each applied to a copy
and checking clean; a local named by its place and its function, never a key; a private function
of an imported file no editor has open planned from disk while the service's own rename there is
still refused; exported, invalid, unchanged, colliding, unentitied, out-of-range, unloaded,
unclean and unparsable requests refused; an automatic and a needs-review fix with the compiler's
applicability and precondition, each applied and its diagnostic gone; no fix at a position with no
diagnostic, at an index past the fixes, after the repair, or for a syntax diagnostic; each
freshness layer going stale when what it protects moves, and the same request re-planned fresh;
byte-identical plans from another directory; the N28 delta after a rename (the old key removed,
the new added, the caller's source, dependencies and callees, and the record it took's references)
and after a fix (the definition's source and diagnostics); and 500 rename and fix plans each
leaving retained state unchanged. The unit tests order edits and refuse duplicates, overlaps and
two insertions at one point. `crates/nazm-cli/tests/schemas.rs` the real commands' patches and
refusals valid against the schema (with `oneOf` added to its validator for the tagged operation).
`crates/nazm-mcp/tests/protocol.rs` the real server: four read-only tools, `nazm.semantic_patch`'s
input the SDK's derivation of its typed request, patches equal to the service's and carried once,
the files untouched, exported refused, unloaded files and `/etc/passwd` refused, a root, an unknown
operation or argument and a client-written replacement protocol errors, a disk edit re-planned, a
broken edit and a vanished target refused, and 1,000 plans with flat resident memory.
Sixteen N29 mutations (area 30).

*N32, 2026-09-28: Machine-addressable documentation index v1, `nazm.docs-index/1` — VERIFIED;
Selective documentation retrieval v1, `nazm.docs-section/1` — VERIFIED; G78 — PARTIAL; G79 —
PARTIAL.* `nazm docs` and `nazm-mcp`'s `nazm.docs_index` and `nazm.docs_section`
(`crates/nazm-docs/`, `architecture.md` §7.34) section Nazm's own fourteen canonical documents,
each with the authority `master-architecture.md` §5 gives it, and return one section's own text
byte for byte by exact id, bound to a corpus state. The index is not a second source of
documentation truth; retrieval never paraphrases; a goal is never evidence and a plan never a
rule. G78 is partial: no semantic search, no documentation history, no rule ids below sections,
no documentation outside a source checkout. G79 is partial: one mechanical audit — no id, and so
no anchored rule, defined twice — and no whole-spec deduplication, rule graph or contradiction
detection. Evidence: `crates/nazm-docs/tests/corpus.rs` — ids from anchors, labels and heading
paths in that order, with two `Child` sections under two parents distinct; a parent's body its own
text and its children listed, a document equal to its sections concatenated; an anchored section
keeping its id reworded and moved, an unanchored one's old id not found; a removed section not
found and an added one moving no other id; a `#` line in a fence not a section and a Unicode
heading's id deterministic, stable under an ASCII anchor; every structural defect — a duplicate id,
anchor or label, a malformed or misplaced anchor, a setext or HTML heading, an unclosed fence, no
title, an empty identity, a broken link, a missing document — refused as unavailable; links
resolved to section ids, outside and external links kept as written, and a link to a reworded
heading refused; a grammar sectioned by production with exact text, examples and references, and
a broken one refused; the corpus state moving with one byte and with the manifest, only the edited
section's digest moving, a stale state refused, unknown ids and paths not found; the same state
and bytes from two directories, naming neither; and on the real corpus, every Markdown document
equal to its sections, `spec:generics` direct, the one fragment link resolved, each document's
authority, and every generated guide rule `generated_from` its production.
`crates/nazm-docs/src/markdown.rs` and `grammar.rs` — fences, anchors, setext and HTML refusal,
links outside code, locale-free slugs; production blocks. `crates/nazm-cli/tests/documentation.rs`
— one object on stdout and nothing on stderr for every answer and refusal, exit 0 and 1; a section
for a person exactly its body; one document's index equal to its part of the whole; two copies of
the repository giving the default's bytes; a rewording stale by state and still found by anchor; a
broken link and an empty root unavailable; every ambiguous flag set refused. `crates/nazm-cli/tests/schemas.rs`
— every status valid against both schemas. `crates/nazm-mcp/tests/protocol.rs` — nine tools, the
documentation tools taking ids and a state and never a path, file or root; answers equal to the
library's and carried once; `../README.md` and `/etc/passwd` not found, a path, a root, a query
and a missing state refused; a disk edit stale by state and read at the next call; a broken corpus
unavailable; and 1,001 calls through edits and repairs with flat resident memory. Seventeen N32
mutations (area 30).

**Limitation** · Diagnostics, definition, hover, references, rename, completion, signature
help, symbols, full-document semantic tokens, quick fixes and call hierarchy only: no
formatting, refactors, source actions or inlay hints, no cancellation and no watched files.
Call hierarchy is one level of direct checked calls per request within the compilation an item
was prepared in: no transitive or persistent call graph, no callers from files that compilation
does not load, and no item for an intrinsic or a function without source. Quick fixes
are the fixes the compiler attaches, for open documents at their current version; a fix whose
file is not open is not offered, and there is no post-apply rollback — the server never writes. Semantic tokens are
`semanticTokens/full` only — no range, no delta, no token cache — and cover identifiers the
checker resolved: no keywords, comments, strings, numbers or operators, which an editor
grammar colours, and nothing for a name the checker did not resolve. Workspace symbols cover the files the open
documents' compilations load: there is no manifest-defined complete project index, no
filesystem scan, and no persistent workspace symbol database, so an unloaded orphan file is
not searched, and a declaration known only from a persisted interface, with no source location,
is not a symbol. That is not a project boundary, and it does not lift the exported-rename
refusal below. Every change re-analyses every open document in full; no
incremental reparse or graph maintenance, and no persistent cache in the editor. A built-in, a
type parameter and anything in a body that did not parse have no definition and no references;
the core prelude has no file, so a definition in it has
no location and a references answer for one of its entities has no declaration. References
cover the programs being analysed — each open document's compilation — and not files on disk
that no open document loads. Rename covers locals and private definitions only: an exported
entity is refused, because a module no open program loads may use it, and a closed file is
never edited, because the protocol cannot make its content a precondition. Completion is of an
identifier in a value, call-head or type position only — no field, variant, label, pattern or
import-path completion, no auto-import, no ranking — and there is none where the source needed
syntax recovery. Signature help is for calls and `spawn`s whose callee
the checker resolved — not record or variant constructions, and none while any file of the
compilation needs syntax recovery (an unclosed argument list included); a built-in's parameters
are shown by type, having no names; no documentation. Structure completion offers no methods
(the language has none), no import paths, no auto-import and no ranking. At a hole — right after
a `.` or `(` with nothing written (N26) — it needs the probe's anchor: nothing at a `(` the
module calls as a function (a record of the same name included), after a generic enum or record
with no type arguments, after a record type, an unknown name, an enum value or a second `.`; no
completion after a comma or at an empty label slot later in a list; nothing in a function with
another syntax error, and nothing when any other file, or the document's top level, recovered.
`.` is the only trigger character; `(` is not one. A hole's completion costs a second parse and
check of the compilation per request (`performance.md`).
`N0204` (no `main`) is not reported, because an open file is analysed as a module. Diagnostics
are published for open documents only. **MCP is nine read-only tools** (N27–N32): the semantic
context packet, the semantic snapshot, the delta against a client-kept snapshot, a planned rename
or fix, the current diagnostics compactly or one in full, a compact summary of the root's
check — no build and no test run, which write files, over stdio, from disk, for one fixed root —
and Nazm's own documentation by section, by id, over one fixed repository — no resources, prompts,
completions, sampling or edits, no tool that applies a patch, no HTTP, no server-side history, and
no view of an editor's unsaved buffers. A patch covers rename and compiler-owned fixes only — no
exported rename, no multi-file edit, and no fix of a syntax diagnostic, whose compilation has no
snapshot to bind it to. A packet has no effects,
capabilities, test mapping or semantic history (each `unsupported`); a snapshot's digests are of the
sections the compiler records, so equal digests are not equal behaviour, and a delta has no rename
inference and no effect, capability or test delta. All three cover only the root's compilation — no
project-wide completeness — and are refused for a compilation that needed syntax recovery.

**Next dependency** · None for what exists. Each further request needs its own semantic
contract; incremental reparse needs latency evidence, which N16's measurements do not give.

**Accepted when** · Met for the LSP, 2026-09-25 (N16): an adapter shares the compiler's
semantic engine rather than re-implementing it. MCP: partial since N27 (2026-09-27) — read-only
tools over the same service, three since N28, four since N29, six since N30, seven since N31, nine since N32; not accepted, since they answer a
few of the questions the service can, and none edits.

### 26. Package and build tooling — PARTIAL

**Evidence** · Multi-file builds work through path-relative `use`, two independent
implementations: `crates/nazm-service/src/load.rs` and `compiler/module.nz`.

*N31, 2026-09-28: Compact Command Summary v1, `nazm.command-summary/1` — VERIFIED; Compact Test
Summary v1, `nazm.test-summary/1` — VERIFIED; G76 — PARTIAL.* `--summary-json` on `nazm check`
and `nazm build` (`crates/nazm-service/src/summary.rs`, `architecture.md` §7.33) prints the
command's own status — `success`, `rejected`, `unreadable`, `refused`, `toolchain_failed`, one per
exit path — its unchanged exit status, counts, and a reference to every diagnostic it reported:
N30's id where N30 indexes it, an explicit `command` reference where it does not (`N0204`, a
backend's `N0101`), code, place and fix counts, no prose. On `nazm test` it prints counts from each
case's own verdict and every case that did not pass, with how each leg ended, from what the runner
did. Summary is an additive view, not a replacement for existing detailed machine output; absence
of diagnostics does not imply command success. G76 is partial: no test impact, test-detail query,
remote logs or agent task cost. Evidence: `crates/nazm-cli/tests/summary.rs` — a clean check, type
errors, a syntax error and a missing `main` each summarized with the check's exit status, exactly
the diagnostics `--json` gives by code, and none of their prose; `N0204` kept as a command reference
with no id, the type error with N30's id, its place and state answered by `nazm diagnostics
--detail`, and the references in N30's canonical order; an unreadable root and a missing module
`unreadable` with their diagnostics; a build that writes its executable and names it as the build
does, builds rejected by the checker, for no `main` and by the backend with nothing written, and a
build refused with no diagnostic at all and not a success; every ambiguous flag pair refused, and
`--json` and the human rendering as they were; a summary the same from another directory; a test
run counting each verdict, listing only the four cases that failed — output mismatched, a runtime
error, a type error, a build the compiler refused — with each leg, and `not_run` when interpreted
only; no cases and an unreadable path refused as the command refuses them; 201 cases summarized by
counts and one failure, no passed name; and the MCP server's check summary equal to `nazm check
--summary-json` with and without its cache. *N31 closure:* every public N31 outcome variant has at
least one regression test that executes its production branch — a test driver that cannot be
started (`could_not_run`, and `could_not_build` beside it), a build that cannot be started after the
program ran (`could_not_build`, not `build_failed`), a build that reports success and writes no
program (`could_not_run`, not `mismatched`), a test run with nowhere to build (`toolchain_failed`),
and `nazm build` with a `clang` that refuses to compile, one that compiles and will not link, no
`clang` on PATH, and nowhere to write (`toolchain_failed`, no diagnostic, exit 4, not a success);
each beside the same run without `--summary-json`, with the same exit status. The runner's driver
is chosen through `NAZM_TEST_DRIVER`, a test seam; `clang` through the process's own PATH. Not
every way a process can fail to start is tested — one per branch. `crates/nazm-service/tests/summary.rs` — a reference
linked only while the file is exactly what the command read. `crates/nazm-cli/tests/schemas.rs`
every status valid against both schemas. `crates/nazm-mcp/tests/protocol.rs` — seven tools, the
summary tool accepting only `check` (not `build`, `test`, a root, arguments or a command), nothing
written, and 1,000 calls through edits, breaks and repairs with flat resident memory.
Fifteen N31 mutations, and seven in its closure (area 30).

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

**Evidence** · `xtask/mutations/mutations.toml` holds **575** catalogued defects (158 when
this sentence was first written, 233 at N12.2, 266 at N13, 288 at N14, 304 at N15, 318 at N16, 334 at N17, 348 at N18, 362 at N19, 376 at N20, 391 at N21, 404 at N22, 418 at N23, 435 at N24, 449 at N25, 457 at N26, 473 at N27 with its closure correction, 493 at N28, 509 at N29, 527 at N30, 542 at N31 before its closure correction, 549 after it, 568 at N32), **387** of them with a verified killer and all
of them owned by one of eleven focused profiles;
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

*N14 (2026-09-25) added twenty-two and repointed six,* each with its test and a killer
verified by `--verify-killer` (27 of 27 verified). They break the body rule (a requirement
ignored, never recorded, keyed by position across owners), what may be written (any name a
capability, a record's parameter allowed), the call (arguments never checked, only the first,
written or inferred ones skipped, a caller's parameter always satisfying), the interface (a
requirement not published, not read back, not validated, the schema or the epoch left
behind), the parser, and the Nazm-written parser and checker. **Campaign
`05f7bfb4d7869a2d`, targeted, one session, 1,025 s:** the 22 new, the 6 repointed, and 12
existing entries on the code N14 changed or beside it — among them N13's four capability-walk
and nominality mutants, because N14 changed that walk's leaf rule. **40 of 40 caught**, 36 at
tier 1 and 4 at tier 2; no survivor, nothing unusable or unapplied, no crash, no timeout; the
workspace suite never ran. N14 did not touch the native equality code, so N13's known
use-after-free survivor was not re-run and stays as it was: live, SURVIVED, no killer.

*N15 (2026-09-25) added sixteen and repointed three.* Of those nineteen entries, **seventeen
list a killer** — the sixteen new ones and the repointed `a-failed-body-drops-the-function` —
and each was verified by `--verify-killer` (17 of 17: pristine passes, mutant fails, restored
passes), first in a session a host build overlapped and then again uncontended, contained at
P1, with the same result. The other two repointed entries, `a-block-does-not-count-its-contents`
and `parentheses-count-as-a-node`, list no killer, as before N15; their tier-2 profile decides
them, and the campaign below caught both there. The seventeen killers are fourteen distinct
tests: `a_lossless_tree_preserves_every_byte_and_recovers_inside_a_function` kills three
entries and `the_abstract_tree_keeps_its_contract_for_a_body_that_recovered` two. They break the tree's promises — whitespace, comments, a refused character or the
trailing trivia missing from it, a leaf normalised or spelled from its kind, trivia inside
the construct before it, nodes at one position nested inside out, a token given the wrong
kind — and recovery's: abandoning the block as before N15, consuming nothing, swallowing
the next statement, leaving no error node, handing a recovered body to the checker,
reporting a cascade, forgetting the braces the failed statement opened. **Campaign
`f1f15052a1be61f0`, targeted, P1, one session, 1,015 s:** every entry on
`crates/nazm-syntax` — the 16 new and the 10 existing parser entries, the three repointed
among them, because N15 rewrote the code they target. **25 caught**, 19 at tier 1 and 6 at
tier 2; no survivor, nothing unusable or unapplied, no timeout; the workspace suite never
ran. **One MUTANT CRASH, `nesting-unbounded`**, the known open entry above, as it was: with
the nesting limit removed the parse stack overflows, as it did at N13 and N14. N15 did not
touch native equality, so N13's use-after-free survivor was not re-run and stays live,
SURVIVED, no killer.

*N16 (2026-09-25) added fourteen,* each with its regression test and a killer verified by
`--verify-killer`, uncontended, contained at P1: 13 in one session, and the fourteenth —
`n16-a-broken-buffer-keeps-the-last-clean-analysis` — after that session reported its first
killer **not a killer**. That was right: the edited document cannot keep an old analysis,
because a change replaces it with none, so the defect reaches only the *other* open
documents; the killer is now the test that asks an importer a question after its dependency
broke, and it verified. The fourteen break what an editor relies on — the disk winning over
a buffer, a closed buffer staying authoritative, an edit reaching only the edited document, a
stale analysis kept or a stale version answered, an empty set not published, a local found by
its spelling, an imported definition pointing at the caller, the prelude given a path, a
position past a name selected, a column counted in bytes either way, a log line on stdout,
completion advertised. **Campaign `7eabdfd883ebbae8`, targeted, P1, one session, 1,017 s:** the
fourteen and `an-absolute-path-reaches-the-module-key`, whose file moved to
`crates/nazm-service/src/load.rs`. **15 of 15 caught**, 14 at tier 1 and that one at tier 3; no
survivor, crash, timeout or unusable entry. Tier 3 was a finding: the moved loader had no
focused profile, where in `nazm-cli` it had been the `cli` profile's. The profile now owns
`crates/nazm-service/src/` and runs the service's suites, and that one entry, re-run alone,
was caught at tier 2 by `nazm-cli --test source_root` — the suite that caught it before N16.
N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are
as they were.

*N17 (2026-09-25) added sixteen and repointed one,* each killer verified by `--verify-killer`,
uncontended, contained at P1, one at a time. The first killer tried for
`n17-an-old-version-answers-references` was reported **not a killer** — its version-1 query
lands in a comment of the version-2 text, which answers nothing either way — and the thousand-
edit test, which asks about version 999 at a name version 1000 still has, verified instead.
The sixteen attack the index's invariants: a slot recorded without its function, a use given
its function's first slot, and same-named functions, fields and variants merged by spelling; an
applied record's field not taken to its definition; a cross-file use dropped; a declaration
listed as a use; uses left in hash order; the checker not recording a construction label's
field; only the asking compilation searched; an imported use placed in the asking file; one use
listed once per compilation; an old version answering; the declaration always, and never,
included. `n16-a-local-is-found-by-its-spelling` was repointed at the index, where a local's
declaration now comes from. **Campaign `a2fedab55b657d80`, targeted, P1, one session, 934 s:**
those seventeen and two unchanged N16 entries on code references now share
(`n16-an-imported-definition-points-at-the-caller`, `n16-a-stale-version-is-answered`). **19 of
19 caught at tier 1**; no survivor, crash, timeout, unusable or uninjected entry, no runner
error. The `references` suite joined the `cli`, `semantics` and `checker` profiles. N13's
use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as
they were.

*N18 (2026-09-25) added fourteen,* each killer verified by `--verify-killer`, contained at P1,
one at a time. `n18-a-plan-ignores-its-text`'s first killer was reported **not a killer**: the
forged text also moved the edited bytes, so the per-edit check refused the plan first; the test
now forges a text that differs only outside the edits, and that killer and
`n18-a-plan-outlives-its-generation`'s, which shares the test, were verified again against it.
`n18-a-written-type-is-not-recorded`'s first verification overlapped a small host build and was
repeated alone. The fourteen attack N18's invariants: a written type and an enum qualifier not
recorded; a rename found by its spelling; an exported entity renamed; the candidate not checked;
its meaning not compared; the comparison made at unmoved offsets; a program with errors renamed;
any name accepted; a plan outliving its generation or ignoring its text; a closed file edited
without a version; an edit losing its version; an unversioned client sent an edit. **Campaign
`0bc0e71f921e7a6e`, targeted, P1, one session, 873 s:** the fourteen and four N17 identity
entries on index code N18 extended (a local's slot, same-named fields and variants, an applied
record's field). **18 of 18 caught at tier 1**; no survivor, crash, timeout, unusable or
uninjected entry, no runner error. The `rename` suite joined the `cli`, `semantics` and
`checker` profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were
not re-run and are as they were.

*N19 (2026-09-25) added fourteen, repointed one and renamed one,* on the frozen final source.
Every killer was verified by `--verify-killer`, contained at P1, one at a time, with no host
build running — the fourteen, and the two N16 entries whose killer,
`the_server_speaks_the_protocol_end_to_end`, N19 edited: 16 of 16, none rejected, and no source
changed after. The fourteen attack N19's invariants: a `let` visible in its own initializer, a
later local visible early, a shadowed binding offered, a closed scope left open, a refused
duplicate offered, locals in walk order, a type parameter leaking out of its definition, a
refused import offered, a type offered as a value, a function offered as a type, the prefix
ignored, a completion inserting rather than replacing, and a recovered function or file offered
names. `imports-become-transitive` (N2) was repointed — its pattern gained the line that starts
the environment's refused-name list — and `n16-completion-is-advertised` became
`n16-signature-help-is-advertised`, the same defect with a capability the server still lacks.
**Campaign `d2e4e1af3eba6c1f`, targeted, P1, one session, 994 s:** those seventeen. **17 of 17
caught — 16 at tier 1, and `imports-become-transitive`, which names no killer, at tier 2 by
`nazm-cli --test visibility`**; no survivor, crash, timeout, unusable or uninjected entry, no
runner error. The `completion` suite joined the `cli`, `semantics` and `checker` profiles.
N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are
as they were.

*N20 (2026-09-26) added fourteen and renamed one,* on the frozen final source. Every killer was
verified by `--verify-killer`, contained at P1, one at a time, with no host build running — the
fourteen, and the two N16 entries whose killer, `the_server_speaks_the_protocol_end_to_end`,
N20 edited: 16 of 16. A fifteenth candidate — the argument list's upper bound moved from the
`)`'s start to its end — was **not a killer's mutant at all**: `--verify-killer` found the
mutant passing, because the walk down the tree enters only a node strictly containing the
position, so either guard alone keeps the byte after `)` out of the call. It is recorded as a
comment in the catalogue, not catalogued; the behaviour stays pinned by a test case. The
fourteen attack N20's invariants: a rejected call not recorded, written type arguments recorded
as inferred, inferred ones discarded, instantiated parameters recorded as declared, the outer
call chosen, every argument the first, a nested comma or a type argument's comma counted, a
position before `(` in the call, a recovered compilation answering, no parameter reported as
a parameter, hover and signature help disagreeing, a requirement hidden, and an absent active
parameter sent past the end. `n16-signature-help-is-advertised` became
`n16-document-symbols-are-advertised`, the same defect with a capability the server still
lacks. **Campaign `ea2f47200328923f`, targeted, P1, one session, 1,003 s:** those sixteen, and
seven N16 and N19 entries on the code signature help shares — a stale version answered, a broken
buffer keeping the last clean analysis, an edit reaching only its own document, the disk winning
over a buffer, a closed buffer staying authoritative, an incoming column counted in bytes, a
recovered file answering. **23 of 23 caught at tier 1**; no survivor, crash, timeout, unusable
or uninjected entry, no runner error. The `signature` suite joined the `cli`, `semantics` and
`checker` profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were
not re-run and are as they were.

*N21 (2026-09-26) added fifteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 15 of 15. They
attack N21's invariants: an unknown field or variant not recorded, a member offered any
record's fields, an applied generic's fields unsubstituted, a variant offered any enum's, a
payload label offered another variant's, a label given elsewhere offered, the label being
edited counted as given, declaration order shown, the active field counted by commas, the
outer construction chosen, a construction inside an argument answered as the call, a
recovery ignored, a recovery in the site's own function answered, and an unmarked constructor
marking the first parameter. **Campaign `443aa47f6a8821cd`, targeted, P1:** those fifteen, and
nineteen N16, N17, N19 and N20 entries on code N21 edited or shares. **34 of 34 caught at tier
1**; no survivor, crash, timeout, unusable or uninjected entry, no runner error. It took three
sessions: the first ran out of its budget with 20 finalized while the host's load average was
near 70 from other processes; the second refused to start because its baseline suite had 2
failing tests, under the same load — the harness counts them and does not name them, and the
same suite then passed contained at P1 on the frozen tree, 1,461 of 1,461, so they were not
identified; the third resumed and finished the other 14. The `structure` suite joined the `cli`,
`semantics` and `checker` profiles. N13's use-after-free survivor and the `nesting-unbounded`
MUTANT CRASH were not re-run and are as they were.

*N22 (2026-09-26) added thirteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 13 of 13. They
attack N22's invariants: an outline listing imported declarations, record fields flattened
beside their record, a payload field identified as another variant's, a selection that is the
whole declaration, an outline in name order, workspace symbols merged by spelling or across
files at the same offsets, private declarations hidden from workspace search, a stale outline
served, a payload field's container missing its enum, workspace order ignoring the name, and
the protocol's selection range and workspace location set to the whole declaration. One
killer was first written against the ordering test that queries a single name, where the
mutant is invisible: it was verified NOT A KILLER, moved to the query test whose names differ,
and verified there. `n16-document-symbols-are-advertised` became
`n16-code-actions-are-advertised`, the same defect with a capability the server still lacks; it
and `n16-the-server-writes-to-stdout`, whose killer test changed, were re-verified: 2 of 2.
**Campaign `e54921e1bd294310`, targeted, P1, one session, 1,107 s:** those fifteen, and nine N16
and N17 entries on code N22 reads — the disk winning over a buffer, a closed buffer staying
authoritative, a stale version answered, the prelude given a path, an imported definition
placed in the asking file, an outgoing column counted in bytes, same-named fields and variants
indexed as one, and one use listed once per compilation. **24 of 24 caught at tier 1**; nothing
at tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry, no runner error. The
`symbols` suite joined the `cli`, `semantics` and `checker` profiles. N13's use-after-free
survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as they were.

*N23 (2026-09-26) added fourteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 14 of 14. They
attack N23's invariants: a parameter classified as a local, declarations never marked, every
occurrence marked a declaration, a payload label classified as a local, an unresolved name
classified, an identifier emitted twice, a stale classification served, the checker recording
no built-in type or no type-parameter use, a refused signature keeping its type parameters, an
intrinsic sent without `defaultLibrary`, overlapping identifiers encoded, a start column sent
absolute on the previous token's line, and range tokens advertised.
`n16-the-server-writes-to-stdout` and `n16-code-actions-are-advertised`, whose killer test
changed again, were re-verified: 2 of 2. **Campaign `45707a4b609ee4d4`, targeted, P1, one session,
1,219 s:** those sixteen, and ten N16, N17 and N18 entries on what N23 reads — a stale version
answered, the disk winning over a buffer, a closed buffer staying authoritative, an outgoing
column counted in bytes, same-named functions, fields and variants indexed as one, a local taken
as its function's first slot, and a written type or an enum qualifier not recorded. **26 of 26
caught at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or uninjected
entry, no runner error. The `semantic` suite joined the `cli`, `semantics` and `checker`
profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run
and are as they were.

*N24 (2026-09-26) added seventeen and repointed two,* on the frozen final source; every
affected killer was verified by `--verify-killer`, contained at P1, one at a time, with no host
build running: 20 of 20. They attack N24's invariants: the requested range ignored, a stale
version planned, a plan of an older generation, one with no version or with the wrong bytes
taken as current, the diagnostic's span edited instead of the fix's, a needs-review fix or one
of several alternatives preferred, a precondition hidden, an unversioned client sent a fix, and
requested kinds ignored — and `nazm fix`'s: a needs-review fix applied, an overlap applied, a
span inside a character edited, changed bytes overwritten, an unparsable result written, and a
failed write claimed. One killer was first verified NOT A KILLER: the plan with no version was
refused by the shared file check for an open document, and the guard matters only for a file
not open whose disk text matches; the test was extended to that case and every killer was
verified again on the final source. `n18-a-plan-ignores-its-text` was repointed at the line
that moved into `file_is_current`, `n16-code-actions-are-advertised` became
`n16-call-hierarchy-is-advertised`, and `n16-the-server-writes-to-stdout`, whose killer test
changed, was re-verified. **Campaign `abbba6ae23198600`, targeted, P1, one session, 1,010 s:**
those twenty, and nine N16 and N18 entries on what N24 shares — the versioned edit's version,
its closed-file and unversioned-client refusals, a rename plan outliving its generation, a stale
version answered, the disk winning over a buffer, a closed buffer staying authoritative, an
outgoing column counted in bytes, and an empty diagnostic set not published. **29 of 29 caught
at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry,
no runner error. The `fixes` suite joined the `cli`, `semantics` and `checker` profiles. N13's
use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as
they were.

*N25 (2026-09-26) added fourteen and repointed two,* on the frozen final source; every killer
was verified by `--verify-killer`, contained at P1, one at a time, with no host build running:
16 of 16, each on its first run. They attack N25's invariants: an item found by spelling, a
reference taken for a call, a recursive call dropped, a call given the wrong caller, a second
call lost, same-named callers merged, another root's compilation answering, an old item
answered after a change, a closed root's item rebound, an item's range its name, a call's range
the whole call, edges ordered ignoring the file, a stale version prepared, and every file's
ranges converted with one line index. `n22-a-selection-is-the-whole-declaration` moved with the
function symbol into the helper the outline and the hierarchy share;
`n16-code-actions-are-advertised`, then `n16-call-hierarchy-is-advertised`, became
`n16-formatting-is-advertised`, the same defect with a capability the server still lacks. **Campaign
`a52490106a612553`, targeted, P1, one session, 1,202 s:** those sixteen, and eight N16, N17,
N20 and N22 entries on what N25 consumes — both column conversions counted in bytes, same-named
functions indexed as one, a call with the wrong argument count not recorded, a write to stdout,
a stale version answered, the position just after a name, and the protocol's selection range.
**24 of 24 caught at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or
uninjected entry, no runner error. The `hierarchy` suite joined the `cli`, `semantics` and
`checker` profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were
not re-run and are as they were.

*N26 (2026-09-27) added eight and repointed two;* every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 10 of 10, each on
its first run — and, after the final source moved the parser's hole code out of line, the four in
the parser verified again: 4 of 4. They attack N26's invariants: a `.` before the cursor taken
for the hole, the hole forgiving every later statement, a bare `(` read as a construction, a
pattern's hole requiring an arm, an ambiguous import anchoring, the probe ignoring other
recoveries, a hole's completion replacing the `.`, and `(` advertised as a trigger. A ninth — a
generic record with no type arguments anchoring — was written, survived, and showed its guard
redundant with the checker's own application of type arguments; the guard and the entry were
removed rather than kept as an equivalent mutant. `n21-a-label-given-elsewhere-is-offered` and
`n21-a-recovery-is-ignored` moved with the code N21 and N26 now share. **Campaign
`c9b1509745e3e043`, targeted, P1, one session, 1,539 s, on the final source:** those ten, and
eighteen N15, N16, N17, N19 and N21 entries on what N26 consumes — a recovered body reaching the
checker, a malformed statement with no error node, a stale version answered, both column
conversions in bytes, a write to stdout, the formatting claim, same-named fields and variants as
one, a completion inserting rather than replacing, an unknown field or variant not recorded, a
member or variant from any definition, generic fields unsubstituted, another variant's label,
declaration order shown, and N21's two recovery rules. **28 of 28 caught at tier 1**; nothing at
tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry, no runner error. (The same
28 on the source before the parser change: campaign `40d19aba85e2276e`, 28 of 28 at tier 1.) The
`incomplete` suite joined the `cli`, `semantics`, `checker` and `syntax` profiles, and `holes` the
`syntax` profile. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not
re-run and are as they were.

*N27 (2026-09-27) added thirteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 13 of 13, the two in
`nazm-mcp` against the real server inside the container. They attack N27's invariants: a target
found by spelling, a local reported as a dependency, dependencies grouped by name, the declaration
listed as a reference, effects claimed, an absolute path serialised, a type given empty callers, the
source not the declaration, a file outside the compilation answered, a recovered compilation
answered, every diagnostic put in every packet, the server printing to stdout, and resources
advertised. **Campaign `fa7cfe95c8339d2e`, targeted, P1, one session, 1,700 s:** those thirteen, and
eleven N17, N18, N22 and N25 entries on what a packet consumes — same-named functions or fields
indexed as one, a cross-file use dropped, a declaration taken for a use, uses in map order, a written
type not recorded, the selection range, another root answering, the wrong caller, references taken
for calls, and edge order ignoring the file. **24 of 24 caught at tier 1**; nothing at tier 2 or 3,
no survivor, crash, timeout, unusable or uninjected entry, no runner error. The `context` suite
joined the `cli`, `semantics` and `checker` profiles, and `crates/nazm-mcp/src/` the `cli` profile
with its binary and `protocol` suite. N13's use-after-free survivor and the `nesting-unbounded`
MUTANT CRASH were not re-run and are as they were.

*N28 (2026-09-27) added twenty,* on the frozen final source, and moved five whose code N28 now
shares — two N25 entries onto the one checked-call rule `hierarchy::callee`, two N27 entries onto
`context::incomplete`, and N27's duplicate-payload entry onto the MCP server's one `once` path —
each re-pointed at the same defect. Every killer of the twenty, and of the 29 existing entries in
the files N28 changed or whose killing suite it changed, was verified by `--verify-killer`,
contained at P1, one at a time, with no host build running: **49 of 49**. The twenty attack N28's
invariants: definitions matched by spelling, the source digest ignored, a source-only change
counted unchanged, a rename inferred, caller and callee changes omitted, references ignored,
effects no longer declared unsupported, the prelude's definitions taken for the root's, the
directory scanned, an absolute path as root identity, the service generation digested, definitions
in declaration order, a recovered compilation snapshotted, a wrong-root, another-schema,
unknown-field or duplicate-identity baseline accepted, the MCP server keeping a snapshot, and a
snapshot sent twice. **Campaign `d7286651a2e99c5c`, targeted, P1, one session, 1,740 s:** those
twenty and the five re-pointed entries. **25 of 25 caught at tier 1**; nothing at tier 2 or 3, no
survivor, crash, timeout, unusable or uninjected entry, no runner error. The `snapshot` suite joined
the `cli`, `semantics` and `checker` profiles. N13's use-after-free survivor and the
`nesting-unbounded` MUTANT CRASH were not re-run and are as they were.

*N29 (2026-09-28) added sixteen,* on the frozen final source. Every killer of the sixteen, of
the nine N18 entries in `rename.rs` (which N29 touched to share its `Edit` and its entity lookup),
and of the five N27 and N28 entries in the MCP server N29 extended, was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: **30 of 30**. The
sixteen attack N29's invariants: the snapshot binding, a file's digest or each edit's expected
bytes of nothing, a whole file emitted as one edit, a duplicate, an overlap, or edits left in the
order they came, needs-review planned as automatic, the precondition dropped, a fix taken from
another position or an index that wraps, an unloaded file planned against the root's text, a
local given its function's key, the MCP server applying the patch, sending it twice, or keeping
the first plan. **Campaign `e0bc0b9125753c45`, targeted, P1, one session, 1,830 s:** those sixteen,
six N18 entries N29 consumes (a rename by spelling, an exported entity renamed, the candidate not
checked, its meaning not compared, any name accepted, a program with errors renamed), two N24
entries (the requested range ignored, the diagnostic's span edited) and three N28 entries on the
snapshot a patch binds to (an absolute path, the service generation, a recovered compilation).
**27 of 27 caught at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or
uninjected entry, no runner error. The `patch` suite joined the `cli`, `semantics` and `checker`
profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and
are as they were.

*N30 (2026-09-28) added eighteen,* on the frozen final source. Every killer of the eighteen, and
of the eight N27, N28 and N29 entries in the MCP server N30 extended, was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: **26 of 26**. The
eighteen attack N30's invariants: the index carrying the message, an id hashing it, an id of
only its code or only an ordinal, a stale state answered, a state of the root file only or over
absolute paths, concatenation offsets given as file offsets, an owner taken from a recovery or
from the nearest declaration, syntax errors given no index, compiler fixes counted as patches, a
syntax fix offered as a patch, needs-review counted automatic, a detail dropping the help, the
directory scanned, and the MCP server sending an index twice or keeping the first. **Campaign
`b488c311e0ac14ee`, targeted, P1, one session, 1,794 s:** those eighteen, two N24 entries (the
requested range ignored, the diagnostic's span edited), three N29 diagnostic-fix entries (a fix
taken from anywhere, an index that wraps, needs-review planned automatic) and two N28 entries (a
recovered compilation snapshotted, an absolute path). **25 of 25 caught at tier 1**; nothing at
tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry, no runner error. The
`diagnostics` suite joined the `cli`, `semantics` and `checker` profiles. N13's use-after-free
survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as they were.

*N31 (2026-09-28) added fifteen,* on the frozen final source. Every killer of the fifteen, of the
sixteen N30 entries in `crates/nazm-service/src/diagnostics.rs` (whose canonical order N31 now
shares), and of the ten N27–N30 entries in the MCP server N31 extended, was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: **41 of 41**. The
fifteen attack N31's invariants: every case counted passed, files without an expectation dropped,
passed cases listed, only the first failure listed, a build failure called a mismatch, a leg not
run called matched, no diagnostics taken for success, a command-only diagnostic dropped or given an
id, references in the compiler's order, references linked to changed sources, the message carried,
a summary exiting zero, and the MCP server sending a summary twice or keeping the first. **Campaign
`ef13a76e70082ddb`, targeted, P1, two resumed sessions:** those fifteen, four N30 entries on the ids
and places N31 reuses, the two existing entries in the check and build drivers N31 changed (neither
with a declared killer), and two MCP safety entries. **23 of 23 caught — 21 at tier 1, 1 at tier 2
(`the-object-key-approximates-the-whole-program`, by `nazm-cli --test schemas`), 1 at tier 3
(`the-prelude-is-left-out-of-every-check-key`, by the workspace suite)**; no survivor, crash, timeout,
unusable or uninjected entry, no runner error. The first session reached its deadline after the
tier-3 entry and the campaign was resumed from its journal, which reran nothing already decided.
The `summary` suites joined the profiles. N13's use-after-free survivor and the `nesting-unbounded`
MUTANT CRASH were not re-run and are as they were.

*N31's closure correction (2026-09-28) added seven,* one for each outcome it newly exercises: a build
that could not be started called `build_failed`, a program that could not be started called
`mismatched`, the test driver seam ignored, a test run with nowhere to build called `failed`, and a
failed link, a refused compile and an unwritable output summarized as a success or a rejection.
On the final source every killer of the twenty-two N31 entries was verified by `--verify-killer`,
contained at P1, one at a time, with no host build running: **24 of 24** (two entries have two
killers each). **Campaign `d56b0589bbcf96b2`, targeted, P1, one session, 984 s:** the seven, the six
earlier entries in the test runner the seam touched, and no-diagnostics-is-success. **14 of 14
caught at tier 1**; no survivor, crash, timeout, unusable or uninjected entry, no runner error.

*N32 (2026-09-29) added seventeen, and N32-H two for the harness,* on the frozen final source;
one N27 entry was re-pointed at the line its rule moved to. The seventeen attack N32's laws: an
anchor ignored, a line number as an id, a duplicate id accepted, an unknown id answered with the
nearest, fences never opened, a parent carrying its children, the goals classified as evidence
and the roadmap as normative, a production's text normalised, a broken link ignored, a corpus
state without the text or with the checkout's absolute path, a section digest without the body,
a stale state answered, the MCP section tool taking any argument, sending a section twice, or
keeping the first. The two guard the warm session: a killer that passed on its mutant counted
verified, and a live supervisor's container reaped. **One warm session, campaign
`81c571e1e7d95767`, targeted, P1, 1,469 s:** those nineteen and the twelve MCP entries of
N27–N31 in the server N32 extended — **31 of 31 caught at tier 1**; no survivor, crash, timeout,
unusable or uninjected entry, no runner error — and, around each mutant's one injection, **every
declared killer verified, 34 of 34**: pristine passes, mutant fails, restored passes. The
verifications an earlier harness had finished for N32 before it was replaced are not counted.
A `docs` profile and a `harness` profile joined the nine, and the `documentation` suite the `cli`
profile. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run
and are as they were.

*N32-H2 (2026-09-29) added seven harness mutations,* on the frozen final source: a pristine
killer result found for another tree, another killer's run read as this one's, Tier 1 alone
accepting an inexact killer, the cache key without the lockfile, a harness change or an unknown
path given the narrow lifecycle, and a mutation session sharing the checkout's target. **One warm
session, campaign `7ee57d2af9d3bb87`, targeted, P1, 767 s:** those seven, N32's seventeen, N32-H's
two and the twelve MCP entries — **38 of 38 caught at tier 1**, no survivor, crash, timeout,
unusable or uninjected entry, no runner error — and **every declared killer verified, 41 of 41**,
each killer's mutant observation the run its verdict came from. The workspace suite was not run as
the session's baseline: no mutant needed a tier beyond its killers.

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

**Since N14** the compiler written in Nazm parses `T: Equality`, records it on the parameter's
entry, reads it in the one place equality is derived, and checks every call's type arguments
against it at the reference's spans. The corpus has 32 cases, 13 of them multi-module (one
calls and forwards to an imported bounded function, including a `count_same[T: Equality]`
over a `Vec[T]`), and 25 refusals, six of them requirements'. No compiler code uses a
requirement: the audit found none that needs one. The fixpoint moved to `0e1a40e6…` at 12,543
source lines, runnable executables byte-identical too.

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
