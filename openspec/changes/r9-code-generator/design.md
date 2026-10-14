## Context

LIR (`docs/architecture.md` §7.106) is the level both native backends consume: every guard,
failure text, offset, runtime entry, retain and release is decided before a backend sees a module.
A backend decides instruction selection, registers, the object format and how a position becomes
debug information — nothing a program can observe. `crates/nazm-codegen-clif` is the existing
translator (835 lines of translation over Cranelift); this change writes the same responsibility
in Nazm, with no Cranelift underneath: the encoder, the frame, the allocator and the object writer
are its own.

Input today is the printed module (`nazm lir --ops FILE`, one `; unit` section per unit, in the
order of `nazm lir`'s `units` and of `nazm build --objects`'s `NN-mK.o` files). The comparison is
the LLVM build of the same program, run.

## Goals / Non-Goals

**Goals:** every LIR instruction, terminator and data form on `aarch64-apple-darwin`, then ELF and
`x86_64`; objects that link with LLVM-built runtime, entry and unit objects (the same calling
convention, so a program may mix them); identical observable behaviour to the LLVM build on every
input in the suite; deterministic output; DWARF line tables.

**Non-goals:** optimisation beyond register allocation (LIR has none and LLVM's `-O2` is the fast
path); the GPU and contract-bytecode generators; `wasm32` (its own format, after x86_64);
replacing the runtime's or the entry's objects, which stay the runtime's own (§7.83).

## Decisions

**D1. Input is LIR's printed text.** The reader parses `nazm lir --ops` output: symbols, data,
functions, `var`/`slot`/`at`/`debug` declarations, blocks, instructions and terminators, exactly the
grammar of `crates/nazm-lir/src/op/print.rs`. The printed text is the stable, reviewed form, and its
digest is already what an object is keyed by. The reader builds an in-memory module the rest of the
generator uses; at integration R3 hands that module over directly and the reader stays for tests
and for `nazm lir` round trips. A line the reader does not recognise is refused, never skipped.

**D2. Stages.** reader → selection (LIR to machine instructions, per target) → encoder (bytes and
relocations, per target) → object writer (per format). One generator program; the target and the
format are records chosen once, so x86_64 and ELF add modules rather than branches throughout.

**D3. Registers: frame slots first, a linear scan second.** Stage A gives every value, variable and
parameter its own 8-byte frame slot (16 for `I128`) and every LIR slot its own aligned area; each
instruction loads its operands into scratch registers (x9–x15 on aarch64), computes and stores its
result. It cannot be wrong about liveness, so correctness is settled before speed is attempted.
Stage B (after the strings and sequences slice) replaces it with a linear scan over live intervals
in block order — values in callee-saved registers where they cross a call, scratch registers
otherwise, spilled to the same slots under pressure — and the suite gains a performance report
against LLVM `-O0` and `-O2`. Performance is reported, not gated, until stage B.

**D4. Calling convention: the platform's, with LLVM's failure bit.** `plain` and Nazm functions use
AAPCS64 as Apple specifies it (stack arguments at their natural alignment, the caller extending a
narrow argument), C functions the same with a `Bit` zero-extended (N42). A Nazm function returns
its result in x0 (d0/s0 for a float, x0:x1 for an `I128`) and its failure bit in the next integer
register — x1, or x0 where there is no integer result, or x2 after an `I128` — which is what LLVM
does with `{T, i1}` and Cranelift with its two returns. Objects of the two backends therefore call
each other, and a program may be linked from both.

**D5. Narrow values are kept zero-extended.** A `bit`, `i8`, `i16` or `i32` in a register or frame
slot always has zero upper bits; a parameter or call result of one is normalised on arrival (the
caller of an internal function promises nothing above its width). An operation that reads a narrow
value as signed — signed division, remainder, right shift, comparison, overflow check, `sext` —
sign-extends it first. Checked arithmetic on a narrow class is computed at 64 bits and is an
overflow when the result does not survive truncation to the class.

**D6. Mach-O first.** One `LC_SEGMENT_64` with `__TEXT,__text`, `__TEXT,__const` (read-only data),
`__DATA,__const` (data holding addresses) and `__DATA,__data` (writable data); `LC_BUILD_VERSION`,
`LC_SYMTAB`, `LC_DYSYMTAB`. Symbols are `_`-prefixed; `local` is a non-external symbol, `hidden`
private-external, `export` external, `import` undefined. Relocations: `BRANCH26` for every call,
`PAGE21`/`PAGEOFF12` for a defined symbol's address, `GOT_LOAD_PAGE21`/`GOT_LOAD_PAGEOFF12` for an
imported one's, `UNSIGNED` for an address written in data. Branches inside a function are resolved
by the generator. No compact-unwind or `__eh_frame` at first: nothing unwinds through Nazm frames
(a failure returns its bit; an abort does not unwind), so a debugger's backtrace through the frame
pointer chain is what is lost, and slice 5 restores it. `section` placement (Gate 3 boards) is
refused until the ELF slice.

**D7. Deterministic.** The same LIR gives the same bytes: no timestamps, no paths, symbols in a
fixed order (locals in definition order; externals and imports sorted by name).

**D8. The differential suite is the contract.** `tests/run.sh`, per input: `nazm build FILE`
(LLVM), run; `nazm build --objects DIR FILE`, the unit objects replaced by this generator's, linked
in `link.txt`'s order with its libraries by the system `cc`, run; standard output, standard error
and exit status compared byte for byte. Inputs: `bench/compare/nazm/fib.nz`, then every program of
`tests/compat/v1/run/`, then `corpus/`. A generator refusal is *refused*, a build failure of the
reference is *skipped*, and neither is ever counted as agreeing (`docs/runbook.md`, *Status
vocabulary*). A per-instruction encoder check against the system assembler (`cc -c` of the same
instructions as text) is added with the encoder's first slice.

**D9. Refuse what is not carried.** Every LIR form the generator does not yet translate is refused
by name, on standard error with exit status 3 — never miscompiled, never silently dropped.

**D10. Line tables from `loc`.** Slice 5: `loc FILE LINE COL` becomes DWARF `.debug_line` rows (and
`.debug_info`/`.debug_abbrev` with one compile unit and a subprogram per function with `at`), in
the `__DWARF` segment on Mach-O and `.debug_*` sections on ELF; the file table from the unit's
source names. Compared with LLVM's by `dwarfdump --debug-line` row sets, not bytes.

**D11. ELF and x86_64.** Slice 6: an ELF64 relocatable writer (`R_AARCH64_CALL26`,
`ADR_PREL_PG_HI21`/`ADD_ABS_LO12_NC`, `ADR_GOT_PAGE`/`LD64_GOT_LO12_NC`, `ABS64`; `R_X86_64_PLT32`,
`PC32`, `GOTPCREL`, `64`), and an x86-64 encoder with System V's convention (failure bit in the
next integer return register, `rdx` after `rax`), reusing the frame-slot selection. Run on Linux
hosts only where one is available without Docker; otherwise the objects are checked by
`llvm-objdump`/`readelf` round trips and recorded as unexecuted.

**Language.** The generator is written in the 0.10.0 language. `Int` arithmetic traps on overflow,
so encodings are built with shifts and masks (which never trap) and `wrapping_*`; bytes are written
through `@std/bytebuf`.

## Risks / Trade-offs

- **Silent miscompilation** is the risk that matters. Mitigation: D8's byte-for-byte comparison on
  every input, D9's refusals, an encoder check against the system assembler, and the frame-slot
  stage before any allocator.
- **Speed of stage A code** is far below LLVM's. Accepted until stage B; reported, not hidden.
- **Apple's arm64 ABI deviations** (stack argument packing, narrow-argument extension) bite only
  with more than eight arguments or narrow C arguments: the suite carries programs that do both.
- **No unwind information** until slice 5: a crash's backtrace stops at a Nazm frame in tools that
  use unwind tables rather than the frame pointer.

## Open Questions

- Whether the generator becomes `nazm build --backend nazm` before R8, beside LLVM and Cranelift,
  or only at cut-over (the coordinator's call at integration).
- Stage B's performance target relative to LLVM `-O0`, set when stage A's numbers are measured.
