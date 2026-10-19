## Why

The copyright holder set Nazm's performance target on 2026-10-15: **10–30% faster than optimised
C, C++ and Rust**, and **the best of every compared language on every measured parameter** —
widened the same day to every language compared: C, C++, Rust, Zig, Go, C#, Java, Swift, Python,
Ruby, JavaScript, TypeScript, Ada, Pascal, Fortran, COBOL and hand-written assembly — run
time, peak memory, compile time, binary size, start-up, latency, multicore scaling and lines of
code — without giving up correctness or memory safety. It is a research target, not an achievement:
on 2026-10-15 Nazm took 55 ms for `fib` (C 35 ms) and 188 ms for word frequency (Rust 65 ms, C 74 ms),
1.5–3× behind, and the comparison has three programs.

## What Changes

- **Phase 1 — a verified baseline (with R1.5, D13 and 5.1).** An audit of the compiler, backends and
  runtime as they are; `bench/compare/` grown from three programs to a broad suite (numeric,
  memory-intensive, branch-heavy, concurrent, I/O-bound, real applications) written with the same
  algorithm and checks in every language; repeated runs with warm-ups, confidence intervals and the
  host disclosed; run time, P50/P95/P99 latency where it applies, peak memory and allocation counts,
  compile time, binary size, single-thread and multicore scaling, lines of code; a safety column and
  safety-equivalent rows (Rust with overflow checks, C with sanitizers) beside the fastest unsafe
  ones.
- **Phase 2 — the compiler (after R2 settles).** Checks removed only where the compiler proves them
  unnecessary (ranges, bounds, overflow), alias and escape analysis from ownership, inlining across
  modules, stack allocation, vectorisation, profile-guided optimisation; each with a measured gain
  and its correctness evidence.
- **Phase 3 — runtime and memory.** Ownership-aware allocation, data layout, the task scheduler.
- **Phase 4 — the hardware.** SIMD, cache locality, multicore scaling, NUMA where it applies.
- **Phase 5 — independent verification.** Reproducible scripts, published raw data, an outside
  review before any claim.

## Focus (the copyright holder, 2026-10-15)

macOS only for now — Apple Silicon, and x86_64 under Rosetta where it costs nothing — and three
parameters first: **run speed with safety on**, **compile time** (clean and cached, against Go and
C) and **multicore** scaling (a parallel workload against Go, Rust and C). The ecosystem —
libraries, community, other platforms — comes later; GitHub's other hosts stay as free evidence,
with no porting effort spent on them.

## Non-negotiable

Correctness, memory safety and the defined semantics are never traded for a benchmark; no workload
is cherry-picked and no competitor is handicapped; each benchmark's logic is Nazm; nothing is
claimed without reproducible evidence, and results, hypotheses and goals are kept apart.

## Capabilities

### New Capabilities
- `performance-leadership`: the measured target and the rules for claiming it. Archived with
  `--skip-specs`.

## Impact

`bench/compare/`, `docs/internal/performance-log.md`, the optimiser, LIR, the runtime and both backends; the
rewrite's stages port whatever this changes. Nothing a program means changes.
