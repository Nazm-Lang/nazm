## 1. The proposal

- [x] 1.1 Proposal, design and the capability's requirements (this change)

## 2. Integers, control flow, calls — `aarch64-apple-darwin`, Mach-O

- [x] 2.1 The LIR text reader: symbols, data, functions, declarations, instructions, terminators
- [x] 2.2 The aarch64 encoder: moves, integer arithmetic and comparisons, loads and stores, branches,
      calls and returns
- [x] 2.3 Frame-slot selection with an AAPCS64 frame (D3 stage A, D4, D5)
- [x] 2.4 The Mach-O object writer: sections, symbols, relocations (D6)
- [x] 2.5 `tests/run.sh`: `bench/compare/nazm/fib.nz` agrees with LLVM, `fib(35)` included

## 3. The failure bit and the runtime

- [x] 3.1 Nazm functions return their failure bit beside the result; `fail`; `failed` results
- [x] 3.2 Calls into the runtime and C: narrow arguments, `Bit` results, more than eight arguments
- [x] 3.3 Data with addresses in it; imported data through the GOT; `unreachable`

## 4. Strings and sequences

- [x] 4.1 Slots, `copy`, `zero`, `switch`, `select`, indirect calls, every integer width
- [x] 4.2 Floats and conversions; `I128`
- [x] 4.3 `tests/compat/v1/run/` and `corpus/` in the suite, every difference fixed or reported
      (`corpus/` holds no program yet; the suite also runs `tests/compat/v1/trap/`, `bench/`,
      `examples/`, `crates/nazm-cli/tests/transfers/` and the generator's own
      `tests/programs/`: 106 agree, 0 differ, 0 refused)
- [x] 4.4 Stage B: the linear-scan allocator; the performance report against LLVM
      (`common.nz` `allocate`: intervals from block liveness, callee-saved registers only —
      x19–x28, rbx/r12–r15 — so no call clobbers an allocated value. `tests/perf.sh`, aarch64,
      best of five: fib 163 ms against LLVM -O0 164 and -O2 72; words over 2,000,000 words
      816 against 725 and 201; bench/programs within 0.9–1.7× of -O0; every output agrees)
- [ ] 4.5 Selection that reads and writes allocated registers in place of the scratch copies,
      and caller-saved registers for values that cross no call

## 5. Line tables

- [x] 5.1 DWARF `.debug_line` from `loc`, a compile unit and subprograms; compared with LLVM's rows
      (`nazm lir --ops --debug`; `tests/lines.sh`: 38 agree on each Mach-O target; ELF verified
      by `tests/elf.sh`)
- [x] 5.2 Unwind information: compact unwind (Mach-O), `.eh_frame` (ELF); `tests/unwind.sh`
      agrees with LLVM on both Mach-O targets

## 6. ELF and x86_64

- [x] 6.1 The ELF64 relocatable writer, aarch64 and x86_64 (`tests/elf.sh`: 38 of 38 inputs
      structurally sound on each Linux target against LLVM's objects — *unexecuted*: this host has
      no Linux linker or kernel, and Docker is not used by this track)
- [x] 6.2 The x86-64 encoder and System V convention; Mach-O and ELF
      (`NZCG_TARGET=x86_64-apple-darwin tests/run.sh` runs under Rosetta: 106 agree, 0 differ,
      0 refused)
- [ ] 6.3 The ELF objects linked and run on a Linux host (the coordinator's, or CI's)
