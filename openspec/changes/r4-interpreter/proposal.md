## Why

`rewrite-in-nazm` (task 5.1, decision 2) rewrites the interpreter in Nazm. `nazm run --interpret`
(today plain `nazm run`) evaluates a checked program's Core IR with a tree-walking interpreter in
Rust (`crates/nazm-core/src/eval/`), and that interpreter is the **semantic oracle**: every
equivalence test holds the native backends to it. The Nazm interpreter therefore has to agree with
it exactly — output, exit status, the code, message and help of every run-time failure, the
iteration and depth budgets, and the memory and scheduler reports — before it can replace it, and
it must keep `nazm run` free of clang.

## What Changes

- **A new program, `toolchain/interp/`**, written in the 0.10.0 language and built with one prebuilt
  `nazm`. It reads a checked program's Core IR as `nazm core-ir --json` prints it
  (`nazm.core-ir/2`), with the program's source files for diagnostics, and runs it.
- **Load once, then execute a flat code.** The Core IR tree is read with `@std/json` and lowered
  once into a flat instruction array with explicit jumps, slots and releases. Execution is one
  loop over that array with an explicit frame and operand stack — no host recursion per Nazm call
  — so a task can be suspended at any blocking operation, the depth budget is a frame count, and
  deep Nazm recursion cannot exhaust the interpreter's own stack.
- **Values and an explicit heap.** Scalars and strings are Nazm values; records, enums and arrays
  are immutable aggregates; every handle (`Vec`/`Ints`/`Strs`, `Box`, `List`, `Cell`, `Bytes`,
  `Chan`, function values, `Atomic`) is an index into the interpreter's heap, reference-counted
  explicitly, so the four memory-report classes are counted at the moments the Rust interpreter
  counts them and `str_append` grows in place exactly when the Rust one does.
- **Failures and `defer` as control transfers.** A run-time failure carries the Rust diagnostic's
  code, message, help and span, unwinds every frame releasing what it owns, and is rendered by the
  same rules as `nazm_diag::Diagnostic::render` (text) and `nazm.diagnostic/1` (JSON).
- **Built-ins dispatched by number.** Each `builtin` name is resolved to a number at load; one
  dispatcher per family (strings, sequences, numbers, bytes and text, channels, time, the OS)
  implements the Rust interpreter's semantics and sentences.
- **A comparison suite, `toolchain/interp/tests/run.sh`**: builds the interpreter, runs it over
  `tests/compat/v1/run/`, `tests/compat/v1/trap/`, `examples/`, `corpus/` and its own cases, and
  compares stdout, stderr and exit status with `$NAZM run` (with `NAZM_RESULT_REPORT` and
  `NAZM_MEMORY_REPORT` where the Rust tests use them), printing `N agree, M differ`.

## Capabilities

### New Capabilities
- `nazm-interpreter`: the observable contract of the interpreter written in Nazm — agreement with
  the Rust interpreter on every input the suite runs. Archived with `--skip-specs`; the normative
  text lands in `docs/internal/architecture.md` when R4 closes.

### Modified Capabilities
None.

## Impact

New: `toolchain/interp/` only. Requested of the coordinator, not changed here (`design.md`,
*Requests*): the printed Core IR should carry the impl table, the generic templates, the canonical
orders, the derived-hash trait and the statics, which the in-memory `cir::Program` has and the
printed form leaves out. Until it does, the interpreter reconstructs what it can from the printed
form and reports the rest by name. No language, library, Rust or document change.
