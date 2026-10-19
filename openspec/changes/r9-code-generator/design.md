## Context

LIR (`docs/internal/architecture.md` §7.106) is the level both native backends consume: every guard,
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
reference is *skipped*, and neither is ever counted as agreeing (`docs/internal/runbook.md`, *Status
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

**D12. Coverage is measured in LIR forms.** A *form* is an operation with the classes it is
applied to (`add i32`, `sext i64<-i8`, `call c arg i16`, `switch i8`, `data writable`), since
selection's templates differ by class, not by value. `nzcg --universe` lists every form LIR's
validator admits (485); `nzcg --forms LIR` the forms a program uses; `tests/cover.sh` reports how
many of the universe the inputs that *agreed* in the last `tests/run.sh` used, and lists the rest.
The universe is wider than what the reference lowering emits — it never writes a `select` of a
float or an address, a `switch` on anything but an `i64`, a C call with a narrow or float
argument (N0381 allows only `Int`, `Bool`, `Str`, handles and C structs), an indirect call of a C
or plain function, a `sext`/`zext` from or `trunc` to a `bit`, or `i128` division, `mul.ovf` and
float conversions (runtime calls) — so a form left uncovered is either one of those or a program
still to write, and the list says which.

**D13. Scope before 1.0: macOS.** The copyright holder narrowed every target to macOS before 1.0
(2026-10-15): `aarch64-apple-darwin`, and `x86_64-apple-darwin` run under Rosetta. The ELF writer
and `tests/elf.sh` stay as built and are not extended; Linux is after 1.0.

**D14. The failure convention: LLVM's, by target.** On the hosted targets the LLVM backend
unwinds on a failure (performance-leadership 4.2, `docs/internal/architecture.md` §7.115). Until 7.5 this
generator kept the bit beside the result (D4), as Cranelift does, so objects of the two backends
were never linked together. Since 7.5 it unwinds where LLVM does on the targets it carries
before 1.0 (D13: `aarch64-apple-darwin`, `x86_64-apple-darwin`), with the same tables, so a
program may again be linked from both backends' objects; the ELF targets, frozen by D13, keep the
bit, which no program can tell apart (4.2, answer 2). How, as LLVM lowers it:

- **Raising.** `fail` calls `nz.raise`, which starts a forced unwind (`_Unwind_ForcedUnwind`,
  phase 2 only, under a stop function that aborts only at the end of the stack). Both are
  written by the generator into each unit with a failure exit, as *local* functions — LLVM's
  are `linkonce_odr hidden`, so a mixed program links either kind — and the exception object
  lives in `nz.raise`'s own frame rather than in LLVM's thread-local `nz.exc`: nothing reads it
  once a landing pad is reached, and every failure raises afresh, so no thread-local storage is
  needed in the generator's objects.
- **Landing pads.** A Nazm call whose failure edge has work (a release, a join) has a landing
  pad, and the pad *is* the edge's block: the call's `failed` value is only ever the condition of
  the branch that ends the call's block (LIR's validator), so the branch becomes a jump to its
  `else` and the unwind lands on its `then`. A call whose edge is a bare `fail` has no pad and
  no branch: the unwind passes through the frame. A call with no failure edge — a plain entry's
  call of its body (`main`, a C export or callback, a task's trampoline) — lands the unwind on
  a two-instruction pad that makes its result a zero and goes on as if the call had returned;
  the entry then reads the flag, so C is never unwound into.
- **Tables.** On Mach-O, each function with a pad has its compact unwind entry name the C
  personality, `__gcc_personality_v0`, and its LSDA in `__TEXT,__gcc_except_tab` (the entry's
  encoding then has `UNWIND_HAS_LSDA`); the linker builds the image's personality array and
  LSDA index. The LSDA is the C personality's: no landing-pad base, no type table, ULEB128 call
  sites (start, length, pad, no action), one per call with a pad. The frame is unchanged —
  compact unwind's frame modes already described where every callee-saved register is saved
  (5.2), which is what the unwinder restores a frame's allocated values from, and `sp` never
  moves after the prologue, so a pad finds the frame as the call left it.
- **Frames share slots.** A failure unwinds as deep as the frames fit: a stack-exhausting
  program (`N0408`) fails at a depth set by the frame's size, and a deep one that LLVM's frames
  fit must fit the generator's. Values and variables left in memory by the allocator now share
  8-byte slots with others whose lives do not overlap (the allocator's own intervals); one held
  in a register has none. `down` in `tests/programs/fail_task.nz`, recursing in a task's stack,
  went from 720 bytes a frame to 176 (LLVM's: 224); before, it exhausted that stack 278 frames
  down where LLVM's reached the failure 400 frames down.

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

**D15. Integration proposal: `nazm build --backend nazm` (a Rust change, the coordinator's).**
A third `nazm_lir::backend::Backend`, beside LLVM's and Cranelift's, in a small crate
(`nazm-codegen-nazm`) with no code generation of its own:

1. **The program.** `nzcg` is built from `toolchain/codegen/` by the build that ships `nazm`
   (`cargo xtask` with the LLVM backend, as the release does today; by itself once R8 cuts over)
   and installed beside it, `libexec/nazm/nzcg`; `NAZM_NZCG` overrides the path for tests. Its
   BLAKE3 digest is the backend's `version()`, so a changed generator is a new cache key, as
   Cranelift's `TRANSLATOR_REVISION` and settings are.
2. **The call.** `plan` is Cranelift's (modules, then instances, in link order). `generate` lowers
   the unit as Cranelift does (`Target::failures` = the bit on every target, D14), prints it with
   `op::print::module` (plus the `--debug` positions and `; file` lines `nazm lir --ops --debug`
   prints, when `--debug`), writes it to the build's temporary directory and runs
   `nzcg --target TRIPLE [--one-thread] UNIT.lir OUTDIR`, reading back `unit-0.o` as the
   `Generated` object. One process per unit keeps the existing per-unit cache; a whole-program
   batch (the text format already holds many units) is the faster option once the cache allows it.
3. **What it must be told.** The triple, and whether the program has one thread
   (`native.single_thread`), as `--one-thread`: with it the generator inlines the stack check
   against a plain `nz.stack_limit`, as LLVM does, instead of calling the runtime (7.3; the test
   scripts derive it today from the runtime object's symbol table).
4. **Refusals.** Exit status 3 with `nzcg: refused: …` lines becomes `N0101` per line, naming the
   construct, with nothing produced and no fallback ("neither falls back to the other", as for
   Cranelift); any other non-zero status is `N0900`, a compiler defect, with its standard error.
5. **Tests.** The backend-differential tests gain a `nazm` column (output, status and memory
   counts against LLVM and the oracle), and `toolchain/codegen/tests/run.sh` becomes a Rust test
   driving `--backend nazm` directly instead of substituting objects. In process (no subprocess)
   comes at R8, when the compiler driving it is itself Nazm.

**D16. Windows x86_64 before 1.0 (tasks section 8).** R8 replaces Cranelift by this generator,
and Cranelift builds `x86_64-pc-windows-gnu` today, so the generator carries it too (the copyright
holder, 2026-10-16: "complete r9 without linux"; Linux stays after 1.0, D13). As LLVM does for
the triple, read from its objects:

- **Objects**: PE/COFF relocatable, AMD64 (`coff.nz`): `.text`, `.data`, `.rdata` (read-only data,
  addresses included — the image relocates them), `.xdata`, `.pdata`, `.drectve`
  (`-exclude-symbols:` per hidden definition) and the DWARF sections, offsets between them
  `SECREL`; relocations `REL32` (calls, `rip`-relative addresses — an import too: PE has no GOT,
  and LLVM addresses `nz.stack_limit` directly), `ADDR64` (data holding addresses, DWARF
  addresses) and `ADDR32NB` (`.pdata`). Addends in place.
- **Convention**: Windows x64 — four positions, rcx/rdx/r8/r9 or xmm0–xmm3 by position, then
  the stack above 32 bytes of shadow space every call reserves; an `i128` two positions, split
  between r9 and the stack when it falls there (LLVM splits it for this triple; Clang's
  by-reference `__int128` does not apply to LIR's `i128`); results as System V's — rax, rax:rdx,
  xmm0, and the failure bit in the next register, as LLVM returns `{T, i1}` here. rsi, rdi and
  xmm6–xmm15 are callee-saved: the allocator's caller-saved pool's rsi and rdi are pushed when
  used, and the scratch xmm pair is xmm4/xmm5 instead of xmm14/xmm15. The other targets' bytes do
  not change.
- **Failures**: the bit, as LLVM keeps it on Windows (`Target::failures`: no unwinder this
  compiler uses there) — so no `nz.raise`, personality or LSDA; a Windows unit links with
  LLVM's runtime objects and with LLVM's units alike.
- **Frames and unwinding**: `push rbp; mov rbp, rsp`, the callee-saved pushes, the allocation —
  from 4096 bytes through MinGW-w64's `___chkstk_ms` (`mov eax, N; call; sub rsp, rax`), as LLVM
  probes for this triple (`Target::probes` = `Platform`) — and one UNWIND_INFO per function (no
  frame register: rsp is fixed after the prologue), its `.pdata` entry for every function; the
  epilogue is the form the Windows unwinder recognises (`add rsp`, pops, `ret`). LLVM gives no
  entry to a frameless leaf; every function here has a frame, so every one has an entry.
- **Evidence**: `tests/coff.sh` checks each object against LLVM's for the triple and links every
  program with MinGW-w64; `BACKEND=nazm scripts/windows-host.sh` runs them on a Windows host
  against the native build (task 8.5: 68 agree, 0 differ).

## Open Questions

- Whether `--backend nazm` (D15) lands before R8, beside LLVM and Cranelift, or only at cut-over
  (the coordinator's call at integration).
- Stage B's performance target relative to LLVM `-O0`, set when stage A's numbers are measured.

## As built (2026-10-15)

- **Files.** `toolchain/codegen/`: `lir.nz` (the reader), `common.nz` (the code buffer and
  relocations, value classes, frame layout, the stage-B allocator), `a64.nz` and `x64.nz` (the
  encoders), `sel.nz` and `sel_x64.nz` (selection), `macho.nz` and `elf.nz` (the writers),
  `forms.nz` (D12), `main.nz` (`nzcg [--target TRIPLE] LIR OUTDIR`). Tests in `tests/`: `run.sh` (D8's
  differential suite), `encode.sh` (D8's encoder check), `elf.sh` (D11's structural check),
  `perf.sh` (D3's report), `cover.sh` (D12) and `programs/`.
- **D3 stage B** allocates into callee-saved registers only (x19–x28 on aarch64; rbx, r12–r15
  on x86-64), saved and restored by the frame. No call can clobber an allocated value, so the
  scratch and argument registers stay the selection's and its templates did not change: a name
  with a register is moved to and from it where stage A loaded and stored its slot. `i128`
  values stay in slots.
- **D4 on x86-64**: the failure bit is in rdx after an integer result, rax after a float or no
  result, rcx after an `i128` — LLVM's `RetCC_X86Common` order for `{T, i1}`.
- **D11 ELF** objects are checked, not run: this host has no Linux linker or kernel and the
  track uses no Docker. Imported symbols are reached through the GOT on every target. On Linux
  aarch64 a stack argument takes at least eight bytes (AAPCS64); Apple's packing applies only
  to the Apple targets.
- **Branch fusion and the remainder's guard (7.6).** On both targets a comparison or a checked
  operation's overflow bit that is its block's last instruction and read only by the block's
  branch stays in the flags (`fusable`); every instruction a template emits after it moves or
  stores and never sets flags. A signed remainder's divisor guard — the LIR sequence
  `cmp eq a, MIN; cmp eq b, -1; and; select …, 1, b` before `srem a, s` — is recognised when
  none of its values is read elsewhere (`guarded_rem`) and not selected: it exists for x86-64's
  `idiv`, which traps on the minimum by -1, so x86-64 branches over `idiv` for a 64-bit divisor
  of -1 (the remainder is 0) and divides narrower classes at 64 bits, and aarch64's `sdiv` and
  `msub` give 0 themselves. A recognition of the lowering's own sequence, not an optimisation
  of arbitrary code: anything else falls back to the per-instruction templates.
- **x86-64 floor, ceil and truncate** use SSE2 alone (convert to an integer and back, adjust by
  one, keep the sign; a magnitude of 2^52 or more, an infinity or a NaN passes through), since
  SSE4.1's `roundsd` is not in x86-64 v1's baseline; one sequence for both x86-64 targets, so
  the Rosetta suite runs the code the Linux target gets. Likewise `popcnt` is the bit-parallel
  count and `clz`/`ctz` are `bsr`/`bsf` with the zero case selected.
- **D10's input** is `nazm lir --ops --debug` (added by this track, at the coordinator's
  request, in `crates/nazm-cli`): it lowers as `nazm build --debug` does, so each function's `at`
  and every `loc` are printed, and after each unit one `; file INDEX NAME` line per file the
  positions name, since the module holds files only as indices. Plain `--ops` is unchanged.
- **D10 as built** (`dwarf.nz`): DWARF 4, one compile unit per object (C99, producer `nazm`,
  as the other two backends write), a subprogram per function with an `at`, and a sequence per
  such function: its declaration line at its first instruction, then a row at the address where
  each `loc`'s code begins. A position with no code of its own — a jump laid out as a
  fall-through — keeps its row at the next position's address, so every position LIR names is
  in the table. In Mach-O the addresses are written as section offsets with section-relative
  relocations, as LLVM's are (`dsymutil` and `llvm-dwarfdump` read the object in place); in
  ELF they are relocations against the functions and the DWARF sections' own symbols.
  `tests/lines.sh` holds them to LLVM's: `llvm-dwarfdump --verify` passes, every (file, line,
  column) row of LLVM's with a nonzero column is one of ours — LLVM's column-0 rows are its
  merged locations — and the program linked with them runs as LLVM's debug build does.
- **5.2 as built.** The frame keeps compact unwind's layouts: aarch64 pushes each used pair of
  x19–x28 with `stp` right below the frame record (`UNWIND_ARM64_MODE_FRAME` and a bit per
  pair), x86-64 pushes rbx/r12–r15 right below rbp (`UNWIND_X86_64_MODE_RBP_FRAME`, their count
  and list). Mach-O objects carry `__LD,__compact_unwind`; ELF objects an `.eh_frame` (a CIE
  and an FDE per function describing the prologue — nothing unwinds from an epilogue).
  `tests/unwind.sh` walks the stack with `_Unwind_Backtrace` from C through Nazm frames that
  keep values in callee-saved registers, built by LLVM and by the generator: the same frames.
