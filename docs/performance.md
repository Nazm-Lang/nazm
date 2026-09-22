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

### Incremental semantic checking — measured 2026-09-22, and smaller than it sounds

N4 lets `nazm check` skip checking the bodies of a module whose semantic inputs are
unchanged. Measured on the largest module graph this repository has — the compiler written
in Nazm, five modules and 5,041 lines — with the release binary, medians of seven runs,
Darwin arm64. **These are within-host comparisons taken minutes apart, which is the only
kind this method supports**; they are not comparable to `bench/baseline-linux-aarch64.json`
and no cross-host claim is made.

| | wall clock | modules checked | reused | entries written |
| --- | ---: | ---: | ---: | ---: |
| `--no-cache` | 40.5 ms | 5 | 0 | 0 |
| cold, store empty | **64.8 ms** | 5 | 0 | 5 |
| warm, nothing changed | 38.9 ms | 0 | 5 | 0 |
| a private definition added to `lex.nz` | 39.0 ms | 1 | 4 | 1 |
| an exported definition added to `lex.nz` | 39.3 ms | 4 | 1 | 4 |

**The reuse decisions are exactly right and the time saved is about a millisecond.** That
is the finding, and it is stated first because the opposite is what a reader expects from a
cache.

The reason is arithmetic rather than a defect. A `nazm check` of a one-line program takes
**31.7 ms** on this machine — process start, dynamic linking, and the interpreter-owned
thread `spec.md` requires. So the whole of this graph's *work* is about 8.8 ms, and what
N4 reuses is the body-checking stage of it:

| | |
| --- | ---: |
| process floor | 31.7 ms |
| work, `--no-cache` | 8.8 ms |
| work, warm | 7.2 ms |
| **saved** | **1.6 ms — 18% of the work, 3.9% of the wall clock** |

Fifteen runs each, twice, put the minima at 39.8/39.9 ms disabled and 38.8/38.8 ms warm,
so the ordering is stable; the magnitude is near this method's noise floor and is reported
as an order of magnitude, not a figure.

**The first run is slower, by 24 ms.** Publishing five entries costs an `fsync` each.
That is the price of a `rename` that cannot expose a half-written file, and it is paid once
per changed module rather than per run — but a build that checks once and never again is
worse off, and saying so is more useful than averaging it away.

**What this says about the next lever.** Parsing and declaring every module happens on
every run and is *most* of the 8.8 ms; body checking is the minority. Making reuse pay
would mean reusing the front of the pipeline too, which needs either an incremental parse
over a lossless CST or a second cache keyed on source alone. Both are real milestones, and
neither is what N4 built. `capability-matrix.md` area 2a states the bound rather than the
hope.

### What a consumer of an interface does not have to read

Not a token measurement: bytes, and no tokenizer has been run. For the same five modules,
what each one's implementation weighs against what its published `nazm.interface/1`
interface weighs:

| | source | interface |
| --- | ---: | ---: |
| `lex.nz` | 9,921 B | 3,344 B |
| `parse.nz` | 40,538 B | 2,090 B |
| `analyse.nz` | 45,536 B | 2,474 B |
| `module.nz` | 11,034 B | 468 B |
| `emit.nz` | 129,467 B | 124 B |
| **total** | **236,496 B** | **8,500 B** |

A consumer that needs only what these modules *offer* reads **3.6%** of what a consumer
that reads their implementations does. The ratio tracks how much of a module is exported
rather than how large it is — `emit.nz` is the biggest file in the tree and exports
nothing, so its interface is 124 bytes. Call this **context reduction**; converting it to
a token claim would need a tokenizer and a stated model, and neither has been run.

### Separate code generation — measured 2026-09-22, and it costs

N5 gives each Nazm module its own LLVM module and object, and links them. Nothing is
reused, so every artefact is regenerated on every build: this is an architecture change,
and the numbers are what it cost. Medians of five, Darwin arm64, against a worktree at the
commit immediately before N5 (`0bf58be`). Within-host, minutes apart, and not comparable to
`bench/baseline-linux-aarch64.json`.

| | before | after | |
| --- | ---: | ---: | --- |
| `sieve.nz` (1 module) build `-O0` | 121.7 ms | **249.6 ms** | +105% |
| `sieve.nz` build `-O2` | 138.1 ms | 267.3 ms | +94% |
| compiler (5 modules) build `-O0` | 335.7 ms | **658.2 ms** | +96% |
| compiler build `-O2` | 1670.8 ms | 1705.8 ms | **+2%** |
| `sieve` executable `-O2` | 50,440 B | 50,552 B | +112 B |
| compiler executable `-O2` | 615,080 B | **554,648 B** | **−10%** |

**The cost is `clang` processes, not code generation.** Timing each step of the five-module
build at `-O0` separately:

| artefact | IR | `clang -c` |
| --- | ---: | ---: |
| `m0` (`emit.nz`) | 1,556,038 B | 191.3 ms |
| `m1` (`analyse.nz`) | 390,149 B | 114.0 ms |
| `m2` (`parse.nz`) | 298,514 B | 106.1 ms |
| `m3` (`module.nz`) | 65,446 B | 85.4 ms |
| `m4` (`lex.nz`) | 75,867 B | 86.3 ms |
| runtime | 8,100 B | 80.5 ms |
| entry | 2,474 B | **79.4 ms** |
| link | | 91.3 ms |

The entry artefact is 2.4 kilobytes of IR and takes 79 ms, so **about 79 ms of each of the
eight invocations is process startup** — roughly 630 ms of the 658 ms build. The marginal
cost of actually generating code is 5–112 ms per artefact. At `-O2` on a real program that
overhead is 2% of the total, because optimisation dominates.

That is also the cost a cache removes: an unchanged module's `clang -c` does not run at
all. N5 does not build one, and `capability-matrix.md` does not claim one.

**The executable got 10% smaller at `-O2`, and that is the same fact from the other side.**
Per-module objects mean LLVM cannot inline across a module boundary, so less code is
duplicated into callers — and less is specialised. Measured against a possible runtime
regression: eleven runs each, twice, put `sieve` at 35.9/35.5 ms minimum before and
36.9/36.6 ms after, with overlapping medians. **About a millisecond on thirty-six, at the
edge of what this method resolves** — reported rather than dismissed, and not called a
regression on this evidence. Recovering cross-module inlining is what LTO is for; N5
establishes the boundary and adds none, because adding it to recover a number would hide
whether the boundary was affordable.

### Native object reuse — measured 2026-09-22, and it pays for the section above

N6 keys each emitted LLVM unit's object on the bytes handed to the object compiler, that
compiler's identity and the configuration it resolves, and links the recorded object instead
of compiling again. The workload is the same five-module compiler, on the same host, with
the same method: medians of five runs, and **the cache state each row names is
re-established before every one of those runs** — a row that cleared the store once and then
timed four warm builds would be reporting a number nobody experiences.

| | wall | units / compiled / reused |
| --- | ---: | --- |
| `--no-cache` clean, `-O0` | 665.4 ms | 7 / 7 / 0 |
| cold store, `-O0` | 730.7 ms | 7 / 7 / 0 |
| **warm unchanged, `-O0`** | **157.6 ms** | 7 / 0 / 7 |
| private body edit, `-O0` | 198.5 ms | 7 / 1 / 6 |
| called export's body edit, `-O0` | 211.2 ms | 7 / 1 / 6 |
| an export nobody calls, `-O0` | 217.7 ms | 7 / 1 / 6 |
| `--no-cache` clean, `-O2` | 1693.0 ms | 7 / 7 / 0 |
| cold store, `-O2` | 1755.4 ms | 7 / 7 / 0 |
| **warm unchanged, `-O2`** | **157.6 ms** | 7 / 0 / 7 |
| private body edit, `-O2` | 255.7 ms | 7 / 1 / 6 |
| called export's body edit, `-O2` | 259.1 ms | 7 / 1 / 6 |
| an export nobody calls, `-O2` | 257.3 ms | 7 / 1 / 6 |

**The three edits are indistinguishable, and that is the result.** A private body, the body
of an export every other module calls, and an export nobody calls all cost one unit of
seven. Nothing in the compiler says so; it follows from keying on the emitted bytes, which
N5 measured to contain a dependency only as a `declare` and a `call`.

**Warm builds are the same at both optimisation levels** — 157.6 ms — because no code is
generated in either. The `-O2` speedup is larger only because the work avoided was.

**The cold build pays about 65 ms**, at both levels: fingerprinting seven units, writing
seven entries and `fsync`ing each. That is 9.8% at `-O0` and 3.7% at `-O2`, and it is
reported rather than optimised away, because the durability is what makes a half-written
entry impossible.

**What is left in a warm build.** Two external processes, against eight with the cache off:

| | |
| --- | ---: |
| `nazm`'s own process, measured on a one-line program | ~36 ms |
| `clang -###`, the probe that reads the resolved configuration | 60.0 ms |
| the link, 7 objects | 89.1 ms |
| everything else — parse, check, admit, lower, emit, hash, look up | the remainder |

Those three do not sum to 157.6 ms; measured standalone from a shell each pays a spawn the
build does not, so they are an ordering rather than an addition. What they do establish is
that **the front end is not the cost** — `nazm check --no-cache` on all five modules is
45.0 ms, of which 36 ms is the process starting.

**Why the backend is identified by its version report and not by its bytes.** Hashing the
driver would be stronger and was measured: `/usr/bin/clang` on this host is a 200 KB shim in
front of a 124 MB executable, so it identifies the wrong file; reading the right one costs
146 ms and digesting it 61 ms. Two hundred milliseconds to protect a 158 ms build is not a
trade, and the limit that leaves — a toolchain replaced in place without changing what it
calls itself — is written down in `architecture.md` §7.6 rather than papered over.

**Store size.** 852 KiB in seven entries at `-O0`, 596 KiB at `-O2`; the largest entry is
562 KB, which is `emit.nz`, and the smallest is 1.6 KB, which is the entry wrapper. Nothing
is evicted. For a project that builds at several optimisation levels and edits often the
store grows without bound until someone deletes it, which is an operational limitation and
not a plan.

### Reclamation — measured 2026-09-22, and the instrument matters

N7 made `Ints` and `Strs` storage reclaimable. Two instruments, because they answer
different questions and each is misleading about the other's.

**Peak resident memory** answers *does this workload still grow*. Medians are not used
here: peak is a maximum, and a maximum over repeated runs of a deterministic program does
not move. Darwin arm64, `-O2`, against the commit before N7.

| Workload | Live set | Before | After |
| --- | --- | ---: | ---: |
| 5,000 short-lived sequences | ~400 B | 4.38 MiB | **1.78 MiB** |
| 20,000 | ~400 B | 12.28 MiB | **1.88 MiB** |
| 80,000 | ~400 B | 43.52 MiB | **1.86 MiB** |
| one 2,000,000-element sequence | 16 MiB | 19.08 MiB | 19.08 MiB |
| string-heavy loop, no sequences | grows | 23.77 MiB | 23.78 MiB |
| the self-hosted compiler on `emit.nz` | — | 46.81 MiB | 42.59 MiB |

The first three rows are the result. Before, memory tracked **work done** — 1 : 3.9 : 15.6
against iteration ratios of 1 : 4 : 16. After, it is flat, and the floor is an empty
program's 1.69 MiB. The next two rows are the control: a sequence that is genuinely live
is unchanged, and a workload whose allocation is `Str` is unchanged, which is what
reclaiming exactly one kind of value should look like.

The compiler's 9% is the honest one. Its memory is mostly `Str` bytes — it builds 2.15 MB
of output by concatenation — and `Str` is outside this slice by the argument in
`architecture.md` §7.7. A number that had moved further would have meant something else was
going on.

**Allocation and reclamation counts** answer *was this value reclaimed*, which peak
resident memory cannot: `free` returns storage to the allocator, not to the operating
system, so a program can reclaim everything and show no change at all. That is not a
hypothetical — it is why the first measurement of this work appeared to show nothing until
the counters existed. Both implementations report the same three numbers under
`NAZM_MEMORY_REPORT`, and `crates/nazm-cli/tests/memory.rs` compares them case by case.

**Runtime cost.** The traffic is at bindings, not at calls: passing a sequence to a
function emits no instruction, which is rule 2 of the constitution turned into a number.
`compiler/*.nz` creates 176 sequences and aliases **zero** of them with a `let`, so the
self-hosted compiler pays for one retain per sequence returned — it returns one — and one
release per binding at each of three exit edges. The bootstrap is unchanged at
`e5779c70…`, and the contained suite is 823 tests against 806.

**What is not claimed.** Peak memory bounded by live data for a whole program. `Str` and
`Chan` storage is still not reclaimed, so the string-heavy row above still grows without
bound, and `capability-matrix.md` area 4 states the scope per type rather than as one word.

*The last paragraph was true until N8, below, which is the same day. The rest of this
section is unchanged and its numbers are what N8 was measured against.*

### Strings and channels — measured 2026-09-22, and the incident's shape is gone

N8 made `Str` and `Chan` storage reclaimable, which completes the current type universe.
Same two instruments, same reason: peak resident memory answers *does this workload still
grow*, and the runtime counters answer *was this value reclaimed*, which peak memory cannot.
Darwin arm64, `-O2`, against the commit before N8. Peak is a maximum over a deterministic
program, so no median is taken.

| Workload | Live set | Before | After |
| --- | --- | ---: | ---: |
| 5,000 temporary heap strings | one string | 1.88 MiB | **1.73 MiB** |
| 20,000 | one string | 2.33 MiB | **1.77 MiB** |
| 80,000 | one string | 4.17 MiB | **1.77 MiB** |
| accumulator loop, 2,000 `str_concat` | 4 KB | 6.28 MiB | **2.02 MiB** |
| accumulator loop, 8,000 `str_concat` | 16 KB | **70.66 MiB** | **2.23 MiB** |
| 200 short-lived channels | one | 1.83 MiB | **1.80 MiB** |
| 2,000 short-lived channels | one | 2.38 MiB | **1.80 MiB** |
| 64 channels of capacity 256 | one | 1.88 MiB | **1.75 MiB** |
| 5,000 / 20,000 / 80,000 short-lived **sequences** | ~400 B | 1.72 / 1.77 / 1.77 MiB | 1.73 / 1.75 / 1.77 MiB |
| one 2,000,000-element sequence, genuinely live | 16 MiB | 19.06 MiB | 19.06 MiB |
| 20,000 values through one channel | one | 1.97 MiB | 1.95 MiB |
| empty program | — | 1.70 MiB | 1.69 MiB |

The fifth row is the one this work was for. That shape — `str_concat` in a loop, small live
result — is what took the machine down on 2026-09-21 and what `bootstrap.md` §1 is about;
at 8,000 iterations it is **31× smaller**, and what remains is the live string plus the
floor. The three sequence rows are the N7 regression check: unchanged, which is what
reclaiming a second kind of value should do to the first. The live sequence and the sieve
are the control.

**`str_concat` in a loop is still quadratic in time.** Nothing here changes that, and
`str_join` is still one pass and one allocation. What changed is that getting it wrong now
costs time rather than the machine.

### What the counting costs

Workloads sized so process start is noise. Median of five, `-O2`, Darwin arm64.

| Workload | Before | After | |
| --- | ---: | ---: | --- |
| 1,000,000 short strings | 31.7 ms | 48.2 ms | **+16.5 ms**, ≈16 ns per string |
| 1,000,000 slices of one string | 31.9 ms | 45.2 ms | **+13.3 ms**, ≈13 ns per slice |
| 200,000 strings through a `Strs` | 45.9 ms | 50.3 ms | +4.4 ms |
| 1,000,000 sequences | 63.5 ms | 64.3 ms | noise |
| 4,000,000 pushes into one sequence | 46.0 ms | 46.0 ms | — |
| 50,000 channels | 38.5 ms | 36.9 ms | noise |
| 200,000 values through one channel | 49.8 ms | 48.6 ms | noise |
| sieve to 2,000,000 | 47.0 ms | 48.1 ms | noise |

The cost is where the ownership is and nowhere else. A string operation pays an atomic
adjustment and, for a heap string, a `free`; a sequence pays nothing new; arithmetic and
indexing pay nothing at all. **13 ns for a slice is the honest price of N8's central
decision** — a slice retains the buffer it points into, and that is what lets a slice
outlive the binding of the buffer it was cut from.

Executable size: **+512 bytes** for a program that touches a string, +352 more for one that
touches a channel, against ~50 KB.

### The compiler process, and the programs it emits

Two different questions, so two measurements. Both inside `nazm-contained:1.98.1`, Linux
aarch64, so the two columns are comparable with each other and not with the table above.

| | Before | After |
| --- | ---: | ---: |
| the **self-hosted** compiler compiling `emit.nz` — the compiler *process* | 52.61 MiB | **24.86 MiB** |
| a program that compiler emitted: 80,000 temporary strings | 6.04 MiB | **1.17 MiB** |
| the **reference** compiler compiling `emit.nz` | 98.82 MiB | 107.61 MiB |

The first row is the milestone's own dogfood: the Nazm-written compiler is a string-heavy
program, so reclaiming strings halves its peak. The second is the question §34 of the
directive keeps separate from it — what the emitted program costs, which is not the same
question and does not have the same answer.

The third row went **up 8.9%**, and that is a real cost rather than noise. The reference
compiler holds the whole LLVM text it is building, the emitted IR now carries retain and
release calls and the string runtime, and a bigger text is a bigger buffer. It is the price
of the emitted program's 5× and it is recorded rather than left for a reader to find.

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
