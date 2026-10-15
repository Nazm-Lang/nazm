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
