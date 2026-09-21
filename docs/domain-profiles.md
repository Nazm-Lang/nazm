# Domain profiles

**Status: DESIGNED. No profile is implemented, no restriction mechanism exists, and there
is exactly one dialect of Nazm today. This document records the model and the
prerequisites; it records no capability.**

A domain is served by **restricting** the one semantic language and by **granting or
withholding capabilities** — never by forking it. That is law 10 in
[`master-architecture.md`](master-architecture.md), adopted now because the cheap moment to
adopt it is before any of it is built.

The rule that makes it a model rather than a slogan:

> **A program valid under a narrower profile is valid under a wider one and means exactly
> the same thing.** A profile refuses more. It never reinterprets.

A profile may therefore withhold a construct, withhold an authority, or require an
additional obligation. It may **not** change what a construct means, add a keyword, or
alter an arithmetic, evaluation-order or failure rule. The moment a profile needs one of
those, it is a dialect and the answer is no.

---

## Where the six stand

| Profile | Status | The one thing blocking it |
| --- | --- | --- |
| General | PARTIAL | it is the only profile, and it is unnamed |
| Systems | MISSING | no FFI, no ABI, no control over allocation |
| Embedded | MISSING | no cross-compilation, no freestanding target |
| Critical | DESIGNED | everything below is absent, starting with a memory model |
| Cybersecurity | DESIGNED | no capabilities, no information flow |
| AI/HPC | RESEARCH | no vectorisation, no accelerator story, and no decision to pursue one |

Evidence for each "absent" claim is in [`capability-matrix.md`](capability-matrix.md),
areas 4–7, 16–21 and 33. It is not repeated here.

---

## General

**What it is** · What `nazm run` and `nazm build` accept today. Ambient authority, checked
arithmetic that traps, structured concurrency over OS threads, no reclamation.

**Restrictions** · None — it is the widest profile, and every other profile is a
restriction of it.

**Status: PARTIAL.** Real and running. It is called PARTIAL rather than VERIFIED because
the interpreter's subset and the native subset are **not the same**: recursion runs under
`nazm run` and is refused by both compilers. A profile whose two implementations accept
different programs is not yet a profile.

**Missing prerequisite** · That the two subsets coincide, or that the difference is itself
declared as a profile boundary rather than a gap.

---

## Systems

**Intended restrictions** · Explicit control over allocation and layout; no hidden
allocation on a path that declares it allocates nothing; a stated ABI.

**Intended grants** · Raw memory access and foreign calls, inside a boundary that is named
and narrow.

**What exists** · Nothing profile-specific. Native compilation exists and the emitted
runtime uses libc, but that is the compiler's own use, not a user-facing capability.

**Missing prerequisites** · FFI and a settled ABI (area 17, MISSING — no `extern`, no ABI
annotation, and the boundary is listed as an open question rather than a design). A memory
model (area 4). Layout control, which `architecture.md` §4 deliberately keeps symbolic
through the IR so that layout selection stays possible — a commitment that must be
honoured, not undone, when this profile arrives.

**Status: MISSING.**

---

## Embedded

**Intended restrictions** · No allocation after startup, or none at all; bounded stack
usage with a computed bound; no I/O except through declared capabilities; a freestanding
target with no libc.

**What exists** · Nothing. `crates/nazm-cli/src/build.rs` has no `--target`, the emitted
module carries no triple, and `clang` supplies the host's. The runtime declares 18 libc and
pthread symbols and cannot currently be built without them.

**Missing prerequisites** · Cross-compilation (area 33). A freestanding runtime, which
means area 13 and therefore area 4 — a language that never frees cannot run on a device
with kilobytes. Stack bounding: `NAZM_AUTONOMOUS_GOAL.md` already states the trap that
**native frame counts do not bound stack bytes**, which is exactly the obligation this
profile inherits.

**Status: MISSING.**

---

## Critical

**Read this paragraph before any other.** This profile is an architectural intention. It is
**not** a claim that Nazm is suitable for any safety-related, airborne, medical, automotive
or otherwise regulated use; it is not a certification path; and no artefact in this
repository has been produced under a safety standard or assessed against one. The
architecture anticipating a Critical profile is **not evidence for one**, and this document
exists partly to make that sentence impossible to skip.

**Intended restrictions** · A subset with no dynamic allocation, no recursion, bounded
loops with an established bound, total functions, and every failure mode enumerated rather
than trapped. Contracts and invariants checked rather than asserted.

**What exists that is genuinely relevant** · Three things, and they are real. Arithmetic is
**specified rather than inherited from the hardware** — overflow traps and
`crates/nazm-lir/src/emit.rs` refuses `nsw` so that no optimiser may delete the check.
Recursion is already **refused by both compilers**, which is the restriction this profile
would want, arrived at for an unrelated reason. Everything outside a backend's coverage is
**refused by name with a span**, never silently approximated or partially compiled.

**Missing prerequisites** · A memory model; effects; contracts; a bounded-stack story; a
verified or qualified toolchain, which the bootstrap explicitly does **not** provide —
`docs/bootstrap.md` says what a reproducible fixpoint does and does not establish, and
compiler trustworthiness is on the "does not" side.

**Status: DESIGNED.**

---

## Cybersecurity

**Intended restrictions** · Untrusted input tracked to its uses; declassification explicit
and audited; no ambient authority; constant-time obligations where they are declared.

**What exists that is genuinely relevant** · The refuse-by-name discipline, and the fact
that a compiled program's failures are diagnostics rather than undefined behaviour. Against
that: authority is **ambient and labelled as such in the source** —
`crates/nazm-core/src/intrinsic.rs` says nothing in the language restricts which functions
may reach the filesystem.

**Missing prerequisites** · Capabilities (area 6) and information flow (area 7), both
MISSING, and both with every design question still open in `docs/spec.md` — implicit flows,
sanitizer declaration, label propagation, declassification authority, and the FFI label.

**Status: DESIGNED.**

---

## AI/HPC

**Intended grants** · Vectorisation, accelerator offload, layout selection for data-parallel
shapes.

**What exists** · Nothing. No tensors, no GPU path, no vectorisation work, no measurement
of any of it.

**Missing prerequisites** · A reason. `architecture.md` §1 records that MLIR should be
revisited *only if* Nazm pivots toward tensor/accelerator codegen, and nothing proposes that
pivot. `roadmap.md` places tensors, GPUs and model providers out of scope and not planned.
Treating this as a profile rather than a rejected direction is itself the open question.

**Status: RESEARCH.** Tracked in [`research-register.md`](research-register.md).

---

## How a profile becomes real

In order, and the first step is the one that is usually skipped:

1. **Name what it refuses**, as a list of constructs and authorities, in `docs/spec.md`.
2. **Show the refusals are expressible** as restrictions — if one requires changing a
   meaning, it is a dialect and it is refused.
3. **Implement the restriction** where refusals already live, with the existing
   refuse-by-name machinery and a distinct diagnostic code.
4. **State the obligation** the profile takes on, and the evidence class that discharges it.
   For Critical that is not a test count; `NAZM_AUTONOMOUS_GOAL.md` already separates
   source inspection, compilation, runtime behaviour, mutation discrimination, performance
   and platform support as different evidence classes.
5. **Only then** describe it as existing, in [`capability-matrix.md`](capability-matrix.md),
   with a path.
