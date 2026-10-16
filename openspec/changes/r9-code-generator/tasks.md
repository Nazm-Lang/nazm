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
- [x] 7.7 D15 implemented (approved as written): `nazm build --backend nazm`, crate
      `nazm-codegen-nazm` (`BackendId::Nazm`); `nzcg` from `NAZM_NZCG` or `libexec/nazm/nzcg`
      beside `nazm`, its BLAKE3 digest in the backend's version and so in every key, with the
      one-thread fact; `--one-thread` from `Native::single_thread`; `--debug` hands over the
      positions and `; file` lines (`nazm lir --ops` now prints through the same function);
      exit 3 is one `N0101` per refused construct, nothing written, no fallback, any other
      failure `N0900`; a missing generator (exit 4) and a target other than the two macOS ones
      (exit 5) refused before compiling. `crates/nazm-cli/tests/nazm_backend.rs`: run.sh's 117
      inputs built with `--backend nazm` agree with LLVM (output, diagnostics, status, memory
      report; 42 s), and the rules above; a `nazm` column (macOS) in the differential harness
      and `failure_convention.rs` — every module using them passes. Docs: `architecture.md`
      §7.116 (and §7.115's bullet), the capability matrix (row 12a, EXPERIMENTAL), `--help`.
      Ten mutants (`r9-…`, seven for D15, three for 7.5), each applied by hand, killed by its
      killer, restored. Not done: installing `nzcg` as `libexec/nazm/nzcg` in the release
      build (`cargo xtask` packaging), and `nazm profile --backend nazm`
- [x] 7.6 aarch64 branch fusion, and the x86_64 remainder's divisor guard in fewer instructions.
      aarch64 now does what x86-64 has done since 7.3: a comparison (integer or float — each
      condition's inverse is exact, an unordered result included) or a checked operation's
      overflow bit that only feeds its block's branch stays in the flags, `cmp`/`fcmp`/`adds`
      then `b.cond`, no `cset`/`cbnz`. The guard LIR puts before `srem` (the minimum by -1 —
      `const`, `cmp eq`, `const`, `cmp eq`, `and`, `const`, `select`: 7 instructions, 24 machine
      instructions on x86-64) is recognised (`common.nz` `guarded_rem`: exactly that sequence,
      no value of it read elsewhere) and not selected: x86-64 divides with one `cmp $-1` and a
      branch over `cqo; idiv` (8 instructions), or at 64 bits for a narrower class, where `idiv`
      cannot trap; aarch64 needs no guard, `sdiv`/`msub` give 0 themselves. New program
      `tests/programs/guards.nz` (each width's minimum by -1, by 1, by itself; every comparison
      ending a block, NaNs among the floats; checked arithmetic in a loop) and a Rust test
      building the generator's programs for `x86_64-apple-darwin` under Rosetta
      (`nazm_backend::the_generators_own_programs_agree_with_llvm_on_x86_64_under_rosetta`).
      `run.sh` 118 agree, 0 differ on each target, mixed (`even`) 118; `lines.sh` 48 agree, 2
      skipped, on each; `unwind.sh` agrees on each; `encode.sh` 117 + 91. Measured on a loaded
      machine (load 17–53, two other tracks running), the generator's executables before and
      after interleaved, best of 21 (ms; LLVM -O0 beside): aarch64 arith 26.7 → 20.4 (24.7),
      words 405.7 → 397.3 (551.0), fib 127.0 → 126.8 (156.8), sieve 17.6 → 17.3 (17.4); x86_64
      under Rosetta arith 45.8 → 33.1 (39.1), fib 129.0 → 129.0 (139.9), words 525.9 → 527.6
      (574.5), sieve 26.5 → 27.9 (26.6) — the last two within this machine's noise. Mutants
      `r9-7-6-…` (three), applied by hand, killed, restored
- [x] 7.8 Packaging: `nzcg` installed as `libexec/nazm/nzcg` wherever a toolchain is assembled,
      on the two macOS targets — `scripts/host-archive.sh` (the archive, with a line in its
      `INSTALL.txt`) and `scripts/release-candidate.sh` (its `MANIFEST.txt` lists the file with
      its digest, `PROVENANCE.txt` names it among the binaries) — built from `toolchain/codegen/`
      by the `nazm` being packaged, `--no-cache` and outside the checkout (which may be
      read-only). `scripts/clean-install-test.sh` builds and runs with `--backend nazm` when the
      archive carries it. `nazm_backend::the_packaged_toolchain_finds_its_generator_beside_nazm_without_the_variable`
      packages the `nazm` under test, passes the clean install, builds with `NAZM_NZCG` unset, and
      with the generator moved away is refused (exit 4) naming `libexec/nazm/nzcg`. Mutant
      `r9-7-8-the-archive-carries-no-generator`, applied by hand, killed, restored
- [x] 7.9 `nazm profile --backend nazm` (it builds with the backend asked for; the counts and,
      with `--sample`, the samples attributed by the generator's line tables — `debugger.rs` and
      `debugger_v3.rs` take a profile under all three backends on macOS); the provenance record of
      a program the generator built names it, `backend.nzcg`, by its BLAKE3 digest, and the SBOM
      lists it among the tools (`nazm.provenance/1` gains the optional field and the backend name
      `nazm`, as R1b and R1d added theirs; `schemas.rs` checks such a record against it;
      `build_provenance::a_program_nazms_own_generator_built_names_it_by_its_digest`). Mutants
      `r9-7-9-the-record-does-not-name-the-generator` and
      `r9-7-9-profile-refuses-the-nazm-backend`, applied by hand, killed, restored
- [x] 7.10 Coverage re-measured (`tests/cover.sh`, after `run.sh` on main `dcdde864` plus 7.6):
      393 of 485 forms on each target, as at 7.2. Which of the 92 the lowering emits was
      established by printing the LIR of every one of the repository's 2,110 `.nz` files with a
      `main` and taking their forms, and by probes per construct: two — `addr import func` (a
      closure written in a generic function: its body is the module's unit, its instances
      reach it as an import) and `umul.ovf i8` (a checked `UInt8` multiplication). New program
      `tests/programs/emitted.nz` uses both, the overflow failing the program at the end:
      395 of 485 on each target; `run.sh` 119 agree, 0 differ on each. The other 90 are forms
      the validator admits and no program or probe produced, with the reason where it is known:
      C calls with a float, narrow or no
      result or argument, and every indirect C or runtime call (the language refuses them —
      N0381, N0387 — and the runtime declares none); `select` on floats or pointers (the
      lowering selects only integers); `i128` division, remainder, checked multiplication and
      float conversions (runtime calls); `switch` on narrow or `i128` values (matches are
      compare chains); extensions from and truncations to `bit` (`Bool` converts to no number,
      N0300); `cmp ne ptr`; writable data holding addresses (a static's cell holds none);
      runtime calls returning `i8`, `i16` or `i128` or with stack arguments; and `loc`, which
      only `--debug` prints (`tests/lines.sh` covers it)

## 8. Windows x86_64 (`x86_64-pc-windows-gnu`, before 1.0: R8 replaces Cranelift, which builds it; design D16)

- [x] 8.1 The PE/COFF relocatable writer, AMD64 (`coff.nz`): `.text`, `.data`, `.rdata`, `.xdata`,
      `.pdata`, `.drectve` (`-exclude-symbols:` per hidden definition, as LLVM writes) and the DWARF
      sections; section symbols with their auxiliary records, a string table for long names
      (`/N` section names), `NRELOC_OVFL` past 65,535 relocations; `REL32` (an import addressed
      directly: no GOT), `ADDR64`, `ADDR32NB` (`.pdata`), `SECREL` (DWARF offsets), addends in
      place. Objects MinGW-w64's `ld` links
- [x] 8.2 Windows x64's convention in the x86-64 selection: four positions (rcx, rdx, r8, r9 or
      xmm0–xmm3), then the stack above 32 bytes of shadow space; an `i128` two positions, split
      between r9 and the stack as LLVM splits it for this triple; results and the failure bit as
      LLVM returns `{T, i1}` here (rax, rax:rdx, xmm0; the bit in the next register); rsi and rdi
      pushed when the allocator uses them and xmm4/xmm5 as the scratch pair (xmm6–xmm15 are
      callee-saved); a frame of 4096 bytes or more allocated through `___chkstk_ms`
      (`mov eax, N; call; sub rsp, rax`), as LLVM probes for MinGW. The other targets' bytes are
      unchanged: the generator before and after writes identical objects for 156 inputs on
      aarch64-apple-darwin, x86_64-apple-darwin and x86_64-unknown-linux-gnu
- [x] 8.3 Unwind tables: an UNWIND_INFO in `.xdata` (version 1, the prologue's pushes and its
      allocation, small or large, no frame register — rsp is fixed after the prologue) and a
      RUNTIME_FUNCTION in `.pdata` for every function; the epilogue in the form Windows's unwinder
      recognises (`add rsp`, pops, `ret`). Failures keep the bit, as hosted LLVM does on Windows
      (`Target::failures`): no `nz.raise`, personality or LSDA there, so a unit links with LLVM's
      units and runtime objects alike. LLVM gives no entry to a frameless leaf; every function
      here has a frame and an entry
- [x] 8.4 `tests/coff.sh`: every input of `run.sh` built for
      `x86_64-pc-windows-gnu`, each unit object compared with LLVM's (same global symbols, every
      local function, relocation kinds and symbols, imports, a RUNTIME_FUNCTION per function with
      its UNWIND_INFO read back and every function LLVM's gives one among them, `llvm-dwarfdump
      --verify` and every line row of LLVM's), written twice the same, and the program linked with
      `x86_64-w64-mingw32-gcc` as link.txt says: **120 structurally sound and linked, 0 not,
      0 refused, 0 skipped** (2026-10-16). `nazm build --backend nazm --target
      x86_64-pc-windows-gnu` is carried (`nazm_codegen_nazm::carries`); `nazm_backend.rs` gains
      `the_generators_own_programs_build_for_windows_and_link_with_mingw` and
      `the_windows_objects_keep_the_win64_convention_and_describe_every_frame` (arguments, shadow
      space, xmm6–xmm15 untouched, rsi/rdi saved, `___chkstk_ms`, each UNWIND_INFO against its
      prologue, `.pdata`'s relocations). Mutants `r9-8-…` (eight), applied by hand, killed,
      restored. **Compile- and link-verified only**
- [ ] 8.5 Run on a Windows host: `BACKEND=nazm scripts/windows-host.sh` (each program's output,
      error output and exit status against the native build). Open: the Windows host (`win-dell`)
      did not answer on 2026-10-16, so nothing built by the generator for Windows has run
