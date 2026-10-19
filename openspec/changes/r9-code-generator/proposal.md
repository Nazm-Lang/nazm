## Why

`rewrite-in-nazm` decision 3: Cranelift is not ported. Nazm gets its own code generator, written in
Nazm, before publication — LIR to machine code and relocatable objects, Mach-O and ELF on aarch64
and x86_64, its own line tables, and a differential suite against LLVM (task 9.1). Until it exists
the toolchain's second native backend is third-party code (cranelift, gimli, object,
target-lexicon), and the Cranelift cases of the backend-differential tests have no replacement at
cut-over. It is the longest and riskiest stage of the rewrite, so it starts before its input stage
(R3) is written: it reads the LIR the Rust toolchain already prints.

## What Changes

- A new program, `toolchain/codegen/` (`nzcg`), written in the 0.10.0 language: it reads a
  program's units as `nazm lir --ops` prints them (`nazm.lir/2`'s `lir.text`) and writes one
  relocatable object per unit, which link with the runtime and entry objects
  `nazm build --objects` writes, in place of that command's own unit objects.
- Its parts: an LIR text reader; an instruction selector per target, first with every value in a
  frame slot and later a linear-scan allocator; an instruction encoder per target; an object writer
  per format; DWARF line tables from LIR's `loc` instructions.
- Targets, in order: `aarch64-apple-darwin` (Mach-O), then ELF and `x86_64` (slice 6).
- A differential suite, `toolchain/codegen/tests/run.sh`: every input is built by the Rust `nazm`
  with LLVM, and again with this generator's objects substituted; standard output, standard error
  and exit status must be identical. It prints `N agree, M differ, K refused`.
- What the generator does not carry yet it refuses by name — an exit status of 3 and the
  instruction — and the suite counts it as refused, never as agreeing.

Nothing a program means changes, and nothing outside `toolchain/codegen/` and this change is
edited: the generator is a consumer of LIR as it is.

## Capabilities

### New Capabilities
- `native-code-generator`: Nazm's own LIR-to-object code generator and its differential suite.

### Modified Capabilities
None.

## Impact

New: `toolchain/codegen/`, this change. At integration (by the coordinator): R3's LIR feeds it in
process; `nazm build --backend` gains it; the Cranelift column of `docs/user/support.md` and the
capability matrix's backend rows move to it; `docs/internal/architecture.md` gains its section. No epoch, ABI
or schema moves: the generator reads `nazm.lir/2` and follows the runtime ABI as LLVM's objects do.
