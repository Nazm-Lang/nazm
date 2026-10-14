## Why

`rewrite-in-nazm` task 4.1 puts the native pipeline — Core IR to MIR (ownership and cleanup made
explicit) and MIR to LIR (layouts, symbols, instructions) — in Nazm. Today it is ~6.5k lines of Rust
in `crates/nazm-mir`, `crates/nazm-lir` and the parts of `crates/nazm-core` that drive them. The
Rust toolchain prints every format this stage reads and writes (`nazm core-ir`, `nazm.core-ir/2`;
`nazm mir`, `nazm.mir/1`; `nazm lir`, `nazm.lir/2`), so the stage can be written now, in parallel
with R2, R4, R7 and R9, reading the Rust toolchain's Core IR and comparing its MIR and LIR with the
Rust toolchain's, byte for byte.

## What Changes

- **`toolchain/pipeline/`**, a Nazm program in the 0.10.0 language, built with the 0.10.0 `nazm`:
  `pipeline mir CORE.json` reads `nazm core-ir --json` output and prints `nazm.mir/1` exactly as
  `nazm mir` does — the same functions in the same order, the same locals, blocks, statements and
  terminators, and the same BLAKE3 executable digests. `pipeline lir` follows (slice 6).
- **The rules ported, not the Rust:** the one cleanup stack (owned locals, open scopes, loop
  markers), the temporaries in evaluation order with hazard snapshots, the field borrow, the failure
  edges through each open scope's join to one shared unwind block, the stack check for call cycles,
  unreachable-block removal and renumbering, the instance plan, and the MIR validator.
- **`toolchain/pipeline/tests/run.sh`** builds the program, runs it over every input that has Core
  IR — `tests/compat/v1/`, `corpus/`, `examples/`, `bench/compare/nazm/` (105 today) — compares with
  `nazm mir` (and, from slice 6, `nazm lir` and `nazm lir --ops`), and prints `N agree, M differ`.
- **A finding about the input format:** `nazm.core-ir/2` is not a complete input for MIR. It omits
  the type table (record and enum fields and their types), function and array types (printed `?`),
  a closure's captures and `closure` flag, `@export`/`@section`/`@interrupt`/`@task`/`@on_failure`,
  statics, and the trait impl table a `call_trait` resolves against. The design records how the
  Nazm stage works around each until the coordinator extends Core IR's printed form (requested in
  the hand-back as `nazm.core-ir/3`), and which inputs differ until then.

## Capabilities

### New Capabilities
- `native-pipeline-in-nazm`: the observable requirements of the stage — what it reads, what it
  writes, and how it is held to the Rust toolchain. Archived with `--skip-specs`; the normative text
  lands in `docs/architecture.md` §7.42/§7.82/§7.106 when the coordinator folds this change in.

### Modified Capabilities
None.

## Impact

New: `toolchain/pipeline/` and this change. Nothing else is touched by this track. Requested of the
coordinator (not made here): a `nazm.core-ir/3` printed form carrying what the design's *Gaps*
table lists, in `crates/nazm-cir/src/print.rs`; nothing the checker or lowering decides changes.
