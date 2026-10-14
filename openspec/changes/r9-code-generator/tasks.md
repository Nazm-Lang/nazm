## 1. The proposal

- [x] 1.1 Proposal, design and the capability's requirements (this change)

## 2. Integers, control flow, calls — `aarch64-apple-darwin`, Mach-O

- [ ] 2.1 The LIR text reader: symbols, data, functions, declarations, instructions, terminators
- [ ] 2.2 The aarch64 encoder: moves, integer arithmetic and comparisons, loads and stores, branches,
      calls and returns
- [ ] 2.3 Frame-slot selection with an AAPCS64 frame (D3 stage A, D4, D5)
- [ ] 2.4 The Mach-O object writer: sections, symbols, relocations (D6)
- [ ] 2.5 `tests/run.sh`: `bench/compare/nazm/fib.nz` agrees with LLVM, `fib(35)` included

## 3. The failure bit and the runtime

- [ ] 3.1 Nazm functions return their failure bit beside the result; `fail`; `failed` results
- [ ] 3.2 Calls into the runtime and C: narrow arguments, `Bit` results, more than eight arguments
- [ ] 3.3 Data with addresses in it; imported data through the GOT; `unreachable`

## 4. Strings and sequences

- [ ] 4.1 Slots, `copy`, `zero`, `switch`, `select`, indirect calls, every integer width
- [ ] 4.2 Floats and conversions; `I128`
- [ ] 4.3 `tests/compat/v1/run/` and `corpus/` in the suite, every difference fixed or reported
- [ ] 4.4 Stage B: the linear-scan allocator; the performance report against LLVM

## 5. Line tables

- [ ] 5.1 DWARF `.debug_line` from `loc`, a compile unit and subprograms; compared with LLVM's rows
- [ ] 5.2 Unwind information (compact unwind) for backtraces

## 6. ELF and x86_64

- [ ] 6.1 The ELF64 relocatable writer, aarch64
- [ ] 6.2 The x86-64 encoder and System V convention; Mach-O and ELF
