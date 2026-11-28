## Why

0.11.0 (2026-11-28) shipped the toolchain written in Nazm for macOS and Windows x86_64. The
copyright holder asked (2026-11-28) what it would take for Nazm to be the best on every criterion
at once: simple enough for a first language, cheap in tokens for language models, and able to hold
very many connections on a small server, as Erlang does. The evidence in the repository answers
"not yet, and here is where": each item below is a gap that a measurement or a missing study shows,
not a claim. This change proposes 0.12.0 as the release that closes or measures them, beside the
platforms already moved to it.

**Scope decided (the copyright holder, 2026-11-28): all five items below are in 0.12.0.** The four
follow-up decisions were taken the same day (the table at the end); nothing is started until each
item's own change is written.

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

### In 0.12.0: five gaps from the evidence (holder, 2026-11-28)

1. **A million connections on a small server, proved.** Today (`performance-log.md`, the
   concurrency round): spawning 1,000,000 tasks takes 1,501 ms against Go's 271 and Node's 183;
   fan-in of 1,000,000 values from 8 producers is 845 ms against Go's 67, eighth of nine, and the one
   regression of that round; memory per blocked task is 1.2 KiB (Go 2.7 KiB), and the HTTP server's
   9.6 MiB under load is the least of six. Proposed: fix fan-in (the 8-producer, one-channel case
   after the lock-free hand-off) and task start-up; then a published, reproducible run of 1,000,000
   concurrent idle and active connections on Linux (needs Linux first), with memory, latency
   percentiles and the host named. Target to be set from the first honest run, not before.
   *Host (decided):* the project's Apple M1 Pro Mac (10 cores, 32 GB), the server in a Docker Linux
   container capped to a small server's size (about 2 CPUs and 4 GiB) and the load generator in
   another container; about 17 source addresses or listening ports, since one address pair holds
   about 64,000 connections; raised file limits; a quiet window only. The report names the laptop
   and the caps. A native macOS run comes first, while the fixes are made.
2. **Erlang-style reliability: supervised tasks.** *Form (decided): a standard-library module
   (`@std/supervise`) over three small runtime and specification additions: links, monitors and a
   task's exit reason; as in Erlang, where supervisors are OTP's library over `spawn_link` and
   `monitor`. No new syntax and no epoch for the supervisors themselves.* Nazm's structured concurrency joins tasks and
   reports failures, but a long-running server has no restart policy. Proposed, spec first
   (`CONTRIBUTING.md`, the constitution before the code): supervisors with one-for-one and
   one-for-all restart, restart intensity limits, and links/monitors over the existing task scopes;
   with a defined interaction with capabilities and cleanup. Deadlock detection on channel waits is a
   candidate in the same design.
3. **What makes a first language hard, and how Nazm compares.** The beginner syntax (5b.11–5b.19)
   and the diagnostics are designed for first-time programmers but were never checked against what
   is known about them. *Method (decided): a desk study, no recruiting.* Published research
   (Stefik and Siebert 2013 on syntax; Becker, Denny and others on error messages; Brown and
   Altadmri's Blackbox study of novice mistakes) and public posts by learners, compared across
   languages on syntax, error messages, installing and the first program, the commonest mistakes,
   and the hard ideas (types, mutation, absence, concurrency, ownership). Then Nazm is checked
   against the findings: the commonest novice mistakes written as Nazm programs, each diagnostic
   judged on whether a beginner could act on it; the gaps become diagnostic and guide changes, each
   through its own review. Posts are cited by link and quoted little; no author is recorded. This
   shows fit with known difficulties, not that beginners find Nazm easy: nothing may claim Nazm
   was tested on beginners. A study with people stays a later option.
4. **A token-cost benchmark for language-model agents.** `docs/internal/research/evaluation.md`
   designs the comparison (same budget pinned in dollars, tokens, turns and wall-clock; repeated runs
   not treated as independent) but has no driver. Proposed: the driver, and a first run of the same
   tasks in Nazm, Python and TypeScript, reporting tokens to a correct, tested solution and the
   failure modes. *Budget (decided): no paid API; the holder's Claude subscription, each task run
   through Claude Code headless (`claude -p`, JSON output for tokens, turns and time).* The driver
   paces itself to the subscription's limits, resumes where it stopped, and runs when no working
   session needs the allowance; a 5-task-per-language trial first. The report names the model and
   Claude Code and says nothing about other vendors' models.
5. **A small-object allocator.** binary-trees (depth 18) is the largest single gap measured: after
   the box and free-list fixes, 1.7× C's time and 5.5× its memory (1,971.7 ms, 98.3 MiB; C 1,158 ms,
   17.8 MiB), thirteenth of sixteen. What remains is the box's two-block representation and the
   retain/release pairs of a MIR without moves (performance-leadership 8.2). Proposed: a one-block
   box and a size-class allocator in the runtime, then move-aware MIR to drop the redundant pairs;
   measured in a quiet window against C, Rust and Go.

### Suggested order

Linux first (items 1 and the platform scope need it), then 5 and the fan-in half of 1 (runtime work,
measurable on macOS now), 2 (spec, then runtime), 4 (driver now, trial when the machine is idle),
and 3 alongside, since it is reading and writing rather than machine time.

## Impact

Runtime (allocator, scheduler, channels), the language specification (supervision: new constructs or
library, with new diagnostic codes and an epoch decision), the code generator and release scripts for
Linux and WASI, the benchmark harness, and a research protocol for the study. Each item reaches main
through its own change and the usual gates; the release gate is the 0.11.0 gate plus Linux.

## Decisions for the holder

All taken by the copyright holder on 2026-11-28.

| | Question | Decided |
| --- | --- | --- |
| (a) | Which of the five go into 0.12.0 | All five |
| (b) | Supervision as language constructs or a library | A library (`@std/supervise`) over links, monitors and exit reasons |
| (c) | The machine for the million-connection run | The project's M1 Pro Mac; Linux in Docker with the server capped to about 2 CPUs and 4 GiB; a quiet window |
| (d) | The token benchmark's budget | No paid API: the holder's Claude subscription through Claude Code headless, paced to its limits |
| (e) | The beginner study's recruiting and consent | No recruiting: a desk study of research and public posts; no claim of testing on beginners |
