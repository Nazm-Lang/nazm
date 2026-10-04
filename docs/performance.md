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

**Every measured section is filed (N92).** `bench/claims.toml` names each section below whose heading
says *measured* or *frozen* as **reproducible** — and the command that regenerates it — **dated**, a
record of the tree on its date and not a claim about this one, or **unreproduced**, a current claim
no command regenerates yet. `cargo xtask check` refuses a section the register does not file.

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

### The call at a position and signature help — measured 2026-09-26

N20 records every checked call as the checker checks it. Measured as N19 was: in process,
release, contained, P1, nothing else running; medians over two runs (`target/n20/bench`, not
committed). The record is written inside the check, so its cost is inside `analyse` and a warm
edit, stated against N19's.

| What | Small (`examples/sum.nz`) | The compiler (`emit.nz` and imports) | 200 nested calls | Multi-module (an imported generic) |
| --- | --- | --- | --- | --- |
| checked calls recorded | 2 | 8,873 | 200 | 2 |
| `call_at` | 2.0 µs | 5.9 µs | 61 µs (innermost) | 2.0 µs |
| `signature_help` (structured, rendered) | 2.3 µs | 6.3 µs | 62 µs | 2.6 µs |

On the compiler, against N19: `analyse` 34.9–38.3 ms (N19: 32.9–33.5); warm edit 84.7–86.4 ms
(N19: 80.7–81.6); index construction 3.93–3.99 ms (N19: 3.85–3.88); `scope_at` 0.70–0.73 ms
and completion 2.09–2.13 ms, unchanged. Resident memory: 58.7–58.8 MB with the compiler open,
and the same after 10 and after 100 edits (N19: 49.5–49.9 open, 51.7–51.8 after edits) — about
9 MB more, and flat, with retained documents, analyses, text and checked-call count held
constant by the thousand-edit test. The protocol's conversion is not measured apart: it is one
label and its parameters' offsets.

**What the numbers say.** A query is a walk down one path of the tree and one lookup, so it is
microseconds even on the whole compiler — faster than hover, which scans the tokens. The cost
is the record: about 8,900 calls, each with its argument spans and parameter types, add 2–5 ms
to checking the compiler, 3–5 ms (4–7%) to a warm edit, and about 9 MB of resident memory. The
memory was not attributed further than that; the record is what changed. It is paid by every
compilation, since the checker writes it whether or not a tool reads it. None of this makes a
case for incremental reparse; whether the record should become cheaper, or optional outside the
language service, is left to review.

**What it added to the binary.** The release `nazm` grew from 2,954,112 to 3,019,648 bytes
(+65,536, +2.2%); no dependency was added.

### Structure at a position — measured 2026-09-26

N21 answers from facts the checker already recorded, plus `Resolution::looked_up_on`, which is
written only where a field or variant lookup fails. **No persistent table was introduced.** The
new fact holds **0 entries** in every clean fixture measured — the compiler included — and one
per unresolved member name in a program being edited.

The host was contended throughout (other processes drove its load average between 3 and 70),
and two sequential runs of the usual benchmark disagreed by up to 5× on code N21 did not touch.
So the cost to checking and editing was measured **interleaved**: the N20 tree (`b7bb35d`) and
the N21 tree built side by side in one container and run alternately, four times each,
contained, P1 (`target/n21/ab`, not committed).

| On the compiler | N20 | N21 |
| --- | --- | --- |
| `analyse` | 38.5–48.9 ms | 37.7–41.6 ms (and one 117 ms run during a load spike) |
| warm edit | 95.0–106.6 ms | 94.2–105.3 ms |
| resident, compiler open | 34.40 MB | 34.40 MB (+4 kB) |
| resident, after 15 edits | 35.5–36.2 MB | 35.5–36.8 MB |

The queries, from the quieter of the two sequential runs (`target/n21/bench`), medians:

| What | Small | The compiler (`emit.nz`) | Generic record and enum | Construction inside 200 nested calls | Imported record and enum |
| --- | --- | --- | --- | --- | --- |
| `structure_at` | 14–15 µs | 3.5–5.5 ms | 9–10 µs | 127–148 µs | 7–8 µs |
| member / variant / label completion | 16 µs | 3.6–5.5 ms | 10–11 µs | 138–161 µs | 8–9 µs |
| constructor signature help (`help_at`) | 5 µs | 14–17 µs | 5 µs | 147–167 µs | 5 µs |

**What the numbers say.** Under the contended host, the interleaved N20/N21 comparison detected
no N21-specific analyse, warm-edit or RSS delta. Because host load was high, this is not precise
evidence that the true delta is zero; it establishes that no delta was detectable in this
measurement. N21 adds no persistent clean-program structure table: `looked_up_on` has zero
entries in clean code. A structure query on the whole
compiler is a few milliseconds, and almost all of it is finding the token under the position —
the same scan N19's completion and N16's definition make; constructor signature help, which
walks down the tree from the root instead, is microseconds. The benchmark's usual whole-program
figures for this run are not reported against N20's, because the host, not N21, moved them.

**What it added to the binary.** Nothing measurable: the release `nazm` is 3,019,648 bytes, as it
was at N20; no dependency was added.

### Document and workspace symbols — measured 2026-09-26

N22 adds no work to analysis and keeps nothing: both queries are derived on demand from the
checker's definition tables and the parsed items, when asked. **No persistent symbol table was
introduced**; the compiler open as four programs holds exactly the documents, analyses and
text it held before (`retained` = 4 documents, 4 analyses, 1,316,013 bytes of text), and the
number of symbol entries kept between queries is zero by construction.

The host was quiet this time (load average about 2.4–3.6), but the comparison was made the same
way as N21's, **interleaved**: the N21 tree (`ce0c4ae`) and the N22 tree built side by side in
one container and run alternately, four times each, contained, P1 (`target/n22/ab`, not
committed).

| On the compiler (`emit.nz`) | N21 | N22 |
| --- | --- | --- |
| `analyse` | 35.5–36.1 ms | 35.4–36.6 ms |
| warm edit | 85.8–87.5 ms | 86.2–88.2 ms |
| resident, compiler open | 34.39–34.61 MB | 34.33–34.54 MB |
| resident, after 15 edits | 35.5–36.6 MB | 35.4–36.5 MB |

The queries, two runs in the same container (`target/n22/bench`), medians:

| What | Result |
| --- | --- |
| `document_symbols`, small (`examples/sum.nz`, 3 symbols) | 2.4–2.5 µs |
| `document_symbols`, generic record and enum (9 symbols) | 3.5–3.7 µs |
| `document_symbols`, the compiler's `emit.nz` (135 symbols) | 133 µs |
| `document_symbols`, one of 40 modules declaring the same names (29 symbols) | 11.5–11.6 µs |
| `workspace_symbols("")`, small | 3.2 µs |
| `workspace_symbols("")`, the compiler, 1 root (5 files, 497 symbols) | 1.48–1.49 ms |
| `workspace_symbols("")`, the compiler, 4 roots sharing `lex.nz` (8 files, 500 symbols) | 2.18–2.19 ms |
| `workspace_symbols("emit")`, same (14 symbols) | 1.88–1.89 ms |
| `workspace_symbols("Zzz")`, same (none) | 1.89 ms |
| `workspace_symbols("")`, 40 roots declaring the same names (1,160 symbols) | 4.19–4.20 ms |
| `workspace_symbols("Point")`, same (40 symbols) | 3.12–3.19 ms |
| `workspace_symbols("f1")`, same (440 symbols) | 3.45–3.54 ms |

**What the numbers say.** The interleaved N21/N22 comparison detected no N22-specific analyse,
warm-edit or RSS delta, and none is expected: N22 changes no code that analysis runs. This is
still a measurement, not a proof — the ranges overlap, which is all it can show. A document's
outline is microseconds, the whole compiler's file 0.13 ms. A workspace query derives every
analysed file's symbols from every current compilation and deduplicates them, so it costs
about the same whatever the query: the filter is applied to a universe built each time, and
most of the cost is building it (four compilations of the compiler, 2.2 ms; forty small
programs, 4.2 ms). That is well inside an interactive request, so no index was kept to make it
cheaper and no result is truncated; if an open universe grows until it is not, an index is the
measured next step, not an assumption. The protocol conversion was not timed separately: it is
one line-table pass per file over results of this size.

**What it added to the binary.** The release `nazm`, built from both trees in the same container,
is 3,019,648 bytes at N21 (the accepted baseline, reproduced) and 3,085,184 bytes at N22:
**+65,536 bytes, +2.2%**, one 64 KiB step of the file's layout, for the symbol module and the two
protocol handlers and their `lsp-types` structures. No dependency was added.

### Semantic identifiers and semantic tokens — measured 2026-09-26

N23 derives its view on demand and keeps no token array, no previous result and no cache. It
adds one checker record, `Resolution::written_type`: every written built-in type name and type
parameter name the checker resolved, and every type parameter's declaration. That record is
kept with each analysis like the other resolution maps, and it is the only new retained state:
**1,989 entries** for `emit.nz`'s compilation (the compiler's emitter and its four imports),
1,151 for `analyse.nz`'s, 12 for `examples/sum.nz`.

Compared interleaved, as at N21 and N22: the N22 tree (`19be58e`) and the N23 tree built side
by side in one container and run alternately, four times each, contained, P1, on a quiet host
(`target/n23/ab`, not committed).

| On the compiler (`emit.nz`) | N22 | N23 |
| --- | --- | --- |
| `analyse` | 34.70–34.85 ms | 34.77–35.58 ms |
| warm edit | 84.45–84.91 ms | 84.03–87.12 ms |
| resident, compiler open | 34.33–34.35 MB | 34.52–34.53 MB |
| resident, after 15 edits | 35.4–36.5 MB | 35.6–36.7 MB |

The queries, two runs in the same container (`target/n23/bench`), medians:

| Document | Identifiers | `semantic_identifiers` | `semanticTokens/full`, real server over stdio |
| --- | ---: | --- | --- |
| `examples/sum.nz` | 34 | 9.9–10.0 µs | 88–91 µs |
| `compiler/emit.nz` (286 KB) | 13,550 | 3.24 ms | 9.27 ms |
| `compiler/analyse.nz` (210 KB) | 10,891 | 2.89–2.94 ms | — |
| 200 functions of shadowing, generics and a record and a function both `Box` | 6,820 | 1.62 ms | 4.95 ms |

**What the numbers say.** The resident delta is real and detected in every round: about
**+180 kB (+0.5%)** with the compiler open, which is what 1,989 recorded spans cost in a hash
map. `analyse` was at or above N22 in each paired round, by 0.0–0.9 ms; the ranges overlap and
this measurement cannot say whether recording the names costs anything measurable — it is not
evidence of zero. The warm edit showed no detectable delta. Deriving a whole compiler file's
identifiers is a few milliseconds, one hash lookup or three per identifier token; the full
request over the protocol, which adds line and UTF-16 conversion, the relative encoding, JSON
of 67,750 integers and the pipe, is about 9 ms for the largest file in the repository, so the
adapter's share is about 6 ms there. Nothing justifies a range request, deltas or a cache yet.

**What it added to the binary.** Nothing measurable: the release `nazm`, built from both trees in
the same container, is 3,085,184 bytes at N22 and at N23 — the new code fits the layout step N22
reached. No dependency was added.

### Quick fixes — measured 2026-09-26

N24 keeps nothing: a fix plan is made per request from the current analysis's diagnostics and
dropped with the response — no action cache, no resolve state, no stored edit. **Persistent
code-action state: zero entries**, and no checker record was added.

Compared interleaved with N23 (`21f626e`), four rounds each in one container, contained, P1
(`target/n24/ab`, not committed):

| On the compiler (`emit.nz`) | N23 | N24 |
| --- | --- | --- |
| `analyse` | 35.33–36.16 ms | 35.29–36.18 ms |
| warm edit | 85.72–87.41 ms | 85.21–87.79 ms |
| resident, compiler open | 34.50–34.52 MB | 34.50–34.53 MB |

The requests, two runs in the same container (`target/n24/bench`), medians:

| Document | Fix-bearing diagnostics | `fix_plans` at a cursor | `fix_plans`, whole file | `fix_plan_is_current`, every plan | `codeAction` over stdio, cursor / whole file |
| --- | ---: | --- | --- | --- | --- |
| one automatic fix | 1 | 1.9–2.0 µs | 1.9 µs | 1.3 µs | 98 / 97 µs |
| one fix needing review | 1 | 1.9–2.0 µs | 1.9 µs | 1.3 µs | 96–100 / 96–99 µs |
| one fix after `é😀` | 1 | 1.9–2.0 µs | 1.9 µs | 1.3 µs | 98 / 97 µs |
| 100 assignments to immutable bindings | 100 | 10–11 µs | 83–84 µs | 131 µs | 139–140 µs / 1.54–1.55 ms |

**What the numbers say.** The interleaved comparison detected no analyse, warm-edit or resident
delta, and none is expected: N24 changes no code that analysis runs, and keeps nothing. Planning
a document's fixes is microseconds; the currency check compares the document's whole text once
per plan, which is what makes 100 plans 131 µs. A request is dominated by the protocol round
trip for a few actions (about 0.1 ms) and by building, converting and serialising the edits for
many — 100 actions in 1.5 ms, of which the adapter's share, conversion to UTF-16 and JSON and
the pipe, is about 1.3 ms.

**What it added to the binary.** The release `nazm`, built from both trees in the same
container, is 3,085,184 bytes at N23 and 3,150,720 bytes at N24: **+65,536 bytes, +2.1%**, one
64 KiB step of the file's layout, for the fix-plan module, the quick-fix handler and their
`lsp-types` structures. No dependency was added.

### Call hierarchy — measured 2026-09-26

N25 adds no per-call state: the caller of each checked call is the body whose `call_sites` the
checker already records at the same point. **Persistent call-hierarchy state: zero entries** —
no item table, no graph, no field on `CheckedCall`. Incoming calls scan every call site of the
item's compilation once (O(calls), a hash lookup each, into a request-local map from caller to
spans that is dropped with the answer); outgoing calls read the item's own call sites
(O(its calls)). On the compiler's largest compilation (`emit.nz`, 286,160 bytes): 120 functions
in the file, 8,873 checked calls in its compilation, 603 outgoing edges carrying 1,702
navigable call occurrences from the file's functions, to 191 distinct callees.

Compared interleaved with N24 (`7406b3e`), four rounds each in one container, contained, P1
(`target/n25/ab`, not committed):

| On the compiler (`emit.nz`) | N24 | N25 |
| --- | --- | --- |
| `analyse` | 34.63–35.28 ms | 34.68–35.76 ms |
| warm edit | 84.58–85.69 ms | 84.98–85.16 ms |
| resident, compiler open | 34.51–34.53 MB | 34.51–34.72 MB |

The requests, two runs in the same container (`target/n25/bench`), medians:

| Fixture | prepare | incoming | outgoing | over stdio: prepare / incoming / outgoing |
| --- | --- | --- | --- | --- |
| two functions, two calls | 4.8 µs | 2.7 µs | 1.4 µs | 83–96 / 91–104 / 79–91 µs |
| four modules, an import cycle | 4.3 µs | 3.8 µs | 3.7 µs | — |
| two roots over one file, each root | 4.1–4.2 µs | 2.5–2.7 µs | 1.5 µs | — |
| one target, 200 callers | 285 µs | 239 µs | 1.7 µs | 587–593 µs / 2.98–2.99 ms / 102–103 µs |
| one caller, 200 callees | 288 µs | 10 µs | 235 µs | 639–680 µs / 110–111 µs / 2.94–2.98 ms |
| `emit.nz`, its busiest caller (30 callees, 260 calls) | 1.84 ms | 369 µs | 124 µs | 2.04–2.15 / 0.78–0.79 / 1.32 ms |
| `emit.nz`, its most-called function (48 callers, 372 calls) | 1.81 ms | 567 µs | 2.1 µs | 2.45–2.46 / 2.04–2.06 ms / 101 µs |

**What the numbers say.** The interleaved comparison detected no analyse, warm-edit or resident
delta beyond the rounds' own spread — one N25 round's resident figure is 0.2 MB higher, within
what the edits leave behind — and none is expected: N25 changes no code analysis runs and keeps
nothing. Preparing costs what finding the name under the cursor costs: `definition` at the same
name in `emit.nz` is 1.84 ms, the same as prepare, because both walk the document's concrete
tree to find the token (N16's rule, shared). Incoming calls over the whole compiler's
compilation are about half a millisecond; outgoing, a tenth. Over stdio, a request with many
edges is dominated by building and serialising the items — 200 items with their data in about
3 ms. Before any evidence run the adapter built a line index of the whole file for every item
and range list, which made incoming calls of `emit.nz`'s most-called function 21 ms over stdio
on the host; it now builds one per file per response (and a mutation guards that).

**What it added to the binary.** The release `nazm`, built from both trees in the same
container, is 3,150,720 bytes at N24 and 3,216,256 bytes at N25: **+65,536 bytes, +2.1%**, one
64 KiB step of the file's layout, for the hierarchy module, the three handlers and their
`lsp-types` structures. No dependency was added.

### Completion at a hole — measured 2026-09-27

N26 keeps nothing and records nothing in an ordinary analysis: **clean-program N26 state is zero
entries**, and an incomplete program's ordinary analysis is unchanged — `nazm check` from the N25
and N26 release binaries gives byte-identical output and status on all 200 `.nz` files of the
repository (the CLI's deliberately broken test programs among them) and thirteen unfinished
fixtures (`target/n26/incomplete`). What a hole costs is its **probe**: one more parse and check
of the compilation, made for the request and dropped with the response, whose resolution holds the
hole's anchor — one unresolved-member or construction entry — and whose diagnostics are never
published. No completion, probe or anchor is cached.

Compared interleaved with N25 (`e0982a5`), four rounds each in one container, contained, P1
(`target/n26/ab`, not committed), on the final source:

| On the compiler (`emit.nz`) | N25 | N26 |
| --- | --- | --- |
| parse | 25.51–26.24 ms | 25.48–25.90 ms |
| `analyse` | 35.02–35.75 ms | 35.20–35.40 ms |
| warm edit | 84.64–85.50 ms | 85.47–86.74 ms |
| resident, compiler open | 34.51–34.54 MB | 34.53–34.54 MB |

**An ordinary-path regression, found and removed.** The first comparison, on the source frozen
before this one, showed the ordinary parse 1.7 ms slower (25.3–25.9 against 27.2–28.2 ms, in
either order across six more rounds, and not on the host) though that path runs no hole code: the
hole blocks inlined into the parser's hottest recursive functions had changed their shape. Moved
into cold, never-inlined helpers, parse and `analyse` overlap N25 again. The warm-edit ranges of
the final run touch rather than overlap (N26's median about 1 ms, 1 %, higher); an earlier
interleaved run of the same code gave 84.99–86.31 against 85.06–85.78 ms. No delta is detected
beyond that, and none is claimed to be zero.

The requests, two runs in the same container (`target/n26/bench`), medians:

| Hole | `completion` in process | `.`-triggered over stdio |
| --- | --- | --- |
| `p.` on a record, small | 92.5–93.2 µs | 195–196 µs |
| `b.` on `Box[Int]`, small | 94.8–94.9 µs | 194–196 µs |
| `State.`, small | 91.8 µs | 191–192 µs |
| `Maybe[Int].`, small | 94.5–95.1 µs | — |
| `Point(`, small | 91.1–92.6 µs | — |
| `State.Done(`, small | 92.3–92.4 µs | — |
| a pattern's `State.Done(`, small | 93.7–94.5 µs | — |
| a member hole in `emit.nz` (286,156 bytes, 19 candidates) | 41.0–41.8 ms | 41.7–42.3 ms |

The probe apart, on the compiler's compilation: **parse 25.4–25.9 ms, parse and check together
33.8–34.8 ms** — the same as an ordinary `analyse` (34.7–34.8 ms), which it is, less the
reference index and concrete tree the service builds beside it. The rest of a compiler-scale
request is finding the hole and building the candidates. Resident memory rises by the probe's
peak while it is alive — 35.0 MB to 51.1–51.8 MB here, the high-water mark of one more analysis of
the compiler — and nothing it allocated is kept. So a hole costs a compilation's analysis: about
0.1 ms on a small file, about 42 ms on the compiler, which is the price of reading a buffer the
ordinary analysis cannot. A name already written is answered from the current analysis as before,
without a probe.

**What it added to the binary.** The release `nazm`, built from both trees in the same
container, is 3,216,256 bytes at N25 and at N26: **no change**, the additions inside the file's
existing 64 KiB layout step. No dependency was added.

### Semantic context packets and `nazm-mcp` — measured 2026-09-27

N27 keeps nothing between requests: a packet is derived from one analysis of its root, serialised
and dropped with it — no packet, dependency graph or analysis cache. **Persistent N27 state: zero
entries.** The ordinary compiler path does no packet work.

Compared interleaved with N26 (`2798b98`), four rounds each in one container, contained, P1
(`target/n27/ab`, not committed):

| On the compiler (`emit.nz`) | N26 | N27 |
| --- | --- | --- |
| parse | 25.06–25.46 ms | 24.92–25.67 ms |
| `analyse` | 34.33–36.42 ms | 34.20–34.52 ms |
| warm edit | 83.34–84.17 ms | 82.88–83.86 ms |
| resident, compiler open | 34.54–34.56 MB | 34.53–34.54 MB |

No delta is detected; the ranges overlap.

A packet, in process, two runs in the same container (`target/n27/bench`), medians — opening the
root from disk and analysing it, deriving the packet, and serialising it, which is what
`nazm context` does once per run:

| Target | open + analyse | derive | serialise | packet | target source | compilation source |
| --- | --- | --- | --- | ---: | ---: | ---: |
| small function | 0.13 ms | 0.049 ms | 0.004 ms | 1,627 B | 35 B | 1,021 B |
| small record | 0.12 ms | 0.036 ms | 0.003 ms | 714 B | 24 B | 1,021 B |
| small enum | 0.12 ms | 0.036 ms | 0.003 ms | 732 B | 32 B | 1,021 B |
| a function with 500 callers, 1,000 references | 6.09 ms | 5.43–5.48 ms | 0.37 ms | 159,709 B | 24 B | 21,316 B |
| `emit.nz`, a typical function (`hex_digit`) | 80.4–80.6 ms | 21.2–21.3 ms | 0.013 ms | 1,225 B | 234 B | 601,518 B |
| `emit.nz`, its largest function (`emit_module`) | 80.4–80.5 ms | 21.6–21.7 ms | 0.14 ms | 103,431 B | 59,243 B | 601,518 B |
| `emit.nz`, a record | 80.6 ms | 20.9 ms | 0.011 ms | 835 B | 48 B | 601,518 B |
| `emit.nz`, an enum | 80.3–80.4 ms | 20.4–20.7 ms | 0.016 ms | 3,284 B | 375 B | 601,518 B |

Over `stdio`, the real `nazm-mcp` process, medians of the full `tools/call` round trip:

| Target | `tools/call` | response frame | server resident: before, after, peak |
| --- | --- | ---: | --- |
| small function | 0.48 ms | 1,715 B (was 3,643) | 3.2 → 4.3 MB, peak 4.3 MB |
| 500 callers | 16.2 ms | 159,796 B (was 347,632) | 3.2 → 11.4 MB |
| `emit.nz`, typical function | 108.1 ms | 1,312 B (was 2,752) | 3.2 → 37.3 MB, peak 37.3 MB |
| `emit.nz`, largest function | 108.4 ms | 103,518 B (was 214,808) | 3.2 → 38.5 MB |

**What the numbers say.** A packet costs one analysis of its root: about 80 ms of the compiler's
105 ms tool call is loading and analysing the compilation from disk, which every call does so
that it answers the disk as it is; the protocol adds a few milliseconds. Deriving a packet at
compiler scale is about 21 ms whatever the target, because it walks the compilation's reference
index once and re-parses the target's file for the token at the offset; serialising is
negligible. Resident memory rises by one analysis during a call and stays at that high-water
mark afterwards (the allocator keeps its pages): 1,000 calls in `crates/nazm-mcp/tests/protocol.rs`
leave it within a megabyte of where it settled, which the test asserts (48 kB in the host run). No cache was added to reduce the 80 ms: that would need a
freshness contract this milestone does not have.

**What a packet weighs.** A typical function's packet is 0.2 % of the bytes its compilation's
sources hold, a record's 0.1 %, an enum's 0.5 %. It is not always smaller than source: a target
with many references carries one link per use — 500 callers and 1,000 references make 160 kB, 7.5
times the fixture's source, because nothing is truncated — and a large function carries its own
source, 59 kB of `emit_module`'s 103 kB packet. These are byte ratios, not token counts: no
tokenizer was measured, and no token saving is claimed. Over MCP the frame carries the packet
**once**, as structured content, with an empty `content`: each frame is the packet plus an 87-byte
JSON-RPC envelope. The first measurement (the "was" column) sent it twice — the SDK's constructor
also copies structured content into a text block, which the specification only suggests for older
clients — and the closure correction removed the copy: **52–54 % fewer bytes per response**, in
bytes, not tokens.

**What it added to the binaries.** Built from both trees in the same container: `nazm` is
3,216,256 bytes at N26 and 3,347,328 at N27, **+131,072 bytes (+4.1 %)**, two 64 KiB steps, for the
packet module, its serialisation and the `context` command — no MCP crate reaches it (`cargo tree
-p nazm-cli` has no `rmcp`, `tokio` or `schemars`). `nazm-mcp` is a separate binary of **2,627,296
bytes**, carrying the SDK: 54 crates new to `Cargo.lock` (`rmcp` 3.4.1 and its tree — `tokio`,
`futures`, `schemars`, `chrono`, `pastey` and others), all in that crate alone, and a one-time
rebuild of the contained image so they are fetched ahead of offline runs.

### Semantic snapshots, deltas and their MCP tools — measured 2026-09-27

N28 keeps nothing between requests: a snapshot or delta is derived from one analysis of its root,
serialised and dropped with it — no snapshot cache, dependency graph, call graph or history.
**Persistent N28 state: zero entries.** The ordinary compiler path does no snapshot work; the only
change on it is two helpers N27 and N25 code now calls (`context::incomplete`,
`hierarchy::callee`) with the bodies they had inline.

In process, contained, P1 (`target/n28/bench`, not committed), medians — opening the root from disk
and analysing it, deriving the snapshot, serialising it:

| Root | definitions | open + analyse | derive | serialise | snapshot | loaded source | all N27 packets |
| --- | ---: | --- | --- | --- | ---: | ---: | ---: |
| small (a record, an enum, two functions) | 4 | 0.13 ms | 0.046 ms | 0.003 ms | 2,958 B | 1,021 B | 4,028 B |
| 500 callers, 1,000 references | 501 | 6.24 ms | 6.29 ms | 0.27 ms | 376,319 B | 21,316 B | 721,000 B |
| 2,000 definitions (250 records, 250 enums, 1,500 functions) | 2,000 | 32.0 ms | 29.7 ms | 1.40 ms | 1,441,797 B | 82,794 B | 3,376,156 B |
| the compiler (`emit.nz` and its imports) | 438 | 80.7 ms | 13.4 ms | 0.33 ms | 332,252 B | 601,518 B | 2,227,799 B |

A delta, in process, against a baseline of the same root — parsing the baseline, validating it (a
baseline refused at its last digest, so every entry is checked), deriving the current snapshot, the
whole delta, and serialising it:

| Root and edit | parse | validate | current snapshot | whole delta | delta |
| --- | --- | --- | --- | --- | ---: |
| small, no edit | 0.007 ms | 0.007 ms | 0.045 ms | 0.053 ms | 329 B |
| 500 callers, no edit | 0.86 ms | 1.08 ms | 6.40 ms | 7.59 ms | 334 B |
| 2,000 definitions, no edit | 3.12 ms | 5.52 ms | 30.3 ms | 36.6 ms | 337 B |
| compiler, no edit | 0.82 ms | 1.19 ms | 13.2 ms | 14.4 ms | 334 B |
| compiler, a comment in one body | 0.80 ms | 1.19 ms | 13.4 ms | 14.5 ms | 600 B — one definition, `source` alone |
| compiler, one new call (`add_line` → `max_instances`) | 0.84 ms | 1.24 ms | 13.6 ms | 15.7 ms | 871 B — the caller's source, dependencies and callees; the callee's references and callers |
| compiler, one added function | 0.81 ms | 1.20 ms | 13.1 ms | 14.6 ms | 449 B — one added |

Comparing is under a millisecond everywhere; a delta costs a snapshot plus reading the baseline.
Over `stdio`, the real `nazm-mcp` process, medians of the full `tools/call` round trip:

| Root | snapshot call | snapshot frame | delta call (baseline sent) | delta frame | server resident: before, after |
| --- | --- | ---: | --- | ---: | --- |
| small | 0.48 ms | 3,045 B | 0.64 ms | 416 B | 3.3 → 4.5 MB |
| 500 callers | 16.8 ms | 376,405 B | 36.0 ms | 420 B | 3.2 → 13.2 MB |
| 2,000 definitions | 76.2 ms | 1,441,883 B | 190 ms | 423 B | 3.2 → 45.7 MB |
| compiler | 99.9 ms | 332,338 B | 125 ms | 420 B | 3.2 → 40.4 MB |

Each frame is the result plus an 86–87-byte JSON-RPC envelope: the result travels once. A delta
call costs more than a snapshot call because the client sends the whole baseline, which the server
parses from the request and validates before deriving the current snapshot. Resident memory rises
by one analysis and stays at that high-water mark (the allocator keeps its pages); 1,000 snapshot
and delta calls in `crates/nazm-mcp/tests/protocol.rs` stay within a megabyte of where they settled,
which the test asserts.

**What a snapshot weighs.** About 720–760 bytes per definition — an identity, a place and eight
64-digit digests — whatever the definition's size. So a snapshot is **smaller than its sources
where definitions are real** — the compiler's is 55 % of its 601 kB of source, and 15 % of the
2.2 MB its 438 N27 packets would be — and **larger** where they are tiny: the generated fixtures of
one-line functions are 17–18 times their source, and the four-definition file is three times its.
A delta is proportional to what changed, not to the compilation: 329–337 bytes for no change,
about 270 more per changed definition. These are byte counts, not token counts: no tokenizer was
measured, and no token saving is claimed.

**What it added to the binaries.** Built in the same container: `nazm` is 3,347,328 bytes at N27
and **3,478,400** at N28, **+131,072 bytes (+3.9 %)**, two 64 KiB steps, for the snapshot and delta
modules, the baseline parser and two commands; `nazm-mcp` is 2,627,296 and **2,823,904**, +196,608
bytes. No crate was added to `Cargo.lock` — one dependency edge: `nazm-service` now uses the
workspace's existing `blake3`, the one hash the cache and interface already use.

### Semantic patch plans and `nazm.semantic_patch` — measured 2026-09-28

N29 keeps nothing between requests: a patch is planned from one analysis of its root, serialised
and dropped. **Persistent N29 state: zero entries** — no patch cache, patch history or
application history, and nothing is applied. The ordinary compiler path does no patch work; the
only change on it is `rename::Edit` gaining a serialisation and one of N18's functions becoming
visible to the patch module.

In process, contained, P1 (`target/n29/bench`, not committed), medians — opening the root from disk
and analysing it; binding the snapshot (deriving `nazm.snapshot/1`, serialising it, BLAKE3); the
whole patch; the underlying N18 rename or N24 fix plan alone; and serialising:

| Case | open + analyse | snapshot binding | whole patch | N18 / N24 plan | serialise | edits | patch | replacement | affected source |
| --- | --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: |
| small, local rename | 0.19 ms | 0.083 ms | 0.31 ms | 0.19 ms | 0.001 ms | 2 | 667 B | 6 B | 312 B |
| small, private function rename | 0.19 ms | 0.083 ms | 0.31 ms | 0.19 ms | 0.002 ms | 2 | 708 B | 12 B | 312 B |
| small, field rename | 0.19 ms | 0.083 ms | 0.32 ms | 0.20 ms | 0.002 ms | 3 | 738 B | 6 B | 312 B |
| small, variant rename | 0.19 ms | 0.083 ms | 0.32 ms | 0.20 ms | 0.002 ms | 3 | 756 B | 12 B | 312 B |
| small, automatic fix | 0.09 ms | 0.017 ms | 0.029 ms | 0.002 ms | 0.001 ms | 1 | 733 B | 5 B | 53 B |
| small, needs-review fix | 0.10 ms | 0.017 ms | 0.029 ms | 0.002 ms | 0.001 ms | 1 | 745 B | 1 B | 99 B |
| compiler, private function `v` (373 occurrences) | 82.7 ms | 14.0 ms | 117 ms | 97.4 ms | 0.036 ms | 373 | 25,096 B | 1,865 B | 286,160 B |
| compiler, automatic fix | 82.1 ms | 14.2 ms | 14.0 ms | 0.012 ms | 0.003 ms | 1 | 749 B | 5 B | 286,216 B |

A rename costs N18's validation — the candidate program re-parsed and re-checked, about 97 ms on
the compiler — plus the snapshot binding, about 14 ms there (N28's derivation and a hash). A fix
costs the binding and almost nothing else: the compiler already attached it. No multi-file case
exists: N18's scope rule keeps a private entity's occurrences in its declaring file.

Over `stdio`, the real `nazm-mcp` process, medians of the full `tools/call` round trip:

| Case | `tools/call` | frame | server resident: before, after |
| --- | --- | ---: | --- |
| small, local rename | 0.82 ms | 754 B | 3.3 → 4.5 MB |
| small, automatic fix | 0.41 ms | 820 B | 3.3 → 4.3 MB |
| compiler, private function rename | 203 ms | 25,182 B | 3.4 → 64.2 MB |
| compiler, automatic fix | 101 ms | 835 B | 3.4 → 36.1 MB |

Each frame is the patch plus an 86–87-byte JSON-RPC envelope: the patch travels once. Resident
memory rises to one analysis's high-water mark — a rename's, which analyses the candidate too, is
higher — and stays there; 1,000 planning calls in `crates/nazm-mcp/tests/protocol.rs` stay within a
megabyte of where they settled, which the test asserts.

**What a patch weighs.** About 650–750 bytes of fixed structure — the operation, the target or
the fix's facts and precondition, three 64-digit digests, the root — plus about 65 bytes per edit.
So **a patch is larger than the source for tiny fixtures**: 2.1–2.4 times the 312-byte file for a
small rename, and 7.5–14 times a one-function file for a fix. At compiler scale it is small beside
what it edits: a 373-occurrence rename is 8.8 % of its 286 kB file, and a fix 0.26 %. The bytes a
patch changes are only its replacements — 1,865 bytes for the 373-edit rename. These are byte
counts, not token counts: no tokenizer was measured, and no token saving is claimed.

**What it added to the binaries.** Built in the same container: `nazm` is 3,478,400 bytes at N28
and **3,609,504** at N29, **+131,104 bytes (+3.8 %)**, for the patch module and the `patch`
command; `nazm-mcp` is 2,823,904 and **3,020,512**, +196,608 bytes, for the tool and its typed
request. No crate was added to `Cargo.lock` — one dependency edge: `nazm-mcp` names `serde`
directly for the request, and uses the `schemars` derivation `rmcp` already carries.

### Compact diagnostics and diagnostic detail — measured 2026-09-28

N30 keeps nothing between requests: an index or a detail is derived from one analysis of its
root and dropped. **Persistent N30 state: zero entries** — no index or detail cache, no history.
The ordinary compiler path, `nazm check --json` and the language server's diagnostics do no N30
work.

In process, contained, P1 (`target/n30/bench`, not committed), medians. `nazm.diagnostic/1` stream
is every diagnostic as `nazm check --json` prints it, one line each:

| Fixture | diagnostics | open + analyse | index | serialise | detail | index | one detail | `nazm.diagnostic/1` stream |
| --- | ---: | --- | --- | --- | --- | ---: | ---: | ---: |
| unterminated string (a syntax error) | 2 | 0.080 ms | 0.008 ms | 0.001 ms | 0.008 ms | 682 B | 881 B | 909 B |
| one type error, two secondary labels | 1 | 0.082 ms | 0.023 ms | 0.001 ms | 0.026 ms | 611 B | 948 B | 356 B |
| one error with help and a fix | 1 | 0.093 ms | 0.023 ms | 0.001 ms | 0.027 ms | 623 B | 1,280 B | 571 B |
| 20 functions, each with help, a fix and labels | 40 | 0.53 ms | 0.31 ms | 0.016 ms | 0.30 ms | 10,156 B | 1,276 B | 18,798 B |
| 500 type errors | 500 | 6.26 ms | 7.25 ms | 0.28 ms | 7.01 ms | 123,238 B | 950 B | 184,864 B |
| the compiler, with 20 such functions added | 40 | 83.0 ms | 16.0 ms | 0.024 ms | 16.2 ms | 10,862 B | 1,334 B | 19,500 B |

A detail costs what an index does: it derives the current diagnostics to find its id and check
its state, then projects one. At compiler scale both are about 16 ms after the 83 ms analysis —
the snapshot binding and the outline behind owners.

**What the bytes say.** An index is about 330 bytes of fixed structure — root, two 64-digit state
digests, counts — plus about 245 bytes per diagnostic (180 for a syntax error with no owner),
whatever its prose. So **for one or two
diagnostics the index is not smaller**: 611 bytes against a 356-byte diagnostic, 1.7 times, and
75 % of a two-diagnostic stream. From about twenty it is **54–67 % of the stream** — 10,156 against
18,798 bytes for forty diagnostics with help, fixes and labels, 123,238 against 184,864 for five
hundred terse ones — the saving larger where the prose is longer. Progressive disclosure: an
agent that reads the index of forty diagnostics and then one in full receives 11,432 bytes, 61 %
of the stream (12,196, 63 %, at compiler scale). These are byte counts, not token counts: no
tokenizer was measured, and no token saving is claimed.

Over `stdio`, the real `nazm-mcp` process, medians of the full `tools/call` round trip:

| Fixture | `nazm.diagnostics` | frame | `nazm.diagnostic_detail` | frame | server resident: before, after |
| --- | --- | ---: | --- | ---: | --- |
| unterminated string | 0.34 ms | 769 B | 0.37 ms | 968 B | 3.4 → 4.7 MB |
| 40 diagnostics | 1.30 ms | 10,243 B | 1.14 ms | 1,363 B | 3.4 → 5.0 MB |
| the compiler, 40 diagnostics | 98.4 ms | 10,948 B | 99.0 ms | 1,420 B | 3.3 → 36.7 MB |

Each frame is the result plus an 87-byte envelope: it travels once. 1,000 index, detail, edit and
repair calls in `crates/nazm-mcp/tests/protocol.rs` stay within a megabyte of resident memory,
which the test asserts.

**What it added to the binaries.** Built in the same container: `nazm` is 3,609,504 bytes at N29
and **3,675,040** at N30, **+65,536 bytes (+1.8 %)**, one 64 KiB step, for the diagnostics module
and the `diagnostics` command; `nazm-mcp` is 3,020,512 and **3,151,584**, +131,072 bytes, for the
two tools. No crate and no dependency edge was added: `Cargo.lock` is unchanged.

### Command and test summaries — measured 2026-09-28

N31 keeps nothing: a summary is derived from the command's own outcome, printed and dropped.
**Persistent N31 state: zero entries**; no timing is recorded, so none is reported. Without
`--summary-json`, `nazm check`, `nazm build` and `nazm test` do no N31 work.

Contained, P1 (`target/n31/bench`, not committed), medians of the real `nazm` process. `--json` is
what the same command prints with it (stdout and stderr); a summary is its one stdout line:

| Command | diagnostics | `--json` | summary | summary + one detail | time: `--json` → summary |
| --- | ---: | ---: | ---: | ---: | --- |
| check, clean | 0 | 3 B | 305 B | — | 1.0 → 1.2 ms |
| check, a syntax error | 1 | 250 B | 494 B | 1,137 B | 1.0 → 1.2 ms |
| check, no `main` | 1 | 311 B | 439 B | — (a command reference has no detail) | 1.0 → 1.2 ms |
| check, 40 diagnostics | 40 | 18,814 B | 7,723 B | 8,986 B | 1.4 → 2.5 ms |
| check, the compiler plus 40 | 40 | 19,516 B | 8,110 B | 9,438 B | 72 → 175 ms |
| build, success | 0 | 6 B | 317 B | — | 36.8 → 37.5 ms |
| build, 40 diagnostics | 40 | 18,814 B | 7,723 B | 8,986 B | 1.4 → 2.5 ms |

A summary costs one more analysis of the root: linking references to N30's ids analyses it as N30
does, in process 1.0 ms after a 0.3 ms check at 40 diagnostics and **98 ms after a 38 ms check on
the compiler** — the summary's dominant cost at scale, paid only when it is asked for. Serialising
is under 0.03 ms.

**What the bytes say.** A summary is about 300 bytes of fixed structure — command, status, exit
status, root, state digest, counts — plus about 185 bytes per diagnostic reference. So **for a clean
or single-diagnostic run it is larger than what the command prints**: 305 bytes against a
three-byte `ok`, 494 against a 250-byte syntax error. With forty diagnostics it is **41–42 % of the
`--json` stream**, and reading the summary and then one diagnostic in full is 48 %. These are
bytes, not tokens: no tokenizer was measured, and no token saving is claimed.

Tests, `--interpret-only`, the same run three ways:

| Run | `nazm.test/1` stream | summary | share | run time |
| --- | ---: | ---: | ---: | --- |
| 1 case, passing | 158 B | 161 B | 102 % | 1.8 ms |
| 10 cases, all passing | 1,590 B | 163 B | 10 % | 9.8 ms |
| 100 cases, 10 failing | 16,301 B | 1,134 B | 7.0 % | 91 ms |
| 1,000 cases, 5 failing | 164,790 B | 650 B | 0.39 % | 919 ms |

A test summary lists no passed case, so its size is about 150 bytes plus about 100 per failure,
whatever the run's size: **a one-case run is not smaller** (161 against 158 bytes), and a thousand
cases with five failures is 0.39 % of the stream. The run time is the tests', unchanged by asking
for a summary.

Over `stdio`, the real `nazm-mcp` process, medians of `nazm.command_summary`: 1.88 ms and a 7,808-byte
frame at 40 diagnostics; 167.5 ms and 8,195 bytes on the compiler, where resident memory rises from
3.3 to 37.4 MB (peak 54.8 MB: the check's analysis and N30's, one after the other). 1,000 calls through
edits, breaks and repairs in `crates/nazm-mcp/tests/protocol.rs` stay within a megabyte, which the
test asserts.

**What it added to the binaries.** Built in the same container: `nazm` is **3,675,040 bytes at N30
and at N31 — no change**, the additions inside the file's existing 64 KiB layout step; `nazm-mcp`
is 3,151,584 and **3,217,120**, +65,536 bytes, for the summary tool. No crate and no dependency edge
was added: `Cargo.lock` is unchanged.

### Documentation index and sections — measured 2026-09-29

`nazm docs` and `nazm-mcp`'s two documentation tools (N32, `architecture.md` §7.34), contained at
P1 on the M1 Pro, release builds, over the corpus as it stood when the gate ran: **14 documents,
1,042 sections, 48 of them grammar productions, 1,157,667 bytes**. Bytes, lines and Unicode scalars
only — no tokenizer was run, and no token claim is made.

| Retrieval | Whole document | Section body | `nazm.docs-section/1` | Document index + section |
| --- | ---: | ---: | ---: | ---: |
| A generic-semantics rule, `spec:generics/generic-definitions-are-checked-once-parametrically` | 142,128 B | 1,005 B (0.71%) | 1,577 B (1.11%) | 28,174 B (19.8%) |
| The diagnostic compatibility law, `diagnostics:what-compatibility-means` | 12,326 B | 1,363 B (11.1%) | 1,903 B (15.4%) | 3,923 B (31.8%) |
| An architecture invariant, `architecture:3/cache-key-correctness` | 259,005 B | 603 B (0.23%) | 1,135 B (0.44%) | 40,311 B (15.6%) |
| G78, `goals:G78` | 130,787 B | 409 B (0.31%) | 924 B (0.71%) | 75,055 B (57.4%) |
| One production, `grammar:function` | 16,667 B | 429 B (2.57%) | 1,732 B (10.4%) | 10,643 B (63.9%) |
| One capability row, `capabilities:25` | 192,375 B | 47,474 B (24.7%) | 48,528 B (25.2%) | 58,450 B (30.4%) |

Three things the table does not hide. **The whole-corpus index is 225,915 bytes**, 19.5% of the
corpus — larger than every document but `architecture.md` — because it lists 1,042 sections with a
64-hex digest each; asking for one document's index (`--document`) is what makes index-then-section
smaller than reading the document, and for the goals, whose 394 sections are short, it is still 57%.
**A tiny section is larger as JSON than as text**: the smallest, `capabilities:tooling`, is 12 bytes
of body and 610 of detail. And a long section is long: capability row 25 is a quarter of its
document. What selective retrieval avoids is the unrelated text, not the metadata.

| Cost | Median |
| --- | ---: |
| read the 14 files | 1.89 ms |
| load: read, section, resolve links, digest | 19.65 ms |
| build the index / serialise it | 0.110 ms / 0.179 ms |
| look up and extract one section / serialise it | 0.0013 ms / 0.0017 ms |
| `nazm docs --index --json`, `--section ID --json`, `--section ID` (process) | 22.3 / 22.0 / 21.9 ms |
| `nazm.docs_index` / `nazm.docs_section` over MCP | 23.7 ms (226,001-byte frame) / 21.2 ms (1,010-byte frame) |

Parsing dominates, and it is paid at every request because nothing is kept. The MCP server's
resident memory was 3.5 MB at start, 10.9 MB after 60 calls and 13.6 MB after 1,000 more over
the full corpus, peak equal to the last — the growth of an allocator's arenas under a 226 KB frame,
not a cache, of which there is none; over a small corpus `crates/nazm-mcp/tests/protocol.rs` holds
1,001 calls through edits within a megabyte. `nazm` grew from 3,675,040 to 3,806,112 bytes
(+131,072) and `nazm-mcp` from 3,217,120 to 3,413,728 (+196,608); one new crate, no new external
dependency.

### Repository map and task contexts — measured 2026-09-29

`nazm repo` and `nazm-mcp`'s two repository tools (N33, `architecture.md` §7.35), contained at P1
on the M1 Pro, release builds, over this repository at `16b5b27`. **Bytes, never tokens**: no
tokenizer was run, and no model.

**The map is 111,263 bytes — 4.1 % of what it indexes**: 1,192,961 bytes of the fourteen canonical
documents, 1,024,292 of Nazm source in the three source roots, and 472,905 of Cargo manifests, the
mutation catalogue and the published schemas (2,690,158 in all). It lists 15 packages, 84 targets,
3 source roots, 30 compilation roots (7 incomplete — they needed syntax recovery), 34 modules, 477
durable definitions, 14 documents, 16 schemas and 12 profiles, and delegates 1,038 sections, 601
mutations, 256 catalogue tests and 16 diagnostics to the answers that enumerate them.

**Seven representative tasks** (`crates/nazm-repo/tests/benchmark.rs`). The naive baseline is what
an agent without the planner reads: the whole files the facts are in, the whole documents the
sections are in, and — for tests — the catalogue entries a search for the file's path returns. Every
row's mechanical checks pass against the authorities themselves (N27's packet, N32's section, the
catalogue parsed independently): the exact signature and source, each direct dependency and
signature type, each direct caller in both compilations, each killer of a mutation inside the seed,
the diagnostic's code, message and owner's source, each linked or generating section's exact text.

| Task | Naive bytes | Context bytes (JSON) | Reduction | Source + text included | Items | Checks |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A understand `analyse.nz::fn check_program` | 356,043 | 20,059 | 94.4 % | 6,642 | 20 | 22 |
| B edit `module.nz::fn attach_core` | 348,233 | 7,844 | 97.7 % | 3,242 | 10 | 7 |
| C diagnose `N0300` in `return_type_mismatch.nz` | 123 | 1,925 | **−1,465 %** | 84 | 2 | 3 |
| D understand `spec:returns` | 142,128 | 3,769 | 97.3 % | 2,540 | 2 | 3 |
| D understand `guide:the-syntax/record` | 53,386 | 2,768 | 94.8 % | 1,561 | 2 | 3 |
| E tests of `package:nazm-docs` | 26,649 | 5,577 | 79.1 % | 0 | 19 | 19 |
| E tests of `module.nz::fn core_module` | 18,223 | 1,911 | 89.5 % | 186 | 4 | 2 |

Row C is recorded as measured: the whole program is 123 bytes, and a structured context with its
diagnostic, owner, retrieval and state is larger than the program. The reduction appears at the
scale of a real module, not of a one-function example.

**Latency**, in-process, three runs each after the first: a map **433 ms the first time in the
process, then 372–380 ms**, serialising 0.16 ms. A task whose seed needs no compilation — a section,
a package's tests, a diagnostic of a small example — **50–54 ms**, almost all of it reading and
digesting the repository's inputs for the state; `lex.nz::fn is_alnum` 170 ms, `attach_core`
(edit: four compilations) 240–244 ms, `check_program` (twenty packets from the largest
compilation) 479–605 ms. Serialising a context: 0.01–0.08 ms. The command line, one process each:
the map 352 ms; tasks 46 ms (`package:nazm-docs`), 50 ms (`spec:returns`), 237 ms
(`attach_core`), 579 ms (`check_program`).

**Memory.** `nazm-mcp` over this repository, five mixed seeds in rotation: **1,000 task contexts at
100.5 ms each on average, resident memory 73,224 kB settled and 73,224 kB after them.** Flat
across those calls — which is what was measured, not a claim that nothing can ever leak. The
protocol suite also runs 1,000 task contexts over a small repository on every workspace run, and
asserts they stay within 1 MB.

**What it added to the binaries.** `nazm` is **4,527,008 bytes, from 3,806,112 at N32-H3:
+720,896 (+19 %)**, for `nazm-repo` and the TOML reader it brings into the binary (`toml` was
already in the lockfile, for `xtask`). The compiler's own speed is unchanged: `cargo xtask contained
bench`, back to back at `75a2859` and at `16b5b27`, has a median ratio of **1.02** over 38
measurements, the largest 1.26 on a 1.9 → 2.4 ms check, under the 5 ms reporting floor. Both are
about twice the 2026-09-21 baseline for the `nazm` process's own stages — compiled programs and
the C reference are unchanged — a drift that predates N33 and is not attributed to it here.

**The MILESTONE gate** (`gate-plan`: full lifecycle, the MCP entries, every benchmark family —
the root manifest and `xtask/` changed), every cache empty after a disk-full Docker failure was
cleaned up: mutation 1,076 s, workspace 319 s (1,757 passed across 99 suites; the container's
memory peak read 4 GiB, its limit, with no OOM kill — a cold build's page cache counts toward
it), lifecycle 188 s, selfhost 57 s, bootstrap 37 s, release benchmarks 535 s (re-run: the first
attempt's script used a shell form the container's `sh` refused, and failed in 10 s before
measuring anything), compiler benchmark 200 s, checks 16 s — **2,428 s, 40.5 minutes**. The
benchmark stage's release builds and the mutation stage's pristine build started from nothing.

### The post-v1 baseline — N75, frozen 2026-10-02

The release gate's benchmark stage at the candidate `34e2380`: `cargo xtask contained bench`, one
CPU, 4 GiB, offline, with the host otherwise idle. Milliseconds, minimum / median, five runs; the
record is kept with the gate's evidence (`bench/record.json` of that run). It replaces N48's as the
reference; no single score is derived from it.

| program | check | build `-O0` | build `-O2` | interpret | native `-O2` |
| --- | --- | --- | --- | --- | --- |
| arith | 2.8 / 2.9 | 88.2 / 95.3 | 98.9 / 99.7 | 2,749.5 / 2,765.2 | 4.5 / 4.5 |
| calls | 3.1 / 3.3 | 91.4 / 93.7 | 98.1 / 100.1 | 1,531.2 / 1,534.7 | 0.9 / 1.0 |
| channels | 2.9 / 3.1 | 99.3 / 103.0 | 149.8 / 150.5 | 1,036.9 / 1,335.9 | 1,259.8 / 1,331.5 |
| floor | 2.8 / 3.0 | 88.7 / 90.4 | 95.3 / 99.2 | 2.4 / 2.5 | 0.3 / 0.3 |
| sequences | 3.0 / 3.1 | 91.8 / 93.5 | 112.7 / 115.6 | 764.5 / 771.1 | 2.3 / 2.7 |
| sieve | 3.0 / 3.1 | 91.3 / 95.1 | 116.7 / 117.5 | 1,003.9 / 1,013.2 | 2.9 / 2.9 |
| strings | 2.9 / 3.2 | 91.4 / 94.3 | 121.7 / 129.0 | 48.7 / 48.9 | 7.1 / 7.4 |
| compiler | 107.2 / 107.4 | 1,924.7 / 1,939.6 | | | |

`sieve/reference-c` 1.8 / 2.6; `reference-python` not measured (no Python in the image). Peak
memory, bytes: interpreted 5.7 M (`arith`) to 13.0 M (`strings`); native 1.2 M to 5.8 M.

**Against N48**, medians: the compiler's own `-O0` build is 9 % faster (2,131 → 1,940 ms), the
direction N59's SSA temporaries predicted; checking is slower — `compiler/check` 97.1 → 107.4 ms
(+11 %), the small programs 0.2–0.5 ms each — over the twenty-six milestones' checker work, not
attributed to one; `-O2` builds of `channels`, `strings`, `sieve` and `sequences` are 11–15 % slower,
and `channels` runs 9 % slower natively (1,225 → 1,332 ms) and interpreted, with a wide spread
(1,037 / 1,336) that one run cannot settle. Nothing here is a claim against another language.

**Beside it, on the host** (Apple M1 Pro, release builds, while the contained mutation campaign used
four of ten cores — indicative, not a baseline): 10,000 tasks started and joined in scopes of 50 in
143.5 ms (LLVM) / 159.6 ms (Cranelift); a channel round trip 4.66 / 4.82 µs; a streamed value 58 /
60 ns; 400 M iterations over 1, 2 and 4 tasks 1,655, 828 and 417 ms (LLVM; Cranelift the same within
1 ms). A three-package build: lock 4.4 ms, check 6.1 ms cold and 7.4 ms warm, build 211.6 ms with no
cache and 82.0 ms warm. The figures of N62–N68 above — a board image's bytes and stack, a vectorised
sum, a GPU map, field-by-field vectors — were measured at their milestones and not again here; their
run-verified tests passed at the candidate. A contract's static gas bound for the `contract`
template: `transfer` 59,420, `pause` 30,824, 831 bytes of runtime code; the EVM run stays inside it.

### The v1 baseline — N48, frozen 2026-10-01

**The numbers a later milestone compares against**, in place of `bench/baseline-linux-aarch64.json`,
which was recorded at `ada3ae2` — before N1 — and against which every row now reads 2–6× slower for
reasons forty milestones old (the per-milestone gate runs have been compared with each other, not
with it, since N33). That file is left as it is; these are the v1 figures.

Contained compiler benchmark (`cargo xtask contained bench`), Linux aarch64, 1 CPU, offline, at
`74727bb` (`a8a7b3f` and `84d3c80` differ from it in tests and the mutation catalogue only), min /
median ms:

| program | check | build `-O0` | build `-O2` | interpret | native `-O2` |
| --- | --- | --- | --- | --- | --- |
| arith | 2.5 / 2.6 | 88.6 / 89.0 | 95.9 / 96.0 | 2,753.8 / 2,760.9 | 4.5 / 4.5 |
| calls | 2.6 / 2.7 | 91.4 / 94.4 | 96.6 / 104.6 | 1,466.8 / 1,471.1 | 1.0 / 1.0 |
| channels | 2.8 / 2.9 | 96.3 / 99.0 | 122.5 / 130.8 | 951.6 / 1,012.0 | 1,203.5 / 1,225.2 |
| floor | 2.5 / 2.8 | 88.2 / 90.2 | 92.9 / 97.9 | 2.0 / 2.0 | 0.3 / 0.3 |
| sequences | 2.6 / 2.6 | 90.6 / 94.9 | 102.3 / 104.4 | 764.4 / 771.8 | 2.3 / 2.4 |
| sieve | 2.7 / 2.7 | 91.0 / 92.6 | 104.4 / 104.8 | 998.7 / 1,025.2 | 2.9 / 3.0 |
| strings | 2.5 / 2.9 | 101.4 / 102.7 | 111.6 / 115.1 | 49.1 / 51.6 | 7.4 / 7.6 |
| compiler | 92.3 / 97.1 | 2,045.7 / 2,131.0 | | | |

`sieve/reference-c` 1.7 / 1.8 ms. The run was not on an idle machine (host builds ran beside it),
so a later comparison should be made on one, or as an interleaved A/B like the one below.

**Against N39's own gate run** the interpreted rows hold within 0.3–6 % (`calls` 1,467 → 1,471,
`sequences` 770 → 772, `arith` 2,748 → 2,761, `sieve` 999 → 1,025, `strings` 48.7 → 51.6) and
`compiler/check` 93.0 → 97.1; `compiler/build-O0` moved 1,847 → 2,159 at N40 (2,131 now). An
interleaved A/B on the host, release, N39's `ab0a761` against HEAD on the same input (N39's
`compiler/emit.nz`) attributes it: `nazm check` 59.6 / 60.1 ms (1.01), `arith` interpreted
617.6 / 611.3 ms (0.99), **`nazm build -O0` 1,025 / 1,201 ms (1.17)**. The build's extra time is
clang's: the MIR emitter (N40) hands it 8.44 MB of LLVM text against 6.77 MB, 177,239
instructions against 138,522, and 13,836 `alloca`s against 3,275 — a stack slot per MIR local,
which `-O0` does not promote — and clang's compile is 1,091 ms of the build's 1,256. No program's
behaviour changed. Promoting locals to SSA in the emitter is the evidence-backed fix, and it is a
backend change for after v1, not a release-audit one.

### Restriction profiles — N47, measured 2026-10-01

Host, release, `nazm check --no-cache`, median of 9. The rules themselves cost nothing measurable:
they read facts the check already settled. What a profile costs is the check it runs to get those
facts, before the command's own: a 2,000-function pure program takes 31.4 ms under `general`, 56.2 ms
under `embedded` (accepted: two checks) and 31.8 ms under `cyber` (refused before the command
checks); `--profile-report` alone costs the same second check, 56.0 ms. On the compiler's own
`compiler/emit.nz`, refused under `embedded` and `critical`, 59.4 ms against 59.5 ms. Reusing the
command's check for the profile would remove the second one; v1 does not.

### Packages — N46, measured 2026-10-01

Host, best of 5 (`what_packages_cost`, `crates/nazm-cli/tests/packages.rs`, `--ignored`), the
three-package diamond of the tests: `nazm lock` 5.2 ms (resolution, three manifests, three
digests, the lockfile's bytes compared and left alone); `nazm check` 8.5 ms with `--no-cache` and
10.0 ms from a warm cache (for three one-function packages the cache's reads cost more than the
checking they save); `nazm build --no-cache` 210.9 ms and a warm build 86.9 ms (clang is most of
both). Two hundred packages, each depending on up to three earlier ones: resolving, locking,
checking and locking again takes 2.6 s in all, dominated by checking 200 modules. Every existing
program is unaffected: nothing that is not a package takes the new paths, and the only change to
the link — `ZERO_AR_DATE=1`, and linking under the final file name — makes executables
reproducible without changing what they contain.

### The standard library — N45, measured 2026-10-01

The standard library is Nazm source, so what it costs is what the code generators make of it.
Host, `-O2`, best of 5 (`what_the_standard_library_costs`, `crates/nazm-cli/tests/stdlib.rs`,
`--ignored`):

| | LLVM | Cranelift |
| --- | --- | --- |
| `text_split` of a 120 KB string into 20,000 pieces, 50 times (1 M pieces) | 111.9 ms | 84.6 ms |
| `text_find` over 100,001 bytes, 20 times (2 M positions) | 27.2 ms | 27.3 ms |
| `text_parse_int` of `int_to_str(i)` for 1 M values | 83.0 ms | 102.8 ms |
| `ints_sum` and `ints_max` over 1 M elements, 20 times | 17.4 ms | 72.6 ms |

About 110 ns a piece to split (each piece a borrowed slice pushed into a `Strs`), 14 ns a
position to search (a slice and a byte comparison), 80–100 ns to format and parse an integer. The
sequence loop is where the backends differ most: LLVM keeps the loop in registers; Cranelift, which
holds every local in a stack slot (§7.43), reloads it each iteration. No program's code changed:
nothing in the tree imports a standard module.

### The formal core's check — N61, measured 2026-10-02

`cargo test -p nazm-formal`, debug, M1 Pro: 94,352 programs enumerated, 27,680 well-typed, each
typed program checked and run in-process by the interpreter and each program checked by the
checker — 54 s for the five properties. The bound is five nodes; six would be roughly twenty times
as many programs.

### Scalar temporaries — N59, measured 2026-10-02

The LLVM emitter now gives no slot to a scalar temporary assigned once and read only in its block
(§7.61). M1 Pro, Apple clang 21.0.0, the debug `nazm` before (`0acdf20`) and after, interleaved,
on a host also running a contained mutation session — so the spread is wide:

| | before | after |
| --- | --- | --- |
| `compiler/emit.nz` at `-O0`: `alloca`s | 13,867 | 6,328 |
| its LLVM text | 8.47 MB, 177,531 instructions | 7.69 MB, 157,328 |
| `nazm build -O0` of it, five runs | 1,992 / 2,067 / 2,087 / 2,091 / 2,101 ms | 1,937 / 1,982 / 1,983 / 2,011 / 2,993 ms |
| a 20 M-iteration loop and `fib(27)`, built `-O0`, three runs | 133 / 144 / 759 ms | 103 / 105 / 443 ms |

The build's median moved 5 %: clang's `-O0` time is not mostly slots. The `-O0` program is about a
quarter faster, because a promoted temporary is a register and not a store and a load. `-O2` is
unchanged in kind — LLVM promoted these already. Cranelift was not touched: its scalar locals were
already SSA variables.

### Targets — N57, measured 2026-10-02

One program (a recursive `fib(20)` and a task sending a string through a `Chan[Str]`), built on an
M1 Pro (macOS, Apple clang 21.0.0) by both backends for each target:

| target | how | result |
| --- | --- | --- |
| `aarch64-apple-darwin` | built and run on the host | `from task 6765`, all reclaimed |
| `x86_64-apple-darwin` | built and linked on the host, run under Rosetta | the same, both backends |
| `aarch64-unknown-linux-gnu` | `--objects` on the host; `cc 00-m0.o 01-runtime.o 02-entry.o -lpthread` and run in `nazm-contained:1.98.1` (`docker run --network none`) | the same, both backends |
| `x86_64-unknown-linux-gnu` | `--objects` on the host; ELF x86-64 objects | compile-only: no amd64 image or emulator here |

Cross-host reproducibility — the same target's objects from two hosts — was not measured.

### Field-by-field vectors — N68, measured 2026-10-02

1,000,000 records of eight `Int` fields in a `Vec`, `--opt-level 2`, Apple M1 Pro; seconds, the
median of the last four of five runs (fill included, ~0.13 s of each):

| loop | element by element (`aos`) | field by field (`soa`) | hand-written, eight `Ints` |
| --- | --- | --- | --- |
| sum of one field, 500 passes | 0.58 | 0.21 | 0.21 |
| sum of all eight fields, 50 passes | 0.10 | 0.20 | — |

The layout wins where a loop reads few fields of wide records and loses where it reads them all;
nothing here chooses between them for a program.

### A GPU map — N67, measured 2026-10-02

`collatz` (steps to 1, plus a helper call) over 1..=1,000,000 on the M1 Pro's GPU through macOS's
OpenCL 1.2 (`nazm accel --no-check`, three runs), against the same sum by a native `-O2` build on one
CPU core:

| | GPU | CPU, one core |
| --- | --- | --- |
| kernel build | 1.6–3.3 ms (the driver's cache warm; 300 ms cold) | — |
| host to device, 8,000,004 B | 2.3–4.4 ms | — |
| run, to the synchronisation | 22.6–29.6 ms | 190 ms |
| device to host, 8,000,004 B | 1.0–1.8 ms | — |
| checksum | 144434412 | 144434412 |

End to end about 6× one core; not compared against all cores, and not a claim about any other
kernel: a map whose elements do little work loses to its transfers.

### Vectorised sums — N66, measured 2026-10-02

`total(v)`, the counted summation loop, against the same loop with its operands swapped
(`s = ints_get(v, i) + s` — the same work and checks, not the idiom), each summing `v` until 2·10⁹
elements are added, `--opt-level 2`, Apple M1 Pro, Apple clang 21. Seconds, steady state (the median
of the last four of five runs; the first run of each is ~0.35 s slower, warming):

| elements | scalar | vectorised | |
| --- | --- | --- | --- |
| 1,000 | 0.77 | 0.40 | 1.9× |
| 100,000 | 0.75 | 0.39 | 1.9× |
| 10,000,000 | 0.79 | 0.45 | 1.75× (memory-bound) |
| 100,000 of ±2^60 (every block on the checked path) | 0.75 | 1.00 | 0.75× — the fallback's scan |

The executables differ by 24 bytes: `nz.ints_sum` is in every runtime that has sequences, used or not.
x86_64 was not measured. At `-O0` the runtime is not vectorised and the speed-up is not claimed.

### Stack: the stated bound and a painted run — N63, measured 2026-10-02

`crates/nazm-cli/tests/realtime.rs`'s ignored test: each program built for `aarch64-unknown-none`
with `--stack-watermark`, `bounds.json` read, the image booted on QEMU `virt` (`nazm-qemu:n62`),
and the deepest painted word the run overwrote reported on the UART. Bytes, measured / bound:

| program | `-O0` | `-O2` |
| --- | --- | --- |
| three counted loops and a call (`BOUNDED`) | 176 / 192 | 24 / 48 |
| a four-deep call chain in a 1,000-turn loop | 304 / 320 | 24 / 48 |
| a nine-argument call in a loop | 264 / 288 | 24 / 48 |

The bound is the claim; each measurement is one run on an emulator and says only that this run
stayed inside it. The gap is the deepest path's frames that this run's path did not take (at `-O2`,
the failure path through `nz.fail`). No timing is reported: an emulator's is not a machine's.

### A freestanding image — N62, measured 2026-10-02

`crates/nazm-cli/tests/freestanding.rs`'s program (`Hi` through `mmio_write32`, a loop and a
recursive `fib(10)`), built on the host with `--target aarch64-unknown-none --objects`, linked with
`ld.lld -T link.ld` and booted with `qemu-system-aarch64 -M virt -cpu cortex-a53 -nographic -nic none
-semihosting` in `nazm-qemu:n62` (`docker/qemu.Dockerfile`, `docker run --network none`):

| | |
| --- | --- |
| image | `.text` 1,424 B, `.rodata` 583 B, `.bss` 24 B — 2,421 B, plus the 64 KiB stack the script reserves |
| output | `Hi`, `100`; status 0 |
| QEMU start to exit, three runs | 50, 27, 29 ms — QEMU's own start-up, not the program |
| an overflow, a deep recursion | `N0400`, `N0408` on the UART; status 2 |

One board, emulated: no hardware was run, and the timing says nothing about one.

### Select and the event count — N54, measured 2026-10-02

N54 added a process-wide event count that every typed send and close advances (§7.56), so what is
measured is what that costs a channel and what a select costs over a receive. Host (Apple M1 Pro,
Apple clang 21.0.0), LLVM `-O2`, three runs of one program at `4ce95f3`+N54, with a contained
mutation session running beside it, so the spread is wide and the figures are an order of
magnitude, not a benchmark:

| | three runs |
| --- | --- |
| 100,000 typed send-then-receive pairs on one task, a 1-slot `Chan[Int]` | 20–30 ns a pair |
| 100,000 round trips between two tasks through two `Chan[Int]`, received with `chan_recv_of` | 6.7–7.4 µs each |
| the same, the worker waiting in `chan_select_of` over one channel | 6.4–7.1 µs each |

- **The event count costs a send one atomic increment and one load** when nobody selects —
  a single-task pair stays in tens of nanoseconds.
- **A select costs no more than a receive** at one channel: both wait on a condition variable and
  are woken by a broadcast; the wake-up dominates.
- The round trip is slower than N44's `Int`-channel 4.6 µs (best of 5, quiet host); this run did
  not repeat N44's conditions, so no regression is claimed or excluded. What a pool would change —
  the thread wake-up behind every figure here — is §7.56's designed and unbuilt part.

### Tasks and channels — N44, measured 2026-10-01

N44 kept the scheduler — one OS thread per task, joined by its scope — and wrote its contract
down, so what is measured is the baseline anything more elaborate would have to beat. Host, `-O2`,
best of 5 (`what_tasks_and_channels_cost`, `crates/nazm-cli/tests/scheduler.rs`, `--ignored`),
with a one-core mutation container running beside it:

| | LLVM | Cranelift |
| --- | --- | --- |
| 10,000 tasks started and joined, in scopes of 50 | 131.7 ms (13.2 µs each) | 128.8 ms (12.9 µs) |
| 100,000 channel round trips between two tasks | 462.7 ms (4.63 µs each) | 468.2 ms (4.68 µs) |
| 1,000,000 values streamed through a 1,024-slot channel | 59.7 ms (60 ns each) | 60.3 ms (60 ns) |
| 400 M loop iterations over 1 / 2 / 4 tasks | 1,653 / 829 / 419 ms | 1,655 / 829 / 416 ms |

- **Scaling** is linear to four tasks (3.95× at 4), because independent tasks share nothing but
  the atomic accounting counters.
- **Memory plateau.** A program running rounds of 100 tasks that each allocate a string and
  report on a channel: peak RSS 3.75 MB after 5,000 tasks, 3.83 MB after 50,000, 3.80 MB after
  500,000 (`/usr/bin/time -l`, 16 KiB pages). No per-task growth: every thread is joined and every
  block freed by its scope.

### The runtime contract — N43, measured 2026-10-01

N43 moved the runtime into `crates/nazm-lir/src/runtime/` (its own crate, `crates/nazm-runtime/`, since N53) and wrote its contract down; it changed
no instruction, so what is measured is what the runtime costs, now that there is a harness that
calls it directly.

- **Nothing a program runs changed.** All 205 sources, N42 (`9b35099`) against N43 release builds,
  `nazm build --no-cache --emit-ir`: the same status and diagnostics for all 205, and every LLVM
  file of the 104 that build — runtime and entry units included — byte-identical.
- **Cold start.** An executable whose `main` returns `0`, host, 200 runs: median 3.65 ms (LLVM),
  3.73 ms (Cranelift); p10–p90 3.25–4.22 and 3.37–4.30 ms. Nothing is initialised lazily and the
  entry does four stores before calling `main`, so this is the process's own cost.
- **What a runtime call costs**, host, clang `-O2`, 10 million iterations, best of four: a string
  backing allocated and released 15.3 ns; a retain and a release of a shared backing (two atomic
  operations) 11.6 ns; a sequence push through `nz.arr_grow`, amortised over doubling, 1.7 ns.
- **Memory.** The same harness peaks at 84.1 MB, which is the 10-million-element sequence
  (80 MB of data after doubling) and nothing else; the direct-test harness runs at under 10 MB.
- **Size.** The smallest executable is 50,680 B (LLVM) and 52,832 B (Cranelift, which links the
  whole runtime); unchanged by N43.

### Foreign calls — N42, measured 2026-10-01

A foreign call is a direct call with the C convention and no failure check after it, so what is
measured is what one costs against a Nazm call, and what the feature costs a build.

- **A call into C costs what a Nazm call costs.** Host, `-O2`, best of 3, a checked loop of 10
  million iterations whose body is one call (`t = add(t, i)`) and an overflow-checked `+`: calling
  a clang `-O1` C `c_add` 56.6 ms with the LLVM backend and 53.8 ms with Cranelift; the same loop
  calling a Nazm `add` 48.0 ms (LLVM, which can inline it) and 62.3 ms (Cranelift, which cannot,
  and checks for a failure after each call). About 5–6 ns an iteration either way; nothing is
  marshalled, because only `Int` and `Bool` cross.
- **Nothing else moved.** A build of the loop takes 0.14 s with or without the foreign
  declaration and `--link`. A program with no foreign declaration is untouched: all 205 sources in
  the tree, N41 (`eceeccc`) against N42 release builds, `nazm build --no-cache --emit-ir` — the same
  status and diagnostics for all 205, and every LLVM file of the 104 that build byte-identical.

### The Cranelift backend — N41, measured 2026-10-01

- **Every buildable program behaves identically under both backends**: the 104 programs of the
  205-source corpus that build, stdout, stderr, exit status and the memory report.
- **Build time.** `nazm build --no-cache compiler/emit.nz` at `-O0`, host: Cranelift 0.74 s, LLVM
  (clang) 1.21 s. The Cranelift figure still includes clang compiling the runtime's LLVM text and
  linking.
- **Code size.** The resulting executable: 1.89 MB (Cranelift) and 1.87 MB (LLVM `-O0`).
- **Reuse.** An unchanged unit's Cranelift object is found by a key computed from MIR before any
  code is generated, so a warm rebuild generates nothing for it
  (`an_unchanged_unit_is_reused_by_its_key_and_a_changed_one_is_regenerated`).
### MIR — N40, measured 2026-10-01

- **Nothing changed in any program.** All 205 sources, N39 against N40: `check`, `run` and `build`
  diagnostics identical, and the 104 buildable programs' stdout, stderr, exit status and memory
  report identical.
- **What each phase costs**, container, release, median of 15 (`what_each_phase_costs_on_the_compiler`):
  `compiler/emit.nz` parse 29.3 ms, checking +19.3, Core IR +11.6 (verifying 0.4), **MIR +8.1
  (verifying 2.3)**, layout +1.2, LLVM text +40.1 — 111.9 ms; a 3,001-function chain 96.8 ms, of
  which MIR 8.3. On the host during development: MIR 5.6 ms (verifying 2.0) in an 85.7 ms pipeline.
- **Its size.** `compiler/emit.nz` is 432 MIR functions, 4,633 blocks, 20,077 statements and 13,835
  locals; the largest function, `check_all`, 427 blocks and 1,238 locals.
- **Scale.** Linear: 1.5 ms for a function of 250 bindings, 6.9 ms for 1,000 (release, host).
- **Size.** Contained release: `nazm` 4,920,224 B (+65,536 against N39's 4,854,688), `nazm-mcp`
  4,265,696 B, unchanged.

### Core IR — N39, measured 2026-09-30

Core IR is a lowering every checked program now passes through (`architecture.md` §7.41), so what
is measured is what it costs a build and a run, what it changed in what programs do, and what
the interpreter gained by running it instead of the tree.

- **Nothing changed in any program.** All 205 `.nz` sources in the tree, under the N38 release
  build (`git archive 5e7e487`) and the N39 one, host: `nazm check --no-cache` output and status
  identical for 205; `nazm run` stdout, stderr, `NAZM_MEMORY_REPORT` line and status identical for
  205 (95 exit 0, the rest their documented diagnostics); `nazm build --no-cache --emit-ir`
  identical output for all 205, and **every one of the 383 LLVM IR files the 104 buildable ones
  emit byte-identical**. The native backend now lowers Core IR and produces the same text,
  hidden slots and all.
- **What each phase costs**, release, host, median of 15
  (`what_each_phase_costs_on_the_compiler`, `crates/nazm-cli/tests/core_ir.rs`), cumulative:
  `compiler/emit.nz` parse 16.7 ms, checking +15.9, **to Core IR +5.6** (verifying it 0.4 of that),
  to LIR +4.1, to LLVM text +29.9 — 72.1 ms; `compiler/check.nz` parse 9.3, checking +8.6, to
  Core IR +2.6; a 3,001-function chain parse 14.0, checking +12.2, to Core IR +3.4, to LIR +4.0,
  to LLVM text +20.4.
- **Whole commands, N38 against N39**, release, host: `nazm check --no-cache compiler/emit.nz`
  57.3 and 57.5 ms, `compiler/check.nz` 34.6 and 34.5 (median of 25) — checking builds no Core
  IR; `nazm build --no-cache compiler/emit.nz` 947 and 958 ms (+1.1 %, median of 7, clang
  dominating); `examples/primes.nz` 137.6 and 136.3 ms.
- **The interpreter got faster**, because it no longer searches frames for a name or asks the
  resolution at every call and field (median of 7): `bench/programs/calls.nz` 897.6 → 430.9 ms
  (−52 %), `sieve.nz` 424.4 → 245.0 (−42 %), `sequences.nz` 330.9 → 199.1 (−40 %), `strings.nz`
  25.8 → 19.7 (−24 %), `arith.nz` 733.6 → 615.0 (−16 %), `channels.nz` 271.8 → 256.7 (−6 %),
  `floor.nz` 4.5 → 4.4.
- **Memory.** Peak RSS, host, median of 5, N38 and N39: `nazm check compiler/emit.nz` 29.3 and
  29.2 MB; `nazm build compiler/emit.nz` 64.0 and 63.5; `nazm run` of the 3,001-function chain
  51.2 and 50.5; `nazm build` of it 68.2 and 68.2; `nazm run` of a 2,000-binding function 37.1 and
  38.6. The tree is dropped as soon as Core IR exists, in both `nazm run` and `nazm build`, and the
  interpreter no longer clones the tree and the resolution into itself; what a run keeps is Core
  IR alone.
- **Scale.** Lowering is linear in functions and in one function's size
  (`crates/nazm-core/tests/core_ir.rs`, a 3,000-function program and a 1,000-binding function
  against a quarter of each). Checking is not linear in the number of bindings one function has:
  `nazm check` of a function with 1,000, 2,000 and 4,000 bindings takes 0.05, 0.19 and 0.69 s under
  N38 and N39 alike — a cost of the checker's own, found by N39's stress test and not addressed by
  it.
- **In the container the interpreter's gain does not hold everywhere.** The compiler benchmark's
  interpreted rows, against N38's own gate run, Linux, 1 CPU: `calls` 1,672 → 1,467 ms (−12 %),
  `strings` 48.6 → 48.7, `sieve` 948 → 999 (+5 %), `sequences` 723 → 770 (+6.5 %), `arith` 2,495
  → 2,748 (+10 %). Those rows had held within about 2 % from N33 to N38, so the slowdown is real on
  that platform while the same programs run 16–52 % faster on the host. Why is not established: the
  interpreter's work per step fell on paper (a slot index for a name search, no frame per region),
  and a profile on Linux is what would say. No optimisation was attempted — N39 adds no optimiser.
- **The compiler benchmark otherwise** moved within a few per cent of N38's run: `compiler/check`
  98.2 → 93.0 ms, `compiler/build-O0` 1,779 → 1,847 (+3.9 %), every program's `-O0` and `-O2`
  build within −4 % to +3.5 %. The 21 drifts it reports are the same pre-N33 baseline ones.
- **In the container**, the phase benchmark (`what_each_phase_costs_on_the_compiler`, release,
  median of 15): `compiler/emit.nz` parse 29.0 ms, checking +20.5, to Core IR +9.3 (verifying 0.4),
  to LIR +7.5, to LLVM text +36.6 — 102.8 ms; the 3,001-function chain 84.1 ms, of which Core IR 7.4.
- **Size.** Linux release, contained: `nazm` 4,723,616 → 4,854,688 B (+131,072, +2.8 %); `nazm-mcp`
  4,265,696 B, unchanged, since the server does not reach the lowering.
- **The planner and servers**, contained: `repo --map` 391–411 ms, the four task contexts 52–635 ms,
  1,000 MCP task contexts 115.5 ms each, the docs index 26.1 ms — within N38's. The MCP memory law
  saw one allocator step during warm-up and a flat measured window (112,012–112,168 kB), `Flat`.

**The gate**, offline, at `ab0a761`: mutation 13,876 s over six sessions (37 of 37 caught — 23 at
tier 1 with killers verified, 13 at tier 2, 1 at tier 3; 34 at `ab0a761` and the three entries
repaired after it at `eb80464`; `capability-matrix.md` area 30), workspace 478 s (1,903 passed, 0
failed, 25 ignored, 121 suites; `memory.peak` again read its 4 GiB limit with no OOM kill), lifecycle
186 s (19 of 19 — selected in full, since the harness's rules and catalogue changed), selfhost 61 s
(38 of 38), bootstrap 39 s (C2 = C3, IR identical, executables identical once the linker UUID is
removed), release benchmarks 960 s, compiler benchmark 212 s, checks 8 s (19 of 19, the new *core
ir boundary* among them) — every stage passed, with no network (`NETWORK-ABSENT`). After the gate,
`eb80464` changed one test's recursion depth, from 1,000 to 200 (area 30); that binary passes 116 of
116 on the host.


### Provenance — N38, measured 2026-09-30

Provenance is a checker fact and a whole-program solve over kept facts (`architecture.md` §7.40),
so what is measured is what it costs the compiler, what it leaves in programs, and what it did to
the servers that analyse the repository.

- **Nothing in the program.** `nazm build --emit-ir -O0` under the N37 and N38 release builds gives
  byte-identical IR for `examples/capabilities/workers.nz` and `ledger.nz`, `examples/effects/tasks.nz`,
  `examples/provenance/tally.nz`, `examples/primes.nz`, `examples/pipeline.nz` and `compiler/emit.nz`
  (host, built and not run). `nazm-lir` reads no provenance.
- **Compatibility.** All 203 `.nz` sources at N37's HEAD (`git archive 6577c14`) give byte-identical
  `nazm check --no-cache` output and exit status under the N37 and N38 release builds: none is newly
  refused, since no existing function with a declared effect set writes to a file-derived path.
- **Checking time.** Release, host, median of 25: `compiler/emit.nz` 51.3 ms under N37 and 56.6 ms
  under N38 (+10 %); `compiler/check.nz` 31.7 and 34.7 ms. Of the added time on `emit.nz`, reducing
  bodies to facts is about 5 ms and the solve about 1.5 ms. Two optimisations came before the gate: a
  join that reports whether it grew, rather than a clone compared after, and re-walking a body only
  when a binding grew after it was read — 261 of `emit.nz`'s 432 functions had needed a second,
  confirming walk. In the container the compiler benchmark's `compiler/check` is 98.2 ms, against
  N37's 80.6 (+22 %).
- **Memory.** Peak RSS of `nazm check compiler/emit.nz`, host, median of 7: 29.0 MB under N37, 29.6 MB
  under N38.
- **Size.** Linux release, contained: `nazm` 4,592,544 → 4,723,616 B (+131,072, +2.85 %); `nazm-mcp`
  4,134,624 → 4,265,696 B (+131,072). Host (macOS) `nazm`: 4,782,752 B.
- **The large graph.** A 2,000-function chain passing the command line through, and a 500-function
  cycle one of whose members reads a file, check in one debug test well under its 60 s bound — the
  whole provenance suite is 1.3 s in debug. Before the worklist the same test took 29 s: rounds that
  moved a summary one call each were quadratic along a chain.
- **Precision.** A function returning a constant is handed a file's contents and its result is local;
  a function returning its second parameter carries only that one; a choice made on `file_exists`
  is local (`crates/nazm-core/tests/provenance.rs`).
- **The planner and servers**, contained: `repo --map` 407–419 ms (N37 352–363), the four task
  contexts 59–634 ms (49–567), 1,000 MCP task contexts 113.6 ms each (104.2), the docs index 28.4 ms
  (25.6) — about 10–15 % slower, since every analysis they request now includes the solve.
- **The MCP task-context memory check, and why its law changed.** In the gate it failed: RSS read
  53,240 kB after warm-up and 93,576 kB after 1,000 calls, over its +4 MB allowance. Diagnosis:
  sampled every 250 calls over 3,000, RSS was flat at 53,252 kB for 750 calls, stepped once to
  93,416 kB, and stayed flat to the end; with `MALLOC_ARENA_MAX=1` — a diagnostic only, never the
  benchmark's environment — the same run was flat at 61,308–61,320 kB throughout. The step is glibc
  giving a worker thread its own arena the first time threads contend, at a moment set by
  scheduling rather than by how many requests came before: a sequential warm-up of 1,000 requests,
  flat throughout, was followed by the step inside the measured window. Concurrent warm-up
  absorbed it, but reached 174–321 MB of retained memory a leak could hide in, and was not used.
  **The law now** (`crates/nazm-mcp/tests/protocol.rs`, `growth`): after a sequential warm-up,
  bounded at 3,000 requests and ended once five consecutive samples 250 requests apart lie within
  4 MB, the 1,000-request window is sampled every 250 requests. It passes if every sample is within
  +4 MB of the baseline, or if there is exactly one adjacent increase over +4 MB with every sample
  before it within +4 MB of the baseline and at least one after it, all within +4 MB of the first
  post-step sample. Two steps, growth past the bound before or after one, and a step at the last
  sample, with no plateau seen, fail — synthetic sequences in its unit test cover each.
  Confirmation, 3,000 sequential requests, normal allocator: 72,904 kB at every one of 13 samples
  (the step came before the first), peak 74,872 kB. The release-benchmark stage, run again alone:
  warm-up 1,000 requests at 53,128 kB, measured window 53,128 kB at all five samples, `Flat`, peak
  72,760 kB — **passed**.

**The gate**, offline, at `2861d31`: mutation 5,848 s (34 of 34 caught: 32 at tier 1 with killers
verified, one killerless entry at tier 2 and one at tier 3, over two sessions and three resumes that
found nothing left), workspace 416 s (1,869 passed, 0 failed, 23 ignored, 117 suites; the container's
`memory.peak` again read its 4 GiB limit with no OOM kill), lifecycle 185 s (19 of 19), selfhost 57 s
(38 of 38), bootstrap 37 s (C2 = C3, IR identical, executables identical once the linker UUID is
removed), release benchmarks 460 s (failed at the MCP memory check above; after its law changed, run
again alone at `e6e9454` and passed in 327 s), compiler benchmark 197
s (the same 21 pre-N33 drifts), checks 17 s (18 of 18) — 7,217 s of wall time, one run, no network
(`NETWORK-ABSENT`). The compiler benchmark's harness counted one `nazm-` container at its end; none
remained afterwards, and which it was is not established.

### Capabilities — N37, measured 2026-09-30

Authority is checked statically and a capability erases (`architecture.md` §7.39), so what is
measured is what the check costs the compiler and what a capability leaves in the program.

- **Next to nothing in the program.** A function taking an `IoCap` lowers to exactly the IR of the
  same function taking an `Int` (`crates/nazm-cli/tests/capabilities.rs`); `main` receives one
  `i64 0` per root from the entry wrapper. `examples/capabilities/workers.nz` and its twin with
  `Int` in place of every capability build to executables of the same size at `-O2` (52,104 B,
  host); the `-O2` IR differs by 254 B, `main`'s two parameters and the entry's call.
- **Compatibility.** The 200 `.nz` sources at N36's HEAD (`git archive 9deed77`) were checked with
  the N36 and N37 release builds (host, `--no-cache`): 197 give byte-identical output and exit
  status; the three that differ are `examples/effects/pure.nz`, `report.nz` and `tasks.nz`, which
  declared effects and exercised them with no capability, and were migrated. The N36 build used is
  the one built from its sources before its last commits, none of which touched `crates/*/src`.
- **Census** over the tree's roots: 1,054 function checks, 20 declaring a set, 11 holding a
  capability; the inferred sets are N36's plus the new examples (870 `{}`, 27 `{ io }`, 3
  `{ spawn }`, 154 `{ io, spawn }`).
- **Checking time.** `nazm check --no-cache`, release, host, median of 25: `compiler/emit.nz`
  52.4–52.9 ms under N36 and 52.9–53.0 ms under N37; `compiler/check.nz` 33.0–33.5 and 33.0–33.4
  ms — the same within noise, since no function there declares a set and the check runs only for
  those that do. A synthetic 3,001-function chain in which every function declares `! { io }` and
  threads an `IoCap` checks in 73.3–73.8 ms, against 66.6–67.8 ms for its twin with no sets and an
  `Int` in place of the capability: about 6.5 ms for 3,001 contracts checked for effects and
  authority. The compiler benchmark's `compiler/check` is 80.6 ms (N36: 83.0).
- **Memory.** Peak RSS of `nazm check compiler/emit.nz`, host: 27.9 MB under N36, 27.8 MB under N37.
  One capability set per call site is recorded.
- **Size.** Linux release, contained: `nazm` 4,592,544 B and `nazm-mcp` 4,134,624 B, both the same as
  N36 at the 64 KiB granularity these sizes move in. Host (macOS) `nazm`: 4,631,328 → 4,647,984 B.
- **The planner and servers**, contained: `repo --map` 352–363 ms, the four task contexts 49–567 ms,
  1,000 MCP task contexts 104.2 ms each, RSS settled 72,268 kB; the docs index 25.6 ms (frame
  237,983 B). Within N36's range.

**The gate**, offline, at `0c94d74`: mutation 4,395 s (33 of 33 caught, 32 at tier 1 with killers
verified and 1 at tier 2, over two sessions and three resumes that found nothing left), workspace
392 s (1,846 passed, 0 failed, 23 ignored, 115 suites), lifecycle 185 s (19 of 19), selfhost 53 s
(38 of 38), bootstrap 37 s (C2 = C3, IR identical, executables identical once the linker UUID is
removed), release benchmarks 463 s, compiler benchmark 195 s (the same 21 pre-N33 drifts), checks
17 s (18 of 18) — 5,737 s of wall time, one run. The container had no network (`NETWORK-ABSENT`).
The workspace container's `memory.peak` read 4,294,967,296 B — its limit — with no OOM kill; that
counter includes page cache, and N36's read 1.96 GB. Why it was higher this time was not
established. After the gate, the tier-2 mutant's declared killer was strengthened and the mutant
verified alone at tier 1 (164 s, one test file changed).

### Typed effects — N36, measured 2026-09-30

Effects are a checker fact (`architecture.md` §7.38), so what is measured is what they cost the
compiler and what they leave in the program.

- **Nothing in the program.** A program with every function's effects declared and the same
  program without them emit byte-identical IR at `-O0`, one module and two
  (`crates/nazm-cli/tests/effects.rs`), with each body at the same line and column in both — a
  runtime error message carries its position, and an annotation on a body's own line moves it.
  The interpreter never reads a set.
- **Compatibility.** Every one of the 116 `.nz` sources the tree had before N36 — the compiler,
  the conformance corpus, `examples/` including `examples/bad/`, `corpus/`, `bench/programs/` —
  gives byte-identical `nazm check` output and exit status under the N35 and the N36 release
  builds (host, `--no-cache`). None needed a change; none declares a set.
- **What the checker infers** over the tree's 34 compilation roots (1,044 function checks, 11
  declared, all in `examples/effects/`): 867 `{}`, 22 `{ io }`, 3 `{ spawn }`, 152 `{ io, spawn }`
  — the last almost all in the multi-module compiler sources, whose calls to undeclared imports
  must assume every effect (`crates/nazm-service/tests/effects.rs`, the ignored census).
- **Checking time.** `nazm check --no-cache`, release, host, median of eight after a warm-up:
  `compiler/emit.nz` 52.9 ms under N35 and 51.9 ms under N36; `compiler/check.nz` 33.9 and 31.5 ms
  — the same within noise. In the container, `analyse` of `compiler/emit.nz` (432 functions)
  takes 40.6 ms in all, effects included. The compiler benchmark's `compiler/check` is 83.0 ms
  against N35's 81.1, inside the drift every measurement shows (below). The synthetic graph — a
  2,000-long chain, a 300-wide fan-out and a 500-long cycle in one module — settles in one test
  in debug, well under its 60 s bound.
- **Memory.** Peak RSS of `nazm check compiler/emit.nz`, host: 27.4 MB under N35, 28.7 MB under
  N36 (+4.5 %): one reason per effect per function, and the declared and inferred sets, are kept;
  no provenance graph is.
- **Size.** `nazm` 4,527,008 B at N35, 4,592,544 B at N36 (+65,536, +1.45 %); `nazm-mcp`
  4,134,624 B, unchanged. Linux release, contained.
- **The planner and servers** over this repository, contained, against N33's run of the same
  harness: `repo --map` 360–367 ms (350–356), the four task contexts 50–594 ms (46–579), 1,000 MCP
  task contexts 107.0 ms each (100.5), RSS settled 73,256 kB (73,224); the docs index 24.7 ms
  (23.1). A few per cent slower over a corpus that has grown since (the docs index frame is
  234,410 B, was 228,934), and nothing on those paths reads an effect beyond the one set a packet
  carries.

**The gate**, offline, at `973ae9f`: mutation 5,862 s (85 of 85 caught: 78 at tier 1 with killers
verified, 7 at tier 2), workspace 360 s (1,825 passed, 0 failed, 23 ignored, 111 suites, peak
1.96 GB), lifecycle 185 s (19 of 19), selfhost 47 s (38 of 38), bootstrap 38 s (C2 = C3, IR
identical, executables identical once the linker UUID is removed), release benchmarks 537 s (run
again on its own: the gate script's form of the stage used a `bash`-only `time ( … )` that the
container's `sh` refused before anything ran), compiler benchmark 200 s (the same 21 pre-N33 drifts
as N34's and N35's, each within about 2 % of them), checks 12 s (18 of 18) — 7,242 s of wall time.
Two earlier runs of the gate were not evidence: the first stopped at its disk guard before the
lifecycle with four failures of new tests in the workspace, the second with two; each was fixed and
the gate run again whole. The container had no network (`NETWORK-ABSENT`).

### Agent task tokens, cost and correctness — N35, measured 2026-09-29

`nazm-agent-bench` (N35, `architecture.md` §7.37). **One model configuration, recorded here and
nowhere generalised:** Anthropic's `claude-haiku-4-5-20251001`, reached through the Claude Code
client in print mode (`claude --print`, client 2.1.283) on this session's own sign-in — one turn,
no tools, no MCP server, no setting source, no thinking (`MAX_THINKING_TOKENS=0`), at most 1,024
output tokens, the benchmark's system text in place of the client's. **The client sets no
temperature, so the model's default applies**; this is the benchmark's largest limitation. The
direct OpenAI and Anthropic adapters exist, are tested against recorded responses, and were not
run: no API key was supplied, and a developer who wants them brings their own
(`runbook.md`, *The agent task benchmark*).

Frozen before the run, at `c130f72`: suite `b7db4ccc…` (`nazm.agent-bench-suite/1`, eight tasks,
116 required facts over two trials), prompt `nazm.agent-bench-prompt/1` (template `de954887…`),
benchmark `ce659979f4f695b8`, two trials per task and arm, the arms alternating in order by
trial, every request independent. The price list is `tools/agent-bench/pricing.toml`, read
2026-09-29 from the provider's pricing page: $1 per million input tokens, $0.10 cache reads, $2
one-hour cache writes, $5 output. The client's own list-price cost of every one of the 34 paid
requests (the pilot's and the run's) equals the benchmark's to the micro-dollar.

**Per task** (medians over two trials; *solved* is every fact correct and no false claim;
*lenient* reads an answer that was prose around a JSON block by that block, and is never the
verdict; cost is the sum of both trials):

| Task | Solved, base / N33 | Lenient | Facts, base / N33 | False claims, base / N33 | Input tokens, base → N33 | Output, base / N33 | Cost billed, base → N33 | Cost uncached, base → N33 | Verdict |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| T1 understand `check_program` | 1/2 / 2/2 | 1/2 / 2/2 | 35/36 / 36/36 | 1 / 0 | 111,398 → 7,791 | 329 / 328 | $0.4489 → $0.0344 | $0.2261 → $0.0189 | N33 better |
| T2 `attach_core`'s dependencies and callers | 0/2 / 0/2 | 0/2 / 0/2 | 0/8 / 8/8 | 0 / 32 | 111,471 → 3,360 | 676 / 293 | $0.4526 → $0.0097 | $0.2297 → $0.0097 | baseline better |
| T3 edit plan for `attach_core` | 0/2 / 2/2 | 0/2 / 2/2 | 10/12 / 12/12 | 3 / 0 | 111,568 → 3,454 | 158 / 152 | $0.4478 → $0.0084 | $0.2247 → $0.0084 | N33 better |
| T4 diagnose N0300 (stress case) | 2/2 / 2/2 | 2/2 / 2/2 | 6/6 / 6/6 | 0 / 0 | 744 → 1,367 | 72 / 72 | $0.0022 → $0.0035 | $0.0022 → $0.0035 | baseline better |
| T5 specification: `return` | 1/2 / 2/2 | 1/2 / 2/2 | 11/12 / 12/12 | 0 / 0 | 37,914 → 1,916 | 122 / 102 | $0.1529 → $0.0049 | $0.0771 → $0.0049 | N33 better |
| T6 grammar: `record` | 2/2 / 2/2 | 2/2 / 2/2 | 12/12 / 12/12 | 0 / 0 | 18,987 → 1,699 | 106 / 100 | $0.0770 → $0.0044 | $0.0390 → $0.0044 | N33 better |
| T7 tests of `nazm-docs` | 0/2 / 0/2 | 0/2 / 0/2 | 21/26 / 22/26 | 0 / 0 | 16,090 → 2,486 | 823 / 324 | $0.0529 → $0.0082 | $0.0404 → $0.0082 | N33 better |
| T8 tests of `core_module` | 0/2 / 2/2 | 2/2 / 2/2 | 2/2 / 2/2 | 0 / 0 | 6,720 → 1,310 | 752 / 90 | $0.0344 → $0.0035 | $0.0210 → $0.0035 | N33 better |

**By category and in all** (input and cost over both trials):

| Group | Correctness preserved | Solved, base / N33 | Facts, base / N33 | Input reduction | Total-token reduction | Cost reduction, billed | Cost reduction, uncached | Context-only reduction (o200k) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| understand (T1, T2) | 1/2 | 1/4 / 2/4 | 35/44 / 44/44 | 95.00 % | 94.74 % | 95.11 % | 93.74 % | 95.40 % |
| edit (T3) | 1/1 | 0/2 / 2/2 | 10/12 / 12/12 | 96.90 % | 96.77 % | 98.12 % | 96.25 % | 97.46 % |
| diagnose (T4) | 1/1 | 2/2 / 2/2 | 6/6 / 6/6 | −83.68 % | −76.30 % | −56.41 % | −56.41 % | −960.38 % |
| docs (T5, T6) | 2/2 | 3/4 / 4/4 | 23/24 / 24/24 | 93.65 % | 93.32 % | 95.98 % | 92.03 % | 96.18 % |
| tests (T7, T8) | 2/2 | 0/4 / 2/4 | 21/30 / 26/30 | 83.35 % | 82.73 % | 86.56 % | 80.88 % | 83.20 % |
| **all eight** | **7/8** | **6/16 / 12/16** | **95/116 / 112/116** | **94.36 %** | **94.05 %** | **95.39 %** | **92.86 %** | **95.43 %** |

- **Correctness first.** The N33 context solved 12 of 16 requests and the baseline 6; N33 got
  112 of 116 facts, the baseline 95. Neither arm hallucinated a repository fact. The baseline's
  four false claims were misreadings (`parse.nz::build_children`, `emit.nz::emit_program`, and a
  package given as a test target); four of its answers broke the JSON-only contract by reasoning
  in prose first — two of them (T8) correct when read leniently.
- **The one correctness regression is T2, by the pre-declared rule.** The N33 arm named all
  eight facts in both trials, and also listed sixteen of the language's builtins
  (`ints_push`, `str_concat`, …) as `module.nz` definitions that `attach_core` depends on: 32
  false claims, each a name the context does show being called. The baseline produced no valid
  answer in either trial. The rule (solved, then facts minus false claims) scores that as
  baseline better; it is reported as the rule says, not special-cased.
- **Tokens and cost.** Across all sixteen pairs the N33 arm used 46,772 input tokens to the
  baseline's 829,789 (−94.36 %), 49,694 total to 835,867 (−94.05 %), and cost $0.0770 to $1.6687
  as billed (−95.39 %) or $0.0614 to $0.8602 with every input token at the uncached rate
  (−92.86 %). The client wrote a one-hour cache for every prompt of 4,096 tokens or more and
  read one back once in 32 requests, so its bill charges large prompts about twice the uncached
  rate; the uncached figure is the comparison no caching choice can tilt. Output was 2,922 tokens
  to 6,078: the smaller context did not buy its saving with longer answers.
- **Efficiency.** Tokens per solved task: 139,311 baseline, 4,141 N33. Cost per solved task:
  $0.2781 and $0.0064. Correct facts per thousand input tokens: 0.11 and 2.39; per dollar billed,
  56.93 and 1,455.32.
- **Scenario C and the crossover.** On the 123-byte program both arms solved both trials; the
  structured context cost 1,367 input tokens to the raw source's 744 (+84 %, $0.0035 to $0.0022).
  The client's own framing is about 430 tokens of every request, and the benchmark's fixed prompt
  233–346 more (o200k); both are the same in both arms. The N33 context was the cheaper input
  on every other task, the smallest of which had a 6,720-token baseline: for this model and suite
  the crossover lies between 744 and 6,720 input tokens. That is an observation, not a routing
  rule; nothing routes on it.
- **Local counts.** Haiku's input was 1.08–1.26× the local `o200k_base` count of the same text
  once the client's ~430 framing tokens are set aside (1.01× on the 53-token program): no N34
  tokenizer is Anthropic's, and none is claimed to be.
- **Latency** (a reply's own; no request was retried): baseline median 5,879 ms, 3,240–14,038;
  N33 median 3,857 ms, 3,153–5,553. One baseline answer (T7, trial 2) reached the output limit
  and the client continued it in a second call, which the record keeps.

Spend: the authoritative run $1.745696, the two pilots $0.016399 and $0.021533 — $1.783628 of the
$2 cap, which every attempt was checked against before it was sent. The pilots are not evidence:
the first found that the worst case priced input as uncached though a cache write costs more, and
that the client's own spend limit, set from that worst case, withheld one answer, which had been
scored as the model's; both were fixed and the pilot re-run before the suite was frozen. The
structured report is `tools/agent-bench/results/n35-v1-claude-code-haiku-4-5.json`; the journal
and raw replies stay in the local, ignored evidence directory.

**The gate**, offline after the paid run, at `0419b25`, contained on the M1 Pro: mutation 1,508 s
(16 of 16 caught, 16 killers verified), workspace 393 s (1,795 passed, 0 failed, 22 ignored, 108
suites, peak 3.6 GB), lifecycle 185 s (19 of 19), selfhost 57 s (38 of 38), bootstrap 38 s (C2 = C3,
IR identical), release benchmarks 596 s, compiler benchmark 198 s (the same 21 pre-N33 drifts as
N34's, each within 1 % of it), checks 2 s (18 of 18) — 2,977 s of wall time. The container had no
network (`NETWORK-ABSENT`); the agent benchmark's 21 tests passed there on the fake provider and
recorded responses. `nazm` is 4,527,008 bytes and `nazm-mcp` 4,134,624, both the same as at N34;
`nazm-agent-bench` is 11,343,120 and ships in neither.

**What this does not show.** One model, one client, two trials, the model's default
temperature; the ranges above are what two samples show, not confidence intervals, and no
significance is claimed. Eight tasks over one repository. At the recorded prices.

### Context under four tokenizers — N34, measured 2026-09-29

`nazm-tokens` (N34, `architecture.md` §7.36), contained at P1 on the M1 Pro with **no network in
the container**, release builds, at `ab499fa`. Four pinned tokenizers of three families:
`openai-bpe/cl100k_base` and `openai-bpe/o200k_base` (one family), `sentencepiece-bpe/mistral-7b-v0.1`
and `sentencepiece-unigram/t5-small`. Content tokens only — no template framing, no chat protocol.
Every count is the library's own encoding of the exact bytes; the golden fixtures hold every
tokenizer to an independent implementation's ids.

**The seven N33 tasks, baselines unchanged** (one definition, `crates/nazm-repo/tests/scenarios/`,
used by both benchmarks), naive → N33 tokens and reduction; every N33 mechanical check passes in the
same run:

| Task | Naive → N33 bytes | cl100k_base | o200k_base | Mistral 7B v0.1 | T5-small | Spread |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A understand `check_program` | 356,043 → 20,059 (94.37 %) | 89,446 → 6,031 (93.26 %) | 89,260 → 5,936 (93.35 %) | 112,475 → 7,248 (93.56 %) | 121,458 → 9,872 (91.87 %) | 1.69 pts |
| B edit `attach_core` | 348,233 → 7,844 (97.75 %) | 88,399 → 2,232 (97.48 %) | 88,337 → 2,245 (97.46 %) | 111,444 → 2,836 (97.46 %) | 124,660 → 3,660 (97.06 %) | 0.42 pts |
| D understand `spec:returns` | 142,128 → 3,769 (97.35 %) | 34,334 → 1,054 (96.93 %) | 34,338 → 1,060 (96.91 %) | 38,782 → 1,256 (96.76 %) | 41,388 → 1,523 (96.32 %) | 0.61 pts |
| D understand guide `record` | 53,386 → 2,768 (94.82 %) | 15,812 → 846 (94.65 %) | 15,857 → 854 (94.61 %) | 18,488 → 1,060 (94.27 %) | 22,383 → 1,329 (94.06 %) | 0.59 pts |
| E tests of `nazm-docs` | 26,649 → 5,577 (79.07 %) | 7,327 → 1,541 (78.97 %) | 7,306 → 1,577 (78.42 %) | 9,605 → 2,081 (78.33 %) | 10,530 → 2,873 (72.72 %) | 6.25 pts |
| E tests of `core_module` | 18,223 → 1,911 (89.51 %) | 4,824 → 535 (88.91 %) | 4,788 → 545 (88.62 %) | 6,023 → 721 (88.03 %) | 6,449 → 983 (84.76 %) | 4.15 pts |
| **C diagnose `N0300` — the fixed-overhead stress case** | 123 → 1,925 (−1,465 %) | 40 → 563 (−1,308 %) | 40 → 565 (−1,313 %) | 50 → 782 (−1,464 %) | 51 → 1,007 (−1,875 %) | 567 pts |

Excluding only the stress case: median reduction **93.95 %** (cl100k), **93.98 %** (o200k),
**93.91 %** (Mistral), **92.96 %** (T5); every tokenizer's worst task is the `nazm-docs` tests
(72.72–78.97 %) and its best the `attach_core` edit (97.06–97.48 %); **median spread 1.15 points,
largest 6.25**. The benefit is not one tokenizer's: the smallest reduction under any tokenizer is
72.72 %, and the ranking of tasks is the same under all four. T5 is consistently the lowest, because
its vocabulary has no piece for `{`, `}` and several other characters JSON is made of — its
repository-map count carries 2,188 unknown pieces — which is a property of the tokenizer, not of the
context. A task context's exact token count also moves with its `state` digest, which is in the
text: two builds of the same tree at different commits differ by a few tokens.

**Where the stress case's tokens go** (cl100k / o200k / Mistral / T5): the whole context 563 / 565
/ 782 / 1,007; without the three retrieval references 437 / 442 / 599 / 788; without the envelope
(schema, state, budget, used, empty lists) 437 / 441 / 600 / 794; without reason, class, authority,
via and compilation 478 / 484 / 663 / 845; the content strings alone (source, message, help) 53 /
53 / 58 / 65; the naive program 40 / 40 / 50 / 51. The overhead is structural and fixed, about 400–700
tokens, spread across retrieval, envelope and explanation roughly equally — and every tokenizer
agrees on that ordering. No compact transport was adopted: each part is what the planner's law
requires (a way to retrieve the item, the state it is of, and why it is there), and it is constant,
not proportional to the task.

**Source, JSON, Markdown and scripts** (tokens, bytes per token): `compiler/lex.nz` (11,558 B) 3,251
(3.56) / 3,231 (3.58) / 3,957 (2.92) / 4,564 (2.53, 355 unknown); the repository map (112,735 B)
29,241 (3.86) / 29,067 (3.88) / 36,608 (3.08) / 59,933 (1.88, 2,188 unknown); `spec:generics` 270 /
270 / 330 / 345; `architecture:7.35` 2,264 / 2,267 / 2,608 / 2,836. Bangla prose (290 B): **133 /
37 / 123 / 35** (T5 with 17 unknown); Arabic (149 B): 63 / 30 / 76 / 27 (13 unknown); mixed English,
Bangla and code (164 B): 64 / 34 / 57 / 42. The families differ most on scripts — `cl100k_base`
spends 3.6× the tokens of `o200k_base` on the same Bangla — and least on JSON and code, where the
two OpenAI vocabularies agree within 1 %.

**One byte budget, one selection** (`check_program`, understand, `max_entities` 3, 8, 20): 3, 8 and
20 items — the planner's, the same whichever tokenizer then counts them — at 10,591, 13,266 and
20,057 bytes; in tokens 3,114–4,995, 3,950–6,454 and 5,936–9,870, **1.63–1.66× between the
cheapest and the dearest tokenizer**. A token budget would select differently for each tokenizer;
the byte and entity budgets select once.

**What measuring costs.** Loading the four tokenizers: 256 ms (the OpenAI vocabularies are digested
rank by rank at load). The stress context (1,925 bytes): 0.37–0.80 ms per tokenizer. Task A's naive
baseline (356,043 bytes): 17.7 ms (o200k), 26.9 ms (cl100k), 100.2 ms (Mistral), 117.4 ms (T5).
**1,000 measurements of an N33 context under all four: 4.24 ms each on average, resident memory
209,392 kB settled and 209,392 kB after** — flat across those calls, in a process that had already
loaded the tokenizers for earlier tests. `nazm-tokens` is 9,180,488 bytes. **`nazm` is 4,527,008
bytes, exactly as at N33**; neither it nor `nazm-mcp` (4,134,624) has a tokenizer in its graph.

**The MILESTONE gate** (`gate-plan`: full lifecycle — `xtask/`, the root manifest and
`tools/` changed — the eleven N34 mutations and every benchmark family; no MCP entries, the server
unchanged), every workload with no network, every cache empty — the lockfile changed, and the
N33-key caches were cleared first for disk: mutation 835 s, workspace 318 s (1,773 passed across
104 suites; the first run, 344 s, failed one test — the interface table did not list
`nazm.token-cost/1` — and was re-run after the row was added), lifecycle 185 s, selfhost 58 s,
bootstrap 39 s, release benchmarks 459 s, compiler benchmark 193 s, checks 3 s: **2,090 s of stages,
2,436 s (40.6 minutes) of wall time** including the failed run. The compiler benchmark shows the
same ~2× drift against the 2026-09-21 baseline as at N33, which N33's back-to-back comparison placed
before N33; `nazm` is unchanged byte for byte. The image was rebuilt once before the gate, with the
network, to carry the new crates; that was bootstrap, not evidence.

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

### Incremental reparse — N76, measured 2026-10-03

**Hypothesis:** an edit inside one item reparses that item, not the file. **Changed path:**
`nazm_syntax::Revision::edit` against `Revision::new` and `parse_both` of the same text.
**Workload:** the three largest compiler sources, one `x` inserted after the first `let ` past the
middle of the file. **Method:** `cargo test --release -p nazm-syntax --test incremental -- --ignored
--nocapture measure_full_parse_against_one_edit`, 5 warm-ups discarded, the median of 31; every
result is checked for being a full or an incremental parse as labelled, and its correctness is
the convergence suite's, not this run's. Apple M1 Pro, macOS 27.2, rustc 1.98.1, release profile,
one thread, same host and process for all three columns.

| File | Bytes | `parse_both` | `Revision::new` | One edit | Relexed | Reparsed tokens | Items kept |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `compiler/emit.nz` | 286,952 | 8.23 ms | 8.11 ms | 1.22 ms | 2 of 58,947 | 618 of 40,284 | 125 of 126 |
| `compiler/analyse.nz` | 210,347 | 6.66 ms | 6.55 ms | 1.01 ms | 2 of 50,327 | 518 of 32,060 | 158 of 159 |
| `compiler/parse.nz` | 77,480 | 2.50 ms | 2.45 ms | 0.38 ms | 2 of 19,030 | 290 of 12,152 | 92 of 93 |

About 6.6–6.8× faster than a full parse. The rest is not parsing: 1.5% of the tokens are reparsed,
and an edit still copies the scan and moves its suffix, which is linear in the file's lexemes. That
copy is the next cost, and it is not attacked here. No variance beyond the median is recorded, and
peak memory was not measured. A revision holds the scan, the chunk table and the tree, which is more
than a parse that drops its scan.

### Performance evidence v2 — N92, measured 2026-10-04

**Method:** `cargo xtask contained bench --save` at N92's tree: one CPU, 4 GiB, offline, the host
otherwise idle, five runs after a discarded warm-up; `nazm.bench/2` in `bench/record.json` and
`bench/baseline-linux-aarch64.json`, which it replaces — the file held a baseline older than N75's,
from another configuration, and `--check` against it reported twenty-one "regressions" that were the
configuration's. Medians in ms, with N75's from the section below.

| program | check | build `-O0` | build `-O2` | interpret | native `-O2` | executable |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| arith | 2.8 | 90.5 (95.3) | 98.9 (99.7) | 2,718.7 (2,765.2) | 4.6 (4.5) | 71,984 B |
| calls | 3.0 | 91.3 (93.7) | 99.1 (100.1) | 1,492.2 (1,534.7) | 1.0 (1.0) | 71,984 B |
| channels | 3.1 | 116.3 (103.0) | 197.8 (150.5) | 1,270.2 (1,335.9) | 1,239.4 (1,331.5) | 77,040 B |
| floor | 2.9 | 89.3 (90.4) | 94.3 (99.2) | 2.3 (2.5) | 0.3 (0.3) | 71,936 B |
| sequences | 3.0 | 91.2 (93.5) | 115.2 (115.6) | 765.0 (771.1) | 2.3 (2.7) | 72,896 B |
| sieve | 3.1 | 90.8 (95.1) | 119.0 (117.5) | 1,009.9 (1,013.2) | 3.4 (2.9) | 72,888 B |
| strings | 3.1 | 93.3 (94.3) | 124.6 (129.0) | 49.5 (48.9) | 7.1 (7.4) | 73,376 B |
| compiler | **165.6 (107.4)** | 2,030.2 (1,939.6) | | | | |

The noise floor (the empty program, natively) is 0.3 ms; the median absolute deviation is under 1.5
ms everywhere but the two `channels` runs (51 and 32 ms), whose spread N75 also recorded. The sieve's
references: C (`-O2`, unchecked) 2.5 ms, **Rust (`-O`, overflow- and bounds-checked) 2.6 ms**, Nazm
3.4 ms; Python and Go are not in the image, and the record says so.

**Two regressions since N75, stated and not hidden.** `nazm check compiler/emit.nz` is 54 % slower
(107.4 → 165.6 ms), and `channels` builds 13 % (`-O0`) and 31 % (`-O2`) slower. Located for the
first by release builds of each milestone's records commit on the host (Apple M1 Pro, eleven runs,
median, the same `compiler/` for all): N75 54.0 ms, N76 63.9, N77 67.3, N78 68.8, N79 67.5, N80
75.2, N92 76.0 — the steps are N76 (the lossless tree beneath every parse) and N80 (provenance v3),
each of which `performance.md` measured on its own files at the time; nothing since moved it. Which
work inside those milestones costs it is not measured. The `channels` build regression is not
located. Interpretation and native code are unchanged within noise.

### Debugging and sampling — N91, measured 2026-10-04

**Hypothesis:** sampling observes a run without changing what it measures much, and Cranelift's
debug information costs a debug build little. **Method:** Apple M1 Pro, macOS 27.2, debug `nazm`;
seven runs each, median (min–max). **Sampling:** one LLVM `--debug` executable of a 120,000,000-step
checked loop (`work` in `crates/nazm-cli/tests/debugger_v3.rs`'s `SPIN`, at that count), run alone
and under `/usr/bin/sample` at two intervals, wall time from launch to exit.

| | wall, ms |
| --- | ---: |
| alone | 775 (774–802) |
| sampled every 1 ms | 821 (811–822) |
| sampled every 10 ms | 783 (782–784) |

About 6% at the default interval, 1% at 10 ms: the sampler suspends the process for each sample, so
the cost is per sample, not per instruction. **A Cranelift debug build:** `bench/programs/strings.nz`,
`--no-cache`, five builds each — 198 ms ordinary, 222 ms with `--debug` (`dsymutil` included). Not
measured: the cost on a large program, and memory.

**A launch is not the program's time.** A fresh executable spends about 300 ms before its first
instruction on this host, and a sample taken then names `_dyld_start` in `dyld`: `nazm profile`
builds a new executable every time, so its `wall_ms` includes that launch, and its samples show it
as system time — never as the program's.

### Accelerators v2 — N88, measured 2026-10-04

**Hypothesis:** a zip and a fold are expressible and agree, and what they cost is stated whole —
transfers and the host fold included. **Workload:** `zipped(x, y) = x * y + x` over 1,000,000 pairs
(`x` = 1…1,000,000, `y` = `x % 7`) folded with `add`; the same as a native loop computing `y`
itself. **Method:** debug `nazm`, OpenCL on the Apple M1 Pro's GPU, three runs (the first builds the
kernel); the native loop at `--opt-level 2`, best of five including the 32 ms process floor.

| | ms |
| --- | ---: |
| kernel build (first run / cached by the driver) | 129.6 / 1.5 |
| host to device, two inputs (16 MB) | 2.9–3.2 |
| run | 0.77–0.85 |
| device to host (8 MB) | 0.94–0.99 |
| fold on the host, in index order (a debug build of `nazm`) | 13.8–14.2 |
| the whole job as a native loop, less the process floor | about 3 |

The device is not the cost; the transfers and the fold are, and the CPU computes this workload faster
than it can be shipped. This is the honest answer for a light kernel: the row's acceptance — a
workload a CPU cannot serve — is unmet, and N88 does not claim it. The fold's time would shrink in a
release build of `nazm`; a device-side `min`/`max` would remove it, and is not built.

### Contracts — N87, measured 2026-10-04

**Hypothesis:** a contract costs its clauses' evaluation and nothing more. **Workload:** 20,000,000
calls of a two-line function `step(n, k)` with `requires n >= 0 && k > 0` and `ensures result >= n`,
against the same program with the two clauses removed; the result printed and equal. **Method:**
debug `nazm`, `--opt-level 2`, best of five wall-clock runs including process start. Apple M1 Pro,
macOS 27.2.

| 20,000,000 calls | LLVM | Cranelift |
| --- | ---: | ---: |
| with the contract | 116.3 ms | 114.6 ms |
| without | 117.1 ms | 108.0 ms |

Under LLVM the difference is inside the run-to-run noise: the checks are compares and a branch to
the failure path, which `-O2` schedules among the call's own work. Under Cranelift about 6 %, the
compares and branches as written. Not measured: checking time (the clauses are typed with the body),
and a clause that calls a function, which costs that call.

### Two boards under QEMU — N86, measured 2026-10-04

**Hypothesis:** a program means the same on both boards, and each board's measured stack stays within
its stated bound. **Method:** `nazm-qemu:n86` (Debian, clang 19 with RISC-V, `ld.lld`, QEMU 10.0),
no network, 8 GiB memory and swap, 4 CPUs; `nazm` built from the tree inside it; each program built
at `-O0` and `-O2`, linked with `ld.lld -T link.ld`, booted with the board's `emulator` line and a
20 s deadline; the stack painted with `--stack-watermark`. Host: Apple M1 Pro, macOS 27.2.

| Program | AArch64 `virt` | RISC-V `virt` |
| --- | --- | --- |
| byte writes to the UART, result `100` | `HI` / `100`, status 0 | `HI` / `100`, status 0 |
| `Int` overflow in a loop | `N0400`, status 2 | `N0400`, status 2 |
| recursion past the stack | `N0408`, status 2 | `N0408`, status 2 |
| stack used ≤ bound, `-O0` | 176 ≤ 176 bytes | 152 ≤ 160 bytes |
| stack used ≤ bound, `-O2` | 32 ≤ 48 bytes | 32 ≤ 48 bytes |
| `fib(20)`, recursive (no bound stated), used `-O0` / `-O2` | 1,696 / 688 bytes | 1,344 / 688 bytes |

Found on the way, and fixed before these numbers: at `-O2` the RISC-V unit carries an `.eh_frame`
section the linker script did not name, which `ld.lld` placed at the load address ahead of `_start`,
so the machine started in data; the script now discards unwind tables and names RISC-V's small-data
sections. The AArch64 bound at `-O0` is met exactly: the bound is clang's frames along the deepest
path, and the run took that path. No timing is claimed: an emulator's speed says nothing about a
board's.

### FFI v3 — N85, measured 2026-10-04

**Hypothesis:** what crosses costs what §7.86 states, and nothing more. A foreign call now also
captures `errno` (one runtime call and a thread-local store); a C struct crosses as a `malloc`'d copy
freed after the call; a `Str` result is a `strlen` and a copy. **Workloads:** a loop of 10,000,000
iterations whose body is one foreign call: `c_add(t, i)` (scalars), `c_psum(P(x: i, flag: true, y:
1))` (a three-field C struct), `str_len(c_hi(i))` (a five-byte C string copied and dropped).
**Method:** debug `nazm`, `--opt-level 2`, clang `-O2` C fixture, best of five wall-clock runs
including process start; an empty program measures 32.3 ms the same way. Apple M1 Pro, macOS 27.2.

| Loop of 10,000,000 | LLVM | Cranelift |
| --- | ---: | ---: |
| scalar call, `errno` captured | 73.0 ms | 77.7 ms |
| C struct by borrowed copy | 198.7 ms | 250.2 ms |
| `Str` result copied | 328.1 ms | 335.3 ms |

Less the floor, a scalar call with its capture is about 4–5 ns an iteration; the struct copy adds
about 13–17 ns (an allocation and a free), the string copy about 30 ns (an allocation, a `strlen`, a
copy, a release). Not isolated: the capture's own cost against an N84 build of the same loop — N42's
56.6 ms for the scalar loop was measured by another method and is not a comparator. A program that
calls no C is unchanged: it reaches no `errno` part and links no `errno` unit under LLVM.

### The standard library 1.0 — N84, measured 2026-10-04

**Workloads:** `ints_sort` of 100,000 pseudo-random integers; 10,000 `map_set`s of distinct keys in a
scrambled order; `json_parse` then `json_encode` of a 128 KB array of 2,000 small objects, five times.
**Method:** release build, LLVM `-O2`, five runs, median. Apple M1 Pro, macOS 27.2.

| Operation | Median |
| --- | ---: |
| `ints_sort`, 100,000 | 19.6 ms |
| `map_set` × 10,000 | 390.6 ms |
| JSON parse + encode, 128 KB × 5 | 69.8 ms |

The map is a sorted array: an insertion moves every entry after it, so building one of `n` keys is
quadratic; at 10,000 keys that is the cost above. A hash map needs hashing in the library, and is not
in 1.0. Sorting is a merge sort through `vec_sort_by`, its comparison an indirect call.

### The task pool — N83, measured 2026-10-04

**Hypothesis:** tasks on a bounded pool start faster than threads, wait as cheaply, and let far more
of them be alive at once. **Workloads:** `spawns.nz` (start and join one empty task, 50,000 times),
`pingpong.nz` (50,000 round trips between `main` and a task over two `Chan`s), `many.nz` (N tasks
each waiting for one job, all alive before the first is sent). **Method:** release build, `-O2`; five
runs each, median; resident set size from `/usr/bin/time -l`. Apple M1 Pro, macOS 27.2, page size
16 KiB.

| Model | Spawn + join | Round trip |
| --- | ---: | ---: |
| One thread per task | 25.44 µs | 6.74 µs |
| Pool, 1 / 2 / 4 workers | 7.43 / 7.38 / 7.40 µs | 4.98 / 4.99 / 4.97 µs |

| Tasks alive at once | Pool (4 workers), resident | Threads, resident |
| ---: | ---: | ---: |
| 1,000 | 19.9 MB | 20.3 MB |
| 4,000 | — | 75.5 MB |
| 10,000 | 179.8 MB | did not finish 6,000 within 40 s |
| 50,000 | 891.1 MB | — |

About 17.8 KB per pooled task (one 16 KiB page of stack touched, and the record), against about
18.4 KB per thread: the pool does not save memory per task here, it saves OS threads, starts faster,
and keeps going where threads stop. The switch is 22 instructions each way; a round trip with `main`
still crosses threads, so it is not a pure switch cost. Not measured: work stealing (there is none),
Linux.

### The runtime artifact — N82, measured 2026-10-03

**Hypothesis:** a prebuilt runtime saves the runtime's compile on every build that does not already
hold that exact text. **Workload:** the program in `runtime_artifact.rs` that reaches every runtime
service, built with `--no-cache` so every unit is compiled. **Method:** release build, the two
configurations interleaved, 11 runs each, median (minimum–maximum). Apple M1 Pro, macOS 27.2,
Apple clang 21.0.0.

| Backend | Generated runtime | `--runtime` artifact |
| --- | ---: | ---: |
| LLVM | 146 ms (144–151) | 112 ms (111–116) |
| Cranelift | 119 ms (117–122) | 85 ms (84–88) |

About 34 ms a build, the runtime's clang compile at `-O2`; a build whose project store already held the
runtime's object saw none of that cost before. The executable is the same program: its output,
ending and memory report are identical (tested), though its bytes need not be, since the artifact is
the runtime with every service.

### Provenance v3 — N80, measured 2026-10-03

**Hypothesis:** container cells, indirect targets and local shapes cost the checker a bounded,
measurable amount, and nothing at runtime. **Changed path:** `nazm check --no-cache`, whose
provenance walk and solve changed; nothing below the checker did. **Workload:** the two largest
compiler sources and one small example, checked cold. **Method:** release builds of N79's head
(`38d8755`, in a worktree) and of N80, the two run interleaved in one Python loop, 21 to 31 runs
each, the median with its minimum and maximum. Apple M1 Pro, macOS 27.2, rustc 1.98.1.

| File | N79 | N80 | Ratio |
| --- | ---: | ---: | ---: |
| `compiler/emit.nz` | 71.1 ms (69.7–83.5) | 80.0 ms (78.3–83.1) | 1.125 |
| `compiler/analyse.nz` | 41.6 ms (40.4–43.0) | 45.2 ms (43.9–46.9) | 1.085 |
| `examples/pipeline.nz` | 5.5 ms (5.0–7.7) | 5.5 ms (5.1–7.0) | 0.99 |

A regression of about 9 ms on the largest file, accepted and recorded. Instrumented, the solve on
`emit.nz` takes four rounds as cells grow, about 7.7 ms in all. Three changes brought it down from a
first build at 1.19×: an index of call sites per body (a container call's cell was a linear search),
receive edges evaluated once per round, and later rounds starting from the previous one's summaries
and re-solving only functions that read a grown cell. Further work would be incremental values and
receives inside a round. Peak memory was not measured. The runtime is unchanged: provenance is
erased before Core IR, and `flow_v3.rs` runs an affected program interpreted and natively.

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

## The warm mutation session — N32-H, measured 2026-09-28/29

N32's killer verification was running at **246–291 s per killer** (18 finished, median 266 s) — 32
of them would have taken about 2 h 20 min before its campaign had started. One of them,
profiled phase by phase at P1: container start 1.4 s; copy and `xtask` build about 16 s; **the
pristine `cargo build --workspace --tests`, cold, 190.3 s — 68%**; killer listing 5.3 s; the
mutant build 31.8 s; the restored build 31.5 s; the killer itself, three runs, about 2 s. Every
killer paid a fresh container and a cold target.

A session is now one warm worker (`runbook.md`, *The warm-worker law*): one pristine build on the
image's warmed dependency cache — 92 s instead of 190 s — one baseline, and killer verification
inside the campaign's loop, around each mutant's one injection. Still P1: one CPU, one job, one
thread, one mutant at a time.

| N32 MILESTONE gate, 2026-09-28/29, M1 Pro | Seconds |
| --- | ---: |
| warm session: 31 mutations, 34 killers verified, targeted campaign — setup 806 s once (pristine build, baseline suite, listing), 31 mutants 662 s, median 21.0 s, p95 40.1 s | 1,478 |
| contained workspace suite, P4T4 | 439 |
| lifecycle suite, P1 | 171 |
| selfhost, P4T4 | 57 |
| bootstrap, P1 | 62 |
| N32 benchmark and binary sizes, P1 | 196 |
| `cargo xtask check` and the doc-claim tests | 3 |
| **the gate, wall clock** | **2,406 — 40.1 min** |

The five-mutant checkpoint before it: setup 793 s, first mutant 13.1 s, then 27.8, 27.1, 32.6 and
53.2 s. No second worker was built: the gate was under an hour without one. The setup is now
dominated by the baseline workspace suite at one CPU, which the campaign needs to be green before
any mutant is judged; it is the next stage to look at, and it was not weakened to get here.

## Verification harness V3 — N32-H3, measured 2026-09-29

From the gate's own summary (`target/n32h3/v3/summary.json`, harness evidence, not a schema):
one authoritative harness-changing gate — 45 mutations (N32's seventeen, the twelve MCP entries,
all sixteen harness entries), 48 killers, the scheduled workspace suite at P4W2T4, the full
lifecycle suite, selfhost, bootstrap, the docs and MCP benchmark, the gates.

| Stage, seconds | V1 | V2 warm | V3 |
| --- | ---: | ---: | ---: |
| mutation session (V3: setup 93, 45 mutants 738, median 13.4, p95 34.9) | 1,478 | 770 | 840 |
| — of which the mutants' workspace builds | not recorded | 497 | 529 |
| workspace suite | 439 | 418 (P4T4) | **257** (P4W2T4) |
| lifecycle suite, full | 171 | 183 | 189 |
| selfhost | 57 | 46 | 46 |
| bootstrap | 62 | 37 | 37 |
| benchmark | 196 | 199 | 224 (first fill of the bench cache) |
| gates | 3 | 2 | 2 |
| **the gate** | **2,406** | **1,655** | **1,595 — 26.6 min** |

V3 decided 45 mutations where V2 decided 38. **The workspace suite fell 39%** against the P4T4
run measured the same morning (424 s): 83 binaries over two workers of four threads, 250.7 s of
running led by `nazm-service test:context` 80.5 s, `test:incomplete` 76.7 s and `nazm-cli
test:selfhost` 60.5 s — the same 1,728 passed, 19 ignored, 95 result lines, each binary's counts
identical to the plain run's, at a container peak of 1.63 GB against P4T4's 2.07 GB.

**The gate is over the 25-minute closure line, and the largest stage is at a floor.** The mutation
session's builds are `cargo build --workspace --tests` after each injection, the check that
separates UNUSABLE from CAUGHT: a `nazm-docs` mutant compiles 40 dependent units in 24–31 s, an
MCP one 2 in about 5.5 s, a harness one 2 to 4 in about 2 s — the order was already grouped by
package, so bucketing measured no saving. The remaining 86 s is the restored killers' rebuild,
which the restoration law requires. Neither is removed. The benchmark's 224 s included its
cache's first fill.

Two P4W* profiles were not run: `p4w2t2` was stopped before it started, and `p4w4t1`, `p4w3t2` and
`p4w1t4` were not run — dominated by the model (serialising the slow few-test binaries, more
oversubscription, `cargo test`'s own order). The scheduler's correctness is its unit tests,
including a real-concurrency stress test repeated ten times, not repeated suite runs.

## The mutation baseline and the persistent cache — N32-H2, measured 2026-09-29

V1's session paid 806 s of setup before its first mutant. V1 did not record its parts; V2's cold
gate did, for everything V1 also did: the pristine build 125.7 s and the killer listing 58.0 s.
The remaining ~620 s of V1's setup was the workspace suite run as the baseline at one CPU — work
no Tier-1 verdict needs, and which the MILESTONE gate's own P4T4 workspace stage repeats anyway.
V2 runs each unique declared killer once on the pristine tree instead (35 of them, 10.9 s), runs
the whole suite only when a mutant needs a later tier (none did), reads each mutant's
verification from the very run its verdict came from, and keeps build state between sessions in
two harness-owned volumes.

| Stage, seconds | V1 (N32 gate) | V2 cold (cache cleared) | V2 warm |
| --- | ---: | ---: | ---: |
| mutation setup | 806 | 195 | **86** |
| — pristine build / listing / pristine killers / baseline suite | not recorded | 125.7 / 58.0 / 10.9 / — | 62.5 / 12.3 / 10.9 / — |
| mutants decided (median, p95) | 662 (21.0, 40.1) | 684 (13.6, 36.1) | 679 (13.4, 35.3) |
| mutation session, contained | 1,478 | 896 | **770** |
| workspace suite, P4T4 | 439 | 452 | 418 |
| lifecycle suite, full | 171 | 183 | 183 |
| selfhost | 57 | 57 | 46 |
| bootstrap | 62 | 38 | 37 |
| N32 benchmark | 196 | 206 | 199 |
| gates | 3 | 8 | 2 |
| **the gate** | **2,406** | **1,841** | **1,655 — 27.6 min** |

V2 decided 38 mutations and verified 41 killers where V1 decided 31 and verified 34: the seven
harness mutations N32-H2 added are in both V2 columns. The warm gate is **31% faster than V1**
(−751 s) and passes the ≤ 30-minute MILESTONE target; the 25-minute stretch was not reached.
The cold gate, 30.7 min, missed the target by 41 s, which is why the warm one was run; its
lifecycle stage also failed, on a test that had assumed macOS's recovery route (below), so it is
a timing and not an acceptance. What is left is the workspace suite — 418 s, of which about 410 s
is test execution, led by five binaries of 20–76 s each — the mutants' own builds (497 s of the
770), and the full lifecycle suite this harness-changing round needs; a milestone that does not
change the harness runs only the lifecycle suite's invariants (`runbook.md`).

One CPU, one job and one thread throughout the mutation session: no memory kill in any stage, and
the benchmark's MCP server settled at 10.9 MB and held 11.1 MB after 1,000 more calls.

Two findings, both kept as tests. **A mutation copy and the checkout must not share a target**:
the first cold gate's workspace suite ran a `nazm` a mutation session had built, because Cargo
copies a binary out under its bare name and does not copy a fresh unit again — the two now have
separate volumes, and a mutation guards it. **A damaged target is recovered, not trusted**: on
macOS rustc's incremental cache reuses damaged object files whose source is unchanged and the
build fails, so a harness-owned target is cleaned and rebuilt once; on Linux Cargo rebuilds them
itself. Neither route ever produced a verdict.

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
