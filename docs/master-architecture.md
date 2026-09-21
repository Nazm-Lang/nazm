# Master architecture

**Status: the durable constitution. Every other document is dated; this one states what
must stay true across all of them.**

This document does not describe the compiler. [`architecture.md`](architecture.md) does
that, and does it well — the IR levels (§2), the layout-selection boundary (§4), mutable
value semantics with a region hedge and the three-way split of effects, capabilities and
provenance (§5). None of that is restated here. What did not exist anywhere until now is
the layer above: the invariants that outrank any particular design, the vocabulary claims
must be made in, and the model under which one language serves several domains.

It is also not a plan. [`roadmap.md`](roadmap.md) is the ordering authority, and
[`direction.md`](direction.md) is the standing decision record. Where this document and a
dated one disagree about *what is true today*, the evidence in
[`capability-matrix.md`](capability-matrix.md) decides — not this file.

---

## 1. How a claim is allowed to be stated

The repository has separated evidence classes since `direction.md:72-84`, which names four
kinds of statement and where each lives. That table stands. This is the same distinction
at the granularity a matrix row needs, and the two map onto each other:

| Word | Means | `direction.md` kind |
| --- | --- | --- |
| **VERIFIED** | Executed evidence against the current tree, and a named artefact that fails if it stops being true | Implemented behaviour |
| **PARTIAL** | Real and running, but narrower than the name suggests. The row says exactly how much narrower | Implemented behaviour, scoped |
| **DESIGNED** | Decided and written down; no implementation. Changing it is a re-founding decision | Accepted design |
| **RESEARCH** | A hypothesis with a falsifier. Not entitled to be called a plan | Research proposal |
| **BLOCKED** | Wanted, specified enough to start, and stopped by a named dependency | Open question, with the blocker named |
| **MISSING** | Absent, and nothing here commits to adding it | — |

Three rules govern their use, and the first is the one this project has actually broken:

1. **A document, a stub, a crate name or a passing test does not move a row up this
   table.** `direction.md:84` said this before any of it was built. It was still violated:
   `crates/nazm-lir` exists and matches the *name* of `architecture.md` §2's level 7, and
   four documents concluded from the name that the level existed. See §3.
2. **VERIFIED cites a path.** A row without one is PARTIAL at best. `cargo xtask check`
   now enforces this — see `xtask/src/main.rs`, `check_doc_claims`.
3. **Absence of a refutation is not evidence.** A quiet period is observation. A test that
   self-skips has established nothing; `crates/nazm-bench/tests/docker_boundary.rs` skips
   without `NAZM_REQUIRE_DOCKER=1` and says so rather than counting itself as coverage.

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
*Honoured, but not yet earned.* There is no GC. There is also no reclamation of any kind:
`@free` appears **zero** times in either emitter, against 11 `@malloc` and 2 `@realloc`
sites in each. "No GC" is currently achieved by never freeing, which is not a memory
strategy. `crates/nazm-core/src/eval.rs:47-48` says so in the source: *"emphatically not a
claim of ownership safety. Nothing prevents aliasing, nothing tracks lifetimes, and
nothing is ever freed."* The law is intact; the obligation behind it is unmet.

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
exists to support the label, and `roadmap.md:931` forbids adding one.

**7 — A successful compilation is not correctness evidence.**
*Honoured, and structurally.* `crates/nazm-cli/tests/build.rs:6-25` states the two-oracle
rule — every native case is checked against a written expectation *and* against the
interpreter, at `-O0` and `-O2`. The mutation catalogue exists to test the tests.
`NAZM_AUTONOMOUS_GOAL.md:342` states the sharp version: LLVM validation catches malformed
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
cannot be while the provenance embeds its own run date. `goal-status.md` and
`delivery.md` state the three-way split precisely; it is not restated here.

**10 — One semantic language, exposed through capability and restriction profiles — never
incompatible dialects.**
*Designed only.* No profile is implemented, no restriction mechanism exists, and there is
exactly one dialect. See [`domain-profiles.md`](domain-profiles.md).

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

**An IR level earns its existence by owning a semantic responsibility no existing level
owns.** Matching the diagram is not such a reason. By that test, the next level is the one
that would end the double resolution — and that is an argument from the tree, not from §2.

---

## 4. One language, many profiles

A domain is served by **restricting** the one semantic language and by **granting or
withholding capabilities** — never by forking the semantics. A program valid under a
narrower profile is valid under a wider one and means the same thing; the narrow profile
refuses more, it does not reinterpret.

This is a rule about how to grow, and it is adopted **before** anything implements it
precisely because the cheap moment to adopt it is now. Nothing in the tree today is a
profile. [`domain-profiles.md`](domain-profiles.md) carries the six and what each would
need; every one but General is DESIGNED or RESEARCH, and the Critical profile in
particular is an architectural intention and **not** a statement about airworthiness,
certification or fitness for any safety-related use.

---

## 5. Where everything else lives

| Question | Document |
| --- | --- |
| Why this compiler design; IR levels; type system; layout boundary; toolchain choices | [`architecture.md`](architecture.md) |
| What the language means, per construct, with the decision behind it | [`spec.md`](spec.md) — *Settled* strata, versioned |
| What is undecided | [`spec.md`](spec.md) *Open*, and [`research-register.md`](research-register.md) |
| What order work happens in | [`roadmap.md`](roadmap.md) |
| Why the language, and not the benchmark, is the primary track | [`direction.md`](direction.md) |
| What exists today, with evidence | [`capability-matrix.md`](capability-matrix.md) |
| Domain profiles and their prerequisites | [`domain-profiles.md`](domain-profiles.md) |
| The machine interface: schemas, spans, fixes, exit statuses | [`diagnostics.md`](diagnostics.md) |
| What self-hosting does and does not establish | [`bootstrap.md`](bootstrap.md) |
| Release evidence, limitations, reproduction | [`delivery.md`](delivery.md) |
| Performance claims and the measurements under them | [`performance.md`](performance.md) |
| The execution contract the first era ran under | [`NAZM_AUTONOMOUS_GOAL.md`](NAZM_AUTONOMOUS_GOAL.md) |
| The running ledger and open items | [`goal-status.md`](goal-status.md) |
| Operating environment and recovery procedures | [`runbook.md`](runbook.md) |

---

## 6. What this document deliberately does not decide

The surface memory model (mutable value semantics against a full region calculus in the
IR), whether divergence is an effect, the eligibility predicate for layout selection, the
authority model that replaces ambient I/O, and whether the scheduler becomes M:N. Each has
a home — `spec.md` *Open*, `architecture.md` §§4-5, `research-register.md` — and each is
open on purpose. A constitution that settled them would be a design document with
delusions of permanence.

**What it does decide** is that none of them may be described as implemented before the
source proves it, and that the proof lives in a row with a path in it.
