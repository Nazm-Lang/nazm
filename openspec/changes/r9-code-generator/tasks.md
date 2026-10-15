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
- [x] 4.5 Selection that reads and writes allocated registers in place of the scratch copies,
      and caller-saved registers for values that cross no call (`use_reg`/`def_reg` in the
      templates that read their operands unchanged — 64-bit arithmetic, comparisons, checked
      add/subtract, loads, stores, constants, addresses, variables, branches; a caller-saved pool
      no template touches — x8/x13–x15 with the `i128` temporaries moved to x3–x5, and
      rsi/rdi/r8/r9 — for names no call falls inside or ends at. `tests/perf.sh`, aarch64, on a
      loaded machine: fib 191 ms against LLVM -O0 235 and -O2 91; words 1210 against 1558 and
      385. x86_64 under Rosetta: fib 454 against 247, words 2023 against 2124. All 106 inputs
      still agree on both targets)

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
- [ ] 6.3 The ELF objects linked and run on a Linux host — **deferred until after 1.0**: the
      copyright holder narrowed every target to macOS before 1.0 (2026-10-15). The ELF writer and
      `tests/elf.sh` stay as built; no further work on ELF or Linux until then

## 7. On main after R1.5, 3.3 and failures by unwinding (macOS only: aarch64-apple-darwin, x86_64-apple-darwin under Rosetta)

- [x] 7.1 Re-established on main `a23a1fe9`, the reference the Rust `nazm` built from the same
      checkout (`NAZM_REF`, `CARGO_TARGET_DIR=~/.cache/nazm-target-track9`), the 0.10.0 release
      only building the generator. Hosted LLVM now unwinds on a failure while the generator keeps
      the bit (performance-leadership 4.2, answer 3), so a program may no longer mix the two
      backends' unit objects: the suites replaced only the modules' objects (`NN-mK.o`) and left
      LLVM's generic instantiations (`NN-gK.o`) in place, which had also meant generic code was
      never the generator's. Every unit object is now the generator's, and `tests/run.sh` checks
      that it wrote as many as `link.txt` lists. `tests/run.sh`: 106 agree, 0 differ, 0 refused on
      aarch64 and on x86_64; `tests/lines.sh` 38 agree on each; `tests/unwind.sh` agrees on each
      (7 frames); `tests/encode.sh` 113 + 89 agree
