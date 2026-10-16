## Context

The audit (task 1.1) starts where the comparison is furthest behind: word frequency, 188 ms against
Rust's 65 ms and C's 74 ms (2026-10-15).

## Findings

### Word frequency, profiled (2026-10-15)

`bench/compare/nazm/words.nz` built by a release `nazm` from `c86f2f81` (`--opt-level 2`, LLVM),
run over ten copies of the comparison's input (20,000,000 words), sampled with macOS `sample` for
one second: 767 samples on top of the stack.

| Where | Samples | Share | What it is |
|---|---|---|---|
| `hashmap_find` + `memcmp` + `Hash/Str/same` | 144 + 46 + 22 | 28% | probing and comparing keys; `counts[w] += 1` probes once (R1, 5b.9) |
| `Hash/Str/hash`, `hash_combine`, `key_hash` | 137 + 16 + 15 | 22% | FNV-1a one byte at a time, each byte's index bounds-checked, then a 64-bit finaliser |
| allocation: `malloc`, `free`, `realloc`, `vec_release`, `arr_grow` | 153 | 20% | each line's `split` builds a `Strs` and frees it; macOS's allocator reads the clock on free (`mach_absolute_time`, 93 samples) |
| `text_split` | 129 | 17% | the scan itself; each word is a view of the line (`str_slice` retains, never copies) |
| `hashmap_set`, `hashmap_at_or`, `main` | 36 + 32 + 28 | 13% | the update and the loop |

Measured: 0.18 s, 20 MB peak, 61 KB executable for the comparison's input.

### Ranked opportunities (hypotheses until measured)

1. **No list for `for w in s.split(sep)`** (R1.5 3.3): the loop takes each view as it is found, so
   the per-line allocation and its release go — the largest share that changes nothing a program
   means. **Done 2026-10-15: 189 → 151 ms (−20%, hyperfine, 15 runs each, same host), 19 MB peak
   unchanged.** The two functions are `pub` because a check reused from a cache sees a module's
   interface, which has no private function.
2. **A hash that reads a word at a time** (3.2): FNV-1a over bytes is a serial chain of multiplies.
   A native built-in with **the same values** keeps every map's order; a faster function changes
   the order of `hashmap_keys`, which the spec promises only to be the same in every run — a choice
   for the copyright holder, since programs may print that order.
3. **The allocator** (3.1): a size-class allocator in the runtime avoids the C library's per-free
   cost; it must keep the heap limit's accounting and every memory test's live counts.
4. **Probing** : a key's stored hash is compared before its bytes already; the remaining cost is
   the bounds checks on the table's vectors, which a proof of `index < len` would remove (phase 2).

### After the split change (2026-10-15)

Re-profiled at 151 ms (828 samples over ten copies): `text_split_end` 29% (a byte loop of about
twelve instructions per byte — the length and pointer reloaded and the index bounds- and
overflow-checked each step), `hashmap_find` 25%, `hashmap_set` + `at_or` 15%, `memcmp` 8%, the
`Str` hash 6%, the program's own loop 7%. Linking every unit with LTO (`clang -flto`) gave only
3.4% (151 → 146 ms): the cost is in the operations, not the calls between modules.

Next, ranked: a `memchr`-backed byte search as a built-in for `text_split_end`, `text_find` and
`text_lines` (a new built-in name, so an epoch); the probe's checks proved away (phase 2); a
string equality that compares lengths and the first word inline before `memcmp`.

### `fib`: what a call costs (2026-10-15)

`fib` at `-O2` (55 ms; C 35 ms). Each call, beside C's work: a load of `nz.stack_limit` and a compare
with the frame pointer (the stack check), the failure bit returned beside the result and tested
after each of the two calls (`tbnz w1`), and the addition's overflow branch. The subtraction's
check is proved away already. C's `fib` is also turned by LLVM into one call and a loop, which the
failure test between the two calls prevents here.

Candidates, each a design change to weigh against what the language promises:
1. **Stack overflow by a guard page** instead of a check per call: the runtime's `SIGSEGV`/`SIGBUS`
   handler on an alternate stack reports the same failure when the fault is in a guard page — every
   task stack needs one. Removes the load and compare from every call.
2. **Failures by unwinding** instead of a returned bit: no cost where nothing fails, as C++ and Rust
   pay; the bit was chosen in R1 (5b.8) for the interpreter's and both backends' simplicity.
3. The overflow check stays — it is what the language means — but a range proof removes it where
   the operands are bounded.

### What each part of a call costs, measured in C (2026-10-15)

`fib(35)` written in C five ways, `clang -O2`, `hyperfine -N --runs 10`, on the same host:

| variant | time | over plain C |
| --- | ---: | ---: |
| plain C | 35.6 ms | — |
| + the overflow check on `+` only | 44.4 ms | +25% |
| + the failure bit only (returned beside the result, tested after each call) | 55.9 ms | +57% |
| + both | 58.3 ms | +64% |
| + both and a stack check against a global limit | 58.2 ms | +64% |

So the **failure bit is Nazm's whole gap on `fib`** (Nazm: 55.4 ms), the overflow check a quarter,
and the stack check nothing measurable.

### Decisions (delegated detail, 2026-10-15)

- **4.1 — no guard page.** The check per call costs nothing measurable, and a guard page could not
  keep what the spec promises for `N0408` — the failure unwinds, `defer` blocks run, an enclosing
  `scope` joins its tasks — since a fault handler cannot resume into a frame's cleanup. Kept as is.
- **4.2 — failures by unwinding**, replacing the returned bit: no cost on the path where nothing
  fails, as C++ and Rust pay, and LLVM may again transform the calls. What a program sees is
  unchanged (the same diagnostic, the same cleanup order, exit status 2). It touches LIR's calling
  convention, both backends (LLVM `invoke`/landing pads with a personality; Cranelift's exception
  calls), the runtime, and R3/R9, which port the convention — so it is made in the reference before
  they port that part. Expected on `fib`: about 44 ms (the overflow check remaining), against C's 36.


### 4.2 in detail: failures by unwinding (design and a measured prototype, 2026-10-15)

#### Where the bit lives today

- **nazm-lir:** `op/ir.rs` (`Conv::Nazm`, `Sig::fails`, `Inst::Call`/`CallIndirect`'s `failed`,
  `Term::Fail`, `cold_blocks`); `op/func.rs` (`call_failing`, `check_call`, `leave_if`, the
  large frame's `frame_entry`, direct and indirect calls, MIR's `Unwind` terminator as
  `Term::Fail`); `op/print.rs` (`failed vN`, `fail`); `op/validate.rs` (the bit's rules);
  `op/interp.rs` (the LIR oracle's failure exit is the bit); `op/llvm.rs` (`{T, i1}` results,
  `insertvalue`/`extractvalue`); `lower.rs` (the plain entry of a function the platform calls).
- **nazm-codegen-clif:** `lib.rs` (`enable_multi_ret_implicit_sret` for the pair,
  `TRANSLATOR_REVISION`'s 4), `translate.rs` (the bit as the last result, the zero stand-in,
  cold blocks).
- **The runtime:** nothing calls a Nazm function directly. `nz.fail` records the failure in the
  thread's flag (`core.rs`, `board.rs`, `wasi.rs`, `wasm.rs`); plain entries — `main`, C
  exports, interrupt handlers, a board's task and failure hook, a task's body — read the flag.
- **nazm-cli:** `build/lower.rs` finds the contract's `init` by `define hidden i1`; tests
  `failure_bit.rs`, `runtime.rs`, `freestanding.rs`, `build.rs`.
- **The ports:** `toolchain/pipeline/ops.nz` (a call's `[result, failed]`, the frame entry),
  `rtext.nz`, `runtime.nz` (R3); `toolchain/codegen/lir.nz` (`dst2`), `sel.nz` (D4: the bit in
  the next integer register), `sel_x64.nz` (`rdx`) (R9).
- **Schema and mutants:** `schema/nazm.lir-2.json` (the text carries `failed`); in
  `xtask/mutations/mutations.toml`, the four `r1-…-failure-exit-says-it-did-not-fail` and
  `r1-…` call mutants and `n50-the-cranelift-indirect-call-ignores-a-failure`.

#### The design

1. **What does not change.** MIR already gives every statement that can fail its failure edge,
   which releases what the frame owns, joins every open `scope`, and ends in `Unwind`
   (`fail` in LIR). `nz.fail` still records the first failure, with its diagnostic and
   position, in the thread's flag; `defer` blocks still do not run on a failure
   (`docs/spec.md`, *What it is not*); the process still exits 2 from the plain entry. Only how
   a failure crosses a *call* changes. The interpreter and the LIR oracle are unchanged: the
   oracle's bit is how it models the edge, not a machine convention.
2. **Raising.** `fail` lowers to a call of the runtime's `nz.raise`, which starts a **forced
   unwind** (`_Unwind_ForcedUnwind`, phase 2 only, a per-thread exception object, a stop
   function that aborts at the end of the stack). Forced rather than `_Unwind_RaiseException`:
   no search phase, no catch clauses, no type information, no C++ runtime.
3. **The personality.** `__gcc_personality_v0`, the C one (`__attribute__((cleanup))`): in
   libSystem's compiler-rt on Apple, `libgcc_s`/`libunwind` on Linux and Android; every landing
   pad is a `cleanup`. No personality of our own is needed; one is an option later (Windows).
4. **Calls.** A Nazm call whose failure edge has work (a release, a `scope` join) is an
   `invoke` whose landing pad continues on that edge; the edge ends in `fail`, which raises
   again, so each frame that owns something lands once and the unwind is linear in the depth.
   A call whose edge is a bare `fail` is a plain `call`: the unwind passes through the frame
   at no cost — `fib` has no landing pad at all, and LLVM may transform its calls again.
5. **Boundaries.** Every call of a Nazm function with no failure edge — a plain entry's call
   of the body: `main`, a C export, a callback C calls, a task's body, an interrupt handler —
   is an `invoke` whose landing pad continues as if the call returned; the plain entry then
   reads the flag, as now. So C is never unwound into, and a task's failure stops at its
   entry, where its `scope` learns of it exactly as today.
6. **`N0408`.** The stack check is unchanged (decision 4.1). The unwinder runs in the quarter of
   the stack the limit leaves as headroom (`nz.stack_check`): at least 2 MiB on every hosted
   thread, far above what `_Unwind_ForcedUnwind` needs. Its slow branch is weighted cold (the
   prototype found this is half of `fib`'s gain, below).
7. **Targets.** The convention is the target's: **hosted** targets (macOS, Linux, Android, iOS)
   unwind; **WebAssembly, WASI and the boards keep the bit** — they have no unwinder, and a
   board's image must not grow one. Windows (MinGW, SEH) unwinds only once verified under Wine
   with `__gcc_personality_seh0`; until then it keeps the bit. One `Conv::Nazm` in LIR; the
   backend lowers it by the target.
8. **LIR's text and schema.** LIR keeps `failed vN` on a call — now *the call's failure edge*,
   an abstract value — and the validator gains one rule: a `failed` value is only the
   condition of the branch that ends the call's block, so every backend can turn it into an
   unwind edge (or keep it as a register on a bit target). `nazm.lir/2`'s grammar is
   unchanged; its description changes. No schema bump; the semantic epoch is unchanged since
   no program's meaning changes.
9. **Revisions.** `RUNTIME_ABI_REVISION` 33 → 34 (a Nazm function's machine result, `nz.raise`
   and the personality in the hosted runtime); the LLVM backend's cache identity follows it.
   Cranelift's `TRANSLATOR_REVISION` moves only when Cranelift unwinds.
10. **Cranelift 0.136.** `try_call`/`try_call_indirect` with exception tables exist and are
    supported for `SystemV` and `AppleAarch64`, and a finalised function lists each call
    site's handlers. But `cranelift-object` writes `.eh_frame` without a personality or LSDA,
    and none at all for Mach-O (it panics), and `nazm-codegen-clif` emits no unwind
    information today. Unwinding there means writing, with `gimli` (already a dependency), a
    CIE carrying the personality, an FDE per function and a `gcc_except_table` per function
    from the handler list, for ELF and Mach-O. Cranelift and LLVM objects are never linked into
    one program and the runtime calls no Nazm function, so **Cranelift may keep the bit** until
    that is done: the convention is per backend, invisible to a program.

#### The order of work

1. The reference (Rust): LIR's validator rule and documentation; `op/llvm.rs` by target; the
   runtime's `nz.raise` and stop function (hosted runtimes only); the debug-information pass and
   the sampler taught `invoke`, landing pads and the new runtime symbols; the boundaries'
   landing pads; tests: `failure_bit.rs` renamed to the failure convention and extended
   (a failure while the stack is exhausted, a failure through a callback C calls, the IR has
   no `{T, i1}` on hosted targets and keeps it on boards and WebAssembly); mutants replaced
   (`the LLVM failure exit returns`, `a call that releases has no landing pad`, `a plain
   entry's call has no landing pad`). The runtime ABI bump in the same commit.
2. R3 ports the printer change to `toolchain/pipeline` (its text must stay the reference's).
3. Cranelift (optional before 1.0): the unwind information above, then `try_call`.
4. R9 keeps the bit until its own code generator writes `.eh_frame` and LSDAs; the same tables
   as Cranelift's, written once, in Nazm.

#### The prototype (branch `proto/unwind-llvm`, not for `main`)

`op/llvm.rs` only, hosted targets only, 95 lines: `fail` calls `nz.raise` (defined in each
module, linkonce), a Nazm call with work on its edge is an `invoke` whose cleanup pad rejoins
the edge (the bit becomes a `phi` that LLVM threads away), a call to a bare `fail` is a plain
`call`, and the stack check's slow branch is weighted cold. Release `nazm`, `--opt-level 2`,
`hyperfine -N --warmup 3 --runs 15`, same host, same session:

| | before | after |
| --- | ---: | ---: |
| `fib(35)` | 55.6 ms ± 0.3 | **44.3 ms ± 0.2** (−20%) |
| word frequency | 133.5 ms ± 3.4 | 131.2 ms ± 1.6 (−2%, within noise) |
| C `fib`, overflow and stack checks added by hand | | 44.2 ms |
| plain C `fib` | | 35.4 ms |

What the prototype found: **neither change alone moves `fib`.** Without the bit but with the
stack check unweighted, the code is instruction for instruction C's with an overflow check, but
the check's fast path is a taken branch: 56 ms. The bit kept and the check weighted (the IR
relinked by hand): 55.7 ms. Both: 44.3 ms, exactly C with the same two checks. The C table above
gave the whole gap to the bit, probably because there the bit also blocked the
recursion-to-loop transform plain C gets; here the overflow check blocks it, so the remaining
25% over C is the overflow check (candidate 3, a range proof).

Correct: `fib` and `words` print the same. `failure_bit`'s six tests (deep recursion, function
values, trait methods, a task's failure failing its `scope`, a C export) pass: the same output,
status and memory counts on the interpreter, LLVM `-O0`/`-O2`, Cranelift and the oracle. An
overflow 10,000 frames deep through `defer`s, and stack exhaustion (`N0408`) after ~10^5 frames
each owning a string, print the same as before the change at `-O0` and `-O2` and exit 2. The
failure path is slower: unwinding that exhaustion takes 1.14 s against 0.35 s (one forced unwind
per frame that owns something). Broken, as expected (19 of `cli_c_d` + `cli_e_l`'s 540): the
boards and freestanding builds (`_Unwind_ForcedUnwind` is not freestanding), two `ffi_v3` tests
on non-hosted targets, debug information's text pass (an `unreachable` without a location) and
the sampler's attribution to Nazm functions.

#### Estimate

The reference with tests, mutants and the bit kept for boards and WebAssembly: 3–4 days. R3's
port: 1 day. Cranelift's unwind tables and `try_call`: 4–6 days, deferrable. R9: after its
object writer, 3–5 days, deferrable. Windows: 1–2 days under Wine, deferrable.

### Coordinator's answers on 4.2 (2026-10-15)

1. The stack check's cold weighting lands **with** unwinding, not alone: alone it gained nothing.
2. Two conventions are accepted: unwinding on hosted targets, the bit on WebAssembly, WASI and the
   boards. A program cannot tell them apart.
3. Cranelift and R9 keep the bit until each writes unwind tables; R9 is to write them before 1.0,
   since it replaces Cranelift as the fast build.
4. A slower failure path on deep unwinds is accepted: a failure ends the program or its task, and is
   not the path speed is measured on.
5. The system's unwinder and the C personality (macOS's libSystem; libgcc_s elsewhere) are accepted.

A correction to the table above: the bit's measured +57% in C included LLVM no longer turning one
call into a loop. The prototype shows the bit and the taken stack-check branch together cost Nazm's
gap: without both, `fib` is 44.3 ms — C with the same overflow and stack checks to the tenth.


### 4.2 in the reference (2026-10-15)

Implemented as designed in the Rust toolchain (`docs/architecture.md` §7.115), with these choices:
`nz.raise`, its stop function and the exception object are defined `linkonce_odr hidden` by each unit
that has a failure exit rather than in the runtime, so the runtime artifact stays target-agnostic
(Windows and Cranelift link the same one) — runtime ABI 34 all the same, since an image now links the
system's unwinder; the convention is chosen by `Target::failures` (macOS, Linux, Android, iOS
unwind); and the stack check's weight is on every target. LIR's validator holds a call's `failed`
value to the branch ending its block; no schema or epoch moves. The prototype's 19 failures were the
debug pass (an `invoke` in a plain entry had no location, so LLVM dropped the unit's debug
information — the sampler's attribution followed from that) and the boards; the boards keep the bit,
the debug pass learned `invoke` and the `linkonce_odr` definitions, and a one-thread program makes
`nz.exc` an ordinary global.

Release `nazm`, `--opt-level 2`, `hyperfine -N --warmup 3 --runs 15`, same host and session, before
(`50933cbd`) and after:

| | before | after |
| --- | ---: | ---: |
| `fib(35)` | 55.7 ms ± 0.3 | **44.3 ms ± 0.3** (−20%) |
| word frequency | 132.4 ms ± 1.0 | 129.4 ms ± 0.8 (−2%) |
| a stack exhausted 196,098 frames deep, each owning a string and a `defer` (failure path) | 8.9 ms | 2.26 s |

The failure path costs one new forced unwind per frame that owns something, and on macOS each is
dominated by the system unwinder's lookup of the frame's unwind information: slower than the
prototype's estimate, accepted by answer 4 and recorded in `docs/limitations.md`.

**What R3 must change in `toolchain/pipeline`** (not done here). Its LIR text is unchanged, so no
digest moves, and it prints no LLVM of its own. It must: regenerate `rtext.nz` from the runtime crate
(`toolchain/pipeline/tests/runtime_text.py`: `rt_abi_revision` 33 → 34, and `nz.exc` among the
one-thread globals if the generator carries `SINGLE_THREAD_STATE`); give its LIR (`ops.nz`) the
validator's rule when it validates LIR — a call's `failed` value only the condition of the branch
ending the call's block, the call that block's last instruction (its lowering already obeys it);
and keep `failure_convention.rs`'s expectations in mind wherever its harness ports
`failure_bit.rs` (renamed) or reads `{i64, i1}` from hosted LLVM text. R9's code generator keeps the
bit (`sel.nz`, `sel_x64.nz`) until it writes unwind tables.

### Word frequency through the standard library and the runtime (2026-10-15)

Release `nazm` from `613047d2`, `--opt-level 2`; A/B as three alternating rounds of
`hyperfine -N --warmup 3 --runs 7` per binary on the comparison's input, the host loaded by other
work, so only relative numbers (and the counted cycles and instructions) are kept. Each candidate
was first measured on a copy of the library's path in a program of its own (the library is compiled
into `nazm`), then in the library.

| change | cycles | instructions | kept |
|---|---:|---:|---|
| `hashmap_find` reads `m.slots`, `m.hashes`, `m.keys` at each step: a field read borrows, where the local copies were three retains and three releases (one a call of `nz.vec_release`) per look-up | −1.9% | −5.2% | yes |
| `text_split_end` compares a separator of several bytes byte by byte, not as a slice: the function calls nothing, so the one-byte loop runs with no frame to save (twelve register saves and restores per piece before) | −2.4% | −2.9% | yes |
| the probe tests a used slot first | −0.5 to −0.8% | −1.4% | yes |
| `hashmap_at_or` hashes inline rather than through `key_hash` | −0.3% | −1.2% | no (noise) |
| `Str`'s `same` as a byte loop below 17 bytes instead of `memcmp` | +3.1% | +5.5% | no: a mispredicted exit per word |
| `text_split_end` testing eight bytes per step, the first match chosen by selects | +7.2% | +5.9% | no: every byte keeps its bounds check |
| `text_lines` with an inner scan per line | +1.6% | −3.4% | no |
| a runtime byte search: `text_split_end` calling `memchr` (linked by hand) | +7.3% | −2.2% | no: words are 3–8 bytes, `memchr`'s set-up costs more than the loop. **No byte-search built-in is needed for this workload** |
| non-atomic `Str` reference counts in a one-thread program (the IR edited by hand) | **−7.4%** | +1.1% | made afterwards in the runtime: see *Plain reference counts* below |

The three kept, in the library: 132.1 → 127.8 ms median (−3.3%), cycles −3.4%, instructions −8.5%;
the output is `words.expected`'s. The hash's values, and so every map's order, are unchanged.

Re-profiled (`sample`, ten copies of the input, 790 samples): `text_split_end` 30%,
`hashmap_find` 24%, `memcmp` + `Hash/Str/same` 14%, `hashmap_set` 7%, the program's loop 7%,
`hashmap_at_or` 5%, the `Str` hash + `hash_combine` 7%, `text_lines` 4%.

What remains is not the library's to remove:

1. **A `Str` read out of a vector is retained and released atomically** (`ldadd`, `ldaddl` with a
   release fence) on every key the probe compares, twice per counted word (`hashmap_find`, then
   `hashmap_set`'s check of the remembered entry). In a program of one thread (`Needs::single`, the
   rule that already makes the failure state plain globals) the counts can be ordinary loads and
   stores: −7.4% measured. It is a runtime change, but its single-thread variant is chosen where
   `nazm-lir`'s backend calls `inline_reference_counting` and in the runtime unit, and R3's
   `toolchain/pipeline` (`rtgen.nz`, `rtext.nz`) must port it so its text stays the reference's —
   **Done afterwards (next section).** Stage P's borrowed reads of vector elements would remove
   the pair altogether.
2. `hashmap_set` must compare the key with the remembered entry again (a different key may be
   passed; mutant `r1-a-remembered-entry-is-any-key-s`): only the compiler knows `m[k] += n` passes
   the same `k` to both calls — stage P (one probe for the read and the write).
3. The one-byte scan is eight instructions a byte, the bounds check's `i < 0` test kept inside the
   loop and the string's pointer reloaded; its exit mispredicts once a word. Range proofs (stage P)
   remove the check; the misprediction is the workload's (C's `strtok` pays it too).
4. FNV-1a is a serial chain of multiplies, four cycles a byte; it stays while map order is promised.

### Plain reference counts in a program of one thread (2026-10-15)

A program of one thread (`Native::single_thread` → `Needs::single`: `main` holds no `SpawnCap`,
`NetCap` or `ForeignCap`, it is not a library and links no prebuilt runtime) cannot hand a value to a
task, so the counts made atomic for tasks need not be. `nazm_runtime::plain_reference_counts`
rewrites, inside the functions of `PLAIN_IN_ONE_THREAD` only (`nz.str_retain`, `nz.str_release`,
`nz.closure_retain`, `nz.closure_release`, `nz.chan_retain`, `nz.chan_release`,
`nz.chanv_release`), each `%r = atomicrmw add|sub ptr %p, i64 1 monotonic|release` into
`%r = load i64, ptr %p` / `%r.plain = add|sub i64 %r, 1` / `store i64 %r.plain, ptr %p` — `%r` is
still the old count, so the test for the last reference and the free are unchanged and the
arithmetic wraps as the atomic one did — and drops their `fence acquire`. It is applied to the whole
runtime unit by `emit_runtime` when `needs.single`, and to the copies a hosted LLVM unit inlines
(`inline_reference_counting(unit, single)`, called from `nazm-lir`'s backend with
`native.single_thread`), so a copy stays the definition the program links. The report's counters
(`nz.*_allocated`, `nz.*_reclaimed`) stay atomic: they are not reference counts and are off the hot
path. Cranelift and R9's `nazm` backend call the runtime's functions and emit no count of their own,
so they get the plain counts through the runtime unit; `nzcg --one-thread` needs nothing more. A
sequence's count was plain already. Boards define their own (empty) string counting.

Release `nazm` with and without the change (otherwise the same tree, `2cc1b14e` + this change),
`--opt-level 2`, the comparison's input, `hyperfine -N --warmup 3 --runs 21`, three alternating
rounds (the host loaded by other work, so relative numbers only):

| | before | after | change |
| --- | ---: | ---: | ---: |
| word frequency, round 1 | 137.7 ms ± 3.7 | 128.0 ms ± 2.5 | −7.0% |
| word frequency, round 2 | 137.0 ms ± 2.2 | 125.5 ms ± 11.9 | −8.4% |
| word frequency, round 3 | 135.3 ms ± 4.1 | 124.7 ms ± 5.4 | −7.9% |
| `fib(35)`, three rounds | 47.3–48.3 ms | 47.8–48.2 ms | −1.2%, +1.0%, +1.9%: noise (no count on its path) |

The output is `words.expected`'s; peak memory 19.3 MiB either way; `ldadd`/`stadd` in the word
frequency executable 105 → 23 (the counters and the pool's). No runtime entry's signature, no
multi-thread runtime text and no runtime digest (taken over `EVERYTHING`, which is not `single`)
changes, so `RUNTIME_ABI_REVISION` stays 34, as it did when the failure state became plain globals
(R1, 5b.8): a prebuilt runtime artifact is never `single`, and a program linked to one keeps atomic
counts in its units too.

**What R3 must follow in `toolchain/pipeline`.** Its `rtgen.nz` emits the runtime unit from
`rtext.nz`: when `single` is among the needs (`Needs::by_names`), apply the same rewrite to the
emitted runtime after everything is pushed — only inside the seven functions above (by their
`define` line, up to the closing `}`), the two atomic forms on `i64 1` with `monotonic` or `release`
into the three plain lines named `%r`, `%r.plain`, and the `  fence acquire` lines there removed;
every other line byte for byte. Regenerating `rtext.nz` brings no new constant (the runtime
constants are unchanged; `PLAIN_IN_ONE_THREAD` can be carried as a list beside
`rt_single_thread_state` if the generator prefers data to a literal). The pipeline prints no hosted
LLVM of its own, so there is no inlined copy to follow; if it ever ports
`inline_reference_counting`, the copies take the same rewrite under `single`. R9's code generator
calls the runtime and needs no change.

### Multicore: the parallel workload and the scheduler (2026-10-15)

`bench/compare/` gained its first parallel program, `primes` (Nazm, C with pthreads, Go, Rust with
`std::thread::scope`): primes below 3,000,000 by trial division, printing 216816, in blocks of
1,000 dealt round robin to W workers, run by `compare.py` at W = 1, 2, 4, 8 and the host's processor
count with each build's speed-up over its own W = 1 and the load average recorded. Blocks, not
n ≡ w mod W: with W even that deals every odd number to the odd workers and idles half of them in
every language.

**Why a first run used two processors at W = 8.** The draft's interleaving idled four of eight
tasks, and the pool started four workers on every machine (`nz.pool_start`'s default; §7.84 had
written the processors online, the capability matrix four). Placement is round robin and pinned, so
the four busy tasks shared two workers. Not causes: `spawn` queues and returns (it never runs or
waits for the task), `main` is not a task, and a channel or join wait parks its task and frees the
worker.

**The change** (runtime ABI 34 → 35; `docs/architecture.md` §7.107, *Amended*): the default worker
count is `nz.os_cpus()` — a function of each target's `stack` unit, `sysconf(_SC_NPROCESSORS_ONLN)`
(58 Darwin, 84 glibc/musl, 0x61 Bionic) or `GetSystemInfo` on Windows — at most 64, four where the
system will not say; `NAZM_WORKERS` overrides it as before and `NAZM_WORKERS=1` stays the
deterministic mode. The dotted name is no C identifier, so nothing is reserved and no epoch moves.
Tests: `scheduler_default` asks for one worker per processor (`available_parallelism`); mutants
`pl-the-pool-keeps-four-workers` and `pl-darwin-asks-for-the-wrong-processor-count` (killed by
`ten_thousand_tasks_run_on_one_worker_per_processor_by_default`), `n83-the-worker-count-is-ignored`
and the revision pin re-run, all killed.

**Measured** (`docs/performance.md`, *Multicore: …*; a shared host, load 17–81 on ten processors,
relative scaling only):

| W | C | Rust | Go | Nazm | Nazm, the old four workers |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 2 | 1.74× | 1.63× | 1.93× | 1.94× | 1.84× |
| 4 | 3.45× | 3.24× | 3.10× | 3.19× | 2.90× |
| 8 | 4.09× | 4.38× | 3.67× | 4.37× | 3.47× |
| 10 | 5.51× | 5.04× | 4.73× | 4.89× | 3.27× |

Nazm now scales with the three; one worker is C's speed (173 against 177 ms in `compare.py`'s run).
What remains is the pool's design, not a defect: with more tasks than workers a pinned task cannot
move to an idle worker (no stealing), and placement ignores how busy a worker is. Both are §7.84's
stated non-goals; a workload of uneven tasks would show them, and a measured case for stealing is
the next multicore question, not this change's.

**What R3 must follow in `toolchain/pipeline`.** Regenerate `rtext.nz` from the runtime crate
(`toolchain/pipeline/tests/runtime_text.py`): `rt_abi_revision` 34 → 35; the pool text's
`declare i64 @nz.os_cpus()` and, at the top of `nz.pool_start`, a two-line comment and `%cpus`, `%known`,
`%dflt`, with `nz.pool_env` given `%dflt` for 4; and each target's stack unit's new
`nz.os_cpus` — for Darwin, Linux and Android a `declare i64 @sysconf(i32)` and the function after
`nz_os_stack_size` with the target's constant, for Windows the function after `nz_os_page_size`.
No other runtime text, signature or schema moves; R9's code generator calls the runtime and needs
nothing.

### Concurrency engineering: the runtime half of 6.2–6.4 (2026-10-16)

The copyright holder chose (2026-10-16) to do the runtime half of stage P now; compiler passes stay
after the cut-over. What 6.1 measured (`docs/performance.md`, *Concurrency*) names the causes: every
`spawn` maps and unmaps a 256 KiB guarded stack (system time dominates `spawn`, and a blocked task
keeps a whole 16 KiB page resident on this Mac), every wait and wake goes through one process-wide
waiter lock, a wake broadcasts, and a worker sleeps in the kernel the moment its queue is empty.
The guarantees stay: data races refused (`N0321`), no task outliving its scope (`N0320`),
authority only as passed, `N0408` for recursion past a task's stack, failures and stack limits per
task, and `NAZM_WORKERS=1` deterministic.

**6.2 — tasks share their worker's stack; a parked task's frames are copied out.** Considered:
(a) smaller `mmap`'d stacks, or pooled ones — a guarded stack can never cost less than one page
resident, 16 KiB on Apple silicon, which is today's 17.4 KiB; (b) unguarded stacks packed in the
heap — unsafe, since the stack check budgets Nazm frames only and the runtime's C calls (formatting,
the resolver, TLS) below the limit would run into a neighbour; (c) segmented stacks — LLVM's
`split-stack` has no `__morestack` on macOS or Windows, and a split in a C call is a crash;
(d) Go's copying growth — it moves frames, which needs a precise map of every pointer into the
stack, which Nazm's code generators do not keep. **Chosen: (e), copied stacks at a fixed address.**
Each worker owns one guarded run stack of the task-stack size (`NAZM_TASK_STACK`, `--stack-size`,
256 KiB by default). A task runs on its worker's run stack; when a *different* task is to run there,
the frames of the one in place (from its saved stack pointer to the top) are copied into a heap
buffer the task owns, and copied back to the same addresses before it next runs. Same addresses, so
nothing moves: frame pointers, the unwinder's tables, stack probes, LLVM's spilled addresses and the
saved registers all stay valid, and the stack check is unchanged — one load and two comparisons, the
limit the run stack's. A task already pins to its worker (§7.84: a compiler may keep a thread-local's
address across a call), so a shared stack costs no mobility a task had. The copy is lazy: a task
resumed while its frames are still in place (one task, or two alternating on two workers) copies
nothing. Sound because nothing outside a task reads its stack while it is parked: a task's arguments
are a heap block (§7.84, N0321 refuses sharing), a waiter is registered by its heap record, and the
runtime's waits keep only a condition variable's address as a key. A blocked task then costs its
record and its frames' bytes; `spawn` costs a record, no system call. Not chosen for this: a pool of
stacks per task (it fixes the start-up cost, not the memory).

**6.3 — per-worker queues, stealing, spinning, and preemption at calls.** Each worker keeps two
queues: tasks never started (which have no frames, so any worker may take them) and tasks resumed
(pinned). A task spawned by a task goes to its own worker's queue — the spawner usually waits on it
next — and one spawned by `main` round robin as today. A worker with nothing to run first steals
half the unstarted tasks of the first other worker that has any, then spins for a short while
(about 50 µs) before sleeping on its condition variable; a waker signals a worker only when it
sleeps. Locks stay per worker (a deque under its own mutex; Chase–Lev lock-free deques are not
needed while a steal is rare and the owner's own lock is uncontended). **Preemption at loop back
edges** (the copyright holder, 2026-10-16: part of 0.11.0): first designed at function entry by
poisoning the stack limit, which would have cost nothing on the fast path but missed every loop
without a call — the case that starves. Built instead: every loop's back edge (found when MIR is
lowered: an edge to a block on the depth-first path) polls one process-wide flag, inlined by LLVM's
lowering as a load and a branch past a call; a monitor thread every 10 ms sets the flag and a
worker's request when that worker has run one task since the last look while others wait, and the
task yields at its next back edge. Not the stack limit: it is thread-local (a call per read on
macOS where LLVM cannot hoist it) and it is set in every task, where a plain global is read with
one instruction. Off with one worker (deterministic mode). Its cost on hot loops (`fib`, `words`,
`primes`, a tight loop with no call) is not measured yet: no quiet host came while it was built;
`bench/compare/preempt_cost.sh BASE NEW OUT --wait` times it in the coordinator's quiet window, and
more than 1-2% reopens the design.

**6.4 — channels.** The `Int` channel's text is the thread model's, line for line in
`compiler/emit.nz` (`runtime parity`); its waits and wakes are already the pool's hooks, so the
hooks are made cheap first: the waiter table's one lock becomes a lock per bucket, and a wake with
nobody parked touches no lock. A direct hand-off to a parked receiver changes the channel text and
its mirror; it is the next step, with `select` over channels of different element types (a language
change: both checkers, the interpreter, the pipeline's built-ins, the semantic epoch).

**Versions.** Runtime ABI 39 → 40 (copied stacks), 41 (scheduler), 42 (waiter table), 43
(preemption). Each bump regenerates `toolchain/pipeline/rtext.nz` and `runtime.nz` with
`rt_abi_revision` in the same commit; preemption's LIR poll is mirrored in
`toolchain/pipeline/ops.nz` (`loop_latches`), so the pipeline's LIR stays byte-identical. No
semantic epoch moves for 6.2–6.3.

**Not yet built: `select` over channels of different element types, and a direct hand-off.**
The shape decided: built-ins rather than syntax, so no grammar moves — `sel_new() -> Select`;
`sel_recv[T](s: Select, c: Chan[T], out: Vec[T]) -> Int` and `sel_send[T](s: Select, c: Chan[T],
v: T) -> Int`, each adding a case and returning its index; `sel_wait(s: Select) -> Int` and
`sel_wait_until(s: Select, ms: Int) -> Int` (a `TimeCap`), the lowest ready index, `-3 - i` for a
closed case, `-2` timed out. The runtime keeps a case array of channel, stride, kind and the
address of a slot the generated code owns, and parks once on every case's waiter bucket; the value
moves into `out` (or out of `v`) in generated code after the wait returns the index, so each case
keeps its own type. It needs: a `Select` type and five intrinsics in the Rust checker and
`compiler/`'s checker (R2's `toolchain/frontend-tests` at 0 differ), MIR and LIR lowering and its
mirror in `toolchain/pipeline/ops.nz`, the Rust interpreter and `toolchain/interp`, both code
generators, `docs/spec.md` with its 1.0 classification row, `library/std/API-1.0` if `@std`
wraps it, and the semantic epoch (90 → 91: the identity doc line and `SEMANTIC_EPOCH`, the
`stability.md` row, two pins in `incremental.rs`, `n38-the-epoch-stays`). The direct hand-off to
a parked receiver changes the `Int` channel's text and its mirror in `compiler/emit.nz`
(`runtime parity`).
