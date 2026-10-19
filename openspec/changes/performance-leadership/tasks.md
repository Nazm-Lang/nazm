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
- [ ] 1.5 The baseline published in `docs/internal/performance-log.md`: where Nazm leads and where it is behind

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
- [x] 4.3 A call in a call cycle is never a tail call (`notail`, LLVM backend), so no optimiser turns
      an unbounded recursion into a loop that spins instead of failing with `N0408` (LLVM 23 did;
      2026-10-19, design.md)

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

- [x] 6.1 The baseline, now (on the reference, after R1.5's current task): spawn 1,000,000 tasks;
      channel ping-pong latency; fan-in throughput; memory per live task; HTTP/1.1 requests per
      second on loopback; a parallel CPU job — Nazm against Go on this Mac, quiet host, recorded in
      `docs/internal/performance-log.md` with what each side guarantees
      — 2026-10-16: the programs, `bench/compare/{nazm,go}/` (`spawn`, `pingpong`, `fanin`, `idle`,
      `httpd`, the shared load generator `go/httpload.go`, and `primes`), and
      `bench/compare/concurrency.sh`, which checks both print the same answers and refuses to time
      on a loaded host; measured from below load 2 (`performance.md`, *Concurrency*): spawn 13–14× and
      hand-offs 14× slower than Go, fan-in 6.5×, 17.4 against 2.7 KiB per blocked task, HTTP 0.84–0.86×
      Go at half its memory, parallel CPU 12–19% behind; two runs
- [x] 6.2 Small tasks: growable or stackless task frames, so millions fit in memory (design first)
      — 2026-10-16: copied stacks (design.md, *Concurrency engineering*): a worker's tasks share its
      run stack and a parked task's frames are copied out; 826 bytes per blocked task against
      17.9 KiB (peak RSS, 20,000 against 1,000 blocked); runtime ABI 40. Timings wait for a quiet host
- [x] 6.3 A work-stealing scheduler, and preemption at loop back-edges
      — 2026-10-16: unstarted and resumed queues per worker, stealing of unstarted tasks, a 50 µs
      spin, a join without a lock (ABI 41); preemption at every loop back edge through a polled flag
      and a 10 ms monitor, off with one worker (ABI 43). Timings, and the poll's cost on hot loops
      (`bench/compare/preempt_cost.sh`), wait for a quiet host
      — 2026-10-17, measured below load 2 (`performance.md`, *Preemption's cost on hot loops*): a
      tight loop +8.9%, `words` +9.7–10.2%, `fib` and `primes` unchanged — above the 1-2% the design
      allows, so the poll's design is reopened
      — 2026-10-17, reopened: preemption by interrupt, no instruction in any loop (ABI 44; design.md,
      *Preemption, second design*). macOS (arm64 and x86-64 under Rosetta) run-verified; Windows
      built and linked with MinGW, not run (host off); Linux and Android preempt nothing yet. Its
      cost waits for the quiet window
- [ ] 6.4 `select` over several channels, and lock-free channels
      — 2026-10-16, part: the waiter table's 64 locks and no lock for an empty bucket (ABI 42).
      Open: `select` over channels of different types (designed, design.md) and a direct hand-off
      — 2026-10-17, part: the direct hand-off on the `Int` channel — a send to a parked receiver
      writes into its wait record and wakes it alone, a receive moves a parked sender's value into
      the ring, both first come first served (runtime ABI 45; `compiler/emit.nz` mirrored). Timings
      wait for the coordinator's quiet window (`bench/compare/concurrency.sh`)
      — 2026-10-17, part: `select` over channels of different types — `Select` and `sel_new`,
      `sel_recv`, `sel_send`, `sel_wait`, `sel_wait_until` (spec *Select over channels of different
      types*), on every tier (both interpreters, LLVM, Cranelift, Nazm's code generator, the
      pipeline); semantic epoch 91, runtime ABI 46. Open: lock-free channels
      — 2026-10-17, part: the direct hand-off on the generic channel `Chan[T]`, the same record,
      park and hand design, a select's offer and look included (runtime ABI 48)
      — 2026-10-17, part: lock-free channels — both kinds one implementation over a ring of
      sequenced slots taken by compare-and-swap while nobody waits (the close a bit of the
      enqueue position), the mutex and the hand-off only once someone waits (runtime ABI 49).
      Timings wait for the coordinator's quiet window
- [ ] 6.5 The comparison re-run against Go, tokio, BEAM, Java and C#; a claim only where measured

6.1 is measured now to know where Nazm stands; 6.2–6.5 are engineering for stage P, after the
cut-over, written once in Nazm (the order rule above: nothing done twice).

## 7. The compiler's own time and memory (the copyright holder, 2026-10-16)

Both compilers — the Rust one and the one written in Nazm — measured phase by phase for how time
and memory grow with the program, and made cheaper, nanoseconds first, then microseconds, then
milliseconds. Measurement on both now; fixes in the Nazm-written compiler (it is `nazm` after R8),
in the Rust compiler only where users would meet the cost before then (the order rule above).

- [x] 7.1 The complexity audit: for each phase (lexing, parsing, resolution, checking, Core IR, MIR,
      LIR, code generation, linking, the caches), generated programs of n, 2n, 4n, 8n lines and of
      deep nesting, long chains and many modules; time and peak memory fitted to a growth class;
      anything worse than n log n flagged with the input that shows it; both compilers; quiet host
      — 2026-10-17: `bench/complexity/{gen,measure}.py`, `performance.md`, *The compilers' growth*:
      flagged — the Nazm pipeline's LIR quadratic in a module (`ops.nz` `line_col`, 12.9 s at 16,104
      lines), `nzcg`'s register allocation 2.67 in nesting (`common.nz` `allocate`), the Rust MIR
      quadratic in a `+` chain (97 MiB for 480 terms), the Nazm Core IR's memory (29 KiB a line);
      limits — 512 `+` terms refused (`N0102`), the pipeline's JSON reader refuses 256 levels
- [x] 7.2 Latency budgets: `nazm check` of a small program in microseconds, a warm `nazm build` in
      milliseconds, per-function work in nanoseconds — set from 7.1 and enforced as tests
      — 2026-10-17: budgets set (`performance.md`, the same section: check ≤ 15 ms wall / 5 ms in
      process, warm build ≤ 100 ms, front end ≤ 40 µs, MIR ≤ 10 µs, LIR ≤ 1 µs a function, growth
      ≤ 1.25); open: the tests that enforce them, with 7.4's growth guards
- [ ] 7.3 Fixes in priority order: superlinear phases first, then the largest constant factors
      (hashing, allocation per node, copies), each measured before and after
      — 2026-10-17, superlinear phases fixed, each argued from the algorithm and output
      byte-identical, the quiet re-measurement (`measure.py`) still to do: the pipeline's LIR
      (`line_col` by a table of line starts), `nzcg`'s `allocate` (liveness name by name), the
      Rust MIR's `+` chain (operands borrowed, not cloned), the Nazm Core IR document's printing
      (one part per line's indentation). Open: the Core IR document is pretty-printed, so its
      size is a nest's or chain's size times its depth in both compilers; the Nazm Core IR's
      29 KiB a line; the Rust `core-ir` and the Nazm checker and parser on `nest`
- [ ] 7.4 A growth guard per phase in both compilers, like `…lowers_in_time_proportional_to_its_size`
      — 2026-10-17, for each phase fixed in 7.3: `bench/complexity/growth.py` counts the
      instructions a Nazm-written phase retires at n and 8n (run by the pipeline's, `nzcg`'s and
      the front end's `run.sh`), and `mir::a_long_plus_chain_lowers_in_time_proportional_to_its_length`;
      the other phases still to guard

## 8. Allocation-heavy programs (binary-trees, 2026-10-19)
- [x] 8.1 A box's storage made with it at one element; a program of one thread keeps every small
      block it frees; a release not the last inlined (`nz.vec_free` the rest); one-thread memory
      report counts plain — runtime ABI 51, 163 -> 98 MiB at depth 18 (design.md). Speed to be
      measured in a quiet window
- [ ] 8.2 A box as one block `{count, value}` (a `Box` type below Core IR) and moves in MIR, so a
      read does not retain and release what it only passes on (design.md, *What is left*)
