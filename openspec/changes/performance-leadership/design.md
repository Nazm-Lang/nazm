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
