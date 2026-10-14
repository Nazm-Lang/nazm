# Capability matrix

**Status: this file is the current status authority for the tree it is committed in.**

Two things are kept apart here. **Frozen candidates** are commits a release candidate was tested
at, each with an immutable record in `releases/` that nothing later updates: the latest is
**`996984f7063a5b9590443c81fb027a7de75c11c4`** ([`releases/996984f.md`](releases/996984f.md), the
v0.3.0 candidate, R1), before it `920a2a5` ([`releases/920a2a5.md`](releases/920a2a5.md), the core
closure, N101–N108) and `84d3c80` ([`releases/84d3c80.md`](releases/84d3c80.md), the v1 foundation,
N40–N48), and the first was `4fb15542dfbf9af0bf34c42fe52783637b3daed4`
([`releases/4fb1554.md`](releases/4fb1554.md)). **The claims below are current-tree claims**: they
include behaviour added after any candidate, each milestone having changed the compiler and run its
own evidence, and a row is checked against its own evidence, not against a candidate commit.

Each row carries the evidence for the milestone that changed it — the tests, the contained
runs and the dates — so a claim is checked against its row, not against a single commit. A
row that rests on nothing newer than the candidate says so or cites only what the candidate
already had.

This is one table serving two purposes. The directive asked for a capability matrix and a
gap map; they are the same information at the same granularity, so the five fields of a
gap map are the columns of the matrix and there is one document instead of two.

**Vocabulary:** VERIFIED · PARTIAL · DESIGNED · RESEARCH · BLOCKED · MISSING, defined in
[`master-architecture.md`](master-architecture.md) §1. A VERIFIED row cites a path; a
`cargo xtask check` rule fails if it does not, or if a cited path stops existing.

**What this document does not cover.** Construct-level support — whether `while` or
`spawn` or a given built-in runs interpreted, compiles natively, or is refused — is
answered by the compiler itself:

```sh
cargo run -p nazm-cli --bin nazm -- capabilities --json
```

That output is read from the compiler's own dispatch tables (`crates/nazm-cli/src/capabilities.rs`),
so it cannot drift from the implementation the way a written table can. Ask it, not this
file. The schema is `schema/nazm.capabilities-1.json`.

---

## Summary

**Current suite: 2,570 passed, 0 failed, 69 ignored** across 205 test binaries, run contained at
`c8dc395` (Q1-C1, 2026-10-08: the v1 exit contract frozen — 0–5, `docs/stability.md`; the four new
ignored tests are the macOS-only host-refusal tests; `target/gates/q1c1/contained-tests.log`),
selfhost 43 of 43 with the bootstrap fixpoint. *Before Q1-C1:* 2,560 passed, 0 failed, 65 ignored
across 203 test binaries, run contained at
`fd90b06` (Q1 of the v4 programme, 2026-10-08; toolchain 1.0.0, semantic epoch 34, runtime ABI 15,
check entries `nazm.check/5`, not yet a release candidate; `target/gates/q1/contained-tests.log`),
selfhost 43 of 43 with the bootstrap chain's fixpoint test passing. Over M1: Q1's fixes and their
tests (`docs/security-qualification.md`); the seven more ignored tests are platform-bound ones Q1
stopped counting as passes off their platform (Q1-D-02). The catalogue holds 1,345 entries. Q1's
campaign — the 21 `q1-*` entries, Gate 2's 38 and the 15 entries Gate 2 and Q1 repointed, 74 in all,
every killer verified — caught 73 at tier 1 (campaigns `a7eac5bb685e1822`, `3e9515c5f7f07b9e`,
`1290489029d48800`); `g2-a-view-does-not-keep-its-buffer` was not caught, and a contained probe of
it showed no observable difference in output or memory counts — Gate 2's contained gate owes its
verdict (`target/gates/q1/probe-mutants-2.log`).

*Historical, not current:* 2,538 passed, 0 failed, 58 ignored across 203 test binaries, run contained at
`8190cca` (M1, 2026-10-08), selfhost 43 of 43 with the fixpoint; M1's two new mutants caught at tier 1.

*Historical, not current:* 2,509 passed, 0 failed, 47 ignored across 180 test binaries, run contained and
offline at `6ab6319` (Gate 1-C1 of the v1 programme, 2026-10-07; toolchain 1.0.0, semantic epoch
30, not yet a release candidate), with selfhost 43 of 43 and the bootstrap's fixpoint, C2 = C3 =
`178a799c…`, 34 conformance cases and 30 refusals agreeing. The catalogue holds 1,285 entries:
Gate 1-C1's six and the fifty responsibility-mapped entries beside them (closures, the ownership
graph, `Vec` retain and release, the cycle refusal, the epoch, Gate 1's two) were caught in
campaign `ee2ea936f8cb26ad`, 55 of 56 at tier 1 with every killer verified (pristine passes, mutant
fails, restored passes); the 56th, made equivalent by Gate 1-C1's shared type list, was retargeted
and caught in `9f505c6cde2715d7`. Area 4 is VERIFIED again since Gate 1-C1.

*Historical, not current:* 2,505 passed, 0 failed, 47 ignored at `709960c` (Gate 1), selfhost 43
of 43, C2 = C3 = `18481f2a…`, 29 refusals; the catalogue held 1,279 entries and Gate 1's six were
caught in `b05c9651bed81041`. Area 4 was PARTIAL — a recorded cycle defect, open for decision.

*Historical, not current:* 2,492 passed, 0 failed, 47 ignored, run contained and offline at the v0.3.0
candidate `996984f` inside both release assemblies (2026-10-07), with selfhost 43 of 43, the
bootstrap's fixpoint and the lifecycle tests 19 of 19 (`releases/996984f.md`). The mutation
catalogue holds 1,273 entries: the 1,268 present at N108's gate were all caught there
(`releases/920a2a5.md`), and R1-C1's two and R1's three were caught in their own
responsibility-mapped campaigns. Each release record under `releases/` is the frozen account of its
own candidate, and the latest one supersedes this line.

*Historical, not current:* the N48 release gate (2026-10-01, at `a8a7b3f`) ended at 2,021 passed,
0 failed, 29 ignored, across 136 suites, with the catalogue 808 of 808 caught. The durable record of
that gate, and of N40–N48 as a whole, is [`releases/84d3c80.md`](releases/84d3c80.md): frozen
release-candidate notes for the tested candidate `84d3c80`, not updated by later commits — the
documentation commits after it do not change what it says. It is the canonical N40–N48 evidence
location; the long per-milestone working report used to produce it is kept outside the repository.

*Historical, not current:* N39 ended at 1,903 passed, 0 failed, 25 ignored, across 121 suites, run contained and
offline (2026-09-30), scheduled at P4W2T4; nineteen of the ignored are the xtask lifecycle
tests, run on their own (19 of 19 passed, contained), and six are N33's, N34's, N36's, N38's (the
3,000-request MCP confirmation) and N39's release-build measurements, run by hand or in the
release-benchmark stage (`performance.md`). N38
ended at 1,869 across 117 with 23 ignored; N37 ended at 1,846 across 115 with 23
ignored; N36 ended at 1,825 across 111 with 23
ignored; N35 ended at 1,795 across 108 with 22
ignored; N34 ended at 1,773 across 104 with 22 ignored; N33 ended at 1,757 across 99 with 21 ignored; N32-H3 ended at 1,728 across 95 with 19 ignored; N32-H2 ended at 1,718 across 95; N32 ended at 1,706 across 95; N31 ended at 1,669 across 91, 1,663 before its closure correction, N30 ended at 1,647 across 89, N29 ended at 1,629 across 88, N28 ended at 1,609 across 87, N27 ended at 1,582 across 86 (1,580 before its closure correction), N26 ended at 1,555 across 83, N25 ended at 1,532 across 81, N24 ended at 1,514 across 80, N23 ended at 1,491 across 79, N22 ended at 1,476 across 78, N21 ended at 1,461 across 77, N20 ended at 1,448 across 76, N19 at 1,435 across 75, N18 at 1,417 across 74, N17 at 1,395 across 73, N16 at 1,376 across 72, N15 at 1,348 across 68, N14 at 1,317 across 67, N13 at 1,291 across 65, N12.2 at 1,255 across 63, N12.1 at 1,211 across
63, N12 at 1,186 across 62, N11 at 1,125 across 58. The frozen count at the tested release commit is in
[`releases/4fb1554.md`](releases/4fb1554.md) and does not move.


| # | Area | Status |
| --- | --- | --- |
| 1 | Syntax, parser, CST | VERIFIED — the reference parser: incremental reparse and recovery (N76); attributes and conditional compilation (Gate 2); the compiler written in Nazm's syntax is N98's |
| 2 | Name resolution and modules | VERIFIED — durable identities, interfaces and resolved units, a declared root, packages, re-exports with one identity (N103, N107); check reuse for builds is area 26's |
| 2a | Incremental semantic checking | VERIFIED |
| 3 | Type system | VERIFIED as scoped (N107, Gate 2) — records, closed enums, generics with bounds, closures, traits with static dispatch (N78); fixed-width integers, `Float32`/`Float64`, bit operators and conversions, fixed arrays of plain data and module constants, `Bytes` buffers and views, and `Text` (Gate 2); trait objects, generic traits and default methods refused by name |
| 4 | Memory model and reclamation | VERIFIED for the current type universe — closure environments are ownership-graph nodes; the capture that could close a cycle is refused (`N0616`, Gate 1-C1) |
| 5 | Effects | VERIFIED as scoped (N107) — inferred, checked compiler-owned effects with subsumption (N79); no handlers or user effects; pure is not total |
| 6 | Capabilities (language) | VERIFIED as scoped (N107) — static, coarse, shareable: no inherited authority (N104), `OutCap` attenuation (N79); `NetCap`, `RandomCap`, `ProcessCap`, and an open handle as its own authority (Gate 2) |
| 7 | Provenance and information flow (language) | VERIFIED as scoped (N107) — explicit data flow by cell, target and field (N80); implicit flow RESEARCH (R6), not claimed |
| 8 | Core IR | VERIFIED (v1) |
| 9 | MIR | VERIFIED (v1) |
| 10 | LIR and the backend contract | VERIFIED (v2) — one instruction-level LIR, lowered once, that LLVM prints and Cranelift translates; validated; LIR's interpreter the oracle for the sequential subset; `nazm.lir/2` (N105) |
| 10a | Per-module code generation and linking | VERIFIED |
| 10b | Native object reuse | VERIFIED |
| 11 | LLVM backend | VERIFIED |
| 12 | Cranelift backend | VERIFIED (v1, native subset, host) |
| 13 | Runtime | VERIFIED (v1 contract; a built, versioned, verified artifact, N82; M:N tasks on bounded workers by default, N106) — no allocator of its own, runtime still text |
| 14 | Structured concurrency | VERIFIED |
| 15 | Advanced scheduler | VERIFIED as scoped (N83; the default since N106): an M:N pool, run on macOS aarch64 and x86-64 and on Linux aarch64; no stealing or preemption |
| 16 | Standard library | VERIFIED as scoped — 1.0 (N84): seventeen modules, a checked API manifest, four representative programs; Gate 2's collections, files and handles, I/O errors, networking, environment, processes, errors, binary encoding, randomness and logging (twenty-nine modules); no cryptography; numbers are the language's since Gate 2 (area 3) |
| 17 | FFI and ABI | VERIFIED as scoped — a practical C ABI subset (N85): opaque handles, C structs by borrowed pointer, strings both ways, `errno`, exports as callbacks, static and shared libraries, header bindings; every number across in both directions (Gate 2); no by-value structs, variadics or `dlopen`; on the 32-bit targets a C pointer crossing is refused by name (`N0381`, Q1-A1-05) |
| 18 | Embedded support | VERIFIED as scoped — three emulated boards in three architecture families (N86, Gate 3E): atomics, statics, `@section`, board manifests, AArch64 and Cortex-M interrupts, a heap arena on 64-bit boards, `@on_failure`, a fixed-priority `@task` scheduler and a watchdog (Gate 3D–3G), each emulator-verified under QEMU; RISC-V has no interrupts, Cortex-M no heap; no hardware |
| 19 | Critical profile | VERIFIED as scoped — profile enforcement and obligation evidence (N87): contracts, an obligation census with three statuses, no obligation of unknown status; not certification |
| 20 | Cybersecurity profile | VERIFIED as scoped — profile enforcement (N87): declared authority, no C, a locked build, contents kept in files, checked paths, no unknown calls; constant time RESEARCH; not certification |
| 21 | AI/HPC | PARTIAL — maps, zips and reductions of `Int`, `Float32` and `Float64` kernels behind a provider boundary: OpenCL on Apple's GPU and a CPU provider that is its second oracle, `Float64` refused on the GPU (N67, N88, Gate 3H); vectorised sums (N66); SIMD measured, no speed-up claimed; Metal, CUDA, ROCm and SPIR-V providers not built |
| 22 | Diagnostics | VERIFIED |
| 23 | Formatter | VERIFIED |
| 24 | Machine-applicable fixes | VERIFIED |
| 25 | LSP and MCP | PARTIAL / PARTIAL — across package boundaries tested; an editor client (N74) |
| 26 | Package and build tooling | PARTIAL — templates, a library check, API docs and a publish dry run (N74); a backtracking resolver that explains a refusal, and `nazm update` (N90); `@test` functions, fuzz targets and `nazm bench`, package features (Gate 2) |
| 27 | Debugger and profiler | PARTIAL — lexical scopes, Cranelift line tables and a sampling profiler on macOS (N91); no Cranelift variables, no DAP |
| 28 | Performance measurement | PARTIAL — every measured claim filed in a gated register (N92): 10 reproducible, 53 dated, 8 current claims unreproduced |
| 29 | Differential testing | VERIFIED |
| 30 | Mutation testing and fuzzing | VERIFIED / PARTIAL — coverage-guided fuzzing of five targets with a replayed corpus (N93; `lir`, N105), re-run in Q1: 3.4 million executions, no crash; no sanitizers, Miri, Loom or backend/FFI/registry fuzzing |
| 31 | Bootstrap | VERIFIED as scoped — the declared subset the Nazm-written compiler carries; not Gate 2 (Q1-D-25) |
| 32 | Reproducibility and provenance | PARTIAL — a stated five-part model, signed in-toto statements and a CycloneDX SBOM (N94); one host and one image, no transparency log |
| 33 | Platform and target matrix | PARTIAL — every cell in the support matrix's words (N95, Gate 3): twelve targets — five run-verified, three emulator-verified, Windows run-verified under Wine only, Android run-verified on a Linux kernel only, x86_64 Linux and iOS compile-only; x86_64 Linux and a Windows host deferred to the final v1 qualification |
| 34 | Smart contracts (chain-neutral model and backends) | PARTIAL — model and simulator (N69); EVM backend, run in py-evm (N70); WebAssembly adapter (N71); signed writes and account metadata, sBPF execution BLOCKED (N72); both VMs held to the simulator on generated sequences (N96) |

After N107, 2026-10-05: twenty-nine VERIFIED (areas 2, 3, 5, 6 and 7 moved, the last four as scoped:
`audit-n107.md`), eight PARTIAL — every one an ecosystem or domain row — none RESEARCH or MISSING.

*Historical.* After N106, 2026-10-05: twenty-four VERIFIED (area 13 moved: the M:N pool by default, its policy
stated), thirteen PARTIAL, none RESEARCH or MISSING.

*Historical.* After N105, 2026-10-05: twenty-three VERIFIED (area 10 moved: one LIR both backends translate, with its
oracle), fourteen PARTIAL, none RESEARCH or MISSING. N101–N104 moved no row.

*Historical.* After N100, 2026-10-04 (`docs/audit-n100.md`): twenty-two VERIFIED (several as scoped, area 30 for
mutation testing), fifteen PARTIAL, none RESEARCH or MISSING; BLOCKED parts named inside areas 12
(JIT), 34 (sBPF) and the formal evidence (proofs). Nothing moved between N88 and N100: N89–N99
widened PARTIAL rows and measured them. The zero-partial objective is not met.

*Historical.* After N88, 2026-10-04: unchanged — area 21 widened and still PARTIAL.

*Historical.* After N87, 2026-10-04: twenty-two VERIFIED (areas 19 and 20 moved, as scoped: profile enforcement
and obligation evidence, not certification), fifteen PARTIAL, none RESEARCH.

*Historical.* After N86, 2026-10-04: twenty VERIFIED (area 18 moved, as scoped: two emulated
boards), seventeen PARTIAL, none RESEARCH.

*Historical.* After N85, 2026-10-04: nineteen VERIFIED (area 17 moved, as scoped: a practical C ABI
subset), eighteen PARTIAL, none RESEARCH.

*Historical.* After N84, 2026-10-04: eighteen VERIFIED (area 16 moved, as scoped: the standard
library 1.0), nineteen PARTIAL, none RESEARCH.

*Historical.* After N83, 2026-10-04: seventeen VERIFIED (area 15 moved, as scoped: an opt-in pool,
macOS), twenty PARTIAL, none RESEARCH; nothing else moved between N77 and N82.

*Historical.* After N76, 2026-10-03: sixteen VERIFIED (area 1 moved, for the reference parser),
twenty PARTIAL, one RESEARCH.

*Historical, not current.* After N75's audit, 2026-10-02, across the 37 rows above (34 areas, with 2a, 10a and 10b): fifteen
VERIFIED (areas 4, 8, 9, 12 and 30 with a stated qualification — area 30 for mutation testing, its
fuzzing PARTIAL), twenty-one PARTIAL, one RESEARCH (15, the advanced scheduler), none MISSING. No
row says DESIGNED or BLOCKED as its whole status; the blocked parts are named inside areas 12 (no JIT:
`unsafe_code = "forbid"`, N65) and 34 (no sBPF toolchain, N72).

*Historical, not current.* After N48 the count read fifteen VERIFIED, sixteen PARTIAL, two RESEARCH,
one MISSING, written as "across the 34 rows" though the table then had the same 37.

*Historical, not current — kept as it was written.* After N39 the count read thirteen VERIFIED,
thirteen PARTIAL, three DESIGNED, two RESEARCH, five MISSING; before that, nine, sixteen, three,
two, five, not kept up since the first era. The first era's summary follows: the shape is
worth reading directly: **everything verified is either a front-end capability, a tooling
interface, or an evidence mechanism.** Nothing in the semantic core — ownership, effects,
capabilities, provenance — exists at all, and that is the accurate summary of where the
project stood after the first era. That stopped being true at N36–N38, and the rows below are the
current account.

---

## Front end

### 1. Syntax, parser, CST — VERIFIED (the reference parser, N76)

**R1d (2026-10-14)** · *A bounded heap: `--heap-limit`* (`spec.md`, *A bounded heap*;
`openspec/changes/r1-language-for-the-rewrite`, D5). `nazm build --heap-limit N` (bytes or
`K`/`M`/`G`, 64 KiB to 1024 GiB), or `heap-limit = "N"` in the root package's manifest, which the
flag overrides, builds a program whose every allocation — its units', the runtime's and the
platform unit's — goes through the counted heap: `nz.heap_malloc`, `nz.heap_realloc` and
`nz.heap_free`, a new runtime unit holding the limit, which keep the bytes asked for in a 16-byte
header and one atomic live count. An allocation that would pass the limit writes `nazm: error[N0406]:
out of memory: the heap limit of N bytes is reached` and exits 2 at once, from any task; the stack
unit's task stacks are not heap and keep C's allocator. LLVM units are renamed in their text, a
Cranelift unit's LIR imports before its object is keyed; refused for a board, WebAssembly, a library
and a runtime artifact. Without a limit the program's text is what it was. Build provenance records
the limit. EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/heap_limit.rs` — 3,000 live
strings print what the interpreter and the LIR oracle print, with the same memory report and nothing
live, under a 4 MiB limit at LLVM `-O0`/`-O2` and Cranelift, and end with the fixed line, status 2
and what was printed before under 64 KiB; four pool tasks five times over allocate millions of bytes
through 64 KiB; the manifest's limit and the flag over it; the refusals; and without a limit no unit
names the counted heap, while with one no unit but the heap's and the stack's reaches C's allocator.

**R1d (2026-10-14)** · *Streaming standard input* (`spec.md`, *Files and handles*, **Lines of
standard input**, and *The standard library*; `openspec/changes/r1-language-for-the-rewrite`, P3).
`os_read_line(h, buf) -> Int`: standard input to and including its next newline, at most the buffer,
never past the line; `os_read` of standard input now reads the same 16 KiB buffer first, so lines
and frames mix — LSP's headers then the body they announce, MCP's lines. Any other handle is a
status (`InvalidInput`, a directory `IsADirectory`). `@std/io` gains `io_stdin_read`,
`io_stdin_read_exact` and `io_stdin_read_line`. EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/stdin_lines.rs` — two `\r\n` header lines, a five-byte frame, a line read
three bytes at a time, a byte, whole lines, a last line without a newline, the end and an
`UnexpectedEof`, alike in the interpreter, LLVM `-O0`/`-O2` and Cranelift, fed whole and a byte at a
time, with the same memory report and nothing live; another handle's statuses alike everywhere;
`os_read`'s authority, effect and provenance. The LIR oracle runs no `os_` built-in, so it has no
answer here.

**R1e (2026-10-13)** · *`@std/regex`* (`spec.md`, *The standard library*; `openspec/changes/r1-language-
for-the-rewrite`, S5). Regular expressions in Nazm, matched in time linear in the text: a pattern is
parsed into a tree, compiled into a Thompson NFA, and every search runs it as a Pike VM — all states
advancing together a scalar value at a time, each held by at most one thread, preferred threads first —
so a search is O(m·n) and nothing backtracks. Leftmost-first, as Rust's `regex` and Perl; capture groups;
`find_all` with Rust's empty-match rule. Rust's syntax less what an automaton cannot do (backreferences,
lookbehind, lookahead past one scalar value — each refused by name); a one-scalar lookahead `(?!C)` is
kept, which the GPT-2 and `cl100k_base` pre-tokenisers' `\s+(?!\S)` needs. UTF-8 aware: `.` and a class
match one scalar value. Unicode 16.0's general categories `L`, `M`, `N`, their subcategories and `Pc`
are carried as compact range tables; `\d`, `\s`, `\w` are Unicode's (`\w` but for a few
`Other_Alphabetic` scalar values); `(?i)` folds ASCII letters only. Errors are `what at byte N`.
EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/regex.rs` — 77 patterns on their texts,
the first part's expectations checked against Python's `re` searched as Rust's `find_iter` searches,
the Unicode classes and both pre-tokenisers' patterns by hand; nine captures; 26 malformed patterns
refused at their offset; five patterns exponential under backtracking (`(a*)*b`, `(a|aa)*c`, …) failing
at once on 40 `a`s interpreted and on 200,000 compiled; a `cl100k_base` pre-split, captures, a search
from an offset and an error alike under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and
Cranelift with nothing live.

**R1b (2026-10-13)** · *Linear-time string building* (`spec.md`, *Strings*, **Building**, and
*What this costs*; `openspec/changes/r1-language-for-the-rewrite`, D2). Core IR lowering writes `s =
str_concat(s, t)` on a `let mut` binding — not one a closure shares — as the internal `str_append`.
LIR appends `t` in place when `s`'s storage has one reference (the binding's), was made by an
append — its capacity carries a mark no other allocation's can — and has room after `s`'s bytes,
taking a reference for the new value that the old value's release gives back; otherwise it copies
into storage of twice the length (at least 16), marked. A literal has no storage of its own and is
copied. The interpreter applies the same rule to a backing an append made, taking the binding's
string out before it appends, so the two allocate the same strings. No checker rule moved, so no
epoch.
EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/string_append.rs` — two names for
one string each appended to, a slice kept across an append, a string appended to itself and a
literal, under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live;
100,000 appends allocating fewer than 64 strings under LLVM and Cranelift (200,000 allocated one
each before); `memory.rs`, sixteen appends filling one allocation exactly and a padded string's
spare byte left alone, counted the same by both implementations. The memory tests whose loops were
allocator churn or a count of concatenations now prepend, which still allocates each time.

**R1b (2026-10-13)** · *Warnings* (`spec.md`, *Patterns*; `diagnostics.md`;
`openspec/changes/r1-language-for-the-rewrite`, L15). A warning severity: `nazm.diagnostic/1`'s
`severity` takes `"warning"`, and the first warning is `N0632` — a `_` or a name in a written
`match` of an enum that covers variants no arm names, listing them (the voice L3 kept when it
admitted `_`). The checker records warnings on the resolution, apart from the errors a stage stops
on; `nazm check` and `nazm build` report them and succeed, `--deny-warnings` refuses, and the
language service publishes them at the warning severity. A module that warns is not kept as
checked, so the warning is not cached away. Semantic epoch 66; EXPERIMENTAL until R1 closes.
`nazm run` and `nazm test` do not report warnings, and a `@deprecated` use draws none yet (`N0620`
reserved). Evidence: `crates/nazm-cli/tests/warnings.rs` — both forms of the warning with exact
lists, none for a match naming every variant or an `if let`, again from the cache, as JSON;
`--deny-warnings` refusing a check and a build; a build that warns and runs.

**R1b (2026-10-13)** · *The stack asked for: `--stack-size`* (`spec.md`, *Native recursion, and
the stack it runs on*; `openspec/changes/r1-language-for-the-rewrite`, L14). `nazm build
--stack-size N` (bytes or `K`/`M`/`G`, 64 KiB to 4 GiB; refused for a board or WebAssembly) runs
`main` on a thread of `N` bytes — the platform entry starts it and joins it, and runs `main` in place
if it cannot start — and stores `N` in the runtime's `nz.stack_bytes`, which a task's thread, the
pool's task stacks (under `NAZM_TASK_STACK`) and `nz.stack_check`'s limit read in place of 8 MiB,
256 KiB and 8 MiB. Runtime ABI 31; build provenance records the size. The interpreter's threads
follow `--max-depth`, as before. EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/stack_size.rs` — a 250,000-deep recursion in `main` and in a task that
meets `N0408` by default and finishes with `--stack-size 64M`, and meets it again with `4M`, under
LLVM and Cranelift, on the pool and a thread per task; the refusals. **Corrected (2026-10-13):**
`main`'s thread ended with its failure state, which is thread-local, so a sized program whose
`main` failed exited 0 with its stand-in value; the thread now hands the state back for the entry's
report. Found by CI, where a million-deep test's 512 MiB stacks passed Linux's address-space
ceiling.

**R1 (2026-10-14)** · *Speed: the failure bit, and the failure paths laid out cold*
(`openspec/changes/r1-language-for-the-rewrite`, 5b.8; `docs/architecture.md` §7.106, *The failure
bit*). A Nazm function returns whether it failed beside its result — `{ T, i1 }` under LLVM, a
second result under Cranelift, a pair in the LIR oracle — false on every `return`, true on the new
`fail` exit; a direct or indirect call of one branches on that bit rather than loading the
thread's failure flag. `nz.fail` still sets the flag and the message, and what calls a Nazm
function from outside still reads it: a task's trampoline, an export's C entry, the platform entry,
which calls `main` (an interrupt handler, a board's task or failure hook) through a plain entry
under its symbol with its old signature, the function itself being `<symbol>.body`. The runtime is
unchanged (ABI as before); `TRANSLATOR_REVISION` 4. Blocks only a failure reaches are laid out cold
in both backends (LLVM branch weights, Cranelift cold blocks), without which the bit was slower
under clang. `fib(35)`, Apple M1 Pro, hyperfine median of 20: LLVM `-O2` 66.8 → 55.4 ms, Cranelift
106.3 → 83.8 ms. Evidence: `crates/nazm-cli/tests/failure_bit.rs` — a failure 400 frames deep in a
direct recursion, through a function value, through a trait method's generic instance, and in a
task, each ending with status 2 and `N0401` at its line with nothing live on the interpreter, LLVM
`-O0` and `-O2` and Cranelift (and the LIR oracle outside tasks); one inside a C export under both
backends at both levels; the LIR naming the bit and the plain entry; the validator refusing a bit
or a `fail` where the convention has none. Seven mutants.

**R1 (2026-10-14)** · *Conveniences for the first program* (`spec.md`, *Conveniences for the
first program*; 5b.16 and 5b.17, in part). `let x: T = e` checks `e` against `T`; a generic call's
type parameter its arguments leave open is unified with the type its result is wanted as
(`check_generic_call`'s `expected`); `/* … */` nests, `N0007` when unclosed, and the formatter keeps
what follows one on its line; the parser reads `loop` as `while true`, `while let` as a `match` with
a `break` arm, and an impl without a trait as functions named `<type>_<method>`. Evidence:
`crates/nazm-cli/tests/conveniences.rs` — all of them in one program, alike everywhere, nothing
live; the refusals that stand. Seven mutants. Left of 5b.16–5b.17: `[1, 2, 3]` as a `Vec`, match
guards, `?` on `Option`.

**R1 (2026-10-14)** · *Methods and indexes by name* (`spec.md`, *Methods and indexes by name*;
5b.15). Where no trait gives the receiver's type the method, `x.m(args)` is checked as the call
`<prefix>_m(x, args)` keyed by the method's name, its prefix the type's name in lower case (and
`text` after `str` for a `Str`), and Core IR lowering reads the call the checker recorded
(`Resolution::named_method`); `xs[i]` and a binding's `xs[i] = v` on a `Vec`, `Ints`, `Strs` or
`Bytes` are its `_get` and `_set` the same way. A compound assignment's read of an element is keyed
apart from its write. Evidence: `crates/nazm-cli/tests/named_methods.rs` — methods on a `Vec`, a
`Str`, an `Int` and a `HashMap`, indexes read, written and compounded, a trait's method winning,
alike everywhere with nothing live; what has no function of the name refused. Six mutants.

**R1 (2026-10-14)** · *R1 promoted to stable* (8.1; `spec.md`, *Stability classification for 1.0*).
Nineteen R1 sections and every inline "EXPERIMENTAL until R1 closes" became STABLE 1.0, as a 1.x
addition stable since R1 closed (0.10.0); `stability.md` and `limitations.md` follow. Seven cases
joined the 1.0 compatibility corpus, pinned (`tests/compat/v1/run/r1-*`): syntax, methods and
maps, patterns and tuples, strings, cells, `defer` and `for`, derived ordering and JSON, and R1e's
libraries — each run interpreted and by both backends. Writing them found three defects, fixed
under epoch 75: a `defer`'s last value was refused (it is discarded, as a loop body's is), a
pattern `let` needed its `;`, and a line beginning with `(` or `[` was read as a call or index of
the line before (`let (a, b) = p` then `(b, a)`); a call's `(` and an index's `[` are now on the
line of what they apply to. Evidence: `crates/nazm-cli/tests/compat_v1.rs`, `defer.rs`,
`line_breaks.rs`. Three mutants.

**R1 (2026-10-14)** · *The interpreter's stack bounds its depth* (4.9). Each evaluation thread —
the main one and every task's — records where its stack ends (`eval::mark_stack`), and a call
made past that, less the margin the shape-recursive passes need, is `N0403` ("recursed deeper than
the interpreter's stack holds"), so a call nested in an expression, several evaluator frames on a
debug build, reports instead of crashing below `--max-depth`. Evidence:
`crates/nazm-cli/tests/stack_floor.rs` — a million-deep nested recursion at the default and at
`--max-depth 15000`, `N0403` and never a signal. One mutant.

**R1 (2026-10-14)** · *Word frequency at 195.5 ms* (5b.9; `docs/performance.md`). The idiomatic
5b.10 program went from 531 to 195.5 ms (Apple M1 Pro, hyperfine median of 20): `@std/text`'s
`text_split` compares a slice only where the first byte matches and `text_lines` is one pass;
`@std/hashmap` remembers the entry `m[k]` found so `m[k] += n` writes without a second probe, and
reads its vectors once a probe; MIR borrows a field of a local read only to be an operand
(`Rvalue::FieldBorrow`: no reference taken or given back, left out of the unwind block's drops, and
the validator refusing a drop or move of one); the LLVM backend declares the runtime's state
`hidden`, so it is addressed without the global offset table. Evidence:
`crates/nazm-cli/tests/borrowed_fields.rs` — a borrowed field live across a failure, every
implementation reclaiming everything; `named_methods.rs` — a remembered entry never another key's,
nor surviving a removal; lines and splits at their edges. Seven mutants.

**R1 (2026-10-14)** · *One list type* (`spec.md`, *Sequences are shared, mutable, and never freed*;
5b.18). `Ints` and `Strs` resolve to `Vec[Int]` and `Vec[Str]` — interned before any type is
resolved, in the built-ins' signatures and when an interface is read — and are shown by those
shorter names; the interpreter's `ints_*` and `strs_*` are its `vec_*` on one value, the separate
`Value::Ints` and `Value::Strs` gone; native code already shared one layout. A `Strs` and a
`Vec[Str]` share one provenance cell. Epoch 74. Evidence: `crates/nazm-cli/tests/lists.rs` — the
two names passed for each other through every family of built-ins and through `@std/text`, alike
everywhere with nothing live; `for_loops.rs` for the shared cell. Three mutants.

**R1 (2026-10-14)** · *The program's authority reported* (`spec.md`, *What `main` holds, and how it
ends*; 5b.1). `nazm build` writes `authority: …` on standard error after a build of a program with a
`main` — each capability `main` holds, given by option C or written, and what it reaches — and
`nazm inspect` gives the same as `authority` (`crates/nazm-cli/src/authority.rs`, read from `main`'s
signature as parsed). 5b.5, effects inferred for a function that is not `pub`, was found to be the
rule since N36 and is pinned. Evidence: `crates/nazm-cli/tests/main_ends.rs` — a given `OutCap` and a
written `IoCap` and `NetCap`, from `build` and `inspect`; a private function with no effect set
called by a pure one, and another module's undeclared one refused (`N0366`). Two mutants.

**R1 (2026-10-14)** · *Match guards* (`spec.md`, *Patterns*, Guards; 5b.17). `P if G => E`: the
parser reads the guard into `MatchArm::guard`; the checker types it as a `Bool` (`N0301`) with the
pattern's names bound and leaves a guarded arm out of exhaustiveness and reachability; Core IR
lowering sends any `match` with a guard to the general lowering, whose arm test becomes `if <pattern
matches> { <names bound>; G } else { false }`, the names bound in a region of their own before the
arm's body binds them again. Evidence: `crates/nazm-cli/tests/match_guards.rs` — guards on
integers, on variants with an `Int` and a `Str` payload, on `Option`, and in a loop, alike
everywhere with nothing live; a guard that is not a `Bool`, a guarded arm relied on for coverage, a
guarded arm below a catch-all, and a name a guard cannot see, refused. Five mutants.

**R1 (2026-10-14)** · *Map entries by index* (`spec.md`, *Methods and indexes by name*; 5b.7). On any
type but a sequence, `m[k]` is `<prefix>_at(m, k)` and a binding's `m[k] = v` its `_set`, the prefix
as a method's; a compound update's read is `<prefix>_at_or(m, k, zero)` with the value type's zero
(`check::zero_of`; `Resolution::map_fallback`, which Core IR lowering reads to pass the same zero).
`@std/hashmap` and `@std/ordmap` gained `_at` and `_at_or`. A compound update's key may be a string.
Evidence: `crates/nazm-cli/tests/named_methods.rs` — counting words, a `Str` and a `Float64` entry
built by `+=` and `*=`, written and read, alike everywhere with nothing live; an absent entry read by
index failing `N0405` in every implementation. Five mutants.

**R1 (2026-10-14)** · *Short variants and inferred enum arguments* (`spec.md`, *Short variants*;
`openspec/changes/r1-language-for-the-rewrite`, 5b.14). An enum's construction written without type
arguments takes them from the type it is wanted as, or from payload fields declared as its
parameters (`check/construct.rs`, `inferred_enum`), each payload value checked once; where neither
decides, `N0352` as before. The parser reads `Some(x)`, `Ok(x)`, `Err(e)` and `None` as the
prelude's variants in expressions and patterns, `Enum.Variant` without `()` in a pattern, and a
capitalised `Name.Member` without `()` in an expression as a variant. Evidence:
`crates/nazm-cli/tests/short_variants.rs` — a program of them alike under the interpreter and each
backend, nothing live; what nothing decides still `N0352`. Seven mutants.

**R1 (2026-10-14)** · *`print` of any value, and `println`* (`spec.md`, *Output*; 5b.4 and 5b.13).
The parser reads `print(x)` and `eprint(x)` of anything but a string literal as `print(f"{x}")`,
and `println(x)`/`eprintln(x)` as `print(f"{x}\n")` — the hole keyed by the argument list's span,
so a `Show` resolves as an interpolation's does; `println` and `eprintln` are built-in names.
Evidence: `crates/nazm-cli/tests/printing.rs` — every kind of value on both streams alike
everywhere, nothing live; what has no text, a function named `println`, and printing without
`OutCap` refused. Three mutants.

**R1 (2026-10-13)** · *Compound assignment* (`spec.md`, *Mutation is opt-in and visible at the
binding*; `openspec/changes/r1-language-for-the-rewrite`, 5b.12). `+=`, `-=`, `*=`, `/=`, `%=`,
`&=`, `|=`, `^=`, `<<=` and `>>=`, each one token, parse as `place = place op e`, so nothing below
the parser changed; an index in the place is a name, a field of one or a number, and one with a
call is refused by name rather than computed twice. Epoch 72. Evidence:
`crates/nazm-cli/tests/compound_assign.rs` — every operator on a binding, a field and an element,
alike under the interpreter and each backend; overflow failing as the operator does; what it cannot
mean refused; the formatter keeping each token. Six mutants.

**R1 (2026-10-13)** · *`else if`, a line break for `;`, and a value nobody wants*
(`spec.md`, *`else if`, and a line break for `;`* and *One block*;
`openspec/changes/r1-language-for-the-rewrite`, 5b.11 and 5b.19). `else if` (and `else if let`)
parses as `else { if … }`, so nothing below the parser changed; `docs/grammar.ebnf` and the guide
had written it since 2026-09-21 while the parser refused it, which no example caught — two
examples now do. A missing statement `;` is taken as written where a line break or the block's
`}` follows; an expression still continues over a line wherever the parser can continue it; a
member's name must be on its `.`'s line. `nazm fmt` reads the same rule, so a program without
semicolons is laid out as one with them is. A loop body's last value is discarded, and an `if` as
a statement — or the last line of a loop body, or of such an `if`'s branch — may end its branches
differently and produces nothing; where a value is wanted, `N0302` stands. Epoch 71. Eight test
fixtures that relied on a line break not ending a statement were put on one line, and an
`incomplete.rs` expectation that existed only because its fixture lacked a `;` now offers the
variants. Evidence: `crates/nazm-cli/tests/line_breaks.rs` — `else if` and `else if let` chains as
values and as statements, a program without semicolons and the discarded values reclaimed, alike
under the interpreter and each backend; what still needs a `;` refused by name; the formatter's
layout of such a program, kept. Eight mutants.

**R1 (2026-10-13)** · *Speed: integer text, the map's table, one string per interpolated
integer, inlined reference counting* (`openspec/changes/r1-language-for-the-rewrite`, 5b.9). An
integer's text is the runtime's own digit loop (`nz.format_int`, `nz.format_u64`), not `snprintf`.
`@std/hashmap`'s table holds positions into dense arrays of hashes, keys and values, a power of two
long: a probe compares hashes, a replaced value allocates nothing, a removal moves the last entry
into the freed place; the slots, and so the iteration order, are as before. An interpolation's hole
of an integer type of at most 64 bits after other text lowers to the internal `str_concat_num`: one
string, the digits written after the text. A hosted unit carries `available_externally` copies of
`nz.arr_retain`, `nz.arr_release`, `nz.seq_release`, `nz.str_retain` and `nz.str_release`, inlined
where it is optimised; a board's runtime defines its own and is left alone. Word frequency over
2,000,000 words under LLVM `-O2`: 392 → 222 ms; `fib(35)` unchanged at 68 ms. Evidence:
`crates/nazm-cli/tests/speed_text.rs` — every integer type's extremes, alone and after text, alike
under the interpreter and each backend; a thousand `f"w{i}"` allocating one string each,
everywhere; the map against a model of itself over 3,000 operations; a hosted unit's copies. Seven
mutants.

**R1 (2026-10-13)** · *Speed: a program of one thread, and the inlined stack check*
(`openspec/changes/r1-language-for-the-rewrite`, 5b.8). The stack check (N49) is three instructions
in the function — this frame against the thread's limit — calling the runtime only on a thread's
first check or past the limit (LLVM; Cranelift still calls it). A program whose `main` holds no
`SpawnCap`, `NetCap` or `ForeignCap` and is not a library has one thread, and its failure state and
stack limit are plain globals rather than thread-local ones, in every unit, the entry and the
runtime (`Native::single_thread`, `Needs::single`); a program linked to a prebuilt runtime keeps
them thread-local. `fib(35)` under LLVM `-O2`: 141.6 ms before, 68.3 ms after (C 36.8, Go 41.0, the
same machine); word frequency 453 → 392 ms. Evidence: `crates/nazm-cli/tests/single_thread.rs` —
the state plain for a `main` of output alone, thread-local for one with `SpawnCap`, `NetCap` or
`ForeignCap` and for a library, the check inlined; a stack overflow and an overflow still reported
by a program of one thread under LLVM and Cranelift. Five mutants.

**R1e (2026-10-13)** · *`@std/sha256`, `@std/blake3` and `file_walk`* (`spec.md`, *The standard
library*; `openspec/changes/r1-language-for-the-rewrite`, D4 and S1). Two digests in Nazm, over
32-bit wrapping arithmetic and rotations: SHA-256 (FIPS 180-4) and BLAKE3's unkeyed 32-byte output
(the reference algorithm: 1024-byte chunks, seven-round compression, a stack of subtree chaining
values, `ROOT` on the last compression). Each is a one-shot function, a hex form and an incremental
hasher — a handle whose finish leaves it unchanged. Digests for identity and integrity, not the
cryptography 1.0 leaves out: no constant-time claim, no audit. `@std/file` gained `file_walk(io,
dir)`: every regular file and directory under `dir`, sorted by bytes. With no `lstat` built-in yet
(P2), a symbolic link is walked as what it names; a dangling one is left out, and one reaching its
own ancestor ends in the system's `Err`. EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/hash_vectors.rs` — FIPS 180-4's examples and messages on each side of the
padding's boundaries, and the BLAKE3 team's `test_vectors.json` lengths up to 3,073 bytes, each
whole and in uneven pieces, alike under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and
Cranelift with nothing live; a million `a`s and every official BLAKE3 length to 102,400 bytes under
LLVM `-O2` and Cranelift; the embedded BLAKE3 vectors checked against the `blake3` crate.
`crates/nazm-cli/tests/file_walk.rs` — a tree sorted as bytes, not as a depth-first listing, under
the interpreter and LLVM; on Unix, links to a file, to a directory, dangling and looping. Seven
mutants.
**R1e (2026-10-13)** · *`@std/args`* (`spec.md`, *The standard library*; `openspec/changes/r1-language-
for-the-rewrite`, S4). A command-line parser in Nazm, for writing the toolchain's own command line:
a specification built by functions (`args_program`, `args_command` for sub-commands nested to any
depth, `args_flag`, `args_option` and its default and required forms, `args_positional`,
`args_rest`, `args_conflict`) into one arena, and `args_parse` over any `Strs` — `process_args(io)`
for the process's own — answering an `Args` read by name, or an `Err` naming the command and the
argument at fault. The POSIX and GNU forms: `--name value`, `--name=value`, `-n value`, `-nvalue`,
bundled `-abc`, `--`, `-`; repeated options and counted flags; `--help`, and the help and usage text
generated from the specification. No new built-in and no checker change: the arguments come from the
existing `arg`/`args_count` through `@std/process`. EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/args.rs` — twenty-six command lines (every form, and every mistake with its
message) and the help of three commands, alike under the interpreter, the LIR oracle, LLVM
`-O0`/`-O2` and Cranelift with nothing live; and executables built by LLVM and Cranelift parsing the
arguments they were started with (spaces and non-ASCII in values, `--`, help on standard output,
an error on standard error with status 2).
**R1e (2026-10-13)** · *`@std/bytebuf`, `@std/ordmap`, `@std/bitset` and the in-place sorts*
(`spec.md`, *The standard library*; `openspec/changes/r1-language-for-the-rewrite`, S2 and S3). In
Nazm, each a handle like the other collections. `ByteBuf`: one buffer doubling as it fills, numbers
in either byte order, ULEB128 and SLEB128, padding to an alignment, patching a field written
earlier, and indexed reads and writes that answer `None` or `false` outside the bytes written.
`@std/ordmap`: the `Ord` trait, implemented for every integer type, `Bool` and `Str`, and
`OrdMap[K: Ord, V]`, an AVL tree in parallel vectors — insertion, lookup and removal in `O(log n)`
comparisons, a removed node's slot taken by the last so nothing removed stays alive — with its ends,
floor and ceiling, a half-open range, and keys, values and entries in ascending order.
`@std/bitset`: a growable set of non-negative `Int`s in 64-bit words, with a walk by `bitset_next`
and union, intersection and difference. `@std/sort` gained `vec_sort_in_place_by` (stable,
effect-polymorphic: insertion runs of sixteen, merged bottom-up through one scratch vector),
`ints_sort_in_place` and `strs_sort_in_place`. API-1.0 gains 57 lines. EXPERIMENTAL until R1
closes. Evidence: `crates/nazm-cli/tests/collections_r1e.rs` — one program, the same output under
the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live: LEB128 at its
edges and at both ends of `Int` and `UInt64`, and six hundred fixed-seed values written alike by a
`ByteBuf` and `@std/binary`'s `Writer`; an `OrdMap` over `Str`, a program's own `Ord` key and `Int`,
the last under three thousand fixed-seed insertions, removals and lookups checked against a sorted
vector, its keys, values, ends, neighbours and ranges compared as it goes, then ascending and
descending runs; a `BitSet` against a vector of flags; the in-place sorts against the copying ones,
stability checked by original positions, at lengths around the insertion runs. Three hundred
thousand ascending keys, half removed, finish natively within twenty seconds; a key type with no
`Ord` is refused. Not measured: speed against the Rust containers the rewrite replaces.

**R1c (2026-10-13)** · *`@derive(toml)`* (`spec.md`, *Derived serialisation*, **TOML**;
`openspec/changes/r1-language-for-the-rewrite`, L2c). The same shape over `@std/toml`'s `Toml` tree,
alone or beside `json`: `t_toml_value` and `t_toml_read` for a record or an enum, and for a record —
a document is a table — `t_to_toml` and `t_from_toml`. `@codec` shapes it as it shapes JSON. TOML
has no `null`: a `None` is left out and a missing key read as `None`, so an `Option` inside another
type is refused (`N0619`). The generator is per format (`crates/nazm-core/src/derive.rs`), the text
importing each format it used; `@std/toml` gained `toml_is_table`. Semantic epoch 70. Evidence:
`crates/nazm-cli/tests/derive_toml.rs` — a deployment of nested records, an array of tables and
both enum encodings, written, read back to the same text, read from hand-written TOML and refused
at its path, under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing
live; the repository's own package manifest read through a derived record; what TOML cannot
carry. Five mutants.

**R1c (2026-10-13)** · *`@codec(…)`* (`spec.md`, *Derived serialisation*, **Shaping it**;
`openspec/changes/r1-language-for-the-rewrite`, L2b). The closed attribute set: on a derived type
`rename_all` (five cases), `deny_unknown`, and for an enum `tag` (internally tagged) or `untagged`
(the first variant that decodes, so its schema is `anyOf`); on a field `rename`, `default` (a
number's zero, `false`, `""`, `None`, an empty `Vec` or `List`), `omit_none`; on a variant
`rename`. An attribute may now stand before a field or a variant — the parser, the incremental
tree, and the formatter (its own line in a list one to a line, inline in one kept on one line) —
and only `@codec` is one there. Every misuse is `N0619`, and nothing is generated for a type it
refuses. Semantic epoch 69. Evidence: `crates/nazm-cli/tests/derive_json.rs` — every argument
under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; thirteen
misuses refused alone; the formatting, and its stability. The three schemas passed an independent
2020-12 validator, valid documents accepted and each malformed one refused. Seven mutants.

**R1c (2026-10-13)** · *`@derive(json)`* (`spec.md`, *Derived serialisation*;
`openspec/changes/r1-language-for-the-rewrite`, L2a). Before a `struct` or `enum` that is not
generic, it gives the type `t_to_json`, `t_from_json`, `t_json_schema` and the helpers a derived
type of another module calls — ordinary Nazm the loader appends to the file
(`crates/nazm-core/src/derive.rs`), with the `use` of `@std/json` it needs, so every implementation
runs the same functions and nothing below the front end is new. Decoding never traps: an `Err`
names the path. The schema is JSON Schema 2020-12 with `$defs`, so a recursive type's is finite. A
generic type, an unknown format, a field of a type the format cannot carry, or of a type the program
declares without the derive, is `N0619` at the attribute or the field. Semantic epoch 68;
EXPERIMENTAL until R1 closes; the attribute set (L2b) and `@derive(toml)` (L2c) remain. Evidence:
`crates/nazm-cli/tests/derive_json.rs` — every carried field kind, out-of-range and mistyped
members refused at their path, under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and
Cranelift with nothing live; a derived type of another module, plain and qualified; each refusal
alone. The schema also passed an independent 2020-12 validator when written. Six mutants.

**R1c (2026-10-13)** · *`@std/toml`* (`spec.md`, *The standard library*; `openspec/changes/r1-language-
for-the-rewrite`, S1). TOML 1.0, in Nazm: `pub enum Toml`, recursive through `List`, and
`toml_parse`, `toml_encode`, `toml_get`. A document is built in an arena of mutable nodes — where a
later header or dotted key can still extend a table, and where each table records how it was made
(explicitly, as a header's prefix, by a dotted key, inline) so that every redefinition TOML forbids is
refused — then turned into the immutable tree. Errors are `line:column: what`; nothing traps. The
LIR oracle gained `nz.float_parse` and `nz.float_parses`, which it had refused by name. The first
repository source to use interpolation and an `if let` with no `else` found two faults the sweeps
over every file now hold: the incremental lexer moved an interpolated string after an edit but not
its holes' positions (fixed), and the syntax test's own walk counted the empty `else` such an `if
let` is given as a written block (fixed in the test). EXPERIMENTAL
until R1 closes. Evidence: `crates/nazm-cli/tests/toml.rs` — a document of every form decoded alike
under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live, its encoding
a fixed point; eighteen invalid shapes refused at their line and column; every `nazm.toml` in the
repository and a `nazm.lock` from `nazm lock`, parsed and round-tripped. Beyond the repository: the
553 TOML 1.0 cases of the `toml-test` suite (v2.2.0), all decoded as expected — and every valid one
re-encoded, re-parsed to the same values and found a fixed point — under LLVM and under Cranelift; the
suite is a third party's and is not vendored.

**R1b (2026-10-13)** · *128-bit integers and the widening multiply* (`spec.md`, *Numbers*;
`openspec/changes/r1-language-for-the-rewrite`, L13b). `Int128` and `UInt128` under every rule of
*Numbers* at their width; literals up to `UInt128`'s maximum, and a negated one past `Int`'s
magnitude judged whole where an `Int128` is expected; `widening_mul(a: UInt64, b: UInt64) ->
UInt128`, exact. A value's exact integer is `nazm_sema::numeric::Wide` (`[-2^127, 2^128 - 1]`) and
its bits 128 wide; LIR has a class `i128`, laid out in 16 bytes aligned to 8 (stated on every LLVM
load and store); LLVM and Cranelift make instructions of the rest, and division, remainder, the
signed checked product, float conversions and text are the runtime's wide part, taking values by
address (runtime ABI 32), which a board or WebAssembly build that reaches it refuses by name. A
derived hash is `hash_combine` of the two words' `hash_u64`, as `@std/hash`'s new impls are. No C
export of one (`N0391`). Semantic epoch 67; EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/wide.rs` — every operation at each bound, the conversions by every mode, a
`Vec` and an `Option` of one, `Bytes` at sixteen, literals, under the interpreter, the LIR oracle,
LLVM `-O0`/`-O2` and Cranelift with nothing live; each failure with its code everywhere; a hash map
keyed by one and derived hashes; a constant across modules and the cache; the refusals. Eleven
mutants; six earlier ones repointed.

**R1b (2026-10-13)** · *Checked arithmetic* (`spec.md`, *Numbers*, **Functions**;
`openspec/changes/r1-language-for-the-rewrite`, L13a). `checked_add`, `checked_sub` and
`checked_mul`, over every integer type, return `Option[T]` — `Some` of the exact result, `None`
where it does not fit — as prelude names; a built-in signature may now name the prelude's `Option`
(`Shape::OptionT`). Core IR lowering writes each from operations every implementation has: a sum or
difference overflows exactly when its wrapping and saturating results differ; a product, `a` being
neither `0` nor `-1`, exactly when the wrapped product divided by `a` is not `b`, and `-1` times a
signed minimum is tested by name. Nothing below Core IR is new. Semantic epoch 65; EXPERIMENTAL
until R1 closes. `Int128`, `UInt128` and the widening multiply (L13b) remain. Evidence:
`crates/nazm-cli/tests/checked.rs` — each bound, `-1` times the minimum and zero at `Int`, `Int8`
and `UInt8` under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing
live; a float refused; the prelude name.

**R1b (2026-10-13)** · *Scope-exit cleanup: `defer`* (`spec.md`; `openspec/changes/
r1-language-for-the-rewrite`, L12). `defer { … }` runs a block whenever its block is left — at its
end, or by `return`, `?`, `break` or `continue` — the last registered first, after a returned value
or a block's final value is computed. `defer` is a word only at a statement's start before `{`. The
checker refuses a body that would leave (`N0631`: `return`, `?`, or a `break`/`continue` out of it)
and a body with a value; a loop inside one may leave itself, a closure inside one may return.
Lowering only: Core IR registers each body with its loop depth and emits it — lowered afresh —
where its block falls through, in `leave` before every `return` (and so every `?` and `ensures`),
and before a `break`/`continue` for the bodies registered inside the loop it leaves; nothing below
Core IR is new. Provenance walks a body where it is written and again where its block ends.
Semantic epoch 64; EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/defer.rs` —
order, nesting, an early return, a loop's `break` and `continue`, `?` on success and failure,
under the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; the
refusals and what stays accepted; provenance reaching a body from after it.

**R1b (2026-10-13)** · *Impls of generic types* (`spec.md`, *Traits and methods*, **Impls of generic
types**; `openspec/changes/r1-language-for-the-rewrite`, L11b). `impl[A: Show, B] Show for Pair[A,
B]` implements a trait for every instance whose arguments meet the impl's requirements. The parser
writes the impl's parameters first into each method, so each is a generic function; the checker
files the impl under the generic definition (`generic_impl_type`; `N0604` unless the header is the
definition applied to exactly the parameters), gives each method `Self` as the definition applied
to its own parameters, and records each parameter's requirement on the impl. A lookup for an
instance falls back to its definition's impl with the instance's arguments
(`Traits::applying`, `Resolution::method_target`), and is usable only where those arguments meet
the requirements, recursively (`Checker::satisfies`); a direct call records the arguments, MIR's
bound dispatch passes them, the instance planner follows each call through a bound inside an
instance to the impl or default it reaches, and Core IR carries each instance's definition for the
interpreter's impl table. Coherence and the orphan rule are the definition's. Semantic epoch 63;
EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/generic_impls.rs` — a pair and an
option of nested instances shown directly, by UFCS, through a bound and by a default, under the
interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; across modules by
both backends; the header's form, unmet requirements by a call and by a bound, coherence, the
orphan rule.

**R1b (2026-10-13)** · *Default trait methods* (`spec.md`, *Traits and methods*, **Default bodies**;
`openspec/changes/r1-language-for-the-rewrite`, L11a). A trait method may end in a body, which an
impl that writes none gets; an impl may still write its own. The parser writes each body as a
function `Trait/default/m` generic over `Self: Trait`, so the checker checks it once, through the
bound, against the signature's declared effects (none written is none); `N0602` spares a method
with a body. A call on a type whose impl leaves the method to it — direct, UFCS, or through a bound
— resolves to the default at that type (`Resolution::method_target`): the checker records the
receiver's type as the call's type argument, MIR's bound dispatch passes the instance's type, the
instance planner plans the default at every type whose impl relies on it, and Core IR's impl
table, which the interpreter dispatches through, fills the slot with the default. A default called
from another module is held to its trait method's effects, as an impl's method is (M1). Semantic
epoch 62; EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/default_methods.rs` —
defaults called directly, by UFCS and through a bound, one overridden, one performing `io`, under
the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; a trait and its
impl in different modules built by both backends; a body checked once, generic over `Self`, held
to its effects; a method with no body still required.

**R1b (2026-10-13)** · *Closures in generic functions; a shared `let mut`* (`spec.md`, *Function
values and closures*; `openspec/changes/r1-language-for-the-rewrite`, L10). A closure, and a named
function's value, may be written in a generic function, the closure's types naming the function's
type parameters (`N0387` narrowed): `nazm run` is parametric, and the instance planner instantiates
each closure with the generic function it is written in — through every closure between —
substituting under that function's parameters (`Resolution::type_owner`); a thunk a generic body
makes a value of is bindable across objects, as what a generic body calls is. A captured `let mut`
binding is shared, not copied (`N0386` narrowed to one that can hold a function value): the
checker records it and every closure's copy as shared, and Core IR lowering keeps it in a `Cell` —
`let mut x = e` makes the cell, a read is `vec_get(x, 0)`, `x = v` is `vec_set(x, 0, v)` and
`x.f = v` copies out, changes and sets back — so nothing below Core IR is new. Provenance reads and
writes a shared binding through its cell, so a closure's write reaches its function's read.
Semantic epoch 61; EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/
generic_closures.rs` — generic closures (nested, over `Int` and `Str`, with a named function's
value) and shared bindings (a closure's writes, a field through a closure, a write after the closure
is made, nested closures, a generic function's) by hand-worked output under the interpreter, the LIR
oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; a generic closure and a thunk built
across modules by both backends; provenance through a shared binding. The compat case
`refuse/capture-let-mut.nz` is SUPERSEDED.

**R1b (2026-10-12)** · *A mutable cell: `Cell[T]`* (`spec.md`; `openspec/changes/
r1-language-for-the-rewrite`, L7). One value that can be replaced, reached through a handle, so a
callee's `cell_set` is its caller's: `cell_new`, `cell_get` and `cell_set` as prelude names. The
kind `Vec` is — a mutable handle — so an ownership edge through it is counted (`N0359`), it has no
equality (`N0304`), it does not cross into a task (`N0321`), and a closure may not capture one that
can hold a function value (`N0616`). A `Vec[T]` of one element underneath and a distinct type to the
checker only: Core IR lowering writes the three as `vec_new` with `vec_push`, `vec_get(c, 0)` and
`vec_set(c, 0, v)`, so the interpreter, MIR, LIR and both backends need nothing new, and no runtime
ABI moved. Provenance follows what a cell holds, from `cell_new` and from every `cell_set`.
Semantic epoch 60; EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/cell.rs` — a
context record updated by callees, sharing, replacement releasing what it replaced (a string, a
record, a vector) and a cell inside a box, by hand-worked output under the interpreter, the LIR
oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; the ownership rule; the refusals; the
prelude names; provenance; a `Cell` type crossing a module's interface.

**R1a (2026-10-12)** · *String interpolation* (`spec.md`; `openspec/changes/
r1-language-for-the-rewrite`, L4). `f"…"` and `f"""…"""`: holes `{expr}` and `{expr:spec}`
(`[<|>][width][.precision]`), `{{`/`}}` for braces; a hole holds a `Str`, a number, a `Bool` or a
value with `Show` in scope (its `show()` resolved as the method call at the hole's braces, through a
bound too). The lexer reads an `FSTRING` token whose holes the parser parses on their own and
places; lowering joins the pieces with `str_concat`. New prelude built-ins `float_to_str_fixed`
(runtime `nz.format_fixed`, C's `%.*f`; runtime ABI 30) and `str_pad_start`/`str_pad_end` (a
loop in LIR). `N0006`; semantic epoch 59; EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/interpolation.rs` — every kind of hole by hand-worked output on the
interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; ties to even and
`-0.0`; refusals by code.

**R1a (2026-10-12)** · *Multi-line and raw strings* (`spec.md`; `openspec/changes/
r1-language-for-the-rewrite`, L9). `"""` strings whose closing delimiter's indentation comes off
every line (`N0005` for a line short of it, or text after the opening delimiter), escapes as an
ordinary literal's and a backslash ending a line joining it; raw `r"…"`/`r#"…"#` with any number of
`#`s, no escape, line breaks as written; CR LF read as LF in both. The lexer decides each from the
literal's own bytes, so incremental relexing is unchanged; an ordinary literal stays one line.
Semantic epoch 58; EXPERIMENTAL until R1 closes. Evidence: `crates/nazm-cli/tests/
string_literals.rs` — the bytes by hand on the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and
Cranelift; the formatter keeping each as written; refusals by code.

**R1a (2026-10-12)** · *Constant tables and exported constants* (`spec.md`;
`openspec/changes/r1-language-for-the-rewrite`, L8). An array may hold `Str`, `Text` and records,
enums and arrays of them (`N0621` names a handle): MIR counts an array managed by its element,
writing an element moves the value in and releases the old one, and LIR retains on every copy in
and out and releases element by element (`each_owned`'s array loop). A constant may be a record,
enum value, tuple or array of constant parts, computed by a second phase once types are known,
and lowered at each use to its construction; `pub const` publishes the value in the interface
(`nazm.interface/12`, each value keyed by field and variant names) and an importer reads it as
`NAME` or `m::NAME`; a stage 1b computes constants in dependency order so one may be computed
from an import's. Semantic epoch 57; EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/const_tables.rs` — string arrays and constant tables by hand-worked output
in the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; constants
across modules, plain and under a name, under `run` and two `build`s (the second from the cache);
the published interface; refusals by code.

**R1a (2026-10-12)** · *Ordering and hashing are derived* (`spec.md`; `openspec/changes/
r1-language-for-the-rewrite`, L6). `< <= > >=` on `Bool`, `Str` (bytes) and records, enums and
tuples of ordered components, by the **names** of fields and variants (the canonical order the
layout already used, so reordering a declaration changes no comparison), a tuple by position (at
most eight elements, so its field names sort as its positions); `==` on `Box` and `List` by what
they hold; `@std/hash`'s `Hash` satisfied without an impl by a concrete record, enum or tuple of
integers, `Bool`s and `Str`s, `hash` folding `hash_combine` from `hash_u64(0)` (or the variant's
name) in the canonical order and `same` being `==`. The interpreter computes both from Core IR's
new canonical-order table; MIR lowers a derived trait call to `Rvalue::Hash` or a comparison; LIR
generates `ord.*` and `hash.*` helpers (FNV-1a and SplitMix64 as instructions, constants folded at
build time). Semantic epoch 56; EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/order_hash.rs` — orders and hashes by hand-worked output, and against the
library's own functions, in the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with
nothing live; reordering a declaration changes no order and no hash; refusals by code, naming the
component; a written impl keeps its meaning.

**R1a (2026-10-12)** · *Tuples* (`spec.md`; `openspec/changes/r1-language-for-the-rewrite`, L5).
`(A, B, …)` types, `(a, b, …)` values, `t.0` (and `t.0.1`, which the lexer reads as a float and the
parser splits), `t.0 = e` on a `let mut`, tuple patterns, `let P = e;` (a `let` whose pattern must
match every value, `N0343` otherwise) and `for (a, b) in e`. A tuple is an anonymous generic record
the checker interns by arity (fields `0`, `1`, …), so layout, release, equality, task crossing and
every backend take it as a record with no new code; an interface writes it as its elements
(`{"tuple":[…]}`). `(x,)` is refused (`N0101`). Semantic epoch 55; EXPERIMENTAL until R1 closes.
Evidence: `crates/nazm-cli/tests/tuples.rs` — a program of every form by hand-worked output in the
interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; refusals by code with
one error each; a tuple across a module's interface under `run` and `build`; the formatter's
spelling.

**R1a (2026-10-12)** · *Patterns* (`spec.md`; `openspec/changes/r1-language-for-the-rewrite`, L3).
`_`, a name, a literal (an integer, perhaps negated, a string, `true`, `false`), a variant pattern
whose fields are patterns, and `|` alternatives — in a `match` of any type a pattern fits, in
`if let`, and in `let … else`, which the parser writes as the `match` each means (a
`MatchSource`). Exhaustiveness and reachability are the standard usefulness algorithm
(`check/usefulness.rs`), naming a value no arm matches; a `match` written variant by variant is
refused with the words it was before R1a. A `match` of plain variant patterns lowers to one Core
IR `match` as before; any other is `{ scrutinee; which arm; that arm }`, each pattern's test a
`Bool` of plain matches and comparisons and each arm's body written once, its bindings read out of
the scrutinee (a branch exhaustiveness rules out is an `exit_with`, which Core IR now counts as a
transfer). `N0629`, `N0630`; semantic epoch 54; EXPERIMENTAL until R1 closes; no warning for a `_`
over variants until L15. Evidence: `crates/nazm-cli/tests/patterns.rs` — every kind of pattern by
hand-worked output in the interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing
live; exhaustiveness witnesses; ten refusals by code; one error, not a cascade, for a wrong
pattern; the syntax, core and service suites unchanged but for three tests whose examples the
language now accepts.

**R1a (2026-10-11)** · *`for` loops* (`spec.md`; `openspec/changes/r1-language-for-the-rewrite`,
D3). `for x in e` over an `Ints`, `Strs`, `Vec[T]` or `Bytes`, and `for x in a..b`; `..` a token, `in`
a word only after a `for` loop's name, and a `for` of any other shape still refused by name
(`N0101`); a non-sequence is `N0628`. Checked once and lowered to Core IR as the `while` loop the
specification gives, so the interpreter, the LIR oracle and both backends run what the `while`
runs. Semantic epoch 52; EXPERIMENTAL in `spec.md`'s classification until R1 closes, so not yet in
the compatibility corpus. The compiler written in Nazm does not carry it (area 31, R2). Evidence:
`crates/nazm-cli/tests/for_loops.rs` — every rule of the section by hand-worked output in the
interpreter, the LIR oracle, LLVM `-O0` and `-O2` and Cranelift, nothing left live in any memory
report; the same program written with the specification's `while` loops printing and reclaiming
identically; ten refusals by code; `in` as a function, parameter and binding name; the formatter
gluing `a..b`; `docs/grammar.ebnf`'s example.

**Gate 2 (2026-10-09)** · *Attributes* and *Conditional compilation* (`general-purpose.md` §16–§17;
`spec.md`). `@name` and `@name(args)` before a top-level declaration, a closed vocabulary refused
by name otherwise (`N0619`); a malformed one is one syntax error. A declaration whose `@cfg` does
not hold is removed before name resolution, an `impl` with its methods, decided by the build's
target (`nazm build --target`), or the host's, and the profiles held; a module that writes `@cfg`
is checked under its configuration and never reused under another. The formatter puts each
attribute on its own line. Semantic epoch 42. *G2-C1:* `@deprecated` is metadata-only deprecation —
on a `pub` function, `struct` or `enum` only (`N0619` elsewhere), published by `nazm doc` as the
item's `deprecated` in `nazm.api-doc/2` and in the Markdown, reported by no diagnostic — epoch 43
(`deprecation_is_metadata_the_api_documentation_publishes`, `schemas.rs`). Evidence: `crates/nazm-cli/tests/attributes.rs` —
the kept declaration in the interpreter and both backends, a cross build that reaches code the
host's check never saw, a check under `--profile` not reusing one without it, fourteen refusals,
one error for a malformed attribute, and the formatter's layout; `docs/grammar.ebnf`'s examples.

**N76 (2026-10-03)** · *Incremental reparse, finer recovery, `\u{…}`* (`architecture.md` §7.77).
`nazm_syntax::Revision` is one file's parse kept per top-level item; `Revision::edit` relexes from
the first lexeme the lexer read past and reparses from the first item that read a changed token,
until both land on old boundaries, keeping the rest (green nodes by reference, items and diagnostics
moved by the edit's length). Its contract is **equality with a fresh parse** — lexemes, every node
and leaf of the concrete tree with kind, span and bytes, the abstract tree with every span, and the
diagnostics in order — held by `crates/nazm-syntax/tests/incremental.rs`: seeded edit sequences over
every `.nz` file in the repository (220 files, 2,604 edits, each compared field by field with
`Revision::new` and `parse_both` of the same text), sixteen edits at the hard places (an item's first
byte, merged tokens, an unclosed delimiter, inside a string and a comment, the whole file), and the
one cross-item read, recovery skipping to the next `fn NAME`, with a test that fails if an item's
lookahead is not recorded. A one-word edit in the middle of `compiler/emit.nz` reparses one of 126
items and 618 of 40,284 tokens, relexes 2 lexemes, and takes 1.2 ms against 8.2 ms for a full parse
in a release build (`performance.md`, *Incremental reparse*).
Recovery reaches inside lists: a malformed record field, enum variant or payload field, parameter,
call argument, field initialiser or `match` arm is an `Error` node of its own beside typed siblings,
and a later independent mistake in the same list is its own diagnostic; a broken `use` ends at its
`;` and recovery stops at `use` (`crates/nazm-syntax/tests/recovery.rs`). The abstract tree's
contract is unchanged on purpose: a declaration that needed recovery is absent, a body that needed
it is. `\u{H…}` is the UTF-8 encoding of a scalar value, refused (`N0004`) for surrogates, values
above `10FFFF` and malformed forms, the same bytes in the interpreter and three native builds
(`usability.rs`); semantic epoch 21. `nazm lsp` synchronises incrementally: the service keeps a
revision per open document, applies each ranged change to it, and analyses the root file from it,
equal to a whole-document change in every published diagnostic and every answer
(`crates/nazm-service/tests/unsaved.rs`, `crates/nazm-cli/tests/lsp.rs`).

**N49 (2026-10-01)** · string escapes (`\n \t \r \0 \\ \" \xHH` up to `7F`, one byte each, decoded
once by the lexer, `N0004` otherwise, kept as written by the formatter), the logical operators
`&&`, `||` and prefix `!` (lowered to `if` in Core IR), and `use "PATH" as NAME;` with `NAME::item`
(the `::` token). `crates/nazm-cli/tests/usability.rs` holds each across the interpreter and three
native builds; `crates/nazm-syntax/src/lexer.rs` its unit tests; the grammar and the guide carry
the rules (`docs/spec.md`, *String escapes*, *Logical operators*, *Qualified names*).

**Evidence** · Hand-written lexer and recursive-descent/Pratt parser,
`crates/nazm-syntax/src/lexer.rs` and `crates/nazm-syntax/src/parser.rs`. The grammar is a
tested artefact, not prose: `crates/nazm-syntax/tests/grammar.rs` requires every lexer
token to appear in `docs/grammar.ebnf`, every nonterminal to be reachable from `program`,
and every `(* @example *)` in it to parse with zero diagnostics. `docs/guide.md`'s syntax
section is generated from the same file by `xtask/src/guide.rs`.

*Since N1, each file is parsed on its own text through `nazm_syntax::parse_map`, so the
parser no longer requires a concatenated buffer. Since N2 the grammar has one visibility
bit — `function = [ visibility ] , "fn" , ...` — and the AST records `pub_span`, which is
where the word was written and nothing about what it means.*

*Since N11, `[` and `]` are tokens and mean one thing: type parameters after a declared name
(`type_parameters`) and type arguments after a type or an expression's name
(`type_arguments`). A `TypeName` is recursive, carries its name's span and its whole span,
and counts against the parse budget; `[]` parses and is the checker's to refuse. The
Nazm-written parser produces the same tree for every generic shape in
`the_nazm_parser_produces_the_same_tree_as_the_reference`, spans included.*

*Since N12, `?` is a token and one postfix operator, parsed beside projection in any order —
`primary = atom , { "." , IDENT | "?" }` — so `-r?` is `-(r?)` and `r??` is two layers. The
formatter glues it to its operand; the Nazm-written parser produces the same trees.*

*Since N15 there is a lossless concrete syntax tree — **the lossless CST is VERIFIED; the
area stays PARTIAL** for the limitations below.* One lossless scan
(`lexer::lex_lossless_in`) gives every byte of a file to exactly one lexeme; the one parser
builds the abstract tree and a `cstree` 0.14 green tree (`crates/nazm-syntax/src/cst.rs`)
from the same decisions, and `parse_cst` returns it. Its leaves, concatenated, are the input
byte for byte — whitespace, comments, punctuation, refused characters, trailing trivia —
and every leaf is the bytes it was scanned from, never a spelling rebuilt from its kind.
Comments are leaves of the tree; the formatter's comment list is derived from the same scan.
Inside a block, a malformed statement becomes an `Error` node and parsing resumes at the next
statement boundary, so what follows it is still structured syntax and a later independent
mistake is its own diagnostic. Evidence in `crates/nazm-syntax/tests/cst.rs`:
`a_lossless_tree_preserves_every_byte_and_recovers_inside_a_function`; every `.nz` file in
the repository and every grammar example round-trips with its byte-ownership invariants;
13 malformed fixtures each keep the construct after the error; seeded random input (3,000
cases) and seeded token deletions of the compiler written in Nazm always round-trip and
terminate; the abstract and concrete trees agree on the span of every construct the checker
sees, over the whole clean corpus. Against the pre-N15 parser, all 196 files give an
identical abstract tree and identical diagnostics, and 4,240 single-token corruptions of
real programs give the same first diagnostic and abstract tree in every case and one more
diagnostic in one (`architecture.md` §7.17). Sixteen N15 mutations were caught with verified
killers; the campaign is in area 30.

**Limitation** · *Narrowed in N76:* until then, full-file parsing only, and recovery at
statements alone. Incremental reparse is per top-level item, so an edit inside one item reparses
that whole item; an edit that leaves a delimiter open reparses to the end of the file; the scan is
copied and its suffix moved on every edit, which is linear in the file's lexemes (`performance.md`).
Imported files in an editor's compilation are parsed in full on every analysis, and every command
outside the language service parses in full. Recovery inside an expression is at list members:
a mistake in an operand that is not a list member (`1 + * 2`) still costs its statement. After an
item's first recovery, if its delimiters do not balance, its further syntax diagnostics are withheld
as cascades. **Deliberate, not debt:** the abstract tree has no body for a function whose body
needed recovery, and no declaration whose header or member list did — the checker is never shown a
declaration with a hole, so recovery adds syntax diagnostics and never semantic cascades; a
hole-tolerant checker is not attempted. The concrete tree is built on every parse and costs about 2×
the parse time and 1.8× the peak parse heap (`performance.md`). The compiler written in Nazm has no
concrete tree, does not recover, and refuses every escape (N98). Parse nesting is bounded at 512 with
a 24 MiB parse stack. `\x` above `7F` stays refused (`spec.md`, *String escapes*).

**Next dependency** · None inside this area's scope. The compiler written in Nazm reached parity on its probes in N102 (area 31).

**Accepted when** · Met, 2026-10-03 (N76), for the reference parser: an edit sequence converges
exactly to a fresh parse in lexemes, both trees and diagnostics, so no stale node or span survives;
the round trip stays lossless and the formatter's tests are unchanged and green; an edit inside one
item reparses that item; recovery reaches every delimited list; the escape policy is decided. Met
before that, 2026-09-25 (N15): a round trip through the tree preserves every byte including trivia,
and the parser produces a usable tree for a file with an error in the middle of a function.

### 2. Name resolution and modules — VERIFIED (N107)

*N107, 2026-10-05: the scope audited (`audit-n107.md`, `architecture.md` §7.108).* Every answer a name has is the same whether a build re-checks a module or reuses it, so the one remaining gap — `nazm build` and `nazm run` re-checking every module — is a cost, and is area 26's limitation now. What VERIFIED means here: one durable identity per definition across modules, packages and re-exports, persisted and read back, in both compilers.

*N103, 2026-10-05: re-exports, one identity.* `architecture.md` §7.104 first. `pub use "PATH";`
offers everything the module at `PATH` offers; `pub use "PATH" { a, b as c };` the names chosen,
`c` the name `b` is offered under. A re-export adds no definition: its export carries the
defining module's identity, so a call, a type, a trait bound, a reference, a rename, a persisted
interface (`nazm.interface/11`) and the API document (`nazm.api-doc/2`) all answer with the
definition. Refused: a name not offered `N0211`, a name offered twice `N0212`, a cycle of
re-exports `N0213`; `pub use … as m`. An impl in a module re-exported from is usable where the
re-exporting module is imported. Across a package boundary too. `nazm interface` publishes from
the stages a compilation runs (`declared_interfaces`), no longer a shorter path of its own. The
compiler written in Nazm reads re-exports and reports `N0211`–`N0213` at the reference's positions.
Held by `crates/nazm-cli/tests/reexports.rs` (nine tests: both backends, refusals, references, the
persisted interface alone, check reuse — a re-exported signature's change re-checks the importer, a
private body's does not — a workspace rename that keeps an alias, the API document, a warm build,
a package) and the selfhost suite. Nine new mutants, all caught. **Stays PARTIAL:** `nazm build`
and `nazm run` still check every module; a warm build reuses objects, not checks.

**N77 (2026-10-03)** · *Resolution, persisted* (`architecture.md` §7.78). A module that checks
cleanly leaves a **resolved unit**, `nazm.resolved/1`: its imports (as written, alias, target
key, target interface hash), its definitions and every resolved occurrence of a declared entity,
each by durable identity (`DefKey`, with a member name for a variant, field or payload field), no
session id, no path, no local. It is written from the checker's own `Resolution` and reference
index (`crates/nazm-iface/src/resolved.rs`) and stored in the module's check entry, now
`nazm.check/3`, under the same key. Its invalidation is therefore the check's, and the table in
§7.78 is held by `crates/nazm-cli/tests/resolved_units.rs`. The same bytes come out of two
processes and two checkouts. A private body change re-checks its module and leaves the importer's
unit byte-identical. An exported change re-checks every direct importer. An alias is part of the
importer's unit. A unit forged to claim other text is never read. `nazm references DEF FILE` reads
reused modules' units instead of checking their bodies: warm, `checked` is 0, and the answer
equals the language service's references from a fresh in-process check, the independent oracle.
`nazm resolve FILE` prints the units. Both schemas are validated against real output
(`schemas.rs`); a missing, foreign or mis-versioned unit is a miss (`crates/nazm-cache/tests/store.rs`).
The compiler written in Nazm's loader, `compiler/module.nz`, refuses `@std/` and `NAME:` imports by
name (`selfhost.rs`), where it used to report a missing file; it resolves relative paths only.

**Evidence** · Lexical scoping with shadowing rules and a separate function namespace,
`crates/nazm-core/src/check/`. `use "relative/path.nz";` resolved by the driver,
`crates/nazm-service/src/load.rs` — canonical-path keyed, read-once, depth-first, cycles
terminate rather than erroring. A second resolver written in Nazm, `compiler/module.nz`,
serves the bootstrap stages. Codes `N0200`–`N0210` in `crates/nazm-diag/src/lib.rs`.

**N49 (2026-10-01)** · qualified names: an import under a name keeps its module's exports out of
the unqualified namespace and holds them as `alias::name` in the same environments, so `NAME::item`
resolves to exactly the definition an unqualified import would, at the item's own span — one
resolver. The graph's edge records whether a plain `use` named the module and each alias.
`N0209` (an alias that already means something), `N0210` (a qualified name that names nothing).
Evidence: `crates/nazm-cli/tests/usability.rs` (two modules exporting one name used side by side,
which was `N0207`; a module imported both ways; `@std`; types, constructions, variants and match
arms), `crates/nazm-service/tests/qualified.rs` (definition and references at the item's bytes).
The compiler written in Nazm refuses `::` and `as` by name.

**Resolution is singular as of N1 (2026-09-21).** `nazm_core::check` produces a
`nazm_sema::Resolution` — function definitions, call targets, and the slot every name
mention refers to — and both the evaluator and `crates/nazm-lir/src/lower.rs` read it.
Since N2 that is true of the **evaluator** as well: its `HashMap<String, usize>` over every
function, and its own copy of the built-in-first rule, are gone. The backend's last two
name lookups went with them — `crates/nazm-lir/src/lower.rs` pairs definitions with
declarations through `FnDef::decl` and finds the entry point through `Resolution::entry`,
so neither consumer knows the spelling `main`.

**Source identity is real as of N1.** `nazm_span::Span` is `{ file, start, end }` with
file-local offsets, and `crates/nazm-service/src/load.rs` produces a `SourceMap` whose files
are parsed separately. The merged buffer, and the binary search that recovered a file from
an offset, are gone.

**There is a compilation unit as of N2 (2026-09-22).** One module is one file.
`crates/nazm-sema/src/module.rs` holds `ModuleId` and the `ModuleGraph` the driver builds
while loading; `crates/nazm-sema/src/interface.rs` holds the `UnitInterface` a module
offers. Checking is two stages, `declare_unit` then `check_unit`
(`crates/nazm-core/src/check/`), and the second takes its dependencies as **interfaces**.
`crates/nazm-core/tests/unit_checking.rs` checks a module against an interface for a module
whose source does not exist, and refuses the same module when that interface is empty.

**Visibility exists as of N2.** Top-level definitions are private to their module; `pub`
exports; imports are not transitive; every collision is a diagnostic naming both sides
(`N0206`–`N0208`). Twenty cases in `crates/nazm-cli/tests/visibility.rs`, most of them run
interpreted and compiled and required to agree. The self-hosted compiler was migrated
rather than exempted: 84 of its 224 functions are `pub`, 140 are private, and since N2.1 it enforces the rule as well as expressing it.

**A toolchain-owned module exists as of N12 (2026-09-24).** The core prelude,
`library/core/prelude.nz`, is loaded into every compilation after its project modules and
imported by every one of them with no `use` (`ModuleGraph::attach_prelude`). Its key,
`@core/prelude`, is fixed by the toolchain and disjoint from every project key; its two type
names are reserved (`N0363`). It is a dependency like any import, so it is in every module's
check key. It is **not** a package system.

**Durable identity and persisted interfaces exist as of N3 (2026-09-22).** A module's
`ModuleKey` is its normalised path relative to the compilation's source root, and a
definition's `DefKey` is that key plus its kind and name
(`crates/nazm-sema/src/key.rs`). `nazm interface FILE` writes a module's interface as
`nazm.interface/3` — durable keys, canonical type names, exported **record and enum
definitions** closed over their field and payload types, no spans, no session ids, no
discriminants (`crates/nazm-iface/`) — and `--hash` fingerprints it with BLAKE3. It was
`/1` until N9 and `/2` until N10; a `/1` file describes a language with no records and a
`/2` one a language with no enums, so reading either as a `/3` would be believing a module
offers less than it does, and every direction is refused rather than reinterpreted. The session ids are
unchanged and were not renamed; both layers coexist.

`crates/nazm-cli/tests/durable_identity.rs` establishes it across real boundaries: the same
project gives identical bytes in two processes and in two different directories, a module
whose private body changed gives the same fingerprint, and **a dependent checks against a
deserialised interface with the dependency's source deleted.** A module outside the source
root, or one a symbolic link gave two names, is refused a durable key rather than given a
guess.

**A project can declare where it begins, as of N4 (2026-09-22).** An empty `nazm.root`
marker, or `--source-root DIR`, fixes the root a module's key is relative to, so
`lib/common.nz` is the same module whichever entry file named it
(`crates/nazm-service/src/root.rs`, ten cases in `crates/nazm-cli/tests/source_root.rs`). With
neither, the root is derived from the named file exactly as before. The marker must be
empty and is refused otherwise: it says where a project begins and owns nothing else.

**Limitation** · A module is named by a relative path, `@std/NAME` or a package's `NAME:path`; the
compiler written in Nazm resolves the first two (N102) and refuses the third by name. A resolved unit carries no types and no slots, so `nazm build` and
`nazm run` still check every module (area 2a); locals have no persisted resolution; the language
service never reads or writes a unit (an unsaved buffer is not cache state). *The sentence that
stood here — that nothing is separately compiled — was true until N5 and is now area 10a.*

**Next dependency** · Check reuse for `nazm build` and `nazm run`: a persisted unit that carries
types and slots, so a warm build lowers a module it did not re-check.

**Accepted when** · Met for identity, interfaces and a declared root.

### 2a. Incremental semantic checking — VERIFIED

**Evidence** · `nazm check` reuses the result of checking a module whose semantic inputs
are unchanged, as of N4 (2026-09-22). `crates/nazm-cache/` computes what those inputs are —
`SourceFingerprint` over the exact source bytes, `CheckerIdentity` over the checker's own
tables plus an explicit `SEMANTIC_EPOCH`, and `CheckKey` over both plus each direct
import's `(ModuleKey, InterfaceHash)` — and stores a `nazm.check/1` entry per module in
`.nazm/check/`, published by `rename` and validated on read.
`crates/nazm-core/src/check/` gained one hook, `Reuse`; `check_unit` is still the only
thing that decides what a body means.

Twenty-nine tests in `crates/nazm-cli/tests/incremental.rs`, each running the binary
repeatedly, establish the model: a private body change re-checks its own module and leaves
its importer alone; an exported signature change re-checks the importer and finds the error
it now has; in `A → B → C`, a change to C that leaves B's interface unchanged stops at B.
Sixteen more in `crates/nazm-cache/tests/store.rs` break one thing about a stored entry —
truncation, a future schema, an edited fingerprint, an edited interface, a valid entry
under the wrong name — and every one is a miss.

**What is reused, exactly** · The second stage of checking a module: its bodies. Every
module is still read, parsed and declared from source on every run, which is why a skipped
module is invisible — the interface other modules' keys depend on, and whether the
compilation has a `main`, are recomputed rather than recalled.

**Limitation** · **This is not incremental native compilation, and no such claim is made.**
`nazm run` and `nazm build` reuse *this* cache not at all: both need the typed resolution that
only checking a body produces. Since N77 a module's *names* persist, as its resolved unit
(area 2), but its types and slots do not, and lowering needs them. (`nazm build` reuses objects as
of N6 — a different cache, a different key, area 10b — and a build that reused every object
still checked every module.) There is no IR or LIR cache, no query database, no scheduler and no
eviction policy. A module with no durable identity is checked every time.

**And the measured gain is about a millisecond.** On the compiler's own five modules,
`performance.md` records 40.5 ms with the cache disabled against 38.9 ms warm — 18% of the
work above a 31.7 ms process floor, and 3.9% of the wall clock. The first run is **24 ms
slower**, because publishing five entries costs an `fsync` each. The decisions are right;
the saving is small because parsing and declaring happen every run and are most of the
cost. Recorded rather than averaged away: a reader deciding whether to depend on this
should know it buys correctness of invalidation, not speed.

**Next dependency** · *Met in part by N77:* name resolution now has a written form, the resolved
unit, and `nazm references` reads it instead of checking a reused module's bodies. A build that
skips bodies needs typed Core IR to be persisted too, which a resolved unit deliberately is not. Not a `CodegenKey`: N6 found that native reuse
needed no semantic key at all (area 10b), so the dependency this row once named is gone
rather than met.

**Accepted when** · Met for semantic checking. Native reuse is a separate row that does not
exist.

### 3. Type system — VERIFIED as scoped (N107, Gate 2)

**Gate 2 (2026-10-08)** · *Bytes* and *Text* (`spec.md`; `general-purpose.md` §5, §6). `Bytes`,
a counted view of a fixed-length buffer: `bytes_new`, `bytes_from_str`, `bytes_get`/`_set`,
`bytes_slice` sharing the buffer, `bytes_copy` (overlap-safe), and reads and writes of every
numeric type in either byte order. `Text`, UTF-8 by construction: literals where a `Text` is
expected, `Text(s)`, `try_convert[Text](s)`, `utf8_valid`, slicing at boundaries, scalar
iteration, `scalar_text`. `crates/nazm-cli/tests/bytes_text.rs` — eight tests, each program run
under the interpreter, LLVM `-O0` and `-O2` and Cranelift: views sharing writes, every width and
byte order, RFC 3629's validation table (27 sequences) agreeing with the host, scalar iteration
and boundaries, every trap (`N0405`, `N0413`) with its sentence, every refusal (`N0003`, `N0203`,
`N0300`, `N0304`, `N0321`, `N0331`, `N0621`), reclamation counted equal (16 of 16) in all three,
and both types across a public signature, cold and cached. Not crossing the C boundary yet, and
refused by name by the EVM and accelerator-kernel targets, as every built-in is.

**Gate 2 (2026-10-08)** · *Arrays* and *Constants* (`spec.md`; `general-purpose.md` §4, §15).
`[T; N]` of plain data — numbers, `Bool`, and records, enums and arrays of them — at most 64 KiB;
`[a, b]`, `[value; count]`, a checked `xs[i]` and `xs[i] = v` through any path of fields and
indexes; copied as a value, compared element by element, held inline in records, vectors and
tasks; and `const NAME: T = value;`, computed by the checker with the same numeric semantics,
module-private. `crates/nazm-cli/tests/arrays.rs` — seven tests, each program run under the
interpreter, LLVM `-O0` and `-O2` and Cranelift: building, copying and writing in place, arrays in
records and records in arrays, a copy crossing into a task, every index (read, write, negative,
nested) stopping with `N0405`, constants, every refusal (`N0003`, `N0101`, `N0300`, `N0618`,
`N0621`–`N0624`) with the 64 KiB bound exact, and an array in a public signature across modules,
cold and from the cached interface. Compatibility corpus: one run case, one trap case, six
refusals. Ten mutants (`g2-…array…`, `g2-…constant…`). Not crossing the C boundary, not in the EVM
or accelerator-kernel targets (refused by name), and no slices or views — that is §5's.

**Gate 2 (2026-10-07)** · *Numbers* (`spec.md`, *Numbers*; `general-purpose.md` §1–§3). `Int8`,
`Int16`, `Int32`, `UInt8`, `UInt16`, `UInt32`, `UInt64`, `Float32` and `Float64` beside `Int`;
float, hexadecimal, octal and binary literals typed by the type expected; `&`, `|`, `^`, `~`, `<<`,
`>>`; `T(x)`, `try_convert`, `wrap_convert`, `saturate_convert`; 43 numeric built-ins, generic over
a class of types. One semantics, `nazm_sema::numeric`, is what the interpreter and the LIR oracle
run; both native backends are held to it by `crates/nazm-cli/tests/numbers.rs` — eleven tests,
each run under the interpreter, LLVM `-O0` and `-O2` and Cranelift: shortest-digit float text,
IEEE comparison with NaN, every width's range and signedness, wrapping and saturation, shifts,
rotations and counts, every conversion mode, numbers in records, vectors and tasks, every trap
(`N0400`, `N0401`, `N0412`) with its code, every refusal (`N0003`, `N0300`, `N0304`, `N0354`,
`N0617`), and the C library's functions to six places. Compatibility corpus: one run case, two
trap cases, three refusals, and the superseded `1.5` refusal now held to checking. Not crossing
the C boundary yet, and not in the EVM or accelerator-kernel targets, which refuse them by name.

*N107, 2026-10-05: the scope audited (`audit-n107.md`, `architecture.md` §7.108).* The scope is the type system the spec states — records, closed enums, exhaustive variant match, first-order generics with `Vec[T]`, typed `Result` and `?`, structural equality and equality bounds, function values and closures, traits with impls, methods and bounds, dispatched statically. Trait objects, generic traits, associated types, default methods and supertraits are not required by any program or stated goal (dynamic dispatch exists through function values and enums, and the goals ask that it be exposed and restricted, not added); each stays refused by name (`N0602`–`N0608`). Checking against a persisted interface alone, without source, cannot use traits: a tooling boundary (area 26), not a wrong answer — the cached check path handles traits.

**N78 (2026-10-03)** · *Traits and methods* (`architecture.md` §7.79, `spec.md`, *Traits and
methods*). `trait Name { fn m(self: Self, …) -> R; }`, `impl Name for Type { … }`, `recv.m(…)` and
`Trait.m(recv, …)`, and a type parameter's bound naming a trait (`[T: Show]`). Coherence holds over
the whole compilation: at most one impl of a trait for a type (`N0600`), in the trait's module or
the type's (`N0601`). An impl defines exactly its trait's methods with its signatures and effects
(`N0602`, `N0603`), for a non-generic record or enum, a numeric type, `Bool` or `Str` (`N0604`). Method
lookup is over the traits in scope and the impls of this module and its direct imports, which keeps
a cached check sound (`N0605`, `N0606`). There are no trait objects (`N0607`), and methods are not
values (`N0608`). A concrete call is resolved before Core IR to the impl's ordinary function; a call
through a bound is Core IR's one new form, `CallTrait`, which MIR resolves per instance and the
interpreter per value. The same program prints the same in the interpreter, LLVM at -O0 and -O2,
and Cranelift, with every allocation reclaimed (`crates/nazm-cli/tests/traits.rs`, 17 tests: impls
on a record, an enum and a built-in, calls through two levels of generic code, cross-module and
aliased traits, `@std/show`'s `show_all` over a `Vec` of a program's records, and each refusal by
code). `@std/show` is the first standard trait. Traits, trait methods and calls through bounds are
reference targets (`nazm resolve`, `nazm references`, the language service; semantic tokens
`interface` and `method`). The persisted interface carries exported traits, every impl and every
bound (`nazm.interface/8`), and adding an impl moves its fingerprint. Semantic epoch 22; Core IR
print `nazm.core-ir/2`.

**N50 (2026-10-02)** · function values and closures, `architecture.md` §7.52 and `docs/spec.md`,
*Function values and closures*. `Type::Fn`, interned by parameters, result and effects
(structural, invariant, no equality, never into a task); a named, monomorphic Nazm function as a
value; `fn(x: T) -> R ! {e} { … }` closures capturing by copy; calls through a binding of function
type. `crates/nazm-cli/tests/closures.rs` holds each across the interpreter, LLVM −O0/−O2 and
Cranelift with the memory report compared line for line, and every refusal (`N0386`, `N0387`, and
the existing `N0300`, `N0303`, `N0304`, `N0310`, `N0321`, `N0366`, `N0369`, `N0372` with their
meanings unchanged). Traits and methods: **DESIGNED**, deferred — §7.52 records why and the
questions they would settle. Closures and function values inside generic functions: refused.


**Evidence** · Six built-in types, `crates/nazm-sema/src/types.rs`: `Int`, `Bool`, `Str`,
`Ints`, `Strs`, `Chan` — **and, since N9, user-defined records; since N10, user-defined
enums**. Local inference for `let`; parameters and returns always written. Completion
analysis decides whether a block produces a value, which is what lets one `if` serve both
statement and value position. Codes `N0300`–`N0314` and `N0330`–`N0345`. 114 behavioural
tests in `crates/nazm-core/tests/language.rs`, 33 in `crates/nazm-core/tests/records.rs`,
47 in `crates/nazm-core/tests/enums.rs`.

*N9, 2026-09-23.* `struct Point { x: Int, y: Int, }` — nominal value records with named
fields, named construction, projection, field replacement through a projection path,
nesting, and separate type and function namespaces. The type enum stopped being closed:
`Type::Record(RecordId)` names a definition rather than a shape, because two records with
identical fields are different types. Every property a record has — cleanup, task safety,
copy, destroy — is **derived from its fields**, recursively, and that composition rather
than the syntax is what the milestone was for. `docs/spec.md`, *Records*, is the law;
`architecture.md` §7.10 is the implementation.

*N10, 2026-09-23.* `enum State { Ready, Done(code: Int), }` with `match` — closed nominal
sum types with named payload fields, qualified construction, exhaustive variant matching
with named payload bindings, and composition with records in both directions. The
milestone's question was not whether enums compile but whether the ownership model holds
when **which fields exist is a runtime fact**: copy and release act on the active variant
alone, derived and dispatched in the emitted program, while task safety stays a property of
the **type** and quantifies over every variant. `docs/spec.md`, *Enums*, is the law;
`architecture.md` §7.11 is the implementation.

*N11, 2026-09-23.* First-order parametric generics — `struct Pair[A, B]`, `enum Maybe[T]`,
`fn identity[T]` — checked **once, parametrically**, with call-site type arguments written
or inferred from the arguments and never from the expected result, and one generic
container, `Vec[T]`. An applied type is an interned table entry with substituted fields, so
every derivation above holds after substitution unchanged; parameters are positions, so
renaming one changes no interface byte. Codes `N0350`–`N0359`. 40 tests in
`crates/nazm-core/tests/generics.rs` with code and span, 7 native ones in
`crates/nazm-cli/tests/generics.rs`, and `the_nazm_checker_agrees_with_the_reference_on_generics`
comparing both checkers on 42 programs by code **and** span. `docs/spec.md`, *Generics*, is
the law; `architecture.md` §7.12 is the implementation.

*N12, 2026-09-24.* **Typed error values.** `Result[T, E]` and `Option[T]` are ordinary
generic enums declared by the core prelude, and `?` propagates the core `Result` — recognised
by definition, never by name or shape — with the exact rule `expr: Result[T, E]` in a function
returning `Result[U, E]` gives `expr?: T`, the identical `E` required. No exceptions, no
unwinding, no conversion between error types, no `?` on `Option`. Codes `N0360`–`N0363`. 33
tests in `crates/nazm-core/tests/results.rs` with code and span, 12 native memory cases in
`crates/nazm-cli/tests/propagation.rs`, 10 module, interface and cache cases in
`crates/nazm-cli/tests/prelude.rs`, and
`the_nazm_checker_agrees_with_the_reference_on_propagation` comparing both checkers on 22
programs by code **and** span. `docs/spec.md`, *Typed error values*, is the law;
`architecture.md` §7.13 is the implementation.

*N12.1, 2026-09-24, changed no rule here.* A block used as a value — a `match` arm, a `let`
initialiser, an argument, a field, an operand — was always part of the checked language; it
now also compiles natively in both compilers, which is area 9's and area 10's row rather
than this one's. What this row gains is the first `Option` doing real work inside the
compiler: `record_owned` returns `Option[Int]` instead of a `-1`.

*N13, 2026-09-25.* **Derived structural equality.** `==` and `!=` apply to two operands of
one type when that type has equality, and for a user type it is derived, never declared:
a record has it exactly when every field does, an enum exactly when every payload field of
**every** variant does, and a concrete generic instance after substitution — so
`Option[Int]` and `Result[Int, Str]` have it as ordinary enums, with no rule naming them.
One derivation, `UserTypes::equality` in `crates/nazm-sema/src/udt.rs`, beside cleanup and
task safety and independent of both; the checker asks it and nothing else, and its refusal
names the blocking component (`Outer`'s `inner.values`). No code was added — the rule moved
under `N0304` — so `SEMANTIC_EPOCH` went 5 → 6; the interface schema is unchanged at
`nazm.interface/4`. 20 tests in `crates/nazm-core/tests/equality.rs` with code and span, 11
native ones in `crates/nazm-cli/tests/equality.rs` at `-O0` and `-O2` with every memory
counter, `the_nazm_checker_agrees_with_the_reference_on_equality` comparing both checkers on
30 programs by code **and** span, and the interface-only case with the dependency deleted.
`docs/spec.md`, *Equality is derived*, is the law; `architecture.md` §7.15 is the
implementation.

| | equality |
| --- | --- |
| `Int`, `Bool`, `Str` (primitive) | VERIFIED, unchanged — `Str` by its bytes |
| record, derived | VERIFIED |
| enum, derived over every variant | VERIFIED |
| concrete generic instance, after substitution | VERIFIED |
| `Option[T]` / `Result[T, E]`, inherited as ordinary enums | VERIFIED |
| `Vec[T]`, `Ints`, `Strs` — sequence equality | absent, deliberately |
| `Chan` | absent, deliberately |
| equality over a constrained type parameter | absent at N13 — there was no constraint mechanism; N14 added one (below) |
| custom (user-defined) equality | absent |

*N14, 2026-09-25.* **A type parameter may require equality.** `fn same[T: Equality](a: T,
b: T) -> Bool { a == b }` is accepted, the same without `: Equality` is still refused, and
every argument — written or inferred, concrete or a caller's own parameter — is checked
against the requirement by N13's one derivation, which now reads a required parameter as
having equality. So `Box[T]`, `Option[T]` and `Result[T, E]` with both required compare
inside a bounded body, `Vec[Int]` and `Option[Vec[Int]]` never satisfy it, and an
unconstrained wrapper cannot forward to a bounded callee. One compiler-owned capability,
`Equality`, on function parameters only; nothing is emitted at run time. Codes `N0364`,
`N0365`; `SEMANTIC_EPOCH` 6 → 7 and the interface schema `/4` → `/5`, which persists each
export's required positions. 14 tests in `crates/nazm-core/tests/constraints.rs` with code
and span, 3 native ones in `crates/nazm-cli/tests/constraints.rs`, interface-only and cache
cases in `durable_identity.rs` and `incremental.rs`, and
`the_nazm_checker_agrees_with_the_reference_on_requirements` comparing both checkers on 23
programs by code **and** span. `docs/spec.md`, *A type parameter may require equality*, is
the law; `architecture.md` §7.16 is the implementation.

| | status |
| --- | --- |
| first-order generics | VERIFIED |
| a generic parameter that requires the built-in equality capability | VERIFIED |
| a requirement on a record's or an enum's parameter | refused (`N0101`), deferred |
| any other capability, user-defined traits, `impl`, methods | absent (traits and methods DESIGNED, §7.52) |
| function types, named functions as values, closures, indirect calls (N50) | VERIFIED (`crates/nazm-cli/tests/closures.rs`) |
| custom equality, sequence or channel equality, ordering, hashing | absent |
| higher kinds, const generics, specialisation | absent |

Seven constructs are verified rather than partial: **records**, **closed enums**,
**exhaustive variant match**, **first-order generics with `Vec[T]`**, **typed `Result`
propagation**, **derived structural equality** and **equality-requiring type parameters**.
What surrounds them is not verified, and none of it is a trait system. Typed errors are **not** effects: a `Result` in a
signature is a return type, and area 5 is unchanged.

**Limitation** · Generics remain narrowly constrained: N14 adds exactly one compiler-owned
requirement, `Equality`, on a function's type parameters, and that is not a trait system.
*Narrowed in N78:* until then there were no user-defined traits, no `impl` and no methods. What
remains is deliberate, and each is refused by name: no trait objects or dynamic dispatch, no
default methods, no associated types or constants, no generic traits or methods, no supertraits,
no impls with bounds or for generic types, no inherent impls without a trait, no methods as values,
one requirement per type parameter. Calling a field of function type as `r.f(1)` stays refused.
Generic function values (`N0387`) are outside the supported model: each would need a thunk per
instance, and a bound covers the programs that wanted one. A closure, and a named function's value,
inside a generic function are accepted since R1b (L10). No
higher kinds, const generics, defaults, variance or specialisation. A persisted interface's traits
are not rehydrated, so a module checked against one (rather than its source) cannot use them. The
compiler written in Nazm has traits, closures and function values since N102. A requirement cannot be written on a record's or an enum's parameter (`N0101`).
`Option` has no `?`, `Result` no
methods and no conversion between error types, and `main` cannot return one; no
indexing syntax on a `Vec`; no `spawn` of a generic function. Function values and closures
exist (N50), inside a generic function too since R1b (L10); no unit type, no `Never`.
Since N78 a generic definition can require a trait of `T`, and a program can declare traits of its
own; nothing can say "`T` can be passed to a task". No ordering, hashing or custom equality on any user type, and no
equality at all on a sequence, a `Vec` or a `Chan`. No field-level or per-variant visibility, no
default field values, no record update syntax. The pattern language is variant selection
with named payload bindings and `_` — no top-level wildcard arm, no guards, no or-patterns,
no literal, record, tuple or range patterns, and no `let` destructuring. Four words — `for`,
`label`, `loop` and `mod` — remain reserved and refused by name rather than silently absent
(`crates/nazm-syntax/src/parser.rs`); `struct` left that list in N9, `enum` and `match`
in N10 and `impl` in N78, for the same reason `print` did in B3 — a reserved word earns its place by naming
something absent. *Until N11 this said neither a record nor an enum could be put in a
sequence; `Vec[T]` holds either.*

**Next dependency** · None forced by a program yet. N14's requirement covered the need N13
exposed with one built-in capability, and its dogfood found no compiler code that wanted
another (`research-register.md`, N14). *Until N14 this named constrained generics, which N14
added for equality; until N13 equality on user types; until N12 typed errors; until N12.1
blocks as values in the native backends.*

**Accepted when** · Chosen by the milestone that needs it, not by completeness.

---

## Semantic core

### 4. Memory model and reclamation — VERIFIED for the current type universe, closure environments included (Gate 1-C1)

**R1a (2026-10-12)** · *Immutable indirection: `Box[T]` and `List[T]`* (`spec.md`;
`openspec/changes/r1-language-for-the-rewrite`, D1/L1). A definition may reach itself through a
`Box` or `List`: the ownership graph labels those edges immutable and refuses only a cycle with a
counted edge on it (`N0359`), with the reclamation argument in the specification. Both are a
`Vec[T]` underneath, nothing writes after they are made, and they are distinct types to the
checker only: Core IR lowering writes `box_new` and `list_from_vec` with three internal built-ins
no program can name, and the interpreter, the LIR oracle and LIR carry each operation out as the
`Vec` one it means. Release is a worklist, not recursion, in all three: the interpreter's
sequence storage hands its elements to a per-thread queue, and the runtime's `nz.vec_release`
takes the queue of the release in progress — runtime ABI 29. Prelude names: a module's own `Box`,
`List` or `box_new` keeps its meaning. Semantic epoch 53; EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/box_list.rs` — trees of boxes and lists by hand-worked output under the
interpreter, the LIR oracle, LLVM `-O0`/`-O2` and Cranelift with nothing live; a million-deep `Box`
chain built and released by the interpreter, LLVM and Cranelift with every sequence reclaimed
(before the queue the interpreter overflowed its stack and a compiled program died releasing it);
the ownership rule's acceptances and refusals; no equality, no task, no bare `Box`; a closure
capturing a `List` of function values accepted and a `Vec` of them refused (`N0616`); the prelude
names; provenance through both; a recursive type crossing a module's interface.

**v1 (2026-10-11, Q1-M-01, Q1-M-02)** · The type universe this row's VERIFIED covers is now written
down whole: `spec.md`'s four kinds list Gate 2's numbers and arrays (immediates), `Text` (a shared
immutable value), `Vec[T]` and `Bytes` (owned mutable handles; a buffer lives while any view does)
and `Chan[T]`, and *What is not promised* names them. `Bytes` and `Text` reclamation is held by two
programs: `every_buffer_is_reclaimed_by_its_last_view_everywhere` (views outliving their buffer's
first holder, under scribbled freed memory) and
`a_buffer_and_a_text_are_reclaimed_in_every_shape_on_every_path` — a buffer and a text in a record
field, a `Vec`, a closure's environment and a `Result` passed on by `?`, on the normal end and on a
trap, every count balanced, under the interpreter and both backends.

**Gate 1-C1 (2026-10-07)** · *Status restored from PARTIAL to VERIFIED by a rule and its evidence,
not by a narrower claim.* A closure's environment is a node of the ownership graph
(`docs/architecture.md` §7.111, `docs/spec.md` *Cycles*): a closure may not capture a value whose
type can hold a function value behind a counted handle (`N0616`), decided from the type alone, so
an alias, a record field, a `Vec` of records, an enum or `Option` payload, nested vectors and a
container made in another function are covered without alias analysis. An environment is
immutable and points only at older values, the only edges into an existing value are stores into
counted handles, and a handle that cannot hold a function value holds no environment — so storing
a function value anywhere, through any parameter, is never refused, and no accepted program forms
a cycle. Evidence:
`a_closure_capturing_a_container_of_function_values_is_refused_in_every_shape` (direct, alias,
mutual, nested, record, records in a vector, enum, `Option`, `Vec[Vec[fn]]`, made in a helper —
each `N0616`, and `nazm run` refuses);
`acyclic_closures_in_containers_are_accepted_and_reclaimed_everywhere` (named functions and
closures capturing values, vectors and other closures, stored in vectors, records and through an
opaque parameter and a generic function: the same output and `live=0` for sequences, strings,
channels and closures in the interpreter, LLVM `-O0` and `-O2` and Cranelift);
`the_rule_holds_where_a_closure_is_written_in_any_module`; five refusal cases and one run case in
the compatibility corpus; the compiler written in Nazm refuses the same programs at the same span
(bootstrap: 30 refusal cases). Semantic epoch 30; `nazm.interface/11` and runtime ABI 14 unchanged.
There is no collector, and none is needed for a program the checker accepts. The rule is
conservative: a closure that only reads a vector of handlers is refused too, and is written by
passing the vector as an argument. The Gate 1 entry below is the historical record of the defect.

*Gate 1 → Gate 1-C1, historical:* the heading read "PARTIAL: verified for the current type
universe, one recorded cycle defect (Gate 1)".

**Gate 1 (2026-10-07), historical — closed by Gate 1-C1 above** · *Status moved from VERIFIED to PARTIAL by a defect, not by a goal.* A
closure that captures a `Vec` able to hold function values and is then stored into that `Vec` is
a value that owns itself. The checker accepts it, and counting does not reclaim it: built natively,
`let fs = vec_new[fn() -> Int](); let f = fn() -> Int { vec_len(fs) }; vec_push(fs, f);` ends
with `nazm-memory: sequences allocated=1 reclaimed=0 live=1 … closures allocated=1 reclaimed=0
live=1`, where the same program with the closure capturing another vector reclaims everything.
N50 added the value kind *Cycles* said must arrive with a policy, and supplied none; the *Next
dependency* below already named "a closure capturing its own handle" as the case to settle, and it
was not settled. `docs/spec.md` *Cycles* carries the correction; the reclamation claim holds for
programs that form no such cycle. **Accepted when** the cycle is refused by the type system (as
`N0359` refuses ownership cycles in definitions) or reclaimed by a stated policy, with a test
either way — a decision for review, not taken in Gate 1.


**N52 (2026-10-02)** · resource sites are facts: `nazm explain-cost` (`nazm.cost/1`) lists each
function's allocation, retain, block, channel, foreign and task sites from MIR with a count per
call — `at-most-once`, or `unknown` inside a loop, never a number the compiler does not know
(`explain_cost_reports_each_site_with_a_count_per_call`). Not a byte count and not a measurement.


**N50 (2026-10-02)** · a function value is a counted closure object; what it captured is released
when its last reference goes, on a normal exit, a `return`, a loop's next iteration and a failure
inside the closure's own code (`a_failure_inside_a_closure_reclaims_its_captures_and_its_callers`,
`an_early_return_and_a_loop_release_the_closures_they_made`). The memory report counts closure
objects as a fourth class, in the interpreter and in both backends' runtime alike.


*This row was "Ownership and resource semantics — MISSING" until N7 (2026-09-22), and
PARTIAL between N7 and N8 (the same day). The paragraph it used to carry is kept below,
because what it said was true and the numbers in it are what the work was measured against.*

**Read the status carefully.** VERIFIED here means: **every heap-backed type the language
currently has is reclaimed, in both compiler implementations and in the reference
interpreter, and that is tested rather than asserted.** It does **not** mean whole-language
leak freedom, and it says nothing about types that do not exist.

*N9, 2026-09-23, extended the type universe and the claim with it.* Records are covered:
a record owns exactly what its fields own, recursively, and copy, destroy, cleanup and
task safety are all **derived** rather than declared. The cycle argument survives because
`docs/spec.md`'s *Cycles* was satisfied the first way it offered — the type system refuses
containment cycles (`N0336`), because a record holds its fields by value and a cycle has
no finite layout.

*N10, 2026-09-23, extended it to variants.* An enum owns exactly what its **active**
variant's payload owns: copy, destroy and cleanup dispatch on the discriminant in the
emitted program, and an inactive variant is ownership-inert. A payload is held inline like
a field, so the same refusal (`N0336`) covers a containment cycle through a variant, across
modules too.

*N11, 2026-09-23, extended it to generics and `Vec[T]`.* An applied record or enum owns
exactly what its substituted fields own; a `Vec[T]` is a reference-counted handle whose
elements are copied in by `vec_push` and `vec_set`, copied out by `vec_get`, transferred
out by `vec_pop`, and released by `T`'s law when the last reference dies. A `Vec` is the
first way a definition can reach itself with a finite layout, so the argument needed its
second satisfaction, and got it the first way again: every definition whose values could
own themselves through a counted handle is refused (`N0359`), over an ownership graph that
keeps acyclic nesting and phantom parameters legal.

*N12, 2026-09-24, needed no extension.* `Result` and `Option` are ordinary enums, so they
reclaim by the enum law, and `?` is a `match` with a `return`: its success payload takes its
own reference before the temporary `Result` is released, and its error payload is owned by
the returned `Result` before anything it came out of is. Every shape — `Ok` and `Err` of
`Int`, `Str`, a record, an enum, a `Vec` and a `Chan`; `None` and `Some` of four kinds — and
every way out — a half-evaluated call, a half-built record or variant, a scope with a running
task, three functions deep — reports `live=0` in both implementations, under allocator churn,
in `crates/nazm-cli/tests/propagation.rs`.

**Evidence** · `docs/spec.md`'s *memory constitution* settles assignment, parameters,
returns, aliasing, mutation, reclamation, cross-task transfer, cycles and cost visibility
as four kinds of value and five rules. `docs/architecture.md` §7.7 carries the audit it was
derived from, §7.8 the string and channel model, §7.9 the one classification cleanup
dispatches on. No program that was legal before changed its meaning.

| Type | Storage | Status |
| --- | --- | --- |
| `Int`, `Bool` | none | — |
| `Str` | a backing allocation, named by the value's third field; null for a literal or an `arg` | **VERIFIED** |
| `Ints`, `Strs` | a reference-counted header; a `Strs` releases every element's backing first | **VERIFIED** |
| `Chan` | a reference-counted header, with its ring, mutex and condition variable | **VERIFIED** |
| a **record** | none of its own — an inline aggregate whose fields own what they own | **VERIFIED**, composed: one generated retain and release helper per record that owns anything, and none at all for one that does not |
| an **enum** | none of its own — an inline discriminant and payload slots; only the active variant's payload owns anything | **VERIFIED** since N10: one generated helper pair per enum that owns anything, dispatching on the discriminant |
| an applied **record** or **enum** | as its definition's, with the substituted field types | **VERIFIED** since N11: its own helpers, generated from the substituted fields |
| `Vec[T]` | the sequence header an `Ints` has, holding elements of `T`'s layout | **VERIFIED** since N11: one release helper per owning element type, releasing each element before the storage; a `Vec` of non-owning elements is released as an `Ints` is |

*N68, 2026-10-02: layout specialisation — VERIFIED for one layout, opt-in.* `architecture.md`
§7.70 and research register R4 (with its prior art) first. `--layout soa` stores a `Vec[R]` of a
record of `Int`s and `Bool`s one array per field (`nz.soa_grow`; runtime ABI 10). Evidence,
`crates/nazm-cli/tests/layout.rs` (6): the same answers and the same `N0405` failures as the
interpreter and the element-by-element layout at `-O0` and `-O2`, through growth past 1,000
elements, set, pop, get; reordered fields giving identical code; the differing unit recompiled
for the other layout and every unit reused for the same; an owning element staying whole with
its reason; refusals. Measured (`performance.md`, N68): a one-field scan of 1,000,000 eight-field
records 2.8× faster than element by element and equal to hand-written per-field `Ints`; reading
all eight fields 2× slower. **Limitation**: opt-in, every eligible type at once; no cost model,
no owning elements, no enums, no hybrid blocking; LLVM only.
| a `spawn`'s argument block, a file path's terminated copy | runtime-internal | **VERIFIED** — freed since N8; each was one `malloc` per operation before |
| a literal's constant, the process's `argv` | not this program's | **not reclaimed, correctly.** Nothing allocated them and nothing may free them |

**How.** Reference counting, which `docs/spec.md` deliberately does **not** make the law —
the rule is that storage stays valid while a reference exists, and an arena, an interning
pass or an escape analysis would satisfy it. The count is non-atomic on a sequence because
`N0321` refuses to let one cross into a task, and atomic on a string backing and a channel
because both do cross. Cleanup is on the three exit edges of a generated function — the
tail, each `return`, and the shared unwind block a failure takes — with parameter slots
excluded, which *is* rule 2.

**Tested.** 68 cases in `crates/nazm-cli/tests/memory.rs`, each run twice and each requiring
the interpreter and the compiled program to agree on the value **and** on what they
allocated and reclaimed, by class: slices outliving their buffers, slices of slices, empty
strings, borrowed strings returned on both edges, strings stored in and read out of
sequences, a string read out of a **temporary** sequence, self-assignment of an element,
embedded NUL bytes, a process argument, the failure path, channel scaling, a returned
channel, blocked senders and receivers woken by a close, a task that fails while holding a
channel, and a string and a channel crossing into two tasks at once.

*The 27 record cases added by N9* cover: a scalar record allocating nothing, a heap-string
field, a sliced-string field, `Ints` and `Strs` and `Chan` fields, a nested owning record,
a copy taking one reference per owning field, whole-record replacement including
self-assignment, field replacement including one backed by the same allocation, tail and
explicit return, a projection out of a temporary, a failed constructor at one and two
levels, a failing replacement expression, a task-safe record crossing into two siblings, a
record built in one branch of an `if`, four hundred short-lived records against a constant
live set, and three cases where a failure leaves while a temporary is still in hand.

*The 14 enum cases added by N10* cover: an inactive owning variant owning nothing, a
zero-payload variant beside owning ones, a copy retaining the active payload once, a
payload returned out of an arm (and still valid after the allocator reused the block), a
discarded payload, replacement across variants, a failed payload initialiser, a failure
inside an arm, a matched sequence handle, a constant live set of short-lived enums, an enum
inside a record and a record inside an enum, a nested enum, and an enum crossing into two
tasks. The Nazm-written compiler is held to the same counts by four enum programs in
`the_nazm_written_compiler_emits_the_same_memory_semantics`.

*The 12 generic cases added by N11* cover: `Vec[Int]` under the `Ints` law, `Vec[Str]`
owning each string, a record read out of a `Vec` surviving its slot being overwritten, a
`Vec` of enums releasing only each element's active payload, a nested `Vec` releasing each
inner vector and what it holds, a `Vec[Chan]` holding a reference to each channel, a `Vec`
returned by tail and by `return`, a generic function returning each kind of value by its own
law, a generic record releasing each field by its substituted type, an element replaced by a
copy of itself surviving the allocator, a failed `Vec` operation giving back what the frame
held, and many short-lived vectors keeping a constant live set. The Nazm-written compiler is
held to the same counts by five generic programs in the same test.

**Two of those were written because a mutation survived**, and the pair is the lesson.
Removing `strs_get`'s own reference broke nothing the suite could see, because a binding
retains what it stores whether or not the built-in already did — the shape that finds it is
a sequence with **no** binding. And releasing a replaced field before taking the new value
broke nothing the *counters* could see, because both orders end with the same two numbers —
the shape that finds it is `h.name = h.name` followed by enough small allocations that the
block the wrong order freed is handed back out, after which the program stops with a
signal. A compensating mechanism makes a defect invisible without making it absent, and
the compensation is sometimes the allocator.

**Measured, N11** (contained, Linux aarch64). 5,000 / 20,000 / 80,000 short-lived `Vec[Str]`,
`Vec[Row]`, `Vec[Tok]` and `Vec[Vec[Str]]` against a constant live set all peak at **1.17 MiB**,
the empty program's floor, with nothing live at exit. `Vec[Int]` and `Vec[Str]` cost what
`Ints` and `Strs` cost. `performance.md` has the rows, and the enum-in-a-`Vec` pressure: 48
bytes a token against 40 for parallel arrays, +18% peak.

**Measured, N9.** 80,000 short-lived two-string records against a constant live set:
**1.72 MiB**, against 1.73 MiB for the same two strings without a record, and 1.69 MiB for
an empty program. The N8 controls are unmoved — 80,000 temporary strings 1.77 MiB, 80,000
short-lived sequences 1.78 MiB, 2,000 short-lived channels 1.75 MiB. In time, at `-O2`, a
record is **not measurable**: three million two-field constructions and reads take 34 ms
against 34 ms for the same two integers through bindings, and a million record copies with
an owning field take 54 ms against 55 ms for copying that field directly. `performance.md`
carries the `-O0` figures, where the helpers are real calls, and the executable-size delta
(**+96 bytes** for a record that owns something, zero for one that does not).

**Measured, N8.** 5,000 / 20,000 / 80,000 temporary heap strings against a constant live set:
1.88 / 2.33 / 4.17 MiB → **1.73 / 1.77 / 1.77 MiB**. The accumulator shape from the
2026-09-21 incident, at 8,000 iterations: 70.66 MiB → **2.23 MiB**. 2,000 short-lived
channels: 2.38 MiB → **1.80 MiB**. Sequences, a genuinely live 2,000,000-element sequence
and a sieve are unchanged, which is the control. `performance.md` has the runtime cost:
about 13–16 ns per string operation, and nothing measurable anywhere else.

**Self-hosted parity — VERIFIED**, records included. `compiler/emit.nz` emits the same model. Its runtime
text is **derived** from `crates/nazm-runtime/src/core.rs` rather than transcribed, and
`cargo xtask check`'s `runtime parity` gate re-derives it and refuses a difference; its
*decisions* are held by `the_nazm_written_compiler_emits_the_same_memory_semantics`, which
builds eight programs with both compilers and requires the same reclamation report from
each. Between N7 and N8 the two disagreed for a whole milestone and every behavioural test
passed throughout, which is what the gate exists to prevent.

**Completion is part of it since N10.1.** A `break`, `continue` or `return` inside a value
position — an argument, an operand, an initialiser, a condition, a scrutinee, an arm, the
operand of `return` — produces no value on that path in either compiler, and
`a_transfer_in_a_value_position_is_never_a_value` holds it: 32 programs in
`crates/nazm-cli/tests/transfers/`, each run by the interpreter, the reference backend, and
the Nazm-written compiler compiled **and** interpreted, whose IR must be byte-identical,
against a value written by hand and a reclamation report that must match and be zero live.
Before it the compiler written in Nazm passed `void` operands for the shape
`pick(int_to_str(i), if i == 1 { continue; } else { i })` and failed inside itself for a
construct whose branches all left. `architecture.md` §7.11 has the mechanism.

**And one leak the reference shared.** `==` on two strings never gave its operands back, in
both native backends, with no transfer involved — `int_to_str(i) == "5"` allocated a string
per evaluation and reclaimed none. The interpreter was right. Fixed in both, held by
`a_string_comparison_gives_back_its_operands`.

**And for refused programs, since N10.2.** The Nazm checker's completion is the
reference's three states — a value, no value, never finishing — so a value position given
nothing is `N0300`, an `if` whose branches disagree is `N0302`, and a body or a `return` is
held to its signature. `the_nazm_checker_agrees_with_the_reference_on_what_produces_a_value`
compares **code and span** with the reference over 40 programs in
`crates/nazm-cli/tests/refusals/`, with the Nazm checker both compiled and interpreted, and
requires the Nazm compiler to refuse each one before emission; one more pair crosses a
module boundary with two records laid out alike. What is claimed is diagnostic *codes and
primary spans*, not message text, and not the diagnostics that follow an earlier error in
the same expression, where the two checkers recover differently (`bootstrap.md`).

**Not claimed.** Whole-language leak freedom as a permanent property; anything about a
kind of value the language does not have — a weak reference, a closure, a trait object;
deadlock freedom; user-visible destruction, RAII or any ordering a program can
observe; a clean sanitizer run. `exit_with` reclaims nothing and reports nothing, which is
correct and is tested so nobody later treats it as a bug.

**Limitation, in the words this row carried before N7** · *"Compiled programs allocate and
never release: `@free` appears zero times in [the Rust emitter, `emit.rs`, retired by N105] and zero times
in `compiler/emit.nz`, against 11 `@malloc` and 2 `@realloc` sites in each."* Both halves
are false now, and the second was still true between N7 and N8. **Whole-language leak
freedom is still NOT VERIFIED and is still not claimed** — the sentence above about types
that do not exist is the reason, and it is not a formality.

**Next dependency** · None for the current type universe. *Until N11 this named a generic
container, which would make a cycle expressible again; N11 added one and refused the cycles
(`N0359`).* The next value kind that could form a cycle — a closure capturing its own
handle, a weak or shared reference — has to satisfy `docs/spec.md`'s *Cycles* a third time.

**Accepted when** · A compiled program's peak RSS is bounded by live data rather than by
total allocation, demonstrated on the self-hosting compiler compiling itself. **Met since
N8**, and unmoved by N9: contained on Linux aarch64, the compiler written in Nazm compiling
`compiler/emit.nz` peaks at 32.75 MiB against a 3,756-line input, and 80,000 short-lived
records with heap fields peak at the empty-program floor.

### 5. Effects — VERIFIED as scoped (N107)

*N107, 2026-10-05: the scope audited (`audit-n107.md`, `architecture.md` §7.108).* The scope is the compiler-owned effects (three; `spec.md` *Effects*), inferred and checked, with declared sets as contracts and purity assertable. Handlers and user-defined effects are algebraic effects, a control mechanism; who may act is capabilities' (area 6), so neither is required. **Pure is not total:** a pure function may diverge; whether divergence is an effect stays an open question (`research-register.md`), and nothing relies on the answer.

*N36, 2026-09-30: Typed Effects v1 — VERIFIED; Effect propagation v1 — VERIFIED; Effect-bearing
interfaces v1 — VERIFIED; Purity checking v1 — VERIFIED. The row is PARTIAL: handlers, effect
polymorphism, user-defined effects and divergence are not here.* Two compiler-owned effects,
`io` (the eight outside-world built-ins) and `spawn` (a `spawn` statement), each with a stable id
and classified by the built-in's identity in an exhaustive `match` (`crates/nazm-sema/src/
effect.rs`); sets are a bit per id, canonical by construction. A function may declare `! { … }`;
every function's effects are inferred from its resolved calls to the least fixed point over the
module, cycles included, with a shortest witness (`crates/nazm-core/src/effects.rs`); a declared
set is a contract (`N0366`), and unknown or repeated names are `N0367` and `N0368`. Across modules
only the declared set travels, in `nazm.interface/6`; an undeclared import is every effect to its
importer. `SEMANTIC_EPOCH` 8. Tools: signatures, hover, `nazm.context/2`, `nazm.snapshot/2`,
`nazm.delta/2` (`architecture.md` §7.38, `spec.md` *Effects*). Evidence: `crates/nazm-core/tests/
effects.rs` — ordinary computation (records, enums, generics, equality, sequences, channels,
loops, `Result` and `?`) pure and assertable; each of the eight built-ins refused in a pure
function; a `spawn` and its task's effects; order and repetition changing nothing, a wider
contract allowed and read by callers; a purity assertion failing through four calls with the
shortest witness; direct, mutual and generic recursion at the least fixed point, in any order;
unknown and repeated names refused once with no cascade and an exact fix; the effect namespace;
a broken body adding nothing; determinism and the recorded witness; a 2,000-long chain, a
300-wide fan-out and a 500-long cycle settled in one test; nothing changing what runs.
`crates/nazm-iface/tests/persistence.rs` — a declared set published by name in id order, absent
for none, read back exactly, malformed ones refused, `/5` refused. `crates/nazm-cli/tests/
incremental.rs` — an effect-only change rechecking the importer and finding what it breaks, a
body change within its contract reaching nobody, an undeclared import every effect to a pure
caller, the cached interface carrying the declared set. `crates/nazm-cli/tests/effects.rs` — the
same IR with and without declarations, and every `examples/effects/` program agreeing interpreted
and compiled. `crates/nazm-service/tests/context.rs`, `snapshot.rs` and `effects.rs` — packet
effects with reasons, an effect-only change as its own snapshot section, and every program of the
tree still checking. Seventeen N36 mutations (area 30).

**Limitation** · Three effects only (`io`, `spawn`, `foreign`), and a function that declares nothing is, to another module,
every effect — so in the multi-module compiler sources 152 of 1,044 function checks come out
`{ io, spawn }`, most because they call an undeclared import, not because they print or spawn.
Pure means *no effect*, not *referentially transparent*: a function may write into a sequence it
was handed. Effect polymorphism is one effect parameter per function (N51), which the
higher-order library uses; no handlers and no user-defined effects; allocation, channels, traps
and divergence are not effects. The compiler written in Nazm reads effect sets and checks a
function value's against the expected type's (N102). Effects are not authority: since N37 that is area 6, checked apart. Typed diagnostic facts (`nazm.diagnostic-detail/1`)
stay unavailable; the effect facts are in the packet.

**Next dependency** · Handlers; more than one effect parameter, and effect subtyping, when a
program needs them. Authority is area 6.

*N51, 2026-10-02: Effect polymorphism v1 — VERIFIED.* One effect parameter per function
(`effects E`), opaque in the body, bound at each call from the function-typed arguments, the
callee's declared set performed with it replaced; through a wrapper (`@std/seq`), a module and its
cached interface, and visible to profiles. `crates/nazm-cli/tests/effect_params.rs`, interpreter
and both backends; `N0388`, `N0389`.

*N79, 2026-10-03: Effect subsumption v1 — VERIFIED.* Where a function value is passed, spawned
with or returned, one whose effects are a subset of the expected type's is accepted; parameters and
results stay invariant, an effect parameter matches only by binding, and nothing converts inside
another type (`architecture.md` §7.80, `spec.md` *Function values*). The checker's one relation is
`assignable` (`crates/nazm-core/src/check/call.rs`); below it, Core IR's verifier compares results up to
effects as it already compared arguments, so the interpreter and both native backends run a
subsumed value unchanged. Evidence: `crates/nazm-cli/tests/authority.rs` —
`a_function_value_with_fewer_effects_is_accepted_passed_or_returned` (interpreter and Cranelift),
`a_function_value_with_more_effects_is_refused` (passed and returned, `N0300`). The row stays
PARTIAL for handlers, user-defined effects and divergence.

*N50, 2026-10-02.* A function type carries an effect set; a call through a function value performs
it, a closure's body is checked against its own set (`! {}` unwritten), and an undeclared function
used as a value is held to the none its value's type assumed (`N0366`). A closure's authority is
its own capability parameters (`N0369`). `a_function_value_carries_its_effects_and_takes_its_authority_as_parameters`.

*Before N36 this row was MISSING. N12 did not change it.* A function's error type is visible because it is inside its
return type, `Result[T, E]`; a caller may store, pass or ignore that value, and nothing about
it is tracked as an effect.

**Evidence of absence** · No effect row, annotation, inference or purity analysis. The
only occurrences of the word in the language crates are the English phrase "evaluated for
its effects" and `crates/nazm-lir/src/lib.rs`'s statement that there are *"no effects to
track"* — given as a reason there is one IR level rather than eight.

**Limitation** · The blocking question is open and named: **is divergence an effect?**
Without an answer, "drop an unused pure call" is unsound, so the optimiser cannot be given
that transformation at all. `docs/spec.md` *Open* carries it, along with whether allocation
failure and panic are effects.

**Next dependency** · An answer to whether divergence is an effect. *Until R1 this named a typed core IR to carry a row on a function type: Core IR (N39) and effect-carrying function types (N50) met it.*

**Accepted when** · A function's row is inferred, checked, and a compile-fail test shows an
effectful call refused where a pure one is accepted.

### 6. Capabilities, as a language feature — VERIFIED as scoped (N107: static, coarse, shareable)

*N107, 2026-10-05: the scope audited (`audit-n107.md`, `architecture.md` §7.108).* The promise is the static one the checker keeps: no authority without being handed it, `main` the only root, `OutCap` narrower than `IoCap`. Runtime enforcement, revocation, finer kinds and linear capabilities are not required — finer authority is a library built from today's kinds — and the row's name now says the scope.

*N104, 2026-10-05: no inherited authority — VERIFIED.* `architecture.md` §7.105 first. The
bridge's authority half is removed: every body, declared or not, holds what it exercises
(`N0369` otherwise), a call needs nothing of its caller, and `main`'s parameters are the only
root. `Resolution::inherits` and `nazm inspect`'s `inherits` are gone (`nazm.inspect/2`); the
`explicit-authority` rule can no longer fire. Every program in the repository was migrated — eleven
`.nz` programs and examples, the compiler written in Nazm (30 of its functions take an `IoCap`,
none a `SpawnCap`), and the programs inside the tests — by threading from `main` the capabilities
each function's own body uses. The migration found two N79 defects in attenuation, both fixed and
held by `authority::an_io_cap_given_for_an_out_cap_compiles_on_both_backends`: an `IoCap` handed
for an `OutCap` failed MIR validation (`N0900`), because MIR's copy of the shape relation had no
capability rule; and a generic function could not be handed one (`N0300`), because a generic
call compared its concrete parameters exactly. The compiler written in Nazm also checks it: `N0369` at the
reference's spans (six new agreement cases). Semantic epoch 29. Nine catalogue entries that
attacked the bridge are retired, three repointed, six added. **Stays PARTIAL** for the reasons
below: static only, coarse, shareable.

*N79, 2026-10-03: Inherited authority as a recorded contract v1 — VERIFIED; Strict authority by
profile v1 — VERIFIED; Attenuation by a narrower kind v1 — VERIFIED; Revocation — declared outside
the model.* The checker computes each function's **inherited authority** — the kinds its body
exercises without holding them — by the rule that checks a declared function, and records it in
the resolution (`Resolution::inherits`, `crates/nazm-core/src/effects.rs`); `nazm inspect` shows it
as each definition's `inherits`. The `authority` profile's `explicit-authority` rule refuses every
function of the program's own modules that inherits, by name and kind (`N0510`). `OutCap` (id 6)
authorises `print` and `eprint` only; an `IoCap` implies it and is accepted where one is expected,
never the reverse, and `main` may take one as a root (`crates/nazm-sema/src/capability.rs`,
`CapabilitySet::implied`). Evidence: `crates/nazm-cli/tests/authority.rs` — an `OutCap` printing and
refused a file read, a write and an existence check (`N0369`); `IoCap` for `OutCap` accepted and the
reverse refused, as argument and result; `main(o: OutCap)` interpreted and native; `OutCap` not
redefinable (`N0331`); the recorded `inherits` of undeclared, pure and declared functions; the
profile refusing inheritance and accepting held authority and pure undeclared code; the profile
report naming the rule. **Still PARTIAL**, for one reason: by default an undeclared function still
inherits its caller's authority — the corpus and the compiler written in Nazm rely on it, and
migrating both is named work. The interface stays `nazm.interface/8`: inherited authority depends
on bodies, and an interface publishes declarations; a declared export inherits nothing, and an
undeclared import is already every effect to its importer (§7.80, as built).

**N51 (2026-10-02)** · a closure holds the capabilities visible where it is written, as captured,
and its type states what it does with them (`a_closure_holds_the_capabilities_it_captures`);
making a value of an undeclared function needs the authority its type's effects need, so declared
code cannot reach the N37 bridge through a value
(`a_value_of_an_undeclared_import_needs_the_authority_a_call_would`). Attenuated capabilities and
revocation: **DESIGNED**, deferred (`architecture.md` §7.53 states the missing use case and the
runtime cost). The trust root is unchanged.


*N37, 2026-09-30: Compiler-owned capability kinds v1 — VERIFIED; Explicit authority passing v1 —
VERIFIED; Effect-capability checking v1 — VERIFIED; Cross-module authority contracts v1 —
VERIFIED; Capability unforgeability v1 — VERIFIED. Attenuation, resource-specific authority,
revocation, linearity and runtime enforcement are not here.* Two compiler-owned kinds with stable
ids, `IoCap` and `SpawnCap` — three since N42 added `ForeignCap`, whose call-site check has no
compatibility bridge (area 17) — as built-in types (`crates/nazm-sema/src/capability.rs`); each
built-in's required authority is an exhaustive table on its identity. A function that declares an
effect set holds exactly the capability values its scope reaches — lexically, recorded per call
site by the checker — and every built-in, `spawn` and call of an undeclared function needs the
authority for what it does, or it is `N0369`, reported apart from and before an effect's `N0366`
(`crates/nazm-core/src/effects.rs`). Nothing constructs a capability (`N0370`); `main` is the root
and takes only capabilities (`N0371`), which the interpreter and the native entry hand it. Authority
crosses a module as ordinary parameter types in `nazm.interface/6` (`architecture.md` §7.39,
`spec.md` *Capabilities*). Evidence: `crates/nazm-core/tests/capabilities.rs` — passing through
helpers, recursion and mutual recursion, storage in a record, a `Vec` and a variant, a pure function
carrying one; every outside-world built-in refused without authority and accepted with it; effect
versus authority errors apart and in order; spawn needing a `SpawnCap`, and a task's authority only
what it is handed; the witness through undeclared functions; shadowing, records, type parameters,
block scope, the wrong kind and a missing argument; construction and redefinition refused, no
equality; `main`'s roots. `crates/nazm-core/tests/effects.rs` — N36's laws unchanged with
capabilities held. `crates/nazm-cli/tests/incremental.rs` — a capability-only change, including a
kind swap under one name, rechecking the importer; an undeclared import needing every authority;
the cached interface carrying `IoCap`. `crates/nazm-cli/tests/capabilities.rs` — a capability
parameter lowering to exactly an `Int` one's IR, the entry passing one word per root, a non-capability
`main` refused before building, and `examples/capabilities/` agreeing interpreted and compiled.
`crates/nazm-service/tests/capabilities.rs` and `snapshot.rs` — hover, completion (a type, never a
constructor), semantic tokens, and a capability-only change as a `shape` change under the same
identity. Twenty N37 mutations (area 30).

**Limitation** · **Static only**: a capability erases to a word nothing reads, and nothing at runtime
enforces authority — there is no sandbox, broker or operating-system boundary, and a Nazm program can
affect its machine as much as the process running it. **Coarse**: one `IoCap` authorises every file,
both standard streams, the arguments and exit; nothing is least-privilege, and there is no
attenuation beyond `OutCap` (N79) because there is no other narrower kind to derive. **Shareable**: capabilities copy freely,
with no linearity and no revocation. **No bridge since N104.** Possession is lexical, not use: the built-ins
do not take the capability as an argument.

**Do not confuse this with `nazm capabilities`.** `schema/nazm.capabilities-1.json` is a
*toolchain descriptor* — what one build of the compiler supports. It is unrelated to
object capabilities or capability-safety, and it is VERIFIED under area 22.

**Next dependency** · For finer attenuation, a
resource-parametrised kind; for linearity, move semantics;
for enforcement beyond the checker, a runtime or operating-system boundary; for foreign code, FFI.

*Before N37 this row was MISSING.* **Evidence of absence, then** · Authority was **ambient and
labelled as such**: `crates/nazm-sema/src/intrinsic.rs` said *"These are callable from anywhere, and
nothing in the language restricts which functions may reach the file system or the process
arguments. That is a real limitation, not a design."* `fn main(io: IO)` had been written down as the
plan of record and then not adopted. N37 adopted a capability parameter on `main`, for the reason
`spec.md` now states: it is the only place a value can come from that no expression produced.

**Accepted when** · A function without the authority cannot reach the filesystem, and the refusal is
a compile error with a span — met for every function that declares its effects, and not for an
ambient one, which is the bridge above.

### 7. Provenance and information flow, as a language feature — VERIFIED as scoped (N107: explicit data flow)

*N107, 2026-10-05: the scope audited (`audit-n107.md`, `architecture.md` §7.108).* The promise is explicit data flow: a `Secret` that reaches an output through data is refused, by cell, target and field, through calls, values, bounds and channels. Implicit flow — non-interference — was never claimed and is RESEARCH (`research-register.md` R6); labels a program writes and per-handle precision would only refine it.

*N80, 2026-10-03: Container cells v1 — VERIFIED; Indirect targets v1 — VERIFIED; Local field and
variant shapes v1 — VERIFIED; Flow policy engine v1 — VERIFIED; Origin-to-sink traces v1 —
VERIFIED.* `architecture.md` §7.81 is the constitution, `spec.md` *Containers* and *Calls through
values and bounds* the law. Every container is read and written through a **cell** named by its
type, so a read carries what any container of that type was given — exhaustively, by the built-in's
identity (`Origin::container`, `crates/nazm-sema/src/provenance.rs`) — and generic code is every
cell; a receive and a select move a channel's cell, and a select's index carries nothing. A call
through a function value reaches every function value of its type up to effects, a call through a
bound every impl of its method; a closure's parameters and captures are received like any
parameter. Within a body a record or variant keeps its fields apart. `N0372` and the profile flow
rules are one engine (`FlowRule`), and `cyber` gains `checked-paths`, the language's rule in every
function. Each sink carries one chain per origin, printed by `nazm explain-flow` as `nazm.flow/2`.
Facts persist in `nazm.check/4`; epoch 24. Evidence: `crates/nazm-cli/tests/flow_v3.rs` — eleven
end-to-end tests, each in both directions, covering every container form, receive and select,
generic code, named values and closures and a pure value standing for an effectful one,
parameters and captures, fields, variants, a field write and `?`, bounds, an export to C,
`checked-paths`, the trace and its determinism, and an affected program run interpreted and natively;
`crates/nazm-core/tests/provenance.rs` (containers by type, fields, the trace through a cell);
`crates/nazm-cli/tests/incremental.rs` — a cell and a function value from a module the cache reused;
`closures.rs`, `cost_and_flow.rs`, `profiles.rs`. **Falsifier, run:** `nazm check` of N79 and N80
over all 237 `.nz` files in the repository gave identical codes on every one — no new refusal and no
other change (`architecture.md` §7.81, as built). **Still PARTIAL**, for named reasons: implicit
flow is not tracked, so this is not non-interference; precision is by type, not by handle, index
or across a call; there are no labels a program writes. Cost: `nazm check` of `compiler/emit.nz`
1.125× (`performance.md`).

**N52 (2026-10-02)** · a sink registry (`SinkClass`: `path`, `output`, `file-data`, and `vouch`
for the evidence of a declassification), each solved through helpers' parameters, closures and
modules with the origins that reach it (`every_sink_class_is_solved_through_helpers_closures_and_containers`);
`nazm explain-flow` prints them as `nazm.flow/1`. Declassification is `str_vouch` under a
`VouchCap` held anywhere, recorded with the origins it cleared
(`a_vouch_needs_its_authority_everywhere_and_is_recorded`). Profiles: `cyber`'s
`contents-stay-in-files`, `critical`'s `no-declassification`. Containers remain whole-container
conservative, stated. A 2,000-function chain solves deterministically within a deadline
(`a_long_call_chain_is_solved_deterministically_and_in_bounded_time`).


**N50 (2026-10-02)** · a closure's body is reduced to flows as a function of its own; its
parameters are of unknown origin, its captures carry what the captured binding carried (unknown
when that depended on its maker's parameters or calls), and the result of an indirect call is of
unknown origin — conservative, so a restricted sink reached through a function value is refused
(`a_path_through_a_function_value_is_of_unknown_origin`).

*N38, 2026-09-30: Provenance identity v1 — VERIFIED; Explicit data-flow propagation v1 — VERIFIED;
Function summary propagation v1 — VERIFIED; Cross-module provenance summaries v1 — VERIFIED;
Restricted-flow checking v1 — VERIFIED. Implicit flow, field and element precision, user labels,
declassification and runtime tracking are not here.* Four compiler-owned origins with stable ids —
`argument`, `file`, `authority`, `unknown` — entered only by the built-ins that bring data in (an
exhaustive table on their identity) and by `main`'s capabilities (`crates/nazm-sema/src/provenance.rs`).
Each module's checked bodies are reduced to flows over parameters and calls, and the whole program's
summaries — origins a result carries, parameters passed on, parameters reaching a sink, and what each
parameter is handed — are solved by worklist to the least fixed point on every run
(`crates/nazm-core/src/provenance.rs`). Facts cross modules in the check entry (`nazm.check/2`), by
durable name, so a reused module still contributes them and no conclusion is cached. One restricted
flow: in a function with a declared effect set, `write_file`'s path may not derive from a file's
contents or from shared storage the checker does not follow, directly or through any function whose
parameter reaches it (`N0372`, one witness chain). Tools: `nazm.context/3`, `nazm.snapshot/3`,
`nazm.delta/3` (`architecture.md` §7.40, `spec.md` *Provenance*). Evidence:
`crates/nazm-core/tests/provenance.rs` — the sources and that output is not input; joins through
operators, built-ins, records, variants, `Result`, `?` and loops, in any order; pass-through, derived
and constant-returning summaries (a constant is never tainted); conditions contributing nothing;
recursion, mutual recursion and reversed source order; shared storage and channels `unknown`;
authority handed through passing, records and a `spawn`; provenance adding no effect and granting no
authority; the restricted path refused with its witness, through declared, undeclared, nested and
spawned helpers, and allowed from the command line or the program; the codes' order; a 2,000-long
chain and a 500-long cycle settling. `crates/nazm-cli/tests/incremental.rs` — a callee's body change
reaching a caller the cache reused, both ways, and every module reused; a helper in another module
carrying the rule; an entry keeping the facts. `crates/nazm-service/tests/snapshot.rs` and
`context.rs` — a provenance-only change as its own section where no byte of the function moved, and
the packet's summary. `crates/nazm-cli/tests/provenance.rs` — `examples/provenance/` agreeing
interpreted and compiled, and a refused flow never built. Twenty-five N38 mutations (area 30).

**Limitation** · **Explicit data flow only**: a value is not tainted by the condition that chose it,
so this is not non-interference, and timing and every side channel are outside it. **Coarse**: records,
variants and `Result` are tracked whole, and every read of a sequence, `Vec` or channel is `unknown`,
because aliasing hides their writers — conservative, and over-restrictive for a path built in one. One
sink, `write_file`'s path; no user labels, sanitisers, declassification or policy. **The bridge**: a
function with no declared effect set is not checked itself, though explicit code cannot launder
through it. Authority has no restriction of its own, since types already keep it out of every output.
Static only: nothing is tracked at runtime. Checking `compiler/emit.nz` costs about 10 % more.

**Next dependency** · For implicit flow, a control-dependence model; for precision, a MIR that tracks
places; for more sinks and declassification, a policy design; for foreign code, FFI summaries.

*Before N38 this row was MISSING.* **Evidence of absence, then** · No labels, lattice, sanitizers or
declassification anywhere in the language crates. `architecture.md` §5 designs a lattice-valued
qualifier (`Untrusted`, `Secret`, `PII`) and records that an earlier draft calling provenance
*"nearly free once effects exist"* was overclaiming; N38 is not that qualifier.

**Accepted when** · A `Secret` reaching an output is refused, with a stated and documented
approximation of what the analysis misses — met in kind for one origin and one sink (a file's contents
at `write_file`'s path), with the approximation above; `Secret`, as a label a program writes, does not
exist.

---

## IR and backend

### 8. Core IR — VERIFIED (v1)

**Evidence** · N39 (2026-09-30). `crates/nazm-cir` is the representation, its invariants
(`verify.rs`), its printed form `nazm.core-ir/2` since N78, `/1` before (`print.rs`, `nazm core-ir`) and each
function's digests (`digest.rs`); `crates/nazm-core/src/lower.rs` is **the one lowering** from a
checked program, private to `nazm-core` and reached only after checking succeeded;
`crates/nazm-core/src/eval/` runs Core IR and nothing else; `crates/nazm-lir/src/lower.rs`
lowers Core IR, and `nazm-lir` no longer depends on `nazm-syntax`. `architecture.md` §7.41 is the
constitution, written before the code.

| Claim | Status | Evidence |
| --- | --- | --- |
| Core IR exists, and is the one checked-to-IR lowering | VERIFIED v1 | both backends consume it; `xtask/src/rules.rs` places `nazm-cir` and forbids `nazm-lir` the syntax tree; `crates/nazm-core/tests/core_ir.rs` (a rejected program never reaches a consumer) |
| Typed Core IR | VERIFIED | every value carries the checker's type or *never*; `cir::verify` runs on every lowering; `every_value_is_typed_by_what_the_checker_established` |
| Canonical semantic lowering | VERIFIED | all 205 `.nz` sources: `nazm check`, `nazm run` (value, output, memory report, status) and every one of 383 emitted LLVM IR files byte-identical before and after (`performance.md`, N39) |
| Control-flow normalisation | VERIFIED | explicit `LoopId` targets, explicit completion, value and statement `if` apart, `?` a `match` whose error arm returns; `every_break_and_continue_names_the_loop_it_leaves`, `a_question_mark_is_a_match_whose_error_arm_returns` |
| Backend-neutral semantic operations | VERIFIED | `Intrinsic` by identity, `Law` on every comparison, records, variants and fields by identity, no layout, symbol or runtime call; the `core ir boundary` gate (`cargo xtask check`, and `nothing_below_core_ir_asks_what_a_source_position_meant` in the suite) |
| Deterministic, durable printed form | VERIFIED | `the_printed_core_ir_is_the_same_bytes_in_every_process` (`crates/nazm-cli/tests/core_ir.rs`) |
| Per-function digests, semantic and body | VERIFIED | the digest-law tests in `crates/nazm-core/tests/core_ir.rs`: a body change moves both, an effect change only the semantic one, a capability parameter both, provenance neither; a span, a local's name, a declaration's order, an arm's order and an unrelated definition none |
| Digests used as a cache key | MISSING | nothing keys on them yet; the object cache keys on emitted IR (area 10b) |

**Limitation** · Level 3 by responsibility, not by `architecture.md` §2's shape: a tree of
structured regions, not ANF. Core IR is built from each run's
full check and never persisted, so no reuse happens at this level. Generic bodies stay generic;
instantiation is the native backend's. Ownership operations are not in it — regions state what
they own, and a MIR would lower from that.

**Accepted when** · Introducing the level **removed** code: the evaluator's name-keyed frames and
per-step resolution lookups, and the native lowering's span lookups and its own `?` lowering, are
gone, and a mechanical gate keeps them gone.

### 9. MIR — VERIFIED (v1)

**Evidence** · N40 (2026-10-01). `crates/nazm-mir` is the representation (`ir.rs`), **the one
lowering** from Core IR (`lower.rs`) with the instance planner moved into it (`instance.rs`), the
validator (`verify.rs`), the printed form `nazm.mir/1` (`print.rs`, `nazm mir`) and each function's
executable digest (`digest.rs`). `nazm build` lowers Core IR to MIR, validates it — a rejection
after a clean check is `N0900` — and hands the native backend MIR and nothing of the checker.
`architecture.md` §7.42 is the constitution, written before the code.

| Claim | Status | Evidence |
| --- | --- | --- |
| MIR exists, and is the one Core IR → native lowering | VERIFIED v1 | `nazm build` goes through `crates/nazm-cli/src/mir.rs::lower`; `nazm-lir` depends on `nazm-mir` and not on `nazm-cir` (`xtask/src/rules.rs`), and the core ir boundary gate refuses `Resolution` or Core IR in it |
| Explicit, deterministic CFG | VERIFIED | basic blocks, one terminator each; blocks, locals and temporaries numbered by a deterministic walk; `lowering_twice_gives_the_same_mir`, `the_printed_mir_is_the_same_bytes_in_every_process` |
| Typed MIR | VERIFIED | concrete types on every local; the validator checks every operation's operand and result types, call and primitive signatures, field and payload identities; `the_validator_refuses_a_local_of_the_wrong_type`, `…_a_call_with_the_wrong_arguments`, `…_a_primitive_with_a_bad_signature` |
| Cleanup obligations explicit and mechanically validated | VERIFIED | `copy`, `move`, `drop`, scope joins and failure edges are statements; an ownership dataflow refuses a use of an empty local, an overwrite of an owned one and anything still owned at `return` or `unwind`; `the_validator_refuses_a_leak_at_return`, `…_a_use_after_move`, `…_a_scope_left_without_its_join`, `…_dropping_a_parameter` |
| Completion, `?`, records, enums and `match` without backend re-analysis | VERIFIED | `?` is a `switch` whose error arm returns; a `switch` names each variant once, and a payload read is valid only in its arm (`the_validator_refuses_a_payload_read_outside_its_arm`, `…_a_switch_that_misses_a_variant`) |
| Finite, typed runtime primitives | VERIFIED | `Intrinsic` by identity with its element type; scope open, spawn, join and the failure check as MIR statements |
| Behaviour preserved | VERIFIED | all 205 `.nz` sources before and after N40: `nazm check` and `nazm run` identical, every build diagnostic identical, and all 104 built executables' stdout, stderr, status and memory report identical (`performance.md`, N40) |
| Executable digest | VERIFIED | formatting, comments, local names, effect contracts and unrelated definitions leave it; an executable change and a capability parameter move it (`crates/nazm-cli/tests/mir.rs`) |
| Digests used as a cache key | MISSING | objects are keyed on the emitted unit's bytes (area 10b), a function of MIR |
| The interpreter on MIR | not wanted | it stays on Core IR, by decision (§7.42): nothing it needs is below Core IR |

**Limitation** · Level 6 by responsibility, not by `architecture.md` §2's shape: no borrow
checking and no optimisation run on it, locals are mutable slots, and there are no phi nodes.
Every local becomes a stack slot in the emitted code, so an unoptimised build carries more
loads and stores than N39's (`performance.md`, N40). MIR is built per run and never persisted.
The ownership dataflow costs a word per 64 tracked locals per block, so a very large function
costs more than linearly to validate (measured, N40).

**Accepted when** · Introducing the level **removed** code: the native lowering's temporaries,
partial-construction slots and `?` completion rules, and the emitter's cleanup bookkeeping — its
`owned`, `outstanding`, `building` and `frame` sets and every per-exit release decision — are
gone; a validator and a mechanical gate keep the boundary.

### 10. LIR and the backend contract — VERIFIED (v2, N105: one instruction-level LIR both backends translate)

*N105, 2026-10-05: one LIR both backends consume — VERIFIED; LIR validator v2 — VERIFIED; LIR
oracle interpreter — VERIFIED for the sequential subset (tasks, channels, C and the clock refused by
name); `nazm.lir/2` — VERIFIED.* *v1 (Q1-B-07): the oracle formats floats and `UInt64`s
(`nz.format_float`, `nz.format_u64`) by the interpreter's own `nazm_sema::numeric::to_str`, so a
program printing a number has its oracle tier — until then it refused them by name
(`floats_and_unsigned_words_print_alike_on_the_oracle`, the `numbers` compatibility case).* `architecture.md` §7.106 is the constitution, written before the
code. `crates/nazm-lir/src/op/` is an instruction set below MIR: values of a machine class (`bit`,
`i8`, `i32`, `i64`, `ptr`) defined once and used where they dominate, variables, stack slots, loads
and stores at byte offsets, wrapping and overflow-checked arithmetic, comparisons, selects, calls
direct, indirect and to the runtime, copies, and source positions. `op/lower.rs`, `func.rs`,
`prims.rs`, `helpers.rs` and `soa.rs` lower each unit's MIR to it once, and that lowering is the
only place a failure's code, message and position, a guard and its order, an offset, a runtime
entry, or a retain or release is decided. The LLVM backend is a printer of it (`op/llvm.rs`); the
Cranelift backend a translator of it (`nazm-codegen-clif/src/translate.rs`), and since N105 may not
depend on `nazm-mir` at all (`xtask/src/rules.rs`,
`the_cranelift_backend_reads_lir_and_nothing_above_it`). The old per-backend lowerings are deleted:
`nazm-lir/src/emit.rs` (4,405 lines) and Cranelift's `func.rs`, `prims.rs`, `helpers.rs`, `unit.rs`
and `layout.rs` (2,963). `op/validate.rs` checks every module before either backend sees it —
definition before use under dominance, classes, every symbol, slot, variable and block, every call
against its callee's signature, every return against its function's — and a broken module is
`N0900`; `validate::tests` breaks each rule by hand and requires it named. `op/interp.rs` executes
LIR against a model of the runtime's services and is `nazm lir --run`.

**Independent evidence** · `crates/nazm-cli/tests/lir_oracle.rs`: programs with answers worked out
by hand — arithmetic and both failure codes, `Int::MIN % -1`, control flow and recursion, strings and
sequences, records, enums and equality, closures in vectors, the program's arguments — each run on
the oracle, LLVM `-O0` and `-O2` and Cranelift, all four required to equal the hand-written answer
(and a program with tasks refused by the oracle, not answered). `crates/nazm-cli/tests/fuzz_diff.rs`
gains the oracle as a fifth tier beside the tree-walking interpreter (independent of LIR): the default
run and 300 further generated programs (seed 105105) agree on all five. The coverage fuzzer gains an
`lir` target (`fuzz/src/main.rs`): a checked program's LIR must validate and run without the oracle
finding a defect. `crates/nazm-cli/tests/layout.rs`: `--layout soa`, refused on Cranelift until N105,
is LIR's now and agrees on both backends. **Falsifier, run:** the host suite (2,424 tests in 196 binaries, the selfhost
suite refusing to run uncontained), and contained: the workspace (2,464 passed, 0 failed, 46 ignored),
selfhost (43 of 43), bootstrap (C2 = C3, 34 conformance cases and 29 refusals agreeing) and the `lir`
fuzz target (319,609 executions in 300 s, no crash); the 76
historical mutants on the deleted code repointed at the LIR and each caught by a named killer.

**What N105 found** · Two-backend agreement is no evidence for a decision both now share. Eleven of
the 76 repointed mutants survived their old killers, which compared LLVM with Cranelift; each now
has a killer with an independent expectation (the memory suite's counts, a unit test, or a
hand-written answer), and three new tests exist for that reason
(`a_reassigned_sequence_is_released_once_when_a_failure_follows`,
`a_c_bool_is_zero_extended_and_an_internal_one_is_not`,
`a_state_field_is_found_at_its_canonical_offset_not_its_declared_one`). The Cranelift object key was
MIR without positions until N105 and reused an object reporting a moved failure at its old line; it
is now the unit's LIR digest, which carries every position, with the translator's revision.

**Limitation** · The oracle runs the sequential subset only: a program with tasks, channels, a
foreign call or the clock is refused (status 1 since Q1-C1; 4 before), and those are held by backend agreement and the
memory and concurrency suites as before. The runtime itself is LLVM text both backends link (area 13),
not LIR. No optimisation happens in LIR; clang's and Cranelift's do.

*N81, 2026-10-03: Target data layout v1 — VERIFIED; Byte layouts owned by LIR v1 — VERIFIED;
Ownership table v1 — VERIFIED; Layout validator v1 — VERIFIED; `nazm.lir/1` — VERIFIED. One
instruction-level LIR both backends consume — MISSING; LIR oracle interpreter — MISSING.*
`architecture.md` §7.82 is the constitution, written before the code and stating that N81 is not
accepted under its own criteria. `crates/nazm-lir/src/abi.rs` owns a target's data layout (a pointer
8 bytes, or 4 on `wasm32`), every record's and enum's byte layout on it, and whether a type owns
anything; the Cranelift backend reads them (its `layout.rs` decides nothing now) and the LLVM backend
reads ownership, and both validate the tables before generating any code (`N0900` if broken). `nazm
lir` prints `nazm.lir/1` (`schema/nazm.lir-1.json`; since N105 `nazm.lir/2`, `schema/nazm.lir-2.json`, the only one in the tree). Evidence: `crates/nazm-cli/tests/lir.rs` —
determinism, canonical order at natural alignment, an enum's tag and payload, wasm32's narrower
handles with nothing else moving, ownership through containment, refusals, and a program using
every shape agreeing on the interpreter, LLVM and Cranelift; `abi::tests` — the validator naming
each broken invariant; `schemas.rs`; the N73 fuzzer generating a three-width record inside an
enum's payload on every tier. **Falsifier, run:** every program in the repository built with both
backends by N79 and by N81 produced byte-identical objects. *N81 left the row PARTIAL because each
backend still translated MIR's operations itself, so their agreement was tested, not constructed, and
LLVM's enum was the product of its variants, not the tagged union these tables describe; N105 closed
both — one lowering, and both backends lay every value out by these tables.*

**Evidence** · Since N40 `crates/nazm-lir` reads **validated MIR** (area 9) and nothing of the
checker: `lower.rs` settles layout (canonical by name, from the order MIR carries), symbols from
MIR's durable identities, linkage, units and externals, and refuses what the native subset does
not cover — recursion over MIR's call graph, a `main` taking anything but capabilities or not
returning `Int`, a sequence handed to a task — with the same diagnostics as before;
`op/lower.rs` (since N105; `emit.rs` before it) writes each MIR block as a label, each local as a slot, each statement and terminator
as the instructions it means. The distinguishing information it adds is how each operation
fails at the machine level; every fallible statement carries a span and a MIR failure edge.
Layering is gated: `xtask/src/rules.rs` forbids `nazm-lir` depending on `nazm-core` or
`nazm-cir`.

**Limitation** · Since N105 LIR is an instruction-level IR of its own, lowered from MIR, validated,
interpreted as an oracle and translated by both backends; between N40 and N105 only layout, symbols
and LLVM text emission over MIR were left of it. No optimisation, inlining or constant folding
happens here; the backends' own do.

*The duplicate built-in table and the second resolver this row used to name were removed by
N1; the forbidden dependency on `nazm-core` still holds, and is satisfied by both crates
depending on `nazm-sema`, which owns the shared vocabulary since N2.*

*N12 added no node.* `?` is lowered to the LIR `match` it means, with a `Stmt::Return` in its
error arm, so its cleanup is `return`'s and the emitter did not change. *Since N39 that `match`
is Core IR's* — the lowering in `nazm-core` makes it once for both backends, and `lower_propagate`
is gone; the LIR it produces is byte-identical.

*N12.1 added two, and removed a refusal.* A block used as a value compiles natively: every
value block goes through one `lower_block`, a `match` arm written as a block is its bindings
followed by that block, `Expr::Block` carries a block in any other value position, and
`Stmt::Match` is the statement `match` whose arms may produce nothing — the pairing
`Stmt::If` already had. Until then *a block used as an expression* was refused as `N0101`;
`nazm capabilities` now reports it compiled. Evidence: `crates/nazm-cli/tests/value_blocks.rs`,
19 cases each interpreted and native and required to agree on value and on every memory
counter. `architecture.md` §7.14.

*The paragraph that stood here until N5 said the backend was whole-program: one LLVM
module, functions numbered `@nz.fN`, modules existing in the front end and nowhere below
it. That was accurate, and it is no longer true — see area 10a.*

**Next dependency** · Only what a real optimisation pass demands. The second backend (Cranelift, N41) consumes the same LIR since N105.

**Accepted when** · A second consumer exists that the contract serves unchanged.

### 10a. Per-module code generation and linking — VERIFIED

**Evidence** · As of N5 (2026-09-22) each Nazm module is lowered, emitted and assembled on
its own, and the objects are linked. `crates/nazm-lir/src/lower.rs` splits into a global
`plan` — signature admissibility and, until N49, the recursion refusal, both of which span modules —
and `lower_module`, which reads one module's bodies and every function's *signature*;
`crates/nazm-lir/src/op/lower.rs` has `lower_unit` (N105; `emit_unit` wraps it), and `crates/nazm-runtime/src/` `emit_runtime` and `emit_entry` (moved by N43, its own crate since N53);
`crates/nazm-cli/src/build/objects.rs` runs `clang -c` per artefact and `build/link.rs` links.

A symbol is a spelling of the definition's `DefKey` (`crates/nazm-lir/src/symbol.rs`), not
a position: `@nz.m.lib_2Futil_2Enz.parse`. The encoding escapes every byte outside
`[A-Za-z0-9]` including `_`, so it is injective and collisions are impossible **by
construction** rather than improbable.

Sixteen cases in `crates/nazm-cli/tests/native_units.rs`, each running the compiler and the
program: a cross-module call resolved by the linker, three modules with the same private
`helper`, a diamond generated once, a legal import cycle, a cross-module call cycle still
refused, an imported `main` that is not the process entry, per-unit constants that do not
collide, a runtime defined exactly once, identical symbols across two checkouts and from
two entry points, and `pub` changing linkage without changing spelling. A `native identity`
gate covers what a test cannot: that no session id, no `pub` and no positional number
reaches a symbol, and no LLVM spelling reaches `nazm-sema`.

**What an object depends on, measured** · A's artefact names B in one `declare` and one
`call`. Changing B's body, adding a private definition to B, or adding an export A does not
call each leave A's artefact **byte-identical** — so an object depends on its dependencies'
symbol identity and ABI signature, and not on their implementations.

*Since N11* a generic function instance is a unit and an object of its own — artefacts
`g0`, `g1`, … — named from its definition's `DefKey` and its type arguments' canonical
keys, never folded into a caller's unit: `one_instance_requested_by_two_modules_is_one_unit_and_one_object`,
`a_callers_unit_never_carries_a_generic_body`, and a caller that declares an instance
carries every type the declaration names (`a_call_carries_the_types_its_callee_is_declared_with`,
found by the conformance corpus).

**Limitation** · The clean build is roughly **twice as slow** for a small program, almost
entirely because there are now several `clang` processes where there was one — measured in
`performance.md` together with a 10% smaller optimised executable, which is cross-module
inlining no longer happening. No LTO. `nazm build` still compiles a narrower subset than
`nazm run`. The call-cycle analysis is still whole-program: since N49 it places the stack check
MIR emits first in every function in a cycle, across modules (area 11).

*Until N6 this row also read "nothing is reused, and no native cache exists". That was
accurate and is no longer true — see area 10b.*

**Next dependency** · None for this row.

**Accepted when** · Met for separate generation and linking. Native reuse is area 10b.

### 10b. Native object reuse — VERIFIED

**Evidence** · As of N6 (2026-09-22) an emitted LLVM unit whose object was compiled before
is not compiled again. `crates/nazm-cache/src/object.rs` defines `ObjectKey` over three
things and nothing else — a digest of the exact bytes handed to the object compiler, the
identity of that compiler, and the configuration it resolves for the run — and
`crates/nazm-cache/src/objects.rs` is the store. `crates/nazm-cli/src/backend.rs` is the
single place the object compiler is invoked: one `clang -###` per build reads the resolved
command line, so the triple, the CPU features, the relocation model and the ABI are in the
key without anyone having enumerated them, and the invocation runs with an environment
built from nothing plus ten named variables, because `CCC_OVERRIDE_OPTIONS`, `SDKROOT` and
`MACOSX_DEPLOYMENT_TARGET` were each measured to change the object.

Thirty-three cases in `crates/nazm-cli/tests/native_cache.rs` and eighteen in
`crates/nazm-cache/tests/objects.rs`. The precision ones: a dependency's body change and an
export the caller never calls each recompile one unit of seven; a comment that moves no
emitted position recompiles nothing, and one above a fallible operation recompiles the unit
it moved. The safety ones: two optimisation levels share nothing, a backend that reports a
different version shares nothing, a truncated, edited, mis-filed or wrongly-headed entry is
a miss rather than a link failure, eight concurrent writers of one key leave one valid
entry, deleting the whole store changes only the time, and cold, warm and `--no-cache`
builds produce the same program. An `object cache` gate covers what a test cannot: that the
key still has all three fields, that a lookup still recomputes the key and the object's
digest, that nothing semantic is imported into the key, that an entry carries no field
nothing validates, and that the arguments hashed and the arguments executed come from one
function.

*Since N11, for generic instances* (`crates/nazm-cli/tests/native_cache.rs`, seven cases):
a new instantiation compiles the caller, the new instance and any runtime it newly reaches;
a generic **body** edit recompiles exactly its instances and no caller — not even its own
module's unit, which never carries the body; renaming a type parameter recompiles nothing;
a warm build compiles nothing. The key needed no change: an instance's key is its bytes.

**Measured** · On the five-module compiler, `-O0`: 665.4 ms with `--no-cache`, 730.7 ms
cold, **157.6 ms** warm; `-O2`: 1693.0 ms, 1755.4 ms, **157.6 ms**. A one-module edit costs
198–218 ms whether the edit is a private body, a called export's body, or an export nobody
calls — the three are indistinguishable, which is the point of the key. A warm build runs
**two** external processes where a cache-disabled one runs eight.

**Limitation** · This is object reuse, not incremental native compilation. Reading,
parsing, checking, backend admissibility, lowering, emission and fingerprinting run on every
build, and so does the final link. No executable cache, no `LinkKey`, no incremental linker,
no persistent LIR or LLVM, no remote cache, no LTO. Cold builds pay about 65 ms more than a
cache-disabled build for hashing, `fsync` and publication. Nothing is evicted: the store
grows until it is deleted, 852 KiB for this workload's seven units at `-O0`. A backend that
reports one version and behaves differently is indistinguishable, which is a stated limit
rather than a missing check.

**Next dependency** · A `LinkKey` if executable reuse is ever justified, and a decision
about whether the per-build `clang -###` probe — 60 ms of a 158 ms warm build — is worth
memoising, which is a cache of a toolchain fact with an invalidation question of its own.

**Accepted when** · Met for per-unit object reuse. Executable and link-result reuse are not
claimed anywhere.

### 11. LLVM backend — VERIFIED

**Evidence** · `crates/nazm-lir/src/op/llvm.rs` prints the LIR as LLVM IR text (N105); `crates/nazm-cli/src/build/`
hands it to `clang`. No LLVM is linked, so there is no build-time LLVM dependency. 127
end-to-end tests in `crates/nazm-cli/tests/build.rs`, each at `-O0` **and** `-O2`, against
a written expectation and against the interpreter.

**N49 (2026-10-01)** · recursion, direct and mutual, across modules and through generic
instances, compiles: MIR places a `StackCheck` first in every function in a call cycle and both
backends emit it as a call to the runtime's `nz.stack_check` and a branch to the failure edge, so
unbounded recursion fails with `N0408` and unwinds instead of dying of a signal (`docs/spec.md`,
*Native recursion*). Evidence: `crates/nazm-cli/tests/usability.rs` (agreement in four builds, a
depth of 50,000 natively, `N0408` under both backends at `-O0` and `-O2` and inside a task),
`crates/nazm-cli/tests/mir.rs` (placement, and none where no cycle is), `native_cache.rs` (an edit
that puts an unchanged module into a cycle recompiles it). The guarantee is conditional on the
non-recursive frames between two checks fitting the quarter of the stack kept as headroom.

**N59 (2026-10-02)** · `architecture.md` §7.61 first. A scalar temporary MIR assigns once and reads
only later in its block has no slot: the compiler written in Nazm's text at `-O0` went from 13,867
`alloca`s to 6,328 and from 177.5k instructions to 157.3k; `nazm build -O0` of it, interleaved A/B,
five runs each, debug compiler: medians 2,087 → 1,983 ms; a loop program built at `-O0` ran
133–144 → 103–105 ms with the same output (`docs/performance.md`, N59). Bindings, parameters,
managed values and anything read across blocks keep their slots
(`a_scalar_temporary_has_no_slot_and_every_other_local_keeps_one`). Both backends still need clang
for the runtime's text and the C driver for the link; `nazm inspect` names both.

**Limitation** · The arithmetic contract is the load-bearing part and is deliberately
conservative: `nsw` is not used, because it would promise overflow cannot happen and
license an optimiser to delete the trap. Debug information is emitted only under `nazm build
--debug` (N48, N58, N91; area 27); an ordinary build carries none.

**Next dependency** · None for correctness.

**Accepted when** · Already met for the current subset.

### 12. Cranelift backend — VERIFIED (v1, for the native subset, on the host)

*N99, 2026-10-04: the interactive tier v2 — the tiers VERIFIED to agree; the JIT still BLOCKED.*
`architecture.md` §7.100 first. `crates/nazm-cli/tests/tiers.rs`: one REPL session's thirteen answers,
four of them failures (`N0400` twice, `N0401`, and `i64::MIN / -1`), equal to `nazm run`'s and to the
native builds' of the same definitions by LLVM and by Cranelift. The JIT stays BLOCKED by two causes,
stated: the workspace forbids `unsafe`, which executable memory needs, and no crate that provides it
(`cranelift-jit`, `region`, `memmap2`) is in the offline cache; the isolated crate that would lift it,
with its W^X and stale-code acceptance tests, is designed in §7.100 and not built.

*N65, 2026-10-02: the interactive tier — VERIFIED for the REPL, comptime and reload checks; the JIT
BLOCKED.* `architecture.md` §7.67 first. `nazm repl` checks the session as one program on every
edit and refuses a redefinition that breaks a caller by the caller's name; `nazm comptime`
evaluates parameterless pure functions under budgets and refuses anything else (`N0393`), a runaway
stopping with `N0402`; `nazm reload-check` names each change `reloadable` or `restart`
(`nazm.reload-check/1`). Evidence: `crates/nazm-cli/tests/live.rs` (5), `live.rs`'s unit tests (5).
**BLOCKED**: an in-process JIT needs `unsafe` code, which the workspace forbids
(`unsafe_code = "forbid"`); lifting that is the project's decision. Designed in §7.67; no JIT
differential exists, and none is claimed. Nothing is hot-reloaded: the reload check is the rule.

**Evidence** · N41 (2026-10-01). `crates/nazm-codegen-clif` implements `nazm-lir`'s `Backend`
(`crates/nazm-lir/src/backend.rs`) with Cranelift 0.136.1, pinned exactly: every MIR function body
of a unit becomes Cranelift IR, verified by Cranelift's verifier, and one object per unit is written
by `cranelift-object`; the per-type retain, release and equality helpers and the task trampolines
are generated with it. `nazm build --backend cranelift` selects it; `llvm` stays the default and
the differential reference. `architecture.md` §7.43 is the constitution, written first.

| Claim | Status | Evidence |
| --- | --- | --- |
| A Cranelift-independent backend boundary | VERIFIED | `Backend` names no Cranelift type; `nazm-lir` and `nazm-cli` do not depend on a code generator (`cargo xtask check`, codegen reach) |
| Cranelift consumes LIR, and nothing above it | VERIFIED | `nazm-codegen-clif` depends on `nazm-lir` (and `nazm-span`, `nazm-diag`) only, since N105 — not `nazm-mir` (`the_cranelift_backend_reads_lir_and_nothing_above_it`) |
| The whole native subset compiles with Cranelift | VERIFIED | all 104 programs of the 205-source corpus that build: stdout, stderr, exit status and memory report identical to the LLVM backend's (`performance.md`, N41); `crates/nazm-cli/tests/cranelift.rs` against the interpreter too |
| Traps and arithmetic edges | VERIFIED | `every_trap_is_the_same_trap` (overflow, `/` and `%` by zero, `MIN / -1`, bounds, capacities), `signed_arithmetic_and_its_edges_agree` (`MIN % -1` is 0) |
| No silent fallback | VERIFIED | `choosing_cranelift_writes_no_llvm_text_for_any_unit`; a refusal writes nothing (`crates/nazm-cli/tests/cranelift.rs`); recursion compiles under both since N49 (`recursion_compiles_under_both_backends_and_agrees`) |
| Typed, centralised runtime imports | VERIFIED | every call into the runtime is built from `crates/nazm-runtime/src/lib.rs`; `a_runtime_entry_the_registry_names_is_one_the_runtime_defines` |
| Object-cache identity and reuse | VERIFIED | a key over each unit's LIR digests (MIR's until N105), layouts, callee signatures, target, backend version and settings and the runtime ABI revision, known before generation; `an_unchanged_unit_is_reused_by_its_key_and_a_changed_one_is_regenerated`, `a_cache_key_moves_with_every_setting_that_changes_the_code_and_only_those` |
| Deterministic objects | VERIFIED | `the_same_mir_gives_the_same_object_bytes` (the compiler's own source) |
| Debug information | VERIFIED as scoped | DWARF line tables, scopes and variables under `--debug` (N58, N91; area 27); MISSING until then, function symbols only (N48) |
| A target other than the host | VERIFIED as scoped | `--target` and object output for another toolchain (N57); each target's evidence in the support matrix (area 33); refused by name until then |

**Limitation** · Every local that is not a scalar lives in a stack slot and is copied by
`memcpy`-sized loads and stores; no optimisation pass runs beyond Cranelift's own. The runtime
and the platform entry are still the runtime's LLVM text compiled by clang, and clang links, so a
Cranelift build still needs clang. A Cranelift build links the whole runtime (a reused object is
not generated, so it cannot say which parts it reaches). The backend has its own internal calling
convention and layout; nothing passes a value between code the two backends generated.

**Accepted when** · Already met for the declared set: the native subset, on the host.

---

## Runtime and concurrency

### 13. Runtime — VERIFIED (the v1 contract; the M:N pool by default since N106; no allocator, and text, not a Rust crate)

*N106, 2026-10-05: the pool by default — VERIFIED; the C policy — VERIFIED; a refused scheduler name
— VERIFIED; a deterministic single-worker mode — VERIFIED.* `architecture.md` §7.107, written first.
With `NAZM_SCHEDULER` unset a native program's tasks run on N83's pool — `NAZM_WORKERS` (4) workers,
each task a guarded `NAZM_TASK_STACK` (256 KiB) stack — on every hosted target with a switch;
`threads` asks for a thread per task, `pool` for the pool, and any other value starts no task
(`N0404`). A program that calls C runs a thread per task unless the pool is asked for — a task
blocked in C would hold its worker — and `nazm explain-cost` says which scheduler and why
(`nazm.cost/2`). Runtime ABI revision 14 (`nz.pool_policy`, stored by the entry before `main`).
Evidence: `crates/nazm-cli/tests/scheduler_default.rs` — the default on both backends with the same
output, ending and reports as on threads; ten thousand tasks on four workers
(`workers=4 peak=10000`); four names refused; the C policy on both backends and in the cost report;
one worker giving the same order five times — and the whole suite, host (macOS aarch64) and
contained (Linux aarch64), running every native program on the default. x86-64 macOS under Rosetta:
a channel program runs on the pool by default. `docs/performance.md` (N106): spawn and join 6.7 µs
against 26.1 on threads, a parked receive woken 7.8 against 24.6, 17.8 KB per live task. **Scope:**
the interpreter and the compiler written in Nazm keep a thread per task; no work stealing,
preemption or hand-off of a blocking C call.


**Evidence** · N43 (2026-10-01): `architecture.md` §7.45 is the runtime constitution, written
first. The runtime is LLVM text generated into each program, emitted only for the parts reached,
from `crates/nazm-runtime/src/` (in `nazm-lir` from N43 to N52): `core.rs` (the portable text — state, failure and report,
sequences, strings, I/O, join, tasks, channels, the second-backend shims), `os.rs` (the operating-
system adapter: the only C-library and POSIX-threads entry points anything calls), `entry.rs`
(the platform entry) and `lib.rs` (the contract). *N53:* those four files are the
`nazm-runtime` crate, which depends on nothing of the compiler (`cargo xtask check`, layering);
`nazm-lir` and `nazm-codegen-clif` consume it. Runtime ABI revision 5, and `digest()` — blake3 of
the whole runtime text, the platform entry and the inventory — enters every object key beside the
revision under both backends (`the_runtime_s_digest_is_part_of_every_configuration`,
`the_runtime_is_revision_five_and_its_digest_covers_generic_channels`, renamed with each revision since — `…_revision_twenty_eight_…` at v1). *N49:* runtime ABI revision 3 — `nz.stack_check`
and its thread-local limit, emitted only where a function in a call cycle calls it, and task
threads started with an 8 MiB stack (`a_task_thread_has_the_stack_the_check_budgets_for_on_every_host`). `compiler/emit.nz`'s copy is **derived** from
`core.rs` line by line, re-derived by `cargo xtask check`'s `runtime parity` gate since N8.

| Claim | Status | Evidence |
| --- | --- | --- |
| One typed, versioned inventory of every runtime symbol | VERIFIED | `INVENTORY` (32 functions: signature, part, responsibility, ownership, effect, failure, thread safety) and `GLOBALS` (11); units' declarations generated from it; `every_runtime_definition_is_inventoried_with_its_exact_signature`, `every_runtime_global_is_inventoried`, `the_word_view_agrees_with_the_inventory` |
| Runtime ABI revision in every object key | VERIFIED | `RUNTIME_ABI_REVISION` (2 when this row was written; 28 at v1); Cranelift keys (N41) and clang's configuration (`the_runtime_abi_revision_is_part_of_every_configuration`) |
| Startup and shutdown defined | VERIFIED | arguments stored once without the program's name, roots minted by the entry alone, three distinguished endings (`the_three_ways_a_program_ends_are_distinguished`, `the_arguments_are_stored_once_at_startup_without_the_program_name`, `root_capabilities_come_from_the_entry_alone`) |
| Memory contract | VERIFIED | strings, sequences, sequences of strings and of records, channels, shared backings across four threads, allocation-failure paths, all counted exactly (`the_runtime_s_services_keep_their_contract_when_called_directly`, a C harness linked against the runtime alone) |
| Failure classes | VERIFIED | language, host-refused, I/O and explicit exit distinguished; a `Result` never becomes one (`an_err_is_data_and_never_a_failure`) |
| Portable core, one OS adapter | VERIFIED | `a_runtime_calls_the_operating_system_only_through_the_adapter`; the same POSIX surface on every v1 target |
| A built, versioned, verified runtime artifact (N82) | VERIFIED | `nazm runtime build` writes `libnazmrt.a` and a `nazm.runtime/1` manifest per target and profile (hosted, board, wasm); `nazm build --runtime DIR` links it on both backends and refuses every artifact that does not fit, by name, before writing anything; `nazm runtime verify`. `crates/nazm-cli/tests/runtime_artifact.rs` (build and determinism, a program reaching every service identical with the artifact on both backends, nine refusals, profiles and a board's archive, provenance), `schemas.rs`; every program in the repository built both ways, 228 builds, identical output, ending and memory report |
| A scheduler beyond one thread per task, by default | VERIFIED | N106: the pool, with its policy stated (`scheduler_default.rs`) |
| A runtime crate, an allocator | MISSING | the runtime is still LLVM text, one implementation for every target; `malloc` is the C library's |

**Limitation** · Still no `nazm-rt` crate: the runtime is text, so its tests link it with a C
harness rather than call it from Rust. Thread safety is by construction (atomic string and channel
counts, sequences confined by `N0321`), not by a sanitizer run. *One OS thread per task* stood here
until N106.

**Next dependency** · None for the v1 contract. The scheduler arrived as the pool (N83, the default
since N106, area 15); a runtime written as a Rust crate would need the `unsafe` allowance the
workspace forbids, and no program has needed an allocator of its own.

**Accepted when** · For v1: met. For the area (met by N106): a scheduler beyond one thread per task, with its
policy stated.

### 14. Structured concurrency — VERIFIED

**Evidence** · Specified in `docs/spec.md` before implementation, then interpreted
(`crates/nazm-core/src/eval/`, 18 tests in `crates/nazm-core/tests/concurrency.rs`), then
compiled (17 tests in `crates/nazm-cli/tests/concurrent.rs` at `-O0` and `-O2`, each
against a written expectation *and* the interpreter), **then compiled by the compiler
written in Nazm** — `compiler/emit.nz` carries the task and channel runtime. Every test
carries a deadline, so a hang fails by name. Static rules are real: `N0320` for `spawn`
outside a `scope`, `N0321` for a sequence crossing into a task.

*Narrowed in N10.1:* the compiler written in Nazm joined a scope when control fell off its
end or failed, and not when a `break`, `continue` or `return` left it — so a task that failed
inside a scope a `break` left was never waited for and its failure never taken on. The
reference and the interpreter were right. Held by the `scope_*_joins` cases in
`crates/nazm-cli/tests/transfers/`.

*N44 (2026-10-01):* `architecture.md` §7.46 is the scheduler contract — task states, scope
ownership, which failure a scope reports (the first in start order, the body's before any
task's), no cancellation, the OS as scheduler and why a pool would break the progress guarantee,
fairness and determinism, authority and provenance across a spawn, what may cross, channels,
shutdown. `crates/nazm-cli/tests/scheduler.rs` holds it under the interpreter, LLVM `-O0`/`-O2`
and Cranelift with every native run fully reclaimed: 5,000 short tasks, thirty nested scopes
joined innermost first, the reported failure in start order not time order, a failure deep in
nested scopes, records and enums handed to tasks, a waiting receiver woken, authority passed,
withheld and bridged, provenance through a spawn (`N0372`), nothing mutable crossing (`N0321`,
transitively), `exit_with` from a task. Peak RSS stays at 3.8 MB from 5,000 to 500,000 tasks.

**Limitation** · A native program's tasks run on the M:N pool by default (N106, area 15) — bounded
workers, a task pinned to one, no work stealing, no preemption and no fairness guarantee; the
interpreter, the compiler written in Nazm and a program that calls C keep a thread per task.
Deadlines exist on selects (N54), and there is **no cancellation beyond closing a channel**.
`Chan` alone carries `Int`; `Chan[T]` (N53) carries any `T` that may cross into a task, no
sequence or closure (`N0390`); since N54 a select over `Chan[T]` receives, a deadline needs a
`TimeCap`, and closing a channel is how a task is cancelled — cooperatively, where it waits. Ownership prevents data races; **that is not deadlock freedom**
and is not claimed as such (`architecture.md` §5).

*N53, 2026-10-02: Generic channels — VERIFIED.* `Chan[T]` with `chan_new_of[T]`, `chan_send_of`,
`chan_recv_of(c, out)` and `chan_close_of`: strings, records and enums with payloads cross tasks
through typed channels identically under the interpreter, LLVM `-O0`/`-O2` and Cranelift, every
run fully reclaimed — a value still queued when the last reference goes released exactly once, a
send after close refused and its value released by the sender. The type persists in interfaces
(`{"chan":T}`, which a `/7` reader refuses as an unknown shape), and a typed channel's operations
are `explain-cost` sites. `crates/nazm-cli/tests/generic_channels.rs`, 8 tests; fourteen N53
mutations.

*N54, 2026-10-02: Select, deadlines, cooperative cancellation, counters — VERIFIED for one OS thread
per task.* `chan_select_of` (lowest ready index; a closed channel is ready) and `chan_select_until`
(`-2` after at least `ms`) over `Vec[Chan[T]]`; `time_now_ms`; both clock built-ins need a `TimeCap`
held (`N0369`), and `critical`'s `no-ambient-time` refuses them. A task waiting in a select is
woken by another task's send and cancelled by a close; N44's failure rule is unchanged.
`NAZM_SCHED_REPORT` counts tasks spawned and joined, selects and timeouts, and the interpreter, LLVM
and Cranelift agree on it. `crates/nazm-cli/tests/select.rs`, 9 tests; sixteen N54 mutations.

**Next dependency** · None for the current model; area 15 for anything beyond it.

**Accepted when** · Met for the model as specified.

### 15. Advanced scheduler — VERIFIED as scoped (N83)

*N106, 2026-10-05: the pool is the default (area 13); the whole suite runs on it, on macOS aarch64 and,
contained, Linux aarch64 — the Linux run N83 did not have.*

*N83, 2026-10-04: M:N tasks on bounded workers v1 — VERIFIED (opt-in); suspension instead of a
blocked worker — VERIFIED; per-task failure state and stack limit — VERIFIED; deadlines on parked
tasks — VERIFIED; work stealing, preemption, blocking-FFI hand-off, a model checker — not here.*
`architecture.md` §7.84 is the constitution, written first. With `NAZM_SCHEDULER=pool` a native
program's tasks run on `NAZM_WORKERS` threads (default 4), each task a guarded stack
(`NAZM_TASK_STACK`, default 256 KiB) pinned to one worker, switched by a per-target routine
(`crates/nazm-runtime/src/switch.rs`); the four suspension points park the task and free the worker
(`nz.cwait`, `nz.ctimedwait`, `nz.cwake`, `nz.join_one`), its failure state and stack limit saved and
restored at every switch. The thread model's runtime texts are unchanged and the pool is joined to
them as they are emitted, so `compiler/emit.nz`'s copy still matches (`runtime parity`). Evidence:
`crates/nazm-cli/tests/scheduler_pool.rs` — four concurrency shapes (fan-in, a three-stage pipeline
of `Chan[Str]`, a select, nested scopes) giving the same output, ending and memory report on the pool
as on threads, on both backends; ten thousand tasks all alive at once on two workers
(`nazm-pool: workers=2 peak=10000`); a failing task and a sibling on one worker, and recursion past a
task's stack reported as `N0408` rather than a crash; a polling select letting a sibling on its worker
run; a parked task's deadline firing; every run under a deadline, so a lost wake-up fails rather than
hangs. The whole `nazm-cli` suite also passed with `NAZM_SCHEDULER=pool` exported, every native
program it builds running on the pool. Measured (`performance.md`): spawn and join 7.4 µs against
25.4 µs on threads, a round trip 5.0 µs against 6.7 µs, about 17.8 KB resident per task against 18.4
KB, page 16 KiB; 50,000 tasks on four workers in 891 MB, where threads did not finish 6,000 within
40 s. **Scope**: opt-in, the default unchanged; verified on `aarch64-apple-darwin` and on
`x86_64-apple-darwin` under Rosetta, compiled and not run for Linux; the interpreter and the compiler
written in Nazm keep a thread per task.

**Evidence** · `architecture.md` §5 and `roadmap.md`'s C-4/C-5 rows: M:N tasks, work
stealing, growable stacks, an event reactor, blocking-FFI handoff. N44 measured the baseline any
of it must beat (`docs/performance.md`, N44): a task started and joined in 13 µs, a channel round
trip between two tasks 4.6 µs, a streamed value 60 ns, and linear scaling to four tasks; and
decided against a fixed worker pool, which with blocking channels and no suspendable tasks would
deadlock programs that are correct today (§7.46).

*N54, 2026-10-02 — DESIGNED.* §7.56 chose the model a pool would use — stackful tasks on guarded
stacks with a per-target switch, stackless continuations ruled out for colouring every function
that can block — and named what has to move first: the thread-local first-failure slot, the stack
limit, and a hand-off for blocking foreign calls. Select, deadlines and cancellation were built so
that nothing in them depends on the thread model.

**Limitation** · No work stealing, no preemption, no hand-off of a blocking foreign call and no
model checker (N83): a task that computes without waiting holds its worker, and a program that
calls C runs a thread per task unless `NAZM_SCHEDULER=pool`. Measured on macOS (N83) and run,
contained, on Linux aarch64 (N106). `architecture.md` records that the "1M live tasks under 2GB"
target was **withdrawn** as arithmetically unsupported — the right precedent for anything proposed
here. *Until R1 this said "none of it exists", true before N83.*

**Next dependency** · Work stealing and a blocking-FFI hand-off, each with its own measurement; neither
is scheduled. *Until R1 this named `nazm-rt` and the `unsafe` allowance; the pool was built without
either.*

**Accepted when** · Spawn-to-first-instruction latency, context-switch cost and task memory
as **RSS, not virtual reservation, with page size recorded**, measured against a named
implementation.

---

## Libraries and interop

### 16. Standard library — VERIFIED as scoped (1.0, N84)

**R1d (2026-10-14)** · *The platform, further: processes and the file system* (`spec.md`, *The
platform, further*; `openspec/changes/r1-language-for-the-rewrite`, D4's built-ins, P1 and P2).
Seventeen prelude built-ins over Gate 2's handle table and statuses. Standard output and errors as
handles (`os_stdout`, `os_stderr`), written in order with `print`, a close leaving the stream open.
A child in a directory with exactly a given environment, handed over ordered by name so every
implementation gives the same one (`os_spawn_in`), leading a new process group or reading a null
standard input (`os_spawn_with`); its id (`os_child_pid`) and this process's (`os_pid`); bounded
waits (`os_try_wait` → `WouldBlock`, `os_wait_until` → `TimedOut`, the child left running); a signal
to a group (`os_kill_group`, `ProcessCap`); a waited child's peak resident memory (`os_child_rusage`,
`wait4`'s `ru_maxrss`). A path's own facts with a symbolic-link kind, device and inode (`os_lstat`),
permissions (`os_chmod`), a canonical path (`os_realpath`), a new private directory whose path is a
`Str` as clean as its parent, so a program may write under it (`os_mkdtemp`), and advisory whole-file
locks held by the open file (`os_lock`, `os_try_lock`, `os_unlock`: `flock`, `LockFileEx`).
`@std/file` (`file_stdout`, `file_stderr`, `EntryKind`, `LinkMetadata`, `file_symlink_metadata`,
`file_set_permissions`, `file_canonical`, `dir_create_temp`, `file_lock`, `file_try_lock`,
`file_unlock`), `@std/process` (`process_id`, `process_spawn_in`, `process_spawn_with`,
`process_child_id`, `process_try_wait`, `process_wait_until`, `process_kill_group`,
`process_peak_memory`) and `@std/fs` (`fs_temp_dir`): API-1.0 gains 20 lines. `file_walk` lists a
symbolic link and never follows it, as D4 approved. EXPERIMENTAL until R1 closes. Evidence:
`crates/nazm-cli/tests/platform_files.rs` and `process_control.rs` — each program in a fresh
directory under the interpreter, LLVM `-O0`/`-O2` and Cranelift with the same memory report and
nothing live: raw bytes among `print`'s and a closed stream handle; a file, a directory, a link, a
dangling link and a hard link by `os_lstat` (the same file by device and inode), `chmod` through a
link, a canonical path, two temporary directories written under and removed; locks held by the open
file, converted, tried, let go by a close, refused on a directory and standard input; a child's
environment exactly given and ordered, a relative program beside its directory, seven refusals
before anything starts, a null standard input; bounded waits on a child that ends and on one that
does not, a group of two ended by `SIGTERM` and checked gone (by the program and again by the test),
id, peak memory (positive natively), and the library over all of it; the authority (`N0369`) and
sink (`N0372`) refusals; a program's own `os_lstat` kept; a WASI build of the file-system ones and
the refusal by name of a process one; the Windows units built by both backends. `file_walk.rs`:
links listed, not followed. **Unexecuted**: Windows (written against Win32 — `CreateProcessW` with an
environment block and directory, `GetFileInformationByHandle`, `GetFinalPathNameByHandleW`,
`LockFileEx`, `K32GetProcessMemoryInfo` — compiled, not run under Wine), Linux and Android (compiled;
the contained run has not executed them), WASI (built, not run under Node). **Scope**: the interpreter
cannot ask a child's peak memory without C and answers `ENOSYS`; it signals a group through the
system's `kill`; no tree removal (listed names are `file`-origin); no signal handling
(`limitations.md`).

**Gate 2 (2026-10-09)** · *Reference applications* (`general-purpose.md` §24). Five programs in
`examples/apps/` — `nwc`, `jsonpipe`, `nbody`, `kvstore`, `kvd` — use the standard library for real
work: files and standard input, JSON and maps, floats, a synced log with atomic compaction, and a
TCP server with a task per connection and an owner task behind channels. Evidence:
`crates/nazm-cli/tests/apps.rs`, every app in the interpreter and three native builds, held to its
recorded output and status; time and memory recorded in `examples/apps/README.md`.

*N84, 2026-10-04: The standard library 1.0 — VERIFIED as scoped.* `architecture.md` §7.85 first.
Seventeen modules, 126 public items (`library/std/API-1.0`): text grown (case, trimming either end,
replace, count, padding, lines, comparison, byte classes), seq grown (reverse, slice, concat, any,
all), a new `sort` (a stable sort by a caller's ordering, polymorphic in its effects; `Int` and `Str`
sorts — its own module so that `@std/seq` stays importable under `embedded`), and
`fmt`, `num`, `map` (ordered by key), `path` (lexical), `fs` (`IoCap`, failures as values), `time`
(`TimeCap`), `json` (an arena document, integers only, every malformed input an `Err` naming its byte
offset) and `test`; channel helpers for `Int` and `Str`. Evidence: `crates/nazm-cli/tests/stdlib_1_0.rs`
— every new function at its edges on the interpreter, LLVM `-O0` and `-O2` and Cranelift with
everything reclaimed; a JSON round trip and nine malformed inputs; four representative programs under
`examples/std/` — a word counter, a table summer, a channel service and a package-manifest tool —
written against the library for all their plumbing and giving the same output on every tier; and the
manifest regenerated and compared, so a 1.x change to a listed item fails. `stdlib.rs` keeps every
module checking cleanly, documented and listed. Measured (`performance.md`): sorting 100,000 integers
19.6 ms; 10,000 map insertions 391 ms (an insertion moves the entries after it); parsing and
re-encoding 128 KB of JSON about 14 ms. **Scope**: no networking, cryptography or floating point, by
decision; the map's insertion is linear; the compiler written in Nazm cannot import any of it.

**Gate 2 (2026-10-09)** · *Logging* (`general-purpose.md` §20). `@std/log`: levelled records with
fields, one JSON object a line on standard error under an `OutCap`, and timed spans. Evidence:
`log_records_are_one_json_object_a_line_alike_everywhere`. **Scope**: no `task_id()` or
`runtime_stats()` (`limitations.md`).

**Gate 2 (2026-10-09)** · *Randomness* (`general-purpose.md` §21; `spec.md`, *Randomness*).
`RandomCap` (id 8) and `os_random_bytes` from the system's entropy; `@std/random`'s seeded
xoshiro256** generator, pure. Runtime ABI 20; semantic epoch 40. Evidence:
`a_seeded_generator_repeats_everywhere_and_entropy_does_not` (the sequence an independent Python
implementation gives, every implementation), `entropy_needs_a_random_cap_and_a_seeded_generator_needs_none`.

**Gate 2 (2026-10-09)** · *Serialization* (`general-purpose.md` §14). `@std/json` reads and writes
floats beside its frozen integer parser, and pretty-prints; `@std/binary` encodes fixed-width
integers and floats in either order, varints, zig-zag and length-prefixed data. Evidence:
`json_reads_and_writes_floats_beside_its_frozen_integers_everywhere`,
`binary_encodes_byte_for_byte_and_reads_back_everywhere` (the bytes Python's `struct` and an
independent LEB128 give) — every implementation.

**Gate 2 (2026-10-09)** · *Errors* (`general-purpose.md` §13). `@std/error`'s `Error`, its
context gathered innermost first and shown outermost first, `error_from_io`, `error_io_result` for
`?`, and `@std/ioerror`'s `io_error_from_errno`. Evidence:
`an_error_gathers_context_and_reads_outermost_first_everywhere` (every implementation).

**Gate 2 (2026-10-09)** · *Process and environment* (`general-purpose.md` §12; `spec.md`,
*Process and environment*). The environment (`os_env_*`, `@std/env`), standard input's helpers
(`@std/io`), and `ProcessCap` (id 9) for `os_spawn`: a program run with three pipes that are handles
waiting as sockets do, its exit code or signal, and `SIGKILL`; `@std/process`'s `process_run`
collects output and errors without a pipe filling unread. Runtime ABI 19; semantic epoch 39.
Evidence: `crates/nazm-cli/tests/process.rs` — a program run with input whose output, errors and
status come back, 300 KB and 200 KB on its two streams, a signal's ending as 265, a missing program
as `NotFound` (interpreter, LLVM −O0/−O2, Cranelift); the environment and standard input alike;
`a_task_reading_a_childs_pipe_does_not_hold_its_worker` (one worker, both backends); the
`ProcessCap`, sink and `IoCap` refusals. **Scope**: no signal handling; a child's exit is waited for
on the worker (`limitations.md`).

**Gate 2 (2026-10-09)** · *Time* (`general-purpose.md` §11; `spec.md`, *Time*). `time_now_ns`
(the monotonic clock `time_now_ms` reads), `time_wall_ns` and `time_sleep_ms`, which parks on the
pool; `@std/time` gains `Duration`, `Instant` and `WallTime`. Runtime ABI 18; semantic epoch 38.
Evidence: `crates/nazm-cli/tests/time.rs` — the clocks, a sleep, deadlines and the library alike in
every implementation; `a_sleeping_task_does_not_hold_its_worker` (one worker, a computing task
finishing before a sleeping one started first, both backends); the `TimeCap` and `critical`
refusals.

**Gate 2 (2026-10-09)** · *Networking* (`general-purpose.md` §9–§10; `spec.md`, *Networking*).
`NetCap` (id 7) and the network `os_` built-ins over the same handle table: TCP listen, accept,
connect with a deadline, read, write, shutdown; UDP bind, send, receive with the sender; local
and peer addresses; name resolution; a deadline per handle. Every wait parks: one reactor thread
(`kqueue` on macOS, `epoll` on Linux) wakes a waiting task through the pool's existing wait, and a
close wakes it with `Closed`. `@std/net` gives `Result` APIs. Runtime ABI 17; semantic epoch 37.
Evidence: `crates/nazm-cli/tests/net.rs` — an echo server answering clients, a datagram's round
trip with its sender, eight failure kinds (`AddressInUse`, `TimedOut` on accept and receive,
`ConnectionRefused`, `Closed`, four `InvalidInput` addresses) and resolution, a close waking a
waiting task, a peer that has gone being an error and never a signal — interpreter, LLVM −O0/−O2,
Cranelift — and `a_task_waiting_on_a_socket_does_not_hold_its_worker` (eight tasks parked on
**one** pool worker while a ninth computes, both backends); the authority, handle and sink
refusals; `every_targets_os_unit_is_llvm_its_toolchain_compiles`. **Scope**: no TLS or
Unix-domain sockets; resolution holds the worker (`limitations.md`).

**Gate 2 (2026-10-09)** · *Files and handles* (`general-purpose.md` §8; `spec.md`, *Files and
handles*). The `os_` built-ins over an `OsHandle` — a descriptor and its generation in a handle
table the interpreter and the runtime each keep — and `@std/ioerror` and `@std/file` over them:
open, create, append, create-new, read, write every byte, seek, sync, close, metadata, list,
make, remove, rename and replace atomically, every failure an `IoError` whose kind each platform
family's table in `@std/ioerror` decides. Runtime ABI 16: the `nz.os_*` entries and a per-target
`os` unit (`open` flags, `struct stat` and `struct dirent` layouts, the 64-bit-inode symbols,
`strerror_r`). Evidence: `crates/nazm-cli/tests/files.rs` —
`the_library_opens_writes_seeks_lists_renames_and_replaces_alike_everywhere` (forty lines through
every operation and seven failure kinds, interpreter, LLVM −O0/−O2 and Cranelift, everything
reclaimed), `a_closed_handle_answers_closed_and_never_reaches_a_reused_descriptor`,
`standard_input_is_read_alike_everywhere`, `a_buffer_too_short_for_a_stat_stops_the_program_everywhere`,
`the_platform_family_is_the_hosts`, and the authority (`N0369`), equality (`N0304`) and sink
(`N0372`) refusals. **Scope**: a handle is closed by `os_close` or at exit, not at its last
reference; reading a file holds the worker; no locking, mapping or link creation
(`limitations.md`).

**Gate 2 (2026-10-09)** · *Collections* (`general-purpose.md` §7; `spec.md`, *Standard library*).
`@std/hash` (a `Hash` trait for every integer type, `Bool` and `Str`; FNV-1a then SplitMix64's
finaliser), `@std/hashmap` (open addressing, linear probing, tombstones, doubling before
three-quarters full; a seed), `@std/hashset` and `@std/deque` (a ring buffer): twenty-one modules,
158 public items. An impl may be for any numeric type (epoch 35). Evidence:
`hash_maps_sets_and_deques_behave_alike_everywhere` (a thousand inserts through growth, removal
past tombstones, a set of a program's records, the hash vectors computed independently, a deque
wrapping both ways — interpreter, LLVM −O0/−O2, Cranelift, everything reclaimed),
`a_seed_changes_a_maps_order_and_nothing_else`,
`a_number_of_any_width_has_impls_and_its_methods_run_everywhere`. **Scope**: `HashMap` is written in
Nazm over `Vec`, so its constant factor is the interpreter's and the backends' `Vec` access, not a
tuned table; no ordered map other than `StrMap`.

**Evidence** · N45 (2026-10-01), `architecture.md` §7.47 first. Seven toolchain-owned Nazm
modules in `library/std/`, imported by name — `use "@std/text";` — resolved by the loader
(`crates/nazm-service/src/load.rs`) to the compiler's copy of their bytes
(`crates/nazm-sema/src/stdlib.rs`) and keyed `@std/NAME`, which no project path can take. They are
ordinary source: no compiler support beyond resolution, and the built-in table is unchanged. The
prelude (`library/core/prelude.nz`, `Result` and `Option`) is still the only module imported
without a `use`.

| Claim | Status | Evidence |
| --- | --- | --- |
| Found by name and only by name; `@std/` never read from disk | VERIFIED | `every_standard_module_is_found_by_name_and_checks_cleanly`, `no_project_path_is_keyed_as_a_standard_module` |
| Text, option, result, sequence, I/O, process and channel helpers behave as documented at their edges | VERIFIED | `text_behaves_as_documented_at_its_edges`, `option_result_and_seq_helpers_work_for_every_type`, `chan_take_receives_one_value_at_a_time_across_tasks` — interpreter, LLVM −O0/−O2 and Cranelift, everything reclaimed |
| Authority visible in every signature | VERIFIED | `io_and_process_need_the_authority_they_show` |
| Origins preserved through wrappers | VERIFIED | `what_a_standard_function_reads_keeps_its_origin` |
| Ordinary failure is a value | VERIFIED | `text_parse_int` `Err` for no digits, a non-digit and both ends of the range; `None` for an absent search or an empty maximum |
| Documented where the spec lists it | VERIFIED | `every_public_standard_function_is_documented_and_listed`; `nazm docs --section spec:std`; the spec's example runs (`the_spec_s_example_runs_everywhere`) |
| Higher-order helpers | PARTIAL | `vec_map`, `vec_filter`, `vec_fold` in `@std/seq` (N50), plain Nazm over function values, on records and enums (`map_filter_and_fold_work_over_records_and_enums`); pure functions only until effects can be abstracted over |
| Formatting, domains (HTTP, JSON, time, crypto) | PARTIAL | JSON (`@std/json`, N84) and time (`@std/time`, Gate 2) exist; HTTP and cryptography MISSING, out of scope for v1 |

**Limitation** · Functions carry their module's name (`text_split`): `use "@std/text" as t;` and
`t::text_split` qualify them since N49, and a plain import still needs the prefix. The compiler
written in Nazm resolves `@std` since N102. No networking, cryptography or floating point, by
decision; a listed item does not change within 1.x (`library/std/API-1.0`). *Until R1 this said
there were no qualified names and that the compiler written in Nazm reported an `@std` import as a
missing module — true before N49 and N102.*

**Next dependency** · None forced by a program: effect polymorphism (N51) lets the higher-order
helpers take effectful functions, and `@std/sort` sorts by a caller's ordering (N84).

**Accepted when** · For v1: met. For the area: a library a real program can be written against
without reimplementing collections or text handling, with a stability policy past 1.0.

### 17. FFI and ABI — VERIFIED as scoped (a practical C ABI subset, N85; native builds on the host)

**Gate 2 (2026-10-09)** · *The C foundation* (`general-purpose.md` §23). An export takes and returns
every fixed-width integer and both floats as their C types, which the header names; with what
foreign calls already carried, the scalar C ABI every binding generator reads is complete.
Semantic epoch 41. Evidence: `crates/nazm-cli/tests/c_foundation.rs` — the header's declarations,
and the same answers at the edges of every width and both floats from a C program linking the
static archive and the shared library and from Python's `ctypes`, under both backends.

**Evidence** · N42 (2026-10-01). `extern "C" fn name(p: T, …) -> R = "symbol";` declares a C
function by its signature and its C symbol (`crates/nazm-syntax/src/parser.rs`; `extern` is
contextual, not a keyword); `architecture.md` §7.44 is the constitution, written first, and
`docs/spec.md` *Foreign functions* the language rule. `nazm build --link FILE` hands objects and
libraries to the linker. Both backends declare the symbol with the target's C convention and call
it with no failure check after it.

| Claim | Status | Evidence |
| --- | --- | --- |
| A C function is declared and called, with both backends agreeing | VERIFIED | `a_program_calls_c_with_ints_and_bools_and_both_backends_agree` (registers and the stack, `INT64_MAX`, nested calls, C state), clang-compiled fixtures |
| Only FFI-safe types cross | VERIFIED | `Int` as `int64_t`, `Bool` as zero-extended `bool`; `Str`, sequences, records, enums, capabilities refused, `N0381` (`every_declaration_outside_the_subset_is_refused_by_its_own_code`) |
| The declaration's shape is checked | VERIFIED | ABI `N0380`, symbol `N0382`, generic or effect-annotated `N0384`, conflicting or runtime-reserved symbol `N0383` in one module and across modules (`two_modules_that_disagree_about_a_symbol_are_refused_before_linking`, `a_reserved_c_symbol_is_every_one_the_runtime_calls`) |
| Calling C is an effect and needs authority | VERIFIED | effect `foreign`; a `ForeignCap` held at every call, declared set or not (`a_foreign_call_needs_a_foreign_cap_everywhere`, `an_imported_foreign_function_needs_a_foreign_cap_too`) |
| The provenance question | VERIFIED | what C returns carries `unknown` and its arguments' origins; `N0372` for a restricted `write_file` path computed from it (`what_c_returns_carries_the_unknown_origin`) |
| Ownership across the call | VERIFIED | nothing managed crosses; values live across calls are reclaimed on every path, a failure after a foreign call included (`a_foreign_call_inside_managed_code_leaks_nothing`) |
| The interpreter | VERIFIED (refusal) | `nazm run` refuses a program calling C before running any of it, `N0385` (`nazm_run_refuses_a_foreign_call_before_running_anything`) |
| Link inputs | VERIFIED | `--link` is passed in order; a missing file is refused before compiling; an unresolved symbol is reported as a missing `--link`, not a compiler bug (`the_linker_is_handed_exactly_what_link_names`) |
| Interface and cache | VERIFIED | `nazm.interface/7` carries a foreign export's symbol; semantic epoch 11; the C symbol enters the object key (`the_c_symbol_enters_the_object_key`) |
| `Str` arguments (N55) | VERIFIED | a borrowed NUL-terminated copy for the call, freed after it, under both backends with a balanced memory report; a NUL byte refused before C runs, `N0405`; a sequence result still `N0381` (`a_str_crosses_into_c_as_a_borrowed_c_string_under_both_backends`, `a_str_holding_a_nul_is_refused_before_c_runs`, `a_str_result_from_c_is_a_copy_since_n85_and_a_sequence_result_is_still_refused`) |
| Exporting Nazm functions to C (N55) | VERIFIED | `pub extern "C" fn … ! {} = "sym" { … }`, `Int`/`Bool` only, `N0391` otherwise; one symbol per program (`N0383`); a failure ends the process with status 2 instead of unwinding into C (`an_export_is_public_effect_free_scalar_and_not_generic`, `an_export_symbol_is_defined_once_in_a_program`) |
| Static libraries (N55) | VERIFIED | `nazm build --lib -o OUT.a` with no `main`, and `OUT.h` in a fixed order; a C program links and calls it under both backends; two builds are the same bytes (`a_library_and_its_header_link_into_a_c_program_under_both_backends`, `a_library_is_the_same_bytes_on_every_build_and_needs_no_main`) |
| Opaque handles (N85) | VERIFIED | `extern "C" struct Db;`: made only by a foreign result, held in a binding, a record field and a `Vec`, passed back to C, under both backends; built or taken apart `N0611`, `==` `N0304`, into a task `N0321`, through a channel `N0390` (`a_handle_a_c_struct_a_c_string_errno_and_a_callback_cross_under_both_backends`, `every_misuse_of_a_handle_a_c_struct_or_a_callback_is_refused_by_its_code`) |
| Nullability (N85) | VERIFIED | `-> Db` and `-> Str` fail the call with `N0409` on null, before Nazm sees it, status 2 under both backends; `-> Option[…]` makes null `None`; `Option[Db]` as an argument `N0381` (`a_null_where_the_declaration_promised_a_value_fails_the_call_with_n0409`) |
| C-layout structs (N85) | VERIFIED | `extern "C" struct P { … }` of `Int`, `Bool`, handles and nested C structs, laid out in declaration order — C reads `flag` at 8 and `y` at 16 whatever order a construction names them in — passed as a `const struct *` to a copy freed after the call (10 000 calls leave the allocator's block count where it was, macOS); any other field, a struct result, a by-value struct `N0381` (`a_struct_passed_to_c_is_a_copy_freed_after_every_call`) |
| `Str` results (N85) | VERIFIED | a `const char *` C keeps, copied into an owned `Str` at the call; balanced memory report (`a_handle_a_c_struct_a_c_string_errno_and_a_callback_cross_under_both_backends`) |
| `errno` (N85) | VERIFIED | `c_errno()` is what the latest foreign call on this thread left, captured as the call returns under both backends; needs a `ForeignCap`; refused by `nazm run` (`N0385`) and on freestanding and WebAssembly targets (`N0613`) (`a_program_reading_errno_runs_only_natively_and_only_where_there_is_one`) |
| Callbacks (N85) | VERIFIED | an export's name passed for a `fn(Int) -> Int` parameter is its C entry: C calls it inside the call and from a thread C starts, under both backends; a closure or a non-export `N0612` |
| Modules and cache (N85) | VERIFIED | a handle and a C struct imported from another module keep what they are through `nazm.interface/9` and the check cache; moving a C struct's fields changes its interface, an ordinary record's does not (`a_handle_and_a_c_struct_keep_what_they_are_across_a_module_boundary_and_in_the_cache`, `a_c_struct_s_field_order_is_its_interface_and_an_ordinary_record_s_is_not`) |
| Shared libraries (N85) | VERIFIED (macOS) | `nazm build --lib --shared`: only the exports are visible (`nm`), named `@rpath/NAME`, linked and run by a C program under both backends; a failure in an export exits 2 (`a_shared_library_links_into_a_c_program_and_shows_only_its_exports`). Linux's `-shared` link is the same command, not run here |
| Bindings (N85) | VERIFIED | `nazm bindgen` reads incomplete structs and `typedef struct S S;` as handles, complete structs of crossing fields as C structs, `const char *` results as `Option[Str]`; arrays, unions, by-value structs, function pointers and buffers refused by name (`bindgen_reads_handles_structs_and_returned_strings_and_names_what_it_refuses`) |
| Outside the subset | Refused by name | floating point, integers other than `int64_t`, variadics, by-value structs and struct results, unions, arrays, bit-fields, closures as callbacks, out-buffers, `dlopen`, ABIs other than `"C"` |
| Other ABIs and cross-target | MISSING | `"C"` only; a foreign call builds for every target of the matrix, and runs on the host only (area 33) |

**Limitation** · The subset above, and only it. C is trusted entirely once called: an abort, a loop,
a use of a freed handle or memory corruption inside it is outside every Nazm guarantee, and nothing
but a promised non-null is checked after the call. `errno` is per thread: a pool task that blocks
between a call and `c_errno()` may read another task's. `nazm run` cannot call C, so a program that
does has one implementation, not two. Shared libraries are linked and run on macOS here; the
self-hosted compiler reads none of N85's declarations (area 30).

**Next dependency** · By-value structs and struct results need each target's register rules for
aggregates; floating point needs the language to have it; closures as callbacks need a `void *`
environment rule a C API states; `dlopen` would need a handle whose lifetime bounds every function
taken from it.

**Accepted when** · Met, as scoped (N85): a record and a string cross with their layout and
ownership stated, in both directions for strings, under both backends, with every shape outside the
subset refused by name.

---

## Domain profiles

`NAZM_LANGUAGE_GOALS.md` §10 owns what each profile is *for*. `master-architecture.md`
§4 owns the invariant that makes them profiles rather than dialects, and the ladder a
profile has to climb to become real. **This section owns what exists**, which is: one
profile, unnamed, and five ambitions.

**General — PARTIAL.** What `nazm run` and `nazm build` accept today: ambient authority,
checked arithmetic that traps, structured concurrency over OS threads, and automatic
reclamation of every heap-backed value with no annotation to write. *That last clause read
"no reclamation" until N8.* It
is the widest profile and every other is a restriction of it. PARTIAL rather than VERIFIED
for one reason: **the interpreter's subset and the native subset are not the same** —
recursion ran under `nazm run` and was refused by both compilers until N49; since, the reference
compiler builds it and the compiler written in Nazm still refuses it, and the interpreter's
iteration budget remains its own. A profile whose implementations accept different programs is
not yet a profile. *Accepted when* the two
subsets coincide, or the difference is itself declared as a profile boundary rather than
left as a gap.

**Systems — MISSING.** Would need explicit control over allocation and layout, no hidden
allocation where none is declared, and a stated ABI; would grant raw memory access and
foreign calls inside a named, narrow boundary. Nothing profile-specific exists. Blocked on
area 17 (FFI and ABI, PARTIAL: scalars only) and on area 4, which is now VERIFIED for the current type
universe but says nothing about *explicit* control over allocation and layout — a profile
that must declare where memory comes from needs more than automatic reclamation. The architectural
commitment that keeps it reachable — field access stays symbolic until the lowest level, so
layout selection remains possible — is already honoured and must be preserved rather than
undone when this profile arrives.

### 18. Embedded support — VERIFIED as scoped (two emulated boards, N86)

**N86: boards as data, a second architecture — VERIFIED, emulated.** `architecture.md` §7.87 first.
A board is a description (`nazm_runtime::board::BOARDS`, printed by `nazm inspect` as
`toolchain.boards`), and its linker script, runtime and entry are generated from it alone
(`every_board_s_texts_are_generated_from_its_description_and_differ_where_it_does`). The second
architecture family, `riscv64gc-unknown-none-elf`, boots on QEMU's RISC-V `virt` machine; device
registers have 8-, 16- and 32-bit accesses. The published support matrix:

| Board | Build | Link | Boot (QEMU) | Failure path | Stack: measured ≤ bound | Hardware |
| --- | --- | --- | --- | --- | --- | --- |
| `aarch64-unknown-none`, QEMU `virt` Cortex-A53 | host or container | `ld.lld` | `-O0`, `-O2` | `N0400`, `N0408`, status 2 | 176 ≤ 176, 32 ≤ 48 bytes | no |
| `riscv64gc-unknown-none-elf`, QEMU `virt` RV64GC, M-mode | container only (needs a RISC-V `clang`; refused by name elsewhere) | `ld.lld` | `-O0`, `-O2` | `N0400`, `N0408`, status 2 | 152 ≤ 160, 32 ≤ 48 bytes | no |

Evidence: the four tests of `crates/nazm-cli/tests/boards.rs` on the host, and its ignored
`both_boards_boot_and_agree_under_qemu`, run in `nazm-qemu:n86` — `nazm` built from the tree in the
container, then each board's programs at `-O0` and `-O2`: identical UART output (`HI` and `100`, the
byte write truncating `0x149` to `I`), the overflow and the exhausted stack each reported with
status 2; with the stack painted, every measured use within the stated bound
(`docs/performance.md`, N86). **Not claimed**: hardware; atomics and memory orderings, interrupts
and vectors, a heap or arena (each DESIGNED in §7.87, with the reason it is not built); `.bss`
zeroing by a loader other than QEMU's; RISC-V on a macOS host's Apple `clang`, which has no RISC-V
code generator.

**N62: a freestanding target — VERIFIED on QEMU's `virt` board, one board.** `aarch64-unknown-none`
builds to objects and `link.ld`, links with `ld.lld` and boots under `qemu-system-aarch64`: no OS, no
C library, no heap; device registers through `mmio_read32`/`mmio_write32` under an `MmioCap`. The
evidence and its limits are in area 33's N62 paragraph. Atomics, interrupts, a heap and `@std` are
DESIGNED only (§7.64); no hardware was run.

**N63: real-time bounds — PARTIAL, an analysable subset.** `architecture.md` §7.65 first. The
`realtime` profile adds `no-blocking` and `bounded-loops` to `embedded`'s rules and `no-ambient-time`;
the profile report states each loop's exact trip bound for the counted form, the call depth and the
site counts, with `wcet` `null`. A board build states a static stack bound from clang's own frame
sizes along the image's deepest call path (`bounds.json`). Evidence:
`crates/nazm-cli/tests/realtime.rs` (9 + 1 ignored) — bounds against hand-computed trip counts,
including the widest `Int` span; every unbounded shape refused as `unknown` with its reason; the
board's bound equal to its path's frames and the same twice; recursion `null`; the search's own
four unit tests (`crates/nazm-cli/src/stack.rs`). **Run-checked**, the ignored test in
`nazm-qemu:n62`: three programs at `-O0` and `-O2`, the stack painted, the measured use below the
stated bound in every run (176/192, 304/320, 264/288 bytes at `-O0`; 24/48 at `-O2`). **Not
claimed**: WCET, latency or jitter bounds, interrupt nesting, certification; the loop form is one
form.

**What it would restrict** · No allocation after startup, or none at all; a stack bound
that is computed;**What it would restrict** · No allocation after startup, or none at all; a stack bound
that is computed; no I/O except through declared capabilities; a freestanding target with
no libc.

**Evidence of absence** · No cross-compilation, no target triple handling, no freestanding
target, no `no_std` equivalent, no control over allocation. `crates/nazm-cli/src/build/`
emits a module with no triple and lets `clang` supply the host's. The emitted runtime
declares 18 libc and pthread symbols and cannot currently be built without them.

**N47: the `embedded` profile — PARTIAL.** *N47, 2026-10-01: restriction profiles v1 (`architecture.md` §7.49).* `--profile NAME` on `check`,
`build` and `run`, or `profile = "NAME"` in any package manifest of the build; rules read semantic
facts only — `main`'s required effect set, each function's declared contract, the build's lock —
and refuse with `N0510` and witnesses; `nazm check --profile-report` prints the deterministic
`nazm.profile-report/1`, marked compiler evidence and not certification. Evidence:
`crates/nazm-cli/tests/profiles.rs` — every profile's verdicts on one program, the chain of calls
in a refusal, the ambient bridge refused, foreign calls refused, `io` seen through a standard
wrapper, a locked package build satisfying the build rule and a tampered one not, a dependency's
required profile holding the whole program, a profile never changing what runs nor its verdict
being cached, and the report's bytes and outcome. `embedded` refuses `io` and `spawn` —
no operating-system services, no scheduler — and allows `foreign`, the way hardware is reached.
It does **not** bound the stack, and there is still no freestanding target, so a program it
accepts still links the host's C library. *N52, 2026-10-02:* allocation is no longer untracked —
`embedded` gains `bounded-allocation`, refusing an allocation site whose count per call is
`unknown` (inside a loop), read off MIR by `nazm_mir::cost`; a per-byte bound is not claimed
(`profiles_read_resource_and_flow_facts`).

**Next dependency** · Cross-compilation (area 33), then a freestanding runtime — which
means area 13. *This clause added "and therefore area 4" with the sentence "A language that
never frees cannot run on a device with kilobytes." The first half of that is closed since
N8; the second is not, because the profile's real requirement is "no allocation after
startup, or none at all", and reclaiming what you allocate is not the same as not
allocating.*

**Accepted when** · A program runs on a target with no libc, within a stack bound that was
computed rather than hoped for. Native frame counts do not bound stack bytes, and that
obligation is inherited here.

### 19. Critical profile — VERIFIED as scoped (enforcement and evidence, N87; not certification)

**N87: contracts and the obligation census — VERIFIED, as enforcement and evidence.**
`architecture.md` §7.88 first. `requires` and `ensures` clauses, checked as a function is entered and
on every return path (its tail, `return`, `?`) identically by `nazm run` and both backends (`N0410`,
`N0411`), never compiled out; a call of literals proved or refused at compile time (`N0615`); a clause
only of what cannot fail for want of authority or memory (`N0614`). `nazm obligations` prints
`nazm.obligations/1`: every contract clause, call of a function with a precondition, overflow,
division, index, allocation, call through a function value and call into C, each `proved` (with its
witness), `checked` as the program runs, or `unknown`. `critical` adds `no-unknown-calls`, so with
`no-foreign` every obligation of an accepted program is proved or checked. Evidence:
`crates/nazm-cli/tests/contracts.rs` (11) — the three tiers agreeing on holds and failures, a `?`
leaving through a postcondition, a `Str` result reclaimed, a failing clause failing the call, the
literal proof and refusal, the clause shape, the census's exact rows and bytes, the profile rule, a
callee's preconditions reaching an importer through `nazm.interface/10` and the cache, the formatter;
`examples/contracts/` (an integer square root and a ledger). **Not claimed**: loop invariants or
ranges as declarations, any proof beyond evaluating a literal call, absence of runtime error beyond
the census's own `proved` entries, and certification against any standard — the next paragraph stands.

**Read this before the rest.** This profile is an architectural intention. It is **not** a
claim that Nazm is suitable for any safety-related, airborne, medical, automotive or
otherwise regulated use; it is not a certification path; and no artefact in this repository
has been produced under a safety standard or assessed against one. **The architecture
anticipating a Critical profile is not evidence for one.**

**What it would restrict** · No dynamic allocation, no recursion, bounded loops with an
established bound, total functions, and every failure mode enumerated rather than trapped;
contracts and invariants checked rather than asserted.

*N97, 2026-10-04: formal semantics v2 — PARTIAL (bounded, machine-checked; proofs BLOCKED).*
`architecture.md` §7.98 first. `nazm.formal-core/2` (`docs/formal-core.md` §2,
`crates/nazm-formal/src/statements.rs`): `let mut`, assignment, `while`, `if` statements, `return` and
one recursive function over `Int`, big-step with fuel. Over all 12,900 programs of a stated bound
(`crates/nazm-formal/tests/statements.rs`): never stuck, deterministic, accepted by the checker, and
for the 11,137 the model finishes — 3,194 of them traps — the interpreter's value or trap is the
model's; 1,763 run out of fuel and are not compared. **Proofs BLOCKED**: no proof assistant or solver
is installed and none can be installed offline, so progress, preservation, effect soundness and flow
properties are not proved. Phase B (ownership, effects, flows) and Phase C (concurrency, refinement)
are not modelled. Two mutants of the model's own rules, each caught by the correspondence.

*N61, 2026-10-02: a formal core — PARTIAL (bounded, machine-checked; no proof).* `docs/formal-core.md`
states integers and booleans with traps, comparison, short-circuit `&&`, `if` and `let` by typing and
big-step rules; `crates/nazm-formal` transcribes them with no dependency on the compiler, and
`crates/nazm-formal/tests/exhaustive.rs` checks five properties over **every** program of the core
up to five nodes — 94,352 programs, 28,238 well-typed (27,680 before R1a made `<` on `Bool` well typed): soundness (never stuck, the right type),
determinism, checked arithmetic against 128-bit integers, the interpreter's outcome equal to the
rules' for every typed program, and the checker accepting exactly what the rules type. The check's
first run found an error in the formal transcription, not the compiler — `i64::MIN % -1` is 0 by
the spec, and the transcription trapped — which is the correspondence working in both directions.
No proof assistant, nothing beyond the bound, no function, loop, string, effect or backend in the core.

*N60, 2026-10-02: restriction profiles v2 — VERIFIED for the rules named.* `architecture.md` §7.62
first. A verdict is `pass`, `fail` or `unknown`, `unknown` refused and reported as such.
`embedded` adds `no-recursion` — a cycle of MIR's direct call and spawn edges, each function in it
named; `unknown` where a call goes through a function value — and `critical` adds `no-select`, by
call site, so with `no-spawn` and `no-ambient-time` a `critical` program's behaviour is a function of
its inputs. Composition: a dependency declaring `general` does not weaken a root requiring
`embedded`. Evidence: `crates/nazm-cli/tests/profiles_v2.rs` (4), the profile tables in
`profiles.rs`; overhead on `compiler/emit.nz`: `general` 652, `critical` 653, `embedded` 739 ms
(medians of five). Constant-time rules are RESEARCH; bounded loops, queues and tasks are N63's.

**What `--profile critical` enforces now** · *N47, 2026-10-01: restriction profiles v1 (`architecture.md` §7.49).* `--profile NAME` on `check`,
`build` and `run`, or `profile = "NAME"` in any package manifest of the build; rules read semantic
facts only — `main`'s required effect set, each function's declared contract, the build's lock —
and refuse with `N0510` and witnesses; `nazm check --profile-report` prints the deterministic
`nazm.profile-report/1`, marked compiler evidence and not certification. Evidence:
`crates/nazm-cli/tests/profiles.rs` — every profile's verdicts on one program, the chain of calls
in a refusal, the ambient bridge refused, foreign calls refused, `io` seen through a standard
wrapper, a locked package build satisfying the build rule and a tampered one not, a dependency's
required profile holding the whole program, a profile never changing what runs nor its verdict
being cached, and the report's bytes and outcome. `critical` requires every function of the
program to declare its effect set (no ambient bridge), forbids `spawn` and `foreign`, and requires a
package build held to its lockfile. Nothing about allocation, loop bounds, totality or contracts:
those stay DESIGNED, and the disclaimer above stands in full.

**What exists that is genuinely relevant** · Three things, and they are real. Arithmetic is
**specified rather than inherited from the hardware** — overflow traps, and
`crates/nazm-lir/src/op/llvm.rs` refuses `nsw` so no optimiser may delete the check. Recursion was
refused by both compilers until N49; it now compiles with a stack guard (`N0408`), so a profile
that wants it refused would need a rule of its own (none exists yet). Everything outside a backend's coverage is **refused by
name with a span**, never silently approximated or partially compiled.

**Missing prerequisites** · A memory model; effects; contracts; a bounded-stack story; and
a qualified toolchain, which the bootstrap explicitly does **not** provide — `docs/bootstrap.md`
§4 puts compiler trustworthiness on the list of things a fixpoint does not establish.

**Accepted when** · A proof obligation is discharged on a real Nazm program and assessed
against a named standard by someone qualified to make that assessment. Not a test count.

### 20. Cybersecurity profile — VERIFIED as scoped (enforcement, N87; not certification)

**N87: `cyber` v3.** `cyber` adds `no-unknown-calls` to declared effects, no C, a locked build,
contents kept in files (N52) and checked paths (N80): no call reaches a callee, or a contract, the
compiler does not know. Every obligation of an accepted program is proved or checked
(`critical_and_cyber_refuse_an_obligation_of_unknown_status`). **Constant time stays RESEARCH**: the
one subset precise enough to state needs secrets in the type system, which provenance tracks only to
its sinks (§7.88). Dynamic loading has no construct to refuse (§7.86). Not a sandbox, and not
certification.

**What it would restrict** · Untrusted input tracked to its uses; declassification
explicit and audited; no ambient authority; constant-time obligations honoured where they
are declared.

**Evidence** · The one real ingredient present is refuse-by-name-with-a-span rather than
silent degradation, and failures that are diagnostics rather than undefined behaviour.

**N47: `--profile cyber`** · *N47, 2026-10-01: restriction profiles v1 (`architecture.md` §7.49).* `--profile NAME` on `check`,
`build` and `run`, or `profile = "NAME"` in any package manifest of the build; rules read semantic
facts only — `main`'s required effect set, each function's declared contract, the build's lock —
and refuse with `N0510` and witnesses; `nazm check --profile-report` prints the deterministic
`nazm.profile-report/1`, marked compiler evidence and not certification. Evidence:
`crates/nazm-cli/tests/profiles.rs` — every profile's verdicts on one program, the chain of calls
in a refusal, the ambient bridge refused, foreign calls refused, `io` seen through a standard
wrapper, a locked package build satisfying the build rule and a tampered one not, a dependency's
required profile holding the whole program, a profile never changing what runs nor its verdict
being cached, and the report's bytes and outcome. `cyber` requires declared effect sets everywhere — so no
ambient authority, and N38's restricted flow (`N0372`) applies to every function — forbids
`foreign`, and requires a locked, integrity-checked package build. Capabilities (area 6, N37) and
information flow (area 7, N38) are what it rests on. It is not a sandbox, it does not make a
program secure, and it claims nothing about side channels, constant time or declassification.

**Limitation** · Untrusted input is tracked only to `write_file` paths (N38); there is no
declassification and no constant-time rule.

**Next dependency** · More sinks and an explicit declassification, in N38's framework.

**Accepted when** · Untrusted input is tracked to its uses and declassification is explicit
and audited, with the approximation's blind spots stated.

### 21. AI/HPC — PARTIAL

*N88, 2026-10-04: maps of one or two sequences, reductions, a numeric oracle — VERIFIED on one GPU;
the area stays PARTIAL.* `architecture.md` §7.89 first. A kernel is `(Int) -> Int` (a map) or
`(Int, Int) -> Int` (a zip over two inputs of one length); `--reduce add|min|max` folds the results
in index order on the host, `add` checked as `ints_sum` is. Every run held to the interpreter —
results and fold. Evidence, `crates/nazm-cli/tests/accel.rs`: on any host, the zip's two-buffer
kernel and its refusals before a device (arity, unequal lengths); on the M1 Pro (ignored, run by
hand), a zip and each reduction agreeing with the interpreter and with an independent computation,
`add` failing at the running sum's overflow, and **40 generated kernels** over `+ - * / %`, `if`
and literals at `Int`'s edges, 200 elements each, agreeing with the interpreter and with the spec's
arithmetic in 128 bits — values, or the lowest failing index and its code. **Why still PARTIAL**: one
provider, OpenCL on macOS, deprecated by its vendor — a second (Metal, SPIR-V) is BLOCKED here for
want of a toolchain (no Metal compiler, no Vulkan loader), not of a design; vector operations in LIR
are DESIGNED, the one vectorised loop (N66) still a pattern; and the measured workload (N88,
`performance.md`) is one a native CPU loop serves faster than the device and its transfers, so the
row's acceptance — a workload a CPU cannot serve — is unmet.

*N67, 2026-10-02: accelerator kernels — VERIFIED on one GPU, OpenCL on an Apple M1 Pro.*
`architecture.md` §7.69 first. `nazm accel FILE KERNEL --input FILE` runs a pure `(Int) -> Int`
function — and the functions it calls — as an OpenCL C kernel generated from Core IR, over up to
8,000,000 `Int`s; checked arithmetic is carried, and a failure is the lowest failing index's at its
site, as a sequential map's. Ineligible functions are refused by name before any device (`N0394`).
`nazm.accel/1` names the device, both transfers with their bytes and time, the one
synchronisation, the build and run times, the kernel's identity (`nazm.kernel/1`), and whether the
results agree with the interpreter's, which they are always held to unless `--no-check`. Evidence,
`crates/nazm-cli/tests/accel.rs` (3 on any host — the kernel's checked operations and failure rule,
six refusals, bad input; 2 ignored, run on the M1 Pro — 20,000 Collatz kernels agreeing with the
interpreter and with an independent Rust computation, transfer bytes, identity stable and distinct;
the lowest failing index for overflow and division by zero, `i64::MIN` edges). Measured
(`performance.md`, N67): 1,000,000 elements in 23–30 ms on the GPU plus 3–6 ms of transfers, against
190 ms on one CPU core. **Limitation**: one API on one vendor's GPU, deprecated by that vendor; maps
of `Int` only; no kernels in the language; no layout specialisation or occupancy; Linux and CI cannot
run it.

**Evidence of absence (before N66)** · No tensors, no GPU path, no vectorisation work, no measurement.

**Next dependency** · A reason. `architecture.md` §1 says MLIR should be revisited *only
if* Nazm pivots toward tensor/accelerator codegen, and nothing proposes that pivot.
Treating this as a profile rather than a declined direction is itself the open question,
tracked as R7 in `research-register.md`.

**Accepted when** · A workload the project actually has, that a CPU backend cannot serve.

---

## Tooling

### 22. Diagnostics — VERIFIED

**Evidence** · `crates/nazm-diag/src/lib.rs` — the stable codes (33 when this row was written, at N31; far more since — `docs/diagnostics.md` lists them), byte-offset spans, a
versioned JSON schema `schema/nazm.diagnostic-1.json` emitted per object. Conformance is
tested against real binary output in `crates/nazm-cli/tests/schemas.rs`, and
`crates/nazm-cli/tests/codes.rs` requires every declared code to be provoked by a test or
listed as unprovokable with a written reason. The capability inventory
(`crates/nazm-cli/src/capabilities.rs`) is read from the compiler's own tables, and
`docs/diagnostics.md` states the compatibility contract.

*N30, 2026-09-28: Compact Diagnostic Index v1, `nazm.diagnostic-index/1` — VERIFIED; Diagnostic
Detail v1, `nazm.diagnostic-detail/1` — VERIFIED; G74, G75, G77 — PARTIAL.* An index
(`crates/nazm-service/src/diagnostics.rs`, `architecture.md` §7.32) is every current diagnostic
of a root compilation as compiler-owned facts — `code@file:start-end#n` id, code, severity, file
and UTF-8 byte range, owning durable definition, fix counts by applicability — with no prose,
bound to a digest of the loaded sources and, where N28 has one, the snapshot; detail is one
diagnostic exactly as `nazm.diagnostic/1` publishes it, its places in their files, and for each
fix the N29 selector where a semantic patch is plannable. Compact diagnostics never infer
structured semantics from diagnostic prose; a compiler fix and an N29 semantic patch are distinct
capabilities. The goals are partial because no diagnostic carries typed expected/actual/entity
facts (`facts: "unavailable"`), only the compact and detail levels exist, and there is no
diagnostic history. Evidence: `crates/nazm-service/tests/diagnostics.rs` — an index with no
sentence the compiler wrote in it, ordered by place, owned by `main`; each detail the canonical
diagnostic with the index's code, place and owner; lexer and parser errors, a missing `;`, an
unterminated string and a mutable parameter indexed with no snapshot and no owner, their compiler
fixes no semantic patches; an automatic and a needs-review fix counted by applicability, each
detail's selector planning exactly that patch through N29; one code twice in one file and across
files as distinct ids, `lib.nz`'s places its own and its published offsets the concatenation's; a
`+` after `é😀` at its byte offset, not its character index; a fake id refused, and after a move, a
repair or another state every old id `stale_state`; byte-identical indexes from another directory,
an edit in an imported file a new state, a broken file nothing imports absent, and two roots over
one file each their own; unknown name, type mismatch, arity, visibility, a refused duplicate
(no owner), a generic mismatch, an unsatisfied requirement and a task capture each indexed with
its owner and detail; and 500 index and detail pairs leaving retained state unchanged. The unit
tests tell two structurally identical diagnostics apart by ordinal alone and show rewording a
diagnostic changes no index byte. `crates/nazm-cli/tests/schemas.rs` the real command's indexes
and details valid against their schemas, every detail's diagnostic equal to a line of `nazm check
--json` and valid as `nazm.diagnostic/1`. `crates/nazm-mcp/tests/protocol.rs` the real server:
six read-only tools, index and detail equal to the service's, the index carried once, no root or
file argument accepted, a stale id after an edit, syntax errors indexed, and 1,000 calls through
edits and repairs with flat resident memory. Eighteen N30 mutations (area 30).

**Limitation** · *At N31:* two codes were unprovokable by construction, and spans were offsets
into a merged buffer, so file identity was recovered rather than carried. *Since:* one code is
unprovokable, and a span carries its file (`FileId`, area 2) — published spans are offsets into the
files' concatenation; the N30 index and
detail recover it, by the same source map. No diagnostic carries typed facts, and the index is a
module's analysis — a missing `main` (`N0204`) is `nazm check`'s, not the index's.

**Next dependency** · None.

**Accepted when** · Met.

### 23. Formatter — VERIFIED

**Evidence** · `crates/nazm-syntax/src/format.rs`, token-driven so comments survive, with
no options by design. `crates/nazm-syntax/tests/format.rs` asserts on every input that
tokens are unchanged, that `format(format(x)) == format(x)`, that comments are identical,
and that output ends in exactly one newline — applied to every `.nz` in the tree and to 15
hand-written ugly inputs.

**Limitation** · No reflow, no reordering, no insertion. Per file, not per merged program.
Canonical in the sense of "one output". Since N15 a lossless tree exists (area 1), and the
formatter reads its comments from the same scan the tree is built from, but it still lays
out from tokens rather than from the tree.

**Next dependency** · None for formatting; edits are a separate item.

**Accepted when** · Met for formatting.

### 24. Machine-applicable fixes — VERIFIED

**Evidence** · `crates/nazm-cli/src/fix.rs`, schema `schema/nazm.fix-1.json`. Only
`Automatic` fixes apply; nothing is written without `--apply`; edits apply right-to-left;
overlaps are skipped; and a result that no longer parses is rolled back wholesale. Ten
workflow tests in `crates/nazm-cli/tests/workflow.rs`. *N24, 2026-09-26:* the code has
**seven** skip reasons, not the four this entry used to count, and each is now reached by its
own unit test in `fix.rs` — `write_back` reads and writes through injected functions, so an
unreadable and an unwritable file are arranged rather than hoped for: a fix that needs review
(`it needs review`), an unreadable file, two overlapping edits (the later applied, the earlier
skipped, and exactly the one written), a span past the end and one inside a character (`the
span does not name a range of this file`), bytes that are no longer the ones checked (nothing
written), a result that does not parse (nothing written), and a failed write (not reported
applied). Each asserts the exact `skipped` text. `docs/diagnostics.md` lists the seven. The
same fixes reach an editor as quick fixes (area 25), a distinct consumer of one fix contract.

**Limitation** · Stale-edit protection is by expected-bytes comparison, not a file hash or
document version — which is what a command that reads and writes a file in one run needs.

**Next dependency** · None.

**Accepted when** · Each skip reason has a test that reaches it: met, 2026-09-26 (N24), for
the seven the code has.

### 25. LSP — PARTIAL · MCP — PARTIAL

**Evidence** · *N16, 2026-09-25.* `nazm lsp` serves the Language Server Protocol over stdio
(`crates/nazm-cli/src/lsp.rs`) as an adapter over a protocol-independent language service
(`crates/nazm-service/src/service.rs`, `architecture.md` §7.18) that answers from the loader,
parser, checker and resolution every command uses — nothing is re-implemented, and the
layering and `one resolver` gates keep it so. Open buffers are the source: they override the
disk for every compilation, an unsaved dependency is what its importer sees, closing a buffer
gives the file back to the disk, and an unsaved `use` changes the graph. Diagnostics keep
their Nazm codes and are published per version, an empty set included. Definition follows the
resolution's identities — calls, locals (by slot), record constructions, variants, fields and
payload fields — across files; hover shows a function's declared signature or a local's type,
from the same resolution. Positions are converted between byte offsets and UTF-16 at the
adapter boundary only. Evidence: `crates/nazm-service/tests/service.rs` (overlays, close,
import edits, versions, a broken buffer never answering from the clean one, every definition
category, a spelling with three meanings, the prelude, the cursor rule, the service's
diagnostics equal to the command line's over every example and multi-module conformance
program, a thousand edits with constant retained state); `crates/nazm-cli/tests/lsp.rs` (the
real process over real framing — handshake, exact capability set, diagnostics and definition
in UTF-16 after `é→😀`, an unsaved dependency and its close, unimplemented methods refused,
malformed source and a malformed frame without a panic, stdout nothing but frames); the
position unit tests in `lsp.rs`. Fourteen N16 mutations were caught with verified killers
(area 30).

*N17, 2026-09-25: references — VERIFIED as a sub-capability.* `textDocument/references` and
`LanguageService::references` answer from one reference index per analysis
(`crates/nazm-sema/src/references.rs`, `architecture.md` §7.19): the resolution read backwards,
with no name resolved again. Functions, locals (parameters, `let`s, pattern bindings, by slot
of a function), records (their constructions), variants, fields (projections, assignments and
construction labels) and payload fields; across files and across every open compilation that
loads the declaring file; from unsaved buffers; declaration apart from uses, combined only for
`includeDeclaration`; ordered by file and position, each occurrence once. Evidence:
`crates/nazm-service/tests/references.rs` — one spelling given to a function, a parameter, two
shadowing `let`s, fields of two records, variants of two enums and private functions of two
modules, each its own set asked from every one of its occurrences; definition and references
agreeing at every byte of every file; shadowing; a generic record's field through two
instances; the unsaved program (a use added, removed, moved to another same-named entity, and
the disk back on close); a broken buffer; only analysed programs searched; unsupported
positions; the prelude; a thousand edits with constant retained state; and, over 40
compilations of the repository's own source (the compiler, the conformance programs, the
examples), every resolved name token in exactly one entity's set and the service's answer equal
to the command line's resolution read backwards. `crates/nazm-cli/tests/lsp.rs` adds the real
process: the capability, UTF-16 after `é→😀` on both the query and the answers, an unsaved
edit, and the same answer with a warm semantic cache beside the sources and without one.
Sixteen N17 mutations, all caught at tier 1 (area 30).

*N18, 2026-09-25: rename — VERIFIED for locals and private definitions only.*
`textDocument/prepareRename` and `textDocument/rename` over `LanguageService::rename`'s
validated plan (`crates/nazm-service/src/rename.rs`, `architecture.md` §7.20). Renameable:
parameters, `let`s and pattern bindings; and private functions, records, enums, and the fields,
variants and payload fields of private records and enums. The proof of completeness is the
language's own: a local is written only in its body, and a private entity only in its module
(`N0337` keeps private types out of public signatures), in a clean analysis whose every written
type name, qualifier, label and use the checker now records — type names and enum qualifiers
since N18. Every plan's candidate program is re-checked in memory in every open compilation
containing the edited file and must partition its occurrences into entities exactly as before.
Refused: exported entities (their importers cannot be known), core-prelude entities, built-ins
and type parameters, programs with errors, a name the lexer does not read as one identifier,
and every collision or capture the checker or the partition finds. The protocol edit is
versioned `documentChanges` only, only for a client that accepts them, and only when every file
the plan edits is open. Evidence: `crates/nazm-service/tests/rename.rs` — a type rename through
declaration, parameter, return, field, payload, nested generic argument, construction and
qualifiers, stated as the exact edited text; one rename from any occurrence; definition on a
type annotation and on a qualifier; same-spelled locals, fields, variants and a record and a
function of one name renamed apart; an enum, its variant, its payload field and the pattern
local bound to it, apart; capture and collision refused and legal shadowing accepted; built-in
names; invalid names; non-entities and the prelude; a program with errors; private accepted and
exported refused across modules, and a private rename validated in the importer's compilation;
stale plans by generation, version and text; a thousand plans with constant retained state.
`references.rs` adds the corpus gate — every name token of every clean compilation of the
repository indexed or explicitly a non-entity. `crates/nazm-cli/tests/lsp.rs` adds the real
process: the capability, prepare-rename's range and placeholder, refusals as `null`, a versioned
edit with every range in UTF-16 after `é→😀`, an unsaved edit's new version, refusals with
reasons, and a client without versioned edits refused. Fourteen N18 mutations, all caught at
tier 1 (area 30).

*N19, 2026-09-25: completion — VERIFIED for identifier completion in value, call-head and type
contexts only.* `LanguageService::scope_at` answers what may be written at any position, and
`completion` and `textDocument/completion` complete the identifier under the cursor, from a
`ScopeTrace` the checker writes as it pushes scopes, defines bindings and builds environments
(`crates/nazm-sema/src/scope.rs`, `architecture.md` §7.21) — with no lookup of its own. Values
are locals (parameters, `let`s, pattern bindings), visible from where the checker introduced
them and hidden where it recorded a nearer binding hiding them; calls are the module's
functions, its direct imports' exports and the built-ins (`Intrinsic::ALL`); types are type
parameters in their own definition, the module's and its direct imports' records and enums,
and the built-in types (`Type::ALL`, `Vec`). A name the checker refused (an ambiguous import, a
collision, a same-scope duplicate) is offered as nothing. A position whose function needed
syntax recovery has no locals, and a compilation with any recovery has no function or type
completion. Invoked only (no trigger characters), `resolveProvider` false, `isIncomplete`
false. Evidence: `crates/nazm-service/tests/completion.rs` — one spelling as a record, a
function, a parameter and a shadowing `let`, each offered only in its own context and scope;
later locals absent, a `let` absent from its own initializer, loop, branch, `scope` and arm
bindings not leaking, sibling arms apart; a later top-level function callable; type parameters
in their own definition only; built-ins from the compiler's inventories and none spelled in the
service; direct imports' exports only — no private, no transitive; an ambiguous import offering
neither; another open program never mixed in; unsaved locals, imports and dependencies, and the
disk back on close; recovery refused, type errors not; non-sites refused; a thousand edits with
constant retained state; and over the repository's own clean compilations every one of 23,821
resolved names offered, once, as exactly what it resolved to. `crates/nazm-cli/tests/lsp.rs`
adds the real process: the capability, the whole identifier replaced in UTF-16 after `é😀`, a
built-in call head by prefix, a string refused, an unsaved local. Fourteen N19 mutations (area
30).

*N20, 2026-09-26: signature help — VERIFIED for calls of user functions (the module's own and
imported ones), generic instantiations (written and inferred) and built-ins, and for `spawn`.*
`LanguageService::call_at` and `signature_help`, and `textDocument/signatureHelp`, report the
call whose argument list holds the position from a `CheckedCall` the checker records wherever
it checks a call against a resolved callee (`Resolution::checked_call`, `architecture.md`
§7.22): the callee, the argument spans, the parameter and return types the environment gave —
an import's from its interface — and what a generic call settled. The tree decides only which
argument list holds the position (innermost by nesting) and which argument (that list's own
commas). Too few or too many arguments and wrong argument types still answer; an unresolved
callee, a record or variant construction, a position outside any argument list and a
compilation with any syntax recovery do not. One `Signature` and one renderer serve hover,
completion and signature help. Triggered by `(` and `,`, no retrigger characters, one
signature, parameters as UTF-16 label offsets; `null` past the last parameter; no active
parameter for a callee with none. Evidence: `crates/nazm-service/tests/signature.rs` — a record,
a function, a local and another module's private function sharing a spelling, each call shown
as the function it resolved to (by declaration, through the reference index); a generic call
inferred and written, with its settled arguments; a built-in; `[T: Equality]`; a zero-parameter
call; a construction refused; twenty-two positions across nested calls, strings, comments,
parenthesised arguments, constructions and nested type arguments, each with its exact active
parameter; too few, too many, wrong types, an unresolved callee, a `spawn`; a stale version, a
broken and a repaired source, a recovery elsewhere in the file, an unsaved and then closed
dependency signature, another open program; a thousand edits with constant retained state and
checked-call count; hover and signature help rendering one signature; a structural test that
the module looks nothing up and substitutes nothing; and over the repository's own clean
compilations 9,030 calls (71 generic) whose record is the resolution's callee with the
declaring signature and the backend's type arguments, 4,814 of them asked of the service.
`crates/nazm-cli/tests/durable_identity.rs` adds a call checked against a deserialised interface
with the dependency deleted, recorded with the interface's signature. `crates/nazm-cli/tests/lsp.rs`
adds the real process: the capability, label offsets, a comma inside a string after `😀`, a
nested built-in, no active parameter, `null` past the end and outside the call, an unsaved edit.
Fourteen N20 mutations (area 30).

*N21, 2026-09-26: structure completion and constructor signature help — VERIFIED for member
fields after `.` (variables, temporaries, chains, generic records), enum variants after `E.`
(in a construction, a pattern, or before its `(` for a non-generic enum), record construction
labels, variant construction payload labels, pattern payload labels, and record and variant
constructor signature help.* `LanguageService::structure_at`, the structure contexts of
`completion`, and `help_at` answer from what the checker recorded — a field's `FieldRef`, a
variant's `VariantRef`, a construction's record, a payload's `PayloadRef` — and from one narrow
checker fact, `Resolution::looked_up_on`, the record or enum an unresolved member name was
looked for on, written only where a lookup failed (`architecture.md` §7.23). The service and
the LSP adapter perform no name lookup and no semantic re-resolution; normal structure sites
consume checker-recorded identities. The only spelling-based lookup N21 introduced is the
canonical checker's incomplete-source anchor for `E.name` written before its `(`: after `E`
fails as a value, the checker consults its existing module type environment and records a
non-generic enum (§7.23). No diagnostic and no language semantics changed. Candidates are that record's fields, that enum's variants or that variant's payload
fields, in canonical order, with substituted types; labels given elsewhere are excluded by
identity and the label being edited is kept. Constructor signature help is its own
`Constructor`, never a call; its active field is its label's identity at its canonical
position. Answered around a recovery only when it is inside another function's body.
Evidence: `crates/nazm-service/tests/structure.rs` — two records sharing `value`, two enums
sharing `Ready`, two variants sharing a payload `value`, a record and a function sharing `Box`,
a temporary receiver, a chain through `Holder[A]`, `Pair[Int, Str]` and `Opt[Int]` substituted,
each candidate identified by its declaration; a pattern binding refused as a label; labels given
elsewhere excluded, the edited label kept, an unknown label offered what is not given; an
unknown field, an unknown variant and `EA.Rea` with no `(` offered their anchor's members, and
unknown receivers, constructors and enums refused; constructor help out of canonical order,
between initialisers, on an unknown label, around a nested call, nested constructions, generics,
variants, a construction inside a call; reordered fields, payloads and variants changing no
answer; recoveries in another function answered and in the same function, the top level and
another file refused; an unsaved and closed dependency, a private dependency record refused,
another open program; a thousand edits with constant retained state; and over the repository's
own source 518 resolved sites — 278 members, 126 variants, 73 construction labels, 31 pattern
labels — each offered, once, as exactly the identity the checker gave it.
`crates/nazm-cli/tests/durable_identity.rs` adds the construction, fields, unknown member and
variant payload read from a deserialised interface with the dependency deleted;
`crates/nazm-cli/tests/lsp.rs` the real process with `é😀` before every site. Fifteen N21
mutations (area 30).

*N22, 2026-09-26: document symbols — VERIFIED; workspace symbols over the current analysed
LanguageService universe — VERIFIED.* `LanguageService::document_symbols` is one open
document's functions, records, enums, variants, fields and payload fields, nested — a record's
fields under it, an enum's variants under it, a variant's payload fields under the variant — in
source order, each with its whole declaration as its range and its declared name as its
selection. `LanguageService::workspace_symbols` is every such declaration of every file the
open documents' compilations load, each once however many compilations load it, identified by
file, kind and name span and never by spelling, filtered by a case-sensitive substring of the
name and ordered by name, file, position and kind. Both are derived on demand from the
checker's definition tables and the parsed items their `decl` indices name
(`architecture.md` §7.24); nothing is kept. Over the protocol: `textDocument/documentSymbol`
(nested, or flat with container names for a client without hierarchical support) and
`workspace/symbol` (`resolveProvider: false`). Evidence: `crates/nazm-service/tests/symbols.rs`
— two records' `value`, two enums' `Ready`, a record and a function both `Box`, two modules'
`helper`, each separate; an import absent from its importer's outline; a broken body kept, an
unreadable item and refused duplicates absent, a type error kept; two roots sharing a module
listing it once, then after one closes, then none; an unimported `orphan.nz` beside them never
listed; the prelude and a missing module never located; unsaved edits seen at once and close
restoring the disk; a stale version refused; the query rule pinned on `Point`, `Pointer`,
`Checkpoint`; one order over ten runs; 1,000 edits with retained state constant; every symbol
the entity the reference index records at its name and where definition goes from every use;
over the repository's own source, every one of 817 declarations in 70 clean documents one
symbol with the same kind, name, range and parent as its concrete tree, and the compiler opened
as four overlapping programs listing each of its 500 declarations in its 8 files exactly once.
`crates/nazm-cli/tests/lsp.rs` the real process, with `é😀` before every checked position.
Thirteen N22 mutations (area 30).

*N23, 2026-09-26: semantic identifiers and `textDocument/semanticTokens/full` — VERIFIED for
functions, intrinsics, parameters, locals and pattern bindings, records, enums, variants,
record fields, payload fields, type parameters and built-in types, each as a declaration or a
reference.* `LanguageService::semantic_identifiers` classifies each identifier of an open
document by what the checker recorded at exactly its span — an entity in the reference index,
a built-in call, or a written built-in or type-parameter name (`Resolution::written_type`, the
one checker record N23 added; `architecture.md` §7.25) — keeping the compiler's identity for
what it names; an identifier the checker resolved to nothing is not classified. Over the
protocol: `semanticTokens/full` only — no range, no delta, no `resultId`, no token cache —
under a fixed legend of nine types and two modifiers (`declaration`, and `defaultLibrary` on
intrinsics and built-in types only), relatively encoded in UTF-16. Semantic identifiers only:
no keywords, comments, strings, numbers or operators. Evidence:
`crates/nazm-service/tests/semantic.rs` — every identifier of a fixture pinned with its class
and role, nothing from a comment or string; a record and a function both `Box`, two records'
`value`, two enums' `Ready`, shadowed locals, a parameter named like a field, and `State.Done(code:
code) => code` each keeping its identity, each declaration and its uses one identity; unknown
call heads, members, types and qualifiers unclassified; a recovered body losing only what it
left unresolved and a renamed declaration's old use disappearing, not remembered; a refused
signature lending its type parameters to no one; an imported function, an unsaved edit,
another open program and close; 1,000 edits with retained state constant; over the
repository's own source, 24,870 entity occurrences in 70 clean documents each one identifier of
the same entity and role, 817 declarations matching their N22 symbols, 2,371 built-in type
names, 94 type-parameter names and 5,875 intrinsic calls, nothing unaccounted, and 2,679 type
names found from the concrete tree alone all classified. `crates/nazm-cli/tests/lsp.rs` the real
process with `é😀` before tokens across blank lines, the exact integer array computed
independently, and range and delta requests refused. Fourteen N23 mutations (area 30).

*N24, 2026-09-26: `textDocument/codeAction` for current compiler diagnostic fixes — VERIFIED;
quick fixes only, open versioned documents only, no resolve, no fix-all; both `automatic` and
`needs_review` fixes are surfaced.* `LanguageService::fix_plans` turns each fix the current
diagnostics touched by the requested range carry into a `FixPlan` — the diagnostic, the fix's
applicability, description and precondition, the generation, and one versioned edit with the
bytes it replaces — and `fix_plan_is_current` holds it to all of them with the freshness law
renames use (`architecture.md` §7.26). Nothing is invented from a code or a message; the
client's diagnostics are not consulted. An automatic fix is preferred when it is its
diagnostic's only fix; a needs-review fix is titled with its precondition and never preferred.
Evidence: `crates/nazm-service/tests/fixes.rs` — all five fixes the compiler attaches (`mut`
on a `let`, `mut` off a parameter, `;` after a loop's value, `;` after an expression, a closing
quote), each planned exactly from its diagnostic, applied in memory to text that parses, with
its diagnostic gone and, when automatic, nothing outside its span changed; the edit on the
fix's span while relevance is the diagnostic's; the range rule at a cursor inside, at either
end, across two diagnostics and adjacent; a plan refused for other bytes, another version, no
version, another generation, a change outside its edit and one inside it; unsaved buffers, an
unsaved dependency and its close; no fix from an earlier analysis; 1,000 edits with retained
state constant; and, at the plan layer, two overlapping alternatives kept apart and a fix in a
file not open never planned. `crates/nazm-cli/tests/lsp.rs` the real process: a quick fix after
`é😀` with its versioned edit's UTF-16 range computed independently, unchanged until the client
sends `didChange`, then gone with its diagnostic; a stale and a fabricated client diagnostic
producing nothing; a needs-review fix with its precondition; `only` honoured; a client without
versioned edits offered none. Seventeen N24 mutations (area 30).

*N25, 2026-09-26: `textDocument/prepareCallHierarchy`, `callHierarchy/incomingCalls` and
`callHierarchy/outgoingCalls` — VERIFIED; source-backed functions only, the bound current
compilation snapshot only, direct checked calls only, no filesystem or project-wide caller
discovery, and no item for an intrinsic or a function without source.* An item is a function
the checker declared from source, prepared from its declaration or a call through the reference
index, with its outline symbol's ranges; an edge is a `CheckedCall` to such a function,
attributed to the body whose call sites the checker recorded it in, grouped by the function at
the other end and located at each callee name (`architecture.md` §7.27). No per-call state was
added and nothing is kept between requests. An item's opaque locator binds it to the
compilation it was prepared in and the generation it was prepared at; any change, or closing
that root, leaves it unanswered. **This is complete only for the bound current compilation
snapshot: it is not evidence that every importer or caller in a project is loaded**, and it
does not lift the exported-rename refusal. Evidence: `crates/nazm-service/tests/hierarchy.rs` —
preparation from a declaration, a call and the position just after a name, and none from a
same-spelled record, a construction, a field, an enum, a variant, a payload label, a local, a
parameter, an intrinsic or a type; items at their N22 symbol's ranges; two callers and two
callees each grouped with every call in order, two calls on one line; calls nested in
arguments, `if`, `while` and `match` owned by their function; recursion and mutual recursion;
three instantiations of one generic function as one item, each range the callee name alone;
constructions, variants and intrinsics not edges, a `spawn` an edge and a refused generic
`spawn` — a reference with no checked call — none; four files through an import cycle, a
private function, and two `helper`s kept apart as callees and as callers; a call with a type
error kept, a misspelt call dropped beside a kept one, an unparsable body losing its calls with
nothing brought back; unsaved callers, an unsaved dependency and its close; a refused duplicate
declaration no item, its calls the checker's; two roots over one shared file each seeing only
its own callers, and a closed root's item unanswered, never rebound; an item unanswered at
another version, after an unrelated edit and after an identical one; 1,000 edits through six
shapes with every old item unanswered and retained state constant; and, over the repository's
own source, every checked call of 70 clean documents to a function with source exactly one edge
occurrence from the body that holds it — found independently by containment — to the function
the checker resolved, seen alike from both ends, at the entity the reference index records,
at items matching their N22 symbols, and classified by N23 as a function declared or
referenced. `crates/nazm-cli/tests/lsp.rs` the real process: prepare, incoming and outgoing
after `é😀` with every range computed independently in UTF-16, a cross-file callee, a stale item
and forged data answered `null`, and two roots over one file. Fourteen N25 mutations (area 30).

*N26, 2026-09-27: completion at a structured hole — a bare member after `.`, a bare enum variant
after `E.` or `E[T].`, a bare construction or pattern label after `(` — and `.` as the
completion trigger character — VERIFIED; anchored only where the canonical compiler establishes
one record, enum or variant.* A request with no name at the cursor, right after a `.` or `(`
token, runs a completion probe: the compilation's current sources parsed by the same parser told
the cursor's offset — which reads a zero-width empty name there and nothing else differently — and
checked by the same checker, which anchors the hole where it already decides: the type an unknown
member was looked for on, the enum an `E.` names when no value does, the variant a construction
or pattern names, and — only where the module calls no function by that name — the record a bare
`Name(` would construct (`architecture.md` §7.28). Its candidates are N21's, from one function:
identities, canonical order, substituted details; the range is empty at the cursor. Nothing is
recorded by an ordinary analysis, the probe's diagnostics are never published, and nothing is
kept. Evidence: `crates/nazm-service/tests/incomplete.rs` — a parameter, a local, a temporary, a
projection chain and a call argument each giving exactly the receiver's fields; an applied generic
record substituted; a non-generic and an applied generic enum's variants, and a generic enum with
no arguments refused; a record type, an unknown name, an enum value, an `Int` and `p..` refused; a
local named like an enum reading as the local; two records' `value`, two enums' `Ready` and two
variants' `value` kept apart; a record's labels after `Point(`, `Point()` and `Point();`, a generic
record's substituted, none for a generic record with no arguments, a function, an unknown name, a
record and a function of one spelling, or a generic record and function of one spelling; payload
labels in a construction, an applied generic's, and a pattern alone, before another arm and after
one; the same candidates as `p.|x`, differing only in the range, and `p.x` unchanged; declaration
order irrelevant; the hole alone answered, another error before or after it in its function, at the
top level or in a dependency refused, another function's allowed; strings, comments, whitespace
and a second dot no hole; a private dependency record and an ambiguous import never anchors;
unsaved receivers, an unsaved dependency and its close, no earlier answer, a stale version refused,
another open program lending nothing; the published diagnostics, the text and retained state
unchanged by asking; 1,000 edits through ten shapes; and, over the repository's own source, cut
member and variant sites each completing to a list holding exactly the member the checker had
resolved there. `crates/nazm-syntax/tests/holes.rs` — over every corpus file, broken test programs
included, a probe parse at an offset with no hole the ordinary parse exactly; the hole read only at
the exact byte and changing only its statement; a later missing `;`, two dots and a written label
still what they were. `crates/nazm-cli/tests/lsp.rs` the real process: `.` advertised alone,
invoked and triggered completion identical after `é😀`, each edit empty at the UTF-16 cursor, and a
trigger in a string, a comment or whitespace answered `null`. Eight N26 mutations (area 30).

*N27, 2026-09-27: Semantic Context Packet v1, `nazm.context/1` — VERIFIED for source-backed
functions, records and enums, in the current root compilation, with direct semantic links and
target-local diagnostics; and MCP — PARTIAL: one read-only stdio tool, `nazm.semantic_context`,
reading the disk at each call.* A packet (`crates/nazm-service/src/context.rs`, `architecture.md`
§7.29) is the target found through the reference index at a byte offset, its durable identity, its
signature or shape by the existing renderers, its exact source by N22's range, and compact links —
identity, kind, name, place — for its dependencies (every entity used inside it, grouped by
identity), related types (N18's written names of program records and enums, directly), references (uses only), callers and
callees (N25; `not_applicable` for a type) and diagnostics inside it; effects, capabilities, tests,
semantic changes and deltas are `unsupported`, never empty. Paths are source-root-relative and
nothing session-local is serialised. `nazm context --json` prints it; `nazm-mcp`
(`crates/nazm-mcp/src/main.rs`), on the official Rust SDK, serves it with
`schema/nazm.context-3.json` (`-2` before N38) as its output schema. This establishes semantic context retrieval for
those three kinds and a compact-by-design contract for the fields it promises; it does not make a
packet sufficient for every task, and G68–G70 remain broader. Evidence:
`crates/nazm-service/tests/context.rs` — a function, record and enum each the same packet from its
declaration and from a use, a record and a function of one name and two modules' `helper` apart;
locals, parameters, fields, variants, labels, intrinsics, built-in types, whitespace, keywords and
comments refused, a file beside the root and `/etc/passwd` not in the compilation; the exact source
slice; dependencies grouped by identity with every occurrence, same-named fields of two records
apart, locals, intrinsics and the target itself excluded; related types direct; the core prelude's
`Option` and `Result` and a built-in type never linked, since v1 links only entities with a
declaration in a file of the compilation, and every link's place checked to be one; a private definition of an imported module still linked; references every use and not the
declaration; callers and callees equal to call
hierarchy's, a type's not applicable; diagnostics only the target's; availability unsupported; a
syntax error or a missing module refusing; two roots over one file each seeing its own; a hundred
serialisations and a copy in another directory byte-identical with no absolute path; each packet of
the disk as it is then; and, over the repository's own source, 649 packets of 70 clean documents
matching N22's ranges and signatures and N23's classification, every dependency occurrence the
reference index's use of the linked entity, and callers and callees N25's. `crates/nazm-cli/tests/schemas.rs`
the real command's packets and refusals valid against the schema. `crates/nazm-mcp/tests/protocol.rs`
the real server through the SDK's client: only tools advertised, one read-only tool with the
published output schema, packets equal to the service's and carried once — structured content, an
empty `content`, the packet's schema string exactly once on the wire — protocol errors for an unknown tool and bad
arguments, refusals for files outside the compilation, the disk re-read between calls with a broken
edit refused and a repair answered, two servers with two roots isolated, 1,000 calls with flat
resident memory, and nothing but JSON-RPC on standard output. Sixteen N27 mutations (area 30). Context
packet v1 exposes source-linked program dependencies only: compiler-owned, built-in, interface-only
or toolchain definitions with no navigable declaration may affect checking but are not linked.

*N28, 2026-09-27: Semantic snapshot v1, `nazm.snapshot/1` — VERIFIED; Semantic delta v1,
`nazm.delta/1` — VERIFIED; for durable source-backed functions, records and enums of the current
root compilation, and the sections the compiler records of them. G71 — PARTIAL; MCP — PARTIAL,
three read-only tools.* A snapshot (`crates/nazm-service/src/snapshot.rs`, `architecture.md` §7.30)
is every such definition by `DefKey`, with one BLAKE3 digest each for its exact source, shape
(kind, visibility, signature or fields or variants), source-linked dependencies, related types,
references, callers and callees (a function's; null for a type) and diagnostics — relationships
digested by identity and count, never offset — plus the root's module key and compiler, a count of
definitions with no durable key, and `unsupported` naming effects, capabilities, tests, semantic
history and behavioural equivalence. A delta (`crates/nazm-service/src/delta.rs`) validates a
baseline as data and reports added, removed and changed definitions by key, with a record of which
sections changed. N28 reports changes in the semantic surfaces Nazm currently records; it is not a
proof of behavioural equivalence: `{ 1 }` to `{ 2 }` is `source` alone, and effects, capabilities
and tests are unsupported, not unchanged. G71 is partial for that reason — a delta names what the
compiler records, and no behavioural, effect or test delta. Evidence:
`crates/nazm-service/tests/snapshot.rs` — every durable definition by identity, ordered, with two
modules' `helper` apart and no source text or packet inside; byte-identical from fresh services, at
any service generation, and from a copy in another directory with no host path and no git
repository; a file nothing imports absent and another root's compilation its own; a module outside
the source root counted untracked, never matched; recovery and a missing module refusing; no edit
and a moved definition no change; a comment and a changed literal `source` alone; a function added,
an enum and a function and a record removed, a rename removed plus added and never inferred; a
signature, a field, a variant and visibility `shape`; a new call changing the callee's references
and callers and the caller's dependencies and callees, and a second call counting; a use elsewhere
changing only a record's references; a new diagnostic changing only its definition; a broken edit
refusing and the repair answering; every malformed baseline refused with its reason — another
schema, an unavailable snapshot, an unknown or missing field, a record's callers, a non-canonical,
wrong-kind, absolute or prelude identity, a duplicate, a bad digest, another root, another
compiler; and 500 snapshot and delta pairs leaving the service's retained state unchanged.
`crates/nazm-cli/tests/schemas.rs` the real commands' snapshots, deltas and refusals valid against
both schemas. `crates/nazm-mcp/tests/protocol.rs` the real server: three read-only tools with the
published output schemas, a snapshot equal to the service's and a delta needing only it, each
carried once on the wire, an edit seen between calls, a broken edit refused for both tools and the
repair answered, a path or path-shaped object refused as a baseline without being read, protocol
errors for missing or extra arguments, and 1,000 snapshot and delta calls with flat resident
memory. Twenty N28 mutations (area 30).

*N29, 2026-09-27: Semantic Patch Plan v1, `nazm.patch/1` — VERIFIED, for two operations,
`rename` and `diagnostic_fix`; patch application — MISSING; MCP write or edit tools — MISSING;
G72 — PARTIAL.* A patch (`crates/nazm-service/src/patch.rs`, `architecture.md` §7.31) is N18's
validated rename or the fix a current diagnostic carries (N24), as minimal exact-byte edits, bound
to BLAKE3 of the root's `nazm.snapshot/1`, each edited file's BLAKE3 digest and each edit's
expected bytes; a fix's applicability and precondition are structured fields. N29 plans edits but
never applies them. Exported rename remains refused, because one root compilation is not a
complete importer universe; N29 does not alter LSP closed-file safety. G72 is partial because
minimal structured edit planning exists for these two operations, and generic semantic edits,
application and write automation do not. Evidence: `crates/nazm-service/tests/patch.rs` — a
private function renamed at exactly its declaration and call, with its durable key, the file's
digest, the snapshot's digest, each edit's bytes and no other text; every N18 category — local,
record, field, enum, variant, payload field — at exactly its occurrences, each applied to a copy
and checking clean; a local named by its place and its function, never a key; a private function
of an imported file no editor has open planned from disk while the service's own rename there is
still refused; exported, invalid, unchanged, colliding, unentitied, out-of-range, unloaded,
unclean and unparsable requests refused; an automatic and a needs-review fix with the compiler's
applicability and precondition, each applied and its diagnostic gone; no fix at a position with no
diagnostic, at an index past the fixes, after the repair, or for a syntax diagnostic; each
freshness layer going stale when what it protects moves, and the same request re-planned fresh;
byte-identical plans from another directory; the N28 delta after a rename (the old key removed,
the new added, the caller's source, dependencies and callees, and the record it took's references)
and after a fix (the definition's source and diagnostics); and 500 rename and fix plans each
leaving retained state unchanged. The unit tests order edits and refuse duplicates, overlaps and
two insertions at one point. `crates/nazm-cli/tests/schemas.rs` the real commands' patches and
refusals valid against the schema (with `oneOf` added to its validator for the tagged operation).
`crates/nazm-mcp/tests/protocol.rs` the real server: four read-only tools, `nazm.semantic_patch`'s
input the SDK's derivation of its typed request, patches equal to the service's and carried once,
the files untouched, exported refused, unloaded files and `/etc/passwd` refused, a root, an unknown
operation or argument and a client-written replacement protocol errors, a disk edit re-planned, a
broken edit and a vanished target refused, and 1,000 plans with flat resident memory.
Sixteen N29 mutations (area 30).

*N32, 2026-09-28: Machine-addressable documentation index v1, `nazm.docs-index/1` — VERIFIED;
Selective documentation retrieval v1, `nazm.docs-section/1` — VERIFIED; G78 — PARTIAL; G79 —
PARTIAL.* `nazm docs` and `nazm-mcp`'s `nazm.docs_index` and `nazm.docs_section`
(`crates/nazm-docs/`, `architecture.md` §7.34) section Nazm's own fourteen canonical documents,
each with the authority `master-architecture.md` §5 gives it, and return one section's own text
byte for byte by exact id, bound to a corpus state. The index is not a second source of
documentation truth; retrieval never paraphrases; a goal is never evidence and a plan never a
rule. G78 is partial: no semantic search, no documentation history, no rule ids below sections,
no documentation outside a source checkout. G79 is partial: one mechanical audit — no id, and so
no anchored rule, defined twice — and no whole-spec deduplication, rule graph or contradiction
detection. Evidence: `crates/nazm-docs/tests/corpus.rs` — ids from anchors, labels and heading
paths in that order, with two `Child` sections under two parents distinct; a parent's body its own
text and its children listed, a document equal to its sections concatenated; an anchored section
keeping its id reworded and moved, an unanchored one's old id not found; a removed section not
found and an added one moving no other id; a `#` line in a fence not a section and a Unicode
heading's id deterministic, stable under an ASCII anchor; every structural defect — a duplicate id,
anchor or label, a malformed or misplaced anchor, a setext or HTML heading, an unclosed fence, no
title, an empty identity, a broken link, a missing document — refused as unavailable; links
resolved to section ids, outside and external links kept as written, and a link to a reworded
heading refused; a grammar sectioned by production with exact text, examples and references, and
a broken one refused; the corpus state moving with one byte and with the manifest, only the edited
section's digest moving, a stale state refused, unknown ids and paths not found; the same state
and bytes from two directories, naming neither; and on the real corpus, every Markdown document
equal to its sections, `spec:generics` direct, the one fragment link resolved, each document's
authority, and every generated guide rule `generated_from` its production.
`crates/nazm-docs/src/markdown.rs` and `grammar.rs` — fences, anchors, setext and HTML refusal,
links outside code, locale-free slugs; production blocks. `crates/nazm-cli/tests/documentation.rs`
— one object on stdout and nothing on stderr for every answer and refusal, exit 0 and 1; a section
for a person exactly its body; one document's index equal to its part of the whole; two copies of
the repository giving the default's bytes; a rewording stale by state and still found by anchor; a
broken link and an empty root unavailable; every ambiguous flag set refused. `crates/nazm-cli/tests/schemas.rs`
— every status valid against both schemas. `crates/nazm-mcp/tests/protocol.rs` — nine tools, the
documentation tools taking ids and a state and never a path, file or root; answers equal to the
library's and carried once; `../README.md` and `/etc/passwd` not found, a path, a root, a query
and a missing state refused; a disk edit stale by state and read at the next call; a broken corpus
unavailable; and 1,001 calls through edits and repairs with flat resident memory. Seventeen N32
mutations (area 30).

*N89, 2026-10-04: workspace editing — VERIFIED for exported renames within a declared workspace and
stale-safe application; the area stays PARTIAL.* `architecture.md` §7.90 first. A **workspace** is a
declared source root — a `nazm.root` marker, `--source-root`, or an application package — and every
`.nz` under it, each analysed as a root over the editor's buffers. An exported function, record,
enum, field, variant or payload field is renamed in every module of it that uses it — those the
requesting root's compilation does not load included — the entity found in each compilation by its
declaration's place, every compilation that contains an edited file re-analysed whole, clean and
partitioned as before; a library package's API (`package_api`) and a root nobody declared
(`exported`) are refused. `nazm patch apply PLAN` writes a `nazm.patch/1` plan all or nothing, only
to files whose bytes hash to the plan's digests. Evidence: `crates/nazm-cli/tests/workspace_edit.rs`
(5) — a function renamed in three modules and nowhere else, applied and run; an edit after planning
refused with nothing written, and a path leaving the base refused; no declared root, a library
package, an application package; a capture in an importer refused; an exported record field renamed
through construction labels and projections. **Why still PARTIAL**: MCP has no planning tool for
the workspace rename and no apply; there is no DAP integration in the language server; and no
editor was driven — protocol tests do not show an editor's behaviour.

*N33, 2026-09-29: Repository Context Map v1, `nazm.repository-map/1` — VERIFIED; Minimum Task
Context Planner v1, `nazm.task-context/1` — VERIFIED; G80 — PARTIAL; G69 — PARTIAL; G68 — PARTIAL,
strengthened; G70 — PARTIAL, unchanged; G83, G84, G85 — RESEARCH, with a byte-level foundation
only.* `nazm repo` and `nazm-mcp`'s `nazm.repository_map` and `nazm.task_context`
(`crates/nazm-repo/`, `architecture.md` §7.35) map this repository's entities that have durable
identity — Cargo packages and targets, three Nazm source roots and their thirty compilation roots,
modules, 477 durable definitions, fourteen documents, sixteen schemas, twelve mutation profiles —
each with its authority and only structural, authority-named relationships, validated before
publication; and plan, for seed ids and an intent, the smallest context the repository can
justify, one step from each seed, every item with its reason, class, authority and retrieval,
under a structural budget whose cut is always `partial`. It composes N27, N28, N30 and N32 and adds
one service accessor; no grammar, semantics, `SEMANTIC_EPOCH` (7), `nazm.interface/5` or existing
schema changed. G80 and G69 are partial: one repository, the manifest's compilations only, no Rust
semantics, no transitive or project-wide closure, and sufficiency shown for seven representative
tasks by mechanical checks, not for every task. G83–G85 have measurements a later agent benchmark
can use — context bytes, entity counts, latency, check success — and no model, token or cost
result. Evidence: `crates/nazm-repo/tests/repository.rs` — ids from each authority, a directory
without `main.rs` not a target, a recovered compilation's modules mapped and its definitions not;
the same bytes twice and from two directories; the state moving with every input class and a stale
one refused; understand, edit and test closures, callers from two compilations, a type's users, a
test related only where its mutation lands; a section's own text and never its document's; a goal
and a plan ranked below the specification and the evidence, and cut first; a budget at, below and
above an exact fit, the seeds refused rather than cut; a cycle visited once and a fan-out of twenty
cut at sixteen and counted; paths, `..`, absolute and glob seeds refused and never read, unknown ids
not found; a definition only a recovered compilation loads `incomplete_compilation`; a broken link,
killer, manifest or root and a compilation reading outside its root unavailable; the validator
refusing duplicate, dangling, unknown and unordered maps; and the workspace equal to `cargo
metadata`'s. `crates/nazm-repo/tests/benchmark.rs` — seven real tasks, each checked against N27,
N32 and the catalogue parsed independently (`performance.md`). `crates/nazm-cli/tests/repository.rs`
and `tests/schemas.rs` — one object and an empty stderr for every answer, exit 0 and 1, the
library's bytes, ambiguous flags refused, every status valid against both schemas.
`crates/nazm-mcp/tests/protocol.rs` — eleven tools, the repository tools taking ids and a state and
never a path, answers equal to the library's and carried once, a disk edit stale by state, and
1,000 task contexts with flat resident memory. Nineteen N33 mutations (area 30).

*R1-C1, 2026-10-07: a function written as a value is a reference — corrected.* Until this step the
reference index held a function's calls and not its uses as a value (`apply_twice(double, 5)`,
N50), so references, `nazm references`, the LSP and a snapshot's `references` left them out, and
rename refused such a function (its re-check found the name undefined). Undocumented until R1's
precheck found it; fixed before v0.3.0 by reading the checker's recorded function values into the
index (`architecture.md` §7.110), with no change to the language. Evidence:
`crates/nazm-service/tests/references.rs`, `rename.rs`, `snapshot.rs`, `closures.rs` and
`structure.rs`; `crates/nazm-cli/tests/lsp.rs` and `resolved_units.rs`; two catalogue entries
(area 30).

**Limitation** · Diagnostics, definition, hover, references, rename, completion, signature
help, symbols, full-document semantic tokens, quick fixes and call hierarchy only: no
formatting, refactors, source actions or inlay hints, no cancellation and no watched files.
Call hierarchy is one level of direct checked calls per request within the compilation an item
was prepared in: no transitive or persistent call graph, no callers from files that compilation
does not load, and no item for an intrinsic or a function without source. Quick fixes
are the fixes the compiler attaches, for open documents at their current version; a fix whose
file is not open is not offered, and there is no post-apply rollback — the server never writes. Semantic tokens are
`semanticTokens/full` only — no range, no delta, no token cache — and cover identifiers the
checker resolved: no keywords, comments, strings, numbers or operators, which an editor
grammar colours, and nothing for a name the checker did not resolve. Workspace symbols cover the files the open
documents' compilations load: there is no manifest-defined complete project index, no
filesystem scan, and no persistent workspace symbol database, so an unloaded orphan file is
not searched, and a declaration known only from a persisted interface, with no source location,
is not a symbol. That is not a project boundary, and it does not lift the exported-rename
refusal below. Every change re-analyses every open document in full; no
incremental reparse or graph maintenance, and no persistent cache in the editor. A built-in, a
type parameter and anything in a body that did not parse have no definition and no references;
the core prelude has no file, so a definition in it has
no location and a references answer for one of its entities has no declaration. References
cover the programs being analysed — each open document's compilation — and not files on disk
that no open document loads. Rename covers locals and private definitions, and — within a declared workspace, since N89 — exported
ones across every module under the root; without a declared root, and for a library package's API,
an exported entity is refused, because a module no walk finds may use it. The editor protocol's
edit is offered only when every file it touches is open, because the protocol cannot make a closed
file's content a precondition; such a plan goes through `nazm patch`, whose `apply` binds every
file's bytes. Completion is of an
identifier in a value, call-head or type position only — no field, variant, label, pattern or
import-path completion, no auto-import, no ranking — and there is none where the source needed
syntax recovery. Signature help is for calls and `spawn`s whose callee
the checker resolved — not record or variant constructions, and none while any file of the
compilation needs syntax recovery (an unclosed argument list included); a built-in's parameters
are shown by type, having no names; no documentation. Structure completion offers no methods
(a method's name after `.` has no member answer; methods are N78's), no import paths, no auto-import and no ranking. At a hole — right after
a `.` or `(` with nothing written (N26) — it needs the probe's anchor: nothing at a `(` the
module calls as a function (a record of the same name included), after a generic enum or record
with no type arguments, after a record type, an unknown name, an enum value or a second `.`; no
completion after a comma or at an empty label slot later in a list; nothing in a function with
another syntax error, and nothing when any other file, or the document's top level, recovered.
`.` is the only trigger character; `(` is not one. A hole's completion costs a second parse and
check of the compilation per request (`performance.md`).
`N0204` (no `main`) is not reported, because an open file is analysed as a module. Diagnostics
are published for open documents only. **MCP is eleven read-only tools** (N27–N33): the semantic
context packet, the semantic snapshot, the delta against a client-kept snapshot, a planned rename
or fix, the current diagnostics compactly or one in full, a compact summary of the root's
check — no build and no test run, which write files, over stdio, from disk, for one fixed root —
and Nazm's own documentation and the repository's context map and task contexts, by id, over one
fixed repository — no resources, prompts,
completions, sampling or edits, no tool that applies a patch, no HTTP, no server-side history, and
no view of an editor's unsaved buffers. A patch covers rename and compiler-owned fixes only — no
exported rename, no multi-file edit, and no fix of a syntax diagnostic, whose compilation has no
snapshot to bind it to. A packet has no effects,
capabilities, test mapping or semantic history (each `unsupported`); a snapshot's digests are of the
sections the compiler records, so equal digests are not equal behaviour, and a delta has no rename
inference and no effect, capability or test delta. All three cover only the root's compilation — no
project-wide completeness — and are refused for a compilation that needed syntax recovery.

**Next dependency** · None for what exists. Each further request needs its own semantic
contract; incremental reparse needs latency evidence, which N16's measurements do not give.

**Accepted when** · Met for the LSP, 2026-09-25 (N16): an adapter shares the compiler's
semantic engine rather than re-implementing it. MCP: partial since N27 (2026-09-27) — read-only
tools over the same service, three since N28, four since N29, six since N30, seven since N31, nine since N32, eleven since N33; not accepted, since they answer a
few of the questions the service can, and none edits.

### 26. Package and build tooling — PARTIAL

**Gate 2 (2026-10-09)** · *Package features* (`general-purpose.md` §22; `spec.md`). `[features]`
in `nazm.toml`, a dependency's `features`, `--features`; additive, settled to a fixed point per
package; each module's `@cfg(feature = …)` reads its own package's set; `nazm.lock/2` records the
enabled sets, `/1` kept without them. Evidence: `crates/nazm-cli/tests/features.rs` — defaults, a
dependent's request, a feature enabling a dependency's, isolation between packages, interpreter
and native build agreeing, three refusals, and the lockfile's schema, contents and staleness.

**Gate 2 (2026-10-09)** · *Tests and benchmarks* (`general-purpose.md` §18–§19; `spec.md`). `nazm
test` runs every `@test` and `@fuzz` function by name under the interpreter and as an executable,
each in its own process through a generated entry that blanks the file's `main` (R1: rather than
renaming it) without moving an offset; `--filter`, `--fuzz N`; `@test(fails)`; `nazm.test/1` gained `kind`. `nazm bench`
measures `@bench` functions natively at `-O2` (`nazm.bench/1`, EXPERIMENTAL). `@std/test` gained
five assertions and `property`. Evidence: `crates/nazm-cli/tests/user_tests.rs` — pass, `Err` with
its message from both legs, a trap under `@test(fails)`, a wrongly expected failure, a test holding
authority, a property over 50 cases, a `@cfg`-excluded test not run, the filter, a fuzz target's
first failing case identical across legs and runs, six shape refusals, a diagnostic at its line,
and a benchmark's row; `schemas.rs` validates both outputs.

**Evidence** · Multi-file builds work through path-relative `use`, two independent
implementations: `crates/nazm-service/src/load.rs` and `compiler/module.nz`.

*N46, 2026-10-01: packages v1 — VERIFIED for path dependencies, one host.* `architecture.md`
§7.48 first. `crates/nazm-package`: the `nazm.package/1` manifest (unknown keys refused), exact
versions, deterministic name-order resolution with conflicts, mismatches and cycles refused by
code (`N0500`–`N0504`), BLAKE3 package digests, and the `nazm.lock/1` lockfile `nazm lock` writes;
`--locked` refuses a missing, mismatched or tampered lock (`N0505`–`N0507`). `use "NAME:path"`
reaches a declared dependency and nothing else (`N0508`); dependency modules are keyed
`NAME@VERSION/path`. Evidence: `crates/nazm-cli/tests/packages.rs` — a diamond built from its
directory by the interpreter and both backends; every refusal by code; visibility; the lockfile's
bytes independent of the directory and not rewritten when unchanged; tampering refused before
anything is built; dependency keys; an unchanged dependency reused and a changed one rechecked
alone; two copies byte-identical from empty caches; 200 packages resolved and locked in name order.
No registry, no remote sources, no version ranges, no build scripts, no workspaces of several
roots.

*N90, 2026-10-04: packages v3 — VERIFIED for backtracking resolution and `nazm update`, one host;
the area stays PARTIAL.* `architecture.md` §7.91 first. `crates/nazm-package/src/lib.rs` `Search`:
registry packages in name order, each one's non-yanked versions newest first with the lockfile's
first while it satisfies; a candidate is kept only if every requirement the current choice places
on it holds, a later requirement on a package already chosen is checked again, and a dead end undoes
the last choice. The first complete assignment is the answer — deterministic, the newest a lockfile
allows. At most 10,000 candidates: past that the refusal says the search stopped, not that no answer
exists. A refusal (`N0513`) names the package no version fitted where the search got deepest and
every requirement on it with who placed it, a registry package at its version. `nazm update DIR
[NAME…]` re-resolves with the lockfile's preference dropped for the named packages (every one
without names), prints `name old → new` per change, and writes the lockfile; a name that is no
registry package is `N0501`. Evidence: `crates/nazm-package/tests/registry.rs` (+4): an older version
chosen where N56 refused, an explained conflict naming both requirements, the bound reached on
100,000 dead-ending assignments, and `update` holding the packages not named;
`crates/nazm-cli/tests/registry.rs` (+1): `nazm update` prints the change, rewrites the lockfile, and
the build runs the new version. Five mutants. Still not here: signatures and trust roots, a remote
registry, features and pre-releases, multi-root workspaces — the area stays PARTIAL.

*N56, 2026-10-02: packages v2 — VERIFIED for a local registry, one host.* `architecture.md` §7.58
first. `crates/nazm-package/src/registry.rs`: `nazm.registry-index/1` indexes and an immutable copy
per version; `^`, `~`, `=` and exact requirements; per name, the lockfile's version while it still
satisfies, else the highest non-yanked, one version per name and no backtracking — backtracking since
N90 — (`N0513` naming every requirement); `nazm publish` and `nazm yank`. Refused by code: a version published twice and
a malformed or equivocating index (`N0512`), a copy changed after publishing (`N0507`), a link in a
package (`N0514`), a path package and a registry dependency of one name (`N0503`), a cycle through
registry edges (`N0504`), a path dependency in a published package. A registry source is
`registry+NAME@VERSION`, so where the registry is enters no lockfile. Evidence:
`crates/nazm-package/tests/registry.rs` (14) — choice by operator, transitive narrowing and a named
conflict, reordered index entries, two registry locations, lockfile preference after a newer
publish, a stale lockfile under `--locked`, yanking with and without a lockfile, immutability,
tampering by edit and by addition, a symbolic link, five malformed indexes and equivocation, a name
that is a path, a cycle, a name conflict — and `crates/nazm-cli/tests/registry.rs` (3): publish,
resolve, lock, `--locked` run and build, yank; tampering refused before anything is written; the
commands' refusals. The compiler written in Nazm reads no manifest (DESIGNED: hand it `nazm.lock/1`).

*N31, 2026-09-28: Compact Command Summary v1, `nazm.command-summary/1` — VERIFIED; Compact Test
Summary v1, `nazm.test-summary/1` — VERIFIED; G76 — PARTIAL.* `--summary-json` on `nazm check`
and `nazm build` (`crates/nazm-service/src/summary.rs`, `architecture.md` §7.33) prints the
command's own status — `success`, `rejected`, `unreadable`, `refused`, `toolchain_failed`, one per
exit path — its unchanged exit status, counts, and a reference to every diagnostic it reported:
N30's id where N30 indexes it, an explicit `command` reference where it does not (`N0204`, a
backend's `N0101`), code, place and fix counts, no prose. On `nazm test` it prints counts from each
case's own verdict and every case that did not pass, with how each leg ended, from what the runner
did. Summary is an additive view, not a replacement for existing detailed machine output; absence
of diagnostics does not imply command success. G76 is partial: no test impact, test-detail query,
remote logs or agent task cost. Evidence: `crates/nazm-cli/tests/summary.rs` — a clean check, type
errors, a syntax error and a missing `main` each summarized with the check's exit status, exactly
the diagnostics `--json` gives by code, and none of their prose; `N0204` kept as a command reference
with no id, the type error with N30's id, its place and state answered by `nazm diagnostics
--detail`, and the references in N30's canonical order; an unreadable root and a missing module
`unreadable` with their diagnostics; a build that writes its executable and names it as the build
does, builds rejected by the checker, for no `main` and by the backend with nothing written, and a
build refused with no diagnostic at all and not a success; every ambiguous flag pair refused, and
`--json` and the human rendering as they were; a summary the same from another directory; a test
run counting each verdict, listing only the four cases that failed — output mismatched, a runtime
error, a type error, a build the compiler refused — with each leg, and `not_run` when interpreted
only; no cases and an unreadable path refused as the command refuses them; 201 cases summarized by
counts and one failure, no passed name; and the MCP server's check summary equal to `nazm check
--summary-json` with and without its cache. *N31 closure:* every public N31 outcome variant has at
least one regression test that executes its production branch — a test driver that cannot be
started (`could_not_run`, and `could_not_build` beside it), a build that cannot be started after the
program ran (`could_not_build`, not `build_failed`), a build that reports success and writes no
program (`could_not_run`, not `mismatched`), a test run with nowhere to build (`toolchain_failed`),
and `nazm build` with a `clang` that refuses to compile, one that compiles and will not link, no
`clang` on PATH, and nowhere to write (`toolchain_failed`, no diagnostic, exit 4, not a success);
each beside the same run without `--summary-json`, with the same exit status. The runner's driver
is chosen through `NAZM_TEST_DRIVER`, a test seam; `clang` through the process's own PATH. Not
every way a process can fail to start is tested — one per branch. `crates/nazm-service/tests/summary.rs` — a reference
linked only while the file is exactly what the command read. `crates/nazm-cli/tests/schemas.rs`
every status valid against both schemas. `crates/nazm-mcp/tests/protocol.rs` — seven tools, the
summary tool accepting only `check` (not `build`, `test`, a root, arguments or a command), nothing
written, and 1,000 calls through edits, breaks and repairs with flat resident memory.
Fifteen N31 mutations, and seven in its closure (area 30).

**Since N107 (moved from area 2)** · `nazm build` and `nazm run` check every module on every run; a warm build reuses objects, not checks. Every answer is the same either way — it is a cost. A check against a persisted interface alone, without source, cannot use that module's traits.

**Limitation** · A package is built from its `nazm.toml` and `nazm.lock` (N46), resolved by a
backtracking resolver that explains a refusal (N90) against a **local** registry (N56): no remote
registry or network source, and no signature on a registry entry. A declared root — a `nazm.root`
marker, `--source-root` or a package directory — fixes where a module's durable identity is
relative to; without one, the source root is derived from the file named (N3). The compiler
written in Nazm reads no manifest and refuses a package import by name. *Until R1 this said there
was no manifest, resolution, lockfile, registry or declared root — true before N46 and N56.*

The two resolvers still differ deliberately — the Nazm one normalises paths textually
because the emitted runtime has no `realpath`, so a symlinked file is read twice there and
once here. N3 settled what that means for identity rather than closing it: a module reached
under two names gets **no** durable key, in either implementation, so the two cannot
disagree about one (`spec.md`, *Durable identity*).

**Next dependency** · A remote registry source and signed entries (a post-release track); the
compiler written in Nazm reading manifests.

**Accepted when** · A project builds from a manifest and the two resolvers agree on
identity.

### 27. Debugger and profiler — PARTIAL

**Evidence** · N48 (2026-10-01), `architecture.md` §7.50 first. `nazm build --debug` emits DWARF
through the LLVM backend (`crates/nazm-lir/src/debug.rs`): a subprogram per function and a source
position on every instruction from MIR's spans, gathered into `OUTPUT.dSYM` on macOS. `nazm
inspect` prints `nazm.inspect/1` (`crates/nazm-cli/src/inspect.rs`); `nazm build --timings` prints
`nazm.timings/1`.

*N91, 2026-10-04: debugger and profiler v3 — VERIFIED for the host matrix below; the area stays
PARTIAL.* `architecture.md` §7.92 first. MIR records each binding's block (`LocalDecl.scope`) and an
LLVM debug build nests a `DILexicalBlock` per block. Cranelift writes DWARF 4 through `gimli`
(`crates/nazm-codegen-clif/src/dwarf.rs`): a compile unit, a subprogram per function and a line table
from the source location set on every statement, relocated per object format; a debug object is never
reused from the cache. `nazm profile --sample` runs the debug build under macOS's `sample` and
attributes each sample's innermost frame to a Nazm function and line, the runtime, foreign code or
the system, counting the system's waits as blocked; `nazm.profile/2`. Evidence:
`crates/nazm-cli/tests/debugger_v3.rs` (5, and 1 run with `--ignored` in the `nazm-debug` image) and
the sampler's attribution unit test; eight mutants, one repointed.

| Feature | LLVM, aarch64-apple-darwin | LLVM, aarch64-linux (gdb, container) | Cranelift, aarch64-apple-darwin | Cranelift, aarch64-linux (gdb, container) |
| --- | --- | --- | --- | --- |
| Breakpoint by function and by `.nz` line | lldb, static | live | lldb, static | live |
| Backtrace naming `.nz` lines; step by statement | — (developer mode) | live | — (developer mode) | live |
| Parameters and bindings (`Int`, `Bool`, `Str`) | described | live | not described | `No locals.` |
| A binding visible only in its block | described (IR) | live | — | — |
| Sampled run attributed to `.nz` lines | `sample` | no sampler in the image | `sample` | no sampler in the image |

| Claim | Status | Evidence |
| --- | --- | --- |
| Line tables: a breakpoint by Nazm function or by `.nz` file and line resolves to that line | VERIFIED (static, lldb on aarch64-apple-darwin) | `a_debugger_finds_nazm_functions_and_lines` |
| A debug build behaves as an ordinary one; an ordinary build carries no debug metadata | VERIFIED | `a_debug_build_runs_as_an_ordinary_one_and_an_ordinary_one_carries_no_debug_information` |
| A live session — run to a breakpoint, backtrace, step (N58) | VERIFIED (gdb, aarch64-unknown-linux-gnu, in the `nazm-debug` image) | a breakpoint by Nazm function name stops on its first statement with the parameters stored; the backtrace names Nazm functions with files and lines through two calls; `next` and `step` by statement (`a_live_session_shows_nazm_frames_lines_and_variables`, run with `--ignored` where Docker is). On macOS, launching under lldb still waits on developer-mode authorisation and is not exercised |
| Local variables, types (N58) | VERIFIED for `Int`, `Bool`, `Str` | parameters (with their positions) and source bindings, never temporaries; `Str` as `{data, len, owner}`; a dropped or not-yet-bound slot reads as its zeroed contents, never freed storage (`parameters_and_bindings_are_described_and_temporaries_are_not`, and the live test). Records, enums, sequences, channels and closures not described; no lexical scopes, so a variable is visible for its whole function |
| Cranelift debug information (N91) | VERIFIED for functions and lines; variables not described | DWARF written with `gimli` from Cranelift's per-instruction source locations: breakpoints by function and line resolved by lldb (`a_debugger_finds_a_cranelift_build_s_functions_and_lines`) and a live gdb session (`a_live_session_on_either_backend_shows_nazm_frames_and_lines`); a line added above the code moves the next build's lines (no debug object is reused). Variables: Cranelift reports where a value lives only through value labels, not tracked — `No locals.` |
| Lexical scopes (N91) | VERIFIED (LLVM) | a branch's binding in a `DILexicalBlock` of its own (`a_binding_is_scoped_to_its_own_block`); live, a binding of a later branch not in scope at the function's first statement and in scope, with its value, in its branch |
| Deterministic inspection of a program's facts | VERIFIED | `inspect_is_one_deterministic_document_of_the_program_s_facts` |
| Compiler phase timings | VERIFIED | `timings_account_for_every_phase` |
| Runtime profiling (N58) | VERIFIED as counts | `nazm profile FILE` prints one run's exit, wall time, output size, memory counts and scheduler counts (tasks spawned and joined, selects, timeouts), under either backend (`a_profile_reports_what_one_run_did_under_both_backends`); `nazm.profile/2` since N91 |
| Sampling profiler (N91) | VERIFIED on aarch64-apple-darwin; BLOCKED on Linux here | `nazm profile --sample [--interval-ms N]`: every sample attributed once, by its innermost frame, to a Nazm function and `.nz` line, the runtime, foreign code or the system, a system wait counted as blocked (`a_sampled_run_is_attributed_to_nazm_functions_and_lines`, both backends; `each_sample_is_attributed_once_by_its_innermost_frame`). macOS's `sample` only: no `perf` in the images, and a host without a sampler is refused. No timeline or event trace |

**Limitation** · The runtime and entry objects carry no debug information. Records, enums,
sequences, channels and closures are not described; Cranelift describes no variables. No DAP: a
debugger is driven by its own commands. Sampling is macOS's `sample`; no Linux sampler here.

**Next dependency** · Cranelift value labels; the remaining types; a debug adapter; a Linux
sampler.

**Accepted when** · A native backtrace taken in a live session names a `.nz` line — the static half
is met.

---

## Evidence and infrastructure


*N66, 2026-10-02: vectorisation — VERIFIED for one idiom, `Ints` sums, on `aarch64-apple-darwin`.*
`architecture.md` §7.68 first. `ints_sum_from(start, v)` keeps the ordered checked sum's meaning exactly
— a 64-element block is added unchecked only when every partial sum is provably inside `Int` — and a
native build replaces the counted summation loop with it; `nazm explain-cost` names every other
loop's reason. Evidence, `crates/nazm-cli/tests/simd.rs` (5): every length 0–200 at three magnitudes
about the fast path's bound, equal to a scalar loop under the interpreter and both backends at `-O0`
and `-O2`; overflows at eight positions about the block boundaries, a block that would wrap, a start
too close to `MAX`, and a prefix overflow whose total fits — all failing where the loop fails; the
rewrite's IR and failure location; the interpreter's loop kept; the explain facts. A C harness of
200,000 random cases against a checked scalar sum found no difference. Measured (`performance.md`,
N66): 1.9× at 1,000 and 100,000 elements, 1.75× at 10,000,000, at `-O2`; the fallback for elements
past 2^56 is 1.33× *slower* than the scalar loop. **Limitation**: one idiom; no element-wise maps;
baseline target features only (NEON, SSE2), no per-build feature selection; x86_64 not measured.

### 28. Performance measurement — PARTIAL


*N106, 2026-10-05: the two regressions since N75 attributed, one partly recovered, both accepted.*
`performance.md` (N106). `channels` builds: N83's pool — the runtime unit grew from 38 to 62.5 KB of
LLVM text and a `switch` unit joined it; the pool is the default runtime's now, paid once per cold
build. Checking the compiler: N76's lossless tree (about a third of the analysis) and N80's
provenance (about a fifth); a comparison sort and SipHash inside them removed — contained,
the contained bench's compiler check 213.7 → 180.2 ms; the fixed-input chain back at N75's parse time. The baseline is
re-saved at N106's tree, so the bench gate holds both. Eight claims are still unreproduced, so the
area stays PARTIAL.
**Evidence** · `xtask/src/bench.rs` times five stages per program plus the compiler's own
source, with a correctness gate that refuses to time anything not producing its `.expected`
output first. One discarded warm-up, then N runs, median compared. Peak RSS captured where
readable. A regression must clear **both** 25% and 5 ms. External references are built and
timed from `bench/reference/sieve.c` and `bench/reference/sieve.py`, and a reference whose
answer differs from the Nazm program's is not timed.

*N92, 2026-10-04: performance evidence v2 — the register VERIFIED; the area stays PARTIAL.*
`architecture.md` §7.93 first. `bench/claims.toml` files each of `performance.md`'s 71 measured
sections — 10 **reproducible** by a named command (`cargo xtask contained bench`, or a measurement
test run with `--ignored`), 53 **dated** (records of the tree before the post-v1 baseline), 8
**unreproduced**: the current claims of N80, N82, N83, N84, N85, N86, N87 and N88, measured once by
the scripts their prose describes. `cargo xtask check` (**performance claims**, `xtask/src/claims.rs`)
refuses a measured section with no entry, an entry for no section, a status it does not know, a dated
or unreproduced entry with no reason, and a reproducible entry whose test or command is not in the
tree. `nazm.bench/2` (`xtask/src/bench.rs`) records the CPU model, logical CPUs, memory and kernel;
every run with p90, maximum and median absolute deviation; each executable's size; the noise floor;
how each reference's arithmetic compares; and each reference not measured, with why. New references:
`bench/reference/sieve.rs` (overflow- and bounds-checked, the closest to Nazm's meaning) and
`sieve.go` (bounds-checked, wrapping). Evidence: the gate's fixture test, the statistics and baseline
tests, a contained run's `bench/baseline-linux-aarch64.json` (replacing one older than N75 that made
`--check` report its configuration as regressions); five mutants. The run measured two regressions
since N75 and `performance.md` states them: checking the compiler written in Nazm is 54 % slower,
stepping up at N76 and N80, and `channels` builds 13–31 % slower, not located. **The area is VERIFIED
when the eight are reproducible**; the N91 section was made so by `what_debugging_and_sampling_cost`.

**Limitation** · `bench/baseline.json` (Darwin) is **stale** and not a valid comparison;
re-recording it means running generated programs on the host, which the resource contract
forbids. `bench/baseline-linux-aarch64.json` is current. Cross-host comparison is refused
rather than attempted. Every measurement under ~15 ms is below the method's noise floor.

*N34, 2026-09-29: Tokenizer-Independent Context Measurement v1, `nazm.token-cost/1` — VERIFIED;
Multi-Tokenizer Benchmark v1 — VERIFIED; G81 Tokenizer Independence — VERIFIED for context
measurement and selection; G82 Multi-Tokenizer Benchmarking — VERIFIED; G80 — PARTIAL,
strengthened; G83, G84, G85 — RESEARCH.* `nazm-tokens` (`crates/nazm-tokens/`, `architecture.md`
§7.36), a tool beside the compiler that nothing depends on (the `tokenizer reach` gate), counts
exact text under four pinned tokenizers of three families — OpenAI byte-level BPE (`cl100k_base`,
`o200k_base`, one family), SentencePiece BPE (Mistral-7B-v0.1) and SentencePiece Unigram
(T5-small) — offline, each identity carrying its library version, source revision, licence,
normalisation, special-token policy and a BLAKE3 digest checked at load. Content only: template
framing and unknown pieces are reported beside the count, and nothing is normalised first. The
seven N33 tasks, with N33's baselines unchanged, keep a 72.7–97.5 % token reduction under every
tokenizer, a median cross-tokenizer spread of 1.15 points and at most 6.25; the one-function
diagnostic is 14–20× its program under all four and is reported as the stress case it is. G81 is
verified at that scope — the planner selects by entities and bytes before any tokenizer counts, and
no tokenizer reaches the compiler, the service, the planner or a server — not as an audit of the
language's syntax decisions against tokenizers. G82 is verified: three families, pinned and
reproducible offline. G83–G85 remain research: no model was run and no cost measured. Evidence:
`crates/nazm-tokens/tests/tokens.rs` — every tokenizer's ids equal to independent Python
implementations' (`tiktoken` 0.12.0, `tokenizers` 0.22.2 with onig) on thirteen fixtures of ASCII,
Nazm, JSON, Markdown, Bangla, Arabic, emoji, combining marks, punctuation, a long identifier and
special-token strings; the families and the two SentencePiece algorithms distinct; the same counts
from every call and every registry; framing and unknown pieces reported, special-token text
ordinary; composed and decomposed text two inputs; unknown ids unsupported and never another
tokenizer; a missing, altered or non-JSON asset refused by name; large and unusual texts counted;
the tool reading only standard input; every answer valid against `schema/nazm.token-cost-1.json`.
`crates/nazm-tokens/tests/benchmark.rs` — the seven tasks under every tokenizer with every N33
mechanical check in the same pass, the report byte-identical twice, every non-stress reduction at
least 50 % under every tokenizer, worst ≤ median ≤ best; the fixed overhead attributed; source,
JSON, Markdown and three scripts compared; one selection's range in tokens (`performance.md`).
Eleven N34 mutations (area 30).

*N35, 2026-09-29: Agent Task Token Benchmark v1 — VERIFIED; Agent Cost Accounting v1 — VERIFIED;
Token-to-Correctness Benchmark v1 — VERIFIED; G83, G84, G85 — VERIFIED within the benchmark's
scope, one model configuration; G80 — PARTIAL, materially strengthened; G68 and G69 — PARTIAL,
strengthened.* `nazm-agent-bench` (`crates/nazm-agent-bench/`, `architecture.md` §7.37),
evaluation tooling nothing depends on (the `network reach` and `tokenizer reach` gates), puts eight
tasks over the seven N33 scenarios — understand, edit, diagnose, documentation, test selection,
Scenario C among them — to a real model with the naive baseline and with `nazm repo --task`'s
context, byte for byte, digest-addressed, one system text, wording, contract and parameter set for
both. Every answer is scored fact by fact, offline, against truths read from the authorities;
hallucinations are counted apart from misreadings; usage is recorded raw and normalised; dated
prices are applied in integer pico-USD, never in an identity, with input, cache reads, cache writes
and output priced separately. **Scope:** `claude-haiku-4-5-20251001` through the Claude Code client
2.1.283 in print mode — no tools, no thinking, the model's default temperature, which the client
does not set — two trials, suite `b7db4ccc…`, prompt `nazm.agent-bench-prompt/1`, at the prices of
2026-09-29. The N33 context solved 12 of 16 requests to the baseline's 6, 112 of 116 facts to 95,
with no hallucination in either arm, 94.36 % fewer input tokens, 94.05 % fewer total tokens, and
95.39 % less cost as billed or 92.86 % with every input token uncached; 4,141 tokens and $0.0064 per
solved task against 139,311 and $0.2781. Correctness was preserved on 7 of 8 tasks: on T2 the N33
arm had every fact and added 32 false claims (builtins listed as definitions) while the baseline
gave no valid answer, which the pre-declared rule scores as baseline better. On the 123-byte
diagnostic both arms solved it and raw source was the cheaper input, 744 tokens to 1,367; every
other task's N33 context was cheaper, so for this model the crossover lies between 744 and 6,720
input tokens. G83–G85 are verified for that configuration only: the direct OpenAI and Anthropic
adapters are tested against recorded responses and were not run, because no key was supplied. G80
stays partial — one repository, one model, eight tasks. Evidence: `crates/nazm-agent-bench/tests/
agent.rs` — the suite's categories, Scenario C, the arms' distinct digests and the N33 arm equal to
the planner's own bytes; one prompt for both arms, as the provider receives it; a perfect answer
solving every task under either arm; missing, wrong and hallucinated claims counted apart; a bare
name only when unambiguous; extra and invented citations; invalid output and refusal; exact cost
arithmetic per category; zero denominators absent; identities and resume; the plan and its
alternation; the cap before and during a run; bounded retries never counted incorrect; model drift;
each adapter's normalisation and redaction; the report deterministic in any order; every record and
report valid against `schema/nazm.agent-benchmark-1.json`; the dry run with no credential and no
network. The results: `tools/agent-bench/results/`, `performance.md`. Sixteen N35 mutations
(area 30).

**Next dependency** · A Darwin machine on which running generated programs is contained.

**Accepted when** · Both baselines are current and recorded under containment.

### 29. Differential testing — VERIFIED

**Scope (v1, Q1-D-20)** · The comparisons with the compiler written in Nazm cover what it carries —
its declared subset, which does not include Gate 2's numbers, arrays, `Bytes`, `Text` or the Gate 2
and Gate 3 built-ins it lists as not carried (`selfhost.rs`, `NOT_CARRIED`). Every other mechanism
here compares the interpreter, the LIR oracle and both backends over the whole language.

**Evidence** · Five independent mechanisms. `crates/nazm-cli/tests/build.rs` — two oracles
per case, both opt levels. `crates/nazm-cli/tests/concurrent.rs` — the same for
concurrency. `crates/nazm-cli/src/test.rs` — `nazm test` runs every case interpreted and
native and requires agreement. `crates/nazm-cli/tests/selfhost.rs` — the Nazm-written
lexer, parser and checker against the Rust ones, token-for-token and span-for-span, over
every `.nz` in the tree including their own sources, and eleven programs built by **both**
compilers and required to report the same reclamation — plus, since N10.1, the 32-case
transfer corpus run five ways. `compiler/bootstrap.sh` — at N10.1, thirteen conformance cases (twenty at v1), six of
them the N10.1 transfer shapes and one the original reproducer byte for byte, and seven
multi-module ones, under C2, C3 and the interpreter. *Since N12, propagation: 22 programs
through both checkers by code and span, four `?` programs built by both compilers with equal
reclamation reports, and a `results.nz` and a `Result`-library module case in the corpus.*
*Since N12.1, value blocks: 26 programs through both checkers by code and span, 10 parser
trees, six block programs built by both compilers with equal reclamation reports, the core
prelude found by key with the prelude last, between two project modules and absent, and a
`blocks.nz`, a value-block module case and three refusals in the corpus.*

**Limitation** · The interpreter is the oracle, so a shared specification error is
invisible to all five. `docs/spec.md` names what must be settled before
`nazm test --backend=all` would mean anything.

**Next dependency** · None.

**Accepted when** · Met.

### 30. Mutation testing — VERIFIED · Fuzzing — PARTIAL (N73; MISSING until then)

*Q1, 2026-10-08: the fuzz campaign re-run contained over all five targets, 300 s each* —
`front` 271,640 executions, `lower` 280,060, `run` 190,551, `lir` 272,626, `manifest` 2,388,876:
no crash; 36 `run` inputs blocked past two seconds and were counted, not kept (Q1-F-01,
`docs/limitations.md`). Log: `target/gates/q1/fuzz-contained.log`. Q1's mutation campaign is
recorded below its fixes (`docs/security-qualification.md`, *Mutation*).

*N108, 2026-10-06: the whole catalogue at the release gate — 1,268 of 1,268 caught.* Contained,
targeted with every killer verified (`releases/920a2a5.md`). Thirty-four entries were first undecidable
on Linux — killers verified on macOS only, and nine whose code N90, N104 and N105 had moved — and were
repaired; eight are observable only with macOS tools and are verified there. Every fuzz target ran 300 s
at the gate, 0 crashes.

*N105, 2026-10-05: a fifth fuzz target and an oracle tier.* `lir` (`fuzz/src/main.rs`): a checked
program's instruction-level LIR must validate and run on LIR's interpreter without a defect; run
contained for 300 s — 319,609 executions, 19,245 edges, no crash, no hang — and its minimised corpus
(532 inputs) is `fuzz/corpus/lir/`. The N73 differential gains LIR's interpreter as a fifth tier. The
mutation catalogue: 76 entries on code N105 deleted repointed, 18 retired with their reasons, 8 new
(`n105-*`), each new or repointed one caught by its named killer on the host.

*N101, 2026-10-05: no silent tier.* `architecture.md` §7.102. N100's one survivor,
`n79-a-held-io-cap-is-read-alone`, is reachable and caught by
`authority::a_held_io_cap_authorises_its_own_print`; N100 had probed it in a directory with a warm
check cache, whose key does not change under a mutant (runbook: a probe runs in a fresh directory).
N100's four without a verdict have focused killers. The 175 other entries without a declared killer
were applied one at a time and each was caught: 132 in a dedicated host worktree, 43 selfhost and
bootstrap entries under the contained runner. Of **1,255** entries, **1,254** declare a killer;
`n13-native-operands-are-released-before-they-are-compared` is a known survivor, its reason in
`no_killer`. `xtask/src/plan.rs` refuses an entry with neither (`an_entry_without_a_killer_must_say_why`).
Not claimed: that a declared killer is a minimal one, or that zero survivors would mean no defect.

*N93, 2026-10-04: coverage-guided fuzzing — VERIFIED for four targets at the stated budget; fuzzing
stays PARTIAL.* `architecture.md` §7.94 first. `fuzz/` (its own workspace): the compiler's crates
built with SanitizerCoverage (`trace-pc-guard`, a stable `rustc` option), the edge callbacks in
uninstrumented C (`fuzz/cov.c`), and an engine that keeps an input when it reaches a new edge and
mutates kept inputs by bytes, whole tokens of the language, whole lines and spliced lines, and names
renamed. Targets: `front` (parse and check), `lower` (Core IR, MIR, and MIR's validator over every
checked program), `run` (the interpreter, each execution bounded to 2 s — a program may block
forever, which the language allows), `manifest`. `cargo xtask contained fuzz [SECONDS] [TARGET…]`;
the minimised corpora are committed as `fuzz/corpus/TARGET/` and replayed by the ordinary suite
(`crates/nazm-cli/tests/fuzz_replay.rs`, 1,826 inputs, about 2 s). **Campaign** (contained, four
CPUs, 300 s per target): `front` 318,377 executions, 13,293 of 109,890 edges, 487 minimised inputs;
`lower` 338,902, 17,467 edges, 531; `run` 221,834, 14,522 edges, 503, 35 inputs that block; `manifest`
2,995,846, 2,347 edges, 298 — **no crash**. A minimised set reaches 99.3 % or more of the edges its
campaign reached, not always all: the remainder are edges whose reaching varies between runs (threads, hash order).
No defect was found, so none gained a regression or a mutant; the engine is outside the workspace the
mutation catalogue covers. Not here: sanitizers (no `unsafe` in the compiler), Miri, Loom, backend and
FFI differential fuzzing beyond N73's generator, registry archives, a contract VM.

**Evidence** · `xtask/mutations/mutations.toml` holds **686** catalogued defects (158 when
this sentence was first written, 233 at N12.2, 266 at N13, 288 at N14, 304 at N15, 318 at N16, 334 at N17, 348 at N18, 362 at N19, 376 at N20, 391 at N21, 404 at N22, 418 at N23, 435 at N24, 449 at N25, 457 at N26, 473 at N27 with its closure correction, 493 at N28, 509 at N29, 527 at N30, 542 at N31 before its closure correction, 549 after it, 568 at N32, 575 at N32-H2, 582 at N32-H3, 601 at N33, 612 at N34, 628 at N35, 643 at N36, 662 at N37), **498** of them with a verified killer and all
of them owned by one of fourteen focused profiles;
`xtask/src/mutate.rs` injects each under a kernel advisory lock with a write-ahead backup,
requires the pattern to match exactly once, rebuilds, runs the suite and restores. It
distinguishes seven verdicts and refuses to fold a non-result into "survived" — a
distinction added after an incident that produced 19 false survivors. Lifecycle behaviour
is tested in `xtask/tests/lifecycle.rs`.

*N4 added six on 2026-09-22 and ran all six contained: four remove one input each from a
check key — the module's own source, its dependencies' interfaces, the checker's identity,
and which module it is — one stops an entry's recorded inputs being checked against the key
it is filed under, and one publishes an entry for a module that did not check cleanly. Each
is a wrong answer rather than a slow build, which is why they were worth the run.*

*N2 added two and ran four contained on 2026-09-22 — `every-definition-is-exported` and
`imports-become-transitive`, plus the two existing entries in the files N2 rewrote most, to
confirm their patches still apply. All four were caught, by 8 to 18 tests each. N2.1 added
four more against the **self-hosted** path — the `pub` bit ignored, resolution across every
module, the first ambiguous import winning, and any `main` accepted as the entry point —
and all four were caught by the parity suite. Both are targeted runs, not catalogue runs;
the row below still stands.*

*N9 added fifteen on 2026-09-23 and ran all fifteen contained and serial, in four
batches.* Each attacks the composition rather than the syntax: a copy that forgets a field,
a drop that omits one, a drop that does not recurse, a replacement that releases before it
takes, a projection out of a temporary that does not preserve ownership, a construction
that fails and leaks what it built, a task-safety derivation that stops at the first level,
an interface that omits its records, a published field that loses its type, a published
record named by its spelling, a layout that follows the declaration, a field read from the
wrong position, a recursive record accepted, a private type leaked through a public
signature, and the **self-hosted** emitter forgetting a record field. All fifteen are
caught; two runs were needed, because one survived and one did not compile.

The survivor is the interesting one and it is written up in `architecture.md` §7.10: a
replacement that releases before it takes is invisible to the reclamation counters, because
both orders end with the same two numbers. The one that did not compile is a different
verdict and was reported as one rather than folded into a pass — which is what the
seven-verdict distinction exists for.

*N10 added twenty-two on 2026-09-23 and ran all twenty-two contained and serial, in five
batches, with nothing else running.* Eighteen attack the composition becoming dynamic: an
enum's properties taken from one variant, a discriminant from the parser rather than from
the names, a payload laid out as written, a published enum in source order, an exported
enum missing from the interface, a helper branching on the wrong tag, a copy that takes no
reference, a `match` that does not take over what it matches and one that never lets go, a
partly built variant left uncleaned, a task that does not give back what it borrowed, the
five checker rules — coverage, a repeated arm, another enum's variant, a missing payload
field, a private payload type — and the **self-hosted** checker skipping exhaustiveness and
the self-hosted emitter not taking over a payload.

Four more came out of the one that survived, and they are the interesting ones.
`a-variant-slot-keeps-what-the-last-round-left-in-it` was injected, compiled, and the suite
still passed. Asking why found that a `break`, a `continue` or a `return` out of a
half-evaluated expression gave back nothing it was holding — a defect of **N8's**, in both
backends and in the compiler written in Nazm — and then that the mutation itself no longer
described a defect, because the slot it zeroes is zero on every path that reaches it.
`architecture.md` §7.11 writes it up. It was retired and replaced by four that do describe
one: a transfer that keeps the temporaries, one that keeps the values being built, one that
releases the temporaries of the loop *around* it — a premature free rather than a leak —
and the same omission in the self-hosted emitter.

*N10.1 added thirteen on 2026-09-23, repointed two whose text the correction moved, and
ran all fifteen contained and serial in five batches of three, with nothing else running:
**fifteen caught, none survived**, none unapplied, none that did not build, no timeout.* Each
puts back one half of the missing representation or one thing the audit found: an `if`
typed by the branch that left — the root cause — a join opened that no branch reached, for
an `if` and for a `match`; a node emitted though control never reaches it, and the same for
assignment alone; a `phi` read from a fixed two incomings; an unreachable node's operands
left on the stack; a `break` and a `return` that leave a `scope` without joining it; the
Nazm checker silent after a transfer; a value that never arrives given no type; and `==`
on strings keeping its operands, in each backend. The two repointed —
`the-self-hosted-emitter-skips-one-release-path` and
`the-self-hosted-compiler-keeps-what-a-continue-abandons` — were caught as before. Targeted
batches, not a catalogue run.

*N10.2 added fifteen on 2026-09-23, repointed one, and retired one it superseded, and ran
the sixteen contained and serial in six batches with nothing else running: **sixteen
caught, none survived**, none unapplied, none that did not build, no timeout.* They are the
ways the three completions can collapse into two — no value read as a value, a value that
never arrives refused as none, an `if` with no `else` taken to leave, a `continue` taken to
complete — each half of the `if` join taking the wrong branch's type, two types accepted, a
value with nothing accepted, a `Unit` arm leaving its `match` a value, a body and a
`return` not held to the signature, the arms of a refused `match` still checked, and the
emitter's discarded arm value kept, or discarded without being given back. The repointed one
is N10.1's `N0313` entry, whose line the block's rewrite moved;
`an-if-is-typed-by-the-branch-that-left` is retired because its rule is now one line of the
three-state join, which `a-then-branch-that-leaves-still-gives-the-type` mutates exactly.
Targeted batches, not a catalogue run.

*N11 added twenty-six on 2026-09-23 and repointed four the diagnostics migration moved, and
ran them contained and serial in five batches with nothing else running.* By category —
**type system** (six: parameter order forgotten, wrong arity accepted, inference keeping the
first conflicting type, `==` on a parameter, two definitions of one shape unified, parameter
names persisted in the interface), **native instances** (four: a symbol built from the first
argument alone, an instance planned twice, an instance's own calls never planned, polymorphic
recursion not counted as expanding), **`Vec` ownership** (five: push without a reference of
its own, get handing out the vector's, set releasing before taking, pop copying instead of
transferring, destruction without the elements), **cycles** (two: a `Vec` hiding what it
owns, every parameter counted as used) and **the compiler written in Nazm** (nine, across
all four). First run: **twenty caught, six survived**, none unapplied, none that did not
build, no timeout. The six were two holes and one equivalence:

* `two-generic-definitions-of-one-shape-unify` and `an-instance-symbol-keeps-only-its-first-argument`
  had no test that could see them — no call passed one generic record where another of the
  same arity was declared, and no two instances differed only after their first argument.
  Now `inference_does_not_unify_two_generic_definitions_of_one_shape` and
  `two_instances_that_differ_only_in_a_later_argument_are_two_functions`;
* `vec-get-returns-a-borrowed-element`, in both compilers, is the N8 lesson a third time:
  every shape bound what it read, and a binding takes a reference of its own. Now
  `an_unbound_vec_read_outlives_the_element_being_replaced`, in both, reads an element into
  an argument and replaces it in the next argument, and churns the allocator in the callee;
* `vec-set-releases-before-it-takes`, in both, survived a second run and is **equivalent**:
  the order is observable only when the new value's one reference is the slot it replaces,
  and `vec_get` always gives out a reference of its own, so no program can build that. Both
  are retired, with the reason in the catalogue.

Rerun: **all four remaining caught**. Final: **24 of 24 live mutations caught**, 2 retired as
equivalent. Targeted batches, not a catalogue run; the catalogue is 182 entries.

*N12 added twenty-six on 2026-09-24 and ran all twenty-six contained and serial, in six
batches, with nothing else running.* Result identity — `?` recognising `Result` by name, by
an `Ok`/`Err` shape, or as the first enum of that name; a project path taking the `@core`
key; the prelude left out of every check key; rehydration giving each interface its own
types. The rule — `?` outside a `Result` function, the error type ignored, the success type
demanded equal, a project type taking a prelude name. Control flow — the lowered `match`
testing the error tag, returning the operand's `Result`, falling through on error, and the
interpreter carrying on with the error. And in the compiler written in Nazm, the same checks
plus ownership and the scope join: the error or the success value not retained, the operand
leaked on success, the join skipped, earlier temporaries or the frame left unreleased, the
error path falling into the success path, the prelude never imported. **All twenty-six
caught on the first run** — none survived, none unusable, none unapplied, none without a
verdict, and none retired. Targeted batches, not a catalogue run; the catalogue is 208
entries.

*N12.1 added twenty-five on 2026-09-24 and re-pointed two of N12's,* whose patterns the
`Option` dogfood had moved; their defects were unchanged, so they were rerun rather than
retired. Run contained and serial, with nothing else running. Prelude identity in the
compiler written in Nazm: the last module taken as the prelude, any module called `prelude`,
a project path keeping the `@core` key, the prelude losing its key, the root module's
`Result`, the reserved-name rule following position — and in the reference graph, the last
module named the prelude. The reference backend: an arm block dropping its statements, a bare
block emitted as its tail alone, a block's value released as it leaves, a statement block
skipped, a statement `match` claiming every arm leaves, an arm that never rejoins, a
scrutinee never let go, a tail emitted after a transfer. The compiler written in Nazm: a block
operand refused or given an empty span, a statement block needing a `;`, every block
expression a value, a diverging one `Unit`, a block expression emitted as something. The
dogfood: `None` confused with `Some`, the `-1` brought back for a compilation with no
prelude, a check's arms reversed, a found declaration discarded. **All twenty-seven caught.**
None survived, none unusable, none unapplied, none retired.

One needed three runs, and the reason is the suite's rather than the compiler's. *A
statement `match` claiming every arm leaves* drops a loop's increment, so the compiled
programs it produces never finish — and three places ran compiled programs with no deadline
(`value_blocks.rs`, the corpus run in `schemas.rs` and `workflow.rs`), while `run_bounded`
killed a stage but not what the stage had started. The first run ended at the batch's 3000 s
deadline with no verdict, and the second was stopped when it hung in the next such place; a
timeout was not counted as a catch. Each now fails its case under a deadline and kills its
process group, and the third run caught the mutant with 25 failing tests. The catalogue is
233 entries.

*N12.2 (2026-09-24/25) replaced the runner's evidence shape without changing what a verdict
means, and then ran the whole catalogue for the first time.* The harness is
`xtask/src/campaign.rs`, `xtask/src/plan.rs` and `xtask/src/procs.rs`, operated as
`docs/runbook.md` "Mutation campaigns" describes: per mutant, a verified **killer** (tier 1),
then a **focused profile** (tier 2), then the **workspace suite** (tier 3), stopping at the
first failure. A tier-1 or tier-2 catch is the same evidence as a tier-3 one — the test is a
member of the suite, passed in the baseline and failed under the mutant — and only tier 3 can
report SURVIVED. A timeout is its own no-verdict. `--strategy full` keeps the legacy shape
as the oracle. Lifecycle is in `xtask/tests/lifecycle.rs`: every tier transition, a deadline
at each tier, stale killer metadata, build reuse across four mutants, interruption and
resume, resume refused for another tree or catalogue, and the watchdog under SIGKILL and
SIGINT.

Measured, contained, serial, nothing else running:

| run | mutants | wall | per mutant, median |
| --- | --- | --- | --- |
| N12.1's catalogue batches, legacy, productive time | 27 | 10,782 s (13,870 s with the no-verdict reruns) | ≈ 240 s |
| the same 27, v2, profiles only | 27 | 1,807 s | 52 s |
| the same 27, v2, verified killers | 27 | **523 s** | **1.4 s** |
| differential sample, `--strategy full` | 10 | 3,058 s | 246 s |
| the same 10, `--strategy targeted`, profiles only | 10 | 945 s | 23 s |
| **whole catalogue, targeted** | **233** | **10,686 s**, five resumable sessions | 22 s |

The differential sample spans the checker, native symbols, memory, the interface, the cache
key, the compiler written in Nazm, the agent harness and the parser: **identical verdicts**
(ten caught under both) and identical original, mutated and restored digests. Two mutants
that crash a binary gave MUTANT CRASH under both strategies too.

**The whole catalogue, on `f444d0d`:** 233 selected, 233 finalized. **228 caught** — 38 at
tier 1, 185 at tier 2, 5 at tier 3. Six workspace-suite runs in all; 227 avoided. And five
problems, each accounted for rather than folded into a pass:

* **2 UNUSABLE** — `a-sequence-does-not-retain-the-string-it-stores` and
  `a-published-field-loses-its-type`: their *replacements* had drifted from the code (a
  renamed field; a field type that is no longer a string), so the mutants did not compile.
  Repaired, rerun, **caught**.
* **1 SURVIVED** — `a-record-that-contains-itself-is-accepted` disabled
  `UserTypes::layout_cycle`, which has had no caller since N11 moved the checker to
  `ownership_cycles`. An equivalent mutation of dead code; **retired**, and
  `an-inline-containment-cycle-is-not-refused` attacks the rule where it lives: **caught**.
* **2 MUTANT CRASH** — `nesting-unbounded` and `stack-not-sized-from-the-budget` overflow the
  stack inside the `nazm-syntax` and `nazm-core` test binaries. The legacy strategy gives the
  same verdict. The effect is observed; no test *asserts* it. Open — the cases must move
  out of process, which is a change to those crates' tests and not N12.2's to make.

*N13 (2026-09-25) added thirty-three and repointed thirteen,* each new one written with the
semantic test that observes it and its killer confirmed by `--verify-killer` before it was
committed. They break the capability derivation (a `Vec`, a sequence, a `Chan` given
equality; only the first component, only the first variant, no nested type walked), the
checker (two types of one shape compared, a refusal that names nothing), the interpreter
(first field only, variants not compared, `!=` not the negation, `Str` by length), the
native helpers (first field only, tags not compared, a zero-payload variant unequal to
itself, arms in the wrong order, `!=` not negated, each temporary leaked, helper names
without their type arguments, operands released before they are read), the Nazm-written
checker and emitter (the same families), and the dogfooded completion fold. Nine of the
thirteen repointed entries compared a completion with `99`; the enum made that
inexpressible, so they became typed (`x != x`) — the invalid state they simulated no longer
exists. **Campaign `418a5e5921b866cc`, targeted, one session, 1,997 s:** the 33 new, the 13
repointed and 12 existing entries whose target code N13's diff touched or sits beside — 58
selected, 58 finalized. **57 caught**, 45 at tier 1 and 12 at tier 2, none at tier 3; no
timeout, no runner error, nothing unapplied or unusable, no crash; the workspace suite ran
once. **1 SURVIVED**: `n13-native-operands-are-released-before-they-are-compared`, a real
use-after-free — the helper reads operands whose storage was just released. It is **not
equivalent** and is **not counted as caught**. It survives because nothing allocates between
the release and the read, so the freed bytes are unchanged and every answer and counter is
right; glibc 2.41 in the image ignores `MALLOC_PERTURB_` and `glibc.malloc.perturb`
(measured), so **no deterministic direct killer exists under the supported allocator and
harness**. It stays **live** in the catalogue, with the `native` profile and a `no_killer`
reason, as a known surviving mutation: a mutation-sensitivity limitation of this suite, not
a verdict about the defect. Three
candidate killers were refuted by verification and not used. The whole catalogue was not
re-run: N13 did not change the harness, and the two open MUTANT CRASH entries were not
reached by it.

*N14 (2026-09-25) added twenty-two and repointed six,* each with its test and a killer
verified by `--verify-killer` (27 of 27 verified). They break the body rule (a requirement
ignored, never recorded, keyed by position across owners), what may be written (any name a
capability, a record's parameter allowed), the call (arguments never checked, only the first,
written or inferred ones skipped, a caller's parameter always satisfying), the interface (a
requirement not published, not read back, not validated, the schema or the epoch left
behind), the parser, and the Nazm-written parser and checker. **Campaign
`05f7bfb4d7869a2d`, targeted, one session, 1,025 s:** the 22 new, the 6 repointed, and 12
existing entries on the code N14 changed or beside it — among them N13's four capability-walk
and nominality mutants, because N14 changed that walk's leaf rule. **40 of 40 caught**, 36 at
tier 1 and 4 at tier 2; no survivor, nothing unusable or unapplied, no crash, no timeout; the
workspace suite never ran. N14 did not touch the native equality code, so N13's known
use-after-free survivor was not re-run and stays as it was: live, SURVIVED, no killer.

*N15 (2026-09-25) added sixteen and repointed three.* Of those nineteen entries, **seventeen
list a killer** — the sixteen new ones and the repointed `a-failed-body-drops-the-function` —
and each was verified by `--verify-killer` (17 of 17: pristine passes, mutant fails, restored
passes), first in a session a host build overlapped and then again uncontended, contained at
P1, with the same result. The other two repointed entries, `a-block-does-not-count-its-contents`
and `parentheses-count-as-a-node`, list no killer, as before N15; their tier-2 profile decides
them, and the campaign below caught both there. The seventeen killers are fourteen distinct
tests: `a_lossless_tree_preserves_every_byte_and_recovers_inside_a_function` kills three
entries and `the_abstract_tree_keeps_its_contract_for_a_body_that_recovered` two. They break the tree's promises — whitespace, comments, a refused character or the
trailing trivia missing from it, a leaf normalised or spelled from its kind, trivia inside
the construct before it, nodes at one position nested inside out, a token given the wrong
kind — and recovery's: abandoning the block as before N15, consuming nothing, swallowing
the next statement, leaving no error node, handing a recovered body to the checker,
reporting a cascade, forgetting the braces the failed statement opened. **Campaign
`f1f15052a1be61f0`, targeted, P1, one session, 1,015 s:** every entry on
`crates/nazm-syntax` — the 16 new and the 10 existing parser entries, the three repointed
among them, because N15 rewrote the code they target. **25 caught**, 19 at tier 1 and 6 at
tier 2; no survivor, nothing unusable or unapplied, no timeout; the workspace suite never
ran. **One MUTANT CRASH, `nesting-unbounded`**, the known open entry above, as it was: with
the nesting limit removed the parse stack overflows, as it did at N13 and N14. N15 did not
touch native equality, so N13's use-after-free survivor was not re-run and stays live,
SURVIVED, no killer.

*N16 (2026-09-25) added fourteen,* each with its regression test and a killer verified by
`--verify-killer`, uncontended, contained at P1: 13 in one session, and the fourteenth —
`n16-a-broken-buffer-keeps-the-last-clean-analysis` — after that session reported its first
killer **not a killer**. That was right: the edited document cannot keep an old analysis,
because a change replaces it with none, so the defect reaches only the *other* open
documents; the killer is now the test that asks an importer a question after its dependency
broke, and it verified. The fourteen break what an editor relies on — the disk winning over
a buffer, a closed buffer staying authoritative, an edit reaching only the edited document, a
stale analysis kept or a stale version answered, an empty set not published, a local found by
its spelling, an imported definition pointing at the caller, the prelude given a path, a
position past a name selected, a column counted in bytes either way, a log line on stdout,
completion advertised. **Campaign `7eabdfd883ebbae8`, targeted, P1, one session, 1,017 s:** the
fourteen and `an-absolute-path-reaches-the-module-key`, whose file moved to
`crates/nazm-service/src/load.rs`. **15 of 15 caught**, 14 at tier 1 and that one at tier 3; no
survivor, crash, timeout or unusable entry. Tier 3 was a finding: the moved loader had no
focused profile, where in `nazm-cli` it had been the `cli` profile's. The profile now owns
`crates/nazm-service/src/` and runs the service's suites, and that one entry, re-run alone,
was caught at tier 2 by `nazm-cli --test source_root` — the suite that caught it before N16.
N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are
as they were.

*N17 (2026-09-25) added sixteen and repointed one,* each killer verified by `--verify-killer`,
uncontended, contained at P1, one at a time. The first killer tried for
`n17-an-old-version-answers-references` was reported **not a killer** — its version-1 query
lands in a comment of the version-2 text, which answers nothing either way — and the thousand-
edit test, which asks about version 999 at a name version 1000 still has, verified instead.
The sixteen attack the index's invariants: a slot recorded without its function, a use given
its function's first slot, and same-named functions, fields and variants merged by spelling; an
applied record's field not taken to its definition; a cross-file use dropped; a declaration
listed as a use; uses left in hash order; the checker not recording a construction label's
field; only the asking compilation searched; an imported use placed in the asking file; one use
listed once per compilation; an old version answering; the declaration always, and never,
included. `n16-a-local-is-found-by-its-spelling` was repointed at the index, where a local's
declaration now comes from. **Campaign `a2fedab55b657d80`, targeted, P1, one session, 934 s:**
those seventeen and two unchanged N16 entries on code references now share
(`n16-an-imported-definition-points-at-the-caller`, `n16-a-stale-version-is-answered`). **19 of
19 caught at tier 1**; no survivor, crash, timeout, unusable or uninjected entry, no runner
error. The `references` suite joined the `cli`, `semantics` and `checker` profiles. N13's
use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as
they were.

*N18 (2026-09-25) added fourteen,* each killer verified by `--verify-killer`, contained at P1,
one at a time. `n18-a-plan-ignores-its-text`'s first killer was reported **not a killer**: the
forged text also moved the edited bytes, so the per-edit check refused the plan first; the test
now forges a text that differs only outside the edits, and that killer and
`n18-a-plan-outlives-its-generation`'s, which shares the test, were verified again against it.
`n18-a-written-type-is-not-recorded`'s first verification overlapped a small host build and was
repeated alone. The fourteen attack N18's invariants: a written type and an enum qualifier not
recorded; a rename found by its spelling; an exported entity renamed; the candidate not checked;
its meaning not compared; the comparison made at unmoved offsets; a program with errors renamed;
any name accepted; a plan outliving its generation or ignoring its text; a closed file edited
without a version; an edit losing its version; an unversioned client sent an edit. **Campaign
`0bc0e71f921e7a6e`, targeted, P1, one session, 873 s:** the fourteen and four N17 identity
entries on index code N18 extended (a local's slot, same-named fields and variants, an applied
record's field). **18 of 18 caught at tier 1**; no survivor, crash, timeout, unusable or
uninjected entry, no runner error. The `rename` suite joined the `cli`, `semantics` and
`checker` profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were
not re-run and are as they were.

*N19 (2026-09-25) added fourteen, repointed one and renamed one,* on the frozen final source.
Every killer was verified by `--verify-killer`, contained at P1, one at a time, with no host
build running — the fourteen, and the two N16 entries whose killer,
`the_server_speaks_the_protocol_end_to_end`, N19 edited: 16 of 16, none rejected, and no source
changed after. The fourteen attack N19's invariants: a `let` visible in its own initializer, a
later local visible early, a shadowed binding offered, a closed scope left open, a refused
duplicate offered, locals in walk order, a type parameter leaking out of its definition, a
refused import offered, a type offered as a value, a function offered as a type, the prefix
ignored, a completion inserting rather than replacing, and a recovered function or file offered
names. `imports-become-transitive` (N2) was repointed — its pattern gained the line that starts
the environment's refused-name list — and `n16-completion-is-advertised` became
`n16-signature-help-is-advertised`, the same defect with a capability the server still lacks.
**Campaign `d2e4e1af3eba6c1f`, targeted, P1, one session, 994 s:** those seventeen. **17 of 17
caught — 16 at tier 1, and `imports-become-transitive`, which names no killer, at tier 2 by
`nazm-cli --test visibility`**; no survivor, crash, timeout, unusable or uninjected entry, no
runner error. The `completion` suite joined the `cli`, `semantics` and `checker` profiles.
N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are
as they were.

*N20 (2026-09-26) added fourteen and renamed one,* on the frozen final source. Every killer was
verified by `--verify-killer`, contained at P1, one at a time, with no host build running — the
fourteen, and the two N16 entries whose killer, `the_server_speaks_the_protocol_end_to_end`,
N20 edited: 16 of 16. A fifteenth candidate — the argument list's upper bound moved from the
`)`'s start to its end — was **not a killer's mutant at all**: `--verify-killer` found the
mutant passing, because the walk down the tree enters only a node strictly containing the
position, so either guard alone keeps the byte after `)` out of the call. It is recorded as a
comment in the catalogue, not catalogued; the behaviour stays pinned by a test case. The
fourteen attack N20's invariants: a rejected call not recorded, written type arguments recorded
as inferred, inferred ones discarded, instantiated parameters recorded as declared, the outer
call chosen, every argument the first, a nested comma or a type argument's comma counted, a
position before `(` in the call, a recovered compilation answering, no parameter reported as
a parameter, hover and signature help disagreeing, a requirement hidden, and an absent active
parameter sent past the end. `n16-signature-help-is-advertised` became
`n16-document-symbols-are-advertised`, the same defect with a capability the server still
lacks. **Campaign `ea2f47200328923f`, targeted, P1, one session, 1,003 s:** those sixteen, and
seven N16 and N19 entries on the code signature help shares — a stale version answered, a broken
buffer keeping the last clean analysis, an edit reaching only its own document, the disk winning
over a buffer, a closed buffer staying authoritative, an incoming column counted in bytes, a
recovered file answering. **23 of 23 caught at tier 1**; no survivor, crash, timeout, unusable
or uninjected entry, no runner error. The `signature` suite joined the `cli`, `semantics` and
`checker` profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were
not re-run and are as they were.

*N21 (2026-09-26) added fifteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 15 of 15. They
attack N21's invariants: an unknown field or variant not recorded, a member offered any
record's fields, an applied generic's fields unsubstituted, a variant offered any enum's, a
payload label offered another variant's, a label given elsewhere offered, the label being
edited counted as given, declaration order shown, the active field counted by commas, the
outer construction chosen, a construction inside an argument answered as the call, a
recovery ignored, a recovery in the site's own function answered, and an unmarked constructor
marking the first parameter. **Campaign `443aa47f6a8821cd`, targeted, P1:** those fifteen, and
nineteen N16, N17, N19 and N20 entries on code N21 edited or shares. **34 of 34 caught at tier
1**; no survivor, crash, timeout, unusable or uninjected entry, no runner error. It took three
sessions: the first ran out of its budget with 20 finalized while the host's load average was
near 70 from other processes; the second refused to start because its baseline suite had 2
failing tests, under the same load — the harness counts them and does not name them, and the
same suite then passed contained at P1 on the frozen tree, 1,461 of 1,461, so they were not
identified; the third resumed and finished the other 14. The `structure` suite joined the `cli`,
`semantics` and `checker` profiles. N13's use-after-free survivor and the `nesting-unbounded`
MUTANT CRASH were not re-run and are as they were.

*N22 (2026-09-26) added thirteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 13 of 13. They
attack N22's invariants: an outline listing imported declarations, record fields flattened
beside their record, a payload field identified as another variant's, a selection that is the
whole declaration, an outline in name order, workspace symbols merged by spelling or across
files at the same offsets, private declarations hidden from workspace search, a stale outline
served, a payload field's container missing its enum, workspace order ignoring the name, and
the protocol's selection range and workspace location set to the whole declaration. One
killer was first written against the ordering test that queries a single name, where the
mutant is invisible: it was verified NOT A KILLER, moved to the query test whose names differ,
and verified there. `n16-document-symbols-are-advertised` became
`n16-code-actions-are-advertised`, the same defect with a capability the server still lacks; it
and `n16-the-server-writes-to-stdout`, whose killer test changed, were re-verified: 2 of 2.
**Campaign `e54921e1bd294310`, targeted, P1, one session, 1,107 s:** those fifteen, and nine N16
and N17 entries on code N22 reads — the disk winning over a buffer, a closed buffer staying
authoritative, a stale version answered, the prelude given a path, an imported definition
placed in the asking file, an outgoing column counted in bytes, same-named fields and variants
indexed as one, and one use listed once per compilation. **24 of 24 caught at tier 1**; nothing
at tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry, no runner error. The
`symbols` suite joined the `cli`, `semantics` and `checker` profiles. N13's use-after-free
survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as they were.

*N23 (2026-09-26) added fourteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 14 of 14. They
attack N23's invariants: a parameter classified as a local, declarations never marked, every
occurrence marked a declaration, a payload label classified as a local, an unresolved name
classified, an identifier emitted twice, a stale classification served, the checker recording
no built-in type or no type-parameter use, a refused signature keeping its type parameters, an
intrinsic sent without `defaultLibrary`, overlapping identifiers encoded, a start column sent
absolute on the previous token's line, and range tokens advertised.
`n16-the-server-writes-to-stdout` and `n16-code-actions-are-advertised`, whose killer test
changed again, were re-verified: 2 of 2. **Campaign `45707a4b609ee4d4`, targeted, P1, one session,
1,219 s:** those sixteen, and ten N16, N17 and N18 entries on what N23 reads — a stale version
answered, the disk winning over a buffer, a closed buffer staying authoritative, an outgoing
column counted in bytes, same-named functions, fields and variants indexed as one, a local taken
as its function's first slot, and a written type or an enum qualifier not recorded. **26 of 26
caught at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or uninjected
entry, no runner error. The `semantic` suite joined the `cli`, `semantics` and `checker`
profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run
and are as they were.

*N24 (2026-09-26) added seventeen and repointed two,* on the frozen final source; every
affected killer was verified by `--verify-killer`, contained at P1, one at a time, with no host
build running: 20 of 20. They attack N24's invariants: the requested range ignored, a stale
version planned, a plan of an older generation, one with no version or with the wrong bytes
taken as current, the diagnostic's span edited instead of the fix's, a needs-review fix or one
of several alternatives preferred, a precondition hidden, an unversioned client sent a fix, and
requested kinds ignored — and `nazm fix`'s: a needs-review fix applied, an overlap applied, a
span inside a character edited, changed bytes overwritten, an unparsable result written, and a
failed write claimed. One killer was first verified NOT A KILLER: the plan with no version was
refused by the shared file check for an open document, and the guard matters only for a file
not open whose disk text matches; the test was extended to that case and every killer was
verified again on the final source. `n18-a-plan-ignores-its-text` was repointed at the line
that moved into `file_is_current`, `n16-code-actions-are-advertised` became
`n16-call-hierarchy-is-advertised`, and `n16-the-server-writes-to-stdout`, whose killer test
changed, was re-verified. **Campaign `abbba6ae23198600`, targeted, P1, one session, 1,010 s:**
those twenty, and nine N16 and N18 entries on what N24 shares — the versioned edit's version,
its closed-file and unversioned-client refusals, a rename plan outliving its generation, a stale
version answered, the disk winning over a buffer, a closed buffer staying authoritative, an
outgoing column counted in bytes, and an empty diagnostic set not published. **29 of 29 caught
at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry,
no runner error. The `fixes` suite joined the `cli`, `semantics` and `checker` profiles. N13's
use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as
they were.

*N25 (2026-09-26) added fourteen and repointed two,* on the frozen final source; every killer
was verified by `--verify-killer`, contained at P1, one at a time, with no host build running:
16 of 16, each on its first run. They attack N25's invariants: an item found by spelling, a
reference taken for a call, a recursive call dropped, a call given the wrong caller, a second
call lost, same-named callers merged, another root's compilation answering, an old item
answered after a change, a closed root's item rebound, an item's range its name, a call's range
the whole call, edges ordered ignoring the file, a stale version prepared, and every file's
ranges converted with one line index. `n22-a-selection-is-the-whole-declaration` moved with the
function symbol into the helper the outline and the hierarchy share;
`n16-code-actions-are-advertised`, then `n16-call-hierarchy-is-advertised`, became
`n16-formatting-is-advertised`, the same defect with a capability the server still lacks. **Campaign
`a52490106a612553`, targeted, P1, one session, 1,202 s:** those sixteen, and eight N16, N17,
N20 and N22 entries on what N25 consumes — both column conversions counted in bytes, same-named
functions indexed as one, a call with the wrong argument count not recorded, a write to stdout,
a stale version answered, the position just after a name, and the protocol's selection range.
**24 of 24 caught at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or
uninjected entry, no runner error. The `hierarchy` suite joined the `cli`, `semantics` and
`checker` profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were
not re-run and are as they were.

*N26 (2026-09-27) added eight and repointed two;* every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 10 of 10, each on
its first run — and, after the final source moved the parser's hole code out of line, the four in
the parser verified again: 4 of 4. They attack N26's invariants: a `.` before the cursor taken
for the hole, the hole forgiving every later statement, a bare `(` read as a construction, a
pattern's hole requiring an arm, an ambiguous import anchoring, the probe ignoring other
recoveries, a hole's completion replacing the `.`, and `(` advertised as a trigger. A ninth — a
generic record with no type arguments anchoring — was written, survived, and showed its guard
redundant with the checker's own application of type arguments; the guard and the entry were
removed rather than kept as an equivalent mutant. `n21-a-label-given-elsewhere-is-offered` and
`n21-a-recovery-is-ignored` moved with the code N21 and N26 now share. **Campaign
`c9b1509745e3e043`, targeted, P1, one session, 1,539 s, on the final source:** those ten, and
eighteen N15, N16, N17, N19 and N21 entries on what N26 consumes — a recovered body reaching the
checker, a malformed statement with no error node, a stale version answered, both column
conversions in bytes, a write to stdout, the formatting claim, same-named fields and variants as
one, a completion inserting rather than replacing, an unknown field or variant not recorded, a
member or variant from any definition, generic fields unsubstituted, another variant's label,
declaration order shown, and N21's two recovery rules. **28 of 28 caught at tier 1**; nothing at
tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry, no runner error. (The same
28 on the source before the parser change: campaign `40d19aba85e2276e`, 28 of 28 at tier 1.) The
`incomplete` suite joined the `cli`, `semantics`, `checker` and `syntax` profiles, and `holes` the
`syntax` profile. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not
re-run and are as they were.

*N27 (2026-09-27) added thirteen,* on the frozen final source; every killer was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: 13 of 13, the two in
`nazm-mcp` against the real server inside the container. They attack N27's invariants: a target
found by spelling, a local reported as a dependency, dependencies grouped by name, the declaration
listed as a reference, effects claimed, an absolute path serialised, a type given empty callers, the
source not the declaration, a file outside the compilation answered, a recovered compilation
answered, every diagnostic put in every packet, the server printing to stdout, and resources
advertised. **Campaign `fa7cfe95c8339d2e`, targeted, P1, one session, 1,700 s:** those thirteen, and
eleven N17, N18, N22 and N25 entries on what a packet consumes — same-named functions or fields
indexed as one, a cross-file use dropped, a declaration taken for a use, uses in map order, a written
type not recorded, the selection range, another root answering, the wrong caller, references taken
for calls, and edge order ignoring the file. **24 of 24 caught at tier 1**; nothing at tier 2 or 3,
no survivor, crash, timeout, unusable or uninjected entry, no runner error. The `context` suite
joined the `cli`, `semantics` and `checker` profiles, and `crates/nazm-mcp/src/` the `cli` profile
with its binary and `protocol` suite. N13's use-after-free survivor and the `nesting-unbounded`
MUTANT CRASH were not re-run and are as they were.

*N28 (2026-09-27) added twenty,* on the frozen final source, and moved five whose code N28 now
shares — two N25 entries onto the one checked-call rule `hierarchy::callee`, two N27 entries onto
`context::incomplete`, and N27's duplicate-payload entry onto the MCP server's one `once` path —
each re-pointed at the same defect. Every killer of the twenty, and of the 29 existing entries in
the files N28 changed or whose killing suite it changed, was verified by `--verify-killer`,
contained at P1, one at a time, with no host build running: **49 of 49**. The twenty attack N28's
invariants: definitions matched by spelling, the source digest ignored, a source-only change
counted unchanged, a rename inferred, caller and callee changes omitted, references ignored,
effects no longer declared unsupported, the prelude's definitions taken for the root's, the
directory scanned, an absolute path as root identity, the service generation digested, definitions
in declaration order, a recovered compilation snapshotted, a wrong-root, another-schema,
unknown-field or duplicate-identity baseline accepted, the MCP server keeping a snapshot, and a
snapshot sent twice. **Campaign `d7286651a2e99c5c`, targeted, P1, one session, 1,740 s:** those
twenty and the five re-pointed entries. **25 of 25 caught at tier 1**; nothing at tier 2 or 3, no
survivor, crash, timeout, unusable or uninjected entry, no runner error. The `snapshot` suite joined
the `cli`, `semantics` and `checker` profiles. N13's use-after-free survivor and the
`nesting-unbounded` MUTANT CRASH were not re-run and are as they were.

*N29 (2026-09-28) added sixteen,* on the frozen final source. Every killer of the sixteen, of
the nine N18 entries in `rename.rs` (which N29 touched to share its `Edit` and its entity lookup),
and of the five N27 and N28 entries in the MCP server N29 extended, was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: **30 of 30**. The
sixteen attack N29's invariants: the snapshot binding, a file's digest or each edit's expected
bytes of nothing, a whole file emitted as one edit, a duplicate, an overlap, or edits left in the
order they came, needs-review planned as automatic, the precondition dropped, a fix taken from
another position or an index that wraps, an unloaded file planned against the root's text, a
local given its function's key, the MCP server applying the patch, sending it twice, or keeping
the first plan. **Campaign `e0bc0b9125753c45`, targeted, P1, one session, 1,830 s:** those sixteen,
six N18 entries N29 consumes (a rename by spelling, an exported entity renamed, the candidate not
checked, its meaning not compared, any name accepted, a program with errors renamed), two N24
entries (the requested range ignored, the diagnostic's span edited) and three N28 entries on the
snapshot a patch binds to (an absolute path, the service generation, a recovered compilation).
**27 of 27 caught at tier 1**; nothing at tier 2 or 3, no survivor, crash, timeout, unusable or
uninjected entry, no runner error. The `patch` suite joined the `cli`, `semantics` and `checker`
profiles. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and
are as they were.

*N30 (2026-09-28) added eighteen,* on the frozen final source. Every killer of the eighteen, and
of the eight N27, N28 and N29 entries in the MCP server N30 extended, was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: **26 of 26**. The
eighteen attack N30's invariants: the index carrying the message, an id hashing it, an id of
only its code or only an ordinal, a stale state answered, a state of the root file only or over
absolute paths, concatenation offsets given as file offsets, an owner taken from a recovery or
from the nearest declaration, syntax errors given no index, compiler fixes counted as patches, a
syntax fix offered as a patch, needs-review counted automatic, a detail dropping the help, the
directory scanned, and the MCP server sending an index twice or keeping the first. **Campaign
`b488c311e0ac14ee`, targeted, P1, one session, 1,794 s:** those eighteen, two N24 entries (the
requested range ignored, the diagnostic's span edited), three N29 diagnostic-fix entries (a fix
taken from anywhere, an index that wraps, needs-review planned automatic) and two N28 entries (a
recovered compilation snapshotted, an absolute path). **25 of 25 caught at tier 1**; nothing at
tier 2 or 3, no survivor, crash, timeout, unusable or uninjected entry, no runner error. The
`diagnostics` suite joined the `cli`, `semantics` and `checker` profiles. N13's use-after-free
survivor and the `nesting-unbounded` MUTANT CRASH were not re-run and are as they were.

*N31 (2026-09-28) added fifteen,* on the frozen final source. Every killer of the fifteen, of the
sixteen N30 entries in `crates/nazm-service/src/diagnostics.rs` (whose canonical order N31 now
shares), and of the ten N27–N30 entries in the MCP server N31 extended, was verified by
`--verify-killer`, contained at P1, one at a time, with no host build running: **41 of 41**. The
fifteen attack N31's invariants: every case counted passed, files without an expectation dropped,
passed cases listed, only the first failure listed, a build failure called a mismatch, a leg not
run called matched, no diagnostics taken for success, a command-only diagnostic dropped or given an
id, references in the compiler's order, references linked to changed sources, the message carried,
a summary exiting zero, and the MCP server sending a summary twice or keeping the first. **Campaign
`ef13a76e70082ddb`, targeted, P1, two resumed sessions:** those fifteen, four N30 entries on the ids
and places N31 reuses, the two existing entries in the check and build drivers N31 changed (neither
with a declared killer), and two MCP safety entries. **23 of 23 caught — 21 at tier 1, 1 at tier 2
(`the-object-key-approximates-the-whole-program`, by `nazm-cli --test schemas`), 1 at tier 3
(`the-prelude-is-left-out-of-every-check-key`, by the workspace suite)**; no survivor, crash, timeout,
unusable or uninjected entry, no runner error. The first session reached its deadline after the
tier-3 entry and the campaign was resumed from its journal, which reran nothing already decided.
The `summary` suites joined the profiles. N13's use-after-free survivor and the `nesting-unbounded`
MUTANT CRASH were not re-run and are as they were.

*N31's closure correction (2026-09-28) added seven,* one for each outcome it newly exercises: a build
that could not be started called `build_failed`, a program that could not be started called
`mismatched`, the test driver seam ignored, a test run with nowhere to build called `failed`, and a
failed link, a refused compile and an unwritable output summarized as a success or a rejection.
On the final source every killer of the twenty-two N31 entries was verified by `--verify-killer`,
contained at P1, one at a time, with no host build running: **24 of 24** (two entries have two
killers each). **Campaign `d56b0589bbcf96b2`, targeted, P1, one session, 984 s:** the seven, the six
earlier entries in the test runner the seam touched, and no-diagnostics-is-success. **14 of 14
caught at tier 1**; no survivor, crash, timeout, unusable or uninjected entry, no runner error.

*N32 (2026-09-29) added seventeen, and N32-H two for the harness,* on the frozen final source;
one N27 entry was re-pointed at the line its rule moved to. The seventeen attack N32's laws: an
anchor ignored, a line number as an id, a duplicate id accepted, an unknown id answered with the
nearest, fences never opened, a parent carrying its children, the goals classified as evidence
and the roadmap as normative, a production's text normalised, a broken link ignored, a corpus
state without the text or with the checkout's absolute path, a section digest without the body,
a stale state answered, the MCP section tool taking any argument, sending a section twice, or
keeping the first. The two guard the warm session: a killer that passed on its mutant counted
verified, and a live supervisor's container reaped. **One warm session, campaign
`81c571e1e7d95767`, targeted, P1, 1,469 s:** those nineteen and the twelve MCP entries of
N27–N31 in the server N32 extended — **31 of 31 caught at tier 1**; no survivor, crash, timeout,
unusable or uninjected entry, no runner error — and, around each mutant's one injection, **every
declared killer verified, 34 of 34**: pristine passes, mutant fails, restored passes. The
verifications an earlier harness had finished for N32 before it was replaced are not counted.
A `docs` profile and a `harness` profile joined the nine, and the `documentation` suite the `cli`
profile. N13's use-after-free survivor and the `nesting-unbounded` MUTANT CRASH were not re-run
and are as they were.

*N32-H2 (2026-09-29) added seven harness mutations,* on the frozen final source: a pristine
killer result found for another tree, another killer's run read as this one's, Tier 1 alone
accepting an inexact killer, the cache key without the lockfile, a harness change or an unknown
path given the narrow lifecycle, and a mutation session sharing the checkout's target. **One warm
session, campaign `7ee57d2af9d3bb87`, targeted, P1, 767 s:** those seven, N32's seventeen, N32-H's
two and the twelve MCP entries — **38 of 38 caught at tier 1**, no survivor, crash, timeout,
unusable or uninjected entry, no runner error — and **every declared killer verified, 41 of 41**,
each killer's mutant observation the run its verdict came from. The workspace suite was not run as
the session's baseline: no mutant needed a tier beyond its killers.

*N32-H3 (2026-09-29) added seven harness mutations,* on the frozen final source: a failing binary
cancelling the rest, results in completion order, the shortest binary first, execution ignoring
the owning package, an unknown path bringing no benchmark, the whole lifecycle suite not
covering its tests, and a package id losing its name. **One warm session, campaign
`f4f5c0b996691ef4`, targeted, P1, 833 s:** those seven, N32's seventeen, the twelve MCP entries and
the nine earlier harness entries — **45 of 45 caught at tier 1**, no survivor, crash, timeout,
unusable or uninjected entry, no runner error — and **every declared killer verified, 48 of 48**.

The run also found three harness defects, all repaired before the counted run:
`memory.rs` ran compiled programs with no deadline (the loop mutant waited 1500 s for a
verdict it could not get); N12.1's process-group kills used `kill -KILL -<pgid>`, which
procps rejects, so inside the container they signalled nothing; and a session killed at its
deadline was reported as a memory limit.

*N33 (2026-09-29) added nineteen mutations and a twelfth profile, `repo`,* on the frozen final
source: the seed left out, a direct dependency dropped, goals outranking the specification, a
budget cut reported complete, a lower-priority item jumping the cut, the seeds cut by the budget, a
stale state answered, the state skipping the text, a path taken as an id, the map not canonical, a
broken relationship published, a killer of another definition related, a fan-out cut silent, a
linking section sent whole, callers from one compilation, a compilation outside its root
answered, a Cargo target found twice, a definition asked for at its declaration, and the MCP task
taking any argument. **One warm session, campaign `82a3ad10e55b2183`, targeted, P1, 1,057 s wall
from empty caches:** those nineteen and the fifteen MCP entries N33's server change touches —
**34 of 34 caught at tier 1**, no survivor, crash, timeout, unusable or uninjected entry — and
**every declared killer verified, 36 of 36**.

*N34 (2026-09-29) added eleven mutations and a thirteenth profile, `tokens`,* on the frozen final
source: one tokenizer counting for all, an unknown tokenizer ignored, bytes reported as tokens,
framing counted as content, an altered asset loaded, the vocabulary digest ignoring the
vocabulary, text normalised before counting, unknown pieces not reported, the stress case dropped
from the report, a reduction's sign inverted, and a tokenizer's worst result reported as its best.
**One warm session, campaign `17c8f2bebd2c1dd4`, targeted, P1, offline, 818 s wall from empty
caches: 11 of 11 caught at tier 1** — no survivor, crash, timeout, unusable or uninjected entry —
and **every declared killer verified, 12 of 12**. Three of the eleven are in the benchmark's own
code (`tests/benchmark.rs`), where the numbers it reports are computed.

*N35 (2026-09-29) added sixteen mutations and a fourteenth profile, `agent`,* on the frozen final
source: both arms carrying one context, every answer scored by another task's oracle, a name the
truths lack taken as a truth, a hallucination counted as a misreading, output recorded as input,
cache reads priced as uncached input, output left out of a total, a zero denominator divided by,
an identity without its prompt, a runner error counted as an answer, the stress case left out of
the plan, one arm given another system text, a third retry, the cap checked without the attempt
about to be made, a credential kept in error text, and the Claude Code client run with its tools.
**One warm session, campaign `e8c0c93340e2158c`, targeted, P1, offline, 1,504 s wall: 16 of 16
caught at tier 1** — no survivor, crash, timeout, unusable or uninjected entry — and **every
declared killer verified, 16 of 16**. A first campaign on the fifteen before the Claude Code
provider existed, `7b5dc0b2ba230b56`, caught all fifteen too.

*N36 (2026-09-30) added seventeen mutations and retired two,* on the frozen final source: a union
dropping an effect, a set iterated against id order, `print` classified as pure, a free `spawn`,
a callee's effects kept from its caller, one round taken as the fixed point, a contract not held,
an unknown effect read as `io`, a repeated effect accepted, an undeclared import taken as pure, the
witness pointing at the function rather than the call, the interface omitting and the reader
dropping a declared set, an effect change invisible to a snapshot, a packet without effects, the
interface schema staying `/5` and the epoch staying 7. N14's two pins on `/5` and epoch 7 are
retired in favour of the last two; N27's and N28's entries on effects being unsupported now guard
capabilities, the section that still is. **Campaign `39b5858f7f2e7ab7`, targeted, P1, offline:
all 85 selected caught** — the seventeen, the two repointed, the sixteen MCP-shared entries and
every existing entry in the interface writer, the packet, the snapshot, the delta and the
signature renderer — **78 at tier 1 with every killer verified in the session that caught it, and
7 of the interface writer's killerless entries at tier 2**; no survivor, crash, timeout, unusable
or uninjected entry. It took two sessions (5,862 s of the gate, three further resumes finding
nothing left), and the harness's one-session coverage check reads *incomplete* because the 78
verifications are split across them, not because any is missing.

*N37 (2026-09-30) added twenty mutations and retired one,* and was the first milestone selected
by the **responsibility law**: new mutations, and existing ones only where N37 changed the code
they guard or the tests that kill them, never because a layer was touched. The twenty attack the
authority laws — the outside-world built-ins or `print` needing nothing, a `SpawnCap` authorising
`io`, a free `spawn`, an undeclared callee or import bringing its own authority, a task inheriting
its spawner's, an effect set granting authority, a `! {}` contract skipping the check, holding
treated as an effect, the witness stopping at the callee, authority by spelling, a shadowed
capability held, a capability built, a `main` taking anything, a capability with equality, the
interface dropping authority or forgetting its kind, `main` handed no roots, and the epoch staying
8. `n36-the-epoch-stays` is retired for the last. Thirteen N36 entries were selected with a link
each: ten whose code (`effects.rs`, where the authority check now sits) or whose killer tests N37
edited to hand functions their capabilities, and three on the interface's effect field and schema
version, which N37 reads as the authority mode and kept at `/6`. N13's equality leaves, N17's
reference index and the lowering entries in the files N37 touched were considered and left out: N37
changes nothing they guard. **Campaign `60c5ee822617e3c9`, targeted, P1, offline: 33 of 33 caught,
32 at tier 1 with killers verified, 1 at tier 2**, no survivor; two sessions (4,395 s of the gate).
The tier-2 one, `n37-holding-authority-is-an-effect`, exposed a declared killer whose pure function
made no call; the test was strengthened after the gate and the mutant verified alone at tier 1
(164 s). The one-session coverage check reads *incomplete* for the split, as N36's did.

*N38 (2026-09-30) added twenty-five mutations and retired one,* selected by the responsibility
law. The twenty-five attack the provenance laws — a file read or the command line misclassified,
output taken for input, shared storage followed or a channel dropping an origin, file contents or
`unknown` let through to `write_file`'s path, a join order-dependent or dropping a side, a binding
keeping only its first value, a condition flowing, a constant inheriting its input, the worklist
stopping after one pass, `main`'s authority misclassified, a `spawn` resetting provenance, a helper —
declared or not — laundering a path, the witness stopping at the callee, a reused module or its entry
losing its facts, the entry, snapshot and epoch pins, and a snapshot or packet without provenance.
`n37-the-epoch-stays` is retired for `n38-the-epoch-stays`. Nine existing entries were selected, each
guarding a contract N38 edited directly: the entry validation and the planner's hit path, which now
carry facts (`a-cache-entry-is-trusted-because-it-parsed`, `the-prelude-is-left-out-of-every-check-key`);
the delta's baseline validation and its sections, which gained `provenance`
(`n28-another-schema-is-accepted-as-a-baseline`, `n28-an-unknown-baseline-field-is-accepted`,
`n28-caller-changes-are-omitted`, `n28-callee-changes-are-omitted`, `n28-a-source-only-change-is-unchanged`);
and the snapshot digest and packet structures N38 extended (`n36-an-effect-change-is-invisible-to-a-snapshot`,
`n36-a-packet-has-no-effects`). Every N38 killer was first verified by hand on the host, which found four
weak ones before the gate, each strengthened. **Campaign, targeted, P1, offline: 34 of 34 caught — 32 at
tier 1 with killers verified, and the two selected killerless entries at tier 2 and tier 3** (the store
suite, and the workspace suite for the prelude key entry); no survivor, over two sessions and three
resumes that found nothing left (5,848 s of the gate).

*N39 (2026-09-30) added nineteen mutations, repointed twelve and retired one.* The nineteen attack
the Core IR laws: a call resolved by its spelling, a read with no type, a value `if`'s branches
swapped, a loop's back edge, a `return` falling through, a `break` in a condition naming its own
loop, a `continue` naming the outermost, an initialiser filling the field at its position, a variant
losing its identity, the effect contract and a function's authority dropped, a digest naming a callee
by session number, ignoring the body, collapsing effects into the body digest or keeping arm order,
the verifier accepting a stray `break`, a generic call keeping its declared result, the interpreter
running a `match`'s first arm, and the backend asking the resolution again — which only the boundary
gate the suite now runs can see. Twelve historical entries were **repointed**, because the rule each
pins moved: into the one lowering (the three `lowered-propagation-*`, now reaching both backends),
into the Core IR evaluator (`call-depth-never-released`, `return-does-not-leave-its-block`,
`remainder-inherits-the-hardware-fault`, `n13-interpreted-inequality-is-not-the-negation`,
`a-returned-value-is-discarded`, `a-flow-in-value-position-is-an-error`) and into native lowering
from Core IR (`return-does-not-end-the-block-it-is-in`, `lowered-arm-block-drops-its-statements`,
`a-statement-block-is-skipped`); the last two of the evaluator's were found *unusable* by the
campaign — their text still matched and their replacements named the tree walker's variables — and
were repointed after it. `interpreted-propagation-carries-on-with-the-error` is **retired**: the
interpreter has no `?` of its own. Six more were selected for a rule N39 rewrote the code of: the
call-depth guard and the stack's sizing (`call-depth-unbounded`, `stack-not-sized-from-the-budget`),
and native lowering's construction cleanup, field positions and statement-`match` completion
(`a-partly-built-record-is-not-cleaned`, `a-partly-built-variant-is-not-cleaned`,
`a-field-is-read-from-the-wrong-position`, `a-statement-match-claims-every-arm-leaves`). Layout,
symbol, linkage and cycle-check entries were not selected: their code did not change and every
emitted IR file is byte-identical. Every N39 killer was verified by hand on the host first, which
found the boundary gate blind to a lookup written across lines; it reads code whitespace-free now.
**Campaign, targeted, P1, offline: 37 of 37 caught — 23 at tier 1 with killers verified, 13 at tier
2, 1 at tier 3 — and no survivor.** `stack-not-sized-from-the-budget`, a MUTANT CRASH in every
campaign that had run it, is caught for the first time: the in-process test that aborted its binary
(`a_raised_budget_still_runs_what_fits_inside_it`) recursed deeper than its own comment's bound, and
recurses 200 levels now, leaving the out-of-process check to fail. It took six sessions and
13,876 s; the first session's journal was not read by the next, whose campaign key differed, so its
23 verdicts were established twice. 34 verdicts are at `ab0a761`; the three repaired entries' are at
`eb80464`.

**Limitation** · **VERIFIED here means the harness runs every catalogued mutant and accounts for
every verdict — not that every meaningful mutation is killed.** A killer verified on one platform
is evidence on that platform (N108); eight entries are observable only with macOS tools or Apple's
clang and are verified on the host. A whole-catalogue campaign takes several contained sessions,
resumed from its journal; a supervisor killed mid-session loses that session's verdicts, which then
simply run again. Fuzzing covers five targets — the front end, lowering with MIR's validator, the
interpreter, package manifests and LIR — at 300 s each per campaign, with no sanitizer, Miri or Loom
run and no backend or FFI target (`limitations.md`). *Until R1 this named a crashing entry and a
known survivor (neither remains: N108 caught all 1,268 entries) and said fuzzing did not exist (N73 began it).*

**Next dependency** · For fuzzing, a sanitizer run and backend and FFI targets. For mutation, none
for the catalogue as it stands.

**Accepted when** · A full catalogue run completes with every verdict accounted for. **Met on
2026-09-25**: 233 of 233, as above. Fuzzing's status is separate and unchanged.

### 31. Bootstrap — VERIFIED as scoped

**Scope (v1, Q1-D-20)** · The bootstrap is of the declared subset the compiler written in Nazm
carries; it does not carry Gate 2 or Gate 3 (`selfhost.rs`, `NOT_CARRIED`; `limitations.md`,
*Self-hosted compiler*), so a program using them is the reference compiler's alone.

*N102, 2026-10-05: parity on every probe — 21 of 21.* `architecture.md` §7.103 first. The eight
refusals N98 measured are gone: effects and capabilities, recursion (the reference's stack check,
derived and gated by `check-runtime`), contracts, foreign declarations (`Int` only), `@std`, traits,
closures and function values, and with them tasks and channels; `&&`, `||` and `!` too.
`compiler/parity.json`: **21 equal, 0 refused, none differs**. The checker's answers are compared
with the reference's — codes and spans — on 39 sources of the N102 features and four new refusals
(`N0602`, `N0386`, `N0366`, `N0614`); `compiler/conformance/features.nz` and a two-module case run
through C2, C3 and the reference with the same memory counters, closures included. Contained:
selfhost 43 of 43, bootstrap C2 = C3 (`8cc750c6…`, 16,030 lines, 34 cases of which 14
multi-module, 29 refusals). **Mutation**, contained: all 100 catalogue entries on `compiler/`,
nine of them new, applied one at a time — 100 caught; one only after the memory test gained a case
(a capture not retained into a closure, invisible until a later string reuses the freed storage),
and one, `selfhost-a-success-value-is-not-retained`, crashing the whole suite and caught cleanly by
its declared killer. Parity on the probes is not parity on the language: what remains is in
`limitations.md` (packages, generic channels, `select`, the clock, devices, `c_errno`, provenance,
profiles, escapes, `::`, `as`). A fixpoint is self-reproduction, not correctness.

*N98, 2026-10-04: parity of the compiler written in Nazm — measured and gated; not reached.*
`architecture.md` §7.99 first. `compiler/parity/` holds 21 probes, one per feature; the contained
test `the_parity_record_is_what_both_compilers_do_with_each_feature` (`crates/nazm-cli/tests/selfhost.rs`)
builds each with the reference and with `compiler/emit.nz`, runs both natively, and holds
`compiler/parity.json` to the verdicts: **13 equal** — arithmetic, loops, both failure traps, modules
of files, records, enums, generics, `Result` with `?`, sequences, `Vec`, strings, equality — **8
refused** — closures, traits, effects with `IoCap`, tasks and channels, contracts, `@std`, foreign
declarations, and a recursive function with `if` as its value (`N0101`) — and **none differs**. The
bootstrap stays VERIFIED; parity with the reference's native subset is not.


**Evidence** · `compiler/bootstrap.sh` runs C1 → C2 → C3 from one source and compares two
things: the emitted IR byte for byte, and the linker-normalised executables byte for byte.
Five conformance cases under `compiler/conformance/` must agree across C2, C3 and the
interpreter. The script refuses to run without an external memory ceiling. `docs/bootstrap.md`
states what this does and does not establish, and the answer is narrower than it sounds.

**Limitation** · A fixpoint establishes self-consistency, not correctness — a compiler with
a bug that reproduces itself reaches a fixpoint too.

**The reference compiler was stale until 2026-09-22.** `compiler/bootstrap.sh` requires
`$nazm` to exist and does not build it, and inside the contained runner `CARGO_TARGET_DIR`
is `/work/target` — which `docker/contained.Dockerfile` warms with a binary at *image build
time*. Every contained bootstrap between the image's build and this date therefore used
that binary for the reference check and for stage 1, whatever the tree said, and reported
nothing about the discrepancy. The workload now runs `cargo build -p nazm-cli` first. The
fixpoint claim itself is unaffected — C1 → C2 → C3 is about the compiler written in Nazm,
and the reference check is explicitly not part of the chain — but *which front end built
C1* was not what the record said it was.

**Since N2** the compiler source states its own module boundaries: 84 of its 224 functions
are `pub`, 140 are private, and `emit.nz`, `check.nz` and `parser.nz` gained the `use` lines
for modules they were reaching through someone else's import.

**Since N2.1 the two implementations accept the same language.** For one day they did not:
the Nazm-written compiler parsed `pub` and resolved every name across the whole program, so
it accepted eight kinds of program the reference refuses and emitted an invalid module for a
ninth. It now carries a module graph, a per-definition owner and visibility, per-module
interfaces collected before any body, and a recorded resolution that emission consumes.
Fifteen rules are compared through both compilers
(`the_two_compilers_agree_on_every_module_and_visibility_rule`), and five multi-module
conformance cases agree across the reference, C2 and C3 — which is a **different** claim
from C2 = C3 and is recorded separately in `bootstrap.md`.

**Since N11** the compiler written in Nazm implements generics and `Vec[T]` and uses one — its
diagnostics are a `Vec[Diag]` — so every stage compiles a `Vec` of records. The corpus has 24
cases, 9 of them multi-module, including a generic library instantiated only by its importer
and one instance requested from two modules, and a **refusal corpus** of 4 programs that C2,
C3 and the reference must each refuse with the named code. The fixpoint moved to
`c58b91dd…` at 11,877 source lines. `bootstrap.md` has the record.

**Since N12** every stage loads the same core prelude — named on its command line, its digest
in the record — and the compiler uses `?` itself: its front end returns
`Result[Checked, Vec[Diag]]`. The corpus has 26 cases, 10 of them multi-module, and 9
refusals. The fixpoint moved to `08895900…` at 12,046 source lines, with the runnable
executables byte-identical as well.

**Since N12.1** the compiler written in Nazm compiles a block used as a value and finds the
prelude by the key `@core/prelude` rather than as the last module, and its checker takes an
`Option[Int]` apart where it used to test for `-1`. The corpus has 28 cases, 11 of them
multi-module, and 12 refusals. The fixpoint moved to `a9ead258…` at 12,164 source lines,
runnable executables byte-identical too.

**Since N13** the compiler written in Nazm derives equality and emits it — a helper per
compared record and enum, a tag comparison and then the active slot alone — and uses it on
itself: a node's completion is an `enum Completion` in a `Vec[Completion]` rather than `0`,
`1` or `2` in an `Ints`, compared with `==`. The corpus has 30 cases, 12 of them
multi-module (one compares an imported record, enum and `Option[Point]`), and 19 refusals,
seven of them equality's. The fixpoint moved to `76598c43…` at 12,412 source lines,
runnable executables byte-identical too.

**Since N14** the compiler written in Nazm parses `T: Equality`, records it on the parameter's
entry, reads it in the one place equality is derived, and checks every call's type arguments
against it at the reference's spans. The corpus has 32 cases, 13 of them multi-module (one
calls and forwards to an imported bounded function, including a `count_same[T: Equality]`
over a `Vec[T]`), and 25 refusals, six of them requirements'. No compiler code uses a
requirement: the audit found none that needs one. The fixpoint moved to `0e1a40e6…` at 12,543
source lines, runnable executables byte-identical too.

**Next dependency** · None.

**Accepted when** · Met.

### 32. Reproducibility and provenance — PARTIAL

*N94, 2026-10-04: reproducibility and provenance v2 — VERIFIED under the model of §7.95, on one host;
the area stays PARTIAL.* `architecture.md` §7.95 first. The model's claims, apart: identical input
and another directory give the same executable, provenance and SBOM bytes under either backend
(`identical_input_gives_identical_artefacts_here_and_in_another_directory`); the release archive (N75)
and offline locked builds (N56) as before; across toolchains, differences classified, never equal
(N73). `nazm build --sbom FILE` writes CycloneDX 1.5 read off the provenance record — the output,
every package with `pkg:generic/NAME@VERSION`, every source module, the runtime, BLAKE3 hashes, the
tools (`the_sbom_lists_every_package_source_and_the_runtime_with_their_digests`). `nazm attest sign`
wraps the record in an in-toto Statement v1 (`urn:nazm:provenance:1`) over artefacts it names, and
signs it with `ssh-keygen -Y sign` in the namespace `nazm-attestation`; `nazm attest verify` checks the
signature for an identity in an allowed-signers file and every artefact's digest among the subjects
(`crates/nazm-cli/src/attest.rs`; `a_signed_statement_verifies_for_its_signer_and_its_artefacts_only`:
a stray artefact refused before a key is used, another identity, an edited artefact, an edited
statement and an unlisted key each refused). Four mutants, one repointed. Not here: a transparency
log, keyless signing, SLSA levels, rebuilds on independent machines, dependency origins beyond N56's
digests.

**Evidence** · `scripts/release-candidate.sh` runs every gate, writes `PROVENANCE.txt` and
`MANIFEST.txt`, and validates the package against its own manifest with counts compared
both ways so a file copied in but not listed fails.

*N46, 2026-10-01: reproducible executables — VERIFIED on one host.* Two copies of one package,
built from empty caches in two directories, give byte-identical executables under both backends
(`two_copies_built_from_empty_caches_are_byte_identical`): the link runs with `ZERO_AR_DATE=1` and
under the final file name, because the Apple linker otherwise mixes a time and a temporary name
into the executable's UUID and signature. The package lockfile records BLAKE3 digests of every
package's sources and refuses a changed one. Not claimed across hosts, operating systems or
toolchain versions.

**Limitation** · Three claims, kept apart. Archive **entry metadata is normalised** — tar
sorted with zeroed owner and mtime, gzip with no timestamp — and since R1 the dates the provenance
and bootstrap records carry are the commit's (`SOURCE_DATE_EPOCH`), not the run's. Whether two
assemblies of one commit are byte-identical is recorded in that candidate's release record
(`releases/`), on one host and one image — never across hosts, operating systems or toolchains.
**C2 and C3 are byte-identical within one run** after the build-id is stripped. The source digest
covers a **declared file set**; since R1 a `build-input-digest` beside it covers every file git
tracks or would (`scripts/source-digest.sh`) — Cargo manifests, `Cargo.lock`,
`rust-toolchain.toml`, the Dockerfile, the catalogue and every fixture included. *Until R1 the
records embedded their own run date, which made bit-reproducibility impossible, and only the
declared set was digested.*

*R1, 2026-10-07: two assemblies, one archive — on one host and one image.* `cargo xtask contained
--profile p4t4 release`, run twice at `996984f` in `nazm-contained:1.98.1` on one macOS host, gave
byte-identical archives (`a5e2ee97…`) with identical `MANIFEST.txt` and `PROVENANCE.txt`, every date
inside the commit's; the manifest was redigested independently, 220 of 220
(`releases/996984f.md`). The archive was signed by nobody, and nothing is claimed across hosts.

**Next dependency** · A comparison across two independent hosts, which needs a second machine.

**Accepted when** · Two runs at one commit produce identical archives, or the claim is
permanently retired. *Met on one host and one image, R1 (2026-10-07); the row stays PARTIAL for what
N94 lists as not here — a transparency log, keyless signing, SLSA levels and rebuilds on independent
machines.*

### 33. Platform and target matrix — PARTIAL

**Ignored tests, run and kept (v1, 2026-10-11, Q1-D-04)** · Every cell and sub-claim here and in
areas 18, 19, 21, 27 and 34 that rests on an `#[ignore]`d test — one that starts a container,
QEMU, a WebAssembly engine, Wine, py-evm, gdb or the GPU — was run at `6b3b85c`, one at a time:
**38 run, 38 passed**, `target/gates/v1/ignored.log` (08:47–08:58 UTC). They are: both boards and
the Cortex-M board booting, their interrupts, sections, manifests, the heap arena and the kernel
workload; the six domain workloads and the traffic light; the realtime scheduler and stack bound;
the WebAssembly corpus, its absent authority and its stack check, and WASI's two; the EVM and
WebAssembly contracts against the simulator, generated sequences included (N96); the GPU's four
and the CPU provider's; the Bionic and Cocoa runs; Windows under Wine, both; and gdb live under both
backends. The cost measurements (`what_…_cost…`) were not re-run: they record numbers, not claims.
A VERIFIED sub-claim resting on an ignored test cites this log, and the release gate re-runs it.

**Gate 3 (2026-10-10)** · *Prebuilt toolchains* (`systems-domains.md` Part A; `support.md`). Host
archives by `scripts/host-archive.sh` — the release binary with the standard library and runtime
inside it — and `scripts/clean-install-test.sh`, which installs one where no Rust is reachable and
checks, runs, builds and runs a program: passed for `aarch64-apple-darwin` (native),
`x86_64-apple-darwin` (cross-built, **under Rosetta**) and `aarch64-unknown-linux-gnu` (built by the
new `cargo xtask contained archive`, installed in a clean Debian container with only clang,
`docker/clean-install.Dockerfile`). The contained runner gained `--platform amd64` for the
`linux/amd64` image. Found on the way: `nazm build` blamed the generated IR for a toolchain failure
in the link step; it now says the toolchain or its environment failed
(`a_toolchain_environment_failure_is_not_blamed_on_the_ir`).

**Gate 3C (2026-10-10)** · *Windows x86_64* (`spec.md` *Windows x86_64*; `systems-domains.md` Part
A-W). `x86_64-pc-windows-gnu`, built to objects and `link.txt` for MinGW-w64's `gcc` (`-static`,
`-lws2_32`, `-lbcrypt`), by both backends. The runtime reaches the system only through its platform
layer — the `os`, `stack` and (Windows) `init` units, runtime ABI 21 — so the Windows port is a
third binding of each function, never a branch in the runtime: the Win64 context switch (the TIB's
stack bounds, `xmm6`–`xmm15`), task stacks by `VirtualAlloc` with a `VirtualProtect` guard, UTF-16
files and directories, Winsock with a `WSAPoll` reactor of epoll's one-shot meaning and pipes
checked by `PeekNamedPipe`, children by `CreateProcessW` with quoted command lines, the clocks by
`QueryPerformanceCounter` and `GetSystemTimePreciseAsFileTime`, entropy by `BCryptGenRandom`, the
arguments as UTF-8 through `__wgetmainargs`; Windows' errors are translated to Linux's numbers at
the boundary, so the library's one mapping serves every target. A DLL is refused by name.
**Run-verified under Wine** (Wine 8 in a Debian x86_64 container, `docker/wine.Dockerfile`; never a
Windows host): `windows.rs`'s ignored tests — the 20 conformance programs, and files, TCP and UDP,
the reactor waking a closed socket's waiter, a peer gone as an error, processes and pipes, the
clocks, a waiting task releasing the only worker (socket, sleep and pipe), UTF-8 arguments, a trap
(`N0400`) and an exhausted stack on a thread and in a task (`N0408`), each agreeing with the native
run, by both backends. Two differences, stated in the spec: `os_family()` is 2, and `os_kill` ends a
child with exit code 1. Found on the way: an IR string carried a literal NUL byte (now `\00`).
**Not done**: execution on a real Windows host (a final-v1 qualification item), a DLL, the MSVC
environment, Windows ARM64, a prebuilt toolchain for a Windows host.

**Evidence** · *At N95:* two verified targets: `aarch64-apple-darwin` and `aarch64-unknown-linux-gnu`
(the latter inside `docker/contained.Dockerfile`). The emitter states its own scope, and
`crates/nazm-cli/tests/docs.rs` asserts the documented target and the emitter agree.

*N57, 2026-10-02: cross-compilation — VERIFIED for four targets, with run evidence for three.*
`architecture.md` §7.59 first. `--target` builds any of the four triples with both backends from
this host: clang is told the triple, Cranelift is built with its `x86` and `arm64` backends, and
the runtime text is one for all four under a stated portability rule. An executable where the
host's toolchain links the target (the host and the other macOS architecture); otherwise
`--objects DIR` writes every object and `link.txt`, and an executable is refused by name. The
triple is in every object key under both backends; `nazm inspect` lists each target and what this
host makes of it. Evidence, `crates/nazm-cli/tests/cross.rs`: all four triples by both backends,
each object's format and architecture read from its header; the refusal; two targets never one
cache entry; triples outside the matrix refused. **Run-verified**: `aarch64-apple-darwin` (host),
`x86_64-apple-darwin` under Rosetta (both backends, `a_program_for_the_other_macos_architecture_runs_where_rosetta_does`),
`aarch64-unknown-linux-gnu` from macOS-built objects linked and run in the Linux container
(both backends, by hand: `performance.md`, N57). **Compile-only**: `x86_64-unknown-linux-gnu`.

*N62, 2026-10-02: a freestanding target — VERIFIED on QEMU, one board.* `architecture.md` §7.64
first. `aarch64-unknown-none`: objects, `link.txt` and `link.ld` (image at `0x40080000`, `_start`
first, a 64 KiB stack); a board runtime with no thread-local storage and nothing of a C library —
failure state in plain globals, reported on the PL011 UART, the machine stopped by semihosting —
and a stack check against the linker's `__stack_bottom`. `mmio_read32`/`mmio_write32` are volatile,
need an `MmioCap` held, and are refused by `nazm run` and hosted builds (`N0392`). What a board
program reaches beyond that is read from the generated units' unresolved symbols and refused by
part. Evidence, `crates/nazm-cli/tests/freestanding.rs` (10, plus one ignored): the objects' ELF
headers, the linker script, no `STT_TLS` symbol in a board unit or runtime against a hosted one that
has them, each refusal; **run-verified** by the ignored test in `nazm-qemu:n62` — `ld.lld` links
the image and `qemu-system-aarch64 -M virt -cpu cortex-a53 -semihosting` boots it: `Hi` written by
`mmio_write32`, then `100`, status 0; an overflow's `N0400` and a deep recursion's `N0408` on the
UART, status 2. **Limitation**: one board; no atomics, interrupts, heap or `@std` (§7.64,
DESIGNED); semihosting stops the machine only under a debugger or QEMU; Cranelift builds no board.

*N64, 2026-10-02: WebAssembly — VERIFIED for the freestanding subset, one engine.*
`architecture.md` §7.66 first. `wasm32-unknown-unknown` from the LLVM backend with the board's
freestanding law; a runtime that reaches its host only through `nazm_host.write` and
`nazm_host.report`, a `__multi3` of its own for checked multiplication; `host.mjs`, a reference host
granting exactly those. Evidence, `crates/nazm-cli/tests/wasm.rs` (5, plus two ignored run in
`nazm-wasm:n64` — `wasm-ld` 19, Node 20): the objects, link line and host; a bound rooted at
`nazm_main`; the same bytes twice; DWARF in the objects under `--debug`; each refusal. **Run**: the
linked module's import section read from its bytes — `write` and `report` with `print`, `report`
alone without an `IoCap`; 64 corpus programs (`transfers`, `compiler/conformance`,
`bench/programs`, `examples`): 14 run with the interpreter's exact standard output and status,
50 refused by part, none disagree. **Limitation**: no heap, so most programs are refused; no WASI;
exports beyond `nazm_main`; no other engine run; a debugger's source mapping not exercised, only
the sections' presence.

*N95, 2026-10-04: the support matrix — VERIFIED as stated; the area stays PARTIAL.*
`architecture.md` §7.96 first. One table, `nazm_lir::backend::SUPPORT`, printed by `nazm inspect`
(`toolchain.support`, `toolchain.non_goals`), and this one held to it cell for cell by
`crates/nazm-cli/tests/platform_matrix.rs`, which also requires every target the compiler accepts to
have a row, every run-verified or compile-only cell to name evidence the tree holds, and each LLVM CPU
to be the one the C compiler driver resolves for the triple.

| Target | LLVM | Cranelift | CPU (LLVM / Cranelift) | Debugger |
| --- | --- | --- | --- | --- |
| `aarch64-apple-darwin` | run-verified | run-verified | apple-m1 / aarch64 baseline | lldb, breakpoints resolved statically, both backends |
| `x86_64-apple-darwin` | run-verified | run-verified | penryn / x86-64 baseline (SSE2) | not exercised |
| `aarch64-unknown-linux-gnu` | run-verified | run-verified | generic / aarch64 baseline | gdb, live, both backends |
| `x86_64-unknown-linux-gnu` | compile-only | compile-only | x86-64 / x86-64 baseline (SSE2) | not exercised |
| `x86_64-pc-windows-gnu` | run-verified under Wine | run-verified under Wine | x86-64 / x86-64 baseline (SSE2) | not exercised |
| `aarch64-unknown-none` | run-verified | unsupported | generic / none | not exercised |
| `riscv64gc-unknown-none-elf` | run-verified | unsupported | generic-rv64 / none | not exercised |
| `thumbv7m-none-eabi` | run-verified | unsupported | cortex-m3 / none | not exercised |
| `wasm32-unknown-unknown` | run-verified | unsupported | generic / none | DWARF present, not exercised |
| `wasm32-wasip1` | run-verified | unsupported | generic / none | DWARF present, not exercised |
| `aarch64-linux-android` | run-verified on a Linux kernel | unsupported | generic / none | not exercised |
| `aarch64-apple-ios` | compile-only | unsupported | apple-a7 / none | not exercised |

Run-verified means programs built for the target ran and agreed with the interpreter: on the host,
under Rosetta, in the Linux image, under QEMU — the three boards, which `support.md` writes
**emulator-verified**, the kind of run-verified no hardware stands behind (Q1-D-24) — under Node — each cell's test is named in the table in
code. `x86_64-unknown-linux-gnu` is compile-only: no x86_64 Linux machine or image is here. A
freestanding target is unsupported by Cranelift, whose object writer has no format for it, and
WebAssembly by this Cranelift build. `x86_64-pc-windows-gnu` is run-verified **under Wine**: Gate
3C's evidence, from Wine in a Linux container, never from a Windows host. **Non-goals, by
decision**: Windows with Microsoft's toolchain and Windows ARM64 (no Microsoft SDK or C runtime is
accepted here, and no ARM64 Windows machine is here), 32-bit hosted targets (`Int` is 64-bit and hosted layouts assume 64-bit
pointers), big-endian targets (nothing here runs one; layouts and the DWARF writer assume
little-endian). The CPU of an LLVM object is the driver's default for its triple, recorded and
checked, not pinned by a flag — and the driver's own: the recorded CPUs are Apple clang 21's, and the
Linux image's clang 19 resolves `core2` for `x86_64-apple-darwin` (found by N100's gate), so the check
runs against Apple clang only. Cranelift detects nothing of the building host.

**Limitation** · No Windows host has run anything (Wine only), no MSVC or Windows ARM64; no 32-bit,
big-endian or CPU-feature targets; no bundled linker or
sysroot, so a Linux target from macOS is objects only; cross-host byte reproducibility not
measured.

**BLOCKED, and it is a real finding** · `.github/workflows/ci.yml` runs
`cargo test --workspace` and sets neither `NAZM_CONTAINED` nor `NAZM_ALLOW_UNCONTAINED`,
while `crates/nazm-cli/tests/common/mod.rs` turns that into a panic for the four selfhost
stage builders. The workflow was last changed before the containment gate existed, so by
reading, those tests cannot pass in CI. CI also runs on `ubuntu-latest` x86_64, which is
**neither** verified target. Two candidate fixes — run the selfhost stages through
`cargo xtask contained`, or exclude them from the CI test step and say so — and choosing
between them is outside this task.

*R1, 2026-10-07: decided, not yet evidenced.* CI is the **host-safe subset**, and not the release
gate. `.github/workflows/ci.yml` runs `cargo xtask workspace-tests --host-safe`, which builds every
test binary and runs each except those whose source calls the containment guard — today
`nazm-cli test:selfhost` — naming each one it leaves out; the guard itself is unchanged. The
mutation catalogue left CI for the contained gate, where the runbook already required it. The
selfhost stages, bootstrap, fuzzing and mutation run only in `cargo xtask contained` and the release
gate. The runner is x86_64 Linux, and that changes no target cell: a compile-only target becomes
run-verified by a recorded run of Nazm-built programs there, not by CI compiling the project.
Nothing has been pushed, so the workflow has not run on GitHub; its first run is the evidence this
section is still waiting for.

**Next dependency** · A first green run of the host-safe workflow on GitHub, recorded.

**Accepted when** · CI runs green on a stated target and the stated target is one the
project verifies.

### 34. Smart contracts — PARTIAL

*N96, 2026-10-04: generated sequences on the reference VMs — VERIFIED; the area stays PARTIAL, sBPF
BLOCKED.* `architecture.md` §7.97 first. `crates/nazm-cli/tests/contract_property.rs`: three seeded
sequences of 250 transactions, generated from `nazm.contract/1`'s entrypoints with callers among the
owner and two others and arguments at `Int`'s edges, over a contract with transfers, an owner's
switch, a conservation breach, a loop, overflow and division. On any host the simulator is
deterministic and every committed state keeps the conserved quantity; with `--ignored`, py-evm
(`nazm-evm:n70`) and the WebAssembly reference host (`nazm-wasm:n64`) agree with it on all 750
outcomes, codes and final states, and every EVM transaction stays inside its stated gas bound. sBPF
remains BLOCKED: this machine has no BPF target in its C compiler and no Solana toolchain, validator or
emulator. Not in the model, and not added: events, external calls and reentrancy, assets as types,
metering beyond the bound, a chain's own WebAssembly host, migrations beyond `--upgrade-check`.

*N69, 2026-10-02: the chain-neutral model — VERIFIED for the model and its simulator.*
`architecture.md` §7.71 first. A contract is an ordinary module — a `State` record of `Int`s and
`Bool`s, `init`, and pure entrypoints returning `Result[State, Int]` — held to the `web3` profile
(declared effects; no `io`, tasks, channels, clock, C or recursion). `nazm contract` writes
`nazm.contract/1`: the state, each entrypoint's arguments, and its read and write sets, exact for a
state built in the result and conservative ("all") where the analysis does not follow it; external
calls do not exist in the model, so no reentrancy is claimed or analysed. `nazm contract --txs` runs
transactions deterministically on the interpreter: `Ok` commits, `Err` and runtime failures revert
with their codes, a declared `conserved` quantity must hold or the transaction is rejected (`N0396`),
and a meter counts calls and loop turns. Evidence, `crates/nazm-cli/tests/contract.rs` (7): the
facts for six entrypoints; thirteen transactions — commits, reverts by code, the owner's authority,
a conservation breach rejected with the state unchanged, malformed lines — and the same bytes twice;
a runtime failure reverting with `N0400`; five profile refusals; four not-a-contract refusals; a pure
function the same under `web3`. **Limitation**: no chain backend yet; assets are a conserved
quantity checked per transaction, not a linear type; no events, external calls or gas.

*N70, 2026-10-02: the EVM backend — VERIFIED against py-evm for the contract model's subset.*
`architecture.md` §7.72 first. `nazm contract --evm DIR` compiles a contract directly from Core IR
to deployment and runtime bytecode (no Yul or `solc`): `i64` held exactly in 256-bit words, helpers
inlined, the state read and written at name-derived slots, `conserved` enforced on-chain, failures
and `Err` reverting distinguishably. `abi.json` gives each `name(int64,…)` and its selector from the
compiler's keccak-256 (agreeing with `eth_hash`); `storage.json` the slots; `--upgrade-check` refuses a
removed or retyped field and allows an added one. Evidence, `crates/nazm-cli/tests/evm.rs` (3 on any
host — selectors and a slot pinned, a bound for every loop-free entrypoint and none for a loop, the
deployment copying its runtime from the right offset, the same bytes twice, a refusal before
emission, the upgrade check; 1 ignored, run in `nazm-evm:n70` — 19 transactions with the
simulator's outcomes, codes and final storage, the language's truncating division, remainder sign,
zero divisor and `i64::MIN / -1` on the EVM, and measured gas inside every stated bound, e.g.
`transfer` 21,598 of 63,806). **Limitation**: one reference EVM (py-evm, Shanghai); no events, value
or external calls (none in the model); the gas bound is loose (worst-case `SSTORE`s) and open for
loops; the EVM's execution semantics are held by the ignored test only.

*N71, 2026-10-02: contracts on WebAssembly — VERIFIED with the reference host, one engine.*
`architecture.md` §7.73 first. `nazm contract --wasm DIR` builds the contract with the ordinary
WebAssembly backend (§7.66) and appends one generated adapter to its own unit: `nazm_init` and a
`nazm_call_<entry>` per entrypoint, moving the state across `nazm_host.state_get`/`state_set` by
declaration index, reverting through `nazm_host.revert`, checking `conserved` in the module, and
reporting failures through `report`. `contract.json` (`nazm.wasm-contract/1`) records fields,
exports, imports and status codes; `contract_host.mjs` keeps the state and runs each transaction in
a fresh instance. Evidence, `crates/nazm-cli/tests/wasm_contract.rs` (2 anywhere — the metadata, the
unit's host imports, the same bytes twice, the ordinary build without the adapter; 1 ignored, run in
`nazm-wasm:n64` — 14 transactions with the simulator's outcomes, codes and final state, a run-time
overflow reverting with `N0400`, and the linked module importing exactly the four host functions).
**Limitation**: no chain's own host interface is bound to the four imports; one engine (Node).

*N72, 2026-10-02: account-oriented contracts — VERIFIED for signed writes and metadata; sBPF execution
BLOCKED.* `architecture.md` §7.74 first. The `accounts` profile adds `signed-writes`: an entrypoint that
writes the state must decide on its `caller` in its own body, or is refused by name; a check made only
in a helper is not trusted. `nazm contract --accounts DIR` writes `accounts.json`
(`nazm.sbpf-accounts/1`): the state account's layout, each instruction's discriminator and argument
offsets, and its accounts' writable and signer constraints derived from the read and write sets.
Evidence, `crates/nazm-cli/tests/accounts.rs` (3): an unguarded write refused while three others pass,
`web3` alone not asking for signers; a helper's check not trusted; the metadata's layout, discriminator,
offsets and constraints. **BLOCKED**: no sBPF toolchain, validator or emulator here; upstream `bpfel`
is not sBPF, so no object or run is claimed.

*N73, 2026-10-02: trust evidence — fuzzing PARTIAL (area 30), build provenance VERIFIED (area 32),
cross-toolchain reproducibility measured.* `architecture.md` §7.75 first. Two seeded fuzzers, the same
inputs on every host. `crates/nazm-core/tests/fuzz_front.rs` mutates every source of six corpora
(deletions, duplications, swaps, replaced characters, inserted tokens) and checks each in process: no
panic, shrunk if one is found; 1,500 cases per run, 60,000 run clean once. `crates/nazm-cli/tests/fuzz_diff.rs`
generates well-typed, terminating programs (every `Int` operator from edge literals, comparisons,
`&&`/`||`, `let`, `if`, bounded `while`, calls, a record) and requires one answer from the interpreter,
LLVM at `-O0` and `-O2` and Cranelift — the value, or the failure code and place; a disagreement is
shrunk by lines, and `tests/fuzz-corpus/` is replayed first. 24 cases per run; 150 on another seed agree.
**Shown to find faults**: an unsigned `<` injected into either backend is caught by the differential
fuzzer (Cranelift's by N41's named test as well), and a lexer panic on one character by the robustness
fuzzer and by no `nazm-syntax` or `nazm-core` test — three mutants whose named killer is a fuzzer. `nazm build --provenance FILE` writes `nazm.provenance/1` — sources and packages with BLAKE3
digests, target, compiler, C compiler, runtime ABI and digest, backend and options, profiles, output
digests — and `--attest-with CMD` keeps a local command's output as `FILE.sig`. `cargo xtask evidence
DIR` writes an evidence bundle's `index.json` (`nazm.evidence/1`: every file's path, size, BLAKE3 and
kind) and the compiler's `sbom.json` (`nazm.sbom/1`: the 261 crates `Cargo.lock` pins, and the
toolchain), offline; `--verify` refuses a changed, missing or unindexed file by name
(`xtask/tests/evidence.rs`, 3). Evidence, `crates/nazm-cli/tests/build_provenance.rs` (5): digests recomputed from the files, two clean
directories giving one record, a change seen in its own digest and the output's, the backend recorded,
a failing signer leaving no signature, a failed build leaving no record. *Across toolchains*: one
program built for `aarch64-unknown-linux-gnu` by Apple clang 21 (host) and clang 19 (Linux image) —
the program's and the entry's objects byte-identical at `-O0` and `-O2`; the runtime's object differs,
at `-O0` by one symbol-table byte (a mapping symbol's type) and at `-O2` by code layout (clang 21's
cold-path splitting). **Limitation**: not coverage-guided (`cargo fuzz` needs nightly and a new
dependency); the differential generator covers `Int`/`Bool` programs only; no sanitizer, Miri or Loom
run (no `unsafe` code to run them on); an attestation is whatever the local command printed — Nazm
neither signs nor verifies.

*N74, 2026-10-02: ecosystem tooling — VERIFIED as scoped (areas 17, 25, 26).* `architecture.md` §7.76
first, amended twice before code. `nazm init` writes six templates — `cli`, `library`, `server`,
`embedded`, `wasm`, `contract` — each a package whose own commands check, test, lock and document it,
and build or run it where the target allows; a library package is checked as its modules. `nazm doc`
documents exactly the public definitions, from the checker's facts, anchored by identity and linked
to source lines; `nazm bindgen` translates the `int64_t`/`bool`/`const char *` subset of a C header
and refuses the rest by name; `nazm publish --dry-run` writes nothing and names the digest the publish
then records; `nazm capabilities` lists every authority type, target and command. The language
service is tested across a package boundary — definition, references, hover and workspace symbols reach
the dependency, its errors are published against it, and renaming its public function is refused —
and `editors/vscode` is a client for `nazm lsp` with a launch configuration for an LLDB adapter.
Evidence: `crates/nazm-cli/tests/ecosystem.rs` (10: every template end to end, init's refusals, every
template file written, the library check reporting a shared error once, the API document's members,
facts, sources, anchors, determinism and size, bindgen's translations and nine refusals, its output
linked against real C and run, the dry run, the inventory, the editor client), `nazm-service`'s
`tests/across_packages.rs` (3), and four new schemas validated against real output in
`tests/schemas.rs`. **Limitation**: the editor client is not exercised inside an editor here and is
not published; no Nazm debug adapter; rename across packages is refused, not performed; `bindgen`
reads declarations, not the C preprocessor; no network registry; the agent benchmark was not re-run,
and no efficiency claim is made.

**Gate 3D (2026-10-10)** · *The systems foundation* (`spec.md` *Atomics*, *Statics*, *Boards*;
`systems-domains.md` Part B). **Atomics:** `Atomic`, one `Int` cell, sequentially consistent, that
crosses into a task — the interpreter, both backends (four runtime entries; `atomic_add` a checked
compare-and-swap loop) and the LIR oracle (`atomics.rs`). **Statics:** a number, a `Bool` or an
atomic, read-only unless an atomic (`N0625`), defined in its module's own unit under a durable
`nz.s.` symbol and imported by every other — generic instances too (`statics.rs`). **Boards:** a
manifest (`--board`, `N0626`) that the built-in boards print as; `@section`, kept by the generated
`link.ld` (`sections.rs`); 64-bit device registers; AArch64 interrupts — a vector table, a GICv2,
the virtual timer, named lines, handlers held to their shape (`N0627`, `interrupts.rs`); a bump
arena for a manifest's `[heap]` that admits strings, sequences and closures, and `no-heap` in the
new `kernel` profile (`heap.rs`); `@on_failure` (`kernel.rs`). **Emulator-verified**, every
mechanism and the reference kernel-style workload booted under QEMU at `-O0` and `-O2` in the
`nazm-qemu:n86` image (each file's ignored test, run by hand); RISC-V has no interrupts, and nothing
has run on hardware. Semantic epoch 49, runtime ABI 24. Found on the way: Cranelift's x86-64
backend converts a float only to a 32- or 64-bit integer (3B-2, fixed); a placed function was
inlined away at `-O2` (now hidden linkage); a failure hook's first call looked like a failure while
the program's flag was set (cleared before the hook).

**Gate 3E (2026-10-11)** · *Embedded* (`spec.md` *A freestanding target*, *Boards*; `systems-domains.md`
Part C). **The Cortex-M board:** `thumbv7m-none-eabi` on QEMU's `mps2-an385`, the ninth target — 32-bit
pointers and a 64-bit `Int`; a vector table at 0; the CMSDK UART; AArch32 semihosting; the 64-bit
atomics (interrupts masked), division and the EABI's memory functions the board's runtime defines;
floating point, 64-bit device registers and a heap refused by name (`cortex_m.rs`). **Interrupts on a
Cortex-M:** an NVIC and SysTick from a manifest's `controller = "nvic"` and `clock_hz`. **`@std/hal`:**
`Uart`, `Pin`, `Timer` and the emulated boards' impls. **The reference workload:** one traffic light,
generic over the HAL, run unchanged on both AArch64 `virt` and the Cortex-M3 (`embedded.rs`).
**Emulator-verified** at `-O0` and `-O2` in the `nazm-qemu:n86` image; nothing on hardware. Runtime ABI
25. Found on the way: an atomic passed by value reached the string runtime's reference counting on a
heapless board (now a no-op there, every owner being null), and a Cortex-M3 has no CMSDK GPIO in QEMU,
so its pins are the FPGA LED register.

**Gate 3F (2026-10-11)** · *Realtime* (`spec.md` *Restriction profiles*, *Boards*; `systems-domains.md`
Part D). `realtime` gains `bounded-stack` (no call cycle, no call through a function value) and
`no-heap`. A board's **fixed-priority scheduler**: `@task(priority = "N", period = "P")` in board-timer
ticks; `main` runs first as the initialisation, then the highest-priority released task runs to
completion, one at a time, the core waiting for an interrupt when none is; a task released again before
it has run fails the image (`N0414`, new); a task's non-zero answer ends the run; the scheduler owns the
timer. **Emulator-verified** on both boards at `-O0` and `-O2` (`realtime_tasks.rs`): the higher priority
ten times by the lower's fourth run (`1004`), an overrun reported. No WCET, no preemption, never "hard
real-time". Semantic epoch 50. Found on the way: `-O2` deletes a busy loop that does nothing visible, so
an overrun test must do observable work.

**Gate 3G-1 (2026-10-11)** · *Domain profiles* (`spec.md` *Restriction profiles*, *Domain profiles*,
*Boards*; `systems-domains.md` Parts E–J). The constitution for the six domain profiles and their
three domain rules (`watchdog`, `audit`, `no-float-nondeterminism`), the runtime watchdog and
`N0415`. Four are enforced now, each a composition held to its definition (`domain_profiles.rs`):
`robotics` (`realtime` + `explicit-authority`), `automotive` (`realtime` + `critical`), `aerospace`
(`embedded` + `realtime` + `critical` + `no-float-nondeterminism`, refusing the nine transcendental
built-ins by site) and `assurance` (`critical` + `no-recursion`, `bounded-loops`, `bounded-stack`,
`no-heap`). A board's periodic program, as a locked package, passes and builds under all four. The
`spec.md` rows for `critical` and `cyber` now name `no-unknown-calls` and `checked-paths`, which they
had held since N80 and N87. No certification, qualification or approval is claimed.

**Gate 3G-2 (2026-10-11)** · *Domain workloads* (`systems-domains.md` Parts E–J; `domains.rs`).
`@std/linalg`: `Vec3`, `Mat3` and their sums, products, norm and length bound over `Float64`, nothing
allocated, every operation correctly rounded. Four reference workloads, each held to its profile as a
locked package and booted under QEMU at `-O0` and `-O2`: **robotics** (sense, control through
`@std/linalg`, a bounded command, convergence, a missed deadline stopping safely), **aerospace**
(simulated sensors, a complementary filter, telemetry, a sensor fault and its transition,
byte-identical across levels), **automotive** (an ECU state machine and checksummed frames,
byte-identical on AArch64 and Cortex-M3) and **assurance** (a heater controller whose every obligation
is `proved` or `checked`, holding and tripping on both boards). Found and fixed on the way: a heapless
64-bit board had no `memcpy` or `memset`, which code generation calls for a large value's copy — now
every AArch64 and RISC-V board has the C library's memory functions; and AArch64 board code may now
make no unaligned access (`-mstrict-align`), since with the MMU off every access is to Device memory
and a 16-byte SIMD copy at an 8-byte boundary faulted. Found, not fixed: `float_round`, `float_min`
and `float_max` call the C library and are refused on a board (`limitations.md`). Emulator-verified;
sensors simulated; no certification.

**Gate 3G-3 (2026-10-11)** · *Industrial and the watchdog* (`spec.md` *Boards*, *Domain profiles*;
`domains.rs`). A board's manifest may have `[watchdog] ticks = N`, counted on the scheduler's ticks:
`watchdog_feed()` — a device's authority, as `mmio_*` — re-arms it, and `N` ticks without a feed fail
the image with `N0415` (new), the failure hook first. Refused by name: a feed for a board without one,
a watchdog with no `@task`, a watchdog without the timer (`N0626`), a hosted feed (`N0392`). The
`industrial` profile is `realtime` + `critical` + `watchdog`, the rule that a `@task` feeds it. The
batch controller runs three batches on both boards at both levels, fails safe on overfill, and a loop
`bounded-loops` admits — 10^12 turns — is caught by the watchdog: a stated bound is not an execution
time. Semantic epoch 51, runtime ABI 26. The watchdog is the runtime's, not a hardware one.

**Gate 3G-4 (2026-10-11)** · *Medical-device style* (`spec.md` *Domain profiles*; `domains.rs`).
`@std/audit`'s `audit_transition` writes a transition's record, a JSON object on its own line, to a
board's UART, allocating nothing. The `medical` profile is `realtime` + `critical` + `audit`: every
function of the program that calls `atomic_store`, `atomic_swap` or `atomic_compare_swap` must call
`audit_transition` itself, each unrecorded change refused by its site; `atomic_add` counts and is not
a transition. The infusion pump runs on both boards at both levels, its six records in order, a pause
and resume, and an occlusion's fault state. No regulatory approval is claimed.

**Gate 3G-5 (2026-10-11)** · *Cybersecurity* (`systems-domains.md` Part K; `secure_service.rs`). The
reference secure service: a TCP service whose protocol answers five capital letters reversed and
anything else `DENY!`, never echoed, its authority handed down from `main`. Held to `cyber` as a
locked package and served to three real clients on the loopback. Each of `cyber`'s refusals is shown
on a variant of it: a source changed after locking, a client's bytes on standard output (and, without
an `OutCap`, the language's own `N0369` first), a function with no declared contract, a call into C,
a call through a function value. Built twice in two directories, its executable, provenance record
and SBOM are byte-identical; its record, signed with `ssh-keygen`, verifies and is refused once the
executable changes. Run-verified on macOS AArch64; signing is the host's `ssh-keygen`. No sandboxing,
non-interference or certification claim.

**Gate 3H (2026-10-11)** · *AI / HPC* (`spec.md` *Accelerator kernels*, *Providers*, *SIMD*;
`systems-domains.md` Part L; `accel_providers.rs`). A **provider boundary**: a kernel is one source
text — the OpenCL C a device builds, with an element function `nz_item` beside the `__kernel` — and a
provider runs it. The OpenCL provider builds it for Apple's GPU; the new **CPU provider** compiles
the same text as C behind a prelude for OpenCL's words and runs `nz_item` over a pool of threads.
`--provider auto|opencl|cpu`: `auto` runs on the GPU where it can and otherwise on the CPU, saying
why in the report and on stderr; where the GPU ran it, the CPU provider runs it too as a second
oracle (`cross_check`). **Float kernels**, `Float32` and `Float64`, one type throughout, `float_sqrt`
and `float_abs` the built-ins they may call, held to the interpreter bit for bit (every NaN alike):
no fused multiply-add on either provider, division and square root correctly rounded on the device.
Verified here: `Int` and `Float32` on the GPU and the CPU, agreeing with each other and the
interpreter; `Float64` refused by name on Apple's GPU (no `cl_khr_fp64`) and run on the CPU. `--scalar`
builds the CPU loop without LLVM's vectoriser; the comparison is filed in `performance.md`.
`nazm.accel/3`. Metal, CUDA, ROCm and SPIR-V remain named future providers.

**Gate 3I (2026-10-11)** · *Storage and the distributed foundation* (`systems-domains.md` Parts M,
N; `storage.rs`, `distributed.rs`). `examples/apps/pagestore`: fixed checksummed pages written in
place behind a redo log of whole page images, a fresh store made atomically, one owner task serving
readers and a writer through channels, a lock for one process. A run is crashed at **every** write
boundary — half that write written — and each recovery checked against a model of the acknowledged
writes: all there, nothing else but the one in flight, torn pages repaired from the log. The test
found two defects in the engine as first written (a log handle kept across the checkpoint that
replaced its file; a reopen that reset the crash counter), both fixed before this was committed.
`@std/dist`: `NodeId`, framed `Msg`, the `Transport` trait with `Tcp` and `Local`, `retry` with
backoff and an idempotency key. `examples/apps/ledger`: a server stopped after logging a credit and
before answering, restarted on its address, the client's retry carrying it through, every credit
applied exactly once; in one process, a lost answer timing out through `select`, interpreted and
native alike. No database-maturity, consensus or partition-tolerance claim.

**Gate 3J (2026-10-11)** · *WASI* (`spec.md` *WASI*; `systems-domains.md` Part O; `wasi.rs`).
`wasm32-wasip1`, the tenth target: a heap — a bump arena grown by
`memory.grow` — under the parts a board with a heap takes, so strings, sequences, `Bytes` and
closures run; standard streams, arguments, the clocks and `time_sleep`; files and handles beneath
the first preopened directory, a path outside it refused by the host; a failure reported with
status 2. Tasks, channels and the network are refused by name: preview 1 has no threads or
sockets. Run-verified at `-O0` and `-O2` under Node's `node:wasi` in `nazm-wasm:n64`, every import
`wasi_snapshot_preview1`'s. Narrowed: no directory listing, environment or freeing.

**Gate 3K (2026-10-11)** · *Mobile and desktop* (`spec.md` *Mobile targets*; `systems-domains.md`
Parts P, Q; `mobile.rs`). Two more targets. `aarch64-linux-android`: Linux's kernel with Bionic, a
switch target of its own; a C host and a Nazm library, and a whole program with its task pool,
channels, files and clock, built as static executables by the NDK r27c and **run on an aarch64 Linux
kernel** — a new support status, `run-verified on a Linux kernel`, because no Android device or
emulator ran them. `aarch64-apple-ios`: Darwin's unit; an archive and a program **linked** against
Xcode 27's iOS SDK and never run (`compile-only`), no simulator runtime being installed. `--backend
cranelift` is refused for both. The desktop example, `examples/desktop/counter`: a Cocoa window whose
button callbacks call a Nazm shared library's state machine through the C ABI, driven by synthetic
clicks, every label checked — run-verified on this host.

**Gate 3L (2026-10-11)** · *Web3 qualification and traceability* (`systems-domains.md` Parts R, S;
`spec.md` *Requirements traceability*). The web3 qualification: every claim of the contract model,
its profiles, the EVM and WebAssembly backends' agreement with the simulator and the upgrade check,
each against the test that holds it. Tightened: an EVM entrypoint's gas bound now runs through a
loop in the counted form — its turns times its condition and body — where it had been `null`; the
EVM stays inside it (`evm.rs`, py-evm). sBPF BLOCKED. `nazm trace FILE --requirements FILE`,
`nazm.trace/1`: each requirement to its definitions by durable identity and Core IR digest, their
obligations keyed by definition, and its `@test`s run; a renamed definition, a missing or failing test
reported by name and failing the trace (`trace.rs`, `examples/requirements`). No certification.

