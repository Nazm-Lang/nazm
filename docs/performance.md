# Performance claims, and how each is measured

**Status: the first measurements exist, and are below.** They cover this implementation
against itself — interpreter against native, and one optimisation pass against its own
baseline. They say **nothing** about C, Rust, Zig, Go or Python: no cross-language
comparison has been run, and the claims in the second half of this file remain targets.

This file exists so that claims are written down *before* benchmarks are chosen, which
is the only order in which a benchmark can falsify anything.

---

## The measurements

`cargo xtask bench` times six programs in `bench/programs/` plus the compiler's own
source, through five stages each. Seven runs after one discarded warm-up; the median is
reported, and `bench/baseline.json` holds the full record.

| | |
| --- | --- |
| Machine | Darwin arm64, 10 CPUs |
| Toolchain | rustc 1.98.1, Apple clang 21.0.0 |
| Baseline commit | `8ec8729`, clean tree |
| Not controlled | thermal state, other processes, CPU frequency scaling, address-space layout |

**The comparison unit is two runs on one machine minutes apart.** `cargo xtask bench
--check` refuses to compare across hosts rather than producing a percentage that means
nothing.

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
- **`calls` is 135×** because the interpreter builds an environment frame per call while
  the native code does not. It is the largest ratio here and the least representative of
  anything.

### Compile time

| | check | build `-O0` | build `-O2` |
| --- | ---: | ---: | ---: |
| Any one benchmark program | 2.8 – 4.4 ms | 59 – 62 ms | 65 – 78 ms |
| The compiler's own 3 500 lines | 8.2 ms | 198.6 ms | — |

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
   [`evaluation.md`](evaluation.md). Go is a runtime reference there, not an arm.
4. **The task-memory benchmark states its terms**: task states distinguished (created /
   parked / runnable / running), stack allocation strategy, lazy commit, **RSS measured
   rather than virtual reservation**, page size recorded per platform.
5. Every published figure carries the machine, toolchain version, and commit hash.
