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

## Order (the copyright holder, 2026-10-15): nothing done twice

Work that survives the rewrite unchanged is done now; work that would be written in the Rust
compiler and then ported is done once, in Nazm, after the cut-over (R8), in a performance stage
before the 1.0 gate. Changes to what generated code looks like are decided now and made in the
reference before R3 and R9 port that part, so the rewrite copies only what stays.

- **Now:** this file's section 1 (the baseline, the suite, the languages); the standard library's
  speed; the runtime's (allocator, byte search, the multicore scheduler and its benchmark); the
  design of a call — 4.1 and 4.2 below.
- **After the cut-over, in Nazm:** section 2's passes and the compiler's own speed.

## 2. Phase 2 — the compiler (after the cut-over, written once in Nazm)
- [ ] 2.1 Ranked optimisation plan from 1.1 and 1.4, each with its acceptance criteria
- [ ] 2.2 Proven-unnecessary checks removed; alias and escape analysis; inlining; vectorisation; PGO

## 4. A call's design (now, before R3 and R9 port it)
- [x] 4.1 Stack overflow by a guard page — decided against (2026-10-15): the check costs nothing
      measurable and a guard page cannot keep `N0408`'s unwinding (design.md)
- [ ] 4.2 Failures by unwinding, replacing the returned bit — decided 2026-10-15 (the bit is
      `fib`'s whole gap, design.md); to implement in the reference before R3/R9 port the convention

## 5. Zero dependence on LLVM (after 1.0)
- [ ] 5.1 Nazm's own optimiser and code generator for optimised builds, so no build needs `clang`
      or LLVM: the copyright holder's goal (2026-10-15), after 1.0 — LLVM's optimiser is what C,
      C++ and Rust use, so this is measured against the LLVM build before it replaces it

## 3. Phases 3–5
- [ ] 3.1 Runtime and memory
- [ ] 3.2 Hardware-aware code generation
- [ ] 3.3 Independent verification before any claim

## 6. Concurrency: lead Go's goroutines (the copyright holder, 2026-10-16)

Target: the best concurrency of any language — Go's goroutines first, then Rust (tokio), Erlang/BEAM,
Java's virtual threads and C#'s async — measured, with Nazm's safety kept: data races refused at
compile time (`N0321`), no task outliving its `scope` (`N0320`), authority only as passed.

- [ ] 6.1 The baseline, now (on the reference, after R1.5's current task): spawn 1,000,000 tasks;
      channel ping-pong latency; fan-in throughput; memory per live task; HTTP/1.1 requests per
      second on loopback; a parallel CPU job — Nazm against Go on this Mac, quiet host, recorded in
      `docs/performance.md` with what each side guarantees
      — 2026-10-16: the programs, `bench/compare/{nazm,go}/` (`spawn`, `pingpong`, `fanin`, `idle`,
      `httpd`, the shared load generator `go/httpload.go`, and `primes`), and
      `bench/compare/concurrency.sh`, which checks both print the same answers and refuses to time
      on a loaded host; measured at load 1.9–3.4 (`performance.md`, *Concurrency*): spawn 13× and
      hand-offs 14× slower than Go, fan-in 6.5×, 17.4 against 2.7 KiB per blocked task, HTTP 0.86×
      Go at half its memory. Still to take quietly: HTTP rounds two and three and `primes`
- [ ] 6.2 Small tasks: growable or stackless task frames, so millions fit in memory (design first)
- [ ] 6.3 A work-stealing scheduler, and preemption at loop back-edges
- [ ] 6.4 `select` over several channels, and lock-free channels
- [ ] 6.5 The comparison re-run against Go, tokio, BEAM, Java and C#; a claim only where measured

6.1 is measured now to know where Nazm stands; 6.2–6.5 are engineering for stage P, after the
cut-over, written once in Nazm (the order rule above: nothing done twice).
