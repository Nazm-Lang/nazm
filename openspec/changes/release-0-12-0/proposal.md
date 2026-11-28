## Why

0.11.0 (2026-11-28) shipped the toolchain written in Nazm for macOS and Windows x86_64. The
copyright holder asked (2026-11-28) what it would take for Nazm to be the best on every criterion
at once: simple enough for a first language, cheap in tokens for language models, and able to hold
very many connections on a small server, as Erlang does. The evidence in the repository answers
"not yet, and here is where": each item below is a gap that a measurement or a missing study shows,
not a claim. This change proposes 0.12.0 as the release that closes or measures them, beside the
platforms already moved to it.

**This is a draft for the holder's review.** Nothing here is started; the scope, the order and each
target are the holder's to accept, cut or change.

## What Changes

### Already decided for 0.12.0

- **Linux, wasm32 and WASI** come back (moved out of 0.11.0 on 2026-11-05 to ship sooner): the
  Nazm-built toolchain for `x86_64`/`aarch64-unknown-linux-gnu` (ELF from Nazm's own code generator,
  R9 6.3), `wasm32-wasip1`, and a Linux release archive.
- **What 0.11.0 carried:** R8 11.1 and 14.1's open refusals and defects, R1.5 8.1 (a faster linker in
  the contained image) and 8.4 (the mutate baseline kept across `--resume`); Windows TLS; cheaper cold
  builds (ship the shared runtime units precompiled per optimisation level: the 2026-11-10 baseline
  accepted slower cold builds for faster programs); a built-in that gives a program its own path, so
  an installed `nazm` started by a full path finds its stages.

### Proposed: five gaps from the evidence

1. **A million connections on a small server, proved.** Today (`performance-log.md`, the
   concurrency round): spawning 1,000,000 tasks takes 1,501 ms against Go's 271 and Node's 183;
   fan-in of 1,000,000 values from 8 producers is 845 ms against Go's 67, eighth of nine, and the one
   regression of that round; memory per blocked task is 1.2 KiB (Go 2.7 KiB), and the HTTP server's
   9.6 MiB under load is the least of six. Proposed: fix fan-in (the 8-producer, one-channel case
   after the lock-free hand-off) and task start-up; then a published, reproducible run of 1,000,000
   concurrent idle and active connections on a small Linux machine (needs Linux first), with memory,
   latency percentiles and the host named. Target to be set from the first honest run, not before.
2. **Erlang-style reliability: supervised tasks.** Nazm's structured concurrency joins tasks and
   reports failures, but a long-running server has no restart policy. Proposed, spec first
   (`CONTRIBUTING.md`, the constitution before the code): supervisors with one-for-one and
   one-for-all restart, restart intensity limits, and links/monitors over the existing task scopes;
   with a defined interaction with capabilities and cleanup. Deadlock detection on channel waits is a
   candidate in the same design.
3. **A study with real beginners.** The beginner syntax (5b.11–5b.19) and the diagnostics are
   designed for first-time programmers but were never tried on them. Proposed: a small, consented
   study (people who have not programmed before, a fixed set of first tasks), recording where they
   stall and which diagnostics they cannot act on; the findings become diagnostic and guide changes,
   each through its own review. No personal data kept beyond what consent allows.
4. **A token-cost benchmark for language-model agents.** `docs/internal/research/evaluation.md`
   designs the comparison (same budget pinned in dollars, tokens, turns and wall-clock; repeated runs
   not treated as independent) but has no driver. Proposed: the driver, and a first run of the same
   tasks in Nazm, Python and TypeScript, reporting tokens to a correct, tested solution and the
   failure modes. Paid model use needs the holder's budget decision before any run.
5. **A small-object allocator.** binary-trees (depth 18) is the largest single gap measured: after
   the box and free-list fixes, 1.7× C's time and 5.5× its memory (1,971.7 ms, 98.3 MiB; C 1,158 ms,
   17.8 MiB), thirteenth of sixteen. What remains is the box's two-block representation and the
   retain/release pairs of a MIR without moves (performance-leadership 8.2). Proposed: a one-block
   box and a size-class allocator in the runtime, then move-aware MIR to drop the redundant pairs;
   measured in a quiet window against C, Rust and Go.

### Suggested order

Linux first (items 1 and the platform scope need it), then 5 and the fan-in half of 1 (runtime work,
measurable on macOS now), 2 (spec, then runtime), 4 (driver now; runs after the budget decision),
and 3 alongside, since it needs people rather than machine time.

## Impact

Runtime (allocator, scheduler, channels), the language specification (supervision: new constructs or
library, with new diagnostic codes and an epoch decision), the code generator and release scripts for
Linux and WASI, the benchmark harness, and a research protocol for the study. Each item reaches main
through its own change and the usual gates; the release gate is the 0.11.0 gate plus Linux.

## Decisions for the holder

| | Question |
| --- | --- |
| (a) | Which of the five go into 0.12.0, and which wait for 1.0 |
| (b) | Supervision as language constructs or as a standard-library module over task scopes |
| (c) | The machine for the million-connection run (a small VPS, its size) |
| (d) | A budget for the token benchmark's model calls |
| (e) | Whether the beginner study recruits through people the holder knows, and the consent form's wording |
