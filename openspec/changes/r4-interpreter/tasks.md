## 1. Slice 1 — the proposal

- [x] 1.1 Values, environments, failure and `defer`, built-in dispatch, concurrency: `design.md`
- [x] 1.2 The requests to the coordinator recorded (`design.md`, *Requests*)

## 2. Slice 2 — integers, booleans, control flow, calls

- [x] 2.1 Load: `@std/json` to the function, constant and code tables
- [x] 2.2 The execution loop: frames, slots, operands, jumps, releases
- [x] 2.3 `Int` arithmetic and comparison with Rust's failures; `Bool`; `if`, `while`, `break`,
      `continue`, `return`, blocks, calls; the iteration and depth budgets
- [x] 2.4 `print`/`eprint`/`exit_with`, `int_to_str`, `str_concat`, `num_to_str` of `Int` — enough to
      print; `main`'s result and `NAZM_RESULT_REPORT`
- [x] 2.5 The diagnostic renderer (text)
- [x] 2.6 `tests/run.sh` and the first comparison: `fib`, the integer and control-flow cases

## 3. Slice 3 — strings and sequences, the memory report

- [x] 3.1 `Str` built-ins with backings and counting; `str_append` in place
- [x] 3.2 `Vec`/`Ints`/`Strs`/`Box`/`List`/`Cell`, reference counting, reclamation by worklist
- [x] 3.3 `NAZM_MEMORY_REPORT`; the comparison over the memory cases

## 4. Slice 4 — records, enums, `match`, generics, closures

- [x] 4.1 Records, enums, arrays, field paths; `match` and `?`
- [x] 4.2 Equality and derived order; `call_trait` dispatch (D8)
- [x] 4.3 Function values, closures, indirect calls
- [x] 4.4 Numbers of every width and floats (D7); bytes and text

## 5. Slice 5 — concurrency, files, processes, time

- [x] 5.1 The task scheduler; `scope`, `spawn`, channels, select; `NAZM_SCHED_REPORT`
- [ ] 5.2 Files, directories, processes, environment, time, network, atomics
- [ ] 5.3 `--json` diagnostics; the whole suite agrees, every difference reported
