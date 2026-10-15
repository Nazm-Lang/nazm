## 1. Phase 1 — a verified baseline (with R1.5)
- [ ] 1.1 Audit (begun 2026-10-15: word frequency profiled, design.md): the passes, backends, runtime, allocation, ownership and concurrency costs as the
      code has them (not as the documents say), with the bottlenecks ranked
- [ ] 1.2 The languages: add Fortran (gfortran), COBOL (GnuCOBOL) and hand-written aarch64
      assembly to `bench/compare/` beside the fifteen there; assembly is the ceiling a compiler is
      measured against, written once per workload by hand
- [ ] 1.3 The suite: 15–20 workloads across numeric, memory-intensive, branch-heavy, concurrent,
      I/O-bound and application kinds, one algorithm and one output check in every language
- [ ] 1.4 The measurements: repeated runs, warm-ups, confidence intervals, host disclosed; run time,
      latency percentiles, peak memory, allocations, compile time, size, scaling, lines
- [ ] 1.5 The baseline published in `docs/performance.md`: where Nazm leads and where it is behind

## 2. Phase 2 — the compiler (after R2)
- [ ] 2.1 Ranked optimisation plan from 1.1 and 1.4, each with its acceptance criteria
- [ ] 2.2 Proven-unnecessary checks removed; alias and escape analysis; inlining; vectorisation; PGO

## 3. Phases 3–5
- [ ] 3.1 Runtime and memory
- [ ] 3.2 Hardware-aware code generation
- [ ] 3.3 Independent verification before any claim
