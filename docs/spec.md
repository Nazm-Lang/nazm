# Nazm language specification

**Status: v0.1 in progress (Phase 1). This is not yet a specification — it is the list of
decisions a specification has to make, with the ones already settled marked as such.**

Publishing the open questions before the answers is deliberate. Every one of them was
either wrong or unstated in an earlier draft of the design, and the cost of discovering
that after 150k lines of compiler is the entire reason this document exists first.

---

## Settled

These are design commitments. Changing one is a re-founding decision, not an edit.

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

## Open — Phase 1 must settle these

### Memory and values

- Exact copy semantics: when is a value copied, when projected, when moved?
- Closure capture: which conventions may a closure capture under, and what escapes?
- Copy elision: what does the compiler *guarantee*, versus what does it merely usually do?
  MVS is unusable if this is folklore.
- Arena/region assignment rules, and how region inference interacts with the exclusivity
  check when they are (by design) the same solve.

### Effects

- **Is divergence an effect?** The `forever()` counterexample in `architecture.md` §5 shows
  that without an answer, "drop an unused pure call" is unsound. Options: treat divergence
  as an effect; track termination separately (`willreturn`-style); or refuse the
  transformation entirely.
- Is allocation failure an effect, a panic, or a return value?
- Is panic an effect, an unwind, or both, and what is observable about it?
- Evaluation order: specified, or deliberately unspecified? If unspecified, differential
  testing across backends cannot compare it (`architecture.md` §6).
- Which handlers lower statically, and which require one-shot continuation capture?

### Provenance and information flow

- Implicit flows: is branching on a `Secret` a leak? Full non-interference is usually
  unusable in practice; what is the chosen approximation, and what does it miss?
- Sanitizer functions: how are they declared, and what stops a no-op being declared one?
- Aliasing: how does a label propagate through a projection?
- Declassification: which capability, granted where, audited how?
- The FFI boundary: what label does a value returned from C carry?

### Concurrency

- Captured variables, globals/statics, and foreign pointers are **not** covered by channel
  ownership transfer. Each needs a rule.
- Preemption: cooperative at back-edges and calls, or signal-based?
- What exactly does cancellation cancel, and what cleanup is guaranteed to run?

### Layout selection

- The eligibility predicate: which types may the compiler re-lay-out?
- Behaviour under address identity, escaping references, FFI, serialization, atomics, and
  separate compilation (`architecture.md` §4).

### Backend semantics

Required before `nazm test --backend=all` means anything:

- integer overflow behaviour
- floating-point semantics (fast-math? reassociation? NaN payloads?)
- panic and unwind behaviour
- what counts as an observable output

### Syntax

- The surface syntax itself, which is the **largest lever against the training-data risk**
  and is nearly free to change now and effectively unchangeable later. Novelty is a budget:
  spend it only where it buys semantics unobtainable otherwise.
- A machine-readable grammar (needed as a spec artifact regardless of whether
  grammar-constrained decoding is ever used).
- A cheatsheet under ~3k tokens, to be injected by `nazm init`.

## Non-goals for v0.1

Deliberately out of scope until the gate:

macros · comptime beyond constant folding · general effect handlers · whole-program taint ·
layout selection · self-hosting · a package manager · a module system beyond files
