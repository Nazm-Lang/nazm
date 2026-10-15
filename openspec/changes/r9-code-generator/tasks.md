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
- [x] 7.2 The program set grown toward the whole LIR (D12): `nzcg --forms`/`--universe` and
      `tests/cover.sh`; `tests/programs/` gains `conversions.nz` (every integer width to and from
      both floats, widening and narrowing), `narrow.nz` (every comparison, wrapping operation,
      rotation, bit count and `match` per width; `i128` rotations, bit counts and byte swap at
      their edges; results of each class directly and through function values), `values.nz`
      (a writable atomic static, `Float32` fields in memory, indirect calls with stack arguments
      and an address result) and `ffi.nz` with `ffi.c` (calls into C with every FFI-safe class,
      more than eight arguments, a C callback into an export, and C calling exports that take
      and return every number; `run.sh` links a program's `FILE.c` beside it). New in the
      generator: `i128` `rotl`/`rotr`, `popcnt`, `clz`, `ctz` and `bswap` on both targets (they
      were refused). Coverage of the 485-form universe: 273 before, 393 after (aarch64);
      `tests/run.sh` 110 agree, 0 differ, 0 refused on each target (393 of 485 forms on each)
- [x] 7.3 The x86_64 slowness under Rosetta, found ours and fixed. Two causes: (1) every
      function called `nz.stack_check` where LLVM inlines it — under Rosetta a call and return
      cost far more than natively (fib with the call removed by hand: 209 to 139 ms); (2) the
      overflow bit of a 64-bit multiply read with `seto` costs Rosetta several times a `jo` on
      it (`i % 7 * 3` in a loop: 378 against LLVM's 131 ms). Now: `nzcg --one-thread` (the
      harness passes it when the reference's runtime defines `nz.stack_limit` as a plain
      global, which is `native.single_thread`) inlines the check as LLVM does, both targets; on
      x86_64 a comparison or checked operation whose result only feeds its block's branch is
      left in the flags (`jcc`, no `setcc`/`test`), and a multiply's overflow bit that must be
      a value is set by a branch, not `seto`. `tests/perf.sh`, best of five, x86_64 under
      Rosetta, LLVM -O0 / -O2 / nzcg: fib 139 / 71 / 129 ms (was 140 / 71 / 199), words
      575 / 181 / 517 (585 / 184 / 571), arith 36 / 23 / 42 (37 / 22 / 70); aarch64: fib
      155 / 53 / 122, words 548 / 140 / 406, arith 24 / 15 / 25. What remains on x86_64 is
      template verbosity (the remainder's divisor guard: ~12 more instructions than LLVM -O0),
      not Rosetta. `tests/run.sh` 110 agree on each target; `lines.sh` 41 agree, 1 skipped
      (`ffi.nz`, whose C it does not link); `unwind.sh` agrees; `encode.sh` 113 + 89
- [x] 7.4 The integration proposed (design D15): `nazm build --backend nazm` as a third
      `Backend` that runs the shipped `nzcg` per unit on the printed LIR, keeping the bit and
      passing `--one-thread`; refusals as `N0101`, no fallback. The Rust change is the
      coordinator's; not made here
- [x] 7.5 Unwind tables and LSDAs, so the generator unwinds on a failure as hosted LLVM does
      (D14, as built there): on both Mach-O targets `fail` calls the unit's own local `nz.raise`
      (a forced unwind under a stop function), a call with work on its failure edge lands on that
      edge's block, a call into a bare `fail` has no pad, and a plain entry's call lands on a pad
      that zeroes its result; each function with a pad has its compact unwind entry name
      `__gcc_personality_v0` and its LSDA in `__gcc_except_tab`; ELF keeps the bit (D13). Frames
      now share slots between names whose lives do not overlap (720 → 176 bytes for a recursive
      `down` holding a string; LLVM -O0: 224), so a failure as deep as LLVM's fits. `tests/run.sh`
      now runs every program with the memory report on, gains seven failure-convention programs
      (`fail_*`: recursion, a function value, a trait method through a generic instance, a task,
      ten thousand frames of `defer`s, an exhausted stack — its allocation counts depth-masked,
      `live` exact — and a callback C calls) and `NZCG_MIX=even|odd` (a program linked from both
      backends' unit objects): 117 agree, 0 differ on aarch64 and on x86_64; mixed, 117 agree on
      each target and parity (19 programs with units of both backends). `tests/unwind.sh` gains a
      failure under fifty frames that own strings, walked and unwound as LLVM's: agree on both,
      61 frames, the image's table naming one personality and its LSDAs. `lines.sh` 47 agree,
      2 skipped (the programs with C); `encode.sh` 113 + 89
- [ ] 7.6 aarch64 branch fusion, and the x86_64 remainder's divisor guard in fewer instructions
