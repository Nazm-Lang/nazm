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

