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
- [x] 5.2 Files, directories, processes, environment, time, network, atomics
- [x] 5.3 `--json` diagnostics; the whole suite agrees, every difference reported

## 6. After the rebase onto R1.5 D6–D8 and Core IR /3

- [x] 6.1 Read `nazm.core-ir/3` (and `/2`); `chain_*` built-ins (D7), `list_*` kept for a /2 input
- [x] 6.2 `tests/run.sh`: the reference's `run --interpret` when it has one; `NAZM_BUILD` apart from
      `NAZM`; files the programs write removed after the run
- [x] 6.3 Sockets on one thread (D10): kvd agrees
- [x] 6.4 The `/3` tables (`impls`, `orders`, `derived_hash`, `statics`) replace D8's reconstruction
- [x] 6.5 `--rust-tests`: the programs the Rust CLI tests embed (raw and plain string literals),
      every difference reported
- [x] 6.6 What `nazm run` refuses before the program starts (D11): calls into C, device registers,
      board-only placements
- [x] 6.7 The clocks from the program's first reading; a child's pipes polled as sockets are; a
      waiting socket tried only after a round in which no other task progressed

## 7. Following main to e7effe0e (epoch 88, @std/http, Windows hosts, R1.5 4.5/4.6, D15)

- [x] 7.1 The reference rebuilt from this checkout; both suites re-run: nothing main's changes
      broke (the pattern-let refusal is the checker's, before the Core IR; the @std/http programs
      agree)
- [x] 7.2 Coverage: the programs that do not check sorted (design.md, *The Rust tests' programs*);
      programs of several modules and `\xNN` escapes carried by `tests/rust-tests.pl`
- [x] 7.3 The networked allocation counts: a connection made or bytes written give way (D10); the
      three networked programs of `http.rs` are cases; the compiled tiers' `atomic_new` counted as
      a string reported (design.md, *Allocation counts in networked programs*)
