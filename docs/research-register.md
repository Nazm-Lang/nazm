# Research register

**Status: hypotheses, kept apart from commitments. Nothing here is a plan, a roadmap entry
or a capability, and nothing here may be cited as evidence for one.**

`master-architecture.md` §1 separates implemented behaviour, accepted design, open
questions and research proposals — a distinction inherited from the 2026-09-20 decision
record, which noted that the last is *"speculative, and not entitled to be described as a
plan"*. That distinction survived the first era in one place — automatic
layout selection, which `architecture.md` §4 labels a proposed research direction pending a
literature review that has not been done — and nowhere else. This document gives the rest of
them the same treatment.

**Every item carries a falsifier.** An item without a result that would invalidate it is
not a hypothesis; it is a preference, and it belongs in `architecture.md` as a design
decision or nowhere.

The spine of the open-question set is `docs/spec.md` *Open* and `docs/performance.md`'s
ranked headroom, both of which state their questions better than a summary would. Those are
referenced, not restated.

---

## R1 · Mutable value semantics is ergonomic enough to be the surface model

**Hypothesis** · A surface model of read-only projection, exclusive mutation, and
consumption/move gives ordinary programs memory safety without a lifetime annotation the
programmer writes, while a full region and borrow calculus is retained *inside* the IR so
the architecture is not trapped if the surface proves insufficient. `architecture.md` §5
states the design and names the hedge.

**Required prerequisite** · A typed core IR to carry places and regions; `docs/spec.md`
*Open* lists the four questions that must be answered first, of which copy elision is the
sharp one — MVS is unusable if what the compiler guarantees, versus what it merely usually
does, is folklore.

**Evidence needed** · The self-hosting compiler rewritten against the surface model without
an escape hatch, and peak RSS bounded by live data rather than total allocation.

**Invalidating result** · A common pattern in the existing `compiler/*.nz` sources that
cannot be expressed without exposing regions in the surface syntax. One would be enough;
the compiler is the honest test case because it was written before the model existed.

**N7 answered this, and the answer was no — for a reason worth keeping.** The hypothesis
was that mutable value semantics should be Nazm's *surface* model. The audit
(`architecture.md` §7.7) established that Nazm's surface model is already the opposite and
has been since sequences existed: `let b = a;` on an `Ints` aliases, a push through one is
visible through the other, and `docs/spec.md` has specified that in those words. Adopting
MVS would not have been choosing a surface model; it would have been **changing** one, and
the measured programs that depend on the current one include every sequence parameter in
the self-hosted compiler.

So the question this item asks — *is MVS ergonomic enough to be the surface model* — turned
out to be the wrong question to ask second. The right one was asked first: *what do values
mean now*. The answer made the hypothesis moot for `Ints` and `Strs`, and **already true**
for `Str`, which has value semantics with no observable copy and needed no machinery at
all to get them.

The prerequisite is also gone rather than met. It said a typed core IR was needed to carry
places and regions, with copy elision as the sharp question. Reclamation arrived with no
core IR, no regions, no places and no elision question, because counting references to a
handle needs none of them.

**What survives, narrower and sharper.** MVS remains a live hypothesis for a *future* kind
of value — user-defined records, where there is no existing meaning to break and where the
choice between value and reference semantics is genuinely open. The invalidating result is
unchanged and is still the right test: a pattern in `compiler/*.nz` that cannot be
expressed without exposing regions.

**Evidence needed** (revised) · Not a rewrite of the compiler. A user-defined record type,
and a demonstration that value semantics for it are ergonomic in the compiler's own
patterns and implementable without regions in the surface syntax.

**Status: RESEARCH, and re-scoped.** Not verified: no design was chosen for records, and a
design chosen is not a hypothesis tested. What changed is that it no longer blocks
reclamation, and reclamation no longer waits on it — which was the dependency this item was
carrying without being asked to.

**N8 sharpened it once more, without answering it.** Completing reclamation for the current
type universe removed the last reason to reach for MVS *defensively* — "peak RSS bounded by
live data rather than total allocation", the evidence this item originally asked for, is now
measured and true for every type the language has, and it was obtained by counting
references rather than by changing what a value means. So the remaining question for records
is a question about **ergonomics and expressiveness alone**, which is what it should always
have been.

One thing N8 did add to the terms. A record is the first type that can be made recursive,
and the leak-freedom claim in `capability-matrix.md` area 4 rests on the fact that no
current value can refer to one that refers back. Whichever way records go, the cycle policy
is part of the design rather than a follow-up: `docs/spec.md`'s *Cycles* says so in advance.
**This item is not resolved by N8 and must not be read as resolved** — strings and channels
being reclaimed says nothing about whether value semantics suit a user-defined type.

---

### N9 ran the experiment, 2026-09-23. Status: **SUPPORTED, not VERIFIED.**

Records exist, and they are **nominal value types whose fields keep their own semantics**:
an `Int` field copies, a `Str` field is a value with the same bytes, an `Ints` or `Chan`
field is another reference to the same storage, and a nested record recurses. No lifetime
syntax, no region, no borrow annotation, and no escape hatch. `docs/spec.md`, *Records*,
is the model; `architecture.md` §7.10 is what it took to implement.

**What the evidence is.** The invalidating result this item names — *a pattern in
`compiler/*.nz` that cannot be expressed without exposing regions in the surface syntax* —
did not appear. Records were implemented in both compilers, used across module boundaries,
passed into tasks, returned from functions on both edges, nested, and copied in loops, and
none of it needed a region, a lifetime or an annotation. 27 memory cases and 15 mutations
hold the ownership half; `spec.md`'s cost table states what a copy costs so that the
ergonomics are not bought with a hidden price.

**And the compiler uses one.** `Place { line, column }` in `compiler/emit.nz`. That is
deliberately one record rather than a migration, and the reason is itself the finding:

* the structures this compiler is built from — the token columns, the node arena, the
  function table, the record table N9 itself added — are **parallel arrays**, and each
  wants a `Vec<Record>` before a rewrite would be honest. There is no container that can
  hold a record, so a migration would have meant packing fields into strings, which is what
  the arrays already avoid;
* several of those arrays are passed **because they are shared mutable state** — the value
  counter, the label counter, the ownership set, the block-liveness flags. A record is a
  value, so it cannot replace them. That is not a defect in records; it is the value/handle
  distinction doing exactly what `spec.md` says it does, and it means records and sequences
  are complements rather than alternatives;
* what was left is the case where the language genuinely could not say what the code meant:
  a function returning two values. `line_of` and `column_of` existed as two functions for
  that reason alone, one scanning forward and one backward. That is a small win, and it is
  a *real* one, and one real win is worth more than a manufactured migration.

**Why SUPPORTED and not VERIFIED.** The hypothesis is about ergonomics, and one record in
one compiler plus a test suite is evidence, not proof. Three questions the experiment did
not reach:

* **no container has been asked the question.** `Vec<Record>` is where value semantics
  usually start to hurt — element access, in-place mutation, and whether `xs[i].f = v` is
  expressible without a place calculus the surface hides;
* **no variant has been asked it.** A payload's ownership is a record's question, but a
  payload that is *sometimes* there is not;
* **copy elision is still folklore.** This item's original prerequisite called that the
  sharp question and it remains unanswered: `let b = a;` on a record with many owning
  fields costs one reference adjustment per field, `spec.md` says so, and nothing yet
  promises when the compiler may elide the pair. `performance.md` measures the cost;
  nobody has specified when it disappears.

**Evidence needed next** · A generic container holding a record, and a statement of when
redundant retain/release pairs may be elided. Either could still invalidate the surface
model; neither has been tried.

---

### N10 asked the second of those three questions, 2026-09-23. Status: still **SUPPORTED,
not VERIFIED.**

*"No variant has been asked it"* was one of the three gaps N9 left. N10 asked it: closed
nominal enums with named payload fields, eliminated by exhaustive `match`, composing with
records in both directions. The answer, on the same terms as before, is that the
invalidating result did not appear — **a payload that is sometimes there needed no region,
no lifetime and no annotation either.**

What the experiment adds to the evidence, and what it does not:

* **the surface model held where the ownership question became dynamic.** A record's copy
  is a fixed walk; an enum's is a branch and then a walk, and which walk is a value the
  program carries. That is the first place the language's memory rules could not be
  answered statically, and the surface did not have to change to accommodate it: `let b =
  a` still means what it meant, and a pattern binding is a borrowed view of a payload with
  nothing written down about how long it lives;
* **the ergonomics were tested where they are sharpest** — a payload returned out of the
  arm that matched it, out of a value dying on the same line. `take_name` is four lines and
  says nothing about lifetimes. That shape is the variant equivalent of N8's return-order
  tests, and it is the one a surface model without regions most plausibly cannot express;
* **the exhaustiveness rule is an ergonomic cost, honestly.** No wildcard arm means adding
  a variant breaks every importer that believed it had handled the whole enum. That is
  intended while the language is young and it is a real friction, and a later
  `non_exhaustive` design will have to weigh it;
* **and the compiler uses one, in the one place a container is not needed.**
  `enum Owning { Nothing, Sequence, Strings, Backing, Channel, Record(at: Int),
  Enum(at: Int) }` replaced the small integer `compiler/emit.nz`'s `owns` returned. The
  node kinds, token kinds and type codes stayed integers, because they live in an `Ints`
  and there is no container that can hold an enum — which is the same wall N9 hit, reached
  from the other side. What `Owning` shows is that the ergonomic win is real where the
  language can express it: the dispatch became exhaustive, and **making it exhaustive found
  a defect N9 had shipped**, where a record crossing a task boundary produced a wrong answer
  and leaked its fields in the self-hosted compiler.

**Why still SUPPORTED and not VERIFIED.** Two of the three gaps N9 named are open, and one
of them is the same one:

* **no container has been asked the question.** Unchanged, and now the sharper of the two:
  `Vec[Token]` is what the self-hosted compiler would need to use an enum for anything, and
  it is what would let an element's payload be mutated in place;
* **copy elision is still folklore.** Unchanged, and an enum makes it slightly worse: a
  copy now costs a branch as well as the reference adjustments, and nothing says when
  either may be elided;
* what N10 *closed* is the variant question, and closing it moved nothing else.

**Evidence needed next** · Unchanged, and now with a name: a generic container holding a
record or an enum, which is `Vec[T]`. It is the prerequisite for the dogfood this item has
now failed to reach twice, and that repetition is itself the finding.

---

### N11 asked the container question, 2026-09-23. Status: still **SUPPORTED, not VERIFIED**
— one of the three gaps closed, the sharpest one left.

`Vec[T]` exists, in both compilers, and holds records, enums, strings and other `Vec`s. The
invalidating result did not appear: **a container of records needed no region, no lifetime,
no annotation and no place calculus.** What made that true is a decision, not a discovery,
and it is the one this item has to weigh:

* **`Vec[T]` is a handle, and its elements are values.** `let b = a` on a `Vec` aliases, as it
  does on an `Ints`; `vec_get` returns **its own copy** of an element by the element's law,
  and `vec_set` replaces one. So a record inside a `Vec` keeps value semantics — mutating the
  copy changes nothing in the vector — and the surface never had to say who may hold a
  reference into the storage, because nobody can.
* **The price is the one this item predicted.** There is no `xs[i].f = v`. Updating an
  element's field is get, change, set — three operations and a copy of every owning field
  twice. That is not hidden: `spec.md` says why the indexing form is absent, and
  `performance.md` measures what the copy costs. It is exactly the place where "value
  semantics usually start to hurt", and the answer N11 gives is to make the hurt explicit
  rather than to remove it.
* **The cycle argument held a second time, by refusal.** A container that can hold a record
  is what makes an ownership cycle expressible, and the type system refuses every definition
  that could form one (`N0359`) while leaving acyclic nesting legal. So counting is still
  complete for every type a program can write.
* **And the dogfood this item failed to reach twice happened.** The self-hosted compiler's
  diagnostics were four parallel arrays and are a `Vec[Diag]` now. No other structure was
  migrated, deliberately: the node arena and the record table are shared mutable state as
  much as they are data, which is the handle/value distinction N9 recorded, and the integer
  tags that N10 wanted as enums are compared in hundreds of places where an enum has no `==`
  — a cost this item should count against value semantics rather than hide.

**Why still SUPPORTED and not VERIFIED.** What remains is the item's original sharp
question, now larger than ever: **copy elision is still folklore.** A `vec_get` of a record
copies every owning field and a `vec_set` copies them again, and nothing says when either
pair may be removed. The surface model is ergonomic where the language has run it; whether
it stays affordable depends on a specification that does not exist.

**Evidence needed next** · A statement of when redundant retain/release pairs may be elided,
tested against the get-change-set shape, and a second real migration — one where the
elements are updated in place in a loop — measured before and after.

### N12 added typed errors as values, 2026-09-24. Status: still **SUPPORTED, not VERIFIED**.

A failure became a value like any other, and the value model needed nothing new for it:
`Result` and `Option` are ordinary enums, a propagated `Vec[Diag]` is one handle through
every return, and the front end's `Result[Checked, Vec[Diag]]` carries twenty handles in a
record without copying a table. So the evidence this item gains is that **value semantics
absorbed error handling without a second model** — no exception object, no unwinding, no
ownership rule special to failures. What N12 adds to the cost side is small and measured
(`performance.md`): `?` on success is a retain of the payload and a release of the
`Result`, which is the pair copy elision would remove, and the explicit form of an early
return from an arm is awkward in the native subset because an arm cannot be a block there.
Neither changes the status.

*N12.1, 2026-09-24, closed the second of those and adds one observation.* A block used as a
value needed no ownership rule of its own in either native backend: its locals are frame
storage, its tail is borrowed, and whatever takes the value over takes its own reference —
the rule every value already follows. And the `Option` dogfood (`record_owned`) removed four
`>= 0` guards whose only job was to stop a `-1` meaning *none* from comparing equal to a
`-1` meaning *not a record*: a sentinel is a value that participates in comparisons it was
never meant for, and an enum is not. Status unchanged.

### N13 derived equality, 2026-09-25. Status: still **SUPPORTED, not VERIFIED** — and one new finding.

**Derived equality composes; abstraction over it does not.** Structural `==` composed
successfully through every nominal value type whose components already have equality —
records, enums over every variant, concrete generic instances after substitution, and
`Option` and `Result` as the ordinary enums they are — with no declaration, no rule naming
a type, and no new ownership rule: comparing borrows, and a compared temporary is released
once, which is the rule a `Str` comparison already followed. The value model gained a
capability and needed nothing for it.

What did **not** compose is generic abstraction over that capability. After N13 a concrete
`Box[Int]` has equality and `fn same_box[T](a: Box[T], b: Box[T]) -> Bool { a == b }` does
not, nor does `fn same[T](a: T, b: T)`, because nothing can say "`T` has equality" — Nazm has
no constraint or trait mechanism, and checking a generic body once, parametrically, is
exactly what makes the refusal correct. This is the first capability the language derives
for concrete types that a generic definition cannot ask for, and it is the evidence that may
justify a constrained-generics milestone; it does not settle what such a mechanism looks
like, and N13 did not begin one.

The dogfood adds the second observation R1 has been collecting: `analyse.nz`'s completion
states were three integers in an `Ints`, with a fourth meaning "no arm yet", and are an
`enum Completion` in a `Vec[Completion]` and an `Option[Completion]` now, compared with
`==`. Nine historical mutations that compared a completion with `99` could no longer be
written: the invalid state they simulated is not a value of the type. Performance moved
within noise (`performance.md`). Status unchanged.

### N14 let a generic definition require equality, 2026-09-25. Status: still **SUPPORTED, not VERIFIED**.

**One built-in capability requirement solved the abstraction N13 could not express.**
`fn same_box[T: Equality](a: Box[T], b: Box[T])` is valid, and nothing new was needed to make
it so beyond recording which parameters require equality: N13's single derivation, reading a
required parameter as having equality, gives `Box[T]`, `Option[T]` and `Result[T, E]` their
answers under the body's assumptions, and the same derivation checks every argument at the
call. No solver, no implication graph, no dictionary, no runtime cost — an instance at `Int`
is the instructions a hand-written `same_int` is.

**What it did not expose is a need for user-defined traits.** Nothing in N14's
implementation reached for one, and its dogfood audit found no generic helper in the
compiler that needed any capability at all: the compiler's membership searches are over
`Ints` and `Strs`, not `Vec[T]`, and the `Vec`s it has are never searched. So the evidence
says the immediate need was a *built-in* capability requirement, and that a full trait system
is **not yet justified** by any program here — the next one would have to be forced by code
that needs behaviour the language cannot derive (an ordering it has not chosen, a conversion
between error types), not by the symmetry of having one bound.

---

## R2 · Content-addressed semantic IR pays for itself

**Hypothesis** · Normalising each top-level definition after type checking — names erased
to de Bruijn indices, dependencies replaced by their hashes — and keying compilation
results on the resulting Merkle DAG makes incremental compilation and cross-machine caching
correct by construction. `architecture.md` §3 carries the design and the cache-key
correctness conditions.

**Required prerequisite** · A compilation unit and an identity that survives a process.
**Both are met as of N3 (2026-09-22).** A module can be checked from its own source and its
dependencies' interfaces (`crates/nazm-core/tests/unit_checking.rs`); a module has a
`ModuleKey` and a definition a `DefKey` that are equal across processes and across
checkouts; and a module's interface has a deterministic persisted form with a BLAKE3
fingerprint (`crates/nazm-cli/tests/durable_identity.rs`).

**A store keyed on semantic inputs exists as of N4 (2026-09-22), and it is not this.**
`nazm-cache` computes `SourceFingerprint`, `CheckerIdentity` and `CheckKey`, and `nazm
check` reuses the result of checking a module whose inputs are unchanged
(`crates/nazm-cli/tests/incremental.rs`). That answers the *dependency* question — which
modules must be re-checked — and it does so from a module's **source bytes** and its
imports' interfaces.

R2 is about **content addressing**: normalising each definition after type checking so that
two definitions which mean the same thing hash the same, and keying compilation *results*
on the resulting Merkle DAG. None of that exists. There is no normalised form of a
definition and no hash of one; `SourceFingerprint` is deliberately the opposite of one, and
its documentation says so — it normalises nothing, so a re-indent moves it. `InterfaceHash`
is deliberately not a cache key for an artefact, because it excludes bodies. What N4 built
is the *architecture* R2 would slot into, not the hypothesis.

**The one measurement R2 asked for now has a first data point, and it is narrow.** A real
edit to one module of a real graph re-checks that module and reuses the rest; the saving is
in `performance.md`. It is a saving on semantic checking, against a key over source bytes —
not a rebuild saving against a key over normalised definitions, which is what would move
this hypothesis.

**Evidence needed** · A measured rebuild saving on a real edit, against a key over
normalised definitions rather than source bytes, and covering compiler identity, schema
version, transitive dependency hashes, target and CPU features, optimisation flags and ABI.
The three-key separation in `architecture.md` §7.4 says which of those N4 has and which it
deliberately does not.

**N5 narrowed one of those inputs, with evidence.** This item and N4's report both assumed
a dependent's *object* would have to be invalidated by its dependency's implementation.
Measured on the architecture N5 actually built (`architecture.md` §7.5): it does not. An
object names its dependencies in a `declare` and a `call`, so it depends on their symbol
identity and ABI signature, and changing a dependency's body leaves it byte-identical —
there is no cross-module optimisation for a body to reach through. A later key over
normalised definitions would therefore need **fewer** transitive inputs than assumed, not
more. Still nothing implemented: there is no normalised form of a definition, no hash of
one, and no artefact store.

**N6 built an artefact store, and it took the opposite approach on purpose.** There is one
now — objects, keyed and validated (`architecture.md` §7.6) — and its key is not over
normalised definitions and not over source. It is a digest of the **exact bytes handed to
the object compiler**, plus that compiler's identity and the configuration it resolves.

That is worth stating as a result against this hypothesis rather than as progress towards
it. R2's argument is that normalising definitions is how a key becomes both complete and
precise. N6 got completeness and precision at that one stage without normalising anything,
because the boundary it caches at consumes a byte string, and hashing what is actually
handed over cannot omit an input by construction. A dependency's body change and an export
the caller never calls both leave the caller's object reusable — the precision R2 promises —
with no semantic fact in the key at all.

What that does **not** settle: the same trick is unavailable anywhere the input is not a
byte string. Reusing a *semantic check*, or a lowering, or a resolution needs a key over
something structured, and that is still R2's territory. So the hypothesis is narrower than
it was, and more clearly aimed: content addressing has to earn its place above the backend
boundary, because below it the bytes are already the address.

**Invalidating result** · The key must cover so much that hits are rare in practice, or an
omission produces a silent miscompile in testing. `architecture.md` already names the
second as the failure mode: anything omitted from the key is a silent miscompile waiting to
happen. N4's six mutations and N6's six remove one input each and require the suite to catch
it; that is the method the eventual content-addressed key would be held to.

**Status: RESEARCH.** Unchanged. The prerequisites are met and two neighbouring results
exist; the hypothesis is untested, and N6 narrowed where it would have to be tested.

**N12 found the identity rule a persisted-interface design has to state, 2026-09-24.**
An interface carries every type its exported shapes mention, including other modules'.
Rehydrated one at a time, each interface gave those types a fresh identity, so two
interfaces that both mentioned the core `Result` produced two `Result`s — invisible until a
consumer compared them, and fatal to `?` checked from interfaces alone. The fix is the rule
this item's design implies: a definition is identified by its `DefKey` across **every**
interface one compilation reads, so a type met twice is one type
(`PersistentUnitInterface::rehydrate_into`). A hash-keyed design would need the same rule
for the same reason, and would have hidden the defect the same way until something shared a
type. Status unchanged.

---

## R3 · Definition-hash equality is useful to an agent

**Hypothesis** · When an agent rewrites a function and the normalised Core hash is
unchanged, the compiler can tell it the edit produced an identical definition — detecting
one class of no-op edit and shortening a class of agent loop. `architecture.md` §3 states
both the idea's origin (Unison) and the contribution claimed here, which is the
agent-facing use rather than the hashing.

**Required prerequisite** · R2. N3 gives a definition an identity that survives a process,
which is the part an agent needs to be *told* which definition is meant — `DefKey` names
one across runs and across checkouts. It is not the part this hypothesis is about:
`DefKey` says *which* definition, and the claim here is about telling an agent that an edit
produced **the same** definition, which needs a content identity R2 does not have yet.
Nothing here is implemented, and `DefKey` existing is not an agent protocol.

**Evidence needed** · A measured reduction in wasted agent effort, under the protocol in
`docs/research/evaluation.md` — whose thresholds are frozen and may not be relaxed — rather than an
anecdote.

**Invalidating result** · No-op edits turn out to be rare, or agents do not act differently
when told. Note the honest scope already recorded: this detects normalised-Core
alpha-equivalence, **not** all behaviour-preserving rewrites, so a result showing agents
need the general case would invalidate the usefulness without touching the mechanism.

**Status: RESEARCH.**

---

## R4 · Automatic data-layout selection is sound and worth it

**Hypothesis** · Keeping field access symbolic through the IR — `project(place, field_id)`,
with offsets appearing only at the lowest level — lets the compiler choose layouts the
programmer did not, and wins enough to justify the constraint. `architecture.md` §4 is the
design and §7 records the Phase 1 deliverable.

**Required prerequisite** · User-defined types, which do not exist. And the literature
review, which `architecture.md` §4 has called outstanding since it was written — **this
project is not entitled to claim the idea is novel until it is done.**

**Evidence needed** · The eligibility predicate stated, behaviour settled under address
identity, escaping references, FFI, serialisation, atomics and separate compilation, and a
measured win on a benchmark that is not chosen to flatter it.

**Invalidating result** · The eligibility predicate excludes everything interesting, or the
literature shows the win is already available without the constraint.

**Status: RESEARCH.** The architectural commitment that preserves the option — offsets only
at the lowest level — is already honoured and is the hardest thing here to reverse, so
keeping it costs nothing today and buys the experiment later.

### N68 ran a first experiment, 2026-10-02. Status: **SUPPORTED for one layout, not for automatic selection.**

*Prior art, recorded before choosing* (from the literature as known to this project; the
citations were not re-fetched in this session, and no novelty is claimed): structure splitting
and field reordering for cache behaviour — Chilimbi, Davidson and Larus, *Cache-Conscious
Structure Definition*, PLDI 1999; Hundt, Mannarswamy and Chakrabarti, *Practical Structure Layout
Optimization and Advice*, CGO 2006; GCC's `-fipa-struct-reorg` (structure peeling, removed in
GCC 4.8 for lack of a safe eligibility analysis); Lattner and Adve's data-structure analysis for
pool allocation. Language-level and library SoA: ISPC's `soa<N>` types, Zig's
`std.MultiArrayList`, Jai's announced SoA, Halide's separation of an algorithm from its storage
schedule. None of them needs a constraint Nazm does not already have: the *eligibility* is what
the literature finds hard in C (addresses, casts, separate compilation), and in Nazm it is nearly
trivial — a `Vec` element has no address, crosses no FFI, is never serialised, and layout was
already by field name (§7.9). That is the finding, and it is about the language, not the idea.

*What was built and measured* (§7.70): `--layout soa` lays out every `Vec[R]` whose fields are
`Int`s and `Bool`s one array per field. Same results and failures everywhere tested; a one-field
scan over 1,000,000 eight-field records is 2.8× faster and equal to hand-written per-field
`Ints`; a scan reading all eight fields is 2× **slower**. So the layout is a win only by access
pattern, which is the cost model the compiler does not yet have — the invalidating half of the
hypothesis (*automatic* selection wins) is untested, and the option stays opt-in.

---

## R5 · An M:N scheduler with growable stacks beats one OS thread per task

**Hypothesis** · Green tasks over a work-stealing scheduler with growable stacks and an
event reactor give task density and latency that one OS thread per task cannot, without
`async`/`await` colouring the language.

**Required prerequisite** · `nazm-rt`, which needs the workspace `unsafe_code = "forbid"`
to narrow to a per-crate allowance, which needs the memory model. Also a settled answer to
preemption — cooperative at back-edges and calls, or signal-based — which `docs/spec.md`
lists as open.

**Evidence needed** · Spawn-to-first-instruction latency, context-switch cost, and task
memory as **RSS, not virtual reservation, with page size recorded**, against a named
implementation.

**Invalidating result** · No measured advantage at the task counts real programs reach.
The precedent is set and should be followed: `architecture.md` withdrew the "1M live tasks
under 2GB" target as arithmetically unsupported rather than keeping it as aspiration.

**Status: RESEARCH.** The shipped model — one POSIX thread per task, a mutex/condvar ring
per channel — is the semantic baseline and stays so until this is measured. Ownership
prevents data races; **that is not deadlock freedom** and must never be reported as such.

---

## R6 · Provenance and information flow can be checked at a usable approximation

**Hypothesis** · A lattice-valued qualifier on types, propagated position-sensitively,
catches real leaks at a false-positive rate programmers tolerate. `architecture.md` §5
states the design, keeps it a separate concern from effects and capabilities even where
machinery is shared, and records that calling it *"nearly free once effects exist"* was
overclaiming.

**Required prerequisite** · Effects, and a MIR that tracks places.

**Evidence needed** · The chosen approximation stated **with what it misses**, and a
corpus where it finds real leaks without drowning the user. Full non-interference is
usually unusable in practice; the claim has to be about the approximation, not the ideal.

**Invalidating result** · Implicit flows force either an unusable false-positive rate or an
approximation that misses the leaks people actually have.

**Status: RESEARCH.** Every sub-question — implicit flows, sanitizers, aliasing through a
projection, declassification, the FFI boundary — is open in `docs/spec.md`.

---

## R7 · Heterogeneous CPU/GPU/accelerator execution belongs in Nazm

**Hypothesis** · There is a version of accelerator support that fits a small semantic
kernel rather than bolting a second language onto the first.

**Required prerequisite** · A reason to believe it, which does not currently exist.
`architecture.md` §1 says MLIR should be revisited *only if* Nazm pivots toward
tensor/accelerator codegen, and `roadmap.md` places tensors, GPUs, model providers and
distributed execution out of scope and not planned.

**Evidence needed** · A workload the project actually has, that a CPU backend cannot serve.

**Invalidating result** · The honest default. This item exists to be **declined explicitly
rather than drift in through the phrase "AI-native"**, which law 6 defines as
machine-consumable compiler interfaces and not as an argument for syntax or for hardware.

**Status: RESEARCH, and presumed no.**

### N67 tested the narrowest version, 2026-10-02. Status: **SUPPORTED for one map, still RESEARCH in general.**

A kernel as an eligibility over existing functions, not a second language: `nazm accel` runs a
pure `(Int) -> Int` function as an OpenCL kernel on the M1 Pro's GPU with the language's checked
arithmetic and the sequential map's failure carried exactly, about 6× one CPU core on a Collatz
map (§7.69). It fits the small semantic kernel; whether it is worth more than a library call is
what a workload of the project's own would decide, and none exists yet.

---

## R8 · Critical-system verification techniques are reachable from this architecture

**Hypothesis** · Specified-rather-than-inherited arithmetic, refuse-by-name, and a
restriction-profile model are the right foundations for a subset amenable to formal
argument, and the distance to one is smaller than starting over.

**Required prerequisite** · Nearly everything: a memory model, effects, contracts, a
bounded-stack story, and a toolchain qualification argument that the bootstrap explicitly
does **not** supply — `docs/bootstrap.md` puts compiler trustworthiness on the list of
things a reproducible fixpoint does not establish.

**Evidence needed** · A proof obligation discharged on a real Nazm program, and an
assessment against a named standard by someone qualified to make it.

**Invalidating result** · A semantic decision made for ergonomics that turns out to be
irreconcilable with the subset — which is the reason this item is worth tracking now rather
than later, since such a decision is cheap to avoid and expensive to undo.

**Status: RESEARCH.** See the warning paragraph in
[`capability-matrix.md`](capability-matrix.md) under area 19; it applies to every sentence
here.

---

## Where the rest of the open questions live

| Subject | Where |
| --- | --- |
| Copy semantics, closure capture, copy elision, region assignment | `docs/spec.md` *Open* → Memory and values |
| **Is divergence an effect?** — blocks dropping an unused pure call | `docs/spec.md` *Open* → Effects |
| Allocation failure, panic, evaluation order, handler lowering | `docs/spec.md` *Open* → Effects |
| Cancellation semantics, preemption, captured globals under transfer | `docs/spec.md` *Open* → Concurrency |
| Layout eligibility and its interaction with identity, FFI, atomics | `docs/spec.md` *Open* → Layout selection |
| What must be settled before `nazm test --backend=all` means anything | `docs/spec.md` *Open* → Backend semantics |
| Surface syntax, and novelty as a budget | `docs/spec.md` *Open* → Syntax |
| Where the performance headroom is, ranked with confidence labels | `docs/performance.md` |
| Authority: ambient today, capability parameter as the growth path | `docs/spec.md`, and `capability-matrix.md` area 6 |
| Visibility: private-by-default is a known future break | `docs/spec.md` |
