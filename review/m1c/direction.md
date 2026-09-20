# Decision record: language development is the primary track

**Date:** 2026-09-20 · **Status:** accepted · **Supersedes:** the framing in
`docs/evaluation.md` §1 under which the three-arm experiment decided whether the language
would exist at all.

---

## What changed

Until this record, the repository was organised around a single question: *does a new
language reduce agent cost enough to justify building one?* The benchmark harness came
first deliberately, and `README.md` said in as many words that the answer might be "build
better TypeScript tooling instead."

The founder has decided otherwise. **Nazm is a language project.** The goal is a fast,
memory-efficient, concurrent language suited to AI-native software, with a compiler
eventually written in Nazm itself.

## What this record does *not* change

Listed explicitly, because the easy failure here is to quietly relax a standard while
changing a goal.

| Preserved | Where |
| --- | --- |
| The evaluation protocol, unaltered | `docs/evaluation.md` |
| The decision rule and its thresholds — `point R_C ≤ 0.80` **and** `upper CB < 1.00`, `Δp`, the void-rate policy | `docs/evaluation.md` §6 |
| The three arms, including **A2 as the control** | `docs/evaluation.md` §2 |
| Every historical result and the harness that produced them | `crates/nazm-bench`, `nazm-bench selftest` |
| The protocol's own status: **draft, not pre-registered** | `docs/evaluation.md` top |

No threshold has been moved. No earlier run has been reinterpreted. `nazm-bench run` is
still disabled, and the harness still cannot produce a benchmark number.

## What the evaluation is now for

Not a gate on the project's existence. **Evidence for specific usability and performance
claims** — the ones that would otherwise be asserted without support:

- that agents spend less effort per accepted change in Nazm than in TypeScript with
  comparable tooling;
- that structured diagnostics and a canonical formatter reduce rework, which is the A2
  arm's actual subject and remains worth measuring on its own;
- eventually, the runtime claims in `docs/performance.md`, which need a backend before
  they mean anything.

A2 keeps its role. If improved TypeScript tooling captures most of a measured benefit,
that is a finding about *that claim*, and it stays in the record. It is no longer a stop
condition for the project.

## Self-hosting

**A long-term objective, and explicitly outside the current interpreter milestone.**

Before this record, self-hosting appeared in the repository three times: once in
`docs/spec.md` as a *non-goal for v0.1*, once in `README.md` as a caveat about what a
reproducible bootstrap does not establish, and once in `docs/architecture.md` §8 as a
reason to prefer pure-Rust dependencies. It was never planned.

It is now a stated objective with a staged path in `docs/roadmap.md` (M3). The gap is
large and worth naming rather than minimising: Nazm today has no collections, no
user-defined types, no pattern matching, no string concatenation, and no I/O. A compiler
cannot be written in it in any form, and no amount of backend work changes that. The
roadmap therefore puts the language facilities a compiler needs (M2) before the bootstrap
(M3), and a native vertical slice (M1) before either.

**The Rust implementation is retained until a replacement is justified.** It is the
bootstrap toolchain, the semantic oracle for differential testing, and the thing that
keeps working while the self-hosted compiler does not.

## Four kinds of statement, kept apart

The repository already separates these well; this record names the distinction so it
survives the change of direction.

| Kind | Meaning | Where it lives |
| --- | --- | --- |
| **Implemented behaviour** | Runs today, with a test that fails if it stops | `README.md` "What is actually here"; `docs/spec.md` *Settled for v0.x*; the test suites |
| **Accepted design** | Decided, not built. Changing one is a re-founding decision | `docs/architecture.md`; `docs/spec.md` *Settled* |
| **Open question** | Known to need an answer, answer not chosen | `docs/spec.md` *Open*; `docs/architecture.md` where it says so |
| **Research proposal** | Speculative, and not entitled to be described as a plan | Automatic layout selection (`docs/architecture.md` §4 — "a **proposed research direction**, pending a literature review that has not been done") |

A document, a stub, or a passing test does not by itself move a statement up this table.

## Backend ordering

**Initial decision, 2026-09-20, supported by the adviser: LLVM IR emitted as text and
assembled by `clang` is the first backend.** Verified end to end before adoption — zero
new Rust dependencies, no MSRV exposure, and it is what `architecture.md` §1 already
prescribes for year one.

**This is an ordering decision, not a rejection of Cranelift.** `architecture.md` §1 still
holds: Cranelift is the intended dev-tier backend because one component does four jobs —
JIT, comptime evaluator, REPL, hot reload. None of those jobs exists yet, and a second
backend before then would be a second set of semantics bugs to keep in agreement for no
present benefit. The decision to revisit is Stage 7's.

## Consequences

1. `docs/roadmap.md` is added and is the ordering authority for language work.
2. `docs/architecture.md` §7 and the `Cargo.toml` header, which both said "Phase 2 is
   `nazm-bench` and `xtask` only — there is no compiler yet", are corrected: the Phase 4
   front-end crates exist and a `.nz` program runs.
3. `docs/spec.md` keeps its version history rather than being rewritten. The v0.1 section
   still records that v0.1 had no `return`; a pointer marks where that was superseded.
4. The harness is maintained, not extended, until the claims in M1–M3 need it.
