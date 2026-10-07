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
is the point: a law nobody has checked against the tree is decoration. *Every status below was
checked against the tree again for the v0.3.0 release (R1, 2026-10-07); a status that had
fallen behind was rewritten, with what it used to say kept where the argument needs it.*

**1 — Plain UTF-8 `.nz` files in git are the authoritative source.**
*Honoured.* No derived-source store exists. `architecture.md` §3 gives the reasoning and
the counter-example (Unison) it is drawn from. Content addressing, if it ever arrives,
applies to derived artefacts only.

**2 — Native execution is the primary execution model.**
*Honoured.* `nazm build` produces a native executable with no runtime process, through LLVM
or Cranelift, and the two paths cover the same language: `nazm capabilities` lists every
construct for both, and the only differences are deliberate — a call into C is compiled and not
interpreted, and the iteration budget is the interpreter's. *Until N49 this read "partial, and the
gap is the wrong way round": recursion ran under `nazm run` and both compilers refused it with
`N0101`. Native recursion has a stack check on entry to every function in a call cycle (`N0408`).*

**3 — Safety must not require a mandatory garbage collector.**
*Honoured, and partly earned as of N7 (2026-09-22).* There is no GC and no tracing of any
kind. Sequence storage is reclaimed by counting references to the handle, deterministically
and on every exit edge including the one a failure takes; `docs/spec.md`'s *memory
constitution* is the law and `architecture.md` §7.7 the implementation. A short-lived
sequence workload that grew linearly with work done — 43.52 MiB for a live set of 400 bytes
— is now flat at 1.86.

*Earned for the current type universe as of N8 (2026-09-22).* `Str` and `Chan` storage is
reclaimed too, in both compilers and in the reference interpreter — a `Str` carries the
allocation that owns its bytes (`architecture.md` §7.8), a channel's ring, mutex and
condition variable go with its last reference, and the self-hosted backend emits the same
model. The string accumulator that took the machine down on 2026-09-21 fell from 70.66 MiB
to 2.23 MiB at 8,000 iterations.

**Whole-language leak freedom is still not claimed, and the reason is not modesty.** The
claim covers six built-in types and rests on the fact that none of them can refer to a value
that refers back. The first recursive user-defined type ends that argument, and
`docs/spec.md`'s *Cycles* states in advance what admitting one requires.
`capability-matrix.md` area 4 states the scope per type.

*Until N7 this paragraph read: "There is also no reclamation of any kind … 'No GC' is
currently achieved by never freeing, which is not a memory strategy." That was accurate,
and it is the sentence the milestone was written to retire. Between N7 and N8 it read
"`Str` and `Chan` storage is still not reclaimed, and the self-hosted backend emits no
reclamation at all"; both halves were true and both are now closed.*

**4 — Runtime cost is zero where possible and observable where it is not.**
*Partial.* The costs that exist are observable and specified: checked arithmetic traps
rather than wrapping, and the LLVM translation (`crates/nazm-lir/src/op/llvm.rs`) emits the
`llvm.s*.with.overflow` intrinsics and never `nsw`, precisely because `nsw` would license an
optimiser to delete the check (`crates/nazm-cli/tests/build.rs` holds it). There is no opt-out, so
"zero-cost where possible" has not been tested against a case where it conflicts with
safety.

**5 — Concurrency is part of the language, not a library.**
*Honoured.* `scope`, `spawn` and `Chan` are grammar (`docs/grammar.ebnf`), are checked
(`N0320` outside a scope, `N0321` for a sequence crossing into a task), lower natively,
and are compiled by the compiler written in Nazm. There is no library alternative and no
way to express a task without them.

**6 — AI-native means machine-consumable compiler interfaces, never AI-themed syntax.**
*Honoured.* Twenty-nine versioned schemas under `schema/`, a diagnostic stream with byte-exact
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
are produced and self-validated; an SBOM and signed in-toto statements are available (N94).
Since R1 the records' dates are the commit's rather than the run's (`SOURCE_DATE_EPOCH`), and a
`build-input-digest` covers every file git tracks or would, beside the older source digest over a
declared file set. Whether two assemblies of one commit are byte-identical is a per-release fact,
recorded in that release's record under `docs/releases/`; nothing is claimed across hosts.
`capability-matrix.md` area 32 keeps the claims apart. *Until R1 this said the archive could not be
bit-reproducible while the provenance embedded its own run date, and that the source digest omitted
every Cargo manifest, `Cargo.lock`, `rust-toolchain.toml` and the Dockerfile — both true then.*

**10 — One semantic language, exposed through capability and restriction profiles — never
incompatible dialects.**
*Honoured as built.* Restriction profiles exist — `general`, `embedded`, `critical`, `cyber`,
`realtime`, `authority` and `web3`, chosen with `--profile NAME` or by a package's manifest
(`spec.md` *Restriction profiles*) — and each refuses more and reinterprets nothing; there is
exactly one dialect. §4 states the invariant; `capability-matrix.md` *Domain profiles* states how
far each is verified, and none is a certification. *Until N47 this read "designed only".*

**11 — Claims follow evidence.**
*Honoured in intent, and it failed in practice.* The discipline was cultural, and culture
does not survive a milestone: on 2026-09-21 the front page said the project was "an
interpreter, not a compiler" forty lines after showing it bootstrap itself, and the ledger
called the formatter "planned | nothing exists" thirty lines after marking that milestone
complete. The response is not more prose — it is
[`capability-matrix.md`](capability-matrix.md) with a gate under it.

**12 — The semantic kernel stays small.**
*Honoured, and grown on purpose.* The kernel is the compiler's own answer: `nazm
capabilities` lists 8 built-in types (two of them capabilities), 7 kinds of authority and 53
built-ins, with one module mechanism; four words — `for`, `label`, `loop` and `mod` — are
reserved and refused **by name** rather than silently unavailable
(`crates/nazm-syntax/src/parser.rs`, `RESERVED`). `struct`, `enum`, `match` and `impl` left that
list as each became the construct it reserved. Refusing by name is what makes a small kernel
legible instead of merely incomplete. *It once listed six types, 31 built-ins and eight
reserved words.*

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
> lossless CST rather than on identity. *N15 supplied the lossless CST; the language server
> itself has not been started.*
>
> "Incremental compilation" in that list is worth reading narrowly: a module's semantic
> check may be skipped, and a unit's `clang -c` may be skipped. Lowering, emission and the
> link run on every build. `capability-matrix.md` areas 2a and 10b state exactly what each
> covers.
>
> The paragraph below still holds, and the level it pointed at was built: the
> responsibility it named was owned twice, and now is not.
>
> **2026-09-30 (N39).** "No level between the AST and this one exists" is no longer true,
> and is kept above for the reason the bullets are. Core IR, `crates/nazm-cir`, sits between
> checking and both backends (`architecture.md` §7.41), and it met this section's test: what
> each position of the tree *means* — which slot, which callee, which field, what `?`
> becomes, which loop a `break` leaves — was decided twice, by the evaluator as it ran and
> by `crates/nazm-lir/src/lower.rs` as it lowered, and is now decided once. It is level 3 by
> responsibility, not by §2's shape: a tree of regions, not ANF. HIR, MIR and
> monomorphisation remain absent.
>
> **2026-10-07 (R1).** Two more levels exist since, each owning a responsibility: MIR (N40,
> `capability-matrix.md` area 9), where every copy, move and drop is explicit and validated, and
> an instruction-level LIR both native backends translate (N105, area 10). Generic bodies are
> monomorphised by the native path. HIR is still absent, and the language server has existed since
> N16.

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
`capability-matrix.md` under *Domain profiles*. Today that is seven named profiles (`spec.md` *Restriction
profiles*), each VERIFIED or PARTIAL as scoped there — and the Critical one in particular is
enforcement and evidence that says nothing about certification or fitness for safety-related use.
*Until N47 this read "one profile, unnamed, and five ambitions".*

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
structure is a precondition for those; it is not a smaller version of them. *All four exist
since — `nazm interface` (N3), `nazm lsp` (N16), `nazm context` (N27) and `nazm delta` (N28) —
and the paragraph is kept for the order of the argument.*

---

## 7. What this document deliberately does not decide

The surface memory model (mutable value semantics against a full region calculus in the
IR), whether divergence is an effect, and the eligibility predicate for layout selection.
*The authority model that replaces ambient I/O and whether the scheduler becomes M:N stood here
too; both are settled now, by evidence — authority as a parameter (N37, N104; area 6) and the M:N
pool by default (N83, N106; area 15).* Each has a home — `spec.md` *Open*, `architecture.md` §§4-5, `research-register.md` — and each is
open on purpose. A constitution that settled them would be a design document with
delusions of permanence.

**What it does decide** is that none of them may be described as implemented before the
source proves it, and that the proof lives in a row with a path in it.
