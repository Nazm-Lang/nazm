# Master architecture

**Status: the durable constitution. Every other document is dated; this one states what
must stay true across all of them.**

This document does not describe the compiler. [`architecture.md`](architecture.md) does
that, and does it well — the IR levels (§2), the layout-selection boundary (§4), mutable
value semantics with a region hedge and the three-way split of effects, capabilities and
provenance (§5). None of that is restated here. What did not exist anywhere until now is
the layer above: the invariants that outrank any particular design, the vocabulary claims
must be made in, and the model under which one language serves several domains.

It is also not a plan and not an ambition. [`roadmap.md`](roadmap.md) is the ordering
authority. [`NAZM_LANGUAGE_GOALS.md`](NAZM_LANGUAGE_GOALS.md) is where Nazm is ultimately
going and why — **this file is what must stay true while it gets there.** The two are
separate on purpose: the Goals document is deliberately broad and changes rarely; this one
has to stay cheap enough to load on every task.

Where this document and a dated one disagree about *what is true today*, the evidence in
[`capability-matrix.md`](capability-matrix.md) decides — not this file. And nothing in the
Goals document is evidence for anything: **a goal appearing there never moves a capability
row.** That file says so itself, in its §0.

---

## 1. How a claim is allowed to be stated

The repository has separated evidence classes since the 2026-09-20 decision record, which
named four kinds of statement and where each lives. **That record was retired on 2026-09-21
and its vocabulary lives here**, at the granularity a matrix row needs:

| Word | Means | The original four kinds |
| --- | --- | --- |
| **VERIFIED** | Executed evidence against the current tree, and a named artefact that fails if it stops being true | Implemented behaviour |
| **PARTIAL** | Real and running, but narrower than the name suggests. The row says exactly how much narrower | Implemented behaviour, scoped |
| **DESIGNED** | Decided and written down; no implementation. Changing it is a re-founding decision | Accepted design |
| **RESEARCH** | A hypothesis with a falsifier. Not entitled to be called a plan | Research proposal |
| **BLOCKED** | Wanted, specified enough to start, and stopped by a named dependency | Open question, with the blocker named |
| **MISSING** | Absent, and nothing here commits to adding it | — |

Three rules govern their use, and the first is the one this project has actually broken:

1. **A document, a stub, a crate name or a passing test does not move a row up this
   table.** The 2026-09-20 record said this before any of it was built. It was still violated:
   `crates/nazm-lir` exists and matches the *name* of `architecture.md` §2's level 7, and
   four documents concluded from the name that the level existed. See §3.
2. **VERIFIED cites a path.** A row without one is PARTIAL at best. `cargo xtask check`
   now enforces this — see `xtask/src/main.rs`, `check_doc_claims`.
3. **Absence of a refutation is not evidence.** A quiet period is observation. A test that
   self-skips has established nothing; `crates/nazm-bench/tests/docker_boundary.rs` skips
   without `NAZM_REQUIRE_DOCKER=1` and says so rather than counting itself as coverage.
4. **A count is not a proof.** Source inspection, a successful compile, runtime behaviour,
   mutation discrimination, performance and platform support are **different evidence
   classes**, and one never substitutes for another. Test counts are not capability proofs.
   LLVM validation catches malformed IR, not semantic miscompilation — which is why every
   native case is checked against an explicit expected outcome rather than against exit
   status. **passed**, **skipped**, **refused**, **failed** and **unexecuted** are five
   outcomes and are never reported as one another.

---

## 2. The twelve laws

Each law is followed by where the repository actually stands against it. That second line
is the point: a law nobody has checked against the tree is decoration.

**1 — Plain UTF-8 `.nz` files in git are the authoritative source.**
*Honoured.* No derived-source store exists. `architecture.md` §3 gives the reasoning and
the counter-example (Unison) it is drawn from. Content addressing, if it ever arrives,
applies to derived artefacts only.

**2 — Native execution is the primary execution model.**
*Partial, and the gap is the wrong way round.* `nazm build` produces a native executable
with no runtime process. But the interpreter's subset is **larger**: recursion runs under
`nazm run` and is refused by both compilers with `N0101`
(`crates/nazm-lir/src/lower.rs`, `compiler/emit.nz`). Until the two subsets coincide, the
native path is primary by intent and second by coverage.

**3 — Safety must not require a mandatory garbage collector.**
*Honoured, and partly earned as of N7 (2026-09-22).* There is no GC and no tracing of any
kind. Sequence storage is reclaimed by counting references to the handle, deterministically
and on every exit edge including the one a failure takes; `docs/spec.md`'s *memory
constitution* is the law and `architecture.md` §7.7 the implementation. A short-lived
sequence workload that grew linearly with work done — 43.52 MiB for a live set of 400 bytes
— is now flat at 1.86.

Partly, and the part that is missing is named rather than rounded off. `Str` and `Chan`
storage is still not reclaimed, and the self-hosted backend emits no reclamation at all, so
a string-heavy program still grows. **Whole-language leak freedom is not claimed**, and
`capability-matrix.md` area 4 states the scope per type.

*Until N7 this paragraph read: "There is also no reclamation of any kind … 'No GC' is
currently achieved by never freeing, which is not a memory strategy." That was accurate,
and it is the sentence the milestone was written to retire.*

**4 — Runtime cost is zero where possible and observable where it is not.**
*Partial.* The costs that exist are observable and specified: checked arithmetic traps
rather than wrapping, and `crates/nazm-lir/src/emit.rs:1-30` refuses `nsw` precisely
because it would license an optimiser to delete the check. There is no opt-out, so
"zero-cost where possible" has not been tested against a case where it conflicts with
safety.

**5 — Concurrency is part of the language, not a library.**
*Honoured.* `scope`, `spawn` and `Chan` are grammar (`docs/grammar.ebnf`), are checked
(`N0320` outside a scope, `N0321` for a sequence crossing into a task), lower natively,
and are compiled by the compiler written in Nazm. There is no library alternative and no
way to express a task without them.

**6 — AI-native means machine-consumable compiler interfaces, never AI-themed syntax.**
*Honoured.* Four versioned schemas under `schema/`, a diagnostic stream with byte-exact
spans, a capability inventory read from the compiler's own tables. No keyword anywhere
exists to support the label, and `roadmap.md` forbids adding one.

**7 — A successful compilation is not correctness evidence.**
*Honoured, and structurally.* `crates/nazm-cli/tests/build.rs:6-25` states the two-oracle
rule — every native case is checked against a written expectation *and* against the
interpreter, at `-O0` and `-O2`. The mutation catalogue exists to test the tests.
The sharp version, stated in §1 rule 4: LLVM validation catches malformed
IR, not semantic miscompilation.

**8 — Unsafe code stays narrowly isolated and explicitly justified.**
*Honoured, more strictly than the law requires.* `Cargo.toml:97` is
`unsafe_code = "forbid"` workspace-wide — not `deny`, not per-crate. This already shapes
design rather than merely constraining it: the test harness bounds a child's address space
through a shell wrapper (`crates/nazm-cli/tests/common/mod.rs`) because `pre_exec` is
unavailable. When `nazm-rt` eventually needs it, the allowance narrows to that crate and
`xtask check` enforces the narrowness (`architecture.md:317-319`).

**9 — Reproducibility and provenance are first-class.**
*Partial, and honestly scoped.* A bootstrap record, `PROVENANCE.txt` and `MANIFEST.txt`
are produced and self-validated. What is **not** established: the source digest covers a
declared file set rather than the tree — omitting every Cargo manifest, `Cargo.lock`,
`rust-toolchain.toml` and the Dockerfile — and the archive is not bit-reproducible and
cannot be while the provenance embeds its own run date. The release report states the
three-way split precisely; it is not restated here.

**10 — One semantic language, exposed through capability and restriction profiles — never
incompatible dialects.**
*Designed only.* No profile is implemented, no restriction mechanism exists, and there is
exactly one dialect. §4 states the invariant; `capability-matrix.md` states what exists.

**11 — Claims follow evidence.**
*Honoured in intent, and it failed in practice.* The discipline was cultural, and culture
does not survive a milestone: on 2026-09-21 the front page said the project was "an
interpreter, not a compiler" forty lines after showing it bootstrap itself, and the ledger
called the formatter "planned | nothing exists" thirty lines after marking that milestone
complete. The response is not more prose — it is
[`capability-matrix.md`](capability-matrix.md) with a gate under it.

**12 — The semantic kernel stays small.**
*Honoured.* Six types, 31 built-ins, one module mechanism, and eight reserved words —
`for`, `label`, `loop`, `struct`, `enum`, `match`, `mod`, `impl` — refused **by name**
rather than silently unavailable (`crates/nazm-syntax/src/parser.rs:18-36`). Refusing by
name is what makes a small kernel legible instead of merely incomplete.

### Two invariants that are easy to lose in a refactor

*Absorbed 2026-09-21 from the Era-1 execution contract's §5, which had no other home.*

**A refusal is never disguised as a result.** No dummy value, silent fallback type,
poison-producing arithmetic shortcut, or unreachable terminator may stand in for something
the backend does not support or lowers incorrectly. Everything outside a subset is refused
**by name, with a span**, before anything is written — a refusal that arrives after the
output is on stdout is a refusal nobody can act on. This is law 7 read from the
implementation side: a program that compiles because the compiler invented a value has not
been compiled.

**Recursive work over the tree stays on a stack whose size is known.** Parsing, checking,
lowering and *dropping* an AST all recurse with the shape of the input, so they run on the
interpreter's own sized stack rather than the caller's — including teardown, because a
deeply nested tree can overflow while being freed. Returning a recursive owned structure to
an inadequately sized caller stack reintroduces the bound it was written to remove. Note
what does **not** follow from this: an AST nesting limit bounds nesting, not call-graph
depth, module depth or runtime call depth; and **native frame counts do not bound stack
bytes.**

---

## 3. What the IR diagram means, and what exists

`architecture.md` §2 gives eight levels, source through machine code. Exactly **one**
crate below the AST exists, and the relationship between the two needs stating plainly,
because four documents got it wrong by reading the crate list as a progress bar.

`crates/nazm-lir` is named for level 7 and `crates/nazm-lir/src/lib.rs:1-27` says it is
"level 7 and nothing else". Its *content* is not level 7 as §2 defines it. §2 describes
level 7 as SSA with concrete layouts over a CFG of places; what `crates/nazm-lir/src/ir.rs`
defines is a **structured, typed tree over numbered slots** — `While`, `If` and `Scope`
nest, `Local`s are indices, and nothing is in SSA form. It is a real IR that owns a real
responsibility: it records *which operations can fail and where the source said so*, which
the AST does not carry and the backend cannot recover.

So the honest statement is: **the diagram is a target, the crate shares its name, and no
level between the AST and this one exists.** Neither HIR, nor a typed core, nor MIR, nor
monomorphisation — and the last of those is not a gap, because nothing is polymorphic.

Two consequences worth naming, because both are dependencies rather than complaints:

- **Name resolution happens twice**, independently — once in `crates/nazm-core/src/check.rs`
  and again in `crates/nazm-lir/src/lower.rs`, against two separately transcribed built-in
  tables of 31 entries each (`crates/nazm-core/src/intrinsic.rs`,
  `crates/nazm-lir/src/ir.rs`) kept in agreement by a test rather than by construction.
  That is one semantic responsibility with two implementations.
- **A span has no file identity.** `architecture.md:307` credits `nazm-span` with a
  `FileId`; no such type exists. `crates/nazm-cli/src/load.rs` concatenates every reachable
  file into a single buffer and recovers the filename from the offset. That is a sound
  design for a whole-program compiler and a hard stop for separate compilation, incremental
  compilation, content-addressed definitions and any language server.

> **Both bullets were closed between 2026-09-21 and 2026-09-22, and are kept above because
> the argument they make is the reason the work happened.**
>
> N1 ended the double resolution: `crates/nazm-core/src/check.rs` produces one
> `nazm_sema::Resolution` and both the evaluator and `crates/nazm-lir/src/lower.rs` read
> it, with a `cargo xtask check` rule failing if the backend declares a name-keyed table.
> N1 also gave `nazm_span::Span` a `FileId` and replaced the merged buffer with a
> `SourceMap`. N2 added a compilation unit and per-unit checking, N3 durable identity and a
> persisted interface, **N4 incremental semantic checking**, **N5 separate code generation
> and linking**, and **N6 native object reuse** — so of the four things the second bullet
> called blocked, three now exist and the fourth, a language server, is blocked on a
> lossless CST rather than on identity.
>
> "Incremental compilation" in that list is worth reading narrowly: a module's semantic
> check may be skipped, and a unit's `clang -c` may be skipped. Lowering, emission and the
> link run on every build. `capability-matrix.md` areas 2a and 10b state exactly what each
> covers.
>
> The paragraph below still holds, and the level it pointed at was built: the
> responsibility it named was owned twice, and now is not.

**An IR level earns its existence by owning a semantic responsibility no existing level
owns.** Matching the diagram is not such a reason. By that test, the next level is the one
that would end the double resolution — and that is an argument from the tree, not from §2.

---

## 4. One language, many profiles

A domain is served by **restricting** the one semantic language and by **granting or
withholding capabilities** — never by forking the semantics. That is law 10, and this is
what it means precisely:

> **A program valid under a narrower profile is valid under a wider one and means exactly
> the same thing.** A profile refuses more. It never reinterprets.

A profile may withhold a construct, withhold an authority, or impose an additional
obligation. It may **not** change what a construct means, add a keyword, or alter an
arithmetic, evaluation-order or failure rule. The moment a profile needs one of those it is
a dialect, and the answer is no.

This is adopted before anything implements it, because that is the cheap moment to adopt
it. **A profile becomes real in this order**, and the first step is the one usually skipped:

1. **Name what it refuses** — constructs and authorities — in `spec.md`.
2. **Show the refusals are expressible as restrictions.** If one requires changing a
   meaning, it is a dialect and it is refused.
3. **Implement the restriction** where refusals already live, using the existing
   refuse-by-name machinery and a distinct diagnostic code.
4. **State the obligation** the profile takes on and the evidence class that discharges it.
   Source inspection, successful compilation, runtime behaviour, mutation discrimination,
   performance and platform support are different evidence classes and one does not
   substitute for another.
5. **Only then** describe it as existing, in `capability-matrix.md`, with a path.

What each profile is *for* is `NAZM_LANGUAGE_GOALS.md` §10. What exists today is
`capability-matrix.md` under *Domain profiles*. Today that is one profile, unnamed, and
five ambitions — and the Critical one in particular is an architectural intention that says
nothing about certification or fitness for safety-related use.

## 5. Documentation authority

**Every normative statement has exactly one owner.** Another document may summarise it in a
sentence and link; no document may restate it at length. When two files disagree, the owner
wins — and the other one is the bug.

| Authority | Owns | Changes |
| --- | --- | --- |
| [`NAZM_LANGUAGE_GOALS.md`](NAZM_LANGUAGE_GOALS.md) | Why Nazm exists · historical barriers · ultimate objectives · domain ambitions · the token-efficiency constitution · non-goals | **rarely** |
| **this file** | Durable engineering law · the evidence vocabulary · the one-language/many-profiles invariant · context routing | rarely |
| [`architecture.md`](architecture.md) | Compiler architecture · IR responsibilities · backend strategy · the layout boundary · effects/capabilities/provenance separation | as design evolves |
| [`spec.md`](spec.md) | **What a Nazm program means.** Settled strata, versioned; *Open* is the register of what is not | per language change |
| [`capability-matrix.md`](capability-matrix.md) | **What exists today**, with evidence — the only status authority | per implementation change |
| [`roadmap.md`](roadmap.md) | What comes next, and why in that order | per milestone |
| [`research-register.md`](research-register.md) | Hypotheses, each with a falsifier. Deliberately not commitments | per hypothesis |
| [`bootstrap.md`](bootstrap.md) | Self-hosting: its design, its result, and what it does **not** establish | rarely |
| [`diagnostics.md`](diagnostics.md) | The machine interface: schemas, spans, fixes, exit statuses, the compatibility contract | per schema version |
| [`performance.md`](performance.md) | Runtime and compiler performance claims, and the measurements under them | per measurement |
| [`runbook.md`](runbook.md) | **Containment, resource safety, recovery.** Read before anything heavy | per incident |
| [`guide.md`](guide.md) + [`grammar.ebnf`](grammar.ebnf) | The human guide, and the machine-checked canonical grammar it is generated from | per syntax change |
| `releases/` | Frozen evidence for one tested commit each. Never updated after the fact | per release |
| `research/` | The comparative agent-cost protocol, with frozen thresholds | rarely |

Three separations are deliberate and must not be collapsed. **Goals answer where we are
ultimately going; this file answers what must stay true while we get there.** **`spec.md`
says what a program means; `architecture.md` says how the compiler is built** — semantics
and implementation are different authorities. **`performance.md` measures the language;
`research/evaluation.md` measures agent development efficiency** — two experiments, never
one score.

Documents retired on 2026-09-21, their obligations rehomed and their text kept by Git:
`direction.md` (the 2026-09-20 decision record), `NAZM_AUTONOMOUS_GOAL.md` (the completed
Era-1 execution contract), `goal-status.md` (its ledger), `domain-profiles.md` and
`bootstrap-design.md`.

---

## 6. Context routing

**Load the minimum sufficient authoritative context, not the whole docs directory.** This
is `NAZM_LANGUAGE_GOALS.md`'s token-efficiency constitution applied to this repository: a
reader who has to ingest everything to change anything is paying for the documentation's
disorganisation.

| Task | Read |
| --- | --- |
| **Foundational language decision** — a principle changes | `NAZM_LANGUAGE_GOALS.md` · this file · the relevant `architecture.md` section · the relevant `spec.md` section · `capability-matrix.md` · the relevant research item |
| **Ordinary semantic change** | the relevant `spec.md` section · `capability-matrix.md` · `roadmap.md` · the relevant `architecture.md` section. **Not** the whole Goals document |
| **Compiler implementation change** | this file · the relevant `architecture.md` section · the relevant `spec.md` section · `capability-matrix.md` · `roadmap.md` |
| **Bootstrap or self-hosting** | `bootstrap.md` · the relevant `spec.md` section · `runbook.md` · the capability row |
| **Performance** | `performance.md` · `runbook.md` · the capability row |
| **AI tooling or diagnostics** | `diagnostics.md` · the Goals token-efficiency sections (§6) · the capability row. `research/evaluation.md` **only** when running a comparative agent evaluation |
| **Anything heavy — generated stages, selfhost, bootstrap, mutation, benchmarks** | **`runbook.md`, always, first** |
| **"Is X implemented?"** | `capability-matrix.md`. For a construct or built-in, ask `nazm capabilities --json` instead — it is read from the compiler's own tables and cannot drift |

Two rules that save more context than any routing table. **Ask the compiler before asking a
document**: `nazm capabilities --json` answers construct-level questions from the
implementation. **A document's own status line is not evidence**; `capability-matrix.md`
cites paths, and `cargo xtask check` fails when one of them stops existing.

### The same rule, applied to source — the structural half, since N2

Routing documents is the half this repository can do by writing things down. The half that
needs the language is routing **source**, and N2 built its prerequisite rather than its
mechanism: a module is now private by default, exports an explicit interface, and does not
pass its dependencies on to its importers. So "what does a reader of module A need" has an
answer that is not "everything reachable" — A's source, and the *interfaces* of what A
directly imports.

`nazm_core::check_unit` takes exactly that and no more, which is the property being relied
on rather than a description of an intention. What does **not** exist is anything that
hands an agent that subset: no context packet, no semantic delta, no language server, and
no interface that can be written to a file. `capability-matrix.md` area 2 says so. The
structure is a precondition for those; it is not a smaller version of them.

---

## 7. What this document deliberately does not decide

The surface memory model (mutable value semantics against a full region calculus in the
IR), whether divergence is an effect, the eligibility predicate for layout selection, the
authority model that replaces ambient I/O, and whether the scheduler becomes M:N. Each has
a home — `spec.md` *Open*, `architecture.md` §§4-5, `research-register.md` — and each is
open on purpose. A constitution that settled them would be a design document with
delusions of permanence.

**What it does decide** is that none of them may be described as implemented before the
source proves it, and that the proof lives in a row with a path in it.
