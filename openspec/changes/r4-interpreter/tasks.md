## 1. Slice 1 — the proposal

- [x] 1.1 Values, environments, failure and `defer`, built-in dispatch, concurrency: `design.md`
- [x] 1.2 The requests to the coordinator recorded (`design.md`, *Requests*)

## 2. Slice 2 — integers, booleans, control flow, calls

- [ ] 2.1 Load: `@std/json` to the function, constant and code tables
- [ ] 2.2 The execution loop: frames, slots, operands, jumps, releases
- [ ] 2.3 `Int` arithmetic and comparison with Rust's failures; `Bool`; `if`, `while`, `break`,
      `continue`, `return`, blocks, calls; the iteration and depth budgets
- [ ] 2.4 `print`/`eprint`/`exit_with`, `int_to_str`, `str_concat`, `num_to_str` of `Int` — enough to
      print; `main`'s result and `NAZM_RESULT_REPORT`
- [ ] 2.5 The diagnostic renderer (text)
- [ ] 2.6 `tests/run.sh` and the first comparison: `fib`, the integer and control-flow cases

## 3. Slice 3 — strings and sequences, the memory report

- [ ] 3.1 `Str` built-ins with backings and counting; `str_append` in place
- [ ] 3.2 `Vec`/`Ints`/`Strs`/`Box`/`List`/`Cell`, reference counting, reclamation by worklist
- [ ] 3.3 `NAZM_MEMORY_REPORT`; the comparison over the memory cases

## 4. Slice 4 — records, enums, `match`, generics, closures

- [ ] 4.1 Records, enums, arrays, field paths; `match` and `?`
- [ ] 4.2 Equality and derived order; `call_trait` dispatch (D8)
- [ ] 4.3 Function values, closures, indirect calls
- [ ] 4.4 Numbers of every width and floats (D7); bytes and text

## 5. Slice 5 — concurrency, files, processes, time

- [ ] 5.1 The task scheduler; `scope`, `spawn`, channels, select; `NAZM_SCHED_REPORT`
- [ ] 5.2 Files, directories, processes, environment, time, network, atomics
- [ ] 5.3 `--json` diagnostics; the whole suite agrees, every difference reported
