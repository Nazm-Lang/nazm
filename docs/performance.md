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
what each one's implementation weighs against what its published interface weighs — the
schema was `nazm.interface/1` when this was measured, `/2` since N9 and `/3` since N10, each
of which added definitions — records, then enums — that move these numbers for a module
exporting one:

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
reclaiming two more kinds of value should do to the first. The genuinely live sequence, the
channel handoff and the empty program are the controls — memory that is *in use* does not
move, and neither does the floor.

**`str_concat` in a loop is still quadratic in time.** Nothing here changes that, and
`str_join` is still one pass and one allocation. What changed is that getting it wrong now
costs time rather than the machine.

### Records — measured 2026-09-23, and the cost is the fields'

N9 added user-defined records. A record allocates nothing of its own, so the question is
whether the *composition* costs anything: whether reading a field is slower than reading a
binding, and whether copying a record is slower than copying what it holds.

Median of five, `-O2`, Darwin arm64. Each loop runs three million times except the two
marked, which run one million.

| Workload | Without a record | With one | |
| --- | ---: | ---: | --- |
| two `Int`s built and read | 34 ms | **34 ms** | not measurable |
| a record inside a record, three fields read | — | **36 ms** | +2 ms over the flat case |
| three million field replacements | — | **32 ms** | |
| copying a `Str` twice, one million times | 55 ms | **54 ms** | not measurable |
| a cross-module call returning two values | 43 ms | **40 ms** | one call instead of two |

The last row is the shape `compiler/emit.nz` actually adopted: `line_of` and `column_of`
were two functions because a function returns one value, and `Place { line, column }` is
one. The measured win is small and the reason it exists is not — the second scan ran
*backwards* over the same bytes.

At `-O0`, where the generated retain and release helpers are real calls rather than
inlined, the cost appears and is still small:

| Workload | Without | With | |
| --- | ---: | ---: | --- |
| two `Int`s built and read | 39 ms | 41 ms | +2 ms over three million |
| a record inside a record | — | 49 ms | +10 ms; two aggregates per iteration |
| copying a `Str` twice | 55 ms | 56 ms | +1 ms over one million |

**Peak resident memory**, same platform, `-O2`:

| Workload | Live set | Peak |
| --- | --- | ---: |
| 80,000 short-lived records of two heap strings | one record | **1.72 MiB** |
| the same two strings, no record | one string | 1.73 MiB |
| 80,000 temporary strings — the N8 control | one string | 1.77 MiB |
| 80,000 short-lived sequences — the N7 control | ~400 B | 1.78 MiB |
| 2,000 short-lived channels — the N8 control | one | 1.75 MiB |
| empty program | — | 1.69 MiB |

The first two rows are the claim: a record holding heap strings costs what the strings
cost. The next three are the controls, and they are unmoved — adding a composite type did
not disturb the reclamation of the types it composes.

**Executable size.** A program using a record whose fields own nothing is **byte-identical**
in size to the same program written without one (50,696 bytes), because no helper is
generated. One whose fields own something costs **+96 bytes** — the retain and release
helpers, once each, however many times the record is copied.

**The compiler itself**, Darwin arm64 host: `nazm check compiler/emit.nz` 42 ms,
`nazm build … --opt-level 0` 143 ms, peak 44.75 MiB.

Contained, Linux aarch64, on `compiler/emit.nz` — now 3,756 lines:

| | Peak | Wall |
| --- | ---: | ---: |
| the reference compiler | 112.94 MiB | 1.01 s |
| the compiler written in Nazm | **32.75 MiB** | 4.01 s |

N8 recorded 107.61 MiB and 24.86 MiB for the same pair, **on a 3,221-line file**, so these
are not a controlled comparison and are not presented as one. The reference compiler's
figure moved about as much as the input did; the self-hosted one moved more, and the reason
is visible rather than mysterious — its node arena gained two columns and it now carries a
record table of its own. What is being claimed here is only that neither grew unexpectedly,
and that both still fit the 4,096 MiB the runbook allows them.

### Enums — measured 2026-09-23, and the cost is a branch and some space

N10 added closed sum types. An enum allocates nothing of its own either, so there are two
questions: what a value **costs in space**, because the representation is not a union; and
what selecting on one **costs in time**, because copy and release now branch.

Median of five, extremes dropped, `-O2`, Darwin arm64. The process floor on this machine is
**6.5 ms**, and a row at or under it is not a measurement.

**Space, computed by LLVM's own layout** for the types the compiler emits. The right-hand
column is what an ideal tagged union would be — the tag plus the largest payload — and the
gap is the cost of one slot per variant:

| Enum | Ours | Ideal union | |
| --- | ---: | ---: | --- |
| `{ A, B, C(n: Int) }` | 16 | 16 | zero-payload variants are size 0 |
| `{ Empty, Text(value: Str) }` | 32 | 32 | one owning variant costs nothing extra |
| `{ A(v: Str), B(v: Str), C(v: Str) }` | **80** | 32 | 2.5× |
| `{ Nothing, Rec(r: Big), Small(n: Int), Wide(a: Str, b: Str) }` | **120** | 64 | 1.9× |
| the record `Big { a: Str, b: Str, c: Int }`, for scale | 56 | 56 | |

**So the waste is exactly proportional to how many variants carry a payload**, and it is
nothing at all when one does. `architecture.md` §7.11 says why this form was chosen over a
sized payload area — an arithmetic mistake in the second is a buffer overflow rather than a
compile error — and `spec.md` promises nothing about an enum's size, so it can change.

**Time**, `-O2`:

| Workload | Without an enum | With one | |
| --- | ---: | ---: | --- |
| 30M selections on a tag | 26.3 ms (an `Int` and an `if`) | **26.2 ms** | not measurable |
| 3M of the same | 5.4 ms | 5.2 ms | both under the floor |
| 1M × build, copy twice, read a `Str` | 71.5 ms | **81.4 ms** | +10 ns per round |
| 1M of the same over a record payload | 71.5 ms | **81.5 ms** | +10 ns per round |
| 3M cross-module calls | 15.6 ms (two `Int` calls) | **11.9 ms** | one call instead of two |

A scalar enum costs nothing a hand-rolled integer tag does not. An enum that **owns**
something costs about ten nanoseconds a round more than the payload alone: a branch on the
discriminant, and a larger aggregate to move. The last row is the same shape records
produced and for the same reason — returning one value instead of making two calls.

At `-O0`, where the generated helpers are real calls rather than inlined, the cost appears
and is the branch:

| Workload | Without | With | |
| --- | ---: | ---: | --- |
| 3M selections on a tag | 19.7 ms | 23.3 ms | +1.2 ns per round |
| 1M × copy a `Str` twice | 78.6 ms | 104.6 ms | +26 ns per round |
| 1M of the same over a record payload | 83.2 ms | 117.9 ms | +35 ns per round |

**Peak resident memory**, `-O2`:

| Workload | Live set | Peak |
| --- | --- | ---: |
| 80,000 short-lived enums of two heap strings | one value | **1.73 MiB** |
| 80,000 short-lived records of the same two strings | one record | 1.72 MiB |
| empty program | — | 1.67 MiB |

That is the scaling claim: a value holding heap strings costs what the strings cost, and an
enum costs what the same record does. The count of live backings does not grow with the
number of values ever built.

**Executable size**, `-O0`, where the helpers survive. An enum whose payloads own nothing is
**byte-identical** to the same program written with an integer tag and an `if` (51,480
bytes both). One that owns something costs **+144 bytes** over the same program without it,
against a record's +120 — the difference is the `switch` the enum's two helpers carry.

**The compiler itself**, Darwin arm64 host, release binary, on a `compiler/emit.nz` that
grew from 3,756 lines to 4,276 (and to 4,359 with what a `break` gives back): `nazm check --no-cache` 10 ms, peak 12.6 MiB; `nazm build
--no-cache --opt-level 0` 720 ms, peak 57.7 MiB, and 120 ms with a warm object cache. These
are not comparable with N8's or N9's figures for the same command — different binary and
different cache state — and are recorded as a baseline rather than as a delta.

### Generics and `Vec[T]` — measured 2026-09-23, and the cost is the instances

N11 added parametric generics, monomorphised by `nazm build`, and one generic container.
Measured **contained, Linux aarch64** (`nazm-contained:1.98.1`, one CPU), release compiler,
median of five by a nanosecond clock; these are not comparable with the Darwin rows above,
and the process floor here is about **1.3 ms**. The programs are `target/n11/perf/gen.py`.

**`Vec[Int]` against `Ints`, `Vec[Str]` against `Strs`** — 3M push, get-and-set and pop of
`Int`s; 1M of the same over heap strings, each set taking another element's string:

| Workload | `Ints` / `Strs` | `Vec[T]` | |
| --- | ---: | ---: | --- |
| 3M `Int`s, `-O2` | 19.5 ms, 24.0 MiB | **19.4 ms**, 24.7 MiB | the same code |
| 3M `Int`s, `-O0` | 60.5 ms | 59.1 ms | |
| 1M strings, `-O2` | 156.2 ms, 69.7 MiB | **151.3 ms**, 70.4 MiB | within noise |
| 1M strings, `-O0` | 159.6 ms | 162.6 ms | |

A `Vec[T]` is the sequence runtime with the element's layout, and it costs what the
sequence it generalises costs. The stride is asked of LLVM, and at `-O2` it folds.

**Peak resident memory, short-lived containers against a constant live set**, `-O2`:

| Workload, per iteration | 5,000 | 20,000 | 80,000 |
| --- | ---: | ---: | ---: |
| a `Vec[Str]` of two heap strings | 1.17 MiB | 1.17 MiB | 1.17 MiB |
| a `Vec[Row]`, `Row { name: Str, n: Int, tags: Ints }` | 1.17 MiB | 1.17 MiB | 1.17 MiB |
| a `Vec[Tok]` of a string, an `Int` and a zero-payload variant | 1.17 MiB | 1.17 MiB | 1.17 MiB |
| a `Vec[Vec[Str]]` holding one inner vector | 1.17 MiB | 1.17 MiB | 1.17 MiB |
| empty program | 1.16 MiB | | |

Every row reports `live=0` for sequences and strings. What a program ever built does not
reach its peak; what it holds does.

**Enum pressure** — the first workload where an enum's size is multiplied. 200,001 tokens
of `enum Tok { Word(text: Str), Num(v: Int), Punct(c: Int), End }` in a `Vec[Tok]`, against
the same tokens as three parallel arrays (`Ints` kind, `Ints` number, `Strs` text), built
and then read by a `match`:

| | parallel arrays | `Vec[Tok]` | |
| --- | ---: | ---: | --- |
| bytes per token | 40 (8 + 8 + 24) | **48** (tag 8, `Num` 8, `Punct` 8, `Word` 24, `End` 0) | +20% |
| peak RSS, `-O2` | 11.8 MiB | **14.0 MiB** | +18% |
| time, `-O2` | 12.3 ms | 12.5 ms | not measurable |

The slot-per-variant layout N10 chose costs 8 bytes a token here, a fifth; an ideal union
would be 32. `spec.md` promises nothing about the size, so a compact representation stays
possible — and is a separate milestone, not an N11 optimisation.

**Monomorphisation's growth** — one generic function, `twice[T]`, instantiated for 1, 10 and
100 distinct records:

| Instances | `nazm check` | cold build | warm build | instance units compiled cold / warm | IR | cache | executable |
| ---: | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| 1 | 2.2 ms | 0.11 s | 0.04 s | 1 / **0** | 19 KB, 4 units | 12 KB | 72,496 B |
| 10 | 1.9 ms | 0.27 s | 0.04 s | 10 / **0** | 75 KB, 13 units | 41 KB | 74,360 B |
| 100 | 2.7 ms | 1.79 s | 0.05 s | 100 / **0** | 637 KB, 103 units | 324 KB | 93,008 B |

**Checking does not grow**: a generic body is checked once, however many instances there
are. **Code generation grows linearly**: about 6.2 KB of IR, 3.2 KB of cached object, 206
bytes of executable and 17 ms of cold build per instance, almost all of it one `clang -c`
each. A warm build compiles **no** instance, and the build report says so
(`native.instances.compiled = 0`). An instance symbol names its definition and its
arguments' canonical keys, which makes it long — 60 bytes for `twice[R7]` in a module named
`inst_10.nz` — and injective. No cross-instance deduplication, polymorphisation or LTO was
added; this is the measurement a later milestone would start from.

**The compiler itself**, same host: `nazm check --no-cache compiler/emit.nz` 45.5 ms, peak
20.5 MiB; `nazm build` cold 1.68 s, peak 119 MiB, warm **0.15 s**.

**The dogfood** — the Nazm-written compiler's diagnostics moved from four parallel arrays to
one `Vec[Diag]`. C1 compiling `compiler/emit.nz`, three runs each:

| | four arrays | `Vec[Diag]` |
| --- | ---: | ---: |
| source lines, `compiler/*.nz` | 11,885 | 11,877 |
| wall | 10.31–10.36 s | 10.22–10.27 s |
| peak RSS | 54.1–56.3 MiB | 51.7–52.3 MiB |
| emitted IR | 6,000,902 B | 5,974,992 B |
| C2 executable | 1,724,704 B | 1,724,800 B |

The same output, a little less IR and memory, and no claim about speed: 1% of wall time is
inside the noise of one run to the next. What the migration bought is the invariant — a code
cannot be pushed without its message — not a number.

### Typed errors — measured 2026-09-24, and `?` costs what the `match` it means costs

N12 added `Result`, `Option` and `?`. Measured **contained, Linux aarch64**
(`nazm-contained:1.98.1`, one CPU), release compiler, median of five by a nanosecond clock,
as for N11. The programs are `target/n12/perf/gen.py`; the raw output is
`target/n12/perf/results.txt`.

**Layout** — read off the emitted IR. An enum is one slot per variant (N10's choice), so a
`Result` carries both payloads:

| Type | bytes | an ideal tagged union | |
| --- | ---: | ---: | --- |
| `Result[Int, Int]` | 24 | 16 | tag 8, `Err` 8, `Ok` 8 |
| `Result[Str, Str]` | 56 | 32 | two 24-byte strings |
| `Result[Large, Small]`, a 72-byte record and an 8-byte one | 88 | 80 | the small variant's size is the overhead |
| `Result[Int, Vec[Diag]]` | 24 | 16 | a `Vec` is one pointer |
| `Option[Int]` | 16 | 16 | `None` is empty |
| `Option[Str]` | 32 | 32 | |
| `Option[Rec]`, a 32-byte record | 40 | 40 | |

`Option` costs nothing over a tagged union, because `None` has no payload; `Result` costs the
smaller variant. No niche is used anywhere — `Option[Str]` is not a null pointer — and none is
promised; a compact representation is a later milestone that can start from this table.

**Explicit `match` against `?`** — the same function spelled both ways, 20,000,000 calls,
each taking apart a `Result[Int, Oops]` from a leaf and returning its own:

| | `match` | `?` |
| --- | ---: | ---: |
| success, `-O2` | 21.3 ms | 21.3 ms |
| error, `-O2` | 19.4 ms | 19.2 ms |
| success, `-O0` | 284.2 ms | 288.7 ms |
| error, `-O0` | 353.0 ms | 350.0 ms |

**No difference**, which is the expected result: `?` lowers to the `match` (`architecture.md`
§7.13), and every gap above is inside run-to-run noise. At `-O0` a propagation is about 14 ns
of the loop; at `-O2` the leaf and the `match` fold.

**Propagation depth** — a static chain `f100 → f99 → … → f0`, each level `f(k-1)(…)? + 1`,
the top called 1,000,000 times:

| Depth | `nazm check` | `nazm build` | IR | executable | success, `-O0` | error, `-O0` |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1.5 ms | 72 ms | 23 KB | 71,648 B | 15.0 ms | 18.4 ms |
| 10 | 1.7 ms | 81 ms | 69 KB | 72,128 B | 80.5 ms | 92.5 ms |
| 100 | 3.2 ms | 135 ms | 537 KB | 77,088 B | 1,070 ms | 1,197 ms |

Linear throughout: about 5.1 KB of IR, 55 bytes of executable and 0.6 ms of build per level,
and 10–12 ns per level per call unoptimised. The chain is static, so the stack is the depth of
ordinary calls and nothing more; an error at depth 100 travels a hundred returns and costs a
little more than a success because each return releases the `Result` it took apart.

**Code per generic instance** — `pass[T, E](r: Result[T, E]) -> Result[T, E]` instantiated for
1, 10 and 50 records, once returning `r` and once written `let v = r?; Result[T, E].Ok(value: v)`:

| Instances | IR, plain | IR, with `?` | executable, plain | executable, with `?` |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 29 KB | 39 KB | 73,056 B | 73,072 B |
| 10 | 185 KB | 282 KB | 78,224 B | 78,456 B |
| 50 | 886 KB | 1,379 KB | 166,880 B | 233,608 B |

From 10 to 50 instances a plain instance costs 17.5 KB of IR and 2.2 KB of executable, one with
`?` 27.4 KB and 3.9 KB: the propagation's two blocks, its tag test and the error path's
release of the frame are duplicated in **every** instance, as monomorphisation duplicates
everything. No cross-instance deduplication was added. A warm rebuild of the 50-instance
program compiles **0** units and **0** instances (`native.instances.compiled = 0`).

**The dogfood** — the Nazm-written compiler's front end as one stage returning
`Result[Checked, Vec[Diag]]`, with `emit_program` beginning `check_program(…)?`. C1 (built by
the reference) compiling `compiler/emit.nz`, three runs each, against the same compiler with
the migration mechanically undone (`target/n12/dogfood/`):

| | before | after |
| --- | ---: | ---: |
| source lines, `compiler/*.nz` | 12,227 | 12,216 |
| `check.nz` / `emit.nz` / `analyse.nz` | 171 / 5,236 / 4,409 | **64** / 5,174 / 4,567 |
| front-end pipelines written out | 2 | **1** |
| drivers testing `vec_len(diags) > 0` to decide whether to continue | 2 | 1 (`check.nz`'s output) |
| driver lines threading the diagnostic sink, `check.nz` + `emit.nz`'s `main` | 12 + 12 | 4 + 0 (the four print it) |
| tables declared by `check.nz` | 57 | 7 |
| wall | 10.93–10.97 s | 11.04–11.09 s |
| peak RSS | 55.4–56.8 MiB | 54.6–55.4 MiB |
| emitted IR | 6,332,226 B | 6,360,949 B |
| C2 executable | 1,987,168 B | 1,987,712 B |
| C2 compiling itself | 10.97 s | 11.03 s |

What moved is the structure: the front end exists once, and `check.nz` lost two-thirds of its
lines. The cost is 0.45% more IR and about 1% more time — consistent across the runs, and too
small to attribute with one session's measurements; no speed claim is made in either
direction. The C2 and C3 of the migrated compiler emit identical IR.

### Value blocks and the first `Option` in the compiler — measured 2026-09-24

N12.1 made a block used as a value compile natively and moved one compiler table from a `-1`
sentinel to an `Option[Int]`. Measured **contained, Linux aarch64** (`nazm-contained:1.98.1`,
one CPU), release compiler, median of five, as for N12. Programs: `target/n121/perf/gen.py`;
raw output: `target/n121/perf/results.txt` and `target/n121/dogfood/results.txt`.

**One `match` arm written four ways**, 20,000,000 calls of a function that builds a
two-variant enum (an `Int` payload or a `Str` one) and takes it apart:

| arm | `-O2` | `-O0` | IR | `pick` in IR |
| --- | ---: | ---: | ---: | ---: |
| `=> n + 1` — a direct expression | 510.0 ms | 743.3 ms | 23,377 B | 129 lines |
| `=> { n + 1 }` — the same, as a block | 514.8 ms | 738.9 ms | 23,287 B | 129 lines |
| `=> { let m = n + 1; m }` — one local | 506.5 ms | 773.4 ms | 23,487 B | 135 lines |
| `=> { let t = s0; n + str_len(t) }` — an owning local | 847.1 ms | 1,053.4 ms | 24,804 B | 161 lines |

**A block costs nothing over the expression it contains**: the first two rows lower to the
same instructions, and every gap between them is noise. A local costs a store and a load at
`-O0` — about 1.5 ns a call — and nothing at `-O2`, where the slot is promoted. The owning
row does more work, not the same work more slowly: binding a `Str` local takes a reference
and the frame gives it back, and the arm calls `str_len`, so the extra ~17 ns a call is that
pair and that call. No claim is made that blocks are free in general: that is measured for
these shapes, and a block whose locals own something pays for what it owns.

**Arms per `match`** — an enum of N one-field variants and one `match` over it, each arm a
direct expression or a block with one local:

| N | IR, direct | IR, blocks | executable, direct / blocks | `nazm build` | `nazm check` |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 11,672 B | 11,744 B | 71,600 B / 71,600 B | 68 / 71 ms | 1.6 / 1.5 ms |
| 10 | 19,443 B | 20,391 B | 71,608 B / 71,600 B | 73 / 71 ms | 1.6 / 1.6 ms |
| 100 | 100,756 B | 110,935 B | 71,632 B / 71,632 B | 94 / 102 ms | 2.2 / 2.6 ms |

Linear: a block arm with a local costs about 100 bytes of IR more than the direct arm — the
local's store and load — and nothing in the executable at these sizes. No pathological
growth.

**The `Option` dogfood** — `record_owned` returning `Option[Int]` instead of `-1`. C1 (built
by the reference) compiling `compiler/emit.nz`, three runs each, the N12 script unchanged;
*before* is N12.1's compiler with the migration undone:

| | before | after |
| --- | ---: | ---: |
| guards testing a `record_owned` result for `>= 0` | 4 | **0** |
| sites taking a `record_owned` result apart | 4 `if`s | 4 `match`es and `applies_core_result` |
| `analyse.nz` lines / non-comment lines | 4,580 / 3,613 | 4,598 / 3,622 |
| wall | 11.03–11.11 s | 10.91–10.95 s |
| peak RSS | 53.4–55.5 MiB | 53.2–55.3 MiB |
| emitted IR | 6,366,359 B | 6,361,900 B |
| C2 executable | 1,987,872 B | 1,922,368 B |
| C2 compiling itself | 10.98 s | 10.94 s |

What moved is the meaning: "none" is a variant, so no comparison of a declaration with a
definition can see it. The code is 0.07% smaller in IR; the time and memory differences are
within one session's run-to-run spread, and no speed claim is made. The executable size
moves by one 64 KiB page between runs of unrelated compilers here — N12's own compiler,
measured in the same session, gave 1,922,176 B — so it is not attributed to the migration.
C2 and C3 of the migrated compiler emit identical IR.

### Derived equality — measured 2026-09-25, and at `-O2` it is the fields' comparisons

N13 made records, enums and their generic instances comparable, each through a helper the
emitter generates per compared type. Two questions: what a comparison costs against the
scalar comparisons it stands for, and what the helpers cost in code.

**Time.** Contained (Linux aarch64, 1 CPU), median of five, one comparison per iteration with
operands built from the loop counter so nothing is loop-invariant; 10M iterations at `-O0`,
30M at `-O2`. The rows are the programs in `target/n13/bench/` at the N13 commit.

| Workload | `-O0`, 10M | `-O2`, 30M |
| --- | ---: | ---: |
| `Int` baseline — `(i % 8) == ((i / 2) % 8)` | 56 ms | 14 ms |
| `Str` baseline — two literals chosen by parity | 157 ms | 176 ms |
| record `{ x, y: Int }`, equal a quarter of the time | 71 ms | 15 ms |
| record, unequal in the first field in layout order | 31 ms | 0 ms — folded |
| record, unequal in the last field | 60 ms | 14 ms |
| nested record (two records and an `Int`) | 94 ms | 14 ms |
| enum, different variants | 57 ms | 0 ms — folded |
| enum, same variant, payload compared | 78 ms | 15 ms |
| enum, same variant, payload always unequal | 38 ms | 0 ms — folded |
| `Option[Int]`, both `Some` | 78 ms | 14 ms |
| `Result[Int, Str]`, both `Ok` | 88 ms | 15 ms |

At `-O2` the helpers are inlined and a composite comparison costs what its field comparisons
cost: every row that compares is within a millisecond of the `Int` baseline over 30M, and
the rows LLVM can decide from the construction — different variants, a first field that
always differs — are folded away with the loop. **At `-O0`** the helper is a real call: about
**1.5 ns** per comparison over the baseline for a two-field record, 3.8 ns for the nested
one, 2.2 ns for an enum's tag and payload, and 3.2 ns for a `Result` — and less than the
baseline where the first comparison decides (the unequal-early record and different-variant
enum stop there). The `Str` rows are the existing `memcmp` path and are not changed by N13.
No zero-cost claim is made for `-O0`.

**Code.** One helper per compared record or enum per unit, generated only when a comparison
reaches it — a type nobody compares carries none — and one per instance: `Box[Int]` and
`Box[Str]` are two. A nested type's helper calls its fields' helpers (the nested row above
has two). Against the `Int` baseline program, the two-field record's helper adds 1,011 bytes
of unoptimised IR and 56 bytes of executable at `-O0`; the enum's adds 2,293 bytes of IR; a
`Result[Int, Str]`'s, which compares a `Str` arm too, 2,828. At `-O2` every executable is
within 24 bytes of the baseline's 72,928. **Across units** a helper is `internal` and
duplicated: a record compared in two modules is emitted once in each (measured: one helper
in each of the two units, none in the entry or runtime units). Nothing is exported, so no
symbol is promised.

**The dogfood** — the compiler's completion states as an enum, compared with `==` —
measured by the bootstrap of record before and after, contained:

| | before | after |
| --- | ---: | ---: |
| `analyse.nz` lines | 4,662 | 4,678 |
| C2 IR | 6,486,154 B | 6,488,724 B (+2,570, +0.04%) |
| C2 executable | 1,922,560 B | 1,922,496 B (−64) |
| C2 compiling `emit.nz`, median of 3 | 11.64 s, 54.2 MB peak | 11.65 s, 54.0 MB peak |
| C2 compiling `check.nz`, median of 3 | 3.30 s, 25.8 MB peak | 3.29 s, 25.8 MB peak |

Every difference is within noise; none is claimed. What changed is what a completion can be:
three variants rather than any `Int`, and "no arm yet" an `Option` rather than `-1`.

### Equality requirements — measured 2026-09-25, and the requirement itself costs nothing

N14 lets a generic function require equality of `T`. A requirement is a fact for the checker,
so the questions are what it costs to check and publish, and whether anything of it reaches
the program. Contained (Linux aarch64, 1 CPU), median of five; programs in
`target/n14/bench/` at the N14 commit.

**Checking**, release `nazm check --no-cache` on 200 generic functions each called once:
bounded and compared, 4 ms; the same 200 as concrete `Int` functions, 4 ms; unbounded, 4 ms;
200 refused calls, 5 ms. All at the process floor — no cost is measurable, and none is
claimed either way.

**The interface.** `pub fn same[T](…)` publishes 226 bytes; `pub fn same[T: Equality](…)`
250 — the 24 bytes of `"requires_equality":[0],`. Renaming `T` to `Value` gives identical bytes
and an identical `InterfaceHash`; adding the requirement moves it. A function that requires
nothing is written exactly as before the field existed.

**The program.** One comparison per iteration, operands built from the loop counter:

| Workload | `-O0`, 10M | `-O2`, 30M | exe `-O2` |
| --- | ---: | ---: | ---: |
| hand-written `same_int(a: Int, b: Int)` | 70 ms | **15 ms** | 72,936 B |
| unbounded generic call at `Int` (`keep[T]`) | 88 ms | 50 ms | 73,072 B |
| bounded `same[T: Equality]` at `Int` | 72 ms | 50 ms | 73,048 B |
| … at a two-field record | 82 ms | 50 ms | 73,096 B |
| … at an enum, same variant | 91 ms | 54 ms | 73,088 B |
| … at `Box[Int]` | 83 ms | 50 ms | 73,096 B |
| … at `Str` (the `memcmp` path) | 180 ms | 342 ms | 73,120 B |

**The requirement adds nothing**: a bounded instance runs as fast as an unbounded generic
call, and its IR body is the concrete function's instruction for instruction
(`a_requirement_leaves_no_trace_in_the_program`) — no dictionary, descriptor or test exists.
**What does cost is genericity, and it predates N14**: an instance is defined in a unit of its
own and is external to every caller (`architecture.md` §7.5), so at `-O2` the call is not
inlined and `same[Int]` costs 50 ms where an inlined `same_int` costs 15. That is recorded as
the cross-unit-inlining item in `roadmap.md`, not as a cost of requirements. The `Str` row is
slower at `-O2` than at `-O0` over three times the iterations for the same reason — the call
survives — and is not a regression of N14.

**Dogfood**: none. No compiler code uses a requirement, so the bootstrap's size and speed
moved only by the checker's own new code (`bootstrap.md`).

### The lossless syntax tree — measured 2026-09-25, and it doubles the parse

N15 builds a concrete tree on every parse, so parsing now keeps what it used to discard:
every byte of trivia, every punctuation token, and a node per construct. Measured in
process, because a parse is below process-launch noise: the committed pre-N15 parser
(`git show a129cae`'s `nazm-syntax`, abstract tree only) against the N15 parser on the same
text, release build, median of 30 parses after 3 warm-ups, heap from a counting allocator.
Contained, P1, nothing else running (`target/n15/bench`, not committed).

| Input | Bytes | Leaves / nodes | pre-N15 | N15 `parse_file` | N15 `parse_cst` | Peak heap, pre-N15 → N15 |
| --- | --- | --- | --- | --- | --- | --- |
| `compiler/*.nz`, concatenated | 606,088 | 136,056 / 56,847 | 14.3 ms | 28.9 ms (**2.0×**) | 29.0 ms | 15.8 MB → 29.2 MB (1.8×) |
| the same, every 40th token removed | 599,185 | 133,089 / 9,912 | 5.1 ms | 15.7 ms (3.1×) | 14.8 ms | 9.6 MB → 19.1 MB |
| `examples/sum.nz` | 550 | 197 / 70 | 13.5 µs | 36.2 µs | 34.4 µs | 15 KB → 38 KB |
| `library/core/prelude.nz` | 901 | 83 / 23 | 2.8 µs | 14.2 µs | 12.8 µs | 6 KB → 20 KB |

**What the numbers say.** On real code the tree costs about as much as the parse did before
it: 2.0× the time and 1.8× the peak heap, for a tree that holds 2.7 MB of a 606 KB source.
The malformed input is 3.1× because the pre-N15 parser did less work there, not because
recovery is slow: it abandoned every broken body, and N15 parses the rest of each. The two
small files show a fixed cost of about 10 µs — the builder and its interner — which is
invisible in any command but is most of their ratio. `parse_file`, which the compiler uses,
builds the tree and drops it; a parse that built only one tree would be a second path
through the grammar, which N15 does not have.

**Where it does not show.** Parsing is a small part of any command: the contained selfhost
suite, which parses the compiler written in Nazm many times over, took 42.8 s of test time
against 37.2 s at N14.1's final run (contained, P4T4, one run each): +15%, which is the tree's
cost where parsing is a large share of the work. The workspace suite took 150 s against
118 s, with 27 more tests in it. The release `nazm` binary grew from 2,312,000 to 2,397,984 bytes
(+3.7%), and the workspace gained ten locked crates, nine of them `cstree`'s: `lock_api`,
`parking_lot`, `parking_lot_core`, `rustc-hash`, `scopeguard`, `stable_deref_trait`,
`text-size`, `triomphe`, and `redox_syscall`, which is only resolved for that target.

One cost was removed after it was measured: the first version copied every token — each
identifier's and string's text — out of the lossless scan for the parser to read. Borrowing
them took the compiler's parse from 33.6 ms and 34.6 MB to the figures above.

### The language service — measured 2026-09-25, and it is fast enough without incremental reparse

`nazm lsp` re-analyses every open document in full on every change (`architecture.md`
§7.18). Whether that is acceptable is a latency question, measured rather than assumed: in
process against `nazm-service` for everything but startup, and against the real release
binary for startup. Release build, contained, P1 — one CPU — nothing else running; medians
(`target/n16/bench`, not committed).

| What | Small file (`examples/sum.nz`, 550 B) | The compiler written in Nazm (`compiler/emit.nz` and its imports, 12,543 lines) |
| --- | --- | --- |
| cold start: spawn to `initialize` response | 0.8 ms (10 runs) | — |
| cold open → diagnostics, new service | 0.3 ms | 76.8 ms (7) |
| warm edit → diagnostics, same process | 0.2 ms | 79.2 ms (7) |
| definition query | 7.5 µs | 1.86 ms |

And on the compiler: a syntax error in the middle of `emit.nz`, cold open → diagnostics,
77.4 ms, two diagnostics; a signature edit in `lex.nz` with both `lex.nz` and `emit.nz` open,
82.5 ms to both documents' diagnostics. Resident memory: 1.9 MB at start, 44.2 MB with the
compiler open, 46.3 MB after 10 edits and 46.3 MB after 100 — flat, with one analysis and
0.89 MB of source text retained whatever the edit count.

**What the numbers say.** A warm edit costs what a cold open does: nothing is reused
between versions except the process, and the process was never the cost — startup is under
a millisecond. On the largest program there is, an edit is answered in about 80 ms on one
CPU; on anything a person edits by hand it is below a millisecond, which is below the noise
of any editor's round trip and is reported as measured rather than as a percentage. The
definition query walks the document's leaves to find the token, which is the 1.9 ms on the
compiler. None of this makes a case for incremental reparse yet; a file much larger than
the compiler, or a workspace with many large documents open, would be the evidence.

**What it added to the binary.** The release `nazm` grew from 2,397,984 to 2,768,592 bytes
(+370,608, +15.5%), almost all of it the protocol's message types; the workspace gained six
locked crates — `lsp-server`, `lsp-types`, `crossbeam-channel`, `crossbeam-utils`,
`fluent-uri`, `serde_repr` — all pure Rust. That was the measurement the choice between
`nazm lsp` and a separate `nazm-lsp` binary waited for, and a third of a megabyte did not
earn a second executable every editor configuration would have to find: the server is
`nazm lsp`.

### The reference index — measured 2026-09-25, and it does not change the incremental-reparse answer

N17 builds one reference index per analysis (`architecture.md` §7.19) and rebuilds it with the
analysis on every change. Measured as N16 was: in process against `nazm-service`, release,
contained, P1 — one CPU — nothing else running; medians (`target/n17/bench`, not committed).
Index construction is timed apart from analysis, and a query apart from both.

| What | Small file (`examples/sum.nz`) | The compiler (`compiler/emit.nz` and its imports) |
| --- | --- | --- |
| parse and check (`nazm_core::analyse`) | 0.08 ms | 32.3 ms |
| **index construction** (`ReferenceIndex::of`) | 0.006 ms | **3.85 ms** |
| cold open → diagnostics | 0.2 ms (N16: 0.3) | 80.3 ms (N16: 76.8) |
| warm edit → diagnostics and a new index | 0.2 ms (N16: 0.2) | 81.4 ms (N16: 79.2) |
| definition query | 7.1 µs (N16: 7.5) | 1.72 ms (N16: 1.86) |
| references query | 8.0 µs | 1.76 ms (a function, 62 uses); 1.76 ms (a local of the emitter) |
| cross-file references, three compilations open | — | 2.27 ms (`t_int`, 62 uses) |

The compiler's index: 21,884 occurrences, 3,752 entities, 18,132 uses. Resident memory: 1.9 MB
at start, 48.1 MB with the compiler open (N16: 44.2), 50.2 MB after 10 edits and 50.2 MB after
100 (N16: 46.3 both) — flat, with one analysis and one index retained whatever the edit count.

**What the numbers say.** The index costs about 12% of a check and under 5% of an edit on the
largest program there is — 3.9 ms of an 81 ms warm edit, which is inside the run-to-run spread
of the N16 figures it is compared with. A query costs what finding the token does: the 1.7 ms
is the leaf walk definition already paid, and the reverse lookup itself is a hash probe and a
copy of the uses. About 4 MB more is resident with the compiler open, the index's spans.
Nothing here makes a case for incremental reparse or for incremental index maintenance: a
complete rebuild with the analysis is cheap, and correct by construction.

**What it added to the binary.** The release `nazm` grew from 2,768,592 to 2,823,040 bytes
(+54,448, +2.0%); no dependency was added.

### Rename — measured 2026-09-25

N18 adds type-name occurrences to every index and a rename that re-analyses its candidate
program. Measured as N17 was: in process, release, contained, P1, nothing else running;
medians (`target/n18/bench`, not committed). Planning and validation are one call, so the
validation's share is stated from its parts: a rename re-runs one full analysis per open
compilation that contains the edited file.

| What | Small (`sum_to` in `examples/sum.nz`) | The compiler (private `nl` in `emit.nz`) | Cross-file (private `is_digit` in `lex.nz`, `lex.nz` and `emit.nz` open) |
| --- | --- | --- | --- |
| prepareRename | 12.2 µs | 2.25 ms | 175 µs |
| rename: plan, candidate re-analysis, partition comparison | 0.3 ms (2 edits) | **93.3 ms** (17 edits, 1 compilation) | 97.8 ms (4 edits, 2 compilations) |

And against N17's figures, on the compiler: index construction 3.91 ms (N17: 3.85), now 22,088
occurrences (N17: 21,884 — the difference is the written type names); warm edit 83.3 ms (N17:
81.4); references 1.78 ms (N17: 1.76); cross-file references 2.17 ms (N17: 2.27); definition
1.81 ms. Resident memory: 48.1 MB with the compiler open (N17: 48.1), 48.6 MB after 10 edits
and 50.5 MB after 100 (N17: 50.2 at both) — one sample each, with retained documents, analyses,
text and index sizes constant (`retained()`, `indexed()` and the thousand-plan test); a
1.9 MB difference between two RSS samples is not evidence of growth, and is not claimed as its
absence either.

**What the numbers say.** Recording type names costs nothing measurable: every difference from
N17 is inside the run-to-run spread. A rename costs what it proves: the plan itself is a
references query, and the rest is one full check of the candidate program per compilation that
contains the file — about 80 ms for the whole compiler, so a rename there is answered in under
a tenth of a second, and a rename in a file anyone edits by hand in well under a millisecond. A
rename request is expected to cost more than a cursor query, and this one does not make a case
for incremental reparse. Serialising the edit is not measured separately: it is a handful of
ranges.

**What it added to the binary.** The release `nazm` grew from 2,823,040 to 2,888,576 bytes
(+65,536, +2.3%); no dependency was added.

### Scope at a position and completion — measured 2026-09-25

N19 records the checker's scopes as it checks. Measured as N18 was: in process, release,
contained, P1, nothing else running; medians (`target/n19/bench`, not committed). The trace is
written inside the check, so its cost is inside `analyse` and is stated as the difference from
N18's.

| What | Small (`examples/sum.nz`) | The compiler (`emit.nz` and imports) | Multi-module (`diamond/main.nz`) | 200 nested scopes |
| --- | --- | --- | --- | --- |
| scope trace recorded (frames, bindings) | 8, 7 | 2,725, 3,250 | 8, 0 | 202, 200 |
| `scope_at` | 6.6 µs | 0.72 ms | 5.8 µs | 59 µs |
| `completion` | 14.3 µs (1 item) | 2.12 ms (a call head, 16 items) | 15.2 µs (40 items) | 208 µs (111 items) |

On the compiler, against N18: `analyse` 32.9–33.5 ms over two runs (N18: 33.6) — the trace's
cost is not separable from the run-to-run spread; warm edit 80.7–81.6 ms (N18: 83.3); index
construction 3.85–3.88 ms (N18: 3.91). Resident memory: 49.5–49.9 MB with the compiler open
(N18: 48.1), 51.7–51.8 MB after 10 edits and the same after 100 (N18: 48.6 and 50.5) — about
1.5 MB more, the trace's frames and bindings, and flat across edits, with retained documents,
analyses, text, index and trace sizes held constant by the thousand-edit test.

**What the numbers say.** Recording what the checker already decides costs nothing measurable.
A scope query on the whole compiler is under a millisecond — it scans the trace's frames and
checks the function around the position for recovery — and a completion there is about 2 ms,
most of it finding the token, as definition's is. The protocol's conversion is not measured
apart: it is one range and a list of names. None of this makes a case for incremental reparse.

**What it added to the binary.** The release `nazm` grew from 2,888,576 to 2,954,112 bytes
(+65,536, +2.3%); no dependency was added.

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

## Mutation harness v2 — what a mutant costs (N12.2, 2026-09-24/25)

Tooling, not language performance, recorded here because this is where measured cost lives.
Contained, one CPU, one Cargo job, one test thread, nothing else running.

**Where the legacy runner's time went, per mutant:** a workspace rebuild of 11–15 s for a Rust
file and 0.1 s for a `compiler/*.nz` one, then the workspace suite, **228 s**. Per session, a
cold build of 92 s and the baseline suite. The suite, run for every mutant, was the cost.

| | legacy (N12.1 batches) | v2, profiles only | v2, verified killers |
| --- | --- | --- | --- |
| the 27 N12.1 mutants, wall | 10,782 s productive, 13,870 s with no-verdict reruns | 1,807 s | **523 s** (20.6× less) |
| per mutant, median / p90 | ≈ 240 s | 52 s / 84 s | **1.4 s / 13.8 s** |
| workspace suites run | 27, plus baselines | 0 | 0 |

The 523 s is 331 s of cold build and baseline, 131 s of workspace builds — kept, because
UNUSABLE is decided on the whole tree — and 44 s of killers.

**The whole catalogue, targeted:** 233 mutants in 10,686 s over five sessions (1,770 s of it
cold builds and baselines), median 22 s and p90 84 s per mutant; 2,116 s of workspace
builds, 6,694 s of tests. Six workspace suites were run and 227 avoided. The legacy shape
would have run 233 suites at about 245 s each — about sixteen hours, never measured, because
no session could hold it.

**What remains:** the selfhost profile. The 71 `compiler/*.nz` entries are 45 % of per-mutant
time, and the 51 without a killer each pay most of the selfhost binary (≈ 80–100 s) at tier 2.
A killer takes such an entry to a few seconds. Compilation is the second cost (24 %), and it
is the build UNUSABLE depends on.

## Development and evidence pipeline parallelism — N14.1, measured 2026-09-25

Tooling, not language performance. The question: how much of the machine may **one**
contained workload use inside the same ceilings — 4 GiB memory with swap equal, 512 pids,
8 GiB storage, the same deadlines — without changing a verdict or approaching a limit.

**The machine.** Apple M1 Pro, 10 cores (8 performance, 2 efficiency), 32 GiB; Docker
Desktop VM with 10 CPUs and 7.75 GiB, kernel 7.0.12-linuxkit, aarch64, `nazm-contained:1.98.1`.
Nothing else heavy ran during any cell; cells alternated profiles so thermal drift could
not line up with one of them.

**The gates, frozen before any profile was chosen.** Total cgroup peak (`memory.peak`) at or
below 85 % of the limit, 3,482 MiB; anonymous memory at or below 70 %, 2,867 MiB; zero
`memory.events` max, oom and oom_kill; no swap. Total, anonymous and page-cache peaks are
reported separately because they mean different things (the first round's build cells
recorded only the total; anonymous and page-cache figures come from the later rounds): anonymous memory is what the
workload needs, page cache is what the kernel kept of the files it wrote.

**Semantics did not move.** Every cell of every profile gave the same answer: the workspace
suite 1,317 passed / 0 failed / 13 ignored in 67 binaries, selfhost 38 of 38, every mutant
the same verdict and tier. No cell anywhere had an OOM kill, a limit event or swap.

| Workload (median of n) | P1 | P4T4 | P6 (6 CPUs, 6 jobs, 4 threads) |
| --- | --- | --- | --- |
| cold workspace build | 97.5 s (4) | **26.9 s (3), 3.6×** | 19.2 s (4) |
| incremental rebuild, one `nazm-core` file | 16.0 s (4) | **4.7 s (3), 3.4×** | 3.3 s (4) |
| worst total / anonymous peak, build | 3,328 / 375 MiB | 3,311 / 1,050 MiB | **3,658** / 1,302 MiB |
| workspace suite | 294.4 s (3) | **97.0 s (3), 3.03×** | 96.9 s (3) |
| worst total / anonymous peak, suite | 2,704 / 354 MiB | 2,857 / 359 MiB | 2,944 / 362 MiB |
| selfhost suite | 131.2 s (2) | **38.1 s (2), 3.44×** | 38.0 s (2) |
| worst total / anonymous peak, selfhost | 764 / 100 MiB | 1,076 / 303 MiB | 1,102 / 287 MiB |

**What decided it.** The suites scale with *test threads*, not with the CPU quota: P4 with
two threads ran the suite in 146.6 s and P6 with two in 146.7 s, P4 with one thread in
270.2 s. P6's extra quota helps compilation only, and its incremental rebuild crossed the
total gate in all four measurements (3,506–3,658 MiB) — six concurrent `rustc` processes'
anonymous memory on top of 2.3 GiB of page cache. P1's own worst build total, 3,328 MiB, is
above P4T4's: the total is mostly page cache, which is why anonymous memory is reported
beside it. Where P4T4 and P6 tie, the rule was the lower-resource profile. P2 (1.9×) and the one- and two-thread P4 variants were measured in
the first round and dropped.

**Mutation stays P1.** One fast tier-1 mutant, `n13-a-vec-has-equality`:

| | wall | verdict | total / anonymous / page cache |
| --- | --- | --- | --- |
| P1 | 374.9 s | caught, tier 1 | 2,998 / 373 / 2,630 MiB |
| P4T4 | 130.8 s | caught, tier 1 | **3,623** / 868 / 2,642 MiB |
| 4 CPUs, 2 jobs, 4 threads | 156.7 s | caught, tier 1 | **3,853** / 619 / 3,292 MiB |

Both parallel profiles crossed the total gate. The total is dominated by page cache — a
campaign holds a tree copy, a target directory and every test binary it built — and it is
not monotonic in parallelism: halving the jobs lowered anonymous memory by 250 MiB and the
total *rose* by 230 MiB. The gate was not changed after this evidence appeared, so the
default did not move and **N14.1 claims no mutation speedup.** Accelerating mutation needs
its own experiment on how page cache and the container's memory limit interact, not a
looser gate.

**The defaults.** `contained tests` and `contained selfhost` take P4T4; `mutate`,
`bootstrap`, `release` and `run` stay P1; `bench` is P1 permanently. `--profile p1`
reproduces any older result. All build-cell and mutation numbers above are direct paired
measurements on the current tree; the per-workload speedups are measured, not derived.

**The final runs, on the frozen tree that was then committed unchanged, each under its
adopted default:** the workspace suite at P4T4 in 116 s of container time (1,321 passed,
the four new tests included), selfhost at P4T4 in 45 s, the xtask lifecycle suite 13 of
13, a frozen seven-mutant P1 sample with every verdict and tier as established (five at
tier 1, two at tier 2, no timeout, every source restored to its digest), and the bootstrap
at P1 in 52 s with `0e1a40e6…` unchanged.

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
