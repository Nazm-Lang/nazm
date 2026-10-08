# Quality model

How a defect in Nazm is classified, and what a classification means for a release. Adopted in Q1,
2026-10-08. The findings of Q1 itself, classified this way, are in `docs/security-qualification.md`.

A finding carries **two independent labels**: how bad it is technically, and what it means for the
programme. They are never collapsed into one score. A HIGH defect in an experimental backend that a
release does not claim can be POST-V1; a LOW defect that makes a stable claim false is not.

## Technical severity

| Severity | Means |
| --- | --- |
| **CRITICAL** | A program the checker accepts runs with a different meaning than `docs/spec.md` gives it, or violates memory safety, a capability, a profile or provenance — silently, on a claimed target and path; or a tool can be made to run code or delete what it does not own |
| **HIGH** | The same, but needing an unusual configuration, or loud rather than silent; or a crash, hang or corruption reachable from crafted input to a stable surface (source, manifest, lockfile, registry index, interface or cache file) |
| **MEDIUM** | A wrong refusal, a wrong diagnostic code or span, a stated guarantee met only on some paths, a stable surface that drifts from its documentation |
| **LOW** | Robustness or hygiene with no wrong result: litter after an interruption, a misleading message, an unscoped sentence in a document |
| **INFO** | Recorded for the next reader; not a defect |

## Programme disposition

| Disposition | Means |
| --- | --- |
| **RELEASE-BLOCKER** | No 1.x release, including a release candidate, while it is open |
| **GATE-BLOCKER** | The current gate is not accepted while it is open; the next gate does not begin |
| **FIX-BEFORE-V1** | Must be fixed before the final v1 release gate; does not stop the current gate |
| **ACCEPTED-LIMITATION** | Stays, stated in `docs/limitations.md` or the capability matrix so no claim contradicts it |
| **POST-V1** | Real, deliberately deferred past v1; recorded so it is not lost |

## Release-blocking rules

These are RELEASE-BLOCKER whatever their technical severity:

1. **A valid-source miscompile:** a program the checker accepts whose result, trap class or output
   differs between the interpreter and a claimed native path (LLVM at any optimisation level,
   Cranelift, the self-hosted compiler) — differential testing's invariant (`docs/architecture.md`).
2. **A stable semantic contradiction:** the checker accepts what `docs/spec.md` refuses, or the
   reverse, on a stable surface.
3. **An accepted-program memory defect** that contradicts the reclamation and safety guarantees
   the specification states for the current type universe.
4. **A stable API or schema break** (`docs/stability.md`): a code reused, a schema field's meaning
   changed, a stable command's exit status changed, without the version law being followed.
5. **A capability, effect, profile or provenance bypass within claimed scope:** what is refused
   directly becomes accepted by hiding it behind another stable mechanism.
6. **A cleanup path that can delete a resource it does not own** — container, network, volume,
   file outside its own scratch space (`docs/runbook.md`).
7. **An unhandled compiler crash** (panic, segfault, unbounded hang) on an audited malformed-input
   class.
8. **A VERIFIED capability-matrix row whose evidence no longer meets its acceptance criteria** and
   which is not downgraded.

## Bug taxonomy

One primary tag per finding: SEMANTIC, MEMORY, CONCURRENCY, BACKEND, RUNTIME, PLATFORM, FFI,
SECURITY, PACKAGE, SCHEMA_API, CACHE_INCREMENTAL, TOOLING, HARNESS, RELEASE, PERFORMANCE,
DOC_DRIFT, SUPPLY_CHAIN, INSTALLER, ROBUSTNESS.

## Discovery origin

Where the defect was found, so the methods that find defects can be told from the ones that do
not: UNIT, MUTATION, FUZZ, PROPERTY, INTEGRATION, GATE_AUDIT, MANUAL, USER_REPORT, POST_RELEASE.

## The broken-feature policy

A capability-matrix row is a claim, and a claim is only as good as its evidence:

claim → a positive test → a negative or refusal test → a mutation the tests kill →
backend and runtime evidence → the public documentation and help → its compatibility status.

When a link is missing, the row says so or moves down (`docs/runbook.md`, *Status vocabulary*): a
gate may **downgrade** a VERIFIED row on evidence, and only evidence moves one up. A feature that
is wired but not exercised is PARTIAL, not VERIFIED; a feature whose tests pass on an empty input
is not tested.

## Every fix

A fix of a classified defect carries, in the same change: a regression test that fails before it,
a focused mutation where one is useful (pristine passes, mutant fails, restored passes —
`docs/runbook.md`, mutation law), the authority documents it changes, and a version bump when the
fix changes what a program means (`docs/stability.md`). A security fix that must refuse a program
which was accepted only because of the defect refuses it with a diagnostic naming the rule
(`SECURITY.md`).
