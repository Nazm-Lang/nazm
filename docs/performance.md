# Performance claims, and how each is measured

**Status: the first measurements exist, and are below.** They cover this implementation
against itself — interpreter against native, and one optimisation pass against its own
baseline — and, **on one program only**, against `clang` and `python3`.

**Corrected 2026-09-21.** This paragraph said the measurements *"say nothing about C, Rust,
Zig, Go or Python: no cross-language comparison has been run"*. That stopped being true
when `bench/reference/sieve.c` and `bench/reference/sieve.py` were added and `xtask bench`
began timing them — refusing to time a reference whose answer differs from the Nazm
program's. What is accurate now: **one program, one host, one C compiler**, recorded in
`bench/baseline-linux-aarch64.json` as the `reference-c` stage. Nothing has been measured
against Rust, Zig or Go, and one sieve is not a language comparison. The claims in the
second half of this file remain targets.

This file exists so that claims are written down *before* benchmarks are chosen, which
is the only order in which a benchmark can falsify anything.

---

## The measurements

`cargo xtask bench` times six programs in `bench/programs/` plus the compiler's own
source, through five stages each — six where an external reference exists (`reference-c`,
`reference-python`), and a seventh program, `floor.nz`, exists to measure the process floor
rather than the language. Seven runs after one discarded warm-up; the median is
reported, and one baseline file per host holds the full record.

| | |
| --- | --- |
| Machine | Darwin arm64, 10 CPUs |
| Toolchain | rustc 1.98.1, Apple clang 21.0.0 |
| Baseline commit | `8ec8729`, clean tree |
| Not controlled | thermal state, other processes, CPU frequency scaling, address-space layout |

> **`bench/baseline.json` (Darwin) is stale and has not been re-recorded.** Two things
> changed under it: a failure now leaves a compiled function through an explicit exit, so
> the emitter writes a check after every call; and the compiler benchmark's input changed
> from a four-file concatenation to the real five-file source with its imports resolved.
> Re-recording it means running generated programs on the host, which the resource
> contract forbids, so it stays as it is and is **not** a valid comparison until someone
> re-records it on a Darwin machine. `bench/baseline-linux-aarch64.json` *has* been
> re-recorded, inside the contained runner, and is current.

### What the emitter revision cost

Measured rather than estimated, because the number is large enough to matter:

| | before | after |
| --- | ---: | ---: |
| `strings/build-O0` (unchanged program) | 38.8 ms | 39.1 ms |
| `strings/build-O2` (unchanged program) | 49.1 ms | 49.7 ms |
| `compiler/check` (no emission) | 12.4 ms | 12.5 ms |
| `compiler/build-O0` | 200.1 ms | 468.7 ms |

An unchanged program costs **under 1%**. The compiler's own build is 2.4×, and the two
causes — a different emitter and a different input — are separated by measuring all four
combinations rather than by assuming the split follows the line count. The *before* row is
`crates/nazm-lir/src/emit.rs` and `ir.rs` restored to `cea9158^`, with the rest of the tree
left alone; every cell was measured in one session, on one machine, inside the runner,
three runs each:

| median of 3 | baseline-era input, 3,561 lines | current input, 4,501 lines |
| --- | ---: | ---: |
| emitter before this change | **190 ms** | **247 ms** |
| emitter after it | **380 ms** | **475 ms** |

The emitter revision costs **2.00× on the old input and 1.92× on the new one**; the input
change costs **1.30× with the old emitter and 1.25× with the new one**. The two are
consistent across both rows and both columns, which is what makes *this* decomposition a
measurement rather than an apportionment. The 190 ms cell also corroborates the recorded
baseline's 200.1 ms for the same combination.

**What it does not decompose.** Three commits touched those two files between the rows —
`cea9158` (the failure protocol), `eefac34` (`chan_recv`'s allocation failure) and
`ada3ae2` (branching a failed call straight to the function exit) — 241 insertions against
15 deletions. So the 2.00× is the **emitter revision's overall effect**, not the isolated
cost of the failure check. Splitting it further would need a build with only `cea9158`
reverted, and that has not been measured; claiming the whole figure for the failure
protocol would repeat, one level down, the error corrected below.

An earlier draft of this section attributed 1.22× to the input because the line count had
risen 22%. That was a guess dressed as an attribution — line count is not compile time,
and the measured figure is 1.25–1.30×. It is recorded here because the number happening to
land near the guess is exactly what would have kept the guess from being noticed.

The failure protocol is the largest part of that revision and the part with a cost model:
the check is written once per call site, so the cost tracks how call-dense a program is
rather than how long it is. The compiler has about sixteen hundred call sites; the
benchmark programs have a handful each, which is why they moved under 1%. That explains
why the revision should be expensive here and cheap there — it is not a measurement of the
check on its own.

Of the 503 ms measured before the block reduction below, **457 ms was clang** and about
46 ms was Nazm's own emission — so this is IR volume reaching the assembler, not the
emitter being slow. Branching a failed call straight to the function's exit, rather than
through a block that only forwards to it, removed 1,564 basic blocks of 9,310 and about
28 ms. The remainder is the price of reporting a failure only once every task in scope has
been joined, and it was paid deliberately.

**The comparison unit is two runs on one machine minutes apart.** `cargo xtask bench
--check` refuses to compare across hosts rather than producing a percentage that means
nothing.

### The regression threshold, and the noise floor under it

A change counts as a regression when the median is **both more than 25% and more than
5 ms slower** than the baseline. The absolute bar is not a way of being lenient; it is
there because a ratio is the wrong test near zero, and the first `--check` run proved
it by reporting `arith/check` at 3.3 → 5.8 ms as a regression on a code path nothing
had touched. Timing the same command fifteen times in one burst gives a spread of 1.13×
to 1.34× on anything in the three-to-eight millisecond range; across bursts minutes
apart it is wider, because process start-up and filesystem cache state are most of what
is being timed.

The consequence, stated rather than left to be discovered: **every measurement under
about fifteen milliseconds is below this method's noise floor**. Those numbers are
recorded and not gated. Measuring them properly needs a harness that does not pay
process start-up per sample, and this one does.

### Verifying that the work is real

Two things can make a benchmark number meaningless, and both were checked rather than
assumed.

**The compiled form might have been folded away.** None of these programs reads external
input, so an optimiser is entitled to compute the answer at compile time and leave a
loop that does nothing. Checked by scaling the input and seeing whether the time
scales — `calls` at 1M, 4M and 16M iterations, at `-O2`, with the process floor
subtracted:

| iterations | wall | minus the floor |
| ---: | ---: | ---: |
| 1 000 000 | 3.80 ms | 0.92 ms |
| 4 000 000 | 6.03 ms | 3.15 ms |
| 16 000 000 | 13.41 ms | 10.53 ms |

Four times the work takes about 3.4× and 3.3× the time. That is linear within the
noise, so the loop runs.

**The two implementations might not be doing the same thing.** Every program states its
answer in a `.expected` file beside it, and `cargo xtask bench` runs both forms and
compares before it times anything. A program whose implementations disagree, or which
has no recorded answer, is refused rather than measured.

**Everything below includes process start-up.** `floor.nz` is an empty program and
measures that cost alone: about **2.9 ms**. It is not subtracted from the table — the
numbers are wall time of a whole process, which is the thing a person waits for — but
it is measured every run so it can be.

### Interpreter against native

Median milliseconds, after the optimisation pass below.

| Program | `nazm run` | native `-O2` | ratio |
| --- | ---: | ---: | ---: |
| `arith` — 3M iterations of checked arithmetic | 423.3 | 8.2 | 52× |
| `calls` — 1M iterations through two small functions | 647.3 | 4.8 | 135× |
| `sieve` — primes below 400 000 | 294.2 | 6.0 | 49× |
| `sequences` — 400 000 pushes, then a read/write pass | 228.0 | 4.9 | 47× |
| `strings` — 60 000 parts through one `str_join` | 17.5 | 6.8 | 2.6× |
| `channels` — 50 000 hand-offs over a capacity-1 channel | 254.0 | 223.3 | 1.1× |

Three of these are worth saying out loud because they are the ones that could be
misread:

- **`strings` is 2.6×, not 50×.** Both implementations spend their time in the
  allocator, so compiling the surrounding loop buys little. A speed claim drawn from
  `arith` and applied to string-building work would be wrong by a factor of twenty.
- **`channels` is 1.1×.** 223 ms for 50 000 hand-offs is about **4.5 µs per message**,
  and both implementations are dominated by the cost of waking a thread rather than by
  anything the compiler emits. The structured-concurrency model is usable for
  coarse-grained work and is **not** suitable for fine-grained message passing; nothing
  here claims otherwise, and no scheduler work has been done.
- **`calls` is 135×, and that ratio is the least useful number here.** The interpreter
  builds an environment frame per call and the native code does not, so the gap is
  real — but 4.8 ms of native wall time is about 1.9 ms of work on a 2.9 ms floor, so
  the ratio is as much a statement about process start-up as about the compiler. Every
  native figure in the table has the same shape; `arith`, with roughly 4.8 ms of work,
  is the one where the measurement is mostly the program.

### Compile time

| | check | build `-O0` | build `-O2` |
| --- | ---: | ---: | ---: |
| Any one benchmark program | 2.8 – 4.4 ms | 59 – 62 ms | 65 – 78 ms |
| The compiler's own source (3 500 lines then; **4,647** now, so this row is not comparable to a current run) | 8.2 ms | 198.6 ms | — |

**Nazm's own front end is not what a `nazm build` waits for.** Checking the whole
self-hosting compiler takes 8 ms; building a four-line program takes 60. The difference
is `clang` assembling and linking, which is a fixed cost this compiler pays per
invocation and has done nothing about. That is the honest answer to "is it fast to
compile": the part written here is fast, and it is a small part of the wall clock.

## The optimisation pass, and what it was based on

One pass, chosen from a profile rather than from intuition. `sample` on the release
binary running `bench/programs/arith.nz` — a loop doing nothing but arithmetic on five
locals — attributed **81 of 277 samples on the evaluation thread (29%)** to looking up
variable names:

| | samples |
| --- | ---: |
| `RandomState::hash_one::<String>` | 43 |
| SipHash `Hasher::write` | 23 |
| `memcmp` (hash-map key comparison) | 15 |
| `Interpreter::eval` (the dispatch itself) | 122 |
| `Interpreter::block` | 26 |
| `drop_glue::<Value>` | 20 |
| `Interpreter::binary` | 15 |

The environment was `Vec<HashMap<String, Value>>`, so every variable read hashed a
string. A frame holds a handful of bindings and the checker refuses two of one name in
one scope, so the map was replaced with a short association list scanned linearly.

Measured against the recorded baseline, same machine, minutes apart:

| Program | before | after | |
| --- | ---: | ---: | ---: |
| `arith/interpret` | 876.7 ms | 423.3 ms | **2.07×** |
| `calls/interpret` | 908.8 ms | 647.3 ms | **1.40×** |
| `sieve/interpret` | 467.0 ms | 294.2 ms | **1.59×** |
| `sequences/interpret` | 290.0 ms | 228.0 ms | 1.27× |

Larger than the 29% the profile accounted for, because removing the hashing also removes
the per-lookup setup and touches less memory. Nothing measured got slower.

Re-profiling afterwards: SipHash is absent, and the remaining time is the `eval` dispatch
itself (65 of 119 non-idle samples), `block` (15), `memcmp` from the linear scan (15),
`binary` (11) and dropping `Value` (10). **The next pass would have to attack the
dispatch**, which means resolving names to slot indices at check time — a much larger
change, and not one to start without a reason beyond "it is next in the profile".

---

## The claim, stated so a hostile reader can check it

> Nazm compiles through LLVM for release builds, so on scalar single-threaded code it
> targets **parity with C, Rust, and Zig — not a win**.
>
> Where Nazm is designed to win:
> - **Layout-bound workloads**, measured against **both idiomatic Rust and hand-written SoA
>   Rust**. Beating only idiomatic Rust measures a different representation, not a better
>   compiler.
> - **Allocation-heavy workloads vs Go**: throughput and p99.9 latency, no GC.
> - **Scripts vs Python**.
>
> Nazm does not claim to beat C on scalar code.

**All numeric targets are provisional until baselines are measured.** An earlier draft
asserted "geomean within 1.05× of Rust and 1.10× of C" and "any loss beyond 1.2× is a
tracked bug". Those are removed rather than left standing: there is no measured
distribution to set them against, so they were decoration.

## Where the headroom actually is

Ranked by expected payoff. Only the first is a genuine differentiator.

| Lever | Expected payoff | Against | Confidence |
| --- | --- | --- | --- |
| Data-oriented layout selection (auto SoA, field reorder, hot/cold split) | large on layout-bound code | C, Rust, Zig, Go | the one real unlock — and unproven |
| Escape analysis + region/arena inference, no GC | large on allocation-heavy code; p99.9 latency | Go, Java, Python | high |
| Comptime + whole-program specialisation | moderate | Go interfaces, C++ vtables | medium-high |
| No-alias from the ownership model | small | C | medium — Rust has this and has struggled to cash it |
| Runtime quality (scheduler, allocator, no false sharing) | tail latency, not throughput | Go | medium |
| PGO / JIT respecialisation | moderate, branchy code | everyone | expensive, stage late |
| Auto-vectorisation | ~zero as a differentiator | nobody | **do not market this.** LLVM already does it; wins attributed to it are usually the layout lever giving the vectoriser something it can use |

Avoiding LLVM's compile-time cost is a **compile-time** lever. It does not belong in a
runtime performance claim, and Cranelift output is meaningfully slower than LLVM `-O2`.

## What will make Nazm slower than C for a long time

The standard library. A young `HashMap` loses to `hashbrown`, and library quality compounds
across a whole program.

An earlier draft said "real programs are 80% library calls" and predicted a three-year
disadvantage. Both are removed — the percentage was unsupported and the duration was a
guess presented as a forecast.

## Measurement rules

1. **Layout wins are measured against hand-written SoA Rust**, not only idiomatic Rust.
2. **Compile time and runtime are reported separately.** They trade against each other.
3. **Agent development efficiency is reported separately from both**, under
   [`evaluation.md`](research/evaluation.md). Go is a runtime reference there, not an arm.
4. **The task-memory benchmark states its terms**: task states distinguished (created /
   parked / runnable / running), stack allocation strategy, lazy commit, **RSS measured
   rather than virtual reservation**, page size recorded per platform.
5. Every published figure carries the machine, toolchain version, and commit hash.
6. **An external comparator must do equivalent work and produce the same result**, and is
   not timed at all if its answer differs from the Nazm program's. `xtask/src/bench.rs`
   enforces this rather than trusting it: a reference program has to reproduce the case's
   `.expected` output before a single measurement is taken. *(Absorbed 2026-09-21 from the
   Era-1 contract §12, which is otherwise retired.)*
