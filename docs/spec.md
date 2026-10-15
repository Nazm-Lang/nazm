# Nazm language specification

**Status: the 1.0 draft of the Nazm language, the executable language of the toolchain 0.10.0
public preview, and the draft of the 1.x line's stable contract** — in force from 1.0, and until
then a target a 0.x release may still change, saying so in its release notes. It is normative for
what it settles:
a program's meaning under 1.0 is what the sections classified STABLE 1.0 say, and where the
reference implementation disagrees with one, that is a defect in one of them, not a second
meaning. What 1.x promises across releases, and what may change, is in
[`stability.md`](stability.md); every section of this document is classified below.

The document holds four kinds of text, and every section says which it is:

| Kind | Where | Normative for 1.0? |
| --- | --- | --- |
| **Current language** | *Settled for v0.1*, *Settled for v0.2*, and the sections after them up to *Open* | yes, as classified below — a later section or a marked correction governs an earlier one |
| **Historical** | an entry or paragraph marked *Superseded*, *Corrected* or *Achieved*, kept with its original wording | no — it records what an earlier version decided and why |
| **Design commitments** | *Settled*, directly below | they bind future design; where one names a mechanism 1.0 does not have, the commitment constrains how it is added, and 1.0 refuses the construct |
| **Open / research** | *Open — later phases must settle these*, and *Non-goals* | no — undecided, or deliberately out of scope |

Sections are versioned and kept in order, so a *Settled for v0.1* entry records what v0.1
decided; where a later version changed one, the earlier entry says so and points forward. The
"v0.1", "v0.2" and "v0.3" in headings and corrections date a decision; they do not limit it —
a section classified STABLE 1.0 is current whatever version first settled it. What 1.0 does not
have is listed under *Not in Nazm 1.0*, and a program using any of it is refused by name.
Ecosystem and domain support — targets, profiles, tools — is evidence rather than semantics, and
lives in `capability-matrix.md` and `limitations.md`.

**Licence.** This specification is licensed under the Creative Commons Attribution 4.0
International licence (SPDX: CC-BY-4.0; [`LICENSE-CC-BY-4.0`](../LICENSE-CC-BY-4.0)): you may
copy, translate, adapt and republish it, including commercially, if you credit *the Nazm language
specification, © 2026 Ferdous Alam*, link the licence and say what you changed. As part of the
repository it is also available under the project's licence, `Apache-2.0 WITH LLVM-exception`, so
its code fragments may be used as code ([`licensing.md`](licensing.md)).

## Stability classification for 1.0
<!-- id: classification -->

Every section with an anchor, by class (Gate 1, 2026-10-07). **STABLE 1.0** is covered by the
1.x promise (`stability.md`); a 1.x addition takes it when its milestone closes, as R1's did at
0.10.0. **COMPATIBLE EXTENSION POINT** is stable as written and names where
1.x may add without changing a valid program's meaning. **EXPERIMENTAL** may change in a 1.x
minor release, announced. **RESEARCH/OPEN** is undecided. **NON-GOAL** is out of scope by
decision. **HISTORICAL** records an earlier decision and governs nothing. A subsection without an
anchor takes its section's class, except where its own heading or a marked correction says it is
history (*What records are not, in N9* and its siblings are HISTORICAL). No current rule rests on
historical wording: where a section's own sentence was superseded, the class names what governs.
`crates/nazm-cli/tests/docs.rs` fails if an anchored section is missing here.

| Section | Class | Notes |
| --- | --- | --- |
| `settled` | STABLE 1.0 | design commitments; items 3 (parameter conventions beyond `let`, a region calculus) and 4–5 beyond what is realised are COMPATIBLE EXTENSION POINTS |
| `v0-1` | STABLE 1.0 | heading of the sections below |
| `overflow` | STABLE 1.0 | the explicit wrapping it promised is `wrapping_add` and its kin since Gate 2 (`numbers`); R1b's `checked_add`, `checked_sub` and `checked_mul` are stable since R1 closed (0.10.0) |
| `division` | STABLE 1.0 | |
| `shadowing` | STABLE 1.0 | |
| `evaluation-order` | STABLE 1.0 | |
| `returns` | HISTORICAL | superseded by `return` and `blocks`: a function's value is its final expression, and `return` leaves early |
| `types` | STABLE 1.0 | the built-in types grew later; no inference across function boundaries stands |
| `records` | STABLE 1.0 | *What records are not, in N9* is HISTORICAL |
| `enums` | STABLE 1.0 | *What enums are not, in N10* is HISTORICAL |
| `generics` | STABLE 1.0 | *What generics are not, in N11* is HISTORICAL; higher-order generics would be an extension |
| `errors` | STABLE 1.0 | `?` on `Result` only |
| `std` | STABLE 1.0 | the API is `library/std/API-1.0` |
| `equality` | STABLE 1.0 | |
| `equality-requirement` | STABLE 1.0 | traits widened bounds (`traits`) |
| `v0-2` | STABLE 1.0 | heading of the sections below |
| `traits` | STABLE 1.0 | nominal and static; trait objects are not in 1.0; R1b's default bodies and impls of generic types (L11) are stable since R1 closed (0.10.0) |
| `mutation` | STABLE 1.0 | |
| `while` | STABLE 1.0 | |
| `blocks` | STABLE 1.0 | |
| `line-breaks` | STABLE 1.0 | R1, 5b.11 and 5b.19: `else if`, and a line break or a block's `}` for a statement's `;`; a 1.x addition, stable since R1 closed (0.10.0) |
| `short-variants` | STABLE 1.0 | R1, 5b.14: an enum construction infers its type arguments; `Some`, `None`, `Ok`, `Err`; a variant without `()`; a 1.x addition, stable since R1 closed (0.10.0) |
| `shorter-forms` | EXPERIMENTAL | R1.5's spellings (D1–D8 of its design); stable when R1.5 closes |
| `named-methods` | STABLE 1.0 | R1, 5b.15: a method no trait gives is a function by name; `xs[i]` and `xs[i] = v` on a sequence; a 1.x addition, stable since R1 closed (0.10.0) |
| `conveniences` | STABLE 1.0 | R1, 5b.16 and 5b.17: `let x: T`, inference from the wanted type, block comments, `loop`, `while let`, an impl without a trait; a 1.x addition, stable since R1 closed (0.10.0) |
| `no-result` | STABLE 1.0 | R1, 5b.2: a function with no result type; a 1.x addition, stable since R1 closed (0.10.0) |
| `main-ends` | STABLE 1.0 | R1, 5b.1 and 5b.3: `main` holds an `OutCap` unwritten; its result is the exit status; a 1.x addition, stable since R1 closed (0.10.0) |
| `break-continue` | STABLE 1.0 | |
| `return` | STABLE 1.0 | |
| `unreachable-code` | STABLE 1.0 | |
| `run-compiles` | EXPERIMENTAL | R1.5, D11: `nazm run` compiles with Cranelift, `--interpret` interprets; stable when R1.5 closes |
| `limits` | STABLE 1.0 | the rule is stable; the limits' values are the implementation's and may be raised, not lowered, in 1.x |
| `nesting` | STABLE 1.0 | the bound may be raised, not lowered, in 1.x |
| `native-recursion` | STABLE 1.0 | stack sizes are the implementation's |
| `strings` | STABLE 1.0 | |
| `string-escapes` | STABLE 1.0 | |
| `string-forms` | STABLE 1.0 | R1a: multi-line and raw strings, a 1.x addition, stable since R1 closed (0.10.0) |
| `interpolation` | STABLE 1.0 | R1a: `f"…"`, fixed-point floats and padding, a 1.x addition, stable since R1 closed (0.10.0) |
| `derived-serialisation` | STABLE 1.0 | R1c: `@derive(json)`, its codec and schema, a 1.x addition, stable since R1 closed (0.10.0) |
| `sequences` | STABLE 1.0 | its *Destruction* row is superseded by `memory`: storage is reclaimed |
| `memory` | STABLE 1.0 | *Cycles* covers closure environments since Gate 1-C1 (`N0616`) |
| `outside-world` | STABLE 1.0 | *Authority is explicit where a contract is written, and ambient elsewhere* is HISTORICAL: `capabilities` (N104) governs |
| `modules` | STABLE 1.0 | |
| `packages` | STABLE 1.0 | |
| `profiles` | STABLE 1.0 / EXPERIMENTAL | the mechanism — a profile only refuses, and never changes what an accepted program does — is stable; each profile's rule set is experimental and may grow |
| `durable-identity` | STABLE 1.0 / DEBUG | what a module path resolves to is stable; the persisted key format is internal (`nazm.interface/13`) |
| `concurrency` | STABLE 1.0 | the M:N scheduler is an implementation choice |
| `effects` | STABLE 1.0 | three effects (the section's own "exactly two" is corrected in place) |
| `capabilities` | STABLE 1.0 | revocation and finer-grained authority would be extensions |
| `foreign` | STABLE 1.0 | widened by `ffi-v2` and `ffi-v3` |
| `provenance` | STABLE 1.0 | no implicit-flow or non-interference claim |
| `integer-boundaries` | STABLE 1.0 | |
| `one-error` | EXPERIMENTAL | diagnostic recovery: codes are stable, which further errors are reported is not |
| `logical-operators` | STABLE 1.0 | |
| `qualified-names` | STABLE 1.0 | |
| `function-values` | STABLE 1.0 | its capability-capture row is superseded by `effect-parameters` (N51): a closure may capture a capability; R1b's rows (L10: a shared `let mut`, closures and function values in a generic function) are stable since R1 closed (0.10.0) |
| `effect-parameters` | STABLE 1.0 | more than one effect parameter would be an extension |
| `declassification` | STABLE 1.0 | |
| `generic-channels` | STABLE 1.0 | |
| `select-deadlines` | STABLE 1.0 | |
| `ffi-v2` | STABLE 1.0 | |
| `ffi-v3` | STABLE 1.0 | |
| `function-contracts` | STABLE 1.0 | what is proved at compile time may grow; a clause's run-time check is never removed |
| `registry` | STABLE 1.0 | local registries; no hosted registry is promised |
| `freestanding` | EXPERIMENTAL | emulator-verified boards, a restricted subset |
| `webassembly` | EXPERIMENTAL | |
| `wasi` | EXPERIMENTAL | Gate 3: `wasm32-wasip1`, run under Node's WASI only |
| `trace` | EXPERIMENTAL | Gate 3: `nazm trace`, `nazm.trace/1` |
| `mobile` | EXPERIMENTAL | Gate 3: Android run on a Linux kernel as static Bionic; iOS link-only |
| `interactive` | EXPERIMENTAL | `repl`, `comptime`, `reload-check` |
| `accel` | EXPERIMENTAL | |
| `contracts` | EXPERIMENTAL | the `web3` profile and `nazm contract` |
| `evm` | EXPERIMENTAL | |
| `wasm-contract` | EXPERIMENTAL | |
| `accounts` | EXPERIMENTAL | |
| `provenance-record` | STABLE 1.0 | `nazm.provenance/1` |
| `bytes` | STABLE 1.0 | |
| `text` | STABLE 1.0 | graphemes and normalisation are not the language's |
| `files` | STABLE 1.0 | Gate 2: a handle is closed by `os_close` or at exit; a status's number is the platform's, its kind (`@std/ioerror`) is not. R1d's line reads of standard input (P3: `os_read_line`, its buffer shared with `os_read`) are stable since R1 closed (0.10.0) |
| `time` | STABLE 1.0 | Gate 2: nanosecond clocks and a sleep that parks; no timers beyond a sleep in a task or a select's deadline |
| `process` | STABLE 1.0 | Gate 2: the environment, and a program run with three pipes; no signal handling |
| `randomness` | STABLE 1.0 | Gate 2: entropy behind `RandomCap`; the seeded generator is a library's |
| `attributes` | COMPATIBLE EXTENSION POINT | Gate 2: a closed vocabulary; 1.x may add an attribute, never change one |
| `conditional-compilation` | STABLE 1.0 | Gate 2: conditions read only the build's configuration |
| `tests-and-benchmarks` | STABLE 1.0 | Gate 2: the test and fuzz shapes and outcomes; `nazm bench`'s measurements are EXPERIMENTAL |
| `package-features` | STABLE 1.0 | Gate 2: additive features per package; `nazm.lock/2` |
| `windows-x86-64` | EXPERIMENTAL | Gate 3: run under Wine only; STABLE once a real Windows host has run it |
| `atomics` | STABLE 1.0 | Gate 3: sequentially consistent only; an ordering parameter would be an extension |
| `statics` | STABLE 1.0 | Gate 3: a number, a `Bool` or an atomic; read-only, or an atomic; module-private. A record, enum or array would be an extension |
| `boards` | EXPERIMENTAL | Gate 3: manifests, sections, interrupts, the arena and the failure hook, emulator-verified |
| `for` | STABLE 1.0 | R1a: a 1.x addition, stable since R1 closed (0.10.0); each form means its `while` loop |
| `box-list` | STABLE 1.0 | R1a: a 1.x addition, stable since R1 closed (0.10.0); recursive types through immutable indirection. R1.5 (D7) renames its sequence `Chain[T]`, with built-ins `chain_*`: a breaking change, listed for the next release (`N0637` names the new built-in) |
| `heap-limit` | STABLE 1.0 | R1d, D5: a 1.x addition, stable since R1 closed (0.10.0); `nazm build --heap-limit` and the manifest's `heap-limit` |
| `cell` | STABLE 1.0 | R1b: a 1.x addition, stable since R1 closed (0.10.0); a mutable handle of one value, the kind `Vec` is |
| `defer` | STABLE 1.0 | R1b: a 1.x addition, stable since R1 closed (0.10.0); `defer` is a word only at a statement's start before `{` |
| `patterns` | STABLE 1.0 | R1a: a 1.x addition, stable since R1 closed (0.10.0); supersedes the N10 limits on patterns where it says so |
| `tuples` | STABLE 1.0 | R1a: a 1.x addition, stable since R1 closed (0.10.0); a tuple is an anonymous record |
| `const-tables` | STABLE 1.0 | R1a: a 1.x addition, stable since R1 closed (0.10.0); widens *Constants* and *Arrays* |
| `platform-r1d` | STABLE 1.0 | R1d: standard output and errors as handles, a child's directory and environment, process groups and bounded waits, symbolic links, permissions, canonical paths, temporary directories and file locks — 1.x additions, a 1.x addition, stable since R1 closed (0.10.0); widens `files` and `process` |
| `order-hash` | STABLE 1.0 | R1a: a 1.x addition, stable since R1 closed (0.10.0); widens *Equality is derived*'s "not ordering, not hashing" |
| `networking` | STABLE 1.0 | Gate 2: TCP and UDP over literal addresses, a deadline per handle, waits that park on the pool; no TLS, no Unix-domain sockets |
| `arrays` | STABLE 1.0 | plain-data elements and the 64 KiB bound are the rule; a wider element class or bound would be an extension |
| `constants` | STABLE 1.0 | module-private; exporting a constant would be an extension |
| `numbers` | STABLE 1.0 | Gate 2: fixed-width integers, `Float32`, `Float64`, bit operators and conversions; the C library's transcendental functions are promised to the host library's accuracy, not bit for bit |
| `project-tooling` | STABLE 1.0 / EXPERIMENTAL | `init`, `doc`, library checking and `publish --dry-run` are stable (what a template writes is not); `bindgen` is experimental |
| `not-in-v0-3` | STABLE 1.0 | each construct is refused by name; adding one is a compatible extension, and Gate 2 added floating point and integer widths |
| `classification` | STABLE 1.0 | this table |
| `open` and every `open-*` | RESEARCH/OPEN | |
| `non-goals` | HISTORICAL / NON-GOAL | the first interpreter milestone's list; what 1.0 leaves out is `not-in-v0-3` |

*History.* Until Gate 1 (2026-10-07) the status paragraph named language v0.3 and toolchain
0.3.0 and pointed to a pre-1.0 policy. Until R1 (2026-10-07) it read: "this is not yet a specification — it
is the list of decisions a specification has to make, with the ones already settled marked as
such." Publishing the open questions before the answers was deliberate: every one of them
was either wrong or unstated in an earlier draft of the design, and the cost of discovering
that after 150k lines of compiler is the entire reason this document exists first. The open
questions are still here; what changed is that the settled part now describes a released
language.

---

## Settled
<!-- id: settled -->

These are design commitments. Changing one is a re-founding decision, not an edit.

*Status for 1.0:* commitments, not a list of features. Items 1, 2, 6 and 7 hold of 1.0 as
implemented. Item 3's parameter conventions beyond `let` are not in 1.0 — a mutable
parameter is refused (*Not in Nazm 1.0*) — and no region calculus is exposed; item 4 is realised
as declared and inferred effect sets with one effect parameter per function (*Effects*,
*Effect parameters*); item 5 is realised as provenance with one restricted sink
(*Provenance*). Where 1.0 has less, the commitment says how more may be added, not that it
exists.

1. **Source of truth is plain UTF-8 `.nz` files in git.** Content addressing applies to
   derived artifacts only. (`architecture.md` §3)
2. **Field access is a symbolic projection through Mono MIR.** Byte offsets exist only at
   LIR. Without this, automatic layout selection is unrecoverable. (`architecture.md` §2)
3. **No lifetime syntax in the surface language.** Mutable value semantics with parameter
   conventions `let` / `inout` / `sink` / `set`, checked intraprocedurally. The IR carries
   a full region calculus anyway, as a hedge. (`architecture.md` §5)
4. **Effect rows are inferred and polymorphic.** Colouring is prevented by polymorphism
   over the effect variable, not by hiding the distinction.
5. **Taint is position-sensitive.** Untrusted data in a bound-parameter position is legal;
   in a query-construction position it is an error.
6. **The structured-concurrency borrow region is `[spawn point, scope end]`**, with parent
   and sibling access rejected throughout — not merely at the join.
7. **Purity is a prerequisite, not a licence.** An empty effect row does not by itself
   authorise dropping, reordering, memoizing, or parallelising a call.

## Settled for v0.1 — the executable subset
<!-- id: v0-1 -->

These are the decisions the first interpreter needs and no more. Each is chosen for the
same reason: **a reader should be able to predict what a line does from the line and the
compiler's response to it.** Where two designs are defensible, the one that removes a
question wins over the one that adds an option.

### Integer overflow — traps, identically in every build
<!-- id: overflow -->

`Int` is 64-bit signed. Overflow is an error that stops the program, in development and in
release alike.

Wrapping silently is the classic source of a bug nobody reads: the arithmetic looks
correct and the number is wrong much later. Differing between debug and release is worse
— it makes a class of bug reproducible only where it cannot be debugged. Trapping costs a
check per operation, which matters for a language claiming performance, and the escape
hatch when it does is an explicit wrapping operator, not a build flag.

### Division by zero — an error, not a value and not undefined
<!-- id: division -->

`/` and `%` by zero stop the program with a diagnostic. Not a NaN-like sentinel, which
propagates silently; not undefined behaviour, which is unrepresentable in a language with
no `unsafe` yet.

### Shadowing — allowed in a nested scope, refused in the same one
<!-- id: shadowing -->

```nazm
let x = 1;
let x = 2;        // error: `x` is already defined in this scope
if c { let x = 3; }   // fine: a different scope
```

Rust permits same-scope shadowing and it is genuinely convenient. It is refused here
because of what it does to *reading a change*: when a later line mentions `x`, telling
which binding it means requires scanning upward for every intervening `let`. A diff that
inserts one silently changes the meaning of code it does not touch.

### Argument evaluation order — left to right, specified
<!-- id: evaluation-order -->

Specified, not "unspecified for optimisation". Unspecified order is a licence for two
correct compilers to disagree, and the resulting bug appears only after a toolchain
upgrade. v0.1 has no side effects for the order to be observable through, which is
precisely why it costs nothing to fix it now and would be a breaking change later.

### Function returns — the final expression, and nothing else
<!-- id: returns -->

A function body is a block; the block's value is its final expression; that is the return
value. There is no `return` statement in v0.1.

One exit point means the value a function produces is always in the same place. `return`
earns its keep with guard clauses, and it is not ruled out — it is deferred until there is
something to guard against.

> **Superseded in v0.3.** `return` exists and is implemented — see [`return`](#return)
> below. This entry is kept rather than edited because it records what v0.1 committed to
> and why, and the reasoning ("deferred until there is something to guard against") is
> what the later section answers. **The current executable language is 1.0**; a function's
> value is still its final expression, and `return` is for the paths that leave early.

### Types — `Int`, `Bool`, `Str`, and no inference across function boundaries
<!-- id: types -->

Parameter and return types are always written. Local `let` bindings infer from their
initialiser. Both branches of an `if` must have the same type, and a condition must be
`Bool` — there is no truthiness.


### Records — the first user-defined type
<!-- id: records -->

Settled by N9, 2026-09-23. A **record** is a nominal product type with named fields. The
source keyword is `struct`; the word *record* names the category everywhere else.

```nazm
pub struct Point {
    x: Int,
    y: Int,
}
```

#### One concept, two words, and why that is not two spellings

`struct` was reserved by name from the beginning — `crates/nazm-syntax/src/parser.rs`
refused it with *"there are no user-defined types yet"* — so the keyword was chosen before
this milestone and N9 only stopped refusing it. There is no `record` keyword and never
will be: a language with two spellings for one thing has two things to explain. The word
*record* is used for the semantic category (`DefKind::Record`, `docs/architecture.md`,
this section) precisely because it is **not** a keyword, so it can name the idea without
being mistaken for syntax.

#### Nominal, always

Two records with identical fields are different types.

```nazm
struct A { x: Int, }
struct B { x: Int, }
```

There is no conversion between `A` and `B`, implicit or explicit, and no structural
subtyping. A record's identity is its **declaration**: the module it is declared in and
the name it is declared under, which is the same rule a function's identity follows and is
spelled the same way — `model.nz::record Point`. Two modules may each declare `Thing`, and
they are two types.

#### Types and functions are separate namespaces

A module may declare a record `Point` and a function `Point`. Which one a name means is
decided by where it is written: a type position (`: T`, `-> T`) means the type, and a call
means the function. The grammar always knows, and that is the condition under which two
namespaces are worth having.

Built-in type names are **not** in the type namespace and cannot be shadowed: `struct Int`
is `N0331`. `Int` means the same thing in every module of every program.

Collisions between two imported types, or between an imported type and a local one, are
refused where they are created — the same rule and the same codes as the function
namespace, `N0207` and `N0208`. A name is never silently shadowed.

#### A record is a value, and its fields keep their own semantics

**Assignment creates another record value.** A record has no object identity: there is no
address-of, no reference equality, and no operation through which two bindings of one
record could be observed to be the same object. What each field does when the record is
copied is **that field type's existing law**, unchanged:

```nazm
struct Box {
    count: Int,
    name: Str,
    values: Ints,
}

let mut a = Box(count: 1, name: "n", values: ints_new());
let b = a;
```

| Field | After `let b = a` |
| --- | --- |
| `count` | two independent `Int`s. `a.count = 7` does not change `b.count` |
| `name` | two `Str` values with the same bytes. Whether they share a backing is unobservable, exactly as *Strings are bytes* already promised |
| `values` | **two references to one sequence.** `ints_push(a.values, 7)` is visible through `b.values` |

That last row is the design, not a concession. A record is a value; an `Ints` is an owned
mutable handle; a record *containing* one contains the handle, so copying the record copies
the handle and both name one storage. Making `Ints` copy because it sits inside a record
would have given the same type two meanings depending on where it was written.

Consequently the memory constitution needs **no fifth kind of value** for records. A record
is a composition of kinds, and every rule below is derived from its fields rather than
declared for it:

| | |
| --- | --- |
| **copy** | copy or share each field, by that field's rule |
| **destroy** | release each field, by that field's rule, in an order no program can observe |
| **needs cleanup** | some field does, recursively |
| **may cross into a task** | every field may, recursively |

#### Construction names every field

```nazm
let p = Point(x: 1, y: 2);
```

Named, never positional, so field order is never an implicit API. Every field is given
exactly one value: a missing one is `N0334`, an unknown one `N0333`, a repeated one
`N0332`. There are no default values, no positional fallback, and no update or spread form.

**Why the parenthesised form and not `Point { x: 1 }`.** The braced form is more familiar
and it collides with the language's own grammar: `if p { … }` is an `if` whose condition is
a bare name, and `p { … }` would be a record literal, and no amount of parser lookahead
turns that into a decision rather than a guess. The parenthesised form is told from a call
by two tokens — `IDENT "(" IDENT ":"` — and that *is* a decision, because no argument
expression can begin with a name followed by a colon. The smallest grammar with the fewest
contextual exceptions won.

**Initialisers evaluate in the order written.** `Point(x: f(), y: g())` calls `f` before
`g`, whatever order the fields were declared in and wherever the backend puts them. Physical
layout is unrelated to evaluation order, and a backend that filled fields in layout order
would have made field order observable through side effects.

**A construction that fails part way through cleans up.** If an initialiser fails after an
earlier one has produced a heap string or a sequence, that value is released. A record is
either fully initialised or it does not exist; there is no partially initialised record a
program can reach.

**A record with no fields is refused** (`N0101`), because `Marker()` is exactly how a call
to a function named `Marker` is written, and types and functions are separate namespaces,
so neither reading could be preferred. If a later milestone gives constructions a form that
cannot be a call, the refusal can be lifted without changing anything else.

#### Field order is not part of what a record means

Moving two field declarations changes nothing: not the type, not what constructs it, not
the module's interface fingerprint, and not the native layout. Construction is by name and
the persisted interface sorts fields by name, so the fingerprint cannot move; the backend
lays fields out in **canonical field-name order**, so the object file cannot either. An
edit the language says means nothing also *does* nothing — which is the only honest way to
make the claim, since the alternative is an ABI difference produced by an edit the compiler
itself calls meaningless.

#### Reading a field

`p.field` produces a value according to the field type's own law: an `Int` copies, a `Str`
is a value with the same bytes, an `Ints` or a `Chan` is another reference to the same
storage, and a nested record is another record value whose fields follow these rules again.

Field lookup starts from the **resolved type of the expression it is read from**, and goes
nowhere else. There is no global field namespace: `value.y` where `value` is an `A` that
has no `y` is `N0333`, whatever other record has a `y`.

**A projection out of a temporary is valid.** In

```nazm
let s = make().name;
```

`s` is a valid `Str` after the record `make()` returned has been destroyed. The value read
out holds whatever reference it needs before the record it came from is released; this is
rule 3 of the memory constitution applied to a projection.

#### Binding mutability reaches every field

```nazm
let p = Point(x: 1, y: 2);
p.x = 2;                      // N0310 — `p` is not mutable
```

```nazm
let mut p = Point(x: 1, y: 2);
p.x = 2;                      // fine
```

Mutability is checked at the **root binding** of the place being written, at any depth:
`a.inner.count = 3` needs `let mut a`. A record is a value, so replacing part of what a
binding holds is changing the binding — the same act as `p = q`, and it needs the same
permission.

Parameters remain immutable bindings, so a `struct` parameter's field cannot be replaced
either. There is no `inout`.

**Mutating a field and mutating what a field refers to are different things.** Given
`w: Work` with a field `values: Ints`, `w.values = other` is refused and
`ints_push(w.values, 1)` is not — the second changes the sequence, not the record. This
distinction already existed for sequence parameters; records inherit it rather than
introduce it.

**Replacement secures the new value before releasing the old.** `p.name = e` evaluates `e`,
takes whatever reference the new value needs, releases the old field, and stores. So
`p.name = p.name` is safe, a new value backed by the same allocation as the old one is
safe, and a failing `e` leaves the old field intact and valid.

#### Parameters, returns and tasks

A record parameter is **borrowed**, like every other parameter: calling `size(p)` neither
consumes nor duplicates the caller's record, and the callee cannot keep it. Returning a
record **transfers**, like every other return: each owning field survives into the caller,
on both the tail-expression edge and the explicit `return` edge.

Whether a record may cross into a task is **derived, not declared**: a record may cross
exactly when every field may, recursively. There is no marker to write and none to forget.

```nazm
struct Message { id: Int, text: Str, reply: Chan, }   // may cross
struct Work { values: Ints, }                          // may not — N0321
```

A record containing an `Ints` at any depth may not cross, and the diagnostic names the
field path that decided it — `Outer.inner.values` rather than "this record cannot cross".
Wrapping a sequence in a record does not launder it past `N0321`.

#### Visibility, and the API being closed

Record declarations follow the existing law: private by default, `pub` exports, imports
non-transitive. N9 records are **transparent** — a module that can see a record can
construct it and project its fields — so there are no private fields, no opaque records and
no accessors.

Because there are no opaque types, a public item may not name a private one:

```nazm
struct Secret { value: Int, }
pub fn get() -> Secret { … }              // N0337
pub struct Public { secret: Secret, }     // N0337
```

An importer would have to name the type to use the item and cannot, so the interface would
be one nobody could consume. The diagnostic names both the public item and the private type
it exposes.

A module's persisted interface carries every record definition its public surface mentions,
closed over field types — including one declared in a third module, because an importer has
to understand a shape it can project through and cannot go and read a module it never
imported. Carrying the *shape* is not carrying the *name*: nothing extra becomes writable,
and the non-transitive import rule is unchanged.

#### Recursion is refused as an impossible layout

```nazm
struct Node { next: Node, }        // N0336
struct A { b: B, }                 // N0336, including across modules
struct B { a: A, }
```

A record contains its fields **by value**, so a chain of record-typed fields that returns to
where it started describes a value that contains itself, and no finite layout exists. This
is not a memory-safety refusal and it is not converted into a pointer behind the reader's
back: it is an impossible layout, stated as one.

The rule is written over **inline value containment**, deliberately, so that a future type
which holds its contents somewhere else makes the same shape expressible without this rule
changing. A module import cycle remains legal and is a different thing entirely; only a
layout cycle is refused.

Records nested inside records are bounded at 32 levels (`N0338`). The bound is on the
emitted program rather than on the compiler — each level multiplies the copy and cleanup
code generated for the outer record — and it is stated rather than discovered.

#### What records are not, in N9

No variants, no pattern matching, no generics, no traits, no methods, no `impl`, no
closures, no `Option` or `Result`, no user-defined destructors, no operator overloading, no
inheritance, no field-level visibility, no default values, no update syntax.

**Equality is derived, since N13.** `a == b` on two records of one type compares their
fields, and is refused (`N0304`) when any field has no equality — a sequence field would
force the language to decide whether equality means the same storage or the same contents,
and it has not chosen. *Equality is derived* below owns the rule.

**`pub` still promises nothing about a binary.** A record's layout, its field offsets, its
size and its calling convention are compiler-private and may change between builds. `pub`
says which *Nazm modules* may name the type. There is no `repr`, no FFI layout, no
serialisation format and no storage layout; each of those needs an explicit contract and
none is made here.

### Enums — the second user-defined type, and the first sum
<!-- id: enums -->

Settled by N10, 2026-09-23. An **enum** is a nominal sum type: a closed set of named
variants, exactly one of which is active in any value, each carrying its own named payload
fields.

```nazm
pub enum Token {
    Eof,
    Number(value: Int),
    Name(text: Str),
    Pair(left: Int, right: Int),
}
```

`enum` and `match` were both reserved by name from the beginning —
`crates/nazm-syntax/src/parser.rs` refused them with *"there are no variant types yet"* and
*"there is no pattern matching yet"* — so the keywords were chosen before this milestone
and N10 only stopped refusing them. There is no `variant`, `union`, `case` or `switch`: one
concept gets one spelling, the rule `struct`/record already follows.

#### Exactly one variant is active, and that is a runtime fact

A record owns every field it declares. An enum value owns **the fields of its active
variant and nothing else**. That difference is the whole of what N10 adds to the memory
constitution, and it is a runtime fact rather than a static one: which fields exist is
determined by a discriminant the value carries.

An enum has a nominal identity, an active variant identity, and the payload values of that
variant. It has no object identity of its own — no address-of, no identity comparison, no
way to observe that two bindings came from one value; the `==` N13 derives compares values,
not identities. Two separately declared enums are different types
however identical their variants, exactly as two records are.

#### Declaration

A zero-payload variant is written with no parentheses; a variant with payload fields names
each one and its type, exactly as a record field is written.

```nazm
enum State {
    Ready,
    Failed(code: Int, why: Str),
}
```

The declaration has no parentheses for `Ready` because there is no field list to write, and
a record with no fields is refused for the same reason. Construction and patterns always
write them — see below — because there the parentheses are what separates a variant from a
field projection.

**An enum with no variants is refused** (`N0339`). A type with no values raises
`Never`/uninhabited semantics — what a function returning one means, whether a `match` on
one is vacuously exhaustive — that this language has not settled, and inventing an answer
as a side effect of allowing `enum Never {}` would settle it by accident.

**An enum with one variant is allowed.** It is a nominal sum type with one family of
inhabitants, and it is not silently a record: `match` still names the variant, and adding a
second variant later is an ordinary edit rather than a change of kind.

#### Variants are named by their enum

```nazm
Token.Eof()
Token.Number(value: 42)
```

A variant is never injected into the ordinary value namespace. Three consequences, all of
them the point:

- the same variant name may exist in many enums, so `enum A { Ready, }` and
  `enum B { Ready, }` coexist and `A.Ready()` and `B.Ready()` are different values of
  different types;
- importing a module brings in no constructor names;
- a reader and the parser both learn the nominal owner from the same two tokens.

**The parentheses are not optional, including for a zero-payload variant.** `Token.Eof`
alone is exactly the shape of a field projection out of a binding called `Token`, and the
language would then need to know which it was before it knew what `Token` referred to.
`IDENT . IDENT (` is a decision the parser can make from three tokens, and it is a decision
rather than a guess: there are no methods and no function values, so nothing else in the
language can put a `(` after a projection.

#### Construction names every payload field

```nazm
Token.Pair(
    left: 1,
    right: 2,
)
```

Every payload field exactly once, by name, with no unknown field, no duplicate, and a value
of the declared type. There is no positional form, no default value and no update syntax —
the same law records follow, for the same reason: declaration position is not an API.

Initialisers are evaluated **exactly once, in the order they are written**, whatever order
the payload is physically stored in:

```nazm
Token.Pair(
    right: f(),
    left: g(),
)
```

evaluates `f()` and then `g()`. Evaluation order, semantic field identity and physical
layout are three different things and this specification keeps them apart.

**If an initialiser fails, no enum value ever becomes live.** The values already produced
are released, the failing one never becomes a field, and the initialisers after it are never
evaluated. The discriminant is written only once every payload field has been established,
so cleanup after a partial construction never dispatches on a variant the value does not
yet have.

#### An enum is a value, and its active payload keeps its own semantics

```nazm
enum Data {
    Text(value: Str),
    Numbers(value: Ints),
    Channel(value: Chan),
}

let a = Data.Numbers(value: ints_new());
let b = a;
```

After `let b = a` both values have the same active variant and the same payload values, and
each payload value obeys **its own type's existing law** — unchanged, and unchanged by being
inside an enum:

| Active variant | After `let b = a` |
| --- | --- |
| `Text` | two `Str` values with the same bytes; sharing unobservable |
| `Numbers` | **two references to one sequence**; a push through either is visible through both |
| `Channel` | two references to one channel |

**The inactive variants own nothing.** `Text` does not hold a `Str` while `Numbers` is
active; there is no inactive payload, semantically, and no copy or release operation is
performed for one. That is not an optimisation, it is what a sum type means.

So the memory constitution still needs no new kind of value. Like a record, an enum is a
composition of existing kinds — the difference is only *which* composition applies:

| | Record | Enum |
| --- | --- | --- |
| **copy** | copy or share every field, by that field's rule | copy the discriminant, then copy or share **the active variant's** fields |
| **destroy** | release every field | release **the active variant's** fields |
| **needs cleanup** | some field does, recursively | some field of **some** variant does, recursively |
| **may cross into a task** | every field may, recursively | every field of **every** variant may, recursively |

The last row is deliberately not symmetric with the first two. Copy and destroy are runtime
operations and ask about the value; task safety is a **type-level** guarantee and asks about
the type. `enum Work { None, Values(xs: Ints), }` may not cross into a task even while it
holds `None`, because what a `spawn` is given is a value of a type, and no flow-sensitive
escape is offered.

#### Variant and payload-field order are not part of what an enum means

Moving two variant declarations, or two payload-field declarations, changes nothing: not
what the program means, not the interface fingerprint, and not the native representation.
The persisted interface sorts variants and fields by name and the backend assigns
discriminants and lays payloads out in the same canonical order, so the claim is made true
rather than asserted.

Initialiser evaluation order is a separate question and is the order written, as above.

#### `match` is an expression

```nazm
let n = match token {
    Token.Number(value: n) => n,
    Token.Name(text: _) => 0,
    Token.Pair(left: a, right: _) => a,
    Token.Eof() => -1,
};
```

It composes wherever an expression may appear, including as a statement, as a `return`
operand and as an initialiser, and it reuses the existing block and completion rules rather
than introducing a second set. An arm body is an expression, and a block is an expression,
so `=> { let n = str_len(s); n },` is the ordinary block, not a statement grammar of its own.

Every arm of a value-producing `match` produces the same type. There is no union inference
and no implicit conversion. An arm that transfers control — `return`, `break`, `continue` —
produces no value and constrains nothing, exactly as a branch of an `if` does.

**The scrutinee is evaluated exactly once**, before any arm is selected, and the `match`
holds that value for as long as the selected arm runs. The scrutinee is a value the `match`
owns, not the caller's storage: `match make_token() { … }` has a clear owner for the
temporary, and `match t { … }` leaves `t` untouched and independent. A `match` does not
consume the binding it reads.

The discriminant selects exactly one arm. There is no fallthrough, no backtracking and no
guard, so an expression in an unselected arm never runs.

#### Patterns name every payload field

```nazm
Token.Pair(
    left: x,
    right: _,
)
```

A variant pattern names **every payload field of that variant, exactly once**, either
binding it to a name or discarding it with `_`. There is no `..` rest syntax, and that is an
evolution property rather than pedantry: adding a payload field makes existing patterns
fail to check instead of silently ignoring new state.

`_` means *introduce no binding*. It does not mean *forget this field*: the value is still
the enum's, and it is still released when the `match`'s scrutinee dies. Two payload fields
may both be `_`; two bindings may not share a name.

Bindings are **borrowed, immutable views of the payload held by the match's scrutinee**,
live only inside the arm that introduced them. Matching does not consume the payload. A
binding that names an owned mutable handle borrows the handle, so `E.Values(items: xs) =>
ints_push(xs, 1)` mutates the referenced sequence — *immutable binding* is a statement about
the name, never about the storage it refers to.

If an arm's value is a payload — `Token.Name(text: s) => s` — the result takes its own
reference to whatever backs it **before** the arm's bindings end and before the scrutinee is
released. That is the same rule *A projection produces a value* already states for records,
applied to a value whose fields are a runtime fact.

A pattern's variant must belong to the scrutinee's enum. `B.Ready(…)` against an `A` is
refused (`N0345`) even if both enums have a `Ready`; there is no structural variant
matching.

#### Exhaustiveness

Every `match` must name **every variant of its enum, exactly once**.

- a missing variant is `N0343`, and the diagnostic lists the variants that are missing;
- a repeated variant is `N0344`, reported at the second arm;
- there is **no `_ => …` catch-all arm** in N10. *Corrected 2026-10-12, R1a:* `_` and a name are
  patterns now (*Patterns*, below); a `match` written variant by variant keeps everything this
  section says.

No runtime trap substitutes for the check. A valid program's discriminant always has a
statically known arm, so an unreachable default exists in the generated code as a structural
requirement of the instruction set and never as language semantics.

The absent wildcard is a decision about evolution, not an oversight. While the language is
young, adding a variant *should* break every importer that believed it had handled the whole
enum. A `non_exhaustive`-style opt-out, and the wildcard that goes with it, is a later
design that must be made deliberately.

#### `match` is the only elimination form

There is no `is`, no `instanceof`, no `tag()` and no `variant_name()`. The discriminant is
representation, not a value a program can read.

There is also **no payload projection outside a pattern**: `value.payload` is not written,
because which fields exist depends on the active variant. To change an enum value, assign a
whole new one.

#### Assignment and replacement

```nazm
let mut x = E.Text(value: "a");
x = E.Values(items: ints_new());
```

The new value is fully evaluated and owned first, then the old value's **active** payload is
released, then the discriminant and payload are replaced. Self-assignment, and a new value
backed by the same allocation as the old one, are safe consequences of that order rather
than special cases.

#### Records and enums compose, in both directions

A record field may be an enum, an enum payload field may be a record, and an enum payload
field may be another enum. Copy, release, task safety and layout all recurse through both
kinds. Nothing is derived from which kind happens to be on the outside.

#### Recursion is refused as an impossible layout

```nazm
enum List { End, Next(rest: List), }     // N0336
struct A { b: B, }                       // N0336
enum B { Value(a: A), }
```

The rule N9 stated over records is stated over **every inline user-defined value type**: a
chain of by-value containment that returns to where it started describes a value that
contains itself, and no finite layout exists. Records and enums form one containment graph
and the cycle is refused wherever it runs.

It is not secretly boxed, and it is not forbidden forever: the refusal is of an **inline
infinite layout**, so a later type that holds its contents somewhere else — a box, an arena
handle, a generic container — makes the same shape expressible without this rule changing.

Enums and records nested inside one another are bounded at 32 levels (`N0338`), for the
reason records already were: each level multiplies the copy and cleanup code generated for
the outer value.

#### Visibility, and the API being closed

An enum is private to its module unless it is written `pub`, exactly as a record is. For a
public enum, **every variant and every payload shape is part of the public type** — there is
no per-variant visibility and no private variant inside a public enum, because either would
be an opaque or open enum and neither has been designed.

A public type may not expose a private one (`N0337`), and the check runs recursively through
records and enums together: a public enum whose payload is a public record whose field is a
private enum is refused, because an importer needs that type to understand the shape it was
given.

#### Adding a variant is a breaking change, deliberately

For an exported enum, adding, removing or renaming a variant — or adding, removing or
retyping a payload field — changes the module's interface fingerprint, which re-checks every
importer, which rejects every `match` that is no longer exhaustive. A rename is a removal and
an addition; no lineage is invented.

That is the intended cost of a closed enum with no wildcard arm, and it is why the
discriminant is not a stable identity: the *meaning* of a variant is its enum and its name,
and the number the backend gives it is chosen fresh for each build.

#### What enums are not, in N10

No generics, so no `Option`, no `Result` and no generic enum of any kind — a monomorphic
`enum MaybeInt { None, Some(value: Int), }` is an ordinary user declaration and the language
offers no built-in like it. No typed errors, no `throw`, no `?`, no `try`: an enum can model
a result by hand, and that is not an error-handling feature. *(N12 added `Result`, `Option`
and `?` — as ordinary generic enums and a `match` — in* Typed error values *below; there is
still no `throw` and no `try`.)*

*Corrected 2026-10-12, R1a:* literal, or- and nested patterns, `_` and a name at the top level,
`if let` and `let … else` exist (*Patterns*, below); tuple patterns and `let (a, b) = e;` too
(*Tuples*). What follows was N10's list.

No literal, record, tuple, range or or-patterns; no guards; no top-level wildcard; no `let`
destructuring; no by-move or mutable patterns; no match ergonomics inferred from ownership.
Pattern matching in N10 selects a variant and binds its named payload fields, and nothing
else.

**Equality is derived, since N13.** `a == b` on two enum values of one type compares their
variants and then the active variant's payload, and is refused (`N0304`) when a payload field
of **any** variant has no equality. *Equality is derived* below owns the rule.

**`pub` still promises nothing about a binary.** An enum's discriminant values, its payload
layout, its size and its calling convention are compiler-private and may change between
builds. There is no `repr`, no C ABI, no FFI layout, no serialisation format and no storage
layout.


### Generics — first-order parametric types, and `Vec[T]`
<!-- id: generics -->

Settled by N11, 2026-09-23. A record, an enum and a function may declare **type parameters**,
and a type may be **applied** to type arguments. There is one kind — *type* — and nothing
else: no constraints, no traits, no methods, no higher-kinded parameter, no type-level
computation, no defaults, no variance, no specialisation. What N11 adds is first-order
parametric polymorphism and one generic container, `Vec[T]`, and every law below is written
so that the ones before it — records, enums, the memory constitution — survive substitution
unchanged.

```nazm
struct Pair[A, B] {
    first: A,
    second: B,
}

enum Maybe[T] {
    None,
    Some(value: T),
}

fn identity[T](value: T) -> T {
    value
}

fn main() -> Int {
    let xs = vec_new[Pair[Int, Str]]();
    vec_push(xs, Pair[Int, Str](first: 7, second: "nazm"));
    let p = vec_get(xs, 0);
    p.first
}
```

#### One syntax for each thing

| | |
| --- | --- |
| **Parameter list** | `[A, B]` after the declared name: `struct Pair[A, B]`, `enum Maybe[T]`, `fn identity[T](…)`. At least one name, no trailing comma |
| **Type application** | `Pair[Int, Str]`, `Maybe[Point]`, `Vec[Vec[Int]]`, anywhere a type is written |
| **Explicit function arguments** | `identity[Int](7)`, `vec_new[Token]()` |
| **Generic construction** | `Pair[Int, Str](first: 1, second: "x")` — the type arguments are written |
| **Generic variant** | `Maybe[Int].Some(value: 42)`, `Maybe[Int].None()` — the type arguments are written |
| **Pattern** | `Maybe.Some(value: x)` — the enum's **name**, never its arguments; the scrutinee's type supplies them |

Square brackets, and no angle brackets as a second spelling. `[` and `]` are new tokens in
N11 and mean this and nothing else, and there is no indexing expression for an application
to be confused with: `name[` in an expression can only begin type arguments — where `<`
would have been a comparison until the parser guessed otherwise. A pattern takes no
type arguments because it cannot choose them — the value being matched already has one
concrete type, and writing it again in every arm would be a second place to get it wrong.

#### A type parameter is a type and nothing else

Inside the declaration that owns it, a parameter may be written wherever a type is. It is
not a value, not an integer, not runtime metadata and not something a program can inspect.
Its names are unique within one list (`N0350`), may not be a built-in type name or `Vec`
(`N0331`) — and neither may a declared record or enum, since a type written `Vec` is always
the built-in — and are resolved lexically: a parameter belongs to its own declaration only, so
one function's `T` is never another's, and a parameter may shadow a user-defined type of the
same name inside the declaration that introduces it.

#### Order is semantic; names are not

`Pair[A, B]` and `Pair[B, A]` with the same field declarations are different definitions:
the first argument of an application fills the first position. **Renaming** a parameter —
`[A, B]` to `[X, Y]` with every use renamed — changes nothing: not what the definition
means, not its interface fingerprint, not any object. A parameter's identity is its
**position** in its owner's list, and that is what a persisted interface writes.

#### Identity

A generic definition is one definition — its `DefKey` and kind, exactly as before — and its
**arity** is part of its interface. An **applied type** is identified by the definition and
its ordered type arguments, recursively: `Pair[Int, Str]` is the same type in every module
of a compilation that resolves the same `Pair`, `Pair[Str, Int]` is a different one, and
`a.nz`'s `Box[Int]` is not `b.nz`'s `Box[Int]`, because the definitions differ. Nothing about
the shape is compared: two generic records with identical fields are still two types after
substitution, for the reason two ordinary records are.

#### Generic definitions are checked once, parametrically

A generic function is checked **once, as a generic definition**, with each parameter an
unknown type. It is never re-checked for a particular use, and whether it is valid does not
depend on how it is called. An unconstrained `T` supports exactly what every value in the
language supports: it may be bound, copied by its own law, passed, returned, stored in a
field or a payload of a generic type, pushed into a `Vec[T]` and taken out of one. It may
**not** be added, ordered, compared with `==` (`N0304`), projected (`N0335`), matched
(`N0342`), used as a condition (`N0301`) or passed to a task (`N0321`), because none of those
is valid for every type. `fn equal[T](a: T, b: T) -> Bool { a == b }` is refused at its
definition. Since N14 one constraint exists: a parameter declared `T: Equality` may be
compared with `==` — *A type parameter may require equality* owns it. Nothing else can be
required, so the rest of this list is unchanged.

#### Calling a generic function

The type arguments are either **written** or **inferred from the arguments**, and nothing
else — *(R1, 5b.16:)* except that a parameter the arguments leave open is then settled by the
type the call's result is wanted as, where it is wanted as one (*Conveniences for the first
program*):

- Inference unifies each parameter's declared type with the argument's type, recursing
  through applications and `Vec`: passing a `Vec[Point]` for `Vec[T]` gives `T = Point`.
- **The arguments decide first.** The expected result type only settles what they leave
  open: `fn make[T]() -> T` called as `make()` with nothing wanting a type is still refused
  (`N0354`) and is written `make[Int]()`, while `let x: Int = make()` settles `T = Int`.
  Local, left-to-right, from the arguments — so a call's meaning is visible in the call or
  the line it is on.
- Two arguments that give one parameter different types are **refused** (`N0355`) —
  `same(1, "x")` for `fn same[T](a: T, b: T)` does not choose the first.
- Explicit arguments fix every parameter, and an argument that then disagrees with the
  signature is an ordinary mismatch (`N0300`).
- The wrong number of type arguments is `N0353` wherever it is written — `Pair[Int]`,
  `Vec[Int, Str]`, `identity[Int, Str](…)`; type arguments on a definition that has no
  parameters are `N0351`; a generic type written without its arguments where a concrete type
  is needed — a field, a parameter, a construction — is `N0352`; since R1 (5b.14) an enum's
  construction written without them takes them from the type it is wanted as, or from its payload
  (*Short variants*, below), and is `N0352` only where neither says.
- An argument that never produces a value (every path `return`s, `break`s or `continue`s)
  contributes nothing to inference.

Inference is not overload resolution. A name still resolves to exactly one definition, and
generics add no overloading: `fn f[T]` and `fn f` in one module are two definitions of one
name, which is `N0203` as it always was. A generic `main` is refused (`N0356`): nothing
could choose its arguments. A `spawn` runs a non-generic function (`N0101`).

#### Generic records and enums

Every law in *Records* and *Enums* holds **after substitution**. `Box[Int]` and `Box[Str]`
are two concrete types of one definition; each is nominal, a value, constructed by naming
every field, copied and destroyed field by field according to the field types it has *after*
substitution. A generic enum's variants are its definition's, for every instantiation:
`Maybe[Int]` and `Maybe[Str]` have the same variant set, exhaustiveness is decided over it,
and only the active variant's payload owns anything.

Visibility is unchanged, and the closure rule recurses through applications: a public
signature or shape may not mention a private type **anywhere in a type argument** either
(`N0337`).

#### `Vec[T]` — a mutable handle, by the sequence law

`Vec[T]` is the generic counterpart of `Ints` and `Strs` and follows **their** law, not a
value law: it is an *owned mutable handle* in the memory constitution's table.

| | |
| --- | --- |
| **Creation** | `vec_new[T]()` — explicit, because nothing else could say what `T` is |
| **Identity** | a handle. `let b = a;` makes `b` the same vector, and a push through either is visible through both. Nothing is copied |
| **Length** | `vec_len(xs)` |
| **Reading** | `vec_get(xs, i)` returns **its own copy** of the element by `T`'s copy law, `N0405` unless `0 <= i < len` |
| **Replacing** | `vec_set(xs, i, v)` stores a copy of `v` and returns the index written. The new value is taken before the old one is released |
| **Appending** | `vec_push(xs, v)` stores a copy of `v` and returns the new length. A failed growth (`N0406`) leaves the vector exactly as it was |
| **Removing** | `vec_pop(xs)` **transfers** the last element out; `N0405` on an empty vector |
| **Equality** | refused (`N0304`), for the reason it is refused on `Ints` |
| **Tasks** | a `Vec[T]` **never** crosses into a task, whatever `T` is (`N0321`). It is shared mutable storage with an unsynchronised count, and a record or enum containing one inherits that |
| **Destruction** | when the last reference dies, every element is released by `T`'s law, then the storage |

There is **no indexing syntax**. `xs[i].field = v` would need a place calculus — what is
evaluated once, what is copied, what happens on failure part way through — and nothing in
N11 needs one. Updating a record element is written as what it is: get a copy, change it,
set it back.

*Corrected by R1, 5b.18:* `Ints` and `Strs` are `Vec[Int]` and `Vec[Str]` — two names for each of
two types, not four types — so either is accepted wherever the other is wanted, and the `ints_*`,
`strs_*` and `vec_*` functions apply alike (*Sequences are shared, mutable, and never freed*).

#### What may not be expressed: an ownership cycle

A `Vec` holds its elements **somewhere else**, so a type can now contain a handle to storage
holding values of that same type — `struct Node { children: Vec[Node], }`. Its layout is
finite, and a program could still build a `Node` whose `children` contain itself, which no
reference count ever reclaims. The memory constitution said in advance what admitting such a
type requires; **N11 takes the first option again, and refuses it.**

The rule is over the **ownership graph of definitions**: an edge runs from a record or enum
to every definition its fields or payloads mention, labelled *inline* or *through a
reference-counted container* by the path that reaches it — a field is inline, a `Vec`
element is not, and an argument of a generic type inherits how that type uses the
corresponding parameter. Then:

- a cycle whose every edge is inline is an **impossible layout** — `N0336`, as before;
- any other cycle is an **ownership cycle** — `N0359`, with the path that closes it.

*R1a* labels an edge through `Box` or `Chain` **immutable** and accepts a cycle of inline and immutable
edges (*Immutable indirection*, below); a cycle through a `Vec` is refused as here.

So `Node { children: Vec[Node] }`, `A → Vec[B] → B → Vec[A]`, `enum Tree { Leaf, Node(kids: Vec[Tree]) }`
and `Tree[T] { kids: Vec[Tree[T]] }` are refused, and `Vec[Vec[Int]]`, `Vec[Pair[Int, Str]]`,
a record holding a `Vec[Int]` and an enum holding a `Vec[Str]` are not: an acyclic ownership
graph is a graph whose values cannot own themselves. A parameter that no field uses
contributes no edge. The rule also bounds every instance family: with no cycle among the
definitions, substitution can only nest types a finite way deep, so it never runs forever.

#### How much a generic value costs

Nothing about generics is free and nothing claims to be. Copying a `Pair[Str, Chan]` is two
reference adjustments; copying a `Maybe[Record]` is a branch on the discriminant and then the
active payload's copy; assigning a `Vec[T]` is one adjustment, never a copy of its elements.
The same table as *What this costs* applies to every instantiation, with the field types it
has after substitution.

#### What a program cannot observe

Which native code implements a generic function, whether one copy of it serves two
instantiations, what size `T` has and what a specialisation is called are not part of the
language. `nazm check` and `nazm run` are parametric; `nazm build` currently
**monomorphises** — one native instance per concrete argument list — and that is a property
of the current LLVM backend, not of the language (`architecture.md` §7.12). The native
backend refuses polymorphic recursion (`N0357`) — recursion through which an instance would
need another instance without end — and bounds how many instances one program may need (`N0358`);
ordinary recursion through a generic function compiles (N49).

#### What generics are not, in N11

*Superseded in part by N78:* traits, `impl`, methods and trait bounds now exist (*Traits and
methods*, below). The rest of this list stands.

No traits, interfaces or protocols; no `impl` and no methods; no constraints or `where`; no
associated types or constants; no higher-kinded parameters; no const generics; no defaults;
no variance or subtyping; no specialisation or overloading; no reflection. No closures, no
function values, no `Never`, no unit type. Construction infers nothing: its type arguments
are written. *(R1, 5b.14: an enum's construction now infers them — *Short variants*, below.)*

**Corrected 2026-09-24, N12.** This paragraph said there was no built-in `Option` or
`Result`. There still is none built into the checker: both are now ordinary generic enums
declared by the core prelude — the next section — and nothing about generics changed to
admit them.

### Typed error values — `Result`, `Option`, and `?`
<!-- id: errors -->

*Settled by N12, 2026-09-24.* **A typed failure is a value, and propagation is structured
control flow over that value.** There is no exception, no `throw` or `catch`, no unwinding,
no hidden error object and no conversion between error types. `Result` and `Option` are
two ordinary generic enums, and `?` is one postfix operator that means a `match` with a
`return` in one arm.

#### The core prelude

One module belongs to the toolchain rather than to a project: **the core prelude**, whose
source is `library/core/prelude.nz` and which declares exactly

```nazm
pub enum Result[T, E] {
    Ok(value: T),
    Err(error: E),
}

pub enum Option[T] {
    None,
    Some(value: T),
}
```

It is ordinary source, checked by the ordinary checker, and it is part of **every**
compilation — including one given as a string. Its rules:

- **Every module imports it**, with no `use` written. Its two types are therefore in every
  module's type namespace exactly as a direct import's exported types are, and by the same
  rule; nothing is copied into a module, and every module names the same two definitions.
- **Its identity is the toolchain's.** Its `ModuleKey` is `@core/prelude` in every project,
  from every working directory and every checkout, so `Result` is
  `@core/prelude::enum Result` everywhere. No project path produces a key beginning
  `@core`: a project file that happens to sit under a directory of that name compiles, and
  has no durable identity (*Durable identity*, below).
- **It is a dependency like any other.** A module's check key includes the prelude's key
  and interface hash, as it includes every import's. A change to the prelude's public
  shape reaches every module; one that moves no public shape reaches none. Every module
  depends on the whole prelude — there is no per-name slicing — which costs nothing while
  it declares two types.
- **Its names are reserved.** A project module may not declare a type named `Result` or
  `Option` (`N0363`, at the declaration). Otherwise ordinary lookup would find the
  project's while `?` meant the prelude's. Only the **type** namespace: a function called
  `Result` is legal, as it always was.
- **It is not a package system.** There is one such module, with two declarations; nothing
  can add a second, and there is no registry, version, manifest or namespace mechanism
  behind it.

#### `Result` and `Option` are ordinary enums

Everything *Enums* and *Generics* say applies, unchanged, and nothing is added:

- construction is qualified and carries its arguments — `Result[Int, Str].Ok(value: 1)`,
  `Option[Str].None()` — and a pattern names the definition, `Result.Ok(value: v)`;
- `match` is exhaustive over both variants;
- layout, copying and release are an enum's: one active variant, released by its own law;
- task safety is derived: `Result[Int, Str]` and `Result[Int, Chan]` may cross into a task,
  `Result[Int, Vec[Diag]]` and `Option[Vec[Int]]` may not (`N0321`);
- the ownership graph sees through them: `struct Node { next: Option[Vec[Node]] }` is an
  ownership cycle (`N0359`), and `struct Node { next: Option[Node] }` has no layout
  (`N0336`).

**There is no null.** `Option[Str].None()` is a variant, not a null pointer, and
`Option[Int].None()` is not a magic integer. **There are no methods** — no `unwrap`, `map`,
`is_ok` — and no panicking accessor: a program takes a value apart with `match`.

#### `?` — propagation

`?` is a postfix operator beside projection, in any order and any number: `r?`,
`make()?.x`, `r??`. It binds tighter than every operator, including negation, so `-r?` is
`-(r?)`. `??` is two `?` tokens and removes two layers; `(r?)?` is the same thing written
out.

**The rule.** If `expr : Result[T, E]` — an application of the core prelude's `Result`,
recognised by **definition**, never by name or by the shape of its variants — and the
enclosing function returns `Result[U, E]` with the **identical** `E`, then `expr? : T`. `U`
is free: `?` yields the operand's success value where it is written, and only the error
leaves. `?` means exactly

```nazm
match expr {
    Result.Ok(value: v) => v,
    Result.Err(error: e) => return Result[U, E].Err(error: e),
}
```

with `expr` evaluated **once**. Everything below follows from that sentence.

**What is refused**, each at the `?`:

| code | meaning |
| --- | --- |
| `N0360` | the operand is not the core `Result` — an `Option`, a project enum with `Ok` and `Err` variants, anything else |
| `N0361` | the enclosing function does not return the core `Result` — including `main`, and including a function returning a lookalike |
| `N0362` | the operand's error type is not exactly the function's: two records with identical fields are two error types, and two type parameters `E1`, `E2` are not known to be equal |

There is no conversion: to change an error type, `match` and construct the other.

**Completion.** A propagation completes as `Value(T)` when its operand does; the `Err`
path is a `return` and adds no completion state of its own. An operand that never
produces a value makes the propagation transfer too.

**Evaluation order.** The operand is evaluated once, where the left-to-right rules put it.
When it is an `Err`, nothing after it in the enclosing expression is evaluated: no later
argument and no call, no arithmetic on it, no store of an assignment — the destination
keeps its value — no later field initialiser and no completed record or variant, whose
earlier fields are released, and no inspection of a `match` whose scrutinee it is.

**Cleanup is exactly `return`'s.** Every `scope` the propagation leaves is joined,
innermost first, before the function returns — a task still running is waited for, and a
task's failure is taken on as it is for `return`. Every temporary the enclosing
expressions hold and every local of the function are released once.

**Ownership.** On `Ok`, the payload takes its own reference before the temporary `Result`
is released, so a `Str` that came out of one outlives it. On `Err`, the error value is
owned by the returned `Result` before anything it came out of is released. A `Vec` error
is a handle: propagated through three functions, it is one vector with one logical
reference at each return boundary, never a copy.

**What `?` is not.** Not an exception: no unwinding, no landing pad, no catch table, no
runtime failure and no message — a propagated `Err` is a normal return. Not an effect: a
function's error type is visible in its signature because it is inside its return type,
and a caller may store, pass or ignore the `Result` like any value. Runtime failures —
`N0405`, `N0406`, division by zero, overflow — are unchanged and never become an `Err`.
Not available on `Option`, which a program takes apart with `match`: a second carrier
would need a propagation protocol, and traits do not exist.

**`main` is unchanged.** It returns `Int`, so a program handles its errors itself with a
`match` and decides what they mean; nothing maps an `Err` to an exit status.

### The standard library — `use "@std/…"`
<!-- id: std -->

A **standard module** is a toolchain-owned Nazm module a program imports by name. There are
twenty-nine (N84's seventeen, the 1.0 surface, and Gate 2's four collections, `ioerror`,
`file`, `net`, `env`, `error`, `binary`, `random` and `log`),
each ordinary source checked like any module, and none is visible unless a module writes its `use`:

```nazm
use "@std/text";
use "@std/io";

fn main(io: IoCap) -> Int ! { io } {
    let words = text_split("alpha,beta,,gamma", ",");
    io_println(io, int_to_str(strs_len(words)));
    match text_parse_int(text_trim("  42 ")) {
        Result.Ok(value: n) => n,
        Result.Err(error: e) => 0,
    }
}
```

**Resolution.** An import path beginning with `@std/` names a module the toolchain provides and
is never looked for on disk: `@std/NAME` is the standard module `NAME`, and any other `@std/` path
is `N0205`. Every other path, including one beginning with another `@`, is relative to the
importing file as before. A standard module's durable identity is `@std/NAME`, which no project file can have. Its
exported functions enter the importer's namespace like any module's, so each is named after its
module — `text_split`, not `split` — and a program may not define a function of the same name in a
module that imports it (`N0208`).

**Contracts.** Every standard function declares its effect set. The pure ones are `! {}`; the ones
that reach the outside world take the `IoCap` they need and declare `! { io }`, so a caller's
authority is as visible as with the built-ins. What they read carries the built-ins' origins:
`io_read`'s result is `file`.

**Errors.** Ordinary failure is a value: `Err` or `None`, never a trap. A standard function fails
only where a built-in it calls would — `ints_sum` overflows as `+` does, and `io_read` of a file
that exists but cannot be read is `N0407`.

| Module | Functions |
| --- | --- |
| `@std/text` | `text_starts_with(s, prefix) -> Bool`, `text_ends_with(s, suffix) -> Bool`, `text_find(s, needle) -> Option[Int]`, `text_contains(s, needle) -> Bool`, `text_repeat(s, n) -> Str`, `text_split(s, sep) -> Strs`, `text_is_space(byte) -> Bool`, `text_trim(s) -> Str`, `text_parse_int(s) -> Result[Int, Str]`, and (N84) `text_trim_start(s)`, `text_trim_end(s)`, `text_is_digit(byte)`, `text_is_alpha(byte)`, `text_to_upper(s)`, `text_to_lower(s)` (ASCII), `text_replace(s, from, to)`, `text_count(s, needle)`, `text_pad_left(s, width, fill)`, `text_pad_right(s, width, fill)`, `text_lines(s) -> Strs`, `text_compare(a, b) -> Int` (byte order); and (R1.5, 3.3) `text_split_end(s, sep, from) -> Int` and `text_split_next(s, sep, end) -> Int`, where a piece of `text_split` ends and the next begins — what `for w in s.split(sep)` reads its pieces through, building no list |
| `@std/option` | `option_is_some[T](o) -> Bool`, `option_is_none[T](o) -> Bool`, `option_unwrap_or[T](o, fallback) -> T` — so `o.unwrap_or(0)`, `o.is_some()`; in a program's own module with no `use` (R1.5, D9, 4.5) |
| `@std/result` | `result_is_ok[T, E](r) -> Bool`, `result_is_err[T, E](r) -> Bool`, `result_unwrap_or[T, E](r, fallback) -> T`, `result_ok[T, E](r) -> Option[T]`, `result_err[T, E](r) -> Option[E]` — so `r.unwrap_or(1)`, `r.ok()`; in a program's own module with no `use` (R1.5, D9, 4.5) |
| `@std/seq` | `ints_index_of(xs, x) -> Option[Int]`, `ints_contains(xs, x) -> Bool`, `ints_sum(xs) -> Int`, `ints_max(xs) -> Option[Int]`, `ints_min(xs) -> Option[Int]`, `strs_index_of(xs, x) -> Option[Int]`, `strs_contains(xs, x) -> Bool`, `vec_index_of[T: Equality](xs, x) -> Option[Int]`, and (N50) `vec_map[T, U](xs, f: fn(T) -> U) -> Vec[U]`, `vec_filter[T](xs, keep: fn(T) -> Bool) -> Vec[T]`, `vec_fold[T, A](xs, init: A, f: fn(A, T) -> A) -> A` — each, since N51, polymorphic in the effects of the function it takes (`effects E`), and doing exactly what that function does; and (N84) `vec_reverse[T](xs)`, `vec_slice[T](xs, from, to)` (clamped), `vec_concat[T](a, b)`, `vec_any[T](xs, test)`, `vec_all[T](xs, test)` |
| `@std/sort` | (N84) `vec_sort_by[T](xs, less: fn(T, T) -> Bool) -> Vec[T]` (stable, effect-polymorphic), `ints_sort(xs) -> Ints`, `strs_sort(xs) -> Strs` — a module of its own because a sort allocates in a loop and calls through a function value, and a restriction profile refusing either refuses every function of a module it imports; and (R1e, S3; stable since R1 closed (0.10.0)) the in-place sorts, which reorder the sequence they are given — every handle to it sees the new order — and return `0`: `vec_sort_in_place_by[T](xs, less) -> Int` (stable, effect-polymorphic, `O(n log n)` calls of `less`: runs of sixteen by insertion, merged bottom-up through one scratch vector of `xs`'s length), `ints_sort_in_place(xs)`, `strs_sort_in_place(xs)` (byte order, a prefix first); and (R1.5, D10) `hashmap_sort_by[K: Hash, V](m, less: fn((K, V), (K, V)) -> Bool) -> Vec[(K, V)]`, a map's entries ordered by `less`, stably over the map's order |
| `@std/io` | `io_println(io, s) -> Int`, `io_eprintln(io, s) -> Int`, `io_read(io, path) -> Result[Str, Str]` — each `! { io }`; and (Gate 2) `io_stdin_read_all(io) -> Result[Bytes, IoError]`, `io_stdin_lines(io) -> Result[Strs, IoError]`; and (R1d, P3, stable since R1 closed (0.10.0)) `io_stdin_read(io, buf) -> Result[Int, IoError]` (what standard input holds, at most the buffer; `0` at the end), `io_stdin_read_exact(io, buf) -> Result[Int, IoError]` (exactly the buffer; `UnexpectedEof` if standard input ends first) and `io_stdin_read_line(io) -> Result[Option[Str], IoError]` (the next line without its newline, a carriage return before it kept; `None` at the end) — each through standard input's one buffer, so they mix with each other and with `io_stdin_read_all` |
| `@std/random` | (Gate 2) `Rng`, xoshiro256** seeded through SplitMix64: `random_new(seed)`, `random_u64`, `random_int(r, lo, hi)` (every value of `lo..hi` equally likely), `random_float` (`[0, 1)`, 53 bits), `random_shuffle[T]` — pure; and `random_bytes(rc, buf)`, `random_seed(rc)` from the operating system's entropy, each taking the `RandomCap` that allows it |
| `@std/hal` | (Gate 3) Device code against traits: `Uart` (`put`), `Pin` (`set`, `get`) and `Timer` (`ticks`), every device method taking the `MmioCap` that allows it; the emulated boards' devices behind them — `Pl011`, `Ns16550` and `CmsdkUart`, a PL061's line (`Pl061`, `pl061_output`), the MPS2's FPGA LED bits (`FpgaLed`) — and `Ticks`, an atomic tick count a timer handler advances (`ticks_advance`) and `wait_ticks[T: Timer]` waits on; `uart_write[U: Uart]`. No device is the language's, and none has run on hardware |
| `@std/audit` | (Gate 3) `audit_transition(uart, m, machine, from, to)`: one state transition's record, `{"audit":MACHINE,"from":FROM,"to":TO}` on its own line, written byte by byte to the UART whose data register is `uart`, with the `MmioCap` that allows it. Nothing allocated and every loop counted, so a heapless board, `no-heap` and `bounded-loops` admit it; the record of what the `audit` rule asks for |
| `@std/dist` | (Gate 3) The distributed foundation's contract: `NodeId` (`node_id(name, addr)`), a value, never ambient; `Msg` — sender, idempotency `key`, `kind`, `body` — framed by `msg_encode` as four little-endian bytes of length and its fields over `@std/binary`, `msg_decode` refusing a payload that is not exactly one; `tcp_send_msg` and `tcp_recv_msg`, a frame over a stream, longer than a mebibyte refused; the `Transport` trait, `ask(self, net, time, m, timeout_ms) -> Result[Msg, Str]`, with `Tcp` (a connection a request) and `Local` (a channel each way, the deadline through `chan_select_until`); and `retry(t, net, time, m, attempts, base_ms, timeout_ms)`, the same message each try, waiting `base_ms` and doubling between. A receiver that records the keys it applied answers a repeat without applying it twice; the library does not do that for it |
| `@std/linalg` | (Gate 3) Small fixed vectors and matrices over `Float64`: `Vec3` (`vec3`, `vec3_add`, `vec3_sub`, `vec3_scale`, `vec3_dot`, `vec3_norm`, `vec3_clamp` — a length bound) and `Mat3`, by rows (`mat3`, `mat3_identity`, `mat3_apply`, `mat3_transpose`, `mat3_mul`). Values, nothing allocated, so a heapless board and `no-heap` admit them; the arithmetic operators and `float_sqrt` only, each correctly rounded, so `no-float-nondeterminism` does too |
| `@std/process` | `process_args(io) -> Strs` — `! { io }`; and (Gate 2) `Child`, `ProcessOutput { status, stdout, stderr }`, `process_spawn(p, program, args)`, `process_stdin`, `process_stdout`, `process_stderr` (pipes as `@std/file` `File`s), `process_wait`, `process_kill`, `process_run(p, sp, program, args, input)` — the input and the errors moved in tasks of their own, so neither pipe fills unread; and (R1d, P1; stable since R1 closed (0.10.0), over *The platform, further*) `process_id(io) -> Int`, `process_spawn_in(p, program, args, dir, env)`, `process_spawn_with(p, program, args, dir, env, new_group, null_stdin)`, `process_child_id(c) -> Int`, `process_try_wait(c)` and `process_wait_until(c, deadline)` — each `Result[Option[Int], IoError]`, `None` while the child runs — `process_kill_group(p, group, signal)` and `process_peak_memory(c)` |
| `@std/chan` | `chan_take(c) -> Option[Int]`: the next value, or `None` once closed and drained; and (N84) `chan_send_all_ints(c, xs) -> Int`, `chan_collect_ints(c, n) -> Vec[Int]`, `chan_drain_ints(c) -> Vec[Int]`, `chan_send_all_strs(c, xs) -> Int`, `chan_collect_strs(c, n) -> Vec[Str]`, `chan_drain_strs(c) -> Vec[Str]` — for `Int` and `Str` because a channel cannot carry a type parameter (`N0390`) |
| `@std/show` | the `Show` trait and `show_all[T: Show](v, sep) -> Str` (N78) |
| `@std/fmt` | (N84) `fmt_int_zero(n, width)`, `fmt_int_width(n, width)`, `fmt_hex(n)` (two's complement for a negative), `fmt_bool(b)`, `fmt_list(items, sep, open, close)`, `fmt_ints(xs, sep)` |
| `@std/num` | (N84) `num_abs(n)`, `num_sign(n)`, `num_min(a, b)`, `num_max(a, b)`, `num_clamp(n, lo, hi)`, `num_pow(base, exp) -> Option[Int]` (`None` past `Int`'s magnitude), `num_gcd(a, b)` |
| `@std/log` | (Gate 2) `LogLevel` (`Debug`, `Info`, `Warn`, `Error`), `Logger`, `LogSpan`: `log_new(min)`, `log_with(l, fields)`, `log_str`, `log_int`, `log_bool` (one field each), `log_fields()`, `log_line`, `log_at`, `log_debug`, `log_info`, `log_warn`, `log_error` — one JSON object a line on standard error, needing an `OutCap`; `log_level_name`; `log_span_begin(t, name)`, `log_span_end(out, t, l, s, fields)` — a span's name and nanoseconds, under a `TimeCap` |
| `@std/map` | (N84) `StrMap[T]` and `Entry[T]`: `map_new[T]()`, `map_len`, `map_get -> Option[T]`, `map_has`, `map_set -> Bool` (whether it replaced), `map_remove -> Option[T]`, `map_keys -> Strs`, `map_values -> Vec[T]` — ordered by key, found by binary search, shared like any handle |
| `@std/path` | (N84) `path_join(base, rest)`, `path_base(p)`, `path_dir(p)`, `path_ext(p)`, `path_stem(p)`, `path_normalize(p)` — pure, lexical, `/`-separated |
| `@std/fs` | (N84) `fs_exists(io, path) -> Bool`, `fs_read(io, path) -> Result[Str, Str]`, `fs_read_lines(io, path) -> Result[Strs, Str]`, `fs_write(io, path, text) -> Result[Int, Str]`, `fs_write_lines(io, path, lines) -> Result[Int, Str]` — each `! { io }`; and (R1d, P2; stable since R1 closed (0.10.0)) `fs_temp_dir(io) -> Result[Str, Str]`, a new directory of `os_mkdtemp`'s under `TMPDIR`, or `/tmp` where that is unset or empty |
| `@std/time` | (N84) `time_ms(t)`, `time_since(t, start)`, `time_deadline(t, ms)`, `time_passed(t, deadline) -> Bool` — each taking the `TimeCap` that allows it, and no effect; and (Gate 2) `Duration { nanos }`, `Instant { nanos }`, `WallTime { unix_nanos }`, `time_duration_ms(ms)`, `time_duration_ns(ns)`, `time_duration_to_ms(d)`, `time_duration_add(a, b)`, `time_now(t)`, `time_elapsed(t, since)`, `time_after(t, d)`, `time_reached(t, at)`, `time_between(a, b)`, `time_wall(t)`, `time_sleep(t, d)` |
| `@std/json` | (N84) `JsonDoc` and `JsonNode`, an arena document: `json_parse(text) -> Result[JsonDoc, Str]` (an `Err` names the byte offset; integers only), `json_encode(doc)`, `json_encode_node(doc, node)`, `json_quote(s)`, `json_root`, `json_kind`, `json_len`, `json_at`, `json_get`, `json_key_at`, `json_value_at`, `json_int`, `json_str`, `json_bool`, and the builders `json_new`, `json_with_root`, `json_add_null`, `json_add_bool`, `json_add_int`, `json_add_str`, `json_add_array`, `json_add_object`; and (Gate 2) `json_parse_floats(text)` — a number with a fraction or an exponent, or an integer `Int` cannot hold, is a `Float64` node, which `json_kind` calls a `number` and `json_int` does not answer; `json_parse` still refuses them — `json_add_float(doc, x)`, `json_float(doc, node) -> Option[Float64]` (an integer node converted), and `json_encode_pretty(doc)` (two spaces a level); a float is written as the shortest text that reads back to it, with `.0` where it would read as an integer, and an infinity or NaN as `null` |
| `@std/toml` | (R1c, S1; stable since R1 closed (0.10.0)) TOML 1.0 as a tree: `pub enum Toml` — `Str`, `Int`, `Float`, `Bool`, `OffsetDatetime`, `LocalDatetime`, `LocalDate`, `LocalTime` (each date-time its RFC 3339 text, `T` and `Z` upper-case), `Array(items: Chain[Toml])`, `Table(keys: Chain[Str], values: Chain[Toml])`, a table's keys in the order they were first defined. `toml_parse(text) -> Result[Toml, Str]` checks every TOML 1.0 rule — a key or table defined twice, a header after dotted keys made the table, an inline table extended, an integer past `Int`, a date that is not one — and never traps: an `Err` is `line:column: what`. `toml_encode(t)` writes a table's plain values, then each sub-table under its header and each array of tables as `[[...]]`: the text parses back to the same values, each table's keys in that order, and encoding again gives the same text. `toml_get(t, key) -> Option[Toml]`; (L2c) `toml_is_table(t) -> Bool`, what a derived record is read from |
| `@std/sha256` | (R1e, S1; stable since R1 closed, 0.10.0) SHA-256 as FIPS 180-4 defines it: `sha256(data: Bytes) -> Bytes`, the 32-byte digest; `sha256_hex(data) -> Str`, its 64 lower-case hexadecimal digits; and an incremental `Sha256`, a handle shared like a `Vec` — `sha256_new()`, `sha256_update(h, data) -> Int` (`0`), `sha256_finish(h) -> Bytes`, which pads a copy and leaves the hasher as it was, so more may be given and finished again. The same digest for the same bytes in every implementation, however they are split. A digest for identity and integrity — content addresses, lockfiles — written in Nazm, not the cryptography 1.0 leaves out (below): no constant-time claim and no audit |
| `@std/blake3` | (R1e, S1; stable since R1 closed, 0.10.0) BLAKE3's default 32-byte output, as its reference implementation computes it — 1024-byte chunks, seven-round compression, the tree of chaining values with `CHUNK_START`, `CHUNK_END`, `PARENT` and `ROOT`: `blake3(data: Bytes) -> Bytes`, `blake3_hex(data) -> Str`, and an incremental `Blake3` — `blake3_new()`, `blake3_update(h, data) -> Int` (`0`), `blake3_finish(h) -> Bytes`, leaving the hasher as it was. Unkeyed hashing only: no keyed mode, key derivation or longer output. A digest, as `@std/sha256`'s is |
| `@std/regex` | (R1e, S5; stable since R1 closed (0.10.0)) Regular expressions matched in time linear in the text: a pattern is compiled into a Thompson NFA and run as a Pike VM, every state advancing together a scalar value at a time and each held by one thread, so a search is O(m·n) for m instructions and n bytes and nothing backtracks. `pub struct Match { start, end }` (byte offsets, `end` exclusive), `pub struct Regex`; `regex_compile(pattern) -> Result[Regex, Str]` — an `Err` is `what at byte N`; `regex_is_match(r, text) -> Bool`; `regex_find(r, text, from) -> Option[Match]`, leftmost-first as in Rust's `regex` and Perl (the earliest start, then the pattern's preference: an earlier branch, a greedy repetition's longer run, a lazy one's shorter), `None` too for `from` outside `0..=str_len(text)`; `regex_find_all(r, text) -> Vec[Match]`, non-overlapping, an empty match right where the previous one ended skipped, as Rust's `find_iter` does (worst case O(m·n²), as there); `regex_captures(r, text, from) -> Option[Vec[Option[Match]]]` — item 0 the match, item `k` group `k`, `None` for a group that took no part; `regex_groups(r)`. Rust's syntax: literals and escapes (`\n \t \r \f \v`, `\xHH`, `\x{…}`, `\uHHHH`, `\u{…}`, `\UHHHHHHHH`, `\` before ASCII punctuation), `.`, classes `[…]` and `[^…]` with ranges, `\d \w \s` and their negations (Unicode: `\d` is `\p{Nd}`, `\s` `White_Space`, `\w` letters, marks, `Nd`, `Nl`, `Pc`, the joiners and the circled letters), `\p{…}`, `\P{…}` and `\pX` for the general categories `L` `Lu` `Ll` `Lt` `Lm` `Lo` `M` `Mn` `Mc` `Me` `N` `Nd` `Nl` `No` `Pc` of Unicode 16.0, `^ $ \A \z \b \B`, `\|`, groups `(…)`, `(?:…)`, `(?<name>…)`, `(?P<name>…)` (numbered; names not kept), flags `(?ims-ims)` and `(?ims:…)` (`i` folds ASCII letters only), `* + ? {n} {n,} {n,m}` greedy or lazy, and a one-scalar lookahead `(?=C)` / `(?!C)` — enough for the GPT-2 and `cl100k_base` pre-tokenisers' `\s+(?!\S)`. `.` and a class match one whole UTF-8 scalar value; a byte that begins no valid sequence is read as U+FFFD. Refused by name: backreferences, lookbehind, longer lookaheads, nested classes and `&&`, POSIX classes, other properties and scripts, other flags, and a pattern past 100,000 instructions once its counts are written out |
| `@std/args` | (R1e, S4; stable since R1 closed (0.10.0)) a command line read against a specification, with no effect. Built by functions, each returning the command it declares on: `args_program(name, about) -> ArgCommand`, `args_command(parent, name, about)` (a sub-command, chosen where its parent's first positional argument would be), `args_flag(c, name, short, help)`, `args_option(c, name, short, value_name, help)`, `args_option_default(c, name, short, value_name, default, help)`, `args_option_required(...)`, `args_positional(c, name, required, help)`, `args_rest(c, name, help)` (every further positional argument), `args_conflict(c, a, b)`; one arena, `ArgSpec` (`ArgCmd`, `ArgDecl`, `ArgConflict`), shared by its commands. `args_parse(c, argv) -> Result[Args, Str]` reads `--name`, `--name value`, `--name=value`, `-n`, `-n value`, `-nvalue`, bundled `-abc` (the last may take a value), `--` (every argument after it positional) and `-` (positional); an option takes the next argument whatever it is; a command's options are accepted after its sub-commands; every option may be repeated. `--help`, and `-h` unless the command declares `-h`, stops the parse with nothing required. An `Err` is `command path: what`, naming the argument — an unknown option, command or argument, a missing or unwanted value, a missing required option or argument, a conflict, or a specification declaring a name twice. `args_has`, `args_times` (`-vvv` is three), `args_get -> Option[Str]` (the last value, else the default), `args_get_all -> Strs`, `args_subcommand` (the chosen path, `"pkg add"`, or `""`), `args_help_wanted`, `args_help_text`; `args_usage(c)` and `args_help(c)`, generated: usage, arguments, options (its own, its parents', `--help`) and sub-commands, aligned |
| `@std/bytebuf` | (R1e, S2; stable since R1 closed (0.10.0)) `ByteBuf`, one growable buffer shared like any handle, for building binary output — appending amortised constant time a byte (the storage doubles), reading, overwriting and patching constant time: `bytebuf_new()`, `bytebuf_with_capacity(n)`, `bytebuf_len`, `bytebuf_clear` (keeps the storage), `bytebuf_push(b, v: UInt8)`, `bytebuf_push_bytes(b, data)`, `bytebuf_push_str(b, s)` (no length), `bytebuf_put_u16_le`, `bytebuf_put_u16_be`, `bytebuf_put_u32_le`, `bytebuf_put_u32_be`, `bytebuf_put_u64_le`, `bytebuf_put_u64_be`, `bytebuf_put_uleb128(b, v: UInt64)` and `bytebuf_put_sleb128(b, v: Int)` (each answering how many bytes it took), `bytebuf_align_to(b, n)` (zero bytes to a multiple of `n`; the new length), `bytebuf_get(b, i) -> Option[UInt8]`, `bytebuf_set(b, i, v) -> Bool`, `bytebuf_patch_u32_le`, `bytebuf_patch_u32_be`, `bytebuf_patch_u64_le`, `bytebuf_patch_u64_be` (`(b, at, v) -> Bool`: a field written before its value was known), `bytebuf_to_bytes(b)` (a copy of exactly the bytes written). An index outside the bytes written is `None` or `false`, never a trap |
| `@std/ordmap` | (R1e, S3; stable since R1 closed (0.10.0)) The `Ord` trait — `compare(self, other) -> Int`, negative, zero or positive, a total order — implemented for every integer type, `Bool` (`false` first) and `Str` (byte order, a prefix first); `OrdMap[K: Ord, V]`, an AVL tree in vectors shared like any handle, and `OrdEntry[K, V] { key, value }`: `ordmap_new[K, V]()`, `ordmap_len`, `ordmap_set -> Bool` (whether it replaced), `ordmap_get -> Option[V]`, `ordmap_at -> V` and `ordmap_at_or(m, key, fallback) -> V` (R1, 5b.7: what `m[k]` and `m[k] += n` read), `ordmap_has`, `ordmap_remove -> Option[V]`, each `O(log n)` comparisons; `ordmap_first`, `ordmap_last`, `ordmap_floor(m, key)` (the greatest key not after it), `ordmap_ceiling(m, key)` (the least not before it), each `-> Option[K]`; `ordmap_keys -> Vec[K]`, `ordmap_values -> Vec[V]`, `ordmap_entries -> Vec[OrdEntry[K, V]]`, all in ascending key order, and `ordmap_range(m, lo, hi)`, the entries with `lo <= key < hi`. A removed entry's storage is taken by the last, so nothing removed stays alive |
| `@std/bitset` | (R1e, S3; stable since R1 closed (0.10.0)) `BitSet`, a growable set of non-negative `Int`s, a bit each in 64-bit words shared like any handle: `bitset_new()`, `bitset_add -> Bool` (whether it was new), `bitset_remove -> Bool` (whether it was there), `bitset_has`, `bitset_count`, `bitset_clear`, `bitset_next(s, from) -> Option[Int]` (the least member at or after `from`: a walk without allocation), `bitset_items -> Ints` (ascending), `bitset_union`, `bitset_intersection`, `bitset_difference` (each a new set), `bitset_equal` (members, not words). A negative member is never added nor held, never a trap |
| `@std/test` | (N84) `test_int(what, actual, expected) -> Result[Int, Str]`, `test_str(what, actual, expected) -> Result[Str, Str]`, `test_true(what, cond) -> Result[Bool, Str]`; (Gate 2) `assert_true`, `assert_eq_int`, `assert_eq_str`, `assert_some`, `assert_ok`, each answering what a `@test` function returns, and `property(seed, cases, check)` |
| `@std/hash` | (Gate 2) the `Hash` trait — `hash(self) -> UInt64` and `same(self, other) -> Bool`, two keys that are `same` hashing alike — implemented for every integer type, `Bool` and `Str`; `hash_bytes(s)` (64-bit FNV-1a over the bytes, then SplitMix64's finaliser), `hash_u64(x)` (the same over a word's eight bytes, least significant first), `hash_combine(a, b)`, `hash_finish(x)`. A fixed function: the same in every implementation and every run. Floats are not keys (a NaN is not equal to itself) |
| `@std/hashmap` | (Gate 2) `HashMap[K: Hash, V]`, open addressing with linear probing, shared like any handle: `hashmap_new[K, V]()`, `hashmap_with_seed[K, V](seed)`, `hashmap_single(key, value)` (R1.5, D5: a map holding one entry, where a map literal begins), `hashmap_len`, `hashmap_set -> Bool` (whether it replaced), `hashmap_get -> Option[V]`, `hashmap_at -> V` and `hashmap_at_or(m, key, fallback) -> V` (R1, 5b.7: what `m[k]` and `m[k] += n` read), `hashmap_has`, `hashmap_remove -> Option[V]`, `hashmap_keys -> Vec[K]`, `hashmap_values -> Vec[V]`. Insert, lookup and removal take expected constant time, growth amortised constant time; iteration is in table order, which the keys, their order of insertion and the seed decide — the same in every run, and unpredictable to whoever chooses the keys only when the seed is (`Slot`, the table's entry until R1, is kept in the interface and no longer used); and (R1.5, D10) `hashmap_top[K: Ord, V: Ord](m, n) -> Vec[(K, V)]`, the `n` entries with the largest values, largest first, a tie by key ascending (`counts.top(10)`) |
| `@std/hashset` | (Gate 2) `HashSet[K: Hash]` over a `HashMap`: `hashset_new[K]()`, `hashset_with_seed[K](seed)`, `hashset_add -> Bool` (whether it was new), `hashset_has`, `hashset_remove -> Bool`, `hashset_len`, `hashset_items -> Vec[K]` |
| `@std/deque` | (Gate 2) `Deque[T]`, a ring buffer, shared like any handle: `deque_new[T]()`, `deque_len`, `deque_push_back`, `deque_push_front` (amortised constant time), `deque_pop_front -> Option[T]`, `deque_pop_back -> Option[T]`, `deque_get(d, i) -> Option[T]` (constant time, from the front) |
| `@std/ioerror` | (Gate 2) `IoErrorKind` (`NotFound`, `PermissionDenied`, `AlreadyExists`, `WouldBlock`, `InvalidInput`, `TimedOut`, `Interrupted`, `UnexpectedEof`, `ConnectionRefused`, `ConnectionReset`, `AddressInUse`, `AddressNotAvailable`, `BrokenPipe`, `NotConnected`, `IsADirectory`, `NotADirectory`, `DirectoryNotEmpty`, `Closed`, `Cancelled`, `Other`) and `IoError { kind, code, message, context }`: `io_kind_of(status)` — each platform family's table from error number to kind, here and nowhere else — `io_error_from(status)`, `io_error_new(kind, message)`, `io_context(e, doing)` (contexts read outermost first), `io_result(status) -> Result[Int, IoError]`, `io_kind_name(k)`, `io_error_show(e)`; and (§13) `io_error_from_errno(errno)` for what `c_errno()` reads |
| `@std/file` | (Gate 2) `File`, `FileKind` (`Regular`, `Directory`, `Other`) and `Metadata { size, kind, modified_ns, permissions }`; each `! { io }` and answering `Result[_, IoError]`; what names a path takes the `IoCap` that allows it, and what uses a `File` takes only the file: `file_open`, `file_create`, `file_append`, `file_create_new`, `file_open_rw`, `file_read(f, buf)`, `file_read_rest`, `file_read_all(io, path) -> Bytes`, `file_write` (every byte, however many writes it takes), `file_write_str`, `file_seek`, `file_seek_end`, `file_position`, `file_sync`, `file_close`, `file_metadata`, `file_exists_at -> Bool`, `dir_list` (sorted by bytes), (R1e, S1; stable since R1 closed, 0.10.0) `file_walk(io, dir) -> Result[Strs, IoError]` (every regular file, directory and symbolic link under `dir` at any depth, `dir` joined to each with `/`, sorted by bytes, never `dir` itself; an entry that is none of them, or is gone when looked at, is left out; since R1d a **symbolic link is listed and never followed**, as D4 approved — a link to a directory is not walked into, and a dangling or looping one is listed like any other), `dir_create`, `dir_create_all`, `dir_remove`, `file_remove`, `file_rename`, `file_replace_atomic(io, path, data)` (a new file beside it, written, put on storage, renamed over it); and (R1d, D4 and P2; stable since R1 closed (0.10.0), over *The platform, further*) `file_stdout(io) -> File`, `file_stderr(io) -> File`, `EntryKind` (`Regular`, `Directory`, `Symlink`, `Other`), `LinkMetadata { size, kind, modified_ns, permissions, device, inode }`, `file_symlink_metadata(io, path)` (a final link not followed), `file_set_permissions(io, path, mode)`, `file_canonical(io, path) -> Result[Str, IoError]`, `dir_create_temp(io, parent) -> Result[Str, IoError]` (an `Err` of kind `Other` when none could be made, since asking the system why would make the path the file system's), `file_lock(f, exclusive)`, `file_try_lock(f, exclusive) -> Result[Bool, IoError]` (`false` while another holds it) and `file_unlock(f)` |
| `@std/net` | (Gate 2) `TcpListener`, `TcpStream`, `UdpSocket`, `Datagram { size, from }`; each `! { io }`; what names an address takes the `NetCap` that allows it, and what uses a socket takes only the socket: `tcp_listen(net, addr)`, `tcp_accept`, `tcp_connect(net, addr, timeout_ms)`, `tcp_read`, `tcp_read_exact` (`UnexpectedEof` if the peer shuts first), `tcp_write` (every byte), `tcp_write_str`, `tcp_set_timeout`, `tcp_listener_set_timeout`, `tcp_shutdown`, `tcp_close`, `tcp_listener_close`, `tcp_local_addr`, `tcp_peer_addr`, `tcp_listener_addr`, `udp_bind(net, addr)`, `udp_send_to(net, u, data, addr)`, `udp_recv_from`, `udp_set_timeout`, `udp_local_addr`, `udp_close`, `net_resolve(net, name, port) -> Strs` |
| `@std/binary` | (Gate 2) `Writer` and `Reader` over `Bytes`, the same bytes for the same values everywhere: `binary_writer()`, `binary_put_u8`, `binary_put_u16_le`, `binary_put_u16_be`, `binary_put_u32_le`, `binary_put_u32_be`, `binary_put_u64_le`, `binary_put_u64_be`, `binary_put_i64_le`, `binary_put_i64_be`, `binary_put_f64_le`, `binary_put_f64_be`, `binary_put_varint` (LEB128), `binary_put_zigzag`, `binary_put_raw`, `binary_put_bytes` and `binary_put_str` (a varint length first), `binary_len`, `binary_finish`; `binary_reader(data)`, `binary_get_u8`, `binary_get_u16_le`, `binary_get_u16_be`, `binary_get_u32_le`, `binary_get_u32_be`, `binary_get_u64_le`, `binary_get_u64_be`, `binary_get_i64_le`, `binary_get_i64_be`, `binary_get_f64_le`, `binary_get_f64_be`, `binary_get_varint`, `binary_get_zigzag`, `binary_get_raw(r, n)`, `binary_get_bytes`, `binary_get_str` — each a `Result[_, Str]` naming what was short or wrong and the byte where — `binary_position`, `binary_remaining` |
| `@std/env` | (Gate 2) `env_get(io, name) -> Option[Str]`, `env_vars(io) -> Strs` (`NAME=value`, the environment's order) — each `! { io }` |
| `@std/error` | (Gate 2) `Error { message, context: Vec[Str] }`: `error_new(message)`, `error_context(e, doing)`, `error_from_io(e)`, `error_io_result[T](r: Result[T, IoError]) -> Result[T, Error]`, `error_show(e)` (what the program was doing, outermost first, then what went wrong) — each pure |

**Stability: 1.0 (N84).** `library/std/API-1.0` lists every public item of every standard module
with its declaration; a test regenerates it and requires equality. Within 1.x an item may be
**added**, never removed or changed — not its signature, its effects or its documented behaviour.
A removal or a change is **2.0**; if the language ever needs an edition, it is how a program would opt
into 2.0. A standard module's interface hash says when it changed, and no semantic epoch moves for it.
Not in 1.0, deliberately: networking (no network authority exists), cryptography (only an audited
external implementation would do), floating point (the language has none).

### Equality is derived — `==` on the value types that can have it
<!-- id: equality -->

Settled by N13, 2026-09-25. Built-in `==` and `!=` apply to two operands of **one** type,
and only when the language defines equality for that type. For a user-defined type it is
not declared, written or opted into: it is **derived** from what the type contains.

| type | built-in equality |
| --- | --- |
| `Int`, `Bool` | yes — the same value |
| `Str` | yes — the same bytes (*Strings are bytes*). A shared backing makes nothing equal, and separate backings make nothing unequal |
| a record | yes exactly when **every field's** type has equality |
| an enum | yes exactly when **every payload field of every variant** has equality |
| a concrete instance of a generic record or enum — `Box[Int]`, `Option[Str]`, `Result[Int, Str]` | derived as above, over its fields **after substitution** |
| `Ints`, `Strs`, `Vec[T]`, `Chan` | **no** — a handle's equality would have to mean the same storage or the same contents, and the language has chosen neither |
| an unconstrained type parameter `T` | **no** — `==` is not valid for every type, and there is no constraint that could say "for these types only" |

A type with no equality is refused where it is compared, with `N0304`, and the message names
the first component responsible: `Outer`'s `inner.values` is a `Vec[Int]`.

**One type, nominally.** Two records with identical fields are two types and cannot be
compared, nor can `Box[Int]` and `Box[Str]`, `Option[Int]` and `Option[Str]`, or `A[Int]`
and `B[Int]`. Nothing converts, implicitly or otherwise.

**What it means.** Two records are equal when every corresponding field is equal under that
field type's equality — two records built separately with equal fields are equal, because
a record has no identity. Two enum values are equal when they hold the **same variant** and
that variant's payload fields are pairwise equal; different variants are unequal, and their
payloads are not compared. A zero-payload variant is equal to itself, and an inactive
variant is never inspected. `!=` is the negation of `==`, always.

**Decided from the type, never from the value.** An enum with one variant that cannot be
compared cannot be compared at all: given `enum Work { Empty, Values(value: Vec[Int]), }`,
`Work.Empty() == Work.Empty()` is refused, because `==` is checked where the active variant
is not known and a guarantee about a type is about every one of its values. This is the
rule task safety follows, for the same reason.

**Generic bodies need a requirement.** A concrete `Box[Int]` compares, and without one
`fn same[T](a: T, b: T) -> Bool { a == b }` is still refused at its definition, as is
`fn same_box[T](a: Box[T], b: Box[T]) -> Bool { a == b }`: inside the body `Box[T]` has a
field of an unconstrained type, so it has no equality either. Nothing is inferred, deferred to
an instantiation, or accepted because some caller would pass a comparable type. *(N14: a
definition that declares `T: Equality` may compare both — see* A type parameter may require
equality *below.)*

**`Option` and `Result` have no rule of their own.** They are the core prelude's ordinary
generic enums, so `Option[Int]` and `Result[Int, Str]` compare and `Option[Vec[Int]]` does
not, by the rule above.

**Order is not meaning.** Declaration order of fields, of variants and of payload fields is
not part of what a type means, and not part of what its equality means either.

**Evaluation.** The left operand is evaluated, then the right, each once, and then they are
compared. An operand that leaves — returns, breaks, continues or propagates — leaves no
comparison behind, and what the other side already built is released on the way out.
Comparing observes: a bound operand is still usable afterwards, and a temporary operand is
released once the answer exists. Comparing is pure, so the order in which fields are
compared is not observable, and an implementation may stop at the first unequal one.

**What equality is not.** Not declared: there is no `derive`, no `Eq` and no trait. Not
customisable: a type cannot define its own `==`. Not identity: no value's address, owner or
handle is compared. Not ordering: `< <= > >=` remain `Int`-only. Not hashing. *(Corrected
2026-10-12, R1a: ordering and hashing are derived too — see* Ordering and hashing are derived*;
`Box` and `Chain` compare by what they hold.)* And not
defined on sequences, vectors or channels, deliberately, by the rule that stops a record
containing one from comparing.

### A type parameter may require equality
<!-- id: equality-requirement -->

Settled by N14, 2026-09-25. A generic function may state that one of its type parameters
ranges only over types that have equality:

```nazm
fn same[T: Equality](a: T, b: T) -> Bool {
    a == b
}
```

**The law.** A type parameter may carry an explicit equality requirement, written
`T: Equality`. Inside that definition the parameter has built-in equality, and so does every
composite whose equality derives from it by the rule above — `Box[T]`, `Option[T]`,
`Result[T, E]` when both `T` and `E` require it. Every type argument, written or inferred,
must satisfy the requirement by the same rule: a concrete type satisfies it exactly when it
has equality, and a caller's own type parameter exactly when *its* declaration requires it.
Requirements are checked statically, and they create no user-defined equality: `==` means
what *Equality is derived* says it means, for every type, with or without one.

| | |
| --- | --- |
| **Syntax** | `[T: Equality]` — one requirement per parameter, no list. `Equality` is the only one. It is not a type and nothing declares it: it means something only after the `:`, so no record, enum or import can shadow it, and a record named `Equality` is an ordinary record |
| **Identity** | A requirement belongs to the parameter — its owner and position — never to its name. Renaming `T` changes nothing, in the program or in its interface |
| **The body** | Checked once, against what the requirements guarantee. Nothing is inferred from what the body does: an unconstrained `T` still has no equality, and `fn same[T](a: T, b: T) -> Bool { a == b }` is still refused (`N0304`) |
| **The arguments** | Each is checked when the call is: written or inferred, `N0364` names the argument and the component responsible. `same(v, v)` for a `Vec[Int]`, `same(Work.Empty(), …)` for an enum with a `Vec` in any variant, and `same(o, o)` for an `Option[Vec[Int]]` holding `None` are all refused — a requirement is satisfied by a type, never by a value |
| **Forwarding** | A generic caller may pass its own parameter to a bounded callee only if its own declaration requires the same: `fn wrap[T: Equality](a: T, b: T) -> Bool { same(a, b) }` is accepted, the same without `: Equality` is `N0364` |
| **Nominality** | Unchanged. Two parameters that both require equality are still two types, and `a: A == b: B` is refused |
| **Where** | Only on a function's parameters. A requirement on a record's or an enum's parameter is refused (`N0101`, not supported yet) rather than parsed and ignored |
| **Interfaces** | A public function's requirements are part of what it offers. They are persisted by position (`nazm.interface/5`), an importer is held to them when checking against the interface alone, and adding or removing one moves the interface's fingerprint |
| **At run time** | Nothing. A requirement is a fact for the checker: no dictionary, no descriptor and no test is emitted, and an instance of `same` at `Int` is the same instructions as a hand-written `same_int` |

**What this is not.** Not a trait: there is no declaration of a capability, no `impl`, no
method, and nothing a program can implement or override. Not an inference: the requirement is
written where the parameter is declared, so a body edit cannot change which callers are
valid. Not a second equality: a bound proves that an operand's type is in `==`'s domain and
says nothing about what `==` does there.

## Settled for v0.2 — mutation and iteration
<!-- id: v0-2 -->

### Traits and methods — N78
<!-- id: traits -->

A **trait** names a set of methods a type may implement. An **impl** says that one type implements
one trait, and gives each method's body. A **method call** is resolved, by the checker, to exactly
one function. `architecture.md` §7.79 is the design record.

```nazm
trait Show {
    fn show(self: Self) -> Str;
}

struct Point { x: Int, y: Int, }

impl Show for Point {
    fn show(self: Point) -> Str { str_concat(int_to_str(self.x), int_to_str(self.y)) }
}

fn twice[T: Show](x: T) -> Str { str_concat(x.show(), Show.show(x)) }
```

| Rule | |
| --- | --- |
| **Declaring** | A trait is a list of signatures, each ended by `;`, whose first parameter is `self: Self` (`N0609`). Since R1b a signature may instead end in a default body (below); until R1b a body in a trait was a syntax error. A trait shares the type namespace with records and enums, and `pub` exports it |
| **Impls of generic types** (R1b, L11) | `impl[T: Show, U] Show for Pair[T, U] { … }`: an impl for every instance of a generic record or enum, written for the type applied to **exactly** its parameters, in order (`N0604` for anything else — `Pair[T, Int]`, a type that is not generic). Each parameter may require `Equality` or a trait. The impl applies to an instance — `Pair[Int, Str]` — whose arguments meet those requirements; a method call, UFCS, and a bound (`[T: Show]` given `Pair[Int, Str]`) all use it, each method run at the instance's arguments. Inside, `Self` is the type applied to the impl's parameters. Coherence and the orphan rule are the definition's: one impl of a trait per generic definition (`N0600`), in the trait's module or the definition's (`N0601`). A built-in generic type (`Vec[T]`, `Box[T]`, `Chain[T]`, `Cell[T]`) has none |
| **Default bodies** (R1b, L11) | `fn m(self: Self, …) -> R ! {e} { … }` in a trait: what an impl that defines no `m` gets. The body is checked **once**, as a function generic over `Self` that requires the trait — so it may call the trait's other methods on `self`, and nothing else about `Self` — against the signature's declared effects (none written is none). An impl may still define `m`, which then replaces it for that type. A call on a type whose impl leaves `m` to the default, directly, by UFCS or through a bound, runs the default at that type |
| **Implementing** | `impl Trait for Type { fn … }`, or `impl[T…] Trait for Type[T…] { fn … }` since R1b (below), defines every method of the trait that has no default body exactly once, and any that has one at most once (`N0602`), each with the trait's signature with `Self` replaced by `Type`, effects included, and its first parameter named `self` (`N0603`). `Self` may be written for the type inside the impl. A method that writes no effect set has its trait's. `Type` is a non-generic record or enum, a numeric type, `Bool` or `Str` (`N0604`) |
| **Coherence** | A compilation has at most one impl of a trait for a type (`N0600`). An impl is in the module declaring the trait or the one declaring the type, and for `Int`, `Bool` and `Str` only the trait's (`N0601`) |
| **Calling** | `recv.m(args)` finds the traits **in scope** — declared in this module, or exported by a module it imports — that declare `m`, keeps those with an impl for the receiver's type in this module or a module it imports directly, and needs exactly one (`N0605` for none, `N0606` for several). `Trait.m(recv, args)` names the trait (UFCS), and may be qualified, `alias::Trait.m(recv)`. An impl's methods are not in the function namespace: `show(p)` is not a call of one |
| **Bounds** | `[T: Trait]` lets a generic body call the trait's methods on a `T`, and every call site's argument for `T` must have an impl, as for `Equality`. One requirement per parameter |
| **What a call means** | The impl's function, called with the receiver first. Through a bound, the impl of the type the generic function was instantiated at |
| **Not here** | A trait as a type (`N0607`): no trait objects, no dynamic dispatch. A method as a value, `x.show` (`N0608`). Generic traits and methods, associated types and constants, supertraits, impls for one instance of a generic type (`impl Show for Pair[Int, Str]`) and inherent impls are refused. Impls with bounds and for generic types arrive with R1b (above) |
| **Unchanged meanings** | `E.A()` is a variant wherever `E` is an enum. `Name.m(…)` with unlabelled arguments, or with `Name` a value, was refused before N78, so no program that checked changed meaning. `r.f(1)` with `f` a field of function type is still refused: bind it first |

A trait method's effects are a promise: an impl's must equal them, so a call through a bound exercises
no more than the trait declares. For provenance, a call through a bound is of unknown origin joined
with its arguments, as a call through a function value is.

### Mutation is opt-in and visible at the binding
<!-- id: mutation -->

```nazm
let x = 1;        // immutable
let mut n = 0;    // mutable
n = n + 1;        // assignment is a statement
x = 2;            // error: `x` is not mutable
```

Immutable by default, `mut` to opt in. The alternative — mutable by default with a `const`
opt-out — puts the annotation on the rarer case and makes the common one silent, so a
reader has to check every binding to know whether a later line can change it.

Assignment is a **statement**, not an expression. It has no value, so `if (x = 1) { … }`
does not parse. That is one of the oldest C bugs and there is nothing to gain by
reproducing it.

**Parameters are not mutable**, and `fn f(mut x: Int)` is refused by name. A mutable
parameter is a local copy, which looks like an out-parameter to anyone who has not checked
the calling convention. A `let mut` inside the body says the same thing without the
ambiguity.

Assignment does not change a binding's type: `n` declared from an `Int` stays `Int`.

**Compound assignment (R1, 5b.12; stable since R1 closed (0.10.0)).** `place op= e`, for `op` one of
`+ - * / % & | ^ << >>`, means `place = place op e`, with every rule of the operator and of the
assignment: `n += 1` needs `let mut n`, overflows as `n + 1` does, and is a statement. Each of
`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=` is one token, so `n + = 1` is not
one. The place is read as an expression, so an index in it is a name, a field of one or a
number — `v[i] += 1` — and one that would be computed twice (`v[next()] += 1`) is refused by
name: bind it first.

### `while` is a statement, and there is no loop value
<!-- id: while -->

```nazm
fn sum_to(n: Int) -> Int {
    let mut total = 0;
    let mut i = 1;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    total
}
```

A loop has no natural value. The two ways to give it one are a unit type or an arbitrary
rule, and a unit type is a real decision with consequences for function returns and block
tails — too large to make in passing for the sake of `while`. So a block is *statements
followed by a tail expression*, and `while` is a statement. The tail is still required.

The condition must be `Bool`. There is still no truthiness.

### One block, and completion decided by checking
<!-- id: blocks -->

A block is **statements followed by an optional tail expression**. There is one `Block`
and one `if`; whether either produces a value is determined when checking, not by which
syntactic form was parsed.

The tail is simply *an expression not followed by `;`*:

```nazm
{ let x = 1; x }      // tail: the block's value is `x`
{ let x = 1; x; }     // no tail: `x;` is a statement, the block has no value
{ if c { a() } }      // tail: the `if`'s value is the block's
{ if c { a(); } d }   // the `if` is a statement; `d` is the tail
```

A block-like expression — `if`, `while`, a bare `{ … }` — needs no `;` in statement
position, so `if c { x = 1; }` followed by more statements reads naturally.

An earlier version had two of everything: an expression block and a statement block, an
`if` expression and an `if` statement, and a function that scanned forward balancing braces
to guess which `if` it was looking at. That guess had a case it got wrong, and every
construct added afterwards would have needed its own place in it. **This is the replacement
and the scanner is gone.**

Checking gives each block one of three completions, which are internal — the language has
no `unit` or `never` type to write:

| completion | meaning |
| --- | --- |
| **value** | falls off the end with a value of some type |
| **unit** | falls off the end with no value |
| **diverges** | never falls off the end: every path `return`s, `break`s or `continue`s |

A function body must complete with a value of the declared type, or diverge. An `if` used
where a value is wanted must have both branches, and they must agree — unless one diverges,
in which case the other decides. Unreachable code is a statement after one that diverges.

**Where nobody wants a value (R1, 5b.19; stable since R1 closed (0.10.0)).** A loop body's last
line may produce a value, which is discarded as `;` would discard it — until R1 that was
refused as a missing `;`, which with lines ending statements is no longer a mistake. And an
`if` that is a statement — or the last line of a loop body, or of a branch of such an `if`,
which `else if` makes of every link of a chain — may have branches that end differently, in
values of different types or one in none: it produces nothing, as an `if` without `else` does.
Where a value is wanted the rule above is unchanged, and so is its error.

### Conveniences for the first program
<!-- id: conveniences -->

*R1, 5b.16 and 5b.17; stable since R1 closed (0.10.0).* Each is a spelling of something the language
already had, and means exactly that.

| Written | Means |
| --- | --- |
| `let x: T = e` | `e` checked against `T` (`N0300` otherwise), and the binding is a `T`: `let o: Option[Int] = None` |
| a generic call whose arguments leave a type parameter open, where its result is wanted as a type | the parameter settled from that type: `let v: Vec[Str] = vec_new()`, `vec_new()` as a function's `Vec[Int]` result. The arguments decide first; where neither does, `N0354` |
| `[a, b, c]` where a `Vec[T]`, an `Ints` or a `Strs` is wanted | that sequence, made and pushed in order — `[]` an empty one — each element checked against its element type; anywhere else a list too since R1.5 (D6, *Shorter forms*), and an array only where `[T; n]` is expected |
| `s + t` on two `Str`s (5b.6) | `str_concat(s, t)`; so `s = s + t` and `s += t` append in linear time as `s = str_concat(s, t)` does (*What this costs*). `+` on a `Str` and anything else is still `N0304` |
| `/* … */` | a comment, which may span lines and nests; one the file ends inside is `N0007` |
| `loop { … }` | `while true { … }` |
| `while let P = e { … }` | `while true { match e { P => { … }, _ => { break } } }`: `e` evaluated afresh each time |
| `o?` on an `Option[T]`, in a function returning an `Option[U]` | `match o { Some(v) => v, None => return None }`: `T`, or `None` leaving the function; in a function returning anything else, `N0361` as a `Result`'s `?` there is |
| `impl Point { fn area(self: Point) -> Int { … } }` | the function `point_area(self: Point) -> Int` — the type's name in lower case, then the method's — which `p.area()` reads by name (*Methods and indexes by name*); an impl without a trait has no other effect |

### Shorter forms (R1.5)
<!-- id: shorter-forms -->

*R1.5 (`openspec/changes/r1-5-performance-and-dx`, *design.md*); EXPERIMENTAL until R1.5 closes.*
Each is a spelling of something the language already had, and means exactly that.

| Written | Means |
| --- | --- |
| `fn fib(n: Int): Int { … }` (D1) | `fn fib(n: Int) -> Int { … }`: a result type is written after `:`, as a parameter's is — in a function, a trait method, a function type (`fn(Int): Int`) and a function expression (`fn(n: Int): Int { … }`). `->` is still read and is removed at 1.0; the effect set follows the type either way, `fn read(io: IoCap): Str ! {io}` |
| `fn double(n: Int): Int = n * 2` (D2) | `fn double(n: Int): Int { n * 2 }`: a body that is one expression, ended where a statement ends |
| `\|a, b\| a < b`, `\|n: Int\| n + 1`, `\|\| work()` (D3) | `fn(a: A, b: B): R ! {e} { a < b }`: a closure whose unwritten parameter types, result type and effect set are the function type's expected where it is written — an argument, a typed `let`, a field. Where none is expected, each parameter writes its type (`N0635`), the result type is the body's, and the effect set is empty. `{ … }` may be its body. Captures are a function expression's |
| `Shape.Rect(3, 4)`, `Point(1, 2)` (D4) | `Shape.Rect(w: 3, h: 4)`, `Point(x: 1, y: 2)`: every field given by position, in the order declared. Positional and named are not mixed in one construction or pattern, and a positional list gives every field (`N0334`) |
| `Circle(2)`, `Dot` (D4) | `Shape.Circle(2)`, `Shape.Dot`: a capitalised name before `(` — or alone — that no function in scope has is a record of that name, or else the variant of the one enum in scope that has it; where two enums do, `N0636` names both. A function in scope keeps its name: `Box(x)` calls a function `Box` as it always did. A built-in type's `Int(x)` is its conversion. In a pattern, `Circle(r)` and `Dot` are the variants of the value's own enum |
| `{"a": 1, "b": 2}`, `Map[Str, Int]` (D5) | `{ let m = hashmap_single("a", 1); hashmap_set(m, "b", 2); m }`: a map literal — a `{` whose first key, a literal or a name, is followed by `:` — builds a `HashMap` whose key and value types its first entry settles; each key and value is evaluated in the order written. `Map[K, V]` is `@std/hashmap`'s `HashMap[K, V]`, so `m[k]`, `m[k] = v` and `m[k] += 1` work on it; both need the hash map in scope, which the prelude gives a program's module (D9). An empty map is `hashmap_new()` |
| `use std.json` (D7) | `use "@std/json"`: a standard module named by its dotted path; a quoted path is still a file, and `as name` works on either |
| `fn greet(name: Str) { println(f"hello {name}") }` (D8) | the same function taking `out: OutCap` and its callers passing it: in a module of the program itself — not a standard module, the prelude, or a package's — every function holds the output authority `main` holds (option C), so printing needs nothing passed. A package's function still takes `out`, so what a dependency can touch stays in its signature; files, the network, processes, spawning, randomness and time are passed as before (`N0369`) |
| `List[Int]` (D7) | `Vec[Int]`: `List[T]` names the growable list, `Vec[T]`, and is the name taught; `Vec[T]`, `Ints` and `Strs` keep working and are the same type, which a message shows as before — `Ints`, `Strs`, `Vec[T]` — and never as `List`. A prelude name, as `Box` is: a module's own type named `List` keeps its meaning there. R1a's immutable sequence, which held the name until R1.5, is `Chain[T]` (*Immutable indirection*) |
| `[1, 2, 3]` (D6) | a `List[Int]`, the growable list (`Vec[Int]`): a list literal is a list wherever no fixed array `[T; n]` is expected — a binding without a type, an argument or field of a list or generic type — its element type the first element's, every other checked against it (`N0300`). A fixed array is written where its type says so, `let a: [Int; 3] = [1, 2, 3]`, and `[v; n]` is always one. **This changes meaning**: an unannotated `let a = [1, 2, 3]` was an array, copied on assignment, and is now a list, shared by every name for it. An empty `[]` has no first element to take a type from and needs its type written, `let v: List[Int] = []` (`N0618`, as an empty array is) |
| `s.split(" ")`, `hashmap_new[Str, Int]()`, `a.compare(b)`, `r.unwrap_or(1)` with no `use` (D9, 4.5) | the same with `use "@std/text"`, `use "@std/hashmap"`, `use "@std/ordmap"` or `use "@std/result"` written: in a module of the program itself (as D8 says), the prelude imports `@std/hash`, `@std/hashmap`, `@std/option`, `@std/ordmap`, `@std/result`, `@std/sort` and `@std/text` — each one the module names: an item of it by name, its `Map`, a function the parser wrote for a form (a map literal's `hashmap_single`), or a method `x.m(…)` one of its functions `<prefix>_m` answers and no built-in does, an interpolation's holes included — and with it the prelude modules it imports, whose impls its signatures need. A name the module declares, or a written import brings in, hides the prelude's, silently; a written `use` of one of them is an ordinary import. A method one of them answers names it whatever the receiver's type: `x.unwrap_or(0)` names both `@std/option` and `@std/result`, and the checker calls the one the receiver's type is (*Methods and indexes by name*). A module that names none of them imports none, so a restriction profile, which holds a program to everything it imports, sees what it saw. A package's and a standard module's imports are only those written |
| `counts.top(10)`, `counts.sort_by(\|a, b\| a.1 > b.1)` (D10) | `hashmap_top(counts, 10)`: the `n` entries with the largest values as `(key, value)` tuples, largest first, a tie by key ascending, both keys and values `Ord` (`@std/ordmap`); fewer when the map holds fewer, none for `n` below 1. And `hashmap_sort_by(counts, less)` (`@std/sort`): every entry ordered by `less`, stably over the map's order |
| `match s {` and its arms each on a line, with no `,` (4.6) | the same arms with `,` after each: a line break after an arm separates it from the next, as it ends a statement (*`else if`, and a line break for `;`*, 5b.19). The rule is that one, in the same words: a missing `,` is accepted when the next arm begins a new line, and a line break ends nothing the parser can continue — an arm's expression may still span lines, so a next line that begins with a binary operator continues it, and an arm before a pattern that begins with `-` (a negative literal) writes its `,`. Two arms on one line still need it (`N0100`). `,` stays correct everywhere, and the two may be mixed; `nazm fmt` keeps whichever was written |
| `fn add(a: Int, b: Int) = a + b` (D2) | the same function with its result type the expression's (`Int`), settled before any caller is checked. A `pub` function writes its result type, since its signature is its interface; so does a method, and a function whose body reaches itself, directly or through others; and an expression with no value has none to give (each `N0634`) |

### Methods and indexes by name
<!-- id: named-methods -->

*R1, 5b.15; stable since R1 closed (0.10.0).*

**A method no trait gives is a function by name.** `x.m(args)`, where no trait in scope gives
`x`'s type a method `m`, is the call `<prefix>_m(x, args)` — the prefix the type's name in lower
case (`vec`, `ints`, `strs`, `bytes`, `str`, `int`, `hashmap`, a record's own), and for a `Str`
`text` after `str` — the first whose function is in scope. So `v.len()` is `vec_len(v)`,
`v.push(3)` is `vec_push(v, 3)`, `s.split(" ")` is `text_split(s, " ")` where `@std/text` is
imported, and `m.get(k)` is `hashmap_get(m, k)`. It is that call in every respect — its checks, its
effects, its authority, its errors, named as the function — and a trait's method of the name
wins. Where neither exists it is `N0605`, as before.

**`xs[i]` and `xs[i] = v` on a sequence.** On a `Vec`, an `Ints`, a `Strs` or a `Bytes`, `xs[i]`
is `vec_get(xs, i)` (`ints_get`, `strs_get`, `bytes_get`) and `xs[i] = v` is `vec_set(xs, i, v)`:
an index outside the sequence fails with `N0405`, as the call does. A sequence is a handle
(*Sequences are shared*), so the write needs no `mut`, as `vec_set` does not; `xs[i] += 1` is
`xs[i] = xs[i] + 1`. Only a binding's own element is written so — `p.items[i] = v` is still
written `vec_set(p.items, i, v)` — and an array's element keeps its rule (*Arrays*).

**`m[k]`, `m[k] = v` and `m[k] += n` on a map** *(R1, 5b.7)*. On any other type the prefix is the
type's name in lower case, as for a method, and a read is `<prefix>_at`: on a `HashMap` or an
`OrdMap`, `m[k]` is `hashmap_at(m, k)` (`ordmap_at`), the value `m` holds for `k`, and where it
holds none it fails with `N0405`, as an index outside a sequence does; `m.get(k)` still answers an
`Option`. `m[k] = v` is `hashmap_set(m, k, v)`, its answer discarded, with no `mut`. A compound
update reads the entry with `hashmap_at_or(m, k, zero)`, so an absent entry starts from its value
type's zero — `0` for an integer, `0.0` for a float, `""` for a `Str` — and `counts[w] += 1`
counts; a map whose value type has no zero reads it with `_at`, and the entry must be there. The
key of a compound update is a name, a number or a string, so it is read once in effect.

### Short variants
<!-- id: short-variants -->

*R1, 5b.14; stable since R1 closed (0.10.0).*

**An enum's construction infers its type arguments.** `Option.Some(value: 3)` is an
`Option[Int]`: where none are written, a construction of a generic enum takes them from the
type it is wanted as — a parameter's, a function's result, a typed binding's — and otherwise
from its payload, each field declared as one of the enum's parameters giving that parameter its
value's type. Where neither decides one, as for `Option.None()` with nothing around it, it is
`N0352` as before, and writing them stays correct everywhere.

**The prelude's variants have short spellings.** `Some(x)` is `Option.Some(value: x)`, `None` is
`Option.None()`, `Ok(x)` is `Result.Ok(value: x)` and `Err(e)` is `Result.Err(error: e)` — in an
expression and, with a pattern for `x` or `e`, in a pattern:

```nazm
fn half(n: Int) -> Option[Int] {
    if n % 2 == 0 { Some(n / 2) } else { None }
}

fn main(out: OutCap) -> Int ! {io} {
    match half(8) {
        Some(h) => print(f"{h}\n"),
        None => print("odd\n"),
    }
    0
}
```

`Some`, `None`, `Ok` and `Err` are read so wherever a name is, as `print` is a built-in's.

**A variant with no payload needs no `()`.** `Color.Red` is `Color.Red()`, in an expression and
in a pattern. In an expression a capitalised name followed by `.` and a capitalised name is read
so — as types and variants are spelled, and bindings and fields are not.

### `else if`, and a line break for `;`
<!-- id: line-breaks -->

*R1, 5b.11 and 5b.19; stable since R1 closed (0.10.0).*

**`else if`.** `else` may be followed by another `if` (or `if let`) instead of a block:

```nazm
let size = if n < 10 { "small" } else if n < 100 { "medium" } else { "large" };
```

It means exactly `else { if … }` — the inner `if` is the else-block's tail — so its value,
its completion and every rule about it are that block's. The grammar and the guide had
written this form since 2026-09-21 while the parser refused it; a chain no longer costs two
levels of nesting an arm.

**A line break may stand for `;`.** Where a statement must end with `;` — after a `let`, an
assignment, an expression statement, `return`, `break`, `continue`, `spawn`, an import and a
constant — a line break before the next token ends it instead:

```nazm
fn main(out: OutCap) -> Int ! {io} {
    let total = 12 * 3
    let message = f"total: {total}"
    if total > 30 { print("large\n") } else if total > 10 { print("medium\n") }
    print(message)
    0
}
```

The rule is only that: a missing `;` is accepted when the next token begins a new line. A
line break is not a token and ends nothing the parser can continue, so an expression may
still span lines — `let x = a` followed by `+ b` on the next line is one statement, `x = a + b`
— and the tail of a block is still *an expression not followed by `;`*: the last line before
`}` is the block's value, as it is with `;` written out. A statement before its block's `}` on
the same line needs no `;` either (`if done { break }`). A member's name is on its `.`'s line, so
a line ending in `.` is unfinished rather than joined to the next; and a call's `(` and an index's
`[` are on the line of what they call or index, so a line beginning with `(` or `[` begins a new
statement — `let (a, b) = p` followed by `(b, a)` is two lines, not the call `p(b, a)`. Writing
`;` stays correct
everywhere, and the two may be mixed; `nazm fmt` keeps whichever was written.

### `break` and `continue`, and the `if` they force
<!-- id: break-continue -->

```nazm
while i < n {
    if skip(i) {
        i = i + 1;
        continue;
    }
    if done(i) { break; }
    total = total + i;
    i = i + 1;
}
```

Both are **statements**, they affect the **innermost** enclosing loop, and there are no
labels. `continue` re-evaluates the condition.

#### Where a loop target binds

**A loop's condition is not inside that loop.** A `break` in a `while`'s condition leaves
the *enclosing* loop; a `continue` there starts the enclosing loop's next iteration. In the
outermost loop's condition there is no enclosing loop, so it is `N0312`.

*Moved here from `roadmap.md` on 2026-09-21: it is a rule about what a program means, so a
plan was the wrong owner — and three source files were citing the plan for it.* The rule
was read out of the implementations rather than assumed: `check.rs` raises `loop_depth`
around the body only, and `eval.rs` evaluates the condition outside the loop's own flow
handling, so a `Flow::Break` raised there is consumed by the enclosing loop. Two probe
programs pin it — a `break` in an inner loop's condition prints `102` and a `continue`
there prints `204`, both only explicable if the target is the outer loop. The native
backend pushes a loop's targets around its **body and not its condition**, and asserts the
same two numbers.

They are only legal inside a loop body, and **only in statement position**. `break` is not
an expression and has no value, so `let x = break;` and `if c { break } else { 1 }` do not
parse. Rust gives `break` the never type and makes this work; a never type is a real
addition with consequences for every other inference rule, and it is not worth adding so
that `break` can sit in an expression it has no business being in.

`while c { if d { break; } }` works because a block's tail is optional: `{ break; }`
completes by diverging, which is not the same as completing with no value, and neither is
an error unless something wanted a value from it.

### `return`
<!-- id: return -->

```nazm
fn classify(n: Int) -> Str {
    if n < 0 {
        return "negative";
    }
    if n == 0 {
        return "zero";
    }
    "positive"
}
```

`return expression;` leaves the enclosing **function**, not merely the enclosing loop, and
its value must match the declared return type. A function whose every path returns needs no
tail expression.

**`return;` with no value is not supported** in a function with a result type. It would need
a unit type to mean anything, and that is a decision this language has not made. Refused by
name. In a function written with no result type it returns (below).

#### A function with no result type
<!-- id: no-result -->

*R1, 5b.2; stable since R1 closed (0.10.0).*

```nazm
fn greet(out: OutCap, who: Str) {
    println(f"hello, {who}")
}
```

A function written without `-> T` returns nothing. Its body's last value, if it has one, is
discarded as a statement's is; `return` alone leaves it; and a call of it is a statement: using
one where a value is wanted is `N0300`, in its own module and in an importer's, which reads it
from the interface. There is still no unit type — the function is `Int` underneath, returning
`0` — so nothing below the checker is new, and a function value of it is an ordinary `fn() -> Int`.
A foreign declaration and a trait method still write their result type.

A function's value is still its final expression; `return` is for the paths that leave
early, which is the case it earns its keep on.

**`return` is a statement, but a block is an expression**, so a `return` may sit inside
anything used as a value:

```nazm
let x = if n > 0 { return 1; } else { 2 };
let y = g(if n > 0 { return 7; } else { 2 });
while (if n > 0 { return 4; } else { i < 3 }) { … }
```

Each of these leaves the function. The enclosing expression has no value and never will,
so evaluation of it stops where the `return` was raised: in `1 + if c { return 5; } else
{ 2 }`, the left operand has already been evaluated and the addition never happens. The
same applies to `break` and `continue` in a value position, and to a `return` nested
inside another — the inner one leaves first, so the outer never gets a value to return.

This follows from the checker rather than being a separate rule: a diverging branch takes
the other branch's type, which is what makes these programs well-typed in the first place.
An evaluator that refused them would be rejecting programs the type system accepts.

### Code after `break`, `continue` or `return` is an error
<!-- id: unreachable-code -->

```nazm
while c {
    break;
    i = i + 1;   // error: this can never run
}
```

An error rather than a warning, because this language has no warning severity yet and
silence is the wrong default for a statement that provably cannot execute. Relaxing an
error to a warning later is a compatible change; tightening a warning to an error is not,
so the strict choice is the reversible one.

### `nazm run` compiles; `nazm run --interpret` interprets (R1.5)
<!-- id: run-compiles -->

*R1.5 (`openspec/changes/r1-5-performance-and-dx`, *design.md* D11); EXPERIMENTAL until R1.5 closes.*

`nazm run FILE` builds the program with the Cranelift backend, in process, into a private
temporary directory, runs the executable with the arguments after `--`, and ends with its exit
status. The program's standard input, output and error are its own — nothing `nazm` would print
about the build (the executable's path, the authority line, the checker's warnings) is printed,
and nothing the program writes is buffered, reordered or rewritten. A program killed by a signal
ends `nazm run` with `128 +` that signal's number, as a shell reports it. The temporary directory
is removed whatever the program did; the project's object store (`.nazm/`, or `NAZM_CACHE_DIR`'s)
is used and filled exactly as `nazm build` uses and fills it.

`nazm run --interpret FILE` is the interpreter. It is the oracle every equivalence test compares
the backends against, and it is what this document means wherever it contrasts `nazm run` with a
native build — its iteration and depth limits (below), its threads, its refusal to call C.

**Neither falls back to the other.** A program only a native build refuses — outside the backend's
subset (`N0101`), or a generic function whose instances would never end or are too many (`N0357`,
`N0358`) — is refused by `nazm run` with that diagnostic, help naming `nazm run --interpret`,
status 1 and nothing run. A program that does not check is refused as the interpreter refuses it,
and its diagnostics name no `--interpret`, which would refuse it too.
A program both accept means the same under both, and what the program writes, the status it ends
with and the variables it reads (`NAZM_RESULT_REPORT`, the memory and scheduler reports) are the
same. `--max-iterations` and `--max-depth` bound the interpreter only, and are a usage error
(status 5) without `--interpret`; `--json`, `--profile`, `--features` and `--locked` mean what
they mean to `nazm build`.

### The iteration and depth limits are the interpreter's, not the language's
<!-- id: limits -->

With no `break` and no I/O, a non-terminating program produces nothing at all. A tool that
hangs is worse than one that reports, so `nazm run --interpret` stops and says so — after a
configurable number of loop iterations (`--max-iterations`, default 10,000,000) or nested
calls (`--max-depth`, default 512).

Both are needed, and one does not imply the other: `fn f(n: Int) -> Int { f(n) }` never
loops, so the iteration budget never sees it. Unbounded, it exhausts the host stack and
kills the process with no diagnostic at all — the one failure a tool cannot explain.

**A bound only means something against a known stack**, and a library cannot know its
caller's: macOS gives the main thread 8 MiB, a spawned thread 2 MiB, and the test harness
2 MiB per test. So the interpreter runs the whole pipeline — parsing, checking, evaluating
and dropping the tree — on a thread whose stack it sets itself, and **sizes that stack from
the budget it is about to enforce**: `24 MiB + max_depth × 32 KiB`. Raising `--max-depth`
raises the reservation with it, so the guarantee holds above the default rather than only
at it. A machine that will not grant the reservation says so (`N0404`) instead of failing
part-way in, and a budget past `1,000,000` is refused with the figure it would have cost.

The per-call figure is measured, not guessed: about 6.6 KB per nested call for ordinary
code on a debug build and about 17 KB for deliberately heavy frames, doubled to 32 KiB. A
test recurses to the bound and requires a diagnostic, so a construct that made frames dear
enough to close the gap fails the suite rather than surfacing as an abort in a terminal.

### Nesting is bounded at parse time, which bounds every pass after it
<!-- id: nesting -->

Call depth is not the only way to run out of stack. `((((1))))`, `- - - - 1` and
`{ { { 1 } } }` recurse through the parser, the checker, the evaluator **and the destructor
of the tree itself**, without a single call frame in the program — so a call-depth budget
never sees them, and `nazm check` never reaches the evaluator at all.

**The syntax tree may be 512 deep** (`N0102`), measured as it is built: every node
records `1 + max(children)`, so the bound is on the tree the later passes walk rather than
an estimate of it. Parsing is the only pass that can report rather than abort, so that is
where the limit lives. Reported once per item, not once per level.

A *second* bound, the same size, applies to the parser's own recursion, because the parser
is a stack consumer too and the two are not the same measurement. Parentheses build no
node — `((((1))))` is a tree of depth 2 however many there are — so only the recursion
bound constrains them. Conversely `1 + 1 + …` is parsed by a loop that never recurses, and
only the tree bound constrains it.

Getting this wrong is easy and was got wrong first. Counting parser recursion alone looked
sufficient, and was not: in `- - - 1 + 1 + 1` the operand is parsed and its levels released
*before* the chain of `+` starts counting, so the peak count is the larger of the two while
the tree is as deep as both together. A test that measured the tree rather than the counter
found one accepted at 1022 against a limit of 512, and composing the shapes would have
compounded it further. The bound is now exact because it is taken from the nodes
themselves; the tests check the deepest tree accepted for every pairing of seven shapes,
each pair probed at its own edge rather than somewhere comfortably inside it.

512 is far past anything written by hand. It is not far past what a generator emits: a
dispatch chain written as `else { if … }` — or, since R1, as `else if`, which means the same —
costs two levels an arm.
A limit tight enough for a 2 MiB stack would have been about 64, which is a real constraint
on real code — so the stack is raised instead, and `parse_source` states the requirement it
cannot enforce. The worst shape measured is nested blocks, at about 14 KB per level; the
tests parse the deepest tree the parser accepts, for four shapes, on exactly the documented
stack, finding each shape's edge by asking the parser rather than by counting levels.

Neither budget is a language rule. Nazm bounds neither iteration nor recursion, and the
nesting limit is the parser's; this implementation has all three so that a runaway program
fails visibly instead of consuming a machine. A different implementation may choose
differently.

#### Native recursion, and the stack it runs on — N49
<!-- id: native-recursion -->

*Until N49 this section was "Why native recursion is refused rather than counted", and its
argument still holds:* a call-depth counter alone does not guarantee a diagnostic before the
native stack is exhausted, because whether that many frames fit depends on frame size and on
how much stack a thread actually has. The interpreter pairs its counter with a stack it sizes
from the counter (`--max-depth`, `N0403`). **`nazm build` now pairs a measurement with the stack
it measures**, and recursion — direct and mutual, across modules and packages — compiles under
both native backends.

| | |
| --- | --- |
| **What is checked** | The stack itself, not a count. On entry to every function that is in a call cycle (a cycle of calls; `spawn` starts a task on its own stack and closes none), the frame's address is compared with the thread's limit. A function whose own frame is a page or more — large arrays, records of them — is checked whether or not it is in a cycle, and **before** its frame exists: it is entered through a check that the whole frame fits above the limit, since a prologue lays out and probes the frame before any statement runs (v1, Q1-A1-01) |
| **The limit** | Computed once per thread, at its first check: that frame's address minus three quarters of the smaller of the stack the thread has left below it and 8 MiB. The stack is asked of the thread itself — its own bounds, `RLIMIT_STACK` only where a system will not give them — so a thread C started, calling an export, is budgeted from the stack it has (v1, Q1-A1-04). Task threads are created with 8 MiB, so no thread's budget exceeds its stack. A task on the pool — the default since N106 — has its own `NAZM_TASK_STACK` (256 KiB unless set) and a limit a quarter of the way up it, saved and restored at every switch, so deep recursion in a task meets `N0408` sooner there and is reported the same way |
| **The stack asked for** (R1b, L14) | `nazm build --stack-size N` (bytes, or `K`, `M`, `G`; 64 KiB to 4 GiB; hosted targets only) runs `main` on a thread of `N` bytes — the process's own stack where that thread cannot start — and makes `N` the stack of every task thread, the default of every pool task's (`NAZM_TASK_STACK` still overrides it), and the size the limit above is computed from in place of 8 MiB. A program that recurses deeply asks for the stack it needs rather than meeting `N0408`. Stable since R1 closed (0.10.0). `nazm run --interpret` sizes its threads from `--max-depth` |
| **What a program sees** | `N0408`, *the native call stack is exhausted*, at the function entered, and then the failure unwinds like any other: what the frames own is released, an enclosing `scope` joins its tasks, and the process exits 2 with the diagnostic |
| **What is guaranteed** | Unbounded recursion meets a check, and so does a frame larger than the stack left. A chain of non-recursive calls between two checks, each frame smaller than a page, is bounded by the program's call graph but its stack is not measured: the guarantee holds while such a chain fits in the quarter kept as headroom (2 MiB at 8 MiB, 64 KiB on a pool task's 256 KiB), and a program whose chain is deeper can still be killed by the operating system |
| **What is not** | Tail calls are ordinary calls: no tail-call optimisation is performed or promised. Recursion depth is not bounded by the language, and the depth at which `N0408` arrives depends on frame sizes, the backend and the optimisation level |
| **The interpreter** | Unchanged: its limit is its call-depth budget, `N0403`. The two implementations agree on what a terminating program produces; each reports its own limit, and neither limit is a language rule |
| **The compiler written in Nazm** | Compiles recursion since N102 with the same check: `nz.stack_check`, derived from the reference runtime and gated by `cargo xtask check-runtime`, on entry to every function in a call cycle or calling through a value, failing `N0408` at the function's name |

**The general rule, of which these are instances: a runtime resource limit is an
implementation contract, never a silently invented language rule.** Where an
implementation imposes one it is stated, named in diagnostics, and recorded as that
implementation's choice — not promoted into the semantics because it happens to hold.

**`nazm build` is that different implementation, and it has chosen none of them.** A
compiled program has no iteration budget and no call-depth budget: a loop whose condition
never becomes false runs until something kills it, with no `N0402`. This is stated here so
that the difference is part of the record rather than a discovery. The two implementations
agree on what a *terminating* program produces; they do not agree on what a runaway one
does, and no test claims otherwise. Recursion was refused outright by both compilers until N49;
`nazm build` now compiles it with a measured stack guard, and since N102 so does the compiler
written in Nazm — see *Native recursion, and the stack it runs on*, below.

#### A bounded heap — R1d (D5)
<!-- id: heap-limit -->

Stable since R1 closed, 0.10.0 (openspec `r1-language-for-the-rewrite`, D5). A native program can
be built to hold at most a stated amount of heap, so a runaway one ends with a diagnostic instead
of exhausting the machine — the mechanism a toolchain built from Nazm runs its own stages under.

| | |
| --- | --- |
| **Asking for one** | `nazm build --heap-limit N`, or `heap-limit = "N"` under `[package]` in the root package's manifest (*Packages*); the flag overrides the key. `N` is bytes, or a number with `K`, `M` or `G` (KiB, MiB, GiB), from 64 KiB to 1024 GiB; anything else is a usage error (`N0500` in a manifest). A dependency's `heap-limit` is not read |
| **What is counted** | Every byte the program's own code and its runtime ask the allocator for and have not given back — strings, sequences, `Bytes`, closures, channels, tasks' records, a handle table, standard input's buffer — as the bytes asked for, with no allocator's rounding or bookkeeping, so the count is the same on every host. Not counted: a task's stack (its own mapping, sized by *Native recursion*), the program's image and static data, and what C allocates for itself |
| **Passing it** | An allocation that would take the live count past `N` does not happen: the program writes `nazm: error[N0406]: out of memory: the heap limit of N bytes is reached` (with `N` in bytes) and a newline to standard error and exits with status 2 at once — from whichever task asked, without unwinding, joining or running a `defer`, and without the memory report. If several tasks pass it together, one writes the message. An allocation the host itself refuses below the limit is `N0406` at its call site, as without a limit |
| **Without one** | Nothing changes: a program built without `--heap-limit` and without the key allocates as before, through the C library directly, and its runtime is the one it always was |
| **Where** | A hosted native program's. A board's heap is its manifest's arena (*Boards*) and a WebAssembly module's its host's; a library's (`--lib`, `--shared`) is its C host's; and a runtime artifact (`--runtime`) was built without the count — each is refused as a usage error (status 5) when a limit is asked for. `nazm run --interpret` has no heap limit: the interpreter's memory is its host's |
| **What is not promised** | The count includes what the runtime allocates for the program — a growing sequence's spare room, a task's record — so the limit a program needs is measured, not read off its source, and a later runtime may need more or less of it. That a given allocation passes a limit is the same under both native backends at every optimisation level only because they lower the same LIR; a program near its limit is not promised to end at the same point after a change to the runtime |

### Strings are bytes, and what that commits to
<!-- id: strings -->

`Str` is an **immutable sequence of bytes**. Not of characters, and not of Unicode scalar
values. The decision is forced rather than chosen: Nazm has no character type, and
inventing one to index strings with would be a second text abstraction added for the sake
of the first.

The commitments, each stated because leaving one to the host language's behaviour is how
two implementations end up disagreeing:

| | |
| --- | --- |
| **Length** | `str_len(s)` is the number of **bytes**. `str_len("é")` is 2 |
| **Indexing** | `str_byte(s, i)` is the byte at offset `i`, as an `Int` in `0..=255`. Never negative, never a code point |
| **Bounds** | `str_byte` fails with `N0405` unless `0 <= i < str_len(s)`. There is no wrapping, no clamping, and no sentinel return |
| **Slicing** | `str_slice(s, a, b)` is the bytes in `[a, b)`, and fails with `N0405` unless `0 <= a <= b <= str_len(s)`. `a == b` is the empty string, and `a == str_len(s)` is allowed |
| **UTF-8 boundaries** | A slice **may** split a multi-byte sequence. The result is a `Str` whose bytes are not valid UTF-8, and that is not an error. Validity is a property of *some* strings, not of the type |
| **Embedded NUL** | A `Str` may contain zero bytes, and they are ordinary bytes. Length comes from the length, never from a terminator |
| **Equality** | `==` and `!=` compare byte sequences. Equal length and equal bytes. No normalisation, no case folding, no collation |
| **Ordering** | `< <= > >=` do **not** apply to `Str`. Ordering text needs a collation decision this language has not made |
| **Concatenation** | `str_concat(a, b)` is the bytes of `a` followed by the bytes of `b` |
| **Conversion** | `int_to_str(n)` is the decimal form, with a leading `-` when negative. `int_to_str(0)` is `"0"`, and the full `Int` range round-trips |
| **Construction** | `str_from_byte(b)` is a one-byte `Str`, and fails with `N0405` unless `0 <= b <= 255`. Since N49 a literal may write a byte below 128 with an escape (*String escapes*, below); `str_from_byte` remains the way to make one above 127 that UTF-8 source does not already spell |
| **Declassification** | `str_vouch(s)` is `s` with no origin, needing a `VouchCap` held (N52, *Declassification*) |
| **Building** (R1b, D2) | `s = str_concat(s, t);` on a `let mut` binding appends in place when `s`'s storage is its own and has room after its bytes, and otherwise copies into storage of twice the length — so a string built by a loop of appends takes time and allocations linear and logarithmic in its length, not quadratic and linear. Nothing a program can observe differs: the bytes are `str_concat`'s, and a string sharing the storage keeps its own. Only storage an append made grows in place, so the interpreter and a compiled program allocate the same strings. stable since R1 closed (0.10.0) |
| **Allocation** | `str_concat`, `int_to_str` and `str_from_byte` allocate (and since R1a `str_pad_start`, `str_pad_end`, `float_to_str_fixed` and an interpolation, *String interpolation*); `str_slice` and `str_vouch` do not. A compiled program reports `N0406` if the allocator refuses. The interpreter cannot: its host aborts on allocation failure, and that difference is stated rather than papered over |
| **Ownership** | Values, not references. Passing, returning, binding and copying a `Str` produce a string with the same bytes, and there is no way to observe whether a copy was made |
| **Mutation** | There is none. No operation changes an existing `Str` |

Source positions stay byte offsets throughout — `Span` is a byte range, diagnostics point
at byte ranges, and `str_len` counts the same units. A compiler written in Nazm therefore
reports positions that mean the same thing as the ones this implementation reports,
without a conversion step that could be got wrong.

#### String escapes — N49
<!-- id: string-escapes -->

| Escape | Byte | | Escape | Byte |
| --- | --- | --- | --- | --- |
| `\n` | 10 | | `\0` | 0 |
| `\t` | 9 | | `\\` | 92 |
| `\r` | 13 | | `\"` | 34 |
| `\xHH` | `0xHH`, for `HH` in `00`–`7F`, either case | | `\u{H…}` | the UTF-8 encoding of the scalar value `H…` (N76) |

Each escape is **one byte**. Everything else in a literal is its own bytes, so `"é"` is still the
two bytes of its UTF-8 encoding. A backslash followed by anything else, `\x` with fewer than two
hexadecimal digits, and `\x80`–`\xFF` are refused (`N0004`) at the escape. `\xHH` stops at `7F`
because a literal's bytes are UTF-8 text throughout this implementation and `str_from_byte` already
makes any byte. `"\0"` is a one-byte string, as *Embedded NUL*
above says any NUL is. Diagnostics point at the bytes written, and the formatter keeps a literal
exactly as written. **Changed meaning:** before N49 a backslash in a literal was itself, so
`"a\nb"` was four bytes; it is now three.

**Unicode escapes — N76.** `\u{H…}` is one to six hexadecimal digits (either case) naming a Unicode
scalar value, and it stands for that value's UTF-8 encoding: one to four bytes, exactly the bytes the
same character written directly in the (UTF-8) source would give. So `"\u{e9}" == "é"`, and both are
two bytes. The encoding decision was already made when a literal's bytes were specified as its source
text's UTF-8 bytes. The escape adds an ASCII spelling and no new value: an invisible or
bidirectional character (`\u{200B}`, `\u{202E}`) can be written visibly. Refused (`N0004`, at the
escape): no `{`, no digits (`\u{}`), more than six digits, a non-hexadecimal digit, no closing `}`, a
surrogate (`D800`–`DFFF`) and a value above `10FFFF`. `\xHH` keeps its limit of `7F`. A `\x80`
meaning one byte would contradict `\u{80}` meaning the two bytes `C2 80`, so a lone byte above 127 is
still made only by `str_from_byte`. The compiler written in Nazm refuses every escape, this one
included (its subset, *Bootstrap*).

#### Multi-line and raw strings — R1
<!-- id: string-forms -->

Settled by R1a, 2026-10-12 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L9).

```nazm
fn template() -> Str {
    """
    define i64 @answer() {
      ret i64 42
    }
    """
}

fn pattern() -> Str { r#"a "quoted" \d+"# }
```

**Multi-line strings.** `"""`, a line break, the lines, and `"""` alone on a line (after any
indentation). The closing delimiter's indentation — the spaces and tabs before it — is taken off
every line, each of which must begin with exactly that indentation unless it is blank (`N0005`);
the line breaks between the lines are the value's, and the one after the opening delimiter and the
one before the closing one are not. So an indented template is written indented with the code
around it. Escapes are an ordinary literal's (*String escapes*), and a backslash ending a line
joins it to the next: neither the line break nor the next line's indentation is in the value. Text
after the opening `"""` on its line is refused (`N0005`), and a `"""` that is not alone on its line
is three quotes of the text.

**Raw strings.** `r"…"`, or `r#"…"#` with any number of `#`s, the same number at each end: the
text as written, with no escape — a backslash is a backslash — ending at the first quote followed
by that many `#`s, so `r#"say "hi""#` holds quotes. A raw string may span lines. `r` begins a raw
string only when a quote or a `#` follows it at once, so a name `r` is unchanged.

**In both**, a line break written CR LF is one LF — a literal means the same bytes from every
editor — and the bytes are otherwise the source's, exactly as an ordinary literal's are. The
formatter keeps either as written. An ordinary literal stays on one line: a quote with no closing
one on its line is still refused there (`N0002`), so an unclosed string cannot swallow the rest of
a file.

#### String interpolation — R1
<!-- id: interpolation -->

Settled by R1a, 2026-10-12 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L4).

```nazm
use "@std/show";

struct Point { x: Int, y: Int, }
impl Show for Point { fn show(self: Point) -> Str { f"({self.x}, {self.y})" } }

fn report(name: Str, count: Int, ratio: Float64, at: Point) -> Str {
    f"{name:<8}|{count:>5}|{ratio:.2}|{at}|{{literal braces}}"
}
```

**Form.** `f"…"`, or `f"""…"""` laid out as *Multi-line and raw strings* says: the text between holes
is an ordinary literal's, escapes and all; `{{` and `}}` are one brace each; and `{expr}` or
`{expr:spec}` is a **hole**. A hole's expression is any expression, its braces and string
literals nested freely; a lone `}` in the text, a `{` with no closing `}`, or an empty hole is
refused (`N0006`). The `f` is a prefix only when a quote follows it at once, so a name `f` is
unchanged — as `r`'s is.

**What a hole may hold**, and how it is written:

| Value | Written as |
| --- | --- |
| `Str` | itself |
| an integer type | its decimal digits, `-` first when negative (`num_to_str`) |
| a float type | the shortest digits that read back to it (`num_to_str`), or exactly `p` decimals with `.p` |
| `Bool` | `true` or `false` |
| a type with a `Show` impl in scope, or a type parameter bound by `Show` | its `show` (`@std/show`) |

Anything else is refused at the hole (`N0300`), naming its type: interpolation chooses no
rendering a program did not ask for.

**Spec.** `spec` is `[<|>][width][.precision]`. `width` is the fewest **bytes** the hole writes —
the value's text, then spaces, after it with `<`, before it with `>`; without either, a number goes
right and anything else left. `.precision` is for a float only (`N0006` otherwise): exactly that
many decimals, correctly rounded from the value's exact binary value, ties to even — C's `%.*f`.
A hole's text is computed as if by `str_concat`, left to right, each expression evaluated once, in
order; an interpolation is a `Str` like any other and allocates as `str_concat` does.

**New built-ins**, usable directly: `float_to_str_fixed(x, digits)` — `x` any float type,
`0 <= digits <= 30` (`N0405` otherwise) — and `str_pad_start(s, width, fill)` and
`str_pad_end(s, width, fill)`, which add copies of the one-byte `fill` (`N0405` otherwise) before
or after `s` until it is `width` bytes; a `s` already as long is returned as it is.

#### What an implementation may not infer from its host

The reference implementation is written in Rust, whose `String` cannot hold invalid UTF-8.
That is a property of *that host*, not of `Str`, and the specification above is written to
what a byte sequence can do. Where the two differ — a slice that splits a code point — the
specification governs and the Rust interpreter's representation is the thing that has to
accommodate it.

This is not hypothetical. The interpreter's `Value::Str` was a Rust `String` when this
section was written, and `str_byte(str_slice("é", 0, 1), 0)` produced **239** — the first
byte of the replacement character the lossy conversion substituted — where the
specification requires **195**. The test that asserts 195 was written from this section
and found the defect immediately; the representation is now `Vec<u8>`. A host type
standing in for an unstated specification is the trap, and it had already been walked
into.

#### Built-ins share the ordinary function namespace

`str_len`, `str_byte`, `str_slice`, `str_concat` and `int_to_str` are called like any
other function and checked by the ordinary call rule. **They may not be redefined**: a
program declaring `fn str_len` is rejected (`N0203`) rather than shadowing the built-in,
because a call whose meaning depends on what happens to be in scope is exactly what this
language is trying not to have.

### Sequences are shared, mutable, and never freed
<!-- id: sequences -->

`Ints` and `Strs` are growable sequences of `Int` and of `Str`. They are the first
**reference** types in the language, and every consequence of that is stated here rather
than discovered.

Named `Ints` and `Strs` rather than `[Int]` and `[Str]` on purpose: the lexer has no
bracket tokens and the type grammar is a bare name, so concrete names commit to nothing
about how a future generic `[T]` will look. A half-built `[T]` syntax would.

**One list type** *(R1, 5b.18)*. Since `Vec[T]` arrived, `Ints` is another name for `Vec[Int]`
and `Strs` for `Vec[Str]`: one type each, whichever name is written, so a `Strs` from
`text_split` is passed where a `Vec[Str]` is wanted, `vec_push(xs, 1)` grows an `Ints`, and
`ints_len(v)` measures a `Vec[Int]`. Diagnostics name the two by their shorter names, `Ints`
and `Strs`. Nothing else in this section changes: what it says of `Ints` and `Strs` is what
*`Vec[T]`* says of every vector.

| | |
| --- | --- |
| **Creation** | `ints_new()` and `strs_new()` produce a new, empty sequence |
| **Identity** | A sequence value is a **handle**. Binding, passing and returning one share the storage; nothing is copied. `let b = a;` makes `b` and `a` the same sequence, and a push through either is visible through both |
| **Mutation** | `ints_push`, `ints_set` and their `Strs` counterparts change shared storage. There is no immutable sequence and no copy operation |
| **Length** | `ints_len` / `strs_len`, in elements |
| **Sum** (N66) | `ints_sum_from(start, v)` (`@std/seq`'s `ints_sum(xs)` is its loop, and is vectorised the same way): `start` plus every element of `v`, left to right, failing with `N0400` exactly where one of those running sums would overflow — even if the total would fit. A native build replaces the loop `while i < ints_len(v) { s = s + ints_get(v, i); i = i + 1; }` (counter from `0`) with `s = ints_sum_from(s, v); i = ints_len(v);`, the failure at the loop's addition; `nazm run --interpret` keeps the loop |
| **Indexing** | `ints_get(a, i)` / `strs_get(a, i)`, failing with `N0405` unless `0 <= i < len`. `ints_set(a, i, v)` has the same bound and returns **the index written** — the language has no unit type, and the index is the answer that is always meaningful |
| **Appending** | `ints_push(a, v)` returns the **new length** |
| **Growth failure** | Reported as `N0406` where the implementation can detect it. A failed growth leaves the sequence exactly as it was, never half-resized |
| **Equality** | `==` and `!=` are **refused** on a sequence (`N0304`). They would have to mean either "the same storage" or "the same contents", and the language has chosen neither; offering one silently is how a program comes to depend on the wrong answer |
| **Ordering** | Not applicable, for the same reason it is not applicable to `Str` |
| **Destruction** | **There is none.** A sequence lives until the process exits |
| **Element copying** | `strs_get` yields a `Str`, which is a value — changing the sequence afterwards does not change a `Str` already read out of it |

#### What this is not

It is **not an ownership system, and no safety is claimed from it.** Nothing prevents
aliasing, nothing tracks lifetimes, nothing detects a sequence used after the code that
built it has finished, and nothing is ever reclaimed. Peak memory grows with everything
a program ever allocates.

That is a deliberate bootstrap trade (`docs/bootstrap.md` §1, *the memory model*): a
compiler reads input, writes output and exits, and reclaiming inside that lifetime buys
nothing while costing an ownership system the bootstrap does not otherwise need. It is a
**stated limitation with a cost that must be measured**, not a design anybody should
inherit. A later allocator with reclamation is intended to change none of the meanings
above.

> **Superseded in part by N7, 2026-09-22.** *"Never freed"* and *"a sequence lives until
> the process exits"* described every implementation up to that milestone. Sequence storage
> is reclaimed now, under the rule in *The memory constitution* below, and **none of the
> meanings above changed** — which is what the last sentence promised. Everything else in
> this section still holds: sequences are still shared, still mutable, still aliasable, and
> a push through one binding is still visible through every other.
>
> **Extended by N8, 2026-09-22.** The same rule now covers `Str` and `Chan` storage, so
> *nothing* in the current type universe is left to the process's exit. Two entries in the
> table above are worth reading again in that light. *Destruction* is no longer "there is
> none": a sequence's storage goes when the last reference to it dies, and a `Strs`
> releases every string in it first. And *element copying* — "changing the sequence
> afterwards does not change a `Str` already read out of it" — used to be true because
> nothing was ever freed. It is true now because `strs_get` hands back its own reference to
> the bytes, which is the same promise resting on something that will go on being true.

#### An expression may be a statement

`ints_push(a, 1);` is an expression evaluated for its effect, with its value discarded.
This is not new syntax — a block has always been statements and an optional tail — but
it is newly *useful*: without it, a language with no unit type would force
`let _unused = ints_push(a, 1);` at every mutation, which is a workaround for a missing
type rather than a rule worth having.

### The memory constitution: four kinds of value, and when storage is reclaimed
<!-- id: memory -->

Settled by N7, 2026-09-22, from an audit of what the two implementations actually do
rather than from what a memory model is usually shaped like. `docs/architecture.md` §7.7
carries the implementation and the evidence; this section is the law.

The whole of it is four kinds of value and five rules. Nothing here is new syntax, nothing
here is an annotation, and **no program that was legal before this section existed changed
its meaning because of it.** That was a requirement rather than an outcome: a memory model
that had to break aliasing to be implementable would have been the wrong model for a
language whose specification already promised aliasing.

#### The four kinds

| Kind | Types | Assignment produces | Storage |
| --- | --- | --- | --- |
| **Immediate** | `Int`, `Bool`; the other numbers and arrays of plain data (Gate 2) | an independent copy | none |
| **Shared immutable value** | `Str`; `Text` (Gate 2); `Box[T]`, `Chain[T]` (R1a) | a value with the same bytes; for `Box` and `Chain`, another reference to the same immutable value | reclaimed when the last value naming it dies; shared or copied **unobservably** until then |
| **Owned mutable handle** | `Ints`, `Strs`; `Vec[T]`; `Bytes` (Gate 2) | another reference to the same storage — for `Bytes`, to the same view of the same buffer | reclaimed when the last reference dies; a `Bytes` buffer when its last view dies |
| **Runtime-managed handle** | `Chan`; `Chan[T]` | another reference to the same channel | the runtime's, reclaimed when the last reference dies |

*Listed since v1 (Q1-M-01):* `Vec[T]`, `Chan[T]`, `Text` and `Bytes` were each in their own section
but not in this table; their rules were the rows they now sit in. Function values are counted
handles too (*Function values*, *Cycles*).

There are four because the language has four kinds of thing, not because four was a good
number. A fifth would need a value that is none of: a number, an immutable byte string, a
mutable container, or a synchronisation primitive.

#### The five rules

**1. Assignment never copies a handle, and never observably copies a `Str`.**
`let b = a;` makes `b` refer to what `a` refers to. For `Ints` and `Strs` that sharing is
observable and specified above. For `Str` it is not observable at all — *Strings are bytes*
already says passing, returning, binding and copying produce a string with the same bytes
and there is no way to tell whether a copy was made — so an implementation may share the
bytes, copy them, or intern them. The reference interpreter copies and the native backend
shares, and both conform.

**2. A parameter is borrowed.** A function may read a handle, mutate what it refers to, and
pass it on. It may not keep it after it returns, and the language gives it nowhere to keep
one: a handle can be stored only in a local binding, and a local binding dies with its
frame. Parameters are not mutable bindings (*Mutation is opt-in*), so a callee cannot even
rebind its own parameter. The caller's reference is unaffected by the call and is still
valid after it.

**3. Returning transfers.** A function that returns a handle hands its caller a reference
that is independent of anything the callee still held. The caller's binding is then one of
the references that keeps the storage alive.

**4. Storage is reclaimed when the last reference to it dies, and never before.** A
reference is a binding, an argument in a call that has not returned, or a value on its way
out of a `return`. An implementation may reclaim later than this and may not reclaim
earlier. It is deliberately not stated *how*: reference counting, an escape analysis, and a
region scheme all satisfy it, and `architecture.md` §7.7 records which one is implemented
and why.

**4a. An expression that never finishes still gives back what it had produced.** There are
four ways to leave one part way through — a run-time failure, `return`, `break` and
`continue` — and every one of them is an ordinary death for the values already in hand:
the arguments already evaluated, the fields already stored into a half-built record, the
payload already stored into a half-built variant, and the value a `match` is holding while
an arm runs. None of them is reachable by the program again, so each is reclaimed then and
not later. `E.Value(a: f(), b: if done { continue; } else { g() })` reclaims what `f()`
produced, on the turn that abandons it, and a value produced **before** a loop and wanted
after it is untouched by a `break` inside that loop.

This is where the milestone's scope is stated rather than implied:

| | |
| --- | --- |
| `Ints`, `Strs` storage | reclaimed |
| `Str` storage | reclaimed **since N8**. The line above read *"not reclaimed"* and gave the reason: a `Str` may point into a literal, into the process's arguments, into a heap buffer, or into the middle of any of those, and the value did not say which. It says which now — `architecture.md` §7.8 — and the four origins are classified rather than guessed at |
| `Chan` storage | reclaimed **since N8**, including its queue, its mutex and its condition variable |
| the interpreter | reclaims under the same rule, and models the same backing rather than inheriting the host's |

**5. What crosses a task boundary is decided by kind.** An immediate crosses by copy. A
`Str` crosses by immutable sharing, which is safe precisely because nothing can mutate it.
A `Chan` crosses by runtime-managed sharing, which is what it is for. A handle **does not
cross at all** — `N0321` refuses it, and that is unchanged.

That refusal is load-bearing rather than incidental. Because a handle cannot reach a second
task, its reference count is never touched by two threads at once, so reclaiming one needs
no atomic operation and no lock. A language rule that existed for data-race safety pays for
itself a second time here.

**And the converse, which N8 had to settle.** A `Str` and a `Chan` *do* cross, so whatever
keeps their storage alive is reached by more than one thread and must be safe under that. A
task can be handed a string, slice it, and bind the slice; two sibling tasks doing that to
the same bytes are two threads adjusting one thing. So the implementation pays for a
thread-safe mechanism there and does not pay for one on a sequence. Neither half is a
judgement about how likely sharing is — each follows from a rule the language already had,
and if `N0321` were ever relaxed the sequence half would have to change in the same commit.

What the *language* says is only this: **immutable string bytes remain valid for every live
`Str` value, and a channel's storage remains valid while any reference to it exists.** That
is a rule about validity, not about counting. `architecture.md` §7.8 records that counting
is what is implemented and why; a future arena, an interning pass, a small-string
optimisation or an escape analysis would satisfy the same sentence.

#### What this costs, stated so it can be checked

Nazm does not call something automatic and leave the reader to find out what it costs.

| Statement | May allocate | May copy | May adjust a reference count | May synchronise | May block |
| --- | --- | --- | --- | --- | --- |
| `let n = a + b;` | no | no | no | no | no |
| `let s = str_concat(a, b);` | **yes** | **yes** | no | no | no |
| `s = str_concat(s, t);`, `s` a `let mut` binding (R1b, D2) | **yes**, amortised | **`t` only**, amortised | no | no | no |
| `let xs = ints_new();` | **yes** | no | no | no | no |
| `let ys = xs;` | no | no | **yes** | no | no |
| `f(xs)` | no | no | no | no | no |
| `return xs;` | no | no | **yes** | no | no |
| `ints_push(xs, v)` | **yes**, on growth | no | no | no | no |
| a binding going out of scope | no | no | **yes**, if it holds one | no | no |
| `chan_send(c, v)` | no | no | no | **yes** | **yes** |
| `let t = str_slice(s, 1, 3);` | no | no | **yes** | no | no |
| `let b = a;` on a `Str` | no | **permitted, and not done** | **yes** | no | no |
| `strs_push(xs, s)` | **yes**, on growth | no | **yes** | no | no |
| `strs_get(xs, i)` | no | no | **yes** | no | no |
| `let c2 = c;` on a `Chan` | no | no | **yes** | no | no |
| `spawn f(s)` | **yes**, for the task's arguments | no | **yes** | **yes** | no |
| `let p = Point(x: 1, y: 2);` | no | no | no | no | no |
| `let q = p;` on a record with owning fields | no | no | **yes, one per owning field** | no | no |
| `p.x` where `x` is an `Int` | no | no | no | no | no |
| `p.name` where `name` is a `Str` | no | no | **yes** | no | no |
| `p.name = s;` | no | no | **yes**, one taken and one released | no | no |
| `let b = box_new(v);` (R1a) | **yes** | no | **yes**, if `v` owns something | no | no |
| `box_get(b)` | no | no | **yes**, if the value owns something | no | no |
| `let l = chain_from_vec(xs);` | **yes** | **yes**, the elements | **yes**, one per element that owns something | no | no |
| `chain_get(l, i)` | no | no | **yes**, if the element owns something | no | no |

Two entries in that table deserve their emphasis. **Passing a value to a function costs
nothing**, which is rule 2 turned into a number: the caller keeps its reference across the
call, so nothing has to be counted. And **the work happens only where something takes a
reference or lets one go** — a binding, a return, a container that keeps what it is given —
which is a place a reader can see and a tool can list.

Two rows say "yes" where a reader might expect "no". `str_slice` does no copying and still
adjusts a count, because the view it produces has to keep the bytes it points into alive.
And a `spawn` argument is held for the task rather than borrowed from the statement that
started it, because the task outlives that statement: a binding reassigned inside the scope
would otherwise release storage the task is still reading.

#### Cycles

A reference cycle is impossible in the language as it stands, and not by luck: a `Ints`
holds `Int`, a `Strs` holds `Str`, `Str` is a leaf, and a `Chan` carries `Int`. **No value
can refer to another value that can refer back.** So counting references reclaims
everything, and the leak freedom claimed for every current heap-backed type is a claim
about this type system rather than about counting in general.

A recursive user-defined type would end that, and the constitution says in advance what
admitting one requires: either the type system refuses ownership cycles, or the value kind
that can form them arrives with its own policy — a weak reference, a region, or a
collector for that kind alone. **What may not happen is a recursive type being added and
the reclamation claim left standing.**

**N9 took the first option, 2026-09-23.** A record holds its fields by value, so a
containment cycle has no finite layout and is refused outright (`N0336`) — which means the
paragraph above is satisfied by the type system rather than by a new policy, and the
argument is unchanged: a record adds no new *kind* of value, only a composition of existing
ones, and a composition of leaves is a leaf. The claim therefore now covers `Int`, `Bool`,
`Str`, `Ints`, `Strs`, `Chan` **and every record built from them**. What would end it is a
type that can refer to another value indirectly — a pointer, a reference, or a generic
container able to hold a record — and none of those exists.

**N11 added the third, and took the first option again, 2026-09-23.** `Vec[T]` holds its
elements through a reference-counted handle, so a definition can reach itself with a finite
layout — `struct Node { children: Vec[Node], }` — and a value of it could own itself. The
type system refuses every such definition (`N0359`) over the ownership graph of definitions
described in *Generics*, beside the impossible layouts `N0336` already refused. So the
argument holds with one more step: no definition can reach itself through what it owns, so
no value can, and counting still reclaims every value of every type a program can write —
now including every `Vec` and every generic record or enum applied to such types.

**N10 extended the refusal rather than the argument.** An enum holds its active payload by
value too, so records and enums form **one** inline-containment graph and a cycle anywhere
in it is refused by the same rule and the same code. An enum adds no new kind of value
either: it is a composition of existing kinds of which one is live at a time, and a choice
between leaves is a leaf. The claim now covers every record **and every enum** built from
the six built-in types. What would end it is unchanged and still absent.

**Gate 1 found a cycle that was not refused, 2026-10-07.** N50 added function values, and a
closure holds a handle to what it captures. A closure that captured a `Vec` whose elements can
be function values, and was then stored into that `Vec`, was a value that owned itself:

```nazm
let fs = vec_new[fn() -> Int]();
let f = fn() -> Int { vec_len(fs) };   // refused since Gate 1-C1: N0616
vec_push(fs, f);                       // `fs` would hold `f`, which holds `fs`
```

The checker accepted it and counting did not reclaim it: a native run's memory report ended with
that sequence and that closure live. *Cycles* had required in advance that a new value kind able
to form a cycle arrive with a policy, and N50 had supplied none.

**Gate 1-C1 took the first option a third time, 2026-10-07.** A closure is a node of the
ownership graph: its **environment** owns each value it captured, by that value's own law — a
copy of a value, a share of a handle, a share of another function value's environment. Three
facts make the rule small:

1. **An environment never changes after it is made.** What a closure captures is copied in when
   it is created, and nothing can be stored into an environment later. Its edges point only at
   values that already existed — so environments alone can never close a cycle. *R1b (L10):* a
   captured `let mut` binding is shared, not copied — the environment holds a `Cell` the binding
   lives in, a counted handle like any other, so fact 3 and the rule below cover it, and a binding
   whose type can hold a function value is still refused (`N0386`).
2. **An edge into an existing value is made only by storing into a counted handle** — `vec_push`,
   `vec_set`, a channel send. Records and enums are values: storing into one stores into a copy.
3. **A counted handle whose elements cannot be or contain a function value holds no
   environment**, and what it holds is finite and acyclic by `N0336` and `N0359`.

So the one rule needed is: **a closure may not capture a value that can hold a function value
behind a counted handle** (`N0616`) — a `Vec` whose element type is, or contains through record
and enum fields or further `Vec`s, a function type; or a record or enum with such a `Vec`
anywhere among its fields. With it, no environment reaches a container that can hold an
environment; from an environment only older environments and containers that hold none are
reachable; and a cycle through an environment would have to come back through one of those,
which none can. A cycle that runs through no environment is a cycle of definitions, refused by
`N0336` and `N0359`. **Every value of every type a program can write is again reclaimed by
counting**, environments included.

**R1a admitted recursive types, 2026-10-11, by the second option's discipline without its cost.**
`Box[T]` and `Chain[T]` (R1a's `List[T]`, renamed by R1.5) are a value kind that can form a definition cycle — and arrive with the
policy: they are immutable, so a value can refer through one only to values older than itself, and
the ownership rule refuses every definition cycle that runs through a mutable handle. The argument,
and the release that takes no stack per level, are in *Immutable indirection: `Box[T]` and
`Chain[T]`*, below.

The rule is decided by the captured binding's **type**, where the closure is written: not by
names, aliases or what the program later does with either value. `let alias = fs;` captured
instead of `fs` is refused the same way, two vectors each meant to hold a closure capturing the
other cannot be built, and a record carrying a `Vec[fn() -> Int]` field cannot be captured. It
refuses some programs that would never close a cycle — a closure that only reads a vector of
handlers — and that is the price of a rule a reader can apply without running the program:
pass such a vector to the function value as an argument instead of capturing it. Storing a
function value is never refused, wherever it came from — a named function has no environment,
and a closure was held to this rule where it was written, in whatever module — so a function
taking `fs: Vec[fn() -> Int]` and `f: fn() -> Int` may push `f` into `fs` without knowing what `f`
captured. Channels cannot carry a function value at all (`N0390`), so no channel is such a
container today; a future owning container is one by the same derivation over its element type.
No collector was introduced: counting stays the whole reclamation story, deterministic and with
no pause, and a future kind of value that needs cycles must arrive with its own policy.

#### What is not promised

- **Leak freedom for a whole program, as a permanent property.** Every heap-backed type
  the language *currently has* is reclaimed — function values and their environments included
  since Gate 1-C1 (*Cycles*, below) — which is a claim about `Int`, `Bool`, `Str`,
  `Ints`, `Strs`, `Chan`, the numbers and arrays, `Text`, `Bytes`, `Vec[T]`, `Chan[T]`, function
  values, and the records and enums composed of them, and about nothing else. It rests on the cycle argument below,
  which is a fact about this type system rather than about counting in general: the first
  recursive user-defined type ends it, and the paragraph after next says in advance what
  admitting one requires. `capability-matrix.md` states the scope per type rather than as
  one word.
- **Reclamation of storage the program did not allocate.** A string literal lives in the
  object's read-only data and a process argument belongs to the operating system. Neither
  is freed, neither is counted, and neither is a leak.
- **Deadlock freedom.** Unchanged, and still not promised.
- **A destructor.** There is no user-visible destruction, no RAII and no ordering a program
  can observe, because nothing a program can write runs at reclamation. A record is
  destroyed by releasing each of its fields, an enum by releasing the fields of its active
  variant, and the order between fields is deliberately unspecified — no program can observe
  it, and fixing it in the specification would be promising something the language has no
  way to let anyone use.
- **Zero-cost record or enum copying.** A record with many owning fields costs one reference
  adjustment per owning field, per copy; an enum costs a branch on its discriminant and then
  the same, for the active variant alone. The design leaves that optimisable — a copy whose
  source dies immediately after needs no traffic at all — and until it is measured, nothing
  here claims it is free. `docs/performance.md` carries the numbers.

### What the outside-world built-ins do and do not promise
<!-- id: outside-world -->

A program can read its arguments, read and write files, write to standard output and
standard error, and end the process. That is enough for a compiler, and it is all.

| | |
| --- | --- |
| **Arguments** | `args_count()` and `arg(i)`, **excluding the program's own name**. `arg(i)` fails with `N0405` outside `0..args_count()` |
| **Reading** | `read_file(path) -> Str` — the file's bytes, whatever they are. Fails with `N0407` |
| **Writing** | `write_file(path, data) -> Int` — truncating; returns the byte count. Fails with `N0407` |
| **Existence** | `file_exists(path) -> Bool` — whether it can be *opened for reading*, which is the question a program actually asks |
| **Output** | `print(s)` and `eprint(s)` write bytes to descriptors 1 and 2 and return the count. Neither adds a newline: `str_from_byte(10)` is how you ask for one. *(R1, 5b.4 and 5b.13; stable since R1 closed (0.10.0):* `print(x)` and `eprint(x)` take any value an interpolation's hole takes — a `Str`, a number, a `Bool`, a `Show` — and write its text, `print(x)` meaning `print(f"{x}")`; `println(x)` and `eprintln(x)` add a newline, `println(x)` meaning `print(f"{x}\n")`, and `println()` writes one. `println` and `eprintln` are built-in names, as `print` is.*) |
| **Ending** | `exit_with(code)` ends the process. It never returns, which the type system cannot express, so it is written `Int` and the value never arrives. **The status** (v1, Q1-B-05): the language's exit status is `0..=255`, and the host observes exactly that value — every host reports those unchanged. A `code` outside that range is not truncated by the host: it fails with `N0416`, *this exit status is outside 0 to 255*, and the process ends with status 2, as any failure does, in the interpreter and in every compiled program alike. Status 2 is therefore both a failure's and `exit_with(2)`'s; standard error tells them apart |

Excluding the program name from `arg(i)` is deliberate: a compiled program's `argv[0]` is
a path and an interpreted one has no equivalent, so including it would make the same
program mean different things under `nazm run` and `nazm build`. Excluded, they agree
exactly.

#### Authority is explicit where a contract is written, and ambient elsewhere

**Since N37 a function that declares an effect set may reach these built-ins only while it
holds an `IoCap`** — a capability value its scope reaches, handed down from `main`, which is
the one place authority enters a program (*Capabilities: what allows a function to act*). A
function that declares no set is still ambient: it exercises its caller's authority, which
is how every program written before N37 is accepted unchanged.

*Corrected 2026-09-30, N37.* This section was titled *Authority is ambient, and that is a
limitation*, and said any function could call `read_file` and there was no capability
parameter. That is no longer true for a function with a declared set, and it is still true
for one without — which was then a **compatibility bridge**, bounded so that code with a
contract never reaches authority it does not hold by calling code without one. *N104 retired
the bridge*: no function exercises its caller's authority (*No inherited authority*). The
limitation that remains is stated there, not hidden here: the checking is static, v1
capabilities are coarse, and nothing at runtime sandboxes a Nazm program.

*Corrected 2026-09-29, N36.* This said there was no effect annotation. There is one now
(*Effects: what a function may do*): a function may declare `! { io }`, and one declaring
`! {}` may not reach these built-ins, directly or through anything it calls. That restricts
**what a function does**, not **who may do it** — which is what N37 then added.

This is stated rather than implied because the alternative — describing the design as
capability-based wherever a capability design exists — would be a claim the implementation
does not support. Partial enforcement labelled as full enforcement is worse than none.

The growth path this section once left open — *a capability parameter on `main`, threaded to
whatever needs it* — is the one N37 took, and for a stated reason rather than because an
earlier note suggested it: `main` is the only function the runtime calls, so it is the only
place a value can come from that no expression produced. The built-ins were shaped so that
the capability changes *what may call them*, not what they mean, and no program's
behaviour changed when it arrived.

#### There is no process spawning, deliberately

A Nazm program cannot run another program. A compiler written in Nazm therefore emits
LLVM IR to a file and a **script** invokes `clang` — which is the documented arrangement,
and much narrower than a language-level `run_command` would be. Adding one would be the
single largest authority grant available and it is not needed.

#### Errors are values only where they can be

There are no exceptions, and the built-ins return no `Result`. A failed read stops the
program with `N0407`. A program that wants to *handle* a missing file asks `file_exists`
first and reports it itself, which is what a compiler wants to do anyway. The gap between
the check and the read is a real race, and for a compiler reading its own inputs it is an
accepted one.

*Corrected 2026-09-24, N12.* This said there was no error type and that closing the race
needed one, which was a later decision. N12 made it: `Result` exists (*Typed error values*).
It did **not** change any built-in's signature — a runtime failure stays a runtime failure,
and never becomes an `Err` — so the race is still open, and a `read_file` returning a
`Result` is a separate decision about the outside-world built-ins.

### A file is a module, and `use` names one by path
<!-- id: modules -->

```nazm
use "util.nz";
use "sub/deep.nz";

fn main() -> Int { quad(5) }
```

| | |
| --- | --- |
| **Syntax** | `use "path";`, at the top level only. The path is a **quoted string**, not an identifier chain: a path is data, so there is no module-path grammar to design and nothing that could be mistaken for one |
| **Resolution** | Relative to the **importing file's** directory, always. Never to the working directory, never to a search path, never to an environment variable — a build that depends on where it was started is not reproducible |
| **Loading** | Every reachable file is read once, keyed by its canonical path, and parsed on its own text. The result is a **graph**: modules, and which modules each one imports |
| **Order** | The root first, then its imports depth-first in source order. Deterministic, and a function of the import graph alone |
| **Diamonds** | A file imported twice is **one module**, not two. Its definitions exist once and have one identity |
| **Cycles** | **Allowed.** See below |
| **Missing file** | `N0205`, pointing at the quoted path |
| **Declarations** | There is no `mod`. A file is the module |
| **The prelude** | Every module also imports the core prelude, with no `use` written, and it is loaded after every project module. N12; see *Typed error values* |

#### One file is one module is one compilation unit

A module is not a namespace bolted onto a program that was really one file all along.
**It is the unit of meaning**: a module owns its definitions, decides which of them
anyone else may name, and is checked against the *interfaces* of the modules it imports
rather than against their bodies.

That property is the point, and it is worth stating as a test rather than as an intention:
*given module A's source and the interfaces of the modules A imports, A can be checked
with no access to those modules' bodies.* Everything below follows from wanting that to
be true, and `architecture.md` §7.2 says where each half of it lives.

What one module is has a boundary today and will move: a package will eventually be able
to present several files as one unit, and a generated or virtual source has no file at
all. So the semantics below are written in terms of **modules**, and "one file is one
module" is how modules currently come into existence rather than what a module is.

#### Visibility: private by default, `pub` exports

```nazm
// lib.nz
pub fn parse(s: Str) -> Int { digits(s) }   // exported
fn digits(s: Str) -> Int { str_len(s) }     // private to lib.nz
```

| | |
| --- | --- |
| **Default** | A top-level definition is **private to its module**. Nothing outside the module may name it |
| **`pub`** | Written before `fn`, it exports that definition: modules that directly import this one may name it. That is all it does — it does not change the definition's meaning, its type, or how its own module sees it |
| **Granularity** | One bit. There is no `private`, no package visibility, no friend module and no export list. A module may re-export (`pub use`, below) |
| **Within a module** | Private and public definitions are alike: a module sees everything it defines, in any order |
| **Built-ins** | Unaffected. They are not a module's definitions and `pub` has nothing to say about them |

**`pub` is about names, not about safety.** It says which names another module may write,
and nothing about capabilities, effects, or what a definition may do. Confusing the two is
how a visibility keyword ends up load-bearing for security in a language that never
promised it.

*Changed on 2026-09-22.* Until then every top-level function in a program was visible to
every file in it. The old text recorded the consequence honestly — "adding private-by-default
later would break every multi-file program that relies on today's behaviour" — and that is
what happened: the multi-file programs in this repository, including the self-hosted
compiler, were migrated by marking their genuine cross-module APIs `pub`. A program written
against the old rule does not silently change meaning; it stops compiling, naming the
definition and the module that cannot see it.

#### `pub use` offers another module's definitions — N103

`pub use "shapes.nz";` offers every name `shapes.nz` offers; `pub use "shapes.nz" { double, Sq as
Square };` offers the names chosen, `Square` the name `Sq` is offered under. A `pub use` is also an
ordinary `use`.

| | |
| --- | --- |
| **Identity** | A re-export adds no definition. `twice`, offered for `shapes.nz`'s `double`, *is* `double`: a reference, a rename, a persisted interface and a symbol all name `shapes.nz::fn double` |
| **What a module offers** | Its own `pub` definitions and what it re-exports. An importer receives exactly that, not transitively beyond it |
| **Traits and impls** | A trait is re-exported under its own name only. An impl in a module re-exported from is usable wherever the re-exporting module is imported |
| **Refused** | A chosen name the module does not offer, `N0211`; a name the module already offers, `N0212`; modules re-exporting from each other in a cycle, `N0213`, at each `pub`; `pub use "PATH" as m;` |
| **Rename** | Renaming a definition renames every use written with its own name, and the name in a re-export's list; a use written under an alias still names it and is kept |

#### `use` makes an interface visible, not a body

`use "lib.nz";` means: **the names `lib.nz` exports may be written in this module,
unqualified**. It is not textual inclusion, and it is not a request to compile anything —
a module is part of the compilation because it is reachable, and `use` decides what this
module may *see*, which is a different question.

| | |
| --- | --- |
| **What it brings in** | Exactly the exported definitions of the named module. Its private definitions are not brought in, and neither is anything *it* imported |
| **Spelling** | Unqualified. An imported `parse` is written `parse`, the same as a local one. There is no qualified path syntax and no alias |
| **Direction** | One way. Importing a module does not let it see the importer |
| **Self-import** | A module that imports itself already sees all its own definitions, so the import adds nothing and is not an error |
| **Duplicate import** | Importing the same module twice adds its exports once |

##### Imports are not transitive

If `A` imports `B` and `B` imports `C`, then **`A` cannot name `C`'s exports.** `B`
depending on `C` is `B`'s business; it is not an interface `B` offers, and a language in
which it were would make every dependency's dependency part of every module's vocabulary.

To use `C`, `A` imports `C`. That is one line, and it makes the dependency true in the
source instead of accidental.

This is the rule with the largest effect on how much context reading one module requires —
the *reason* Nazm has module interfaces at all rather than one program-wide namespace. See
`master-architecture.md` on context routing.

#### Collisions are errors, not silent choices

Import order decides nothing. Every one of these is refused, with both sides named:

| Situation | Code | Where it points |
| --- | --- | --- |
| A module defines the same name twice | `N0203` | The second definition, with the first as a secondary label |
| A module defines a name and imports a module exporting that name | `N0208` | The `use`, with the local definition as a secondary label |
| Two directly imported modules export the same name | `N0207` | The second `use`, with the first as a secondary label |
| A module names a definition that exists in an imported module but is private there | `N0206` | The name, with its private definition as a secondary label |
| A definition uses the name of a built-in | `N0203` | The definition |

**Two modules may each define a private `helper`.** They are different definitions and
neither is in the other's way — that is what owning your names means, and a module system
in which it were not true would be a naming convention.

`N0207` is reported where the ambiguity is created, not where an ambiguous name is used.
The alternative — allowing the import and refusing only a mention of the contested name —
is defensible in a language with qualified paths, because there the program can say which
one it means. This one cannot: with no way to write the distinction, a deferred error only
moves the same dead end further from its cause.

`N0206` exists so that privacy does not present as absence. "`digits` is not defined" is
true from the caller's position and useless: the reader can see it defined in the file they
just imported. Saying that it is defined and private names the actual repair, which is to
export it or to stop calling it.

#### Cycles remain legal

`A` may import `B` while `B` imports `A`. Checking is two stages, and that is what makes it
well defined:

1. **Interfaces**, for every module in the graph: each module's declarations, their
   signatures, and which are exported. Reading a declaration requires no other module.
2. **Bodies**, each module against its own definitions plus the interfaces of the modules
   it directly imports.

Neither stage needs a module ordering, so there is no ordering for a cycle to violate. The
language has no top-level initialisation — no globals, no static constructors, nothing that
runs before `main` — so there is no initialisation order a cycle could make ambiguous
either. A cycle would become a real question the moment either of those changed; until then
outlawing it would be an implementation's convenience written into a language.

#### The root module owns `main`

The program's entry point is **`main` in the root module** — the file named on the command
line. `N0204` is reported when *that module* has no `main`, whatever other modules contain.

An imported module's `main` is an ordinary definition of that module. It is private unless
exported, it does not become a second entry point, and it does not collide with the root's.
A helper file with a `main` of its own — a module that is also runnable on its own — is a
normal thing to write, and until 2026-09-22 it was not expressible: two `main`s in one
compilation were a duplicate definition, so imported files had to rename theirs.

`main` need not be `pub`. Being the entry point is not the same as being importable, and
requiring the keyword would suggest the runtime resolves it by name from outside.

#### What `main` holds, and how it ends
<!-- id: main-ends -->

*R1, 5b.1 and 5b.3; stable since R1 closed (0.10.0).*

**Printing needs nothing written (option C).** A function named `main` that holds neither an
`OutCap` nor an `IoCap` holds an `OutCap` named `out`, as if `out: OutCap` were written after
its parameters: it may print, and pass `out` to a function that prints. Every other authority —
`IoCap`, `NetCap`, `ProcessCap`, `SpawnCap`, `RandomCap`, `TimeCap`, `ForeignCap` — is still
written, so what a program can do beyond writing to its own output is in `main`'s signature.

```nazm
fn main() {
    println("Hello, world!")
}
```

**The program's authority is reported.** `nazm build` writes, on standard error after a build
of a program with a `main`, one line naming every capability `main` holds and what it reaches —
`authority: OutCap (given to main): prints to standard output and error`, or `authority: none` —
and `nazm inspect` gives the same as its `authority` member, an array of `{ capability, implicit,
reaches }` (`null` without a `main`). Every authority a program has enters through `main`, so
this is what the program may touch, read without its body.

**`main`'s result is the exit status.** It is written to no stream: a program ends with status
`main`'s `Int`, which must be `0..=255` — one outside is `N0416`'s failure, *`main`'s result is an
exit status, and this one is outside 0 to 255*, ending with status 2, as `exit_with` of one is. A
`main` with no result type ends with `0`. Until R1 the result was printed in decimal on standard
output and the status was `0`; set `NAZM_RESULT_REPORT` (to anything) and a program still does
that — a report, as `NAZM_MEMORY_REPORT` is, which the tests read results by. `nazm test` runs
every recorded-output case with it set, whatever its caller's environment, so a case's `.expected`
is what the program prints and then `main`'s result, as every one was before R1. On a board, which
has no exit status, the result is written as before.

### Packages — `nazm.toml`, `use "NAME:path"` and `nazm.lock`
<!-- id: packages -->

A **package** is a directory holding a manifest, `nazm.toml` (`nazm.package/1`): its name, its
exact version, its source root (`src` unless it says otherwise), an optional entry module, and
its dependencies — each a path and the exact version that package must state.

```toml
schema = "nazm.package/1"

[package]
name = "app"
version = "0.1.0"
main = "main.nz"

[dependencies]
geo = { path = "../geo", version = "1.2.0" }
```

`use "geo:lib.nz";` imports the module `lib.nz` of the package `app` declares as `geo`, resolved
against `geo`'s source root. A package sees its own dependencies and nothing else — not its
dependencies' dependencies (`N0508`) — and a relative import stays inside its package's source
root. A dependency's module is identified as `NAME@VERSION/path` (`geo@1.2.0/lib.nz`), which no
project path can be.

`nazm check|build|run DIR` builds the package in `DIR` from its `main` (`N0509` without one).
Resolution is exact and deterministic: packages are read in name order; a declared version its
manifest does not state is `N0502`, two packages of one name `N0503`, a cycle `N0504`, a missing
dependency `N0501`, an invalid manifest — an unknown key included — `N0500`. Every source is a
path, so nothing is ever downloaded.

`nazm lock DIR` writes `nazm.lock` (`nazm.lock/1`; `nazm.lock/2` with enabled features since Gate 2, *Package features*): each package's name, version, source relative
to the root package, and a BLAKE3 digest of its manifest and sources, in name order — the same
bytes for the same packages anywhere. `--locked` holds `check`, `build` or `run` to it before
anything is compiled: no lockfile is `N0505`, other packages than it records `N0506`, and a package
whose bytes changed since it was locked `N0507`.

**Reproducible builds.** The same sources, lockfile, toolchain, target, backend and settings give
byte-identical executables on one host, from empty caches, in any directory.

### Restriction profiles — `--profile`
<!-- id: profiles -->

A **profile** refuses some programs and changes nothing about the ones it accepts. It is chosen
with `--profile NAME` on `check`, `build` and `run`, or required by a package's manifest
(`profile = "NAME"` under `[package]`); every profile in force applies to the whole program.

| Profile | Rules |
| --- | --- |
| `general` | none |
| `embedded` | `no-io`, `no-spawn`, `bounded-allocation` (N52), `no-recursion` (N60) |
| `critical` | `declared-effects`, `no-spawn`, `no-foreign`, `locked-build`, `no-declassification` (N52), `no-ambient-time` (N54), `no-select` (N60), `no-unknown-calls` (N87) |
| `cyber` | `declared-effects`, `no-foreign`, `locked-build`, `contents-stay-in-files` (N52), `checked-paths` (N80), `no-unknown-calls` (N87) |
| `realtime` (N63) | `no-io`, `no-spawn`, `bounded-allocation`, `no-ambient-time`, `no-recursion`, `no-blocking`, `bounded-loops`, and since Gate 3 `bounded-stack` (no call cycle and no call through a function value, so the image's stack has a static bound) and `no-heap`. Its tasks are a board's fixed-priority ones (*Boards*): `no-spawn` fixes the topology and `no-blocking` the waits. No execution time is bounded — `bounds.json` keeps `wcet` null — and nothing here is called hard real-time |
| `authority` (N79) | `explicit-authority` |
| `kernel` (Gate 3) | `no-spawn`, `no-foreign`, `no-blocking`, `no-heap`: no allocation site at all, whatever the board's manifest gives (*Boards*) |
| `robotics` (Gate 3) | `realtime`'s, and `explicit-authority`: a control loop's device authority is the one it is handed |
| `automotive` (Gate 3) | `realtime`'s and `critical`'s |
| `industrial` (Gate 3) | `realtime`'s, `critical`'s, and `watchdog` |
| `medical` (Gate 3) | `realtime`'s, `critical`'s, and `audit` |
| `aerospace` (Gate 3) | `embedded`'s, `realtime`'s, `critical`'s, and `no-float-nondeterminism` |
| `assurance` (Gate 3) | `critical`'s, and `no-recursion`, `bounded-loops`, `bounded-stack`, `no-heap` |

`no-io`, `no-spawn` and `no-foreign` refuse a program whose `main` requires that effect, through
any call, standard functions included. `declared-effects` refuses any function of the program that
writes no `! { … }`, so no function exercises its caller's authority. `explicit-authority` (N79)
refused a function whose recorded inherited authority was not empty; since N104 no function
inherits authority, so it is the language's rule and refuses nothing more (*No inherited
authority*). `locked-build` refuses a
build that is not a package build whose lockfile verifies. `no-recursion` refuses a cycle of direct
calls or spawns, and is `unknown` where a call goes through a function value; `no-select` refuses
any select (N60). Each rule's verdict is `pass`, `fail` or `unknown`, and `unknown` is refused: a
rule the compiler could not establish is not met. A refusal is `N0510`, naming the profile,
the rule and where it comes from; an unknown profile is `N0511`. `nazm check --profile-report`
prints `nazm.profile-report/1`. A profile is compiler-enforced evidence about these rules and
nothing more: not a certification, not a sandbox.

**Domain profiles (Gate 3).** `robotics`, `automotive`, `industrial`, `medical`, `aerospace` and
`assurance` are compositions of the rules above with three domain rules; none changes what a
program means, and none is a certification, a qualification or a regulatory approval.
`no-float-nondeterminism` refuses each call of `float_sin`, `float_cos`, `float_tan`, `float_exp`,
`float_ln`, `float_log2`, `float_log10`, `float_pow` and `float_atan2`, whose results are the host
library's and may differ between hosts; `float_sqrt` and the rest are correctly rounded and admitted.
`watchdog` requires the program to feed a board's watchdog (*Boards*) from a `@task`: it fails at
`main` when no task calls `watchdog_feed` itself. `audit` requires every function that changes a
state — calls `atomic_store`, `atomic_swap` or `atomic_compare_swap`, the only ways a static changes
other than counting with `atomic_add` — to call `@std/audit`'s `audit_transition` itself, so each
transition emits its record; each such call site is a witness. The standard library's own functions
are not held to it.

**Bounds (N63).** `no-blocking` refuses every channel send, receive and select by its call site.
`bounded-loops` requires every `while` to have a trip bound the compiler establishes, and is
`unknown` — refused — for any other, naming why. The bounded form: a counter `i` whose last write
before the loop, in the loop's block, is `let mut i = A` or `i = A`; the condition `i < B`, `i <= B`,
`i > B` or `i >= B`; and the body's only write to `i`, at its top level, `i = i + C` (towards a `<`
or `<=` limit) or `i = i - C` (towards `>`, `>=`), with no `continue` to the loop. `A`, `B` and `C` are
`Int` literals, a negated literal, or `B` an immutable local bound to one, and `C` is positive. The
bound is the exact number of turns one entry to the loop makes, computed without overflow; a
`break` or `return` ends it sooner. Wherever MIR facts are computed — `embedded`, `realtime` — the
report carries `bounds`: each loop's bound or `null` with its reason, the longest chain of direct
calls from `main` (`null` with a cycle or a call through a value), the allocation, task and queue
sites as static counts, and `stack_bytes` and `wcet` as `null`, with each number's kind named.

**The board's stack (N63).** `nazm build --target aarch64-unknown-none --objects DIR` also writes
`DIR/bounds.json` (`nazm.bounds/1`): every function's frame size as the code generator reported it,
and the stack bound — those frames summed along the image's deepest call path from `nz_board_main`,
with the path and the 64 KiB reserved — or `null` with the reason (a call cycle, a call through a
value, a callee outside the image). A board build does not reuse cached objects. `--stack-watermark`
paints the board's stack at start and writes `nazm-stack: used=N` after `main`'s result: a
measurement of that run, not a bound; refused for any other target.

### Durable identity: what makes this the same module, or the same definition
<!-- id: durable-identity -->

*Added by N3, 2026-09-22.* Everything above is about one compilation. This is about two.

A compiler that wants to reuse work has to answer a question no single compilation ever
asks: **is this the same thing it was last time?** Load order cannot answer it — the ids a
compilation mints are positions in that compilation — and neither can a file's path, which
says where a checkout happens to live. So the language settles what continuity means, and
an implementation encodes it.

Identity is not content. *Which* definition this is and *what it currently says* are two
questions, and collapsing them would make every edit create a new definition, which is the
opposite of what durable identity is for.

#### The source root

A compilation has a **source root**, and every module's durable identity is its path
**relative to that root**, normalised: `/` between components, `.` removed, `..` folded
away where it can be, and no leading or trailing separator. A checkout moved from one
directory to another has the same root and the same relative paths, so it has the same
identities. The shell's working directory is not one of them: a relative path is resolved
against it to make the comparison possible, and what remains after the root is removed is
all that survives.

There are two ways a compilation gets one.

> **Declared.** A file named `nazm.root` marks the directory that contains it as a source
> root. The nearest one at or above the root module wins. `nazm check` and `nazm interface`
> also accept `--source-root DIR`, which names one without writing a file.
>
> **Derived.** With neither, the root is the directory containing the root module — the
> file named on the command line.

*Declared roots were added by N4, 2026-09-22. Derived ones are unchanged, and a project
with no marker behaves exactly as it did.*

**The marker must be empty.** It answers one question and owns nothing else. The moment it
is allowed to carry a field it is a manifest, and a manifest is a package system's front
door — versions, dependencies, a registry, a resolver. None of those is needed to say where
a project begins, and a marker with content is refused rather than read.

**Why the distinction matters.** Under a derived root the identity a module gets depends on
which file the compilation started from: `lib/util.nz` when it started at `main.nz` beside
`lib/`, and `util.nz` when it started at `lib/util.nz` itself. Both are correct within their
own compilation, and neither can be compared with the other. Under one declared root they
are the same key, which is what lets two compilations of one project agree about what their
modules are. Anything comparing keys across compilations compares them within one declared
root or not at all.

#### Module identity

> **Two modules are the same module when their normalised source-root-relative paths are
> equal.**

That is all of it. Not the absolute path, which differs per machine; not the canonical
path, which differs per checkout and leaks the layout above the root; not a content hash,
which would make every edit a different module.

Two situations have **no** durable module identity, and are refused one rather than given a
guess:

| Situation | Why |
| --- | --- |
| The module sits **outside** the source root — its relative path still begins with `..` after folding | Its identity would depend on the directory the root happens to sit in, which is the thing a durable identity must not depend on |
| Two different import paths reach **one file** through a symbolic link | The implementations disagree about how many modules that is (see below), and an identity the two cannot both compute is not an identity |
| The relative path's first component is `@core` | That name is the toolchain's. Added by N12, so that no project module can ever be keyed as the core prelude is |

**One module's identity is not a path at all.** The core prelude's `ModuleKey` is
`@core/prelude`, fixed by the toolchain and the same in every compilation of every project
(*Typed error values*, above). It is disjoint from every project key by the row above, and
it is the one key a persisted interface may name that no source root computed.

Such a compilation is correct and complete — session-local identity is untouched, the
program checks, runs and compiles exactly as before. What it does not get is a durable key,
and anything built on durable keys declines rather than guesses.

#### Symbolic links, and the one place the two implementations differ

The reference loader reads each file once keyed by its **canonical** path, so two spellings
that reach one file through a symbolic link are one module. The compiler written in Nazm
keys by a **textually normalised** path — its runtime has no `realpath` — so the same two
spellings are two modules to it. That difference predates durable identity and is recorded
in `bootstrap.md`.

Durable identity does not resolve it in favour of either. It **declines** where they could
disagree: a module reached under two different normalised paths has no `ModuleKey`, in
either implementation. Where no alias exists the two agree by construction, because textual
normalisation and canonicalisation give the same answer for a path with no symbolic link in
it.

Making them agree in the aliased case would require the Nazm-written compiler to obtain
filesystem identity — a `realpath` built-in and a runtime symbol it does not have. That is
a language and runtime change, deliberately not made for a case no real program has, and
`roadmap.md` records it as a known consequence rather than a plan.

#### Definition identity

> **A definition is identified by its module, its kind, and the name it is declared
> under.**

Top-level functions do not overload, so a name identifies at most one definition in a
module, and `N0203` is what keeps that true. What follows is therefore decidable:

| The edit | Same definition? |
| --- | --- |
| The body changes | **Yes.** A function whose body changed is that function, changed |
| The signature changes | **Yes.** Its contract changed; the definition did not become another one |
| `pub` is added or removed | **Yes.** What may name it changed; which definition it is did not |
| It moves within its module | **Yes.** Identity is a position in a namespace, not a position in a file |
| It is renamed | **No.** A different name is a different definition, and the old one was removed |
| It is deleted, and later a definition of that name is added | **The same identity.** This models continuity of a *name in a module*, not the history of an author's intent |

The last row is the honest limitation and is stated rather than hidden. Identity here
answers "which definition does this name in this module refer to", which is what a cache
key and a dependency edge need. It does not answer "is this the same definition the author
was thinking of", which is lineage — a different question, for a tool that tracks edits
rather than for the language.

**Built-ins are not definitions under this model.** They belong to no module, so they have
no module to be identified by; they keep the separate, stable identity they already have.
Nothing about a module's exported interface mentions them, because a call to `str_len`
inside a body is not part of what that module offers.

### Structured concurrency: scopes, tasks and channels
<!-- id: concurrency -->

This is the **initial** model. It is deliberately small, and what it does not claim is as
much a part of the specification as what it does — see *What this model does not promise*
below.

```nazm
fn worker(jobs: Chan, results: Chan) -> Int {
    let buf = ints_new();
    while chan_recv(jobs, buf) {
        chan_send(results, ints_pop(buf) * 2);
    }
    0
}

fn feeder(jobs: Chan) -> Int {
    let mut i = 0;
    while i < 10 {
        chan_send(jobs, i);
        i = i + 1;
    }
    chan_close(jobs);
    0
}

fn main(out: OutCap, tasks: SpawnCap) -> Int {
    let jobs = chan_new(4);
    let results = chan_new(4);
    let buf = ints_new();
    let mut total = 0;
    scope {
        spawn feeder(jobs);
        spawn worker(jobs, results);
        spawn worker(jobs, results);
        let mut got = 0;
        while got < 10 {
            chan_recv(results, buf);
            total = total + ints_pop(buf);
            got = got + 1;
        }
    }
    chan_close(results);
    print(str_concat(int_to_str(total), str_from_byte(10)));
    0
}
```

This is `examples/worker-pool.nz`, and a test runs it under both implementations and
checks it prints 90 — the sum of 0..9 doubled.

Its shape is the point. With a **bounded** channel, production and consumption have to
progress at the same time. An earlier version of this example had the parent send all ten
jobs and only then receive, which deadlocks: the workers fill the four-slot results
channel, block in `chan_send`, stop taking jobs, the jobs channel fills behind them, and
the parent blocks sending — with nothing left running to drain anything. Widening the
buffers only moves the job number at which it happens. Feeding is therefore a task of its
own, and the parent drains while the workers run.

#### Scopes and tasks

| | |
| --- | --- |
| **Creation** | `spawn f(args);` starts a task running `f`. It is a statement, and `f` must be a named function. *Corrected 2026-10-07, R1:* this row gave "there are no function values" as the reason, true until N50, and the callee is still a named function. *Corrected again 2026-10-08, Q1:* R1 added that a function value may be a `spawn` argument; it may not — the checker refuses it (`N0321`), as *Function values and closures — N50* says under **Tasks** |
| **Where** | Only inside a `scope { … }` block. `spawn` outside one is `N0320` |
| **Joining** | A `scope` block does not finish until **every** task spawned inside it has finished. There is no way to leave a task running past its scope, and no way to detach one. That is the whole point of the word |
| **Nesting** | A `scope` may contain another. The inner one joins first, because it closes first |
| **The scope's value** | None. `scope { … }` is a statement |
| **Result propagation** | A task's return value is **discarded**. Results travel through channels, which is the one mechanism, rather than two |
| **Error propagation** | A task that fails at run time — overflow, a bad index, a failed allocation — ends the **whole program** with its diagnostic and exit status 2. Tasks do not have private failure modes. It ends **at the scope's closing brace, not the instant the task failed**: the scope joins every task first, so the diagnostic is reported once nothing is still running. Both implementations do this. What follows says what it costs, and how the compiled one manages it |

#### When a task starts, and when a scope waits

**A task starts when its `spawn` runs**, not when the scope closes. It runs *alongside*
the rest of the scope's body.

This is not an implementation detail, and it is written down because the first
implementation got it wrong. Collecting the spawned tasks and starting them all at the
closing brace reads like a harmless reordering — every task still runs, and every task is
still joined. It deadlocks the example at the top of this section: the body sends ten jobs
into a channel of capacity four before any worker exists to take them, so the fifth send
blocks forever and the closing brace that would have started the workers is never reached.
That is the shape of every worker pool, so the property is part of the language.

The closing brace waits for every task, **including when the body itself failed**. A
failure that returned immediately would leave tasks running past the scope that promised
to contain them, which is the one thing a scope is for. When both the body and a task
fail, the body's failure is the one reported: a task that failed because the body stopped
feeding it is a consequence, not a cause.

There is no cancellation beyond closing a channel. A scope whose body fails still waits
for tasks that may be blocked on a channel nobody will now close, and such a program
hangs rather than reporting. This is the price of the paragraph above, and it is the same
mechanism seen from the other side: the diagnostic exists, but every failure leaves through
the join, and a join that never returns never delivers it. Closing the channels a task
waits on is the program's responsibility, and the `while chan_recv(c, buf)` loop is the
idiom that makes it work.

##### How the compiled program does this

A compiled failure does not end the process where it happens. `@nz.fail` records the
diagnostic in **thread-local** state and returns; each generated function gains one extra
exit that a failure leaves by, and every call site checks the flag afterwards, so a
failure walks back out through the calls that led to it. On the way it passes each open
`scope`, joining it — innermost first, which is the nesting. Only the entry wrapper
writes a diagnostic and exits, by which time nothing is still running.

A task that fails leaves its message in its own argument block rather than reporting it.
The scope's join reads that block after `pthread_join` has returned for it, which is a
happens-before edge, so no lock is involved; the earliest failure by spawn position is
the one taken, so which task is reported does not depend on which thread finished first.
Only the first failure a thread records is kept, which is what makes the body's failure
win: a task's is adopted at the join, and the join happens after the body's was recorded.

Verified at `-O0` and `-O2` against the interpreter, for a failing body, a failing task,
a failure two calls below the body, and nested scopes — and for the case where a task
fails *strictly before* the body does, where the body's diagnostic is still the one
reported.

#### What may cross into a task

**`spawn` arguments may be `Int`, `Bool`, `Str` or `Chan`, and nothing else.** Passing an
`Ints` or a `Strs` is rejected with `N0321`.

The reason is not stylistic. `Ints` and `Strs` are handles to shared mutable storage with
no synchronisation of any kind (*Sequences are shared, mutable, and never freed*). Letting
one cross into a task would be a data race the language had invited, and no amount of
documentation would make it safe. `Int`, `Bool` and `Str` are values — a task gets its own
copy of what it was given, and nothing it does is visible to anyone else. `Chan` is the
exception on purpose: it is the primitive *designed* to be shared, and its operations are
the synchronisation.

A task also cannot see its caller's locals. It runs a named function with the arguments it
was given, and a function has never been able to see its caller's bindings.

#### Channels

`Chan` is a **bounded** queue of `Int`.

| | |
| --- | --- |
| `chan_new(capacity)` | A channel holding at most `capacity` values. `capacity` must be at least 1, or `N0405` |
| `chan_send(c, v)` | Blocks while the channel is full. Returns `true` when the value was queued, `false` if the channel is closed — in which case **the value was not sent** |
| `chan_recv(c, out)` | Blocks while the channel is empty. On success pushes one value onto `out` and returns `true`. Returns `false` when the channel is closed **and** drained |
| `chan_close(c)` | Closes it. Idempotent, and wakes every blocked sender and receiver |

Closing is the cancellation mechanism, and `chan_send` and `chan_recv` are the
**observation points**: a task that loops on `while chan_recv(c, buf)` stops when the
channel closes, without needing a separate cancellation flag. Values already queued when
a channel closes are still received — closing ends a channel, it does not discard it.

`Chan` is a reference, like a sequence, and is not comparable with `==`.

#### Lifetime and cleanup

A task's arguments are values or channels, so nothing it holds can dangle. Joining is
guaranteed by the scope's closing brace, and there are no destructors to run — nothing a
program can write runs at reclamation.

> **Superseded in part by N8, 2026-09-22.** This section used to continue: *"a channel
> outlives every scope that uses it because nothing is ever freed … cleanup is therefore
> about joining, not about freeing"*. Freeing happens now. A channel's storage — its queue,
> its mutex and its condition variable — goes when the last reference to it dies, under
> rule 4 like everything else, and **closing a channel is still not destroying one**:
> `chan_close` changes what the channel does and touches no storage at all. Nothing a
> program can observe changed. What makes the two safe to keep apart is that a thread
> blocked in `chan_send` or `chan_recv` reached it from a frame that holds the handle, so a
> waiter implies a live reference and no reference implies no waiter.

#### What this model does not promise

**The runtime was one operating-system thread per task until N106.** `nazm run --interpret` still is: a
thread per task and a mutex and condition variable per channel. `nazm build`'s programs run their
tasks on a pool of worker threads by default (§7.107), parking a task that waits instead of its
worker, with the same mutex-and-condition-variable ring; `NAZM_SCHEDULER=threads` gives the thread
model, `pthread_create` and `pthread_join` per task. The two are written the same way on purpose, so
that they agree by construction rather than by testing — though they are tested against
each other as well, at both optimisation levels.

Every one of the following is *not* claimed, because none of it is implemented:

- No work stealing and no preemption. A native program's tasks run on `NAZM_WORKERS` threads by
  default (N83's pool, the default since N106, `architecture.md` §7.107): a task that waits is parked
  and its worker freed; a task that computes without waiting holds its worker. `NAZM_SCHEDULER` is
  `pool` or `threads`; any other value starts no task (`N0404`). A program that calls C runs a thread
  per task unless the pool is asked for, because a task blocked in C would hold its worker and every
  task on it. The scheduler changes how tasks are carried, never what a program means: the same
  output, ending and memory report. A pool task's stack is `NAZM_TASK_STACK` bytes (256 KiB by
  default), and recursion past it is `N0408`. The interpreter runs a thread per task.
- **No deadlock freedom.** Two tasks that each wait for the other wait forever. Nothing in
  the capture rules prevents it, and no ownership discipline implies it. Neither does a
  scope: it waits for its tasks unconditionally, so a body that blocks on a task that
  cannot proceed hangs the program.
- **No cancellation** other than closing a channel. There is no cancellation token, no
  interruption, no timeout, and a failure elsewhere does not unblock anybody.
- No fairness guarantee between tasks blocked on the same channel.
- No memory-ordering guarantees beyond what the channel operations themselves provide.
  There is no shared mutable state to order, because the capture rule removes it.
- No priorities, no timeouts, no `select`.

The interface is deliberately separable from the runtime: `scope`, `spawn` and the four
channel operations say nothing about threads, so a different runtime strategy could
replace this one without changing what a program means.

### Effects: what a function may do
<!-- id: effects -->

An **effect** is an externally observable power a function exercises beyond computing its
result from its arguments — one a caller could observe, and one someone may later want to
forbid. The checker computes every function's effects from its resolved calls and holds each
declared set to its body. Effects restrict which programs are accepted; they change nothing
about what an accepted program does, and add nothing to what runs.

```nazm
fn area(w: Int, h: Int) -> Int ! {} { w * h }

fn show(io: IoCap, n: Int) -> Int ! { io } { print(int_to_str(n)) }

fn worker(c: Chan) -> Int ! {} { chan_send(c, 1); 0 }

fn run(io: IoCap, tasks: SpawnCap) -> Int ! { io, spawn } {
    let c = chan_new(1);
    scope { spawn worker(c); }
    show(io, area(2, 3))
}
```

#### The effects

There are exactly two, and they are the compiler's. An effect is identified by the compiler,
never by where or how it is written, and there are **no user-defined effects**.

*Corrected 2026-10-07, R1:* written for N36. N42 added a third, `foreign`, exercised by every
call into C (*Foreign functions: calling C*); there are three, and still none user-defined.

| Effect | Exercised by |
| --- | --- |
| `io` | the eight built-ins that reach the outside world — `args_count`, `arg`, `read_file`, `write_file`, `file_exists`, `print`, `eprint`, `exit_with` — and nothing else |
| `spawn` | a `spawn` statement: starting a task |

**Not effects**, and why:

- **Allocation.** Every string operation and every sequence allocates, and nothing the
  language can express restricts it; an `alloc` effect would be on nearly every function and
  forbid nothing anyone could act on. It is not tracked, rather than tracked unsoundly.
- **Mutation.** A `let mut` is local state and is nobody else's business. Writing into a
  sequence or a vector a caller handed over is visible to that caller — sequences are shared
  handles — but it is a property of the value passed, not a power exercised on the world, and
  an empty effect set does **not** promise it does not happen: pure here means *no effect*,
  not *referentially transparent*.
- **Channels.** A channel is a value too; sending, receiving and closing act on it. The power
  to run anything alongside is `spawn`'s, and a receive that blocks forever is divergence,
  which this model does not track (below).
- **Traps.** Overflow, division by zero, an index out of range and a failed read end the
  program; so can almost any arithmetic, so marking them would mark everything.
- **`Result` and `?`.** A failure written in the return type is a value, and `?` is a
  `return`. Neither is an effect, and an `! {}` function may propagate one freely.

#### A function's effect set

After the return type, a function may declare the effects it exercises: `! {}`, `! { io }`,
`! { io, spawn }`. The names live in their **own namespace** — `io` is not a variable, a type
or a function, and a function or parameter named `io` is no conflict. A set means the same in
any order and the formatter writes it in the canonical one; a name that is not an effect is
`N0367`, and one written twice is `N0368`, with a fix that removes the repetition.

**Pure is the empty set**: `! {}` asserts that the function exercises no effect and calls
nothing that does, directly or through any number of calls. There is no separate `pure`
word.

#### Declared and inferred

- **Every function's effects are inferred**: the effects of what its body calls — a
  built-in's own effect, a `spawn`'s `spawn` and everything its task does, and each callee's
  effects — through any number of calls. For recursion and mutual recursion the answer is the
  **least fixed point**: a cycle of functions none of which reaches `io` does not require
  `io`, and one `print` anywhere on it reaches every function of the cycle. Recursion adds no
  effect of its own, and declaration order changes nothing.
- **A declared set is a contract.** The body may require no effect outside it: requiring
  more is `N0366`, at the first call or `spawn` that needs the missing effect, with one
  shortest chain of calls from there to the built-in or `spawn` that exercises it. Declaring
  **more** than the body needs is allowed: the contract is wider than the implementation, and
  a caller reads the contract.
- **A caller reads a callee's contract when it has one**, and what its body requires when it
  has none — but only within the callee's own module.

**No set is not the empty set.** `fn f() -> Int` and `fn f() -> Int ! {}` are different
declarations, and only the second promises anything:

| Written | Contract | Its own module's callers see | Another module's callers see | Authority (N37) |
| --- | --- | --- | --- | --- |
| `fn f() -> Int` | none | what its body requires, inferred | every effect, `{ io, spawn }` | only the capabilities its scope holds (N104; its caller's until then) |
| `fn f() -> Int ! {}` | pure | `{}` | `{}` | only the capabilities its scope holds |
| `fn f(io: IoCap) -> Int ! { io }` | may reach the outside world | `{ io }` | `{ io }` | the `IoCap` it is handed |

Omitting the set is never a way to say *pure*: to another module an undeclared function may
do anything, and its effects are inferred only for its own module's callers.

#### Across modules

**A caller never needs another module's body to know its effects.** What crosses a module
boundary is the declared set, which is part of the module's published interface. An exported
function that declares **no** set promises nothing, so a caller in another module must
assume it may exercise **every** effect: a pure function may call an import only if the import
declares `! {}`. Declaring is how an exported function becomes callable from pure code; a
program that declares nothing anywhere is accepted exactly as before N36.

Changing a declared set changes the interface, so every importer is checked again. Changing a
body within its declared set changes nothing an importer sees.

#### Generics, and what is not here

A generic function's effects are its body's, whatever its type arguments: there are no
function values and no methods, so nothing a type argument brings can call anything. For
the same reason there is no effect polymorphism — no effect variables, no handler, nothing
that removes an effect once it is required. Those are later work, and a later milestone
adds them rather than N36 pretending concrete sets cover them.

*Corrected 2026-10-07, R1:* the paragraph above is N36's. Function values (N50), one effect
parameter per function (N51) and methods (N78) now exist: a call through a function value
performs its type's effects, a call through a trait bound exercises no more than the trait
method declares, and an effect parameter is bound at each call (*Function values and closures —
N50*, *Effect parameters and captured authority — N51*, *Traits and methods — N78*). Handlers,
user-defined effects and effect variables beyond the one parameter remain absent.

**Effects are not authority.** A function declaring `! { io }` says what kind of power it
uses, not that anything granted it. Since N37 who may exercise a power is a separate
question with a separate check: *Capabilities: what allows a function to act*, next.

### Capabilities: what allows a function to act
<!-- id: capabilities -->

An effect describes **behaviour**: `! { io }` says a function may reach the outside world. A
**capability** describes **authority**: a value of a compiler-owned type whose presence in a
function's scope is what allows it to. The two are separate facts, checked separately and
reported under separate codes, and neither implies the other.

```nazm
fn save(io: IoCap, path: Str, data: Str) -> Int ! { io } { write_file(path, data) }

fn log(io: IoCap, s: Str) -> Int ! { io } { print(s) }

fn keep(io: IoCap) -> IoCap ! {} { io }

fn main(io: IoCap) -> Int ! { io } {
    log(keep(io), "saving");
    save(io, "out.txt", "data");
    0
}
```

#### The capabilities

There are exactly ten, and they are the compiler's. There are **no user-defined
capabilities**.

| Kind | Type | Authorises | Effect it authorises |
| --- | --- | --- | --- |
| `Io` | `IoCap` | the eight outside-world built-ins — `args_count`, `arg`, `read_file`, `write_file`, `file_exists`, `print`, `eprint`, `exit_with` — and the `os_` built-ins that name a path or standard input (Gate 2, *Files and handles*) or read the environment (*Process and environment*); implies `OutCap` | `io` |
| `Out` | `OutCap` | the standard streams only: `print` and `eprint` (N79) | `io` |
| `Spawn` | `SpawnCap` | a `spawn` statement: starting a task | `spawn` |
| `Foreign` | `ForeignCap` | a call to a foreign function: running C (N42) | `foreign` |
| `Vouch` | `VouchCap` | `str_vouch`: declassifying a string (N52), in every function | none |
| `Time` | `TimeCap` | `time_now_ms` and `chan_select_until`: reading a clock (N54), in every function | none |
| `Mmio` | `MmioCap` | `mmio_read32` and `mmio_write32` (N62), and `mmio_read8`, `mmio_write8`, `mmio_read16`, `mmio_write16` (N86): a device register, in every function | none |
| `Net` | `NetCap` | the built-ins that name an address — `os_tcp_listen`, `os_tcp_connect`, `os_udp_bind`, `os_udp_send_to`, `os_resolve` (Gate 2, *Networking*); implies nothing, and nothing implies it | `io` |
| `Random` | `RandomCap` | `os_random_bytes`: the operating system's entropy (Gate 2, *Randomness*), in every function | none |
| `Process` | `ProcessCap` | `os_spawn`: running another program, which is every authority that program has (Gate 2, *Process and environment*); implies nothing, and nothing implies it | `io` |

A kind is identified by the compiler, never by how its type is spelled. `IoCap`, `OutCap`,
`SpawnCap`, `ForeignCap`, `VouchCap`, `TimeCap`, `MmioCap`, `NetCap`, `RandomCap` and `ProcessCap` are **built-in type names**, like
`Chan`: they mean the same in every module, and a module may not declare a type of any of those
names (`N0331`). Which authority a built-in needs is a
property of the built-in's identity, decided by an exhaustive table the checker reads — a
program's own function cannot be named `print`, so a call spelled `print` is the built-in.

**v1 capabilities are coarse.** An `IoCap` authorises the arguments, every file, both
standard streams and ending the process alike: it is not least privilege, and it does not
distinguish a read from a write, one file from another, or output from exit. A capability
authorises its **kind**; a resource-specific capability — one file, one directory — would be
a kind with a type argument, and none exists yet.

#### Attenuation: `OutCap` (N79)

**Authority is a preorder.** Holding an `IoCap` is holding everything an `OutCap` authorises,
so an `IoCap` is accepted wherever an `OutCap` is expected — as an argument, a `spawn` argument
or a result. That is the only way to obtain an `OutCap`: there is no constructor (`N0370`) and no
conversion built-in, and `main` may also take one as a root. Nothing implies an `IoCap`, so an
`OutCap` given where an `IoCap` is expected is `N0300`, and there is no path back.

```nazm
fn shout(o: OutCap) -> Int ! { io } { print("loud\n"); 1 }   // the streams, and nothing else
fn main(io: IoCap) -> Int ! { io } { shout(io) }            // attenuated by passing
```

A function holding only an `OutCap` that reads the arguments, touches a file or ends the
process is `N0369`, naming the `IoCap` it lacks. So is one that calls an undeclared function
performing `io`, even one that only prints: across a call, authority is read from effects, and
`io` does not say which part of the outside world. The effect stays `io`: attenuation narrows
authority, not the effect vocabulary. Inside a type — a `Vec[IoCap]` where a `Vec[OutCap]` is
expected — nothing is converted, and that is `N0300`.

#### Authority is a value in scope

**A function that declares an effect set holds exactly the authority of the capability values
its scope reaches**: at each point in its body, every parameter or `let` of a capability type
whose name is visible there. A binding hidden by a later one of the same name holds nothing,
since no expression can reach it; a record, `Vec` or variant that contains a capability holds
nothing until the capability is bound; a type parameter holds nothing whatever it is
instantiated with, since nothing is known about it. Possession is read from bindings a reader
can see, and from nothing else — there is no flag and no hidden grant.

At each point, what is needed:

- **A built-in** needs the authority its identity requires.
- **A `spawn`** needs a `SpawnCap`, to start the task. What the task then does is its function's
  business: a function with a declared set has only what the `spawn` hands it as arguments, so
  authority enters a task only by being **passed**, like any value. A task never gains its
  spawner's authority by being started.
- **A call to a function with a declared set** needs nothing more here. Its authority is in
  its parameters, and the caller supplies them as it supplies any argument — ordinary type
  checking, with a missing or wrong capability being a missing or wrong argument.
- **A call — or a `spawn` — of a function with no declared set** needs whatever that function's
  effects need: in its own module, its inferred effects; from another module, every effect.

Lacking any of these is `N0369`, once per missing kind, at the first point that needs it, with
the chain through undeclared functions to what needs the authority. It is never fixed
automatically: threading authority through parameters changes every caller.

#### Effects and authority are checked apart

| The function… | holds the capability | does not hold it |
| --- | --- | --- |
| **declares the effect** | accepted | `N0369` — authority |
| **does not declare it** | `N0366` — effect | `N0369`, then `N0366` |

A body wrong in both gets both, authority first, in that order every time. Holding a
capability is not exercising it: receiving, passing, returning and storing one adds no effect,
so `fn keep(io: IoCap) -> IoCap ! {}` is pure. Holding authority a body never uses is allowed,
as declaring an effect it never exercises is.

#### Where authority comes from

**`main` is the root of authority.** The runtime calls it, and hands it one capability of each
kind it declares a parameter of — any number, in any order. A `main` parameter of any other
type is refused (`N0371`): the runtime has nothing else to hand over. Every capability in a
running program is one of those, or a copy of one.

**Nothing a program writes constructs one.** `IoCap()` and `IoCap(x: 1)` are refused
(`N0370`); there is no literal, no conversion from another type and no default value, and a
generic function cannot produce a value of a type parameter it was not given. A capability is
unforgeable because no expression but a parameter of `main` has one to give.

#### A value like any other, with two exceptions

A capability is **shareable**: it is copied freely, and a copy is the same authority — there
is no linear or affine use, and so no revocation. It may be stored in a record, a `Vec` or an
enum payload, returned, passed through a generic function, and handed to a task as a `spawn`
argument; matching a payload binds the capability that was stored, and creates none. The two
exceptions:

- **No equality.** `==` on a capability is `N0304`, and a record or enum containing one derives
  none: two authority values are not data to compare.
- **No representation a program can read.** No built-in takes one, and nothing converts one
  to an `Int` or a `Str`, so a capability cannot be printed, written to a file or persisted.

**There is no revocation, deliberately (N79).** A capability has no identity: copies are
indistinguishable and nothing records a grant, so there is nothing to revoke. Revocation would
need a cell per grant and a check at every authorised call — runtime state the static model does
not have. A program that needs a revocable grant passes ordinary data it can invalidate, such as a
channel it closes; that is data, not authority.

#### No inherited authority (N104)

**Every function holds what it exercises.** A built-in that needs a capability, a `spawn`, a
foreign call — each needs one of its kind held at the site, in every function, whether it
declares an effect set or not. Missing authority is `N0369`, at the site, naming the function and
the kind. A call to a function needs nothing at the call: whatever that function exercises it
holds itself, so its authority is in its parameters, and type checking has already made the
caller supply them. `main`'s parameters are the root: the runtime hands `main` one capability per
parameter, and nothing else is a source.

*Until N104* a function with no declared set exercised its **caller's** authority — N37's
compatibility bridge, bounded so that code with a contract could not reach authority through code
without one, and recorded since N79 as each function's *inherited authority*. N104 migrated every
program in the repository, the compiler written in Nazm among them, to take its authority as
parameters, and removed the bridge: `fn main() -> Int { print("x"); 0 }` is now refused, and
`fn main(out: OutCap) -> Int { print("x"); 0 }` is how it is written. The `authority` profile's
`explicit-authority` rule is the language's rule; it keeps its name and can no longer fire.

What remains of the bridge is its **effect** half, which grants nothing: a function in another
module that declares no effect set is assumed to have every effect, `{ io, spawn }`, because its
interface publishes no set.

#### Across modules

**A caller never needs another module's body to know what authority it requires.** A
capability parameter is an ordinary parameter of a built-in type, so it is in the published
interface as any parameter is, and an importer checks its call against it. Changing which
capability a function takes — adding one, removing one, or trading `IoCap` for `SpawnCap` —
changes the interface, and every importer is checked again.

#### What is checked, and what is not

**The checking is static, and nothing at runtime enforces it.** A capability erases: each
lowers to a machine word holding 0, which nothing reads, and a program behaves identically
interpreted and compiled, with its capabilities or with `Int`s in their place. Erasure permits
no forgery, because forgery is a question the checker answers before anything runs: no
expression produces a capability, so there is nothing at runtime to fake.

There is **no sandbox**. A Nazm program can affect its machine exactly as much as the process
running it can; an operating-system boundary, a broker or a resource quota is not part of the
language. What N37 establishes is narrower and exact: **Nazm v1 statically represents and
checks explicit compiler-owned authority** — for every function that declares its effects —
and labels everywhere else as the ambient bridge it is. *Since N104* there is no bridge: every
function's authority is checked.

Not in v1, and not implied by anything above: attenuation (there is no narrower capability to
derive one into), resource-specific authority, revocation, linear or affine capabilities,
capability polymorphism, user-defined capabilities, restriction profiles, and authority for a
foreign function or unsafe code (*Open*, *Capabilities*).

### Foreign functions: calling C
<!-- id: foreign -->

A **foreign function** is a C function a program calls. It is declared by its signature and its
C symbol, and it has no body: the body is C's, compiled by a C compiler and handed to the linker
with `nazm build --link FILE`. Nazm does not compile C, and does not check it.

```nazm
extern "C" fn c_add(a: Int, b: Int) -> Int = "c_add";
extern "C" fn c_is_even(n: Int) -> Bool = "c_is_even";

fn classify(f: ForeignCap, n: Int) -> Int ! { foreign } {
    if c_is_even(n) { c_add(n, 1000) } else { c_add(n, 100) }
}

fn main(io: IoCap, f: ForeignCap) -> Int ! { io, foreign } {
    print(int_to_str(classify(f, 3)));
    0
}
```

**The boundary is the C convention of the target, and only two types cross it.** `Int` is C's
`int64_t` and `Bool` is C's `bool`, both by value. Nothing else is FFI-safe in v1 (`N0381`): a
`Str` is not a C string (it is counted, not terminated, and reference-counted); a record or an
enum has no C layout Nazm promises; a sequence or a channel is the runtime's; and a capability is
authority, never a word C could forge or keep. No pointer crosses, so C cannot retain, free or
alias anything Nazm owns, and nothing Nazm owns needs pinning across a call. No callback crosses
either: Nazm has no function values. *Corrected 2026-10-07, R1:* this is the v1 boundary; later
sections widen it — strings and exports (N55), handles, C structs and callbacks that are an
export's name (N85).

**The declaration.** `extern` begins it, before the ABI string — `"C"`, the only ABI (`N0380`);
`extern` is recognised only there, and is not otherwise reserved. The symbol after `=` is the C
identifier the linker resolves, exactly as written: ASCII letters, digits and `_`, not starting
with a digit (`N0382`). A foreign declaration takes no type parameters and writes no effect set
(`N0384`): its effect is always `foreign`. Two declarations of one symbol must agree about its
signature, and none may name a symbol the native runtime itself declares, such as `malloc` or
`main` (`N0383`). A `pub` foreign declaration is exported like any function, and the persisted
interface carries its symbol.

**Calling one is an effect and needs authority.** A foreign function's effect is `foreign`: a
function whose declared set leaves it out cannot call one (`N0366`). And **every** call to one
needs a `ForeignCap` in scope, whether or not the calling function declares an effect set
(`N0369`): C can do anything, and authority is never inherited — which N104 made true of every
kind, as it was of this one from the start. A function in another module that declares no
effects is assumed to call C only if one of its parameters can carry a `ForeignCap`.

**Failure and ownership.** C cannot fail the Nazm way: after a foreign call nothing is checked,
and a C function that aborts, loops or corrupts memory does so outside every Nazm guarantee. An
integer overflow inside C is C's. What Nazm owns stays Nazm's across the call: values held in
the calling function are released on every path exactly as around any call.

**Provenance.** What C returns carries the origin `unknown`, the worst there is, together with
whatever its arguments carried: it may be anything, and may be derived from what it was given. So
in a function that declares its effects, a `write_file` path computed from a C result is refused
(`N0372`), exactly as one read from a file is. What C does with an argument is not a Nazm sink —
it is authorised by the `ForeignCap` the call needed, and outside what a restriction can see.

**Where it runs.** Only in a native build. `nazm run --interpret` refuses a program that calls a foreign
function before running any of it (`N0385`); one that only declares one runs. `nazm build`
declares each foreign function in every unit that calls it and defines it in none; a missing
definition is the linker's refusal, reported as a missing `--link` naming the symbols. Both
backends call C the same way.

### Provenance: where a value came from, and where it may go
<!-- id: provenance -->

A value's **provenance** is the set of **origins** its contents were derived from by explicit
data flow. It is a checker fact, computed from resolved bodies and erased before anything
runs: not an effect (reading a file is `io`; the text read is not), not a capability (knowing
a value came from the root authority grants none), not a type, and not a runtime tag.

```nazm
fn lines(text: Str) -> Int ! {} {
    let mut n = 0;
    let mut i = 0;
    while i < str_len(text) {
        if str_byte(text, i) == 10 { n = n + 1; }
        i = i + 1;
    }
    n
}

fn main(io: IoCap) -> Int ! { io } {
    let text = read_file("notes.txt");      // from a file's contents
    write_file("count.txt", int_to_str(lines(text)));   // a path the program chose: allowed
    0
}
```

#### The origins

Four, and the compiler's; there are no user-defined labels. The absence of every origin is
**local**: a literal, or anything computed only from literals.

| Origin | Where it enters |
| --- | --- |
| `argument` | `arg` and `args_count`: the command line, supplied by whoever ran the program |
| `file` | `read_file`'s contents and `file_exists`'s answer: supplied by whoever could write that file |
| `authority` | a capability `main` is handed (N37): the runtime's root authority |
| `unknown` | a device register, what a foreign (C) function returns, what C hands an export, or a module whose facts are missing: what the checker does not follow |

An origin is identified by the compiler, and a built-in's by an exhaustive table on its
identity — never by spelling, and no program can write a label. Output is not input: `print`,
`eprint` and `write_file` are not sources. v1 is coarse: an origin is a *kind* of source, not a
particular argument or file, and a diagnostic names where each one was first met.

`unknown` is explicit and never trusted: it may be any of the others, and a restriction treats it
as the worst of them. Since N80 a container's contents are followed (*Containers* below), so a
value read out of one is no longer `unknown` for that reason.

#### How provenance flows

**Explicit data flow only.** A value carries what it was computed from:

- a binding, everything ever assigned to it — flow-insensitively, to a fixed point, so a loop is
  not a special case;
- an operator, both operands'; a built-in that computes, its arguments';
- a record or a variant, its fields'. **Within a body** (N80) a binding assigned only values built
  there keeps each field apart, by variant and field name: a field read, a pattern's binding and
  `?` read that field's flow, and a variant never assigned to the binding gives nothing. A value
  handed in, returned, stored or passed is **whole**: its fields are all of it. A write through a
  place, `a.b = v`, makes the binding whole;
- a call, its callee's summary applied to its arguments; a call through a function value or a
  trait bound, every candidate's (*Calls through values and bounds* below);
- an `if` or a `match`, its arms'. **The condition contributes nothing**: branching on a value is
  implicit flow, which v1 does not track.

A **join** is a set union: commutative, associative and idempotent, so the order values are
combined in changes nothing they carry.

#### Containers (N80)

Every container has a **cell**, named by its type: `Ints`, `Strs`, `Vec[T]`, `Chan` or `Chan[T]`. A
cell holds the join of everything written into **any** container of that type, anywhere in the
program: by `ints_push`, `ints_set`, `strs_push`, `strs_set`, `vec_push`, `vec_set`, `chan_send`
and `chan_send_of`. Every read — `ints_get`, `ints_pop`, `ints_len`, `ints_sum`, the `strs_` and
`vec_` reads, `str_join`, `chan_recv`, `chan_recv_of` — carries its cell, and its other
arguments'. A receive and a select **move** a value: the channel's cell is written into `out`'s.
A select's index says which channel was ready, which is control and carries nothing, as a
condition is.

```nazm
let xs = strs_new();
strs_push(xs, arg(1));
write_file(strs_get(xs, 0), "out")   // the command line, through a `Strs`: allowed
```

This needs no alias analysis and misses nothing: no foreign function takes a handle, the runtime
fills none, and so a container of type `T` holds only what was written into containers of type
`T`. It is coarse where two containers share a type — one `strs_push(ys, read_file(…))` anywhere
reaches every `Strs` read — and the diagnostic names that write. A container whose type mentions
a type parameter, in a generic body, is **every** container: a write there reaches every cell, and
a read there reads all of them.

#### Calls through values and bounds (N80)

A call through a function value may run any function value the program makes **of a matching
type**: a named function written as a value, or a closure. Types match up to effects, since a
value with fewer effects may stand where more are allowed (N79), and exactly otherwise; a type that
mentions a type parameter matches every function value. A call through a trait bound may run every
impl's method of that trait method. The result carries what every candidate returns, given these
arguments; a candidate's parameters receive them. A **closure's parameters** are what those calls
hand it, and its **captures** what its maker held where it wrote it — each an ordinary fact,
followed like a parameter.

#### Function summaries

A function's provenance is summarised, over the whole program, as three facts:

- the **origins** its result carries whatever it is given;
- the **parameters it passes on**: those its result is computed from. `fn id(x: Str) -> Str { x }`
  passes on `x`; `fn len(x: Str) -> Int { str_len(x) }` does too; `fn one(x: Str) -> Int { 1 }`
  passes on nothing, and its result is local whatever it is handed;
- the **parameters that reach a restricted sink** (below).

Summaries are the **least fixed point** over every call, `spawn`, recursion, mutual recursion and
import cycle: they start empty and grow until nothing changes, over a finite set of origins and
parameter positions, so they always settle, and source order changes nothing. A caller reads its
callee's summary, never its body.

#### Across modules

A module's bodies are reduced to **facts** — each function's flows in terms of its parameters and
the calls it makes — which depend only on the module's source and its imports' declarations, as
its checking does. A cache keeps them beside the module's other results, so a module whose bodies
are not checked again still contributes them. Every run then solves the whole program's summaries
from all modules' facts: a callee's changed body changes what its caller is told in the same run,
though the caller was not checked again. An importer never needs another module's body.

#### Provenance, effects and authority are three facts

External data is not an effect: once read, computing over it is pure, and `lines` above declares
`! {}`. Provenance never adds an effect, never removes one, and never grants authority — a value
known to have come from an `IoCap` is not an `IoCap` in scope. Passing, storing and spawning with a
capability keep its `authority` origin: a parameter records the origins its callers hand it, so a
task's capability is recorded as the spawner's.

#### The restricted flow

**In a function that declares an effect set, the path given to `write_file` must not derive from
a file's contents or from what the checker does not follow** (`N0372`). It may come from the
command line — whoever ran the program already holds every authority it has — or be computed in
the program. The rule reaches through calls: a function whose parameter reaches `write_file`'s
path, however many calls deep and whether it declares effects or not, carries the rule to its
callers, and the call that hands it a file's contents is refused there — a direct call, a call
through a value that may run it, or the closure that captures it (N80). The diagnostic shows one
chain, to the `write_file` and to where the contents were read, through every call, container and
capture on the way. There is no automatic fix; `str_vouch` under a `VouchCap` declassifies on
purpose (N52).

**One policy (N80).** The rule above is one rule of a **policy**: a sink class, the origins it may
not receive, and where it applies — in functions that declare an effect set, or everywhere. The
language's policy is that one rule. A profile adds rules of the same shape and is checked by the
same engine: `cyber`'s `contents-stay-in-files` (standard output may not receive a file's contents)
and `checked-paths` (the language's rule in every function, so the bridge cannot write a path from
a file's contents either). Each refusal shows the same chain.

**Traces (N80).** `nazm explain-flow` prints every sink with the origins that reach it and, for
each, one chain from where it entered to the sink; `--json` prints `nazm.flow/2`.

A function that declares no effect set has its own writes left unchecked for restricted flows
(N38); explicit code still cannot launder a file's contents through it.

Authority needs no restriction of its own here: no built-in accepts a capability and none
converts one to data, so authority reaching output, a file or a channel is already impossible by
type, and provenance does not claim to enforce what types already do.

#### What this is not

**Explicit information-flow provenance**, statically, with one language rule and profile rules of
the same shape. Not non-interference — implicit flow through conditions, and which channel a
select found ready, are not tracked — and not taint tracking at runtime, confidentiality
enforcement, side-channel or constant-time guarantees, or a sandbox. Containers are precise by
type, not by handle or index; records and variants by field within a body, not across a call; a
call through a value by type, not by the value. There are no user labels and no policy language a
program writes.

### The integer boundaries, stated rather than inherited
<!-- id: integer-boundaries -->

Two's complement is asymmetric — there is one more negative `Int` than positive — and
every language that leaves the consequences to the hardware gets a rule with an exception
in it. Nazm states all four cases:

| expression | result | why |
| --- | --- | --- |
| `-9223372036854775808` | the smallest `Int` | The sign folds into the literal. The magnitude alone is out of range, so lexing the digits and negating afterwards would make the smallest `Int` unwritable — an arbitrary hole in the range |
| `-9223372036854775809` | error, out of range | The fold checks the magnitude rather than assuming any digits after `-` will fit |
| `-x` where `x` is the smallest `Int` | **traps** | The result has no representation. Traps in every build, like all overflow |
| `x / -1` where `x` is the smallest `Int` | **traps** | The quotient is one past the largest `Int`. On x86 this faults in hardware; a checked division is what turns it into a diagnostic instead of a dead process |
| `x % -1` where `x` is the smallest `Int` | **0** | Mathematically zero, and zero is representable. The same hardware instruction faults on it and Rust's `checked_rem` reports nothing, but inheriting that would make the rule *"`%` fails on a zero divisor, and also on one exact pair of operands"* — a wart to memorise for no gain |

The through-line: `%` has exactly one failure case, a zero divisor. Overflow traps
wherever the result genuinely cannot be represented, and nowhere else.

### One mistake, one error
<!-- id: one-error -->

A parse error inside a function body does not delete the function. Its signature is kept,
so calls to it still check against real parameter and return types instead of reporting
that it does not exist — which would be several errors for one mistake, the exact failure
this language is meant to avoid. Nothing is said about the body itself: statements that
were never built would be claims about the parser's guess, not about the program.

Since N15 the parser resumes after a malformed statement, so a second, independent syntax
error in the same body is reported too; what follows from the first — a stray token left
behind by a missing brace — is not reported again. The body is still withheld from checking.

### Logical operators — `&&`, `||`, `!` — N49
<!-- id: logical-operators -->

| | |
| --- | --- |
| **Syntax** | `!e` is a prefix, beside unary `-`. `a && b` and `a \|\| b` are infix and left-associative |
| **Precedence** | `\|\|` binds loosest, then `&&`, then the comparisons, then `+ -`, then `* / %`, then the prefixes: `a == b && c < d \|\| !e` is `((a == b) && (c < d)) \|\| (!e)` |
| **Types** | Every operand is `Bool`, and so is the result. Anything else is `N0304`, as for every operator |
| **Short-circuit** | The left operand is evaluated first. `a && b` evaluates `b` only when `a` is `true`; `a \|\| b` only when `a` is `false`. So the right operand's effects, failures and non-termination happen only then |
| **Meaning** | `a && b` is `if a { b } else { false }`, `a \|\| b` is `if a { true } else { b }`, and `!e` is `if e { false } else { true }` — not as an analogy: that is what they are, in every analysis and every implementation |
| **Not** | No bitwise operators, no `and`/`or`/`not` words, and no truthiness: `!0` is a type error |

### Qualified names — `use "PATH" as NAME;` and `NAME::item` — N49
<!-- id: qualified-names -->

```nazm
use "@std/text" as t;
use "geometry:shapes.nz" as g;

fn main() -> Int { str_len(t::text_trim(" a ")) + g::area(g::Point(x: 2, y: 3)) }
```

| | |
| --- | --- |
| **Importing under a name** | `use "PATH" as NAME;` loads the module as `use "PATH";` does, and binds `NAME` to it in the importing module. **Its exports are not brought into scope unqualified**: they collide with nothing, and only `NAME::item` names them |
| **Where a qualified name is written** | Wherever a definition is named: a call `NAME::f(…)`, a construction `NAME::R(…)`, a variant `NAME::E.V(…)`, and a type `NAME::R` or `NAME::E[Int]`. Nazm has no module-level values, so a qualified name is never a variable |
| **What it means** | Exactly the definition the unqualified import would have named — the same identity, signature, effects and authority. Qualification chooses a definition; it does not create one |
| **Scope** | An alias belongs to the module that writes it, as an import does. Two aliases may name one module. A module imported both plainly and under a name is one module, seen both ways |
| **Refused** | An alias that is also one of the module's own definitions, a type, or another alias (`N0209`); `NAME::item` where `NAME` is no alias or the module does not export `item` (`N0210`) |
| **`as`** | Recognised only after a `use` path. It takes no identifier from programs |
| **Built-ins and the prelude** | Not modules a program imports, so never qualified: `str_len`, `Result` and the rest are written as before |

### Function values and closures — N50
<!-- id: function-values -->

```nazm
fn double(n: Int) -> Int { n * 2 }
fn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }

fn main() -> Int {
    let step = 3;
    let add = fn(x: Int) -> Int { x + step };
    twice(double, 5) + twice(add, 1)
}
```

| | |
| --- | --- |
| **Type** | `fn(T1, …, Tn) -> R`, with `! { e… }` after `R` for a function that performs effects. Unwritten effects are none. Two function types are one exactly when their parameters, result and effects are equal |
| **Effect subsumption** (N79) | Where a function value is passed as an argument or returned — never to a task (`N0321`) — a value whose effects are a **subset** of the expected type's is accepted: `fn(Int) -> Int ! {}` where `fn(Int) -> Int ! { io }` is expected. Parameters and results stay invariant, a type with an effect parameter is matched only by binding, and inside another type (`Vec[fn() -> Int]`) nothing is converted. A value with more effects than expected is `N0300` |
| **Equality** | None. `==` on a function value is refused, as on a capability |
| **A named function as a value** | A non-generic function that is not foreign, written without a call: `double`. Its type is its signature's. A generic function, a built-in and a foreign function are not values (`N0387`); write a closure that calls one. Since R1b (L10) one may be made inside a generic function too |
| **A closure** | `fn(x: Int) -> Int { … }` in expression position, its parameters and result written in full, its effects `! { … }` declared or none. Its body is checked against its declared effects as a declared function's is |
| **Capture** | A closure may name the bindings of the functions and closures around it. Each it names is **copied** into the closure when the closure is created: a value is a copy, a handle (`Ints`, `Vec`, `Chan`) is the same storage. A capability may be captured, since N51 (*Effect parameters*, **Capture**, below); until N51 it could not, and Q1 corrected this row, which still said so |
| **Capture of a `let mut` binding** (R1b, L10) | **Shared**, not copied: the function and every closure that names it read and write one variable, kept in a `Cell` (*A mutable cell*) — a write through either is seen through both, before and after the closure is made, and the closure may outlive the binding's scope, since the cell is counted. A binding whose type can hold a function value may not be captured (`N0386`): the closure could be stored into it and own itself. Until R1b every `let mut` capture was refused, because a copy would silently stop tracking the binding |
| **Capture of a function-value container** (Gate 1-C1) | A closure may not capture a value that can hold a function value behind a counted handle — a `Vec[fn(…) -> R]`, a `Vec` of records or enums with a function field, a record or enum holding either (`N0616`). Such a capture is the only way a closure could come to own itself; *The memory constitution*'s *Cycles* gives the argument. Storing a function value into a `Vec` is never refused |
| **Call** | `f(a, b)`, where `f` is a parameter or binding of function type: the arguments must match its parameters, the result is its result, and the call performs its type's effects. Such a binding hides a function of the same name for calls in its scope. A function stored in a field is called by binding it first: `let g = r.f; g(1)` |
| **Ownership** | A function value is a handle to its closure; copies share it, and what it captured is released when the last copy goes |
| **Tasks** | A function value may not be passed to a task (`N0321`) |
| **In a generic function** | Since R1b (L10) a closure, and a named function as a value, may be written in the body of a generic function, and a closure's types may name the function's type parameters: `nazm run` is parametric, and `nazm build` instantiates each closure with the function it is written in, through every closure between. Until R1b both were refused (`N0387`) |
| **Recursion through a value** | A native function that calls through a function value checks its stack first, as a recursive one does (`N0408`, *Native recursion*) |
| **Not here** | Traits, methods, generic function values, a closure that names itself, effect-polymorphic function types — see `architecture.md` §7.52 |

### Effect parameters and captured authority — N51
<!-- id: effect-parameters -->

```nazm
use "@std/seq";

fn main(io: IoCap) -> Int ! { io } {
    let xs = vec_new[Int]();
    vec_push(xs, 1);
    vec_push(xs, 2);
    let shown = vec_map(xs, fn(x: Int) -> Int ! { io } { print(int_to_str(x)); x * 10 });
    vec_len(shown)
}
```

| | |
| --- | --- |
| **Declaring one** | `fn name[T…, effects E](…)`: at most one effect parameter, after the type parameters. Its name may be written in the effect sets of the function's own signature and of closures in its body. `main` may not have one |
| **In the body** | Opaque. Calling something whose type has `E` performs `E`, so the declared set must include it; `E` needs no capability |
| **At a call** | Bound from the arguments: a parameter typed `fn(…) -> R ! { k…, E }` given a value of type `fn(…) -> R ! { a… }` binds `E` to `a…` without `k…` (and `k…` must be within `a…`). Bound to two different sets is `N0389`; bound by nothing, it is empty. The call performs the callee's declared set with `E` replaced |
| **Capture** | A closure may capture a capability; its body then holds that authority as well as its own capability parameters'. Its type still declares what it does. A `let mut` binding is shared since R1b, unless it can hold a function value (`N0386`; *Function values and closures*) |
| **Values of undeclared functions** | Making one needs, where it is made, the capabilities its type's effects need, as calling the function would (`N0369`) |
| **Not here** | Two effect parameters, effect subtyping, attenuated capabilities, revocation — see `architecture.md` §7.53 |

### Declassification — N52
<!-- id: declassification -->

```nazm
fn main(io: IoCap, v: VouchCap) -> Int ! { io } {
    let secret = read_file("token.txt");
    let shown = str_vouch(str_slice(secret, 0, 4));
    print(shown)
}
```

| | |
| --- | --- |
| **`str_vouch(s: Str) -> Str`** | Its argument, with no origin. Needs a `VouchCap` held where it is written, in every function (`N0369` otherwise) |
| **`VouchCap`** | A capability kind, minted only for `main`'s parameters; no effect is authorised by it |
| **Evidence** | Every vouch is recorded with the origins it cleared; `nazm explain-flow` prints them beside every sink |
| **Policy** | The language refuses one flow (`N0372`); profiles refuse more: `cyber`'s `contents-stay-in-files`, `critical`'s `no-declassification` |

### Generic channels — N53
<!-- id: generic-channels -->

```nazm
struct Job { name: Str, size: Int, }

fn worker(jobs: Chan[Job], done: Chan[Str]) -> Int ! {} {
    let got = vec_new[Job]();
    while chan_recv_of(jobs, got) {
        let j = vec_pop(got);
        chan_send_of(done, str_concat(j.name, "!"));
    }
    chan_close_of(done)
}
```

| | |
| --- | --- |
| **Type** | `Chan[T]`, for a `T` that may cross into a task — the rule a `spawn` argument follows (`N0390` otherwise). `Chan` alone is the `Int` channel, unchanged |
| **Built-ins** | `chan_new_of[T](capacity: Int) -> Chan[T]`, `chan_send_of(c, v: T) -> Bool`, `chan_recv_of(c, out: Vec[T]) -> Bool`, `chan_close_of(c) -> Int` — `Chan`'s semantics exactly: blocking, closing, draining |
| **Ownership** | A send copies; a value a closed channel refuses is released; a receive moves the value into `out`; a channel destroyed with values queued releases each |
| **Equality, tasks** | As `Chan`: no equality; may cross into a task |

### Select, deadlines and cancellation — N54
<!-- id: select-deadlines -->

```nazm
fn worker(stop: Chan[Int], jobs: Chan[Int], out: Chan[Int]) -> Int ! {} {
    let cs = vec_new[Chan[Int]]();
    vec_push(cs, stop);
    vec_push(cs, jobs);
    let got = vec_new[Int]();
    let mut running = true;
    while running {
        let i = chan_select_of(cs, got);
        if i == 1 && vec_len(got) > 0 {
            chan_send_of(out, vec_pop(got) * 2);
        } else {
            running = false;
        }
    }
    chan_close_of(out)
}
```

| | |
| --- | --- |
| **`chan_select_of(cs: Vec[Chan[T]], out: Vec[T]) -> Int`** | Waits until a channel in `cs` is ready — holds a value, or is closed and drained — and returns the lowest ready index; a value is moved onto `out`, a closed channel gives nothing. `-1` for an empty `cs`. Schedule-dependent, deterministically tie-broken |
| **`chan_select_until(cs, out, ms: Int) -> Int`** | As `chan_select_of`, or `-2` once at least `ms` milliseconds pass; `ms <= 0` looks once. No effect; needs a `TimeCap` held where it is written, in every function (`N0369` otherwise) |
| **`time_now_ms() -> Int`** | A monotonic clock, in milliseconds. No effect; needs a `TimeCap` |
| **`TimeCap`** | A built-in capability type, like `IoCap`; `critical`'s `no-ambient-time` refuses any use |
| **Cancellation** | Cooperative: close a channel the task selects on. Observed only where a task waits; a sibling's failure cancels nothing (N44 unchanged) |
| **Counters** | `NAZM_SCHED_REPORT=1`: `nazm-sched: spawned=… joined=… selects=… timeouts=…` on standard error at exit |

### Foreign functions v2: strings in, exports out — N55
<!-- id: ffi-v2 -->

```nazm
extern "C" fn c_strlen(s: Str) -> Int = "strlen_of";

pub extern "C" fn nazm_triple(x: Int) -> Int ! {} = "nazm_triple" { x * 3 }
```

| | |
| --- | --- |
| **`Str` argument** | A NUL-terminated copy for the call's duration; C borrows it. A `Str` holding a NUL byte fails the call before C runs (`N0405`). A `Str` result was refused (`N0381`) until N85, which copies it (*Foreign functions v3*) |
| **Export** | `pub extern "C" fn … ! {} = "symbol" { … }`: a C-callable function of numbers and `Bool`s, public, effect-free and not generic (`N0391` otherwise). Since Gate 2 every number crosses as its C type — `int8_t` … `uint64_t`, `float`, `double` — which the generated header names (`general-purpose.md` §23); an `Int128` or `UInt128` does not (R1b, L13b: C has no standard type for one) |
| **Failure in an export** | Reported on standard error; the process exits with status 2. Nothing unwinds into C |
| **Library** | `nazm build --lib FILE -o OUT.a` writes the static archive and `OUT.h`; no `main` needed |

### Foreign functions v3: handles, C structs, strings back, `errno`, callbacks — N85
<!-- id: ffi-v3 -->

```nazm
extern "C" struct Db;                                   // an opaque handle: a C pointer
extern "C" struct Point { x: Int, flag: Bool, y: Int }  // laid out as C's struct, in this order

extern "C" fn db_open(path: Str) -> Option[Db] = "db_open";   // null is `None`
extern "C" fn db_errmsg(d: Db) -> Str = "db_errmsg";          // copied; null is `N0409`
extern "C" fn plot(p: Point) -> Int = "plot";                 // `const struct Point *`, borrowed
extern "C" fn each(f: fn(Int) -> Int, n: Int) -> Int = "each"; // a C function pointer

pub extern "C" fn twice(x: Int) -> Int ! {} = "nz_twice" { x * 2 }

fn go(f: ForeignCap) -> Int ! { foreign } { each(twice, 3) + c_errno() }
```

| | |
| --- | --- |
| **Opaque handle** | `extern "C" struct Name;` — a pointer Nazm never reads, frees or compares. Only a foreign result makes one; building one or reading a field is `N0611`, `==` is `N0304`, and it never crosses into a task, by capture (`N0321`) or channel (`N0390`). Otherwise an ordinary value: a local, a record field, a `Vec` element, a result |
| **C-layout struct** | `extern "C" struct Name { … }` — fields `Int` (`int64_t`), `Bool` (`bool`), a handle (a pointer) or another C struct, **in declaration order** with C's alignment; any other field `N0381`, type parameters `N0384`, another ABI `N0380`. Passed to C as a `const struct Name *` to a copy made for the call and freed after it. A struct by value, a struct result, unions, arrays and bit-fields do not cross |
| **Nullability** | A result `-> Db` or `-> Str` promises non-null: a null fails the call with `N0409`, checked before Nazm sees it. `-> Option[Db]` and `-> Option[Str]` make null `None`. An argument is never null; `Option[Db]` as an argument is `N0381` |
| **`Str` result** | A `const char *` C still owns, copied to its NUL into a `Str` Nazm owns, the moment the call returns. A string the caller must free is bound as a handle and its free function |
| **`errno`** | `c_errno() -> Int` is what the latest foreign call on this thread left, captured as it returned. It performs `foreign` (a `ForeignCap`); hosted targets only (`N0613` on a freestanding or WebAssembly build); `nazm run --interpret` refuses it as it refuses calling C (`N0385`) |
| **Callback** | A foreign parameter of type `fn(…) -> …` over `Int` and `Bool` is a C function pointer. Its argument is an export's name — never a closure or another function (`N0612`); C may call it during the call, after it, and from any thread. A failure in it ends the process (status 2) |
| **Shared library** | `nazm build --lib --shared FILE -o libNAME.dylib` (or `.so`): exports are its only visible symbols; it names itself `@rpath/NAME` (the soname `NAME` elsewhere); the same header as `--lib` |
| **Not crossing** | Floating point, integers other than `int64_t`, variadics, by-value structs, closures as callbacks, `dlopen`, ABIs other than `"C"` — each refused by name |

### Function contracts — N87
<!-- id: function-contracts -->

```nazm
fn isqrt(n: Int) -> Int ! {}
    requires n >= 0
    ensures result >= 0
    ensures result * result <= n && n - result * result <= 2 * result
{ … }
```

| | |
| --- | --- |
| **Clauses** | After the effect set and before the body: any number of `requires E`, then any number of `ensures E`. `requires` and `ensures` are words with that meaning only there |
| **What a clause is** | A `Bool` (`N0300` otherwise) of literals, the parameters, `result` (in `ensures`: the value being returned), operators, field reads and calls of pure functions — ones declaring `! {}` that are not foreign, and the built-ins that only read (`str_len`, `str_byte`, `ints_len`, `ints_get`, `strs_len`, `vec_len`, `vec_get`). Anything else — an effect, an allocation, a closure, a construction, a branch, a method call — is `N0614` |
| **When** | Every `requires`, in order, as the function is entered, after its arguments; every `ensures`, in order, as it returns — from its final expression, a `return` or a `?` — with `result` the returned value. Never compiled out: the same under `nazm run`, both backends, every optimisation level and every profile |
| **Failure** | A false `requires` fails the call with `N0410`, a false `ensures` with `N0411`, at the clause, as every failure fails (status 2 natively). A clause that itself fails — an overflow in it — fails the call with that failure: write clauses in `Int` as code is written |
| **Proved at compile time** | A call whose arguments are all literals, of a function of the same module, is checked against its `requires` by evaluating them: false is `N0615` at the call, true makes the call's obligation proved. Nothing else is reasoned about |
| **Across modules** | An export's precondition count is in its interface (`nazm.interface/10`): an importer's calls of it are obligations, checked as they run |
| **`nazm obligations FILE`** | `nazm.obligations/1`: every precondition, postcondition, call of a function with a precondition, overflow, division, index, allocation, call through a function value and call into C, each `proved` (with its witness), `checked` or `unknown`. Compiler evidence, not certification |
| **Profiles** | `critical` and `cyber` add `no-unknown-calls`: no call through a function value, so with `no-foreign` every obligation is proved or checked |

### Packages v2: a registry and version requirements — N56
<!-- id: registry -->

```toml
schema = "nazm.package/1"

[package]
name = "app"
version = "1.0.0"
main = "main.nz"

[registry]
path = "../registry"

[dependencies]
shapes = { version = "^1.2.0" }
geometry = { path = "../geometry", version = "0.3.1" }
```

| | |
| --- | --- |
| **Requirement** | `1.2.3` / `=1.2.3` exactly; `^1.2.3` the same left-most non-zero component; `~1.2.3` the same minor |
| **Choice** | a backtracking search (N90): packages in name order, each one's non-yanked versions newest first, the lockfile's first while it satisfies; the first assignment satisfying every requirement; one version per name; at most 10,000 candidates (`N0513` otherwise, naming the package and every requirement on it with who placed it) |
| **Store** | `REGISTRY/index/NAME.toml` and an immutable copy per version, checked against its digest (`N0507`), refused if it holds a link (`N0514`) or its index is malformed (`N0512`) |
| **Commands** | `nazm publish DIR --registry REG`; `nazm yank NAME@VERSION --registry REG [--undo]`; `nazm lock`, `--locked` as N46; `nazm update DIR [NAME…]` re-resolves without the lockfile's preference for the named packages (all without names) and prints each change |
| **Imports** | unchanged: `use "shapes:polygon.nz";` |

### A freestanding target: device registers and no operating system — N62
<!-- id: freestanding -->

```nazm
fn put(m: MmioCap, c: Int) -> Int ! {} { mmio_write32(150994944, c) }

fn main(m: MmioCap) -> Int ! {} { put(m, 72); put(m, 105); 0 }
```

| | |
| --- | --- |
| **`aarch64-unknown-none`** | A 64-bit ARM machine with no operating system, verified on QEMU's `virt` board. `nazm build --target aarch64-unknown-none --objects DIR` writes the objects, `link.txt` and `link.ld`; LLVM backend only |
| **What builds** | `Int` and `Bool`, records and enums of them, functions — recursion too, under a stack check against the image's own 64 KiB stack (`N0408` past it) — loops, static strings, and device registers. A program that reaches allocation, input and output, tasks, channels, closures or the C library is refused, naming what it reaches |
| **`mmio_read32(addr: Int) -> Int`** | One volatile 32-bit load from a device register, zero-extended. No effect; needs an `MmioCap` held where it is written, in every function (`N0369` otherwise) |
| **`mmio_write32(addr: Int, value: Int) -> Int`** | One volatile 32-bit store of `value`'s low 32 bits; `0`. As `mmio_read32` |
| **`MmioCap`** | A built-in capability type, like `TimeCap`; handed to `main` |
| **`mmio_read8`, `mmio_write8`, `mmio_read16`, `mmio_write16`** (N86) | The same at 8 and 16 bits: one volatile access of exactly that width, zero-extended on read, `value`'s low byte or half written; never merged, split or reordered against another. A byte-wide device (a 16550 UART) needs the byte access: a 32-bit store writes its neighbours too |
| **Where there is no device** | `nazm run` and every hosted build refuse a program that calls any of them (`N0392`) |
| **The board's contract** | `_start` sets the stack and enables floating point, calls `main`, writes its result in decimal and a newline to the board's UART, and stops the machine with status 0; a failure writes its diagnostic there and stops with status 2 |
| **`riscv64gc-unknown-none-elf`** (N86) | RV64GC in machine mode on QEMU's RISC-V `virt` board, no firmware: the image at `0x80000000`, the NS16550A UART at `0x10000000`, stopped by SiFive's test device at `0x100000`. What builds, and the contract, are the AArch64 board's; LLVM backend only, and only where `clang` has a RISC-V code generator (refused by name otherwise) |
| **`thumbv7m-none-eabi`** (Gate 3) | An Arm Cortex-M3 (ARMv7-M, Thumb-2) on QEMU's `mps2-an385`, the one MCU-class board: the image at `0`, its vector table first — the initial stack, `_start`, every other exception a fault reported and stopped — the CMSDK UART at `0x40004000`, enabled before `main`, and AArch32 semihosting's exit. Pointers are 32 bits and `Int` is still 64: 64-bit division and atomics are the board runtime's (atomics with interrupts masked), and so mean what they mean everywhere. It has no floating point and no 64-bit access, so a `Float32` or `Float64`, and `mmio_read64` or `mmio_write64`, are refused by name (`N0392`). LLVM backend only |
| **Boards** | A board is a description outside the language — load address, UART, how the machine stops, startup — printed by `nazm inspect` (`toolchain.boards`); a program names device addresses as integers, and no board changes what a program means. Every board is emulated: none is claimed on hardware |

### WebAssembly: a module whose outside is its imports — N64
<!-- id: webassembly -->

| | |
| --- | --- |
| **`wasm32-unknown-unknown`** | A WebAssembly 1.0 module from the LLVM backend. `nazm build --target wasm32-unknown-unknown --objects DIR` writes the objects, `link.txt` (its `wasm-ld` line), `host.mjs` and `bounds.json`; Cranelift builds none |
| **What builds** | As on the board: `Int` and `Bool`, records and enums of them, functions — recursion under a stack check against the module's own 64 KiB stack (`N0408`) — loops, static strings, `print` and `eprint` of them. Allocation, sequences, strings built at run time, channels, tasks, closures and every other outside built-in are refused, naming the part and its symbols |
| **Imports** | From `nazm_host` only: `write(fd, ptr, len)` where the program writes (an `IoCap`), and `report(ptr, len)`, always. No `IoCap` used, no `write` import |
| **Exports** | `memory` and `nazm_main() -> i64`: `main`'s result, or `0` after `report` |
| **Device registers** | None: `mmio_read32`, `mmio_write32` and their 8- and 16-bit forms are refused (`N0392`) |
| **The reference host** | `node host.mjs module.wasm` grants exactly `write` and `report`, refuses a module asking for anything else, prints the result and exits 0, or 2 after a failure — the interpreter's output and status |

### The interactive tier: `repl`, `comptime`, `reload-check` — N65
<!-- id: interactive -->

| | |
| --- | --- |
| **`nazm repl`** | One line at a time: a line beginning `fn`, `struct` or `enum` (or `pub` before one) defines; any other is an expression, evaluated as the body of a `main(io: IoCap)` and shown as an `Int`, a `Bool` or a `Str`. The session is checked as one program on every line; a redefinition takes its predecessor's place, and one that breaks another definition is refused naming it, the session unchanged. `main` is the REPL's. `:defs`, `:reset`, `:quit` |
| **`nazm comptime FILE FUNCTION`** | Evaluates a function of the file's module that has no parameters and is pure, under `--max-iterations` and `--max-depth`; prints its value. Any other function is refused (`N0393`); exhausting a budget is `N0402` or `N0403` |
| **`nazm reload-check OLD NEW`** | `nazm.reload-check/1`: each definition of the root module that changed, `reloadable` (a body, or something added) or `restart` (a signature — types or effects — a removal, or a record's or enum's fields by name and type), with why; exit status non-zero when any needs a restart |

### Accelerator kernels: `nazm accel` — N67
<!-- id: accel -->

| | |
| --- | --- |
| **A kernel** | A function of the file's module that is pure and maps one `Int` to an `Int`, reaching only functions that are pure functions of `Int`s and `Bool`s — no recursion, built-ins, records, enums, closures, tasks or C. Anything else is `N0394`, naming why and where |
| **A float kernel** (Gate 3) | The same of one float type: `(T) -> T` or `(T, T) -> T` with `T` `Float32` or `Float64`, reaching only functions of `T`s and `Bool`s, which may also call `float_sqrt` and `float_abs`. Its arithmetic is the language's, IEEE 754 and never failing — no contraction into a fused multiply-add, division and square root correctly rounded — so its results are held to the interpreter's **bit for bit**. A kernel mixing number types, or calling another built-in, is `N0394`. Its input is whitespace-separated decimal numbers, `inf`, `-inf` and `nan` among them |
| **The map** | `nazm accel FILE KERNEL --input FILE` applies it to each whitespace-separated `Int` of the input (at most 8,000,000) on the GPU: the results in order, or the failure of the lowest failing element — `N0400`/`N0401` at the operation's span, "at element *i*" — as the interpreter mapping in order would give |
| **A zip** (N88) | A kernel of two `Int`s, `(Int, Int) -> Int`, over two inputs: `--input A --input B`, of one length (`N0394` otherwise, before any device); element *i* of each its arguments. A kernel of three or more is `N0394` |
| **A reduction** (N88) | `--reduce add\|min\|max` folds the results into one `Int` in index order on the host after the transfer: `add` checked as `ints_sum` is — `N0400` where the running sum overflows, though the total fit — `min` and `max` never failing, `null` for no elements. Of a float kernel (Gate 3), in index order too: `add` the IEEE sum, `min` and `max` as `float_min` and `float_max`, a NaN losing to a number |
| **The report** | One `nazm.accel/3` line (Gate 3; `/2` before it): the kernel's `shape` (`map` or `zip`) and `element` type, the `provider` that ran it and why, device, `inputs` (each one's elements and bytes), each transfer (direction, bytes, ms), the synchronisation, build and run times, `reduce` (op, where, ms, value or failure), the kernel's identity, `reference` — the interpreter's agreement, results and fold, checked unless `--no-check` — and, where a device ran it, `cross_check`, the CPU provider's agreement |
| **Providers** (Gate 3) | A kernel's source is one text, written once: the OpenCL C that a device builds, and that the **CPU provider** compiles as C with a prelude naming OpenCL's few words, linked with a host that runs the element function over a pool of threads, one per core. `--provider opencl` runs it on macOS's OpenCL GPU, and refuses where there is none; a `Float64` kernel needs the device's `cl_khr_fp64`, and is refused without it. `--provider cpu` runs it on any host with the C compiler. `--provider auto`, the default, runs it on the GPU where one can run it and otherwise on the CPU provider, the report's `provider.reason` saying why — a fallback is stated, never silent. Where the GPU ran it and the check is on, the CPU provider runs it too, as a second oracle, and a difference fails as the interpreter's would |
| **SIMD** (Gate 3) | The CPU provider's loop over the elements is compiled with LLVM's vectoriser, so a kernel whose body it can vectorise runs in SIMD lanes; `--scalar` compiles it without, the report's `provider.vectorised` saying which. A comparison of the two is a measurement filed in `docs/performance.md`, not a promise |
| **Where it runs** | The OpenCL provider, macOS's GPU; the CPU provider, any host with `clang` and threads. `--print-kernel` shows the source on any host |

### Contracts: the `web3` profile and `nazm contract` — N69
<!-- id: contracts -->

| | |
| --- | --- |
| **`web3`** | A profile: `declared-effects`, `no-io`, `no-spawn`, `no-foreign`, `no-ambient-time`, `no-recursion`, `no-blocking` |
| **A contract** | A module with `pub struct State` (fields `Int`/`Bool`), `fn init() -> State`, and entrypoints `pub fn name(s: State, caller: Int, a: Int, …) -> Result[State, Int] ! {}`; optionally `fn conserved(s: State) -> Int`. Anything else is `N0395` |
| **Facts** | `nazm contract FILE` → `nazm.contract/1`: state fields, entrypoints, read and write sets (`"all"` where not followed) |
| **Transactions** | `nazm contract FILE --txs FILE`, one `CALLER ENTRY ARG…` per line: `Ok` commits; `Err(code)` reverts with `code`; a runtime failure reverts with its diagnostic code; changing `conserved` rejects (`N0396`); a revert or rejection leaves the state as it was. `nazm.contract-run/1`, the same bytes for the same inputs |

### The EVM backend — N70
<!-- id: evm -->

| | |
| --- | --- |
| **`nazm contract FILE --evm DIR`** | Compiles a contract to `deploy.hex` and `runtime.hex`, with `abi.json` (`nazm.evm-abi/1`: each entrypoint as `name(int64,…)`, its selector — the first four bytes of its keccak-256 — and a gas bound or `null`: since Gate 3 a loop in the counted form (N63) contributes its turns times its condition and body, and any other loop leaves the bound `null`), `storage.json` (`nazm.evm-storage/1`: field `f` at slot `keccak256("nazm.state." + f)`) and `evm_run.py`. Anything the backend does not emit is `N0399`, before emission |
| **Semantics** | Exactly the contract model's: `caller` is the EVM's `CALLER` (a sender address that is not an `Int` reverts, `N0397`); malformed calldata reverts (`N0398`); a runtime failure reverts with its five-byte code, an `Err(code)` with the 32-byte word `code`; a change to `conserved` reverts with `N0396`; every revert discards the transaction's writes |
| **Upgrades** | `nazm contract NEW --upgrade-check OLD`: `nazm.evm-upgrade/1`; a removed or retyped field is incompatible (`N0399`), an added field compatible, a reordering nothing |

### WASI: a module with a heap and WASI's imports — Gate 3
<!-- id: wasi -->

| | |
| --- | --- |
| **The target** | `nazm build --target wasm32-wasip1 --objects DIR`: WebAssembly with WASI preview 1, one thread. Its objects, `DIR/link.txt` with the `wasm-ld` line — the command `_start`, linear memory exported, a 1 MiB shadow stack — and `DIR/wasi.mjs`, Node's WASI host. Never linked by `nazm build`, whose host has no `wasm-ld`; `--backend cranelift` is refused as for any WebAssembly |
| **What it has** | A heap: a bump arena from the linker's `__heap_base`, grown by `memory.grow`, never returned, so strings built at run time, sequences, `Bytes`, joins and closures run as a board's with a heap do. Standard output and error (`fd_write`), and (R1d) their handles, `os_stdout` and `os_stderr`. Arguments (`args_get`; the module's own name is not among them). The monotonic and wall clocks and `time_sleep` (`clock_time_get`, `poll_oneoff`). Files and their handles — opened, created, appended, read, written, sought, synced, closed, stat'd, renamed, removed; directories made and removed — beneath the **first preopened directory**, descriptor 3, the only part of a file system a module reaches: a path outside it is refused as WASI refuses it, so `IoCap`'s authority is what the host grants |
| **What it has not** | Tasks, channels and the network: WASI preview 1 has no threads and no sockets, and a program reaching them is refused by name before anything is written. C: a foreign function is an import from outside WASI, which the reference host refuses before the module runs. Listing a directory (`os_dir_open` fails, `Unsupported`), the environment (empty), permission bits (`os_stat` says `0o644`); and (R1d) setting permissions, canonical paths and file locks (`-ENOSYS`), and every process built-in (refused by name) |
| **Failure** | Reported to standard error and the module ends with status 2, as a hosted program's; `main`'s result is written to standard output and the status is 0 |
| **The reference host** | `node wasi.mjs module.wasm DIR [ARGS…]` — `DIR` preopened as `.`, no environment, nothing else granted; an import from any module but `wasi_snapshot_preview1` is refused before the module runs |

### Mobile targets: Android and iOS libraries over the C ABI — Gate 3
<!-- id: mobile -->

| | |
| --- | --- |
| **The targets** | `aarch64-linux-android` (Android ARM64, API level 30, Bionic) and `aarch64-apple-ios` (iOS 17 and later, on the device). Each builds with `--objects DIR` only: the objects, in link order, and `link.txt` — linked by the NDK's `clang` for Android (`-lm`), by `xcrun --sdk iphoneos clang` for iOS. `--lib` builds a library whose `pub extern "C"` functions a host calls: an archive, or with the toolchain's `-shared` a shared library. `--backend cranelift` is refused: the mobile targets are the LLVM backend's |
| **What runs** | The whole language and the task pool: Android is Linux's kernel and AArch64's convention with Bionic, `errno` from `__errno` and the XSI `strerror_r`; iOS is Darwin's unit, as macOS's |
| **What is verified** | Android: static executables run on an aarch64 Linux kernel in a container — Android's C library on Linux, **never an Android device or emulator** (`run-verified on a Linux kernel`). iOS: linked against Xcode's iOS SDK, **never run** (`compile-only`): no simulator runtime is installed |
| **Not here** | A UI framework, Java/Kotlin or Swift bindings, the iOS simulator's own triple, an `.xcframework` |

### Requirements traceability: `nazm trace` — Gate 3
<!-- id: trace -->

| | |
| --- | --- |
| **The requirements** | A project file, `requirements.toml`: `schema = "nazm.requirements/1"`, and each `[[requirement]]` with a stable `id` (written once), its `text`, the `definitions` it constrains — functions of the program's own module, by name — and the `tests`, `@test` functions of the same file, that verify it |
| **The trace** | `nazm trace FILE --requirements FILE` writes `nazm.trace/1`: each requirement, each definition by its **durable identity** (`MODULE::fn NAME`, the key a rename or an edit elsewhere does not move) and its Core IR semantic digest (signature, effects and body, span-free — an edit to the body moves it, an edit elsewhere does not), its obligations from the census keyed by that definition, proved, checked or unknown, and each test's outcome, run as `nazm test` runs it |
| **A broken link** | A definition the file does not have, a test it does not have, a test that fails, or a requirement that names no definition or no test: each written to `broken` and to standard error by the requirement's id, never dropped, and the command exits 1 |
| **What it is not** | Evidence for a reviewer, gathered by the compiler: not a certification, a qualification or a proof that the tests are the right ones |

### Contracts on WebAssembly — N71
<!-- id: wasm-contract -->

`nazm contract FILE --wasm DIR` builds the contract for `wasm32-unknown-unknown` with an adapter:
exports `nazm_init()` and `nazm_call_NAME(caller, args…) -> i64` — `0` committed, `1` reverted (the code
through `nazm_host.revert`), `2` rejected for a change to `conserved`, `3` failed at run time (its text
through `nazm_host.report`) — and imports only `nazm_host.state_get(index) -> i64` and
`state_set(index, value)` besides. A field's index is its declaration index; a `Bool` is 0 or 1.
`contract.json` (`nazm.wasm-contract/1`) and `contract_host.mjs` are written beside the objects.

### Account-oriented contracts — N72
<!-- id: accounts -->

| | |
| --- | --- |
| **`accounts`** | A profile: `web3`'s rules and `signed-writes` — an entrypoint whose write set is not empty must use `caller` in a condition of its own body (`N0510` otherwise); a check made only in a helper does not count |
| **`nazm contract FILE --accounts DIR`** | `accounts.json` (`nazm.sbpf-accounts/1`): the state account (a version byte, then 8 bytes per field by declaration index), and per instruction its discriminator (the first 8 bytes of `keccak256("nazm.instruction." + name)`), its arguments' offsets, and its accounts — the state writable exactly when written, the caller a signer exactly when decided on |
| **Execution** | Not available: no sBPF target is built |

### Build provenance — N73
<!-- id: provenance-record -->

| | |
| --- | --- |
| **`nazm build … --provenance FILE`** | After a successful build only, `FILE` holds `nazm.provenance/1`: every source module (the prelude included) and package with its digest, the target, the compiler's version and semantic epoch, the C compiler's first `--version` line and a digest of its report, the runtime's ABI revision and digest, the backend with its optimisation level, layout, debug and watermark options, every `--link` file's digest, the profiles in force, and the output — an executable, a library and its header, or each file of `--objects` — with its digest |
| **Digests** | `blake3:` and 64 hex digits, BLAKE3 over the file's bytes: recomputable without Nazm |
| **Determinism** | No absolute path, time or host name: one program and one toolchain give one record |
| **`--attest-with CMD`** | Needs `--provenance`. CMD's words, no shell, with the record's path appended; its standard output is written to `FILE.sig`. A failing CMD fails the command and writes no `FILE.sig`; the record and the output stay |

### Project tooling — N74
<!-- id: project-tooling -->

| | |
| --- | --- |
| **`nazm init DIR --template T`** | `cli`, `library`, `server`, `embedded`, `wasm` or `contract`; the directory's name is the package's (`[a-z][a-z0-9-]*`); an existing non-empty directory is refused and nothing is written |
| **`nazm check DIR` on a library** | A package with no `main` is checked as every module of its source root, each needing no `main`, each diagnostic once; `run` and `build` refuse it (`N0509`) |
| **`nazm doc PATH [-o DIR]`** | `nazm.api-doc/1`: per module key, every exported function, record and enum of the program's own modules — signature as written, the comment lines directly above it as its description, parameter and result types, declared and required effects, capabilities, foreign symbol, C export, the source path (relative to the package, or to the file's directory) and line; `-o` writes `api.json` and `api.md`, each definition anchored by its identity. A program that does not check is refused |
| **`nazm bindgen HEADER [-o FILE]`** | A prototype is translated when every parameter is `int64_t`, `bool` or `const char *` and its result `int64_t` or `bool`, and the checker accepts its declaration; anything else — other integer widths, `void`, other pointers, floating point, structs, variadics, function pointers, macros, a declaration the checker refuses — is refused with the reason. Exit 1 when anything was refused |
| **`nazm publish DIR --registry REG --dry-run`** | Every check a publish makes, and `nazm.publish-plan/1` — the version, the digest the index would record, the files it would copy — with nothing written |

### Numbers: integer widths, floating point and bits — Gate 2
<!-- id: numbers -->

Settled by Gate 2, 2026-10-07. `docs/general-purpose.md` §1–§3 is the design record;
`crates/nazm-sema/src/numeric.rs` is the one implementation of these rules the interpreter runs,
and the native backends are held to it by `crates/nazm-cli/tests/numbers.rs`.

**The types.** `Int` is unchanged: 64-bit signed, and the type of an integer literal where
nothing else is expected. Beside it, `Int8`, `Int16`, `Int32` (signed), `UInt8`, `UInt16`,
`UInt32`, `UInt64` (unsigned), and `Float32` and `Float64`, IEEE 754 binary32 and binary64. There
is no `Int64` alias: one type, one name. Each is a value, copied freely, with equality, owning
nothing, and free to cross into a task. Since R1b (L13b, stable since R1 closed (0.10.0)) `Int128`
and `UInt128` too, under every rule below at their width; a value of one is laid out in 16 bytes
aligned to 8. Its division, remainder, signed product, conversion to or from a float and text are
the runtime's, which a board or WebAssembly build does not have: such a build that reaches one is
refused, naming them.

| | |
| --- | --- |
| **Integer literals** | Decimal, `0x`, `0o` or `0b`, with `_` between digits. A literal is an `Int` unless a numeric type is **expected** where it is written — an argument's parameter type, a function's result, the other operand of a binary operator, an element of a `Vec` being pushed, a conversion's target — and then it is that type. A literal outside the range of the type it is written at is refused, `N0003`, naming the type and its range. `-128` written as an `Int8` is one literal, and since R1b (L13b) a literal may be as large as `UInt128`'s maximum, and `-` and one past `Int`'s magnitude one literal where an `Int128` is expected |
| **Float literals** | `1.5`, `0.25`, `2e9`, `2.5e-3`: digits, a `.` and digits, and/or an exponent. A `Float64`, or a `Float32` where one is expected; the decimal text is rounded once, to the nearest value of that type. `1.` and `.5` are not literals, and `1 . 5` is a field read |
| **Arithmetic** | `+ - * / %` and unary `-` on two operands of **one** numeric type, never two: nothing converts implicitly, and `Int + UInt8` is refused, `N0304`. On an integer type, `Int`'s rules at that width: a result outside the type traps (`N0400`), a zero divisor traps (`N0401`), `MIN / -1` traps and `MIN % -1` is 0, and `-x` of an unsigned value other than 0 traps. On a float type: IEEE 754, round to nearest, ties to even, never trapping — overflow is an infinity and an invalid operation a NaN; `%` is the remainder of truncated division, C's `fmod` |
| **Comparison** | `< <= > >=` on two numbers of one type; unsigned types compare as unsigned. `==` and `!=` on every numeric type, and so on records and enums holding them (*Equality is derived*). On floats both are IEEE 754's: every comparison with a NaN is false except `!=`, and `-0.0 == 0.0` — so a record holding a NaN is not equal to itself. *(R1a: `Bool`, `Str`, records, enums and tuples order too — see* Ordering and hashing are derived*; a float makes no structure ordered.)* |
| **Bits** | `&`, `\|`, `^` on two integers of one type; prefix `~`; `<<` and `>>` with an `Int` amount. Precedence, loosest first: `\|\|`, `&&`, comparisons, `\|`, `^`, `&`, shifts, `+ -`, `* / %`, unary — so `a & m == 0` is `(a & m) == 0`. `>>` is arithmetic on a signed type and logical on an unsigned one; `<<` drops the bits shifted out and never traps on them. An amount below 0 or not below the width traps, `N0412`, and a constant one is refused, `N0617`: no amount is masked and none is left to the machine |
| **Conversions** | `T(x)`, `T` a numeric type's name, from any numeric value: the same value or a trap (`N0400`). Integer to float rounds to nearest, ties to even; `Float64` to `Float32` rounds the same way and may become an infinity; float to integer truncates toward zero and traps on a NaN, an infinity or a result outside `T`. `try_convert[T](x)` gives `Option[T]`: `Some` of the exact conversion, `None` where `T(x)` would trap. `wrap_convert[T](x)` is two's complement modulo `2^n`, between integer types only. `saturate_convert[T](x)` clamps to `T`'s range, a NaN to 0. `T(x)` of a literal is the literal written at `T` |
| **Functions** | Integers: `wrapping_add`/`_sub`/`_mul`, `saturating_add`/`_sub`/`_mul`, `count_ones`, `leading_zeros`, `trailing_zeros`, `rotate_left`, `rotate_right` (amount modulo the width; negative traps, `N0412`), `swap_bytes`, `to_big_endian`, `from_big_endian`, `to_little_endian`, `from_little_endian` — each generic over every integer type, `T` inferred; since R1b (L13, stable since R1 closed (0.10.0)) `checked_add`, `checked_sub` and `checked_mul`, which return `Option[T]` — `Some` of the exact result, `None` where it does not fit `T` — as prelude names a module's own of the name hides; since R1b (L13b, EXPERIMENTAL) `widening_mul(a: UInt64, b: UInt64) -> UInt128`, the exact product, which never traps (a narrower or signed one is `T2(a) * T2(b)` at twice the width, exact by the same argument). Floats: `float_sqrt`, `float_abs`, `float_floor`, `float_ceil`, `float_trunc`, `float_round` (half away from zero), `float_min`, `float_max` (a NaN loses), `float_is_nan`, `float_is_infinite`, `float_is_finite`, and from the C library `float_sin`, `float_cos`, `float_tan`, `float_exp`, `float_ln`, `float_log2`, `float_log10`, `float_pow`, `float_atan2`. Bits: `float64_to_bits`, `float64_from_bits`, `float32_to_bits`, `float32_from_bits`. Text: `num_to_str` for every numeric type, `float_parses` and `float_parse_raw`; since R1a (L4) `float_to_str_fixed` for a float (*String interpolation*). A built-in applied outside its class is refused, `N0300` |
| **Text** | `num_to_str` writes an integer in decimal, and a float by ECMAScript's `Number::toString` layout over the shortest digits that read back to the same value in its own type: `100`, `1.5`, `0.001`, `1e+21`, `1.5e-7`, `-0`, `inf`, `-inf`, `nan`. `float_parses(s)` accepts exactly `[+-]? (D+ (. D*)? \| . D+) ([eE] [+-]? D+)?` or `[+-]? (inf \| nan)`; `float_parse_raw(s)` is the correctly rounded `Float64` it writes, or NaN |

**What is exact, and what is not.** The basic operations, `float_sqrt`, `float_floor` and its kin,
every conversion and comparison, `num_to_str` and `float_parse_raw` are exactly specified and
bit-identical in the interpreter, under LLVM at every optimisation level and under Cranelift, on
every supported host: nothing folds a float operation differently from how it runs, contracts
`a * b + c` into a fused multiply-add, or enables fast arithmetic. The C library's functions —
`float_sin` to `float_atan2` — are the host library's and may differ in the last place between
hosts. A NaN's sign and payload are observable only through the bit functions and are not
promised. No floating-point exception flag or rounding mode is exposed.

**Across the C boundary.** Not yet: Gate 2's C interface work states it (*Foreign functions*).

### Arrays: fixed-size storage — Gate 2
<!-- id: arrays -->

Settled by Gate 2, 2026-10-07 (`docs/general-purpose.md` §4).

```nazm
const N: Int = 4;

fn sum(xs: [Int; N]) -> Int {
    let mut t = 0;
    let mut i = 0;
    while i < N {
        t = t + xs[i];
        i = i + 1;
    }
    t
}

fn main() -> Int {
    let mut grid = [[0; 3]; 2];
    grid[1][2] = 7;
    sum([1, 2, 3, 4]) + grid[1][2]
}
```

| | |
| --- | --- |
| **Type** | `[T; N]`: `N` elements of `T`. `N` is an integer literal or an `Int` constant's name (*Constants*), at least 1. `T` is **plain data** — a numeric type, `Bool`, or a record, enum or array of them — and the array at most 65,536 bytes, counting each element's scalars without padding (`N0621` for an element that owns something, `N0618` for one too large or empty). A collection of strings or handles, or a larger buffer, is a `Vec`. *Corrected 2026-10-12, R1a (L8):* `T` may also be `Str` or `Text`, and records, enums and arrays holding them — a **value**: anything but a handle (`N0621` names the handle). A string counts 24 bytes, its pointer, length and owner |
| **Values** | `[a, b, c]` where an array type `[T; n]` is expected — a typed binding, an argument, a field, a result; anywhere else it is a list since R1.5 (*Shorter forms*, D6) — every element of one type (`N0300`), the length the count; `[value; count]` — `count` copies, `count` a literal or a constant. An empty array cannot be written |
| **Indexing** | `xs[i]`, `i` an `Int`, is a copy of the element. An index outside `0..N` stops the program (`N0405`), and a constant one is refused (`N0624`); `[…]` on anything but an array is refused (`N0624`), and a `Vec`'s element is `vec_get`. After a name, a `[` followed by types and a `(` is still type arguments, so a method call on an element is written `(xs[i]).show()` |
| **Writing** | `xs[i] = v`, `grid[r][c] = v`, `points[i].x = v` and `frame.data[i] = v` on a `let mut` binding: the value, then each index outermost first, then one checked write |
| **Value semantics** | An array is a value held inline, like a record: assignment and passing copy it, no two bindings share one, and it owns nothing. *(R1a, L8: it owns the strings its elements hold, by each string's own law — copying an array takes a reference to each, writing an element releases the one it replaces, and an array going away releases them all, exactly as a record's string fields are.)* Equality, task crossing and every ownership rule are its element's (*Equality is derived*); `==` compares element by element, so an array holding a NaN is not equal to itself |
| **Layout** | Its elements contiguous, in index order, each at a multiple of the element's size. Arrays do not cross the C boundary in Gate 2 |

### Constants — Gate 2
<!-- id: constants -->

Settled by Gate 2, 2026-10-07 (`docs/general-purpose.md` §15). `const NAME: T = value;` at a
module's top level declares a value the checker computes once. `T` is a numeric type, `Bool` or
`Str`; `value` is literals, other constants of the module, the operators and numeric conversions
`T(x)` — no call, binding or effect (`N0622`). A literal in it takes `T` where `T` is numeric.
Computing it with the numeric rules every implementation shares, a value that would trap is
refused where the constant is declared (`N0623`), and so is one that depends on itself
(`N0622`). A constant is used by its name anywhere an expression is, and as an array's length;
every use is the value, written as a literal. A constant is its own module's in Gate 2: `pub
const` is refused (`N0101`), because a module's published interface does not carry values yet.
`const` is a word only at the start of an item, so a binding may still be named `const`.

*Corrected 2026-10-12, R1a (L8)* — see *Constant tables and exported constants*: a constant may
be a record, an enum value, a tuple or an array, and `pub const` exports one.

### Bytes: a buffer and its views — Gate 2
<!-- id: bytes -->

Settled by Gate 2, 2026-10-08 (`docs/general-purpose.md` §5).

```nazm
fn main(out: OutCap) -> Int ! {io} {
    let packet = bytes_new(8);
    let header = bytes_slice(packet, 0, 4);
    bytes_write_be(header, 0, UInt32(0xCAFE0001));
    print(num_to_str(bytes_get(packet, 1)));         // 254: one buffer, two views
    0
}
```

| | |
| --- | --- |
| **Type** | `Bytes`: a **counted handle** to a view of a fixed-length buffer of bytes — shared like a `Vec` (*Sequences are shared*): assignment and passing copy the handle, a write through one view is seen through every view of the buffer, and the buffer lives while any view does. It has no `==` (`N0304`), does not cross into a task (`N0321`) and is not plain data, so no array holds one (`N0621`). It holds no values, so it closes no ownership cycle |
| **Making one** | `bytes_new(n)`: `n` zero bytes (`n < 0` stops the program, `N0405`). `bytes_from_str(s)`: a new buffer holding a copy of `s`'s bytes |
| **Bytes** | `bytes_len(b)`; `bytes_get(b, i) -> UInt8`; `bytes_set(b, i, v: UInt8) -> Int` (`0`). An index outside `0..bytes_len(b)` stops the program (`N0405`) |
| **Views** | `bytes_slice(b, lo, hi) -> Bytes`: bytes `lo..hi` of `b`, sharing its buffer, `0 <= lo <= hi <= bytes_len(b)` or `N0405`. A view's bounds are its own, and a buffer never changes length, so no view is ever invalidated |
| **Numbers** | `bytes_read_le[T](b, i)`, `bytes_read_be[T](b, i)`: the `T` whose bytes are `b[i..i + size]` in little- or big-endian order; `bytes_write_le(b, i, v)`, `bytes_write_be(b, i, v)`: `v`'s bytes there, returning `0`. `T` is any numeric type (`Int` is 8 bytes; a float is its IEEE 754 bits); a range not inside the view is `N0405`. No alignment is required |
| **Copying** | `bytes_copy(dst, src) -> Int`: `src`'s bytes over the start of `dst`, as if through a temporary, so two views of one buffer may overlap; returns `bytes_len(src)`, which must not exceed `bytes_len(dst)` (`N0405`). `bytes_to_str(b)`: a new `Str` holding a copy, so a `Str` stays immutable |
| **Provenance** | A buffer is a container whose cell every `Bytes` shares; reads join it, writes add to it. `bytes_from_str(s)` carries `s`'s origin on the new handle, and every read through it or a view of it joins that in |

### Text: UTF-8 by construction — Gate 2
<!-- id: text -->

Settled by Gate 2, 2026-10-08 (`docs/general-purpose.md` §6). `Str` stays bytes (*Strings are
bytes*); `Text` is the type whose values are **always valid UTF-8** — RFC 3629: shortest forms
only, no surrogates, at most U+10FFFF — with `Str`'s representation, so a `Text` is read as a
`Str` for free and a `Str` becomes a `Text` by validation, never by a copy.

```nazm
fn scalars(t: Text) -> Int {
    let mut i = 0;
    let mut n = 0;
    while i < text_len(t) {
        n = n + text_scalar_at(t, i);
        i = text_next(t, i);
    }
    n
}
```

| | |
| --- | --- |
| **Type** | `Text`: an immutable value like `Str` — copied freely, equal by its bytes (`==`, `!=`; byte order is scalar order, so `@std/text`'s `text_compare` of `text_as_str` orders it), crossing into a task |
| **Making one** | A string literal where a `Text` is expected — a parameter, an operand beside a `Text`, `Text(…)`'s argument — since a literal is UTF-8 by construction. `Text(s)` for a `Str`: `s`, or `N0413` if it is not UTF-8. `try_convert[Text](s) -> Option[Text]`: `None` where `Text(s)` would stop. `utf8_valid(s) -> Bool` asks without converting. `scalar_text(c)`: the scalar value `c` as a `Text`, or `N0413` unless `scalar_valid(c)` — `0 <= c <= 0x10FFFF` and not `0xD800..=0xDFFF` |
| **Using one** | `text_as_str(t)`, free; `text_len(t)` in bytes; `text_concat(a, b)`; `text_slice(t, lo, hi)`: bytes `lo..hi`, sharing `t`'s storage — `N0405` unless `0 <= lo <= hi <= text_len(t)`, then `N0413` unless both ends are boundaries. `text_is_boundary(t, i)`: whether `i` is `text_len(t)` or the start of a scalar value — `false`, never a failure, for any other `i` |
| **Scalars** | `text_scalar_at(t, i)`: the scalar value starting at byte `i`; `text_next(t, i)`: the byte index after it; both `N0405` unless `0 <= i < text_len(t)` and `N0413` unless `i` is a boundary. Iteration is a `while` over byte indexes, as above. `text_scalar_count(t)` |
| **Not** | Graphemes, normalisation, case beyond ASCII, collation and locale: the ecosystem's. `Text` is not a `Str` (`N0300`), nor the reverse — `text_as_str` and `Text(…)` say which way |

### Files and handles — Gate 2
<!-- id: files -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §8). The whole-file built-ins of
*What the outside-world built-ins do and do not promise* are unchanged; this adds handles, whose
every failure is a **status** the standard library turns into a value.

```nazm
use "@std/ioerror";
use "@std/file";

fn main(io: IoCap) -> Int ! { io } {
    match file_read_all(io, "notes.txt") {
        Result.Ok(value: b) => bytes_len(b),
        Result.Err(error: e) => 0 - 1,
    }
}
```

| | |
| --- | --- |
| **Type** | `OsHandle`: an open file, directory or standard input, as the operating system holds it. A value, freely copied, that crosses into a task, with no `==` (`N0304`), that no expression constructs: only the built-ins below make one. Every copy names the same open object |
| **Statuses** | A built-in that can fail answers an `Int`: `0` or a count on success, and on failure a **status**, `-errno` — the operating system's error number, negated. A status's number is the platform's: `os_family()` is `0` on macOS and `1` on Linux, and `@std/ioerror` holds each family's table from number to `IoErrorKind`, so a program compares kinds, which are the same everywhere. `os_error_text(status) -> Str` is the system's message, empty for a status that is not negative. Both are pure |
| **Opening** | `os_open_read(path) -> OsHandle` reads an existing file; `os_open_write(path, mode) -> OsHandle` writes one — mode `0` creates or truncates, `1` creates or appends, `2` creates a new file and fails (`AlreadyExists`) if one exists, `3` reads and writes an existing file; any other mode is `InvalidInput`. A created file has permissions `0o666` less the process's umask. `os_dir_open(path) -> OsHandle` opens a directory; `os_stdin() -> OsHandle` is standard input. A failed open still answers a handle, which holds the failure: `os_status(h)` is `0` for an open handle and its status otherwise |
| **Using one** | `os_read(h, buf: Bytes) -> Int`: at most `bytes_len(buf)` bytes into `buf` — the count, `0` at the end; `os_write(h, data: Bytes) -> Int`: the count written, which may be fewer than `bytes_len(data)`; `os_seek(h, offset, whence) -> Int`: the new position, from the start (`0`), the current position (`1`) or the end (`2`); `os_sync(h) -> Int`: the file's data and metadata on storage; `os_dir_next(h, name: Bytes) -> Int`: the next entry's name into `name`, its length, `0` after the last — `.` and `..` are never entries, and the order is the system's. Reading, writing, seeking or syncing a directory is `IsADirectory`, and listing a file `NotADirectory`, in every implementation, without asking the system |
| **Closing** | `os_close(h) -> Int`. Every later use of any copy of `h` answers the status `-65536` — no system's error number, so `@std/ioerror` calls it `Closed` on every platform, with the message `the handle is closed` — and **never reaches a descriptor the system has since reused**: a handle names its descriptor and a generation, and a closed generation is dead. A close while another task is using the handle takes effect when that use ends. A handle not closed stays open until the process ends — it is not closed when its last copy goes (*As built*, `general-purpose.md` §8) |
| **Paths** | `os_stat(path, out: Bytes) -> Int` writes four little-endian `Int`s into `out` (`N0405` if it is shorter than 32 bytes): the size, the kind (`0` a file, `1` a directory, `2` anything else), the modification time in nanoseconds since the Unix epoch, and the permission bits; it follows symbolic links. `os_mkdir(path)`, `os_rmdir(path)`, `os_unlink(path)`, `os_rename(from, to)` answer `0` or a status; `os_rename` replaces an existing `to` atomically on one file system, as POSIX `rename` does |
| **Authority** | Every built-in here except `os_status`, `os_family` and `os_error_text` has the `io` effect. Those that **name a path** — `os_open_read`, `os_open_write`, `os_dir_open`, `os_stat`, `os_mkdir`, `os_rmdir`, `os_unlink`, `os_rename` — and `os_stdin` need an `IoCap` held (*Capabilities*). Those that **use a handle** — `os_read`, `os_write`, `os_seek`, `os_sync`, `os_dir_next`, `os_close` — need only the handle: it reaches nothing but what it names, so a function handed one file can use that file and open nothing else |
| **Provenance** | What `os_read`, `os_dir_next` and `os_stat` put into a buffer is `file`-origin, joined into the buffer's cell. Where a file is written is the restricted sink `write_file`'s path is: `os_open_write`'s path, and the paths of `os_mkdir`, `os_rmdir`, `os_unlink` and `os_rename` |
| **Blocking** | A read of standard input or of a file holds the task's worker on the pool until the system answers (`general-purpose.md` §10 states what does not) |
| **Lines of standard input** (R1d, P3; stable since R1 closed (0.10.0)) | `os_read_line(h, buf: Bytes) -> Int`: standard input up to and including its next newline (byte 10) into `buf` — the count; fewer than the line when `buf` fills first, the rest of the line left for the next read; the last bytes, without a newline, at the end; `0` at the end. Nothing after the newline is taken: standard input has one buffer, which `os_read` of it reads first, so line reads and buffer reads may be mixed — a header's lines, then the frame they announce — and no byte is lost or read twice. A failure is answered as a status only when nothing was read; otherwise the bytes are answered and the next read meets it. Only standard input's handle reads lines: a directory's is `IsADirectory`, any other `InvalidInput`. Its authority, effect and provenance are `os_read`'s; the buffer (16 KiB, made on the first line read) is the process's. Two tasks reading standard input at once take turns, each read whole |
| **Further (R1d)** | Standard output and errors as handles, `os_lstat`, `os_chmod`, `os_realpath`, `os_mkdtemp` and file locks: *The platform, further* (stable since R1 closed (0.10.0)) |

### Networking — Gate 2
<!-- id: networking -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §9 and §10). Sockets are `OsHandle`s
(*Files and handles*): the same statuses, the same generations, the same `os_close`.

```nazm
use "@std/ioerror";
use "@std/net";

fn main(net: NetCap) -> Int ! { io } {
    match tcp_connect(net, "127.0.0.1:7", 1000) {
        Result.Ok(value: s) => { tcp_close(s); 0 },
        Result.Err(error: e) => 1,
    }
}
```

| | |
| --- | --- |
| **Authority** | `NetCap`, a capability of its own (id 7), separate from `IoCap` so a program can be given files without the network or the network without files; `main` may take one. Every network built-in has the `io` effect. Those that **name an address** — `os_tcp_listen`, `os_tcp_connect`, `os_udp_bind`, `os_udp_send_to`, `os_resolve` — need a `NetCap` held; those that **use a socket** — `os_tcp_accept`, `os_udp_recv_from`, `os_set_timeout`, `os_shutdown`, `os_local_addr`, `os_peer_addr`, and `os_read`, `os_write` and `os_close` on one — need only its handle |
| **Provenance** | What arrives through a socket, its sender's and peer's addresses and a name's addresses are `file`-origin, as standard input's are, so none of them may name where a file is written (`N0372`); what `os_udp_send_to` sends is the `file-data` sink `os_write`'s data is |
| **Addresses** | Written `HOST:PORT` with a literal IPv4 host (`127.0.0.1:8080`) or a bracketed IPv6 one (`[::1]:8080`); anything else is `InvalidInput`. A socket's own and its peer's address are written the same way, IPv6 in RFC 5952's shortest form. `os_resolve(name, port, out: Bytes) -> Int` asks the system's resolver: the addresses it answers, in its order, one per line, into `out` — the length written, or a status: `-65537` (`NotFound`, `the name was not found`) for any name the resolver did not answer, `-EINVAL` when `out` is too short |
| **TCP** | `os_tcp_listen(addr) -> OsHandle` (the address reused, so a restarted server binds at once; a backlog of 128); `os_tcp_accept(listener) -> OsHandle`; `os_tcp_connect(addr, timeout_ms) -> OsHandle` (`timeout_ms <= 0` waits as long as the system does). A connected socket is read and written with `os_read` and `os_write`: a read answers what has arrived, at most the buffer, `0` once the peer has shut its side; a write answers how many bytes the system took. Writing to a peer that has gone is `BrokenPipe`, never a signal. `os_shutdown(h, how)` shuts reading (`0`), writing (`1`) or both (`2`) |
| **UDP** | `os_udp_bind(addr) -> OsHandle`; `os_udp_send_to(h, data, addr) -> Int`; `os_udp_recv_from(h, buf, from: Bytes) -> Int` — how many of the datagram's bytes were kept in `buf` (a longer one is cut to it), and its sender's address in `from`, followed by zero bytes |
| **Addresses of a socket** | `os_local_addr(h) -> Str` and `os_peer_addr(h) -> Str`; empty for a handle that has none |
| **Deadlines** | `os_set_timeout(h, ms) -> Int` bounds every later wait on `h` — accept, read, write, receive, send — to `ms` milliseconds (`0`: no bound); a wait that runs out answers `TimedOut` and changes nothing. A connect is bounded by its own argument |
| **Waiting** | A socket wait does **not** hold a worker. On the task pool the task parks at the language's existing wait, its worker runs other tasks, and one runtime thread — `kqueue` on macOS, `epoll` on Linux — wakes it when the socket is ready, its deadline passes, or its handle is closed; with a thread per task, the thread waits; in the interpreter every task is a thread. Nothing is written `async` or `await` |
| **Closing** | `os_close` of a socket another task is waiting on wakes that task, whose operation answers `Closed`; the close itself takes effect when the wait ends |
| **Not here** | TLS, HTTP and every protocol above TCP and UDP (the ecosystem's); Unix-domain sockets; socket options beyond the deadline and address reuse; multicast and broadcast; non-blocking polling of many sockets by one task (a task per socket is the shape) |

### Time — Gate 2
<!-- id: time -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §11). `time_now_ms` (N54) is unchanged.

| | |
| --- | --- |
| **Clocks** | `time_now_ns() -> Int`: the monotonic clock `time_now_ms` reads, in nanoseconds since an unspecified start — readings compare only within one run. `time_wall_ns() -> Int`: the wall clock, in nanoseconds since the Unix epoch; it may jump |
| **Waiting** | `time_sleep_ms(ms) -> Int`: `0` after at least `ms` milliseconds; nothing for `ms <= 0`. On the task pool the task parks at the language's existing timed wait and its worker runs others; with a thread per task, and in the interpreter, the thread waits |
| **Authority** | Each needs a `TimeCap` held and performs no effect, as `time_now_ms` does; `critical`'s `no-ambient-time` refuses them |
| **Library** | `@std/time` gains `Duration`, `Instant` and `WallTime` over them (*The standard library*) |

### Process and environment — Gate 2
<!-- id: process -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §12). The arguments (`args_count`,
`arg`) and `exit_with` are unchanged; standard input is `os_stdin()` (*Files and handles*).

| | |
| --- | --- |
| **Environment** | `os_env_get(name) -> Str` (empty when unset), `os_env_has(name) -> Bool`, `os_env_count() -> Int`, `os_env_at(i) -> Str` — the `i`th entry, `NAME=value`, in the order the process holds them; `N0405` outside `0..os_env_count()`. An entry with no `=` after its first byte is not one. Each needs an `IoCap` held and has the `io` effect; what they answer is `argument`-origin, as the arguments are |
| **Running a program** | `os_spawn(program, args: Strs) -> OsHandle`: `program` found on `PATH` as a shell finds it, started with `args` and the process's environment, its standard input, output and errors three pipes. It needs a `ProcessCap` — every authority the program has, granted explicitly (id 9) — and has the `io` effect; which program runs is the restricted sink a written file's path is (`N0372`). A failure to start is a status (`NotFound` for no such program) |
| **A child** | `os_child_pipe(h, which) -> OsHandle`: its standard input (`0`), output (`1`) or errors (`2`), read and written with `os_read` and `os_write`, waiting as a socket does (*Networking*) — a task reading a child's output parks on the pool. `os_wait(h) -> Int`: its exit code, or `256` and the signal that ended it; the same answer every time; the wait holds the task's worker until the program ends. `os_kill(h) -> Int`: `SIGKILL`, nothing for a child already waited for. Each needs only the child's handle |
| **Signals** | A native program that has started a child ignores `SIGPIPE` from then on, as the interpreter's host always does, so writing to a child that has gone is `BrokenPipe`. No program watches for signals in 1.0 (*As built*, `general-purpose.md` §12) |
| **Further (R1d)** | A child in another directory with an environment of its own, a new process group, a null standard input, process ids, bounded waits, a signal to a group and a child's peak memory: *The platform, further* (stable since R1 closed (0.10.0)) |

### Randomness — Gate 2
<!-- id: randomness -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §21). `os_random_bytes(buf: Bytes) ->
Int` fills `buf` from the operating system's entropy and answers its length, or a status; it needs a
`RandomCap` held (id 8) and performs no effect, as a clock does. A **seeded** generator needs no
authority at all: `@std/random`'s `Rng` is ordinary code, the same sequence for the same seed in
every implementation and run. Reproducibility is the default; unpredictability is asked for by name.

### Attributes — Gate 2
<!-- id: attributes -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §16). An attribute is `@` and a name,
with arguments in parentheses or none, written on the lines before a top-level declaration —
a function, `struct`, `enum`, `trait`, `impl`, `const` or `static` — never before a `use`, and not, in
1.0, inside an `impl` or a `trait`. An argument is a string, a word, `key = "value"`, or
`name(…)` of further arguments. The vocabulary is closed: there are no user-defined attributes
and no macros.

| | |
| --- | --- |
| **`@cfg(condition)`** | The declaration exists only when the condition holds (*Conditional compilation*, below). It may be written more than once; every one must hold |
| **`@test`, `@test(fails)`** | A function `nazm test` runs; `fails` says it passes by failing (`general-purpose.md` §18) |
| **`@bench`** | A function `nazm bench` measures (§19) |
| **`@fuzz`** | A function `nazm test --fuzz` drives with generated input (§18) |
| **`@deprecated`, `@deprecated("why")`** | **Metadata-only deprecation** (G2-C1). It marks a `pub` function, `struct` or `enum` and nothing else (`N0619` elsewhere): `nazm doc` publishes it, as the item's `deprecated` — the reason, or empty — in `nazm.api-doc/2` and as a line in the Markdown. No diagnostic reports a use, and a use compiles exactly as it would without the mark: 1.0 has one diagnostic severity, and compiler warnings are not part of it |
| **`@section("NAME")`** (Gate 3) | Places a function or a static in a board's image (*Boards*, below); before anything else, `N0619` |
| **`@interrupt("NAME")`** (Gate 3) | Makes a function a board's interrupt handler (*Boards*, below); before anything else, `N0619`, and a function of another shape `N0627` |
| **`@on_failure`** (Gate 3) | Makes a function a board's failure hook (*Boards*, below); before anything else, `N0619`, and a function of another shape `N0627` |
| **`@task(priority = "N", period = "P")`** (Gate 3) | Makes a function a periodic task of a board's fixed-priority scheduler (*Boards*, below); before anything else, or with a priority or period out of range, `N0619`, and a function of another shape `N0627` |
| **`@derive(json)`, `@derive(toml)`, `@derive(json, toml)`** (R1c, L2; stable since R1 closed (0.10.0)) | Before a `struct` or `enum` that is not generic: gives it a JSON encoding, decoding and schema, a TOML encoding and decoding, or both (*Derived serialisation*, below); before anything else, or with a field of a type it cannot carry, `N0619` |
| **`@codec(…)`** (R1c, L2b; EXPERIMENTAL) | Shapes a derived encoding (*Derived serialisation*, **Shaping it**): before a type that derives one, `rename_all`, `deny_unknown`, and for an enum `tag` or `untagged`; **before a field**, `rename`, `default`, `omit_none`; **before a variant**, `rename`. The one attribute written inside a declaration. Elsewhere, or with an argument it does not take, `N0619` |
| **Refused** | An attribute not in this list, one on a declaration it does not mark, one with arguments it does not take, or one other than `@cfg` written twice is `N0619`. A malformed argument is a syntax error (`N0100`), reported once; the attribute is then not checked again |

Attributes change no expression's meaning. The formatter writes each on its own line, `@` against
its name.

### Derived serialisation — R1c
<!-- id: derived-serialisation -->

R1c (L2), stable since R1 closed (0.10.0). `@derive(json)` before a `struct` or an `enum` that is not
generic gives it, in its module and public exactly when the type is, functions named by the type's
name in snake case — `HttpRequest` is `http_request`:

```nazm
@derive(json)
pub struct Point { x: Int, label: Option[Str], tags: Vec[Str], }

fn main(out: OutCap) -> Int ! {io} {
    let text = point_to_json(Point(x: 1, label: Option[Str].None(), tags: vec_new[Str]()));
    // {"x":1,"label":null,"tags":[]}
    match point_from_json(text) { Result.Ok(value: p) => p.x, Result.Err(error: e) => 0, }
}
```

| Function | |
| --- | --- |
| `point_to_json(v: Point) -> Str` | Compact JSON text |
| `point_from_json(text: Str) -> Result[Point, Str]` | The value, or an `Err` naming where in the document it went wrong, as a path — `$.tags[2]: expected a string`. Never traps |
| `point_json_schema() -> Str` | A JSON Schema (2020-12) of the type: a `$ref` to it under `$defs`, with every derived type it reaches beside it, so a recursive type has a finite schema |
| `point_json_node(doc: JsonDoc, v: Point) -> Int`, `point_json_read(doc: JsonDoc, node: Int, at: Str) -> Result[Point, Str]`, `point_json_defs(seen: Strs, defs: Strs) -> Int` | What another derived type's functions call for a field of this one |

The module needs no `use "@std/json";` of its own: deriving imports it. A function the module already
has of one of these names is a duplicate (`N0203`).

**The encoding.** Structural, like derived equality (*Ordering and hashing are derived*): a record is
an object of its fields in the order written; an enum value with no payload is its variant's name as
a string, and one with a payload an object of one member, the variant's name, whose value is the
payload as a record's fields are. A field's type decides its value:

| Field type | JSON |
| --- | --- |
| `Int`, `Int8`, `Int16`, `Int32`, `UInt8`, `UInt16`, `UInt32` | An integer; on decoding, one outside the type is an `Err` |
| `Float64`, `Float32` | A number; decoding takes an integer too |
| `Bool`, `Str` | `true`/`false`, a string |
| `Option[T]` | `T`'s value, or `null`; on decoding, a missing member is `None` too |
| `Vec[T]` (so `List[T]`), `Chain[T]` | An array |
| `Box[T]` | `T`'s value |
| A record or enum — of this module or, qualified, another's | Its own encoding: it must derive `json` too — `N0619` at the field where the program declares the type and it does not; for one the checker cannot see from here, the generated call names a function that does not exist (`N0201`) |

Any other field type — `UInt64` and the 128-bit integers, which a JSON integer here does not carry,
`Bytes`, `Text`, a handle, a function, an array or a tuple — is refused at the attribute, `N0619`,
naming the field. Decoding ignores members the type has no field for, unless the type says
`deny_unknown`.

**Shaping it** (L2b). `@codec(…)` changes the encoding and nothing else — the decoding, the schema
and the encoding always agree:

```nazm
@derive(json)
@codec(rename_all = "camelCase", deny_unknown)
pub struct User {
    @codec(rename = "id")
    user_id: Int,
    display_name: Str,                 // "displayName"
    @codec(omit_none)
    email: Option[Str],                // absent rather than null when `None`
    @codec(default)
    tags: Vec[Str],                    // a missing member decodes as empty
}

@derive(json)
@codec(tag = "kind")
pub enum Event { Started, Moved(x: Int, y: Int), }   // {"kind":"Moved","x":1,"y":2}
```

| Where | Argument | Meaning |
| --- | --- | --- |
| A type | `rename_all = "…"` | Every member of a record — every variant of an enum — named in `camelCase`, `snake_case`, `kebab-case`, `PascalCase` or `SCREAMING_SNAKE_CASE`, from its snake-case or Pascal-case name; a `rename` wins over it |
| A type | `deny_unknown` | Decoding a member the type has no field for is an `Err`; the schema says `"additionalProperties": false` |
| An enum | `tag = "…"` | Internally tagged: a value is one object, the tag member naming its variant beside the payload's fields; a variant with no payload is the tag alone. A payload field of the tag's name is refused |
| An enum | `untagged` | A payload's fields alone, and a variant with no payload its name as a string; decoding tries the variants in the order written and takes the first that decodes |
| A field | `rename = "…"` | The member's name |
| A field | `default` | A missing member decodes as the type's empty value: `0`, `0.0`, `false`, `""`, `None`, an empty `Vec` or `Chain`; on a field of any other type, `N0619` |
| A field | `omit_none` | An `Option` field that is `None` is left out rather than written `null`; on another type, `N0619` |
| A variant | `rename = "…"` | The variant's name |

`tag` with `untagged`, an argument written twice, two members or variants given one name, `@codec`
before a type that derives nothing, and any other argument are `N0619`.

**TOML** (L2c). `@derive(toml)` — alone or beside `json` — gives a type the same shape over
`@std/toml`'s `Toml` tree, imported by deriving:

| Function | |
| --- | --- |
| `point_to_toml(v: Point) -> Str` | The document `toml_encode` writes. A record only: a TOML document is a table |
| `point_from_toml(text: Str) -> Result[Point, Str]` | The value, or an `Err` naming the path — or `toml_parse`'s `line:column: what`. A record only |
| `point_toml_value(v: Point) -> Toml`, `point_toml_read(t: Toml, at: Str) -> Result[Point, Str]` | The value as a tree and back, for a record or an enum: what a field of this type is written and read with |

A record is a table, an enum value a string or a table as the JSON encoding has it — `@codec` shapes
both alike — an integer a TOML integer, a float a TOML float (an integer reads as one too), `Vec`
and `Chain` arrays. TOML has no `null`: an `Option` that is `None` is always left out, and a missing
key decodes as `None`; so an `Option` may only be a field's whole type, and one inside a `Vec`, a
`Chain`, a `Box` or another `Option` is refused, `N0619`. There is no TOML schema: the JSON Schema of
`@derive(json)` describes the JSON form.

### Conditional compilation — Gate 2
<!-- id: conditional-compilation -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §17). A declaration whose `@cfg` does not
hold is removed before any name is resolved or type checked — as if it were not written — so two
declarations of one name under exclusive conditions are not a duplicate, and code that only one
target can compile is not checked on another. An `impl` removed takes its methods with it.

| | |
| --- | --- |
| **Conditions** | `os = "…"` (`macos`, `linux`, `wasm`, or `none` for a board), `arch = "…"` (`aarch64`, `x86_64`, `wasm32`, `riscv64gc`, …: the triple's first part), `target = "…"` (the whole triple), `feature = "…"` (a package feature enabled, §22), `profile = "…"` (a restriction profile the build is held to); combined with `all(…)`, `any(…)` and `not(…)`. Anything else is `N0619` |
| **What decides them** | The build's configuration and nothing else: `nazm build --target T` decides for `T`, and with no target, and for `check` and `run`, the host. The profiles are `--profile`'s and every package manifest's. The same source under the same configuration keeps the same declarations |
| **Reuse** | A module that writes `@cfg` is checked as its configuration builds it; its check is filed under that configuration and never reused under another |

### Tests and benchmarks — Gate 2
<!-- id: tests-and-benchmarks -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §18–§19). A marked function is run by a
generated entry: a `main` appended to the file's text as the compiler reads it — never to the file —
that holds the authority the function's parameters ask for, as `main`'s do, calls it, and ends the
process. The file's own `main`, if any, is not part of what the harness builds: its text is read as
blanks, its lines kept, so every diagnostic about the rest of the file lands where it is written;
`nazm check` and `nazm run` still check and run it.

| | |
| --- | --- |
| **`@test`** | `fn NAME(capabilities…) -> Result[Int, Str]`. It passes when it answers `Ok`; an `Err` fails, its message printed, and so does a trap. Both end the process `2`. `@test(fails)` passes only when the process ends `2` — never when the program does not check |
| **`@fuzz`** | `fn NAME(input: Bytes, capabilities…) -> Result[Int, Str]`, called with `nazm test --fuzz N` inputs (100 by default) of 0 to 64 bytes from a fixed xorshift sequence: the same inputs in every implementation and run. The first `Err` fails, naming its case |
| **`@bench`** | `fn NAME(capabilities…) -> Int`, built natively at `-O2` and called `--iterations` times between two readings of the monotonic clock; `nazm bench` reports nanoseconds per iteration (`nazm.bench/1`, EXPERIMENTAL) |
| **Running** | `nazm test` runs every `@test` and `@fuzz` function of each file named, as `STEM::NAME`, under the interpreter and as an executable, each in its own process, beside the recorded-output cases; `--filter TEXT` keeps the cases whose name contains it. A function whose `@cfg` does not hold on this host is not run |
| **Refused** | A marked function of another shape, a generic one, a foreign one, or `main` marked, is `N0619` |
| **Library** | `@std/test`'s `assert_…` functions answer `Result[Int, Str]`, written with `?`; `property(seed, cases, check)` checks `cases` cases drawn from one `@std/random` generator |

### Package features — Gate 2
<!-- id: package-features -->

Settled by Gate 2, 2026-10-09 (`docs/general-purpose.md` §22). A manifest may declare
`[features]`: each a name of a package name's shape, listing what enabling it enables — another of
the package's own features, or `DEPENDENCY/FEATURE` of a dependency it declares. A dependency entry
may ask for features: `shapes = { path = "../shapes", version = "0.1.0", features = ["fast"] }`.

| | |
| --- | --- |
| **Enabled** | Each package's: what its `default` enables, what every dependent asks of it, what `--features` (on `check`, `run` and `build`) asks of the root, and what each enabled feature enables, to a fixed point. Additive: nothing disables a feature, so the order of asking cannot change the set. `default` names a group and is not itself enabled; nothing switches it off |
| **Read by** | `@cfg(feature = "NAME")` (*Conditional compilation*), in each module for **its own package's** set; a toolchain module's set is empty. Nothing from the environment enters it |
| **Refused** | A feature item that is neither the package's own feature nor `DEPENDENCY/FEATURE` of a declared dependency, and a feature asked of a package that does not declare it, are `N0500` before anything is checked; `--features` outside a package is a usage error |
| **Lockfile** | When any package has an enabled feature, `nazm lock` writes `nazm.lock/2`: `/1`'s fields and each package's `features`, the set its manifests enable (not `--features`'). A build with no feature keeps `nazm.lock/1` byte for byte; both are read. `--locked` refuses a recorded set that differs (`N0506`) |

### Windows x86_64 — Gate 3
<!-- id: windows-x86-64 -->

Settled by Gate 3, 2026-10-10 (`docs/systems-domains.md`, Part A-W), before the code. The target is
`x86_64-pc-windows-gnu`: PE/COFF executables and static libraries (a DLL is refused by name:
`--shared` is not built for Windows), the Windows x64 calling convention at every C-ABI crossing,
MinGW-w64's C runtime and import libraries, linked by MinGW-w64's `gcc` driver (`-static`, its own
libgcc; `nazm build --objects` writes the command in `link.txt`). The language is the one language:
nothing below changes what a program means, only how the runtime reaches the operating system, and a
program that runs on Linux prints the same and exits the same here.

| | |
| --- | --- |
| **Text and paths** | A `Str` path is UTF-8; it is converted to UTF-16 at the system boundary and back, and a name the system returns that is not valid UTF-16 is an error (`InvalidData`), never a lossy string. `/` and `\` both separate; a drive letter or `\\?\` prefix is part of the path. Files are opened in binary mode: no line-ending translation, on reading or on writing |
| **Files** | As *Files and handles*: `os_rename` and `file_replace_atomic` replace an existing destination (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`), as POSIX `rename` does; removing an open file or a non-empty directory is an error with the kind the table below gives; `dir_list` is sorted, as everywhere |
| **Errors** | Windows error codes map to the same `IoErrorKind`s: not found, permission denied (including sharing violations), already exists, would block, timed out, broken pipe, connection refused/reset/aborted, address in use, invalid input; anything else is `Other` with its code |
| **Processes** | `os_spawn` starts the program with `CreateProcessW` and a command line quoted by the C runtime's rules; `os_wait` answers the exit code; there are no signals, so `os_kill` ends the child with `TerminateProcess` and exit code 1, and `256 + signal` never occurs. R1d's built-ins on Windows: *The platform, further*, **Where** |
| **Environment, arguments** | UTF-16 from the system, UTF-8 to the program; an entry not valid UTF-16 is skipped rather than mangled. The arguments are the C runtime's own splitting of the command line (`__wgetmainargs`), and a child's command line is quoted so that it splits back into the arguments given |
| **Network** | Winsock 2, started once by the runtime; the reactor waits with `WSAPoll` — the same reactor model as kqueue and epoll, a third binding of it |
| **Time, entropy** | the monotonic clock from `QueryPerformanceCounter`, the wall clock from `GetSystemTimePreciseAsFileTime` (nanoseconds since the Unix epoch, as everywhere); entropy from `BCryptGenRandom` |
| **Failure** | a trap is exit status 2 with its diagnostic on standard error, as everywhere; an exhausted stack is `N0408` by the same stack check as everywhere — a thread's budget from `GetCurrentThreadStackLimits`, a task's a quarter of the way up its own stack — and a task stack's guard page (`VirtualProtect`) is the backstop it is on POSIX. An executable reserves 8 MiB for its main thread (`-Wl,--stack,8388608` in `link.txt`), what macOS and Linux give it, where Windows' own default is 1 MiB, so a recursion meets `N0408` at the same depth on all three; `--stack-size` runs `main` on a thread of its own size there too. A frame of a page or more is probed by the platform's `__chkstk` (MinGW-w64's `___chkstk_ms`), which LLVM calls on Windows, instead of the inline probe the other targets use (Q1-A1-01) |
| **Evidence** | Programs are executed **under Wine** in a Linux container (`run-verified under Wine`), and, since 2026-10-15, on a real Windows 10 x86_64 host by `scripts/windows-host.sh` (`run-verified on a Windows host`), each named for what it is. Microsoft's CRT (`x86_64-pc-windows-msvc`) is not built in Gate 3 |

### Atomics — Gate 3
<!-- id: atomics -->

Settled by Gate 3, 2026-10-10 (`docs/systems-domains.md` Part B).

```nazm
fn count(hits: Atomic) -> Int { atomic_add(hits, 1); 0 }

fn main(out: OutCap, tasks: SpawnCap) -> Int ! {io, spawn} {
    let hits = atomic_new(0);
    scope {
        spawn count(hits);
        spawn count(hits);
    }
    print(num_to_str(atomic_load(hits)));             // 2: one cell, seen from every task
    0
}
```

| | |
| --- | --- |
| **Type** | `Atomic`: a **counted handle** to one `Int` cell, shared like a `Chan`: assignment and passing copy the handle, and every copy reaches the same cell. It **may cross into a task**, as a `Chan` may. It has no `==` (`N0304`) and is not plain data, so no array holds one (`N0621`); a record may, and a `static` may (*Statics*, below) |
| **Operations** | `atomic_new(v: Int) -> Atomic`; `atomic_load(a) -> Int`; `atomic_store(a, v: Int) -> Int`, answering `0`; `atomic_add(a, d: Int) -> Int`, answering the value before; `atomic_swap(a, v: Int) -> Int`, answering the value before; `atomic_compare_swap(a, expected: Int, new: Int) -> Bool`, storing `new` and answering `true` exactly when the cell held `expected` |
| **Ordering** | Every operation is **sequentially consistent**: all of them, on every cell, appear in one order every task agrees on. There is no ordering parameter in 1.0; a weaker ordering would be an extension |
| **Overflow** | `atomic_add` whose sum leaves `Int`'s range stops the program with `N0400`, as `+` does, and leaves the cell unchanged |
| **Authority** | None: an atomic is the program's own memory. No effect, no capability |
| **Where** | Every hosted target, `nazm run`, and the boards. On a target with no threads (`wasm32-unknown-unknown`) an operation is the plain load or store it would be with one task. A board has no allocator unless its manifest gives it a heap (*Boards*, below), so there an atomic is a `static`'s |

### Statics — Gate 3
<!-- id: statics -->

Settled by Gate 3, 2026-10-10 (`docs/systems-domains.md` Part B).

| | |
| --- | --- |
| **Declaration** | `static NAME: T = value;` at a module's top level: storage with **one address** for the whole run, initialised before `main` and never freed |
| **Type and value** | `T` is a numeric type or `Bool`, and `value` is a constant expression with the rules of *Constants* (`N0622`, `N0623`); or `T` is `Atomic` and `value` is `atomic_new(c)` with `c` such an expression, the one call a static's value may make. A record, enum or array is not a static's type in 1.0 (`N0622`): a constant has no value of one to start it with, and allowing one would be an extension |
| **Read-only** | A static is read, never assigned (`N0625`): there are no mutable global variables. A static `Atomic` is changed only through the atomic operations, which is how a task and an interrupt handler share state |
| **Use** | By its name anywhere an expression is; reading it reads the storage. It is its module's own, as a constant is: `pub static` is `N0101` |
| **Placement** | `@section("NAME")` places it (*Boards*, below); without one it is in the target's read-only data, or its writable data when it holds an atomic |
| **Constant or static** | A constant is a value written wherever it is used and has no address; a static is storage. Use a static for a table placed in a section or an atomic cell; a constant everywhere else |

### Boards: manifests, sections, interrupts, a heap and a failure hook — Gate 3
<!-- id: boards -->

Settled by Gate 3, 2026-10-10 (`docs/systems-domains.md` Part B). Everything here is a freestanding
target's (*A freestanding target*, above) and LLVM's; a hosted build or `wasm32-unknown-unknown`
refuses `@section`, `@interrupt` and `@on_failure` by name (`N0392`). Every board is emulated.

| | |
| --- | --- |
| **Manifest** | `nazm build --target T --board board.toml --objects DIR` builds for the board the TOML file describes; without `--board`, the target's built-in board, which is itself a manifest (`nazm inspect` prints both). Keys: `name`; `isa` (`aarch64`, `riscv64`, matching `--target`); `load_address`; `stack` (bytes); `[uart]` `kind` (`pl011`, `ns16550a`) and `address`; `[stop]` `kind` (`semihosting`, `sifive-test`) and `address` where it has one; optional `[heap]` `size`; optional `[interrupts]` (below); optional `[watchdog]` `ticks` (below). A key it does not know, or a value out of range, refuses the build naming the key (`N0626`) |
| **The runtime boundary** | `_start` is the board's, as before: it sets the stack, initialises statics, installs the vector table and enables interrupts when the manifest has one, calls `main`, reports and stops. A program's entry is always `main` |
| **`mmio_read64`, `mmio_write64`** | The 64-bit forms of the device-register built-ins, with their rules: one volatile access of exactly 64 bits |
| **`@section("NAME")`** | On a function or a static: placed in the section `NAME` (letters, digits, `.` and `_`), which the generated `link.ld` keeps and places after the board's own sections of the same kind — code with code, data with data. On anything else, `N0619` |
| **`@interrupt("NAME")`** | On `fn f() -> Int ! {}` or `fn f(m: MmioCap) -> Int ! {}`: the handler for the interrupt the manifest's `[interrupts]` names `NAME`. `[interrupts]` gives the controller — on AArch64 `controller = "gicv2"` with its `distributor` and `cpu` interface addresses; on a Cortex-M `controller = "nvic"` with the core's `clock_hz` — `timer_period_us`, the period of the board's timer, whose interrupt is named `timer` (the virtual timer on AArch64, acknowledged and re-armed by the runtime before its handler runs; SysTick on a Cortex-M, at most 2^24 of the clock's ticks), and `[interrupts.lines]`, each further name and its number — a GICv2 `INTID` from 16 to 1019, or an NVIC external interrupt from 0 to 239 — which the runtime enables and, on a GICv2, acknowledges. Interrupts are enabled before `main` and taken one at a time, on the board's one stack. A handler shares state with the rest of the program only through statics and atomics; its result is discarded, and a failure in it is reported and stops the machine as `main`'s does. A handler for a name the manifest does not have, a name with two handlers, or a handler of another shape is `N0627`. **AArch64 and Cortex-M** in 1.0: a RISC-V board's `[interrupts]` is refused (`N0626`) — its machine-mode interrupts would be a third controller binding |
| **Heap** | A manifest with `[heap] size` gives the board a **bump arena** of that many bytes after the image: allocation takes from it and is never returned. Strings built at run time, sequences, `Bytes` and closures then build on the board; exhausting the arena stops the program with `N0406`. Without a heap, allocation is refused at build time, as before. The `no-heap` rule refuses every allocation site whatever the board, and is the `kernel` profile's, with `no-spawn`, `no-foreign` and `no-blocking` |
| **`@on_failure`** | On one `fn f(code: Int) -> Int ! {}`, or `fn f(code: Int, m: MmioCap) -> Int ! {}` to reach a device: the runtime calls it with the failure's number (`408` for `N0408`) after writing the diagnostic and before stopping the machine with status 2. A failure inside it stops the machine at once. Another shape, or a second hook, is `N0627` |
| **Watchdog** (Gate 3) | A manifest's `[watchdog] ticks = N` (at least 1) arms a watchdog on the scheduler's ticks: `watchdog_feed()` re-arms it, and `N` ticks without a feed fail the image with `N0415`, reported and stopped as any failure, the failure hook first. `watchdog_feed` needs the device's authority, as `mmio_*` does; a build that calls it for a board without `[watchdog]`, or whose watchdog has no `@task` to run beside, is `N0626`; a hosted build, `N0392`. It is the runtime's, on the board's timer: no hardware watchdog is programmed |
| **`@task(priority = "N", period = "P")`** | On `fn f() -> Int ! {}` or `fn f(m: MmioCap) -> Int ! {}`: a periodic task of the board's **fixed-priority scheduler**, released every `P` ticks of the board's timer (a manifest's `timer_period_us`) from its first tick, `N` from 0 (lowest) to 255. `main` runs first, as the image's initialisation; then the scheduler runs, run to completion and one at a time, the highest-priority task that is released and has not run since, and waits for an interrupt when none is. A task released again before it has run has missed its deadline, its period: the image fails with `N0414`, reported and stopped as any failure. A task that answers anything but `0` stops the machine with that answer as the image's result, as `main`'s would be. No lock exists, only atomics and statics, so no priority inversion can; the scheduler owns the board's timer, so `@interrupt("timer")` beside a task is `N0627`, as are a task of another shape and a task on a board with no timer; a priority or period out of range is `N0619` |
| **Not here** | Inline assembly: the C ABI is the low-level escape. A kernel or driver claim: the reference workload is a kernel-*style* image under QEMU, never "OS-ready" |

### `for` loops — R1
<!-- id: for -->

Settled by R1a, 2026-10-11 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, D3).

```nazm
fn total(xs: Vec[Int]) -> Int {
    let mut sum = 0;
    for x in xs {
        sum = sum + x;
    }
    for i in 0..3 {
        sum = sum + i;
    }
    sum
}
```

A `for` loop is a statement, as `while` is, and has no value. It comes in two forms, and **each
means exactly the `while` loop written beside it** — nothing about evaluation, failure,
reclamation or control is new:

| Written | Means |
| --- | --- |
| `for x in e { body }` | `{ let seq = e; let mut i = 0; while i < len(seq) { let x = get(seq, i); i = i + 1; body } }` |
| `for x in a..b { body }` | `{ let mut i = a; let end = b; while i < end { let x = i; i = i + 1; body } }` |

- **What may be iterated.** `e` is an `Ints`, a `Strs`, a `Vec[T]`, a `Chain[T]` or a `Bytes`; `len`
  and `get` are then `ints_len`/`ints_get`, `strs_len`/`strs_get`, `vec_len`/`vec_get`,
  `chain_len`/`chain_get` and `bytes_len`/`bytes_get`, so `x` is an `Int`, a `Str`, a `T` or a `UInt8`. Anything else is
  refused (`N0628`). In the range form `a` and `b` are `Int`s; `a..b` is written only there — it is
  not an expression.
- **Evaluated once, read every step.** `e`, or `a` and `b`, is evaluated once, before the first
  step. The length is read before **every** step, as the `while` loop's condition is: an element
  pushed during the loop is visited, and a `vec_pop` that shortens the sequence ends the loop
  sooner. A handle is shared, not copied (*The memory constitution*), so the loop sees the
  sequence the program is changing.
- **`x` is new each step and immutable.** It is a binding of the loop's body, as a `let` at the
  body's top would be; assigning it is refused as assigning any `let` is (`N0310`), and naming it
  after the loop is an unknown name. Same-scope shadowing is refused as for `let` (`N0202`).
- **`break` and `continue`** leave the `for` loop and start its next step, as in `while`; the step
  is counted before the body runs, so `continue` never repeats an element.
- **The hidden names** `seq`, `i` and `end` above are the explanation, not names a program can
  see or collide with.
- **Words.** `for` was reserved from the beginning, so no program could name anything `for`; `in`
  is recognised only between a `for` loop's name and what it iterates, and stays an ordinary name
  everywhere else. A `for` that is not of either form is still refused by name (`N0101`).

### Immutable indirection: `Box[T]` and `Chain[T]` — R1
<!-- id: box-list -->

Settled by R1a, 2026-10-11 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, D1 and L1).
Renamed by R1.5 (D7), 2026-10-15: R1a called the sequence `List[T]`, with built-ins `list_*`; that
name is now the growable list's, `Vec[T]` (*Shorter forms*), and the sequence is `Chain[T]`, with
built-ins `chain_*`. A call of a `list_*` built-in that the module does not define itself is
refused, `N0637`, naming the `chain_` built-in that replaced it.

```nazm
enum Expr { Num(v: Int), Add(l: Box[Expr], r: Box[Expr]), }

fn eval(e: Expr) -> Int {
    match e {
        Expr.Num(v: v) => v,
        Expr.Add(l: l, r: r) => eval(box_get(l)) + eval(box_get(r)),
    }
}

struct Node { name: Str, kids: Chain[Node], }
```

Two built-in generic types through which a definition may refer to itself. Neither can be changed
after it is made, and that is what makes them safe.

| | `Box[T]` | `Chain[T]` |
| --- | --- | --- |
| Holds | one `T` | a sequence of `T` |
| Made by | `box_new(v: T) -> Box[T]` | `chain_from_vec(xs: Vec[T]) -> Chain[T]`, a copy of `xs`'s elements as they are then — a later change to `xs` does not reach it; `chain_empty[T]() -> Chain[T]` |
| Read by | `box_get(b) -> T`, a copy by `T`'s own law | `chain_len(l) -> Int`; `chain_get(l, i) -> T`, `N0405` outside `0..chain_len(l)`; `for x in l` (*`for` loops*) |
| Changed by | nothing | nothing |
| Assignment | shares: another reference to the same immutable value | shares |
| Equality | what it holds, by that type's equality (L6) | element by element, the lengths first (L6) |
| Into a task | refused (`N0321`), as a handle is | refused (`N0321`) |

**The kind.** Both are **shared immutable values** (*The memory constitution*), the kind `Str`
is: assignment shares, nothing can write through a copy, and the storage is reclaimed when the last
value naming it dies. A `Box[Vec[Int]]` shares the `Vec` it holds, and the `Vec` is still mutable
through whatever reaches it — immutability is of the `Box`, not of what a handle inside it names.

**The ownership rule.** An edge of the ownership graph of definitions (*Generics*, *What may not be
expressed: an ownership cycle*) through a `Box` or a `Chain` is labelled **immutable**, and one
through a mutable handle — `Vec`, `Ints`, `Strs`, `Bytes`, `Chan[T]`, and since R1b `Cell[T]` — **counted**, as before. A path's
label is the strongest along it: counted over immutable over inline. Then:

- a cycle whose every edge is inline is an impossible layout — `N0336`, unchanged;
- a cycle with a counted edge anywhere on it is an ownership cycle — `N0359`, unchanged;
- a cycle of inline and immutable edges, at least one immutable, is **accepted**.

So `enum Expr { Num(v: Int), Add(l: Box[Expr], r: Box[Expr]) }`, `struct Node { kids: Chain[Node] }`
and `struct Tree[T] { kids: Chain[Tree[T]] }` are legal; `struct Node { kids: Vec[Node] }`,
`struct Node { kids: Box[Vec[Node]] }` and `struct A { kids: Vec[B] } struct B { up: Box[A] }` are
still refused — each closes its cycle through a `Vec`.

**Why counting still reclaims everything** (*Cycles* requires the argument before a recursive type
is admitted). A cycle among values follows edges of the definitions' graph, so it is a closed walk
there, and a closed walk through an edge contains a simple cycle through that edge. A value cycle
through a mutable handle would need a definition cycle through a counted edge, which is refused. A
value cycle through inline and immutable edges only cannot exist: inline containment is finite, and
an immutable edge points at a value that existed before the `Box` or `Chain` holding it was made and
can never be made to point elsewhere — the fact that already makes closure environments safe
(*Cycles*, Gate 1-C1, fact 1). So every value of every type a program can write is still reclaimed
by counting.

**Release is not recursion.** A recursive value can be as deep as a program makes it — a list of a
million boxes is a million levels — and releasing it must not take a million stack frames. Every
implementation releases what a value owns through a worklist: a sequence or indirection whose last
reference dies is queued and emptied in a loop, so releasing any value takes stack bounded by the
types, not by the value.

**Closures.** `N0616` (*Cycles*) is about **mutable** handles: a closure may capture a `Box` or
`Chain` that can hold a function value, because what either holds was made before it and cannot be
replaced, so the closure that captures one cannot be inside it.

**Names.** `Box`, `Chain` and their built-ins arrive after 1.0, so they are **prelude names**: a
module that declares a type named `Box` or `Chain`, or a function named like one of the built-ins,
keeps its own, and the built-in is hidden in that module. No program valid under 1.0 changes
meaning. The built-in names of 1.0 (`Int`, `Vec`, `Result` and the rest) stay unshadowable (`N0331`).

### A mutable cell: `Cell[T]` — R1
<!-- id: cell -->

Settled by R1b, 2026-10-12 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L7). EXPERIMENTAL until
R1 closes.

```nazm
struct Counter { seen: Cell[Int], }

fn note(c: Counter) -> Int {
    cell_set(c.seen, cell_get(c.seen) + 1)
}

fn main() -> Int {
    let c = Counter(seen: cell_new(0));
    note(c);
    note(c);
    cell_get(c.seen)          // 2: both calls wrote the one cell
}
```

One value that can be replaced, reached through a handle: the place a callee writes so that its
caller sees the change — a context record's counters, a compiler's next fresh name.

| | `Cell[T]` |
| --- | --- |
| Holds | one `T` |
| Made by | `cell_new(v: T) -> Cell[T]` |
| Read by | `cell_get(c) -> T`, a copy by `T`'s own law |
| Changed by | `cell_set(c, v) -> Int`: stores a copy of `v` and returns 0. The new value is taken before the old one is released |
| Assignment | shares: `let d = c;` makes `d` the same cell, and a `cell_set` through either is seen through both |
| Equality | refused (`N0304`), as on `Vec` |
| Into a task | refused (`N0321`), as a `Vec` is |
| Destruction | when the last reference dies, what it holds is released by `T`'s law, then the storage |

**The kind.** A `Cell[T]` is an **owned mutable handle** (*The memory constitution*), the kind
`Vec[T]` is, and follows the sequence law: it is a `Vec[T]` that always holds exactly one element, and
every implementation may represent it as one.

**The ownership rule.** An edge of the ownership graph through a `Cell` is **counted**, as one through a
`Vec` is (*Immutable indirection*). So `struct Node { next: Cell[Node] }` and
`struct A { b: Cell[B] } struct B { a: Box[A] }` are ownership cycles (`N0359`) — a cell could be set
to hold the value that holds it — and a `Cell` inside a `Box` stays a handle: `Box[Cell[Int]]` is a
box whose one value is a cell, and the cell can still be written.

**Closures.** A `Cell` is a mutable handle for `N0616` (*Cycles*): a closure may not capture a `Cell`
that can hold a function value.

**Names.** `Cell`, `cell_new`, `cell_get` and `cell_set` are **prelude names**, like `Box`'s: a module
that declares a type named `Cell`, or a function named like one of the three, keeps its own.

### Scope-exit cleanup: `defer` — R1
<!-- id: defer -->

Settled by R1b, 2026-10-13 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L12). EXPERIMENTAL until
R1 closes.

```nazm
fn total(out: OutCap, xs: Vec[Int]) -> Result[Int, Str] ! {io} {
    print("start\n");
    defer { print("done\n"); }      // on every way out, the early one included
    let mut sum = 0;
    for x in xs {
        if x < 0 { return Result[Int, Str].Err(error: "negative"); }
        sum = sum + x;
    }
    Result[Int, Str].Ok(value: sum)
}
```

`defer { … }` registers a block to run when the block it is written in is **left** — by reaching its
end, or by `return`, `?`, `break` or `continue` out of it. Every way out runs it; nothing else does.

| | |
| --- | --- |
| **When** | Each time its block is left after the `defer` statement was reached. A `defer` not yet reached — written after a `return` that runs first — does not run |
| **Order** | The last registered first. Several in one block run in reverse; a block's run before the blocks around it, so leaving several blocks at once — a `return` — runs the innermost block's first |
| **With a value** | A `return e` or a block's final expression is computed **before** the deferred blocks run, and is what is returned or produced; a deferred block cannot change it |
| **`break` and `continue`** | Run the deferred blocks registered inside the loop body they leave, not those around the loop |
| **What it sees** | The bindings in scope where it is written, as they are when it runs: a `let mut` assigned after the `defer` is seen with its new value |
| **Its body** | A statement block: a value its last line produces is discarded, as a loop body's is (R1, 5b.19; `N0300` before), and nothing that leaves it — `return`, `?`, or a `break` or `continue` out of it (`N0631`). A loop inside it may `break`; a closure inside it may `return` |
| **What it is not** | Unwinding. A failure that stops the program (`N04xx`, an exit) runs no deferred block. A `scope` waits for its tasks before its own deferred blocks run, since they are the block's |

**The word.** `defer` is a word only at the start of a statement and before `{`; everywhere else it
is a name a 1.0 program may use, and no program valid under 1.0 changes meaning.

### Patterns: wildcards, alternatives, nesting and literals — R1
<!-- id: patterns -->

Settled by R1a, 2026-10-12 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L3).

```nazm
fn describe(e: Expr) -> Str {
    match e {
        Expr.Num(v: 0) => "zero",
        Expr.Num(v: n) => num_to_str(n),
        Expr.Add(l: _, r: _) | Expr.Neg(e: _) => "compound",
    }
}

fn or_zero(o: Option[Int]) -> Int {
    let Option.Some(value: x) = o else { return 0; };
    x
}
```

A pattern is one of:

| Pattern | Matches | Binds |
| --- | --- | --- |
| `_` | anything | nothing |
| `name` | anything | `name`, immutable, to the value (by its type's own law, as a payload binding is) |
| a literal — an integer (with a leading `-` allowed), a string, `true`, `false` | a value equal to it by the type's equality (*Equality is derived*) | nothing |
| `Enum.Variant(field: pattern, …)` | that variant, with every payload field matching its pattern | what its fields' patterns bind |
| `p \| q \| …` | anything one alternative matches | the same names, of the same types, in every alternative (`N0629`) |

Every payload field of a variant pattern is still named exactly once (*Enums*), so adding a
field still breaks the patterns that did not account for it; `field: _` declines it.

**`match`.** The scrutinee is evaluated once; the arms are tried in the order written and the first
whose pattern matches runs, with its bindings in scope in its body only. A `match` may now have any
scrutinee type a pattern fits: an enum, `Bool`, an integer type, `Str`, `Text` — and any type at all
with only `_` and names. A float has no literal pattern (`N0300`): its equality is not what a
literal reads as.

- **Exhaustive.** Every value must be matched by some arm — `N0343`, naming a value no arm matches
  (for an enum, the variants and the payloads still uncovered; for an integer or a string, that a
  `_` or a name is needed).
- **Nothing unreachable.** An arm every value of which an earlier arm already matches is `N0344`.
- **`_` over an enum.** `_` and a name may cover variants the arms above do not name, so adding a
  variant no longer breaks such a `match`. That gives up the protection *Exhaustiveness* (*Enums*)
  describes, where it is written — and so, since R1b (L15), a `_` or a name in a `match` of an
  enum that covers variants no arm names is a **warning**, `N0632`, listing them: the program is
  accepted, builds and runs, and `--deny-warnings` refuses it. An enum matched variant by variant
  keeps the protection.

**`if let`.** `if let P = e { A } else { B }` is `match e { P => A, _ => B }`; without `else` it is a
statement and `B` is empty. `P`'s bindings are in scope in `A` only.

**`let … else`.** `let P = e else { D };` is a statement: `e` is evaluated, and if `P` matches its
bindings are in scope for the rest of the block; otherwise `D` runs, and `D` must leave — `return`,
`break`, `continue`, or a call that does not return (`N0630` when it can complete). A pattern no value
can fail, such as a name, is refused as pointless only by being ordinary: write `let` instead.
*(L5 added `let P = e;`, a `let` whose pattern must match every value — see* Tuples*.)*

**Guards** *(R1, 5b.17)*. `P if G => E`: the arm is chosen where `P` matches and then `G`, a `Bool`
evaluated with `P`'s names bound, is true (`N0301` when it is not a `Bool`); where `G` is false the
arms below are tried, in order, as if this one did not match. A guarded arm covers nothing: it is
left out of exhaustiveness and of whether a later arm is reachable, so `n if n < 0 => …` still needs
an arm for the rest. Arms are tried top to bottom, and a guard runs only when its pattern matched
and no arm above was chosen; it may call, and may have effects, as a condition may.

**What is unchanged.** Matching evaluates nothing but the scrutinee and the guards of the arms it
tries, and compares by equality laws already specified; releasing the scrutinee and the payloads
follows *Enums* exactly. There are still no range patterns, no record patterns, no `..` in a payload
and no `@` bindings.

### Tuples — R1
<!-- id: tuples -->

Settled by R1a, 2026-10-12 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L5).

```nazm
fn div_mod(a: Int, b: Int) -> (Int, Int) { (a / b, a % b) }

fn total(pairs: Vec[(Str, Int)]) -> Int {
    let mut t = 0;
    for (name, n) in pairs { t = t + str_len(name) * n; }
    t
}

fn main() -> Int {
    let (q, r) = div_mod(17, 5);
    let p = (q, "x");
    p.0 * 10 + r
}
```

**Type and value.** `(A, B, …)` is the type of a tuple of two to eight elements, in order; `(a,
b, …)` is a tuple, each element evaluated left to right. A trailing comma is allowed. `(T)` and
`(e)` are `T` and `e`, parenthesised, as they always were; `(T,)` and `(e,)` are refused (`N0101`)
— there is no tuple of one, and none of none — and so is a ninth element: past eight, the elements
want names, which a record gives them.

**What a tuple is.** A tuple is an anonymous record: one generic definition per arity, whose
fields are named `0`, `1`, … and whose type parameters are the element types. So everything
*Records* says holds of it — it is a value; equality is element by element where every element has
equality (*Equality is derived*), and `N0304` otherwise; it is released, element by element, by
its type's own law; it crosses into a task when every element may; a binding's mutability reaches
every element. Two tuples of the same element types are the same type wherever they are written,
in any module: an interface writes a tuple as its elements, not as a definition. Diagnostics spell
the type `(Int, Str)`.

**Element access.** `t.0`, `t.1`, … reads an element (`N0333` for a position the tuple does not
have); `t.0.1` is the second element of the first. `t.1 = e;` replaces one, when `t` is `let mut`
(`N0310` otherwise).

**Patterns.** `(p, q, …)` matches a tuple of as many elements whose every element matches its
pattern (`N0300` for another arity or a value that is not a tuple); it nests in and around the
other patterns (*Patterns*), and exhaustiveness considers every combination of the elements.
`let P = e;` is a `let` whose pattern must match every value of `e`'s type — `N0343`, naming a value
it does not match, otherwise; its bindings are in scope for the rest of the block.
`for P in e { … }` with a tuple pattern is `for x in e { let P = x; … }`.

**What is unchanged.** There is no unit type `()`, no tuple struct and no indexing by a computed
position. A tuple is not a `Vec`: it is not iterable. Derived hashing and ordering wait for L6,
and with them a tuple as a map's key.

### Ordering and hashing are derived — R1
<!-- id: order-hash -->

Settled by R1a, 2026-10-12 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L6).

```nazm
use "@std/hash";
use "@std/hashmap";

struct Point { x: Int, y: Int, }

fn main() -> Int {
    let m = hashmap_new[(Str, Point), Int]();
    hashmap_set(m, ("a", Point(x: 1, y: 2)), 7);
    let first = if "apple" < "banana" && (1, "b") < (2, "a") { 1 } else { 0 };
    first + if Point(x: 1, y: 9) < Point(x: 2, y: 0) { 10 } else { 0 }
}
```

Like equality, ordering and hashing are not declared, written or opted into: they are **derived**
from what a type contains, and a type has them exactly when every component does.

**Ordered types.** `< <= > >=` apply to two operands of one type that is:

| type | order |
| --- | --- |
| an integer type | numeric, as before |
| `Bool` | `false` before `true` |
| `Str` | by bytes, lexicographically: the first differing byte decides, and a prefix comes first |
| a tuple | lexicographically, by position |
| a record | lexicographically, by its fields **in the byte order of their names** |
| an enum | by its variants **in the byte order of their names**, then, within one variant, its payload fields in the byte order of their names |
| a concrete instance of a generic record or enum | as above, over its fields after substitution |

A float compares numerically, as before, but a record, enum or tuple holding one has no order: an
IEEE 754 comparison is not a total order, and a derived order must be. A sequence, `Vec`, `Box`,
`Chain`, channel or function value has none, nor does an unconstrained type parameter; `N0304`
names the component responsible, as for equality.

**Why names, not declaration order.** *Equality is derived* holds that the order in which fields
and variants are declared is not part of what a type means. The derived order keeps that law:
reordering a declaration never changes a comparison, a sort or a map's order. An order that means
something particular — `Low` before `High` — is written as a key: an integer, or a tuple of them.
The derived order is consistent with equality: `a == b` exactly when neither is less than the
other.

**Hashing.** `Hash` (`@std/hash`) is satisfied, without an impl, by a concrete record, enum or
tuple type whose every component is an integer type, `Bool`, `Str`, or itself such a type. So
`hashmap_new[(Str, Int), V]()` and a map keyed by a record need no impl. A component whose type
has a written `Hash` impl of its own makes the container need one too: that impl's `same` may mean
more than `==`, and a derived `same` is `==`. For a derived type:

- `same(a, b)` is `a == b`;
- `hash` of a record or tuple starts from `hash_u64(0)` and folds each component's hash into it
  with `hash_combine`, the fields in the byte order of their names (a tuple's by position);
- `hash` of an enum value starts from `hash_bytes` of its variant's name and folds its payload
  fields the same way;
- a component's hash is its type's: `hash_u64` of an integer's 64-bit pattern (sign-extended) —
  of a 128-bit one (R1b, L13b), `hash_combine` of its low word's and its high word's —
  `hash_u64` of 0 or 1 for a `Bool`, `hash_bytes` for a `Str` — exactly the library's impls.

So a derived hash is the same in every implementation and every run, and unchanged by reordering
a declaration. A type with a written impl keeps it: derivation applies only where there is none.

**What is unchanged.** Equality is as *Equality is derived* says. A generic body still needs a
requirement: inside `fn f[T: Hash](x: Pair[T])`, `Pair[T]` is not derived — only a concrete type
is. There is no ordering requirement on a type parameter yet (`T: Ordered` waits for
`@std/ordmap`, S3), and no customised order: a type cannot define its own `<`.

### Constant tables and exported constants — R1
<!-- id: const-tables -->

Settled by R1a, 2026-10-12 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, L8).

```nazm
struct Keyword { text: Str, code: Int, }

pub const KEYWORDS: [Keyword; 3] = [
    Keyword(text: "fn", code: 1),
    Keyword(text: "let", code: 2),
    Keyword(text: "match", code: 3),
];
pub const ORIGIN: (Int, Int) = (0, 0);

fn code_of(word: Str) -> Int {
    let mut i = 0;
    while i < 3 {
        if KEYWORDS[i].text == word { return KEYWORDS[i].code; }
        i = i + 1;
    }
    ORIGIN.0
}
```

**Aggregate constants.** A constant's type may also be a record, an enum, a tuple or an array
(*Arrays*, which may hold strings since L8), and its value is then built from those: a
construction `R(f: v, …)`, a variant `E.V(f: v, …)`, a tuple `(v, …)`, or an array `[v, …]` or
`[v; n]`, each `v` again such a value or a scalar constant expression as *Constants* allows —
literals, constants, operators and numeric conversions. Nothing else: no call, binding or effect
(`N0622`). The checker computes it once, where the module's types are known; a scalar part that
would trap is refused where the constant is declared (`N0623`), as before. Every use is that
value, built where it is used: a constant holds no storage of its own, so a use of a table is a
copy of it and no two uses share one. An aggregate constant is not an array's length.

**Exported constants.** `pub const NAME: T = value;` publishes the constant's value in the
module's interface, and an importer uses it by its name — `NAME`, or `m::NAME` after `use "…" as
m;` — exactly as a constant of its own: the value, written where it is used. (A function imported under
a name is then a value as `m::f` too, as an unqualified one is, *Function values*.) Every record and
enum `T` mentions must be public, as for an exported function's signature. Changing an exported
constant's value changes the interface, so every importer is checked and built again. `pub
static` is still refused (`N0101`): a static is storage, and storage is its module's.

### The platform, further: processes and the file system — R1d
<!-- id: platform-r1d -->

Settled by R1d, 2026-10-14 (`openspec/changes/archive/2026-10-14-r1-language-for-the-rewrite`, D4's built-ins, P1 and
P2). Stable since R1 closed (0.10.0). Seventeen built-ins beside *Files and handles* and *Process and
environment*, whose rules — statuses, generations, `os_close`, the restricted sink — they keep.
Every one is a **prelude name** (*Immutable indirection*, **Names**): a module that declares a
function of the same name keeps its own, so no program valid under 1.0 changes meaning.

```nazm
use "@std/ioerror";
use "@std/process";
use "@std/time";

fn main(p: ProcessCap, t: TimeCap) -> Int ! { io } {
    let args = strs_new();
    strs_push(args, "-c");
    strs_push(args, "echo $GREETING");
    let env = strs_new();
    strs_push(env, "GREETING=hello");
    // In /tmp, with exactly that environment, leading a new group, reading nothing.
    match process_spawn_with(p, "sh", args, "/tmp", env, true, true) {
        Result.Ok(value: c) => match process_wait_until(c, time_deadline(t, 5000)) {
            Result.Ok(value: Option.Some(value: status)) => status,
            _ => { process_kill_group(p, process_child_id(c), 9); 1 },
        },
        Result.Err(error: e) => 2,
    }
}
```

| | |
| --- | --- |
| **Standard output and errors** | `os_stdout() -> OsHandle` and `os_stderr() -> OsHandle`: the process's standard output and errors, the same handle on every call, as `os_stdin`'s is. `os_write` writes raw bytes to them, in order with `print` and `eprint`, which are unchanged. Reading one answers `-EBADF`, seeking `-ESPIPE`, syncing `InvalidInput`, listing `NotADirectory`, locking `InvalidInput`. `os_close` of one makes that handle `Closed` and leaves the stream open: `print` keeps writing. Each needs an `IoCap` held and has the `io` effect |
| **A child's directory and environment** | `os_spawn_in(program, args: Strs, dir: Str, env: Strs) -> OsHandle`: as `os_spawn`, the child's working directory `dir` (`""`: this process's) and its environment exactly `env` — each entry `NAME=value` with a `=` after its first byte and no zero byte, no name twice, else `InvalidInput` and nothing started — given to the child **ordered by name**, byte by byte, so every implementation gives the same order. The program is found on **this** process's `PATH`, as `os_spawn` finds it, not on `env`'s; a relative program named with a `/` is taken relative to `dir` — made absolute beside `dir`'s canonical path before anything starts, since systems differ on whether the directory changes before the program is looked for. `dir` is asked about first: one that does not exist is `NotFound`, one that is not a directory `NotADirectory`, on every system, and nothing is started |
| **Spawn options** | `os_spawn_with(program, args, dir, env, flags: Int) -> OsHandle`: `os_spawn_in` with `flags` the sum of `1` — the child **leads a new process group**, whose id is its process id — and `2` — its standard input is the **null device**, read as empty, and `os_child_pipe(h, 0)` answers the `Closed` status. Any other bit is `InvalidInput`. `os_spawn_in(…)` is `os_spawn_with(…, 0)` |
| **Ids** | `os_pid() -> Int`: this process's id; an `IoCap` held, `io`, and `argument`-origin, as the environment is. `os_child_pid(h) -> Int`: a child's process id — the same before and after it is waited for, and the id of the group it leads when started with flag `1`; only the handle |
| **Bounded waits** | `os_try_wait(h) -> Int`: `os_wait`'s answer if the child has ended, else `WouldBlock`'s status at once. `os_wait_until(h, deadline: Int) -> Int`: `os_wait`'s answer if the child ends before `time_now_ms()` reads `deadline` — the clock `@std/time`'s `time_deadline` reads — else `TimedOut`'s status, the child left running; a deadline already passed looks once. The wait holds the task's worker and looks again at most every 10 ms, so it may answer up to that long after the child ends. Once a child has ended, `os_wait`, `os_try_wait` and `os_wait_until` each answer the same, every time. Only the handle |
| **Signalling a group** | `os_kill_group(pgid, sig) -> Int`: signal number `sig` — the platform's numbering; `9` and `15` are `SIGKILL` and `SIGTERM` everywhere, and `0` sends nothing and asks whether the group has a process — to every process in group `pgid`: `0`, or the system's status (`-ESRCH` for a group with none, `PermissionDenied`). A `pgid` outside `2..2^31` or a `sig` outside `0..=31` is `InvalidInput`, never sent: `pgid` `1`, `0` and below would name every process, or this process's own group. It reaches processes no handle names, so it needs a `ProcessCap` held, and has the `io` effect |
| **Peak memory** | `os_child_rusage(h) -> Int`: the peak resident memory, in bytes, of a child already waited for (by any of the three waits); `InvalidInput` before. The interpreter cannot ask the system for it without C and answers `-ENOSYS` (kind `Other`) |
| **Symbolic links** | `os_lstat(path, out: Bytes) -> Int`: as `os_stat`, about `path` itself — a final symbolic link is not followed — writing **six** little-endian `Int`s (`N0405` if `out` is shorter than 48 bytes): the size (a link's: its target's length in bytes), the kind (`0` a file, `1` a directory, `2` anything else, `3` a symbolic link), the modification time in nanoseconds, the permission bits, the **device** and the **inode** — two paths name the same file exactly when both match. `file`-origin, into the buffer |
| **Permissions** | `os_chmod(path, mode) -> Int`: `path`'s permission bits made `mode`, following a symbolic link; a `mode` outside `0..=0o7777` is `InvalidInput`. Its path is the restricted sink a written file's is |
| **Canonical paths** | `os_realpath(path, out: Bytes) -> Int`: the absolute path `path` names, every `.`, `..` and symbolic link resolved, into `out`: its length, or a status — the system's for a path that does not exist, `InvalidInput` when `out` is too short. `file`-origin |
| **Temporary directories** | `os_mkdtemp(parent) -> Str`: a **new**, empty directory in `parent`, named `nazm-` and twelve random lowercase letters and digits, made by this call and by no other (the name was free), with permissions `0o700` where the system keeps permissions; its path — `parent`, a `/` unless `parent` ends with one, and the name — or `""` when none could be made: an empty `parent`, the system's refusal, or 64 names all taken. A string, not a buffer, so the path is exactly as clean as `parent` (*Provenance*): a program may write under a directory it made, where a path read out of a `Bytes` would be `file`-origin and refused as a write's path (`N0372`). Nothing removes it. `parent` is the restricted sink |
| **File locks** | `os_lock(h, exclusive: Bool) -> Int`: an **advisory** lock on the whole file `h` opened — exclusive, or shared with other shared locks — waiting until it can be had, which holds the task's worker; `0` or a status. `os_try_lock(h, exclusive)`: the same without waiting — `WouldBlock`'s status while another holds it. `os_unlock(h) -> Int`: `0`. A lock belongs to the **open file**: another `os_open_*` of the same path, in this process or another, is another holder, and closing the handle lets the lock go. Asked again through the same handle, a lock changes kind. A directory's handle is `IsADirectory`; any handle but a file's, `InvalidInput`. Only the handle |
| **Authority** | `os_stdout`, `os_stderr`, `os_pid`, `os_lstat`, `os_chmod`, `os_realpath` and `os_mkdtemp` need an `IoCap` held; `os_spawn_in`, `os_spawn_with` and `os_kill_group` a `ProcessCap`; the rest only their handle. Every one has the `io` effect. Which program runs and in which directory — `os_spawn_in`'s and `os_spawn_with`'s first and third arguments — and the paths of `os_chmod` and `os_mkdtemp`, are the restricted sink (`N0372`) |
| **Where** | Native programs on every hosted target: macOS (`flock`, `posix_spawn` with `posix_spawn_file_actions_addchdir_np`, `wait4`), Linux likewise, and the interpreter. On **Windows** (*Windows x86_64*): the locks are `LockFileEx`, enforced as well on other handles' reads and writes, and a second lock through one handle is another holder; there are no process groups, so flag `1` is `CREATE_NEW_PROCESS_GROUP` and `os_kill_group` ends the process `pgid` itself with exit code 1 for any `sig` but `0`; `os_chmod` keeps only the owner's write bit, as `_wchmod` does; a symbolic link is any reparse point, the device the volume's serial number and the inode the file index. On **Android** (API level 30), whose C library cannot start a program in another directory, a `dir` other than `""` answers `-ENOSYS`. On **WASI** (*WASI*): `os_stdout` and `os_stderr`; `os_lstat` by `path_filestat_get` without following, its permission bits `0o644` as `os_stat`'s are; `os_mkdtemp`, with no permissions to set; `os_chmod`, `os_realpath` and the locks answer `-ENOSYS`, as listing a directory does; and the process built-ins, which need what WASI preview 1 has not got, are refused by name, as tasks are. A **board** has no files and no processes and refuses them all by name |

**What is not here.** Watching for a signal (a program still cannot handle `SIGINT`), setting this
process's own environment (`setenv` is unsafe beside the task pool's threads; a child's is given by
`os_spawn_in`), creating symbolic links, memory mapping, and removing a tree: the names a directory
lists are `file`-origin, so no Nazm function may remove them by name without `str_vouch` (`N0372`),
and a built-in to do it is left for when the harness needs one.

### Not in Nazm 1.0, and unsupported rather than half-present
<!-- id: not-in-v0-3 -->

Valueless `return`, labelled break, `for`, `loop`, mutable parameters, floating point. A
program using any of them is rejected by name rather than misparsed into something else.

**Corrected 2026-10-11, R1a.** `for` left this list: `for x in e` and `for x in a..b` exist
(*`for` loops*, above). A `for` in any other shape is still refused by name, `N0101`.

**Corrected 2026-10-07, Gate 2.** Floating point left this list: `Float32` and `Float64` exist,
with fixed-width integers and bit operators (*Numbers*, above). The compatibility corpus keeps
its refusal case for `1.5` and lists it as superseded (`tests/compat/v1/SUPERSEDED`).

**Corrected 2026-10-07, Gate 1.** The section's anchor and the corrections below keep the name
*Not in v0.3*; the list, with them, is the current answer for 1.0. Floating point was the one
entry not refused by name: `1.5` was reported as a field read whose name is a number (`N0100`).
It is refused by name as floating point (`N0101`) since Gate 1.

**Corrected 2026-10-07, R1.** The N50 correction below says traits and methods "remain absent
and are refused"; N78 added them, nominal and static (*Traits and methods — N78*, above).
Trait objects and dynamic dispatch, effect handlers, user-defined effects and effect
variables beyond one effect parameter per function remain absent. This list, with these
corrections, is the current answer for v0.3; `limitations.md` names the narrower boundaries
of what is present.

**Corrected 2026-10-01, N50.** Function values and closures exist, specified above; traits and
methods remain absent and are refused.

**Corrected 2026-10-01, N49.** String escapes, the logical operators `&&`, `||` and `!`, and
qualified names exist, each specified above; native recursion compiles.

**Corrected 2026-09-29, N36.** This list named **effects**, which N36 added: a function may
declare the effects it exercises and the checker infers and holds them — *Effects: what a
function may do*, above, is the law. Handlers, effect variables and user-defined effects
remain absent.

Exceptions, `throw`, `catch` and unwinding are not in the language and are not planned:
N12's typed error values are values, and `?` is a `return` (*Typed error values*, above).

**Corrected 2026-09-23, N11.** This list named **generics**, which N11 added: records, enums
and functions may declare type parameters, and `Vec[T]` is the one generic built-in — the
section *Generics* above is the law. **Traits, methods, constraints and every other item in
*What generics are not, in N11*** remain absent and remain refused.

**Corrected 2026-09-23.** This list also named **user-defined types**, which N9 added:
`struct` declares a record, and the section *Records* above is the law. It then named
**variants and pattern matching**, which N10 added: `enum` declares a closed sum type and
`match` eliminates one, and the section *Enums* above is the law. Generics, traits and
methods remain absent and remain refused by name.

**Corrected 2026-09-21.** This list also named **I/O, modules and string concatenation**,
and the paragraph below it said there was deliberately no `print` — while all three are
specified two to four hundred lines earlier in this same file. `print` and `eprint` exist;
see *What the outside-world built-ins do*, *A file is a module*, and `str_concat`. The
reasoning that was true, and still is, is that output is an effect **the language does not
track**; what shipped is ambient authority, labelled as a limitation rather than designed.
The original paragraph, unedited:

> `nazm run` prints the value of `main`. There is deliberately no `print`: output is an
> effect, effects are the design's central mechanism, and a builtin that bypassed the
> effect system before it exists would be the wrong shape to unpick later.

## Open — later phases must settle these
<!-- id: open -->

*Not normative.* Everything below this heading is undecided or research. Nothing in it
describes what 1.0 does; where an item was later settled, it is struck through or marked
*Settled by …* and points to the section that now governs.

### Source identity and definition identity
<!-- id: open-identity -->

*Added 2026-09-21. These are the questions the next milestone — `roadmap.md` N1 — has to
answer, and they are semantic rather than merely structural, which is why they are here.*

- ~~**What is a compilation unit?**~~ **Settled by N2, 2026-09-22: one module, and today
  one module is one file.** A module owns its definitions, exports the ones marked `pub`,
  and is checked against the interfaces of the modules it directly imports rather than
  against their bodies. What remains open is not what a unit *is* but what else may become
  one: a package presenting several files as a single unit, and a generated or virtual
  source with no file behind it. Both are additions to how a module comes into existence,
  not revisions of what one means.
- ~~**What does a span identify?**~~ **Settled by N1, 2026-09-21: a span identifies a file
  and a byte range within it.** `Span` is `{ file, start, end }`, offsets are file-local,
  and the file is carried from the lexer rather than recovered from a merged offset. The
  published diagnostic shape did not change — `nazm.diagnostic/1` still reports offsets
  into the concatenation of all files, because `diagnostics.md` promises the shape will not
  move inside a version. That a program's *meaning* is unaffected is why this was an
  implementation change and not a language one.
- ~~**What is the stable identity of a definition, and what changes it?**~~ **Settled by
  N3, 2026-09-22.** A definition is identified by the module that owns it, the kind of
  definition it is, and the name it is declared under; a module is identified by where its
  source sits relative to the compilation's source root. Both survive the process. See
  **Durable identity** below.
- **Do built-ins share that identity model?** They live in the ordinary function namespace
  and may not be redefined, but they are declared in a compiler table rather than in
  source. Whether they are definitions with identities, or something else the resolver
  knows about, is **still undecided**. What N1 settled is narrower and separate: there is
  now **one** built-in inventory rather than two, and resolution answers "built-in or user
  function" once for every call site. Neither makes a built-in a definition.

*Updated 2026-09-22.* This paragraph used to end "the first answer that *does* change a
meaning — visibility, most likely — has to be given deliberately rather than discovered."
It was given deliberately: see **Visibility: private by default, `pub` exports** above.
What is left in this section changes no current program's meaning.

### Memory and values
<!-- id: open-memory -->

*Copy semantics, aliasing, parameter and return conventions, reclamation, cross-task
transfer and cycles were settled by N7 — see **The memory constitution** above. What
remains open is what the four kinds do not yet cover.*

- **`Str` storage.** A `Str` may point into a literal, into `argv`, into a heap buffer, or
  into the interior of any of them, and the value does not record which. Reclaiming one
  needs a representation that does. Which representation, and what it costs on a type that
  is copied in every hot loop, is unsettled.
- **Channel storage.** A channel outlives the tasks that use it and is bounded only by its
  `scope`. Whether that bound is enough to reclaim it, or whether the runtime needs its own
  count, is unsettled.
- **User-defined and recursive types.** The constitution says what admitting a recursive
  type requires. It does not say which of those routes the language takes.
- Closure capture: which conventions may a closure capture under, and what escapes?
- Copy elision: what does the compiler *guarantee*, versus what does it merely usually do?
  MVS is unusable if this is folklore.
- Arena/region assignment rules, and how region inference interacts with the exclusivity
  check when they are (by design) the same solve.

### Effects
<!-- id: open-effects -->

*Settled by N36*: what an effect is, the two built-in effects, declared and inferred sets,
purity as the empty set, and what crosses a module (*Effects: what a function may do*). Still
open:

- **Is divergence an effect?** The `forever()` counterexample in `architecture.md` §5 shows
  that without an answer, "drop an unused pure call" is unsound. Options: treat divergence
  as an effect; track termination separately (`willreturn`-style); or refuse the
  transformation entirely.
- Is allocation failure an effect, a panic, or a return value?
- Is panic an effect, an unwind, or both, and what is observable about it?
- Evaluation order: specified, or deliberately unspecified? If unspecified, differential
  testing across backends cannot compare it (`architecture.md` §6).
- Which handlers lower statically, and which require one-shot continuation capture?

### Capabilities
<!-- id: open-capabilities -->

*Settled by N37*: what a capability is, the two kinds, lexical possession, `main` as the root,
unforgeability, separate authority and effect checks, the ambient bridge and its bound, and
static erasure (*Capabilities: what allows a function to act*). Still open:

- **Attenuation and resource-specific authority.** A read-only `IoCap`, one file's, one
  directory's: each needs a kind with a resource argument, and the rule for deriving a
  narrower capability from a wider one. v1 has no narrower kind to derive into.
- **Linearity.** Whether some authority should be affine — usable once, or movable but not
  copyable — needs move semantics the language does not have.
- **Revocation and lifetime.** A copied capability lives as long as any copy; a scoped grant
  needs a lifetime analysis.
- **Retiring the bridge.** *Settled by N104*: no function is ambient; a program declaring
  nothing takes its authority as parameters from `main`.
- **Runtime enforcement.** Whether a later milestone backs the static check with an
  operating-system boundary for a program that crosses into foreign code.
- **Foreign and unsafe authority.** A foreign-function call or unsafe code would need
  authority of its own kind.

### Provenance and information flow
<!-- id: open-provenance -->

*Settled by N38*: the four origins, explicit data flow, joins, summaries and their fixed point,
facts across modules, the separation from effects and authority, and one restricted sink
(*Provenance: where a value came from*). Still open:

- Field-, element- and alias-precise tracking. *Settled in part by N80* (`architecture.md`
  §7.81): containers by type-keyed cells, calls through values and bounds by type-based targets,
  fields and variants within a body. Per-handle, per-index and cross-call precision stay open.

- Implicit flows: is branching on a `Secret` a leak? Full non-interference is usually
  unusable in practice; what is the chosen approximation, and what does it miss?
- Sanitizer functions: how are they declared, and what stops a no-op being declared one?
- Aliasing: how does a label propagate through a projection?
- Declassification: which capability, granted where, audited how?
- The FFI boundary: what label does a value returned from C carry? *Settled as `unknown`* (N38),
  and what C hands an export is `unknown` too (N80).

### Concurrency
<!-- id: open-concurrency -->

- Captured variables, globals/statics, and foreign pointers are **not** covered by channel
  ownership transfer. Each needs a rule.
- Preemption: cooperative at back-edges and calls, or signal-based?
- What exactly does cancellation cancel, and what cleanup is guaranteed to run?

### Layout selection
<!-- id: open-layout -->

- The eligibility predicate: which types may the compiler re-lay-out?
- Behaviour under address identity, escaping references, FFI, serialization, atomics, and
  separate compilation (`architecture.md` §4).

**Settled in part, N68.** A `Vec[R]` whose record `R` has only `Int` and `Bool` fields may be laid
out one array per field: its elements have no address, cross no FFI, are never serialised, and a
record's layout was already by field name, so no program can tell. `nazm build --layout soa` asks
for it (LLVM backend; `aos`, the default, keeps each element whole); every other `Vec` stays element
by element, and `nazm explain-cost` says which and why. Neither layout changes a result or a
failure. Automatic selection and owning elements remain open.

### Backend semantics
<!-- id: open-backend -->

Required before `nazm test --backend=all` means anything:

- integer overflow behaviour
- floating-point semantics (fast-math? reassociation? NaN payloads?)
- panic and unwind behaviour
- what counts as an observable output

### Syntax
<!-- id: open-syntax -->

- The surface syntax itself, which is the **largest lever against the training-data risk**
  and is nearly free to change now and effectively unchangeable later. Novelty is a budget:
  spend it only where it buys semantics unobtainable otherwise.
- ~~A machine-readable grammar~~ — **delivered.** `docs/grammar.ebnf`, checked against the
  real lexer and parser by `crates/nazm-syntax/tests/grammar.rs`, with `docs/guide.md`
  generated from it. Struck through rather than deleted, because the reason it was wanted
  — a spec artefact regardless of whether grammar-constrained decoding is ever used — is
  why it is maintained.
- A cheatsheet under ~3k tokens, to be injected by `nazm init`.

## Non-goals for the current interpreter milestone
<!-- id: non-goals -->

*Historical.* This list was written for the first interpreter milestone and is kept as
that milestone's record. Several items have since been built — a package manager (N46, N56),
`comptime` as a command (N65) and one opt-in layout (N68) — and are specified above; its
later paragraphs are history too. What v0.3 still leaves out is *Not in v0.3*, above, and
`limitations.md`.

Deliberately out of scope:

macros · comptime beyond constant folding · general effect handlers · whole-program taint ·
layout selection · a package manager · a module system beyond files

**Self-hosting was on this list and has been moved off it.** It is now a stated long-term
objective — recorded in the 2026-09-20 decision record — and the staged path was
[`roadmap.md`](roadmap.md) M3.

**Achieved 2026-09-21.** The sentence that closed this section — *"the language has no
collections, no user-defined types, no pattern matching, no string concatenation and no
I/O, so a compiler cannot be written in it yet at all"* — was true when written and is now
false in four of five clauses. Collections, string concatenation, modules and I/O exist,
and the compiler written in Nazm reaches a C2 ≡ C3 fixpoint. User-defined types and pattern
matching were still absent then; the compiler was written around them with parallel `Ints`/`Strs`
arrays rather than waiting. *Corrected 2026-10-07, Gate 1:* records, enums and `match` arrived in
N9 and N10, and the compiler written in Nazm handles them since N102. The fixpoint shows that
compiler reproduces itself, not that it is correct or that it covers the language: it compiles a
declared subset (`limitations.md`, `bootstrap.md`). The heading above — *"the current interpreter milestone"* — no
longer describes the project either. What this list still correctly records is what that
milestone chose not to do.
