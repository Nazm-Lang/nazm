## 1. The proposal

- [x] 1.1 The data model (Core IR in its JSON arena, types as spellings, MIR as enums) and the order
      of work (`design.md`)
- [x] 1.2 The gaps in `nazm.core-ir/2`, and the request for `nazm.core-ir/3`

## 2. Reading Core IR

- [x] 2.1 `cir.nz`: the document, function headers, locals, constants, typed accessors
- [x] 2.2 `types.nz`: spellings parsed, MIR keys, substitution, the type table recovered from uses

## 3. MIR for integers, calls, branches and loops

- [x] 3.1 The lowering of §7.42 for `Int`/`Bool` functions; the stack check for call cycles
- [x] 3.2 Text, printed form and BLAKE3 digest identical; `fib` and the arithmetic corpus agree
- [x] 3.3 `tests/run.sh`: build, run over every input, compare, one-line summary

## 4. Ownership

- [x] 4.1 Strings, sequences, records, enums: copies, moves, drops, field borrows
- [x] 4.2 `match`, `?`, `return`/`break`/`continue` cleanup, scopes, spawn, failure edges
- [x] 4.3 Generic instances (the planner, sorted by identity), trait calls resolved by the impl
      functions' names, derived `Hash`
- [x] 4.4 Closures (captures recovered as the locals after the parameters) and function-value
      thunks (synthesised into the document, as Rust synthesises their Core IR)
- [x] 4.5 Array types recovered across calls (`refine.nz`); the instance plan bounded (`N0358`)
- [x] 4.6 Comparison: 103 of the brief's 105 inputs identical to `nazm mir`, and all 83 of
      `compiler/` with MIR and this pipeline itself (its own ~500 functions, `@std` included): 187
      agree, 2 differ. The 2 are the Core IR /2 `@export` gap (`desktop/counter`,
      `check/export-numbers`): the digest's canonical text names the C symbol

## 5. The MIR validator

- [x] 5.1 Structure and typing — all of Rust's checks but a primitive's argument and result types
      (the built-ins' signature table is the checker's; to be shared with R2) and the signatures of
      function values and arrays (spelled `?` in Core IR /2)
- [x] 5.2 The ownership dataflow, as Rust's: empty, owned or either per tracked local, joined by
      union, source blocks first
- [x] 5.3 `pipeline mutants`: five kinds of breakage (a drop removed from a source block, a
      failure edge removed, a jump past the last block, a return of another type, a parameter
      moved), the first place each fits in every function: 11967 made over the inputs (this pipeline among them, so the count moves with it), all caught;
      `pipeline mir` prints only what the validator accepted, as `nazm mir` does

## 4b. Core IR /3

- [x] 4b.1 The reconstructions replaced by `/3` (design, *Core IR /3*): the type table, `refine.nz`
      (deleted), closure captures, the impl table with default bodies, derived `Hash`; `@export`
      in the digest. `run.sh` with main's `nazm` (which reaches `compiler/tests/inputs/`): 1449 of
      1449 inputs with MIR identical (from 1425 of 1449 under the reconstructions), 33991 validator
      mutants, all caught

## 6. LIR

- [x] 6.1 Data layout, units, symbols, linkage (`lir.nz`, `pipeline lir`): a unit per module that
      defines a function, then per instance; symbols from identities (`nz.m.MODULE.NAME.g.ARGS`,
      a digest past 512 bytes); `hidden` for `pub`, reached from a generic body or an impl, an
      instance, `main` or a function with a placing attribute; externals sorted
- [x] 6.2 Record and enum layouts: the types each unit mentions closed over their contents, by the
      checker's ids; canonical field and variant order, offsets, sizes, alignments, ownership;
      names as `Types::show` writes them (`Box`/`List`/`Cell` as their `Vec`, `Chan[_]`, `fn`).
      `run.sh` compares `pipeline tables` with `nazm lir` minus each unit's `lir` member
      (`tests/tables.py` re-serialises byte for byte): 1009 of 1009 inputs with LIR identical
      when `compiler/tests/inputs` was in the set; 161 of 161 when it moved to
      `toolchain/frontend-tests`, and 1000 of 1000 since `run.sh` reads it there (6.3.5)
- [x] 6.3 Instructions (`--ops`, `ops.nz`), growing by what the inputs need
  - [x] 6.3.1 Module sources (`nazm core-ir --with-sources`, added read-only at the coordinator's
        decision) and each MIR statement's span (`Block.ats`, not printed)
  - [x] 6.3.2 The module: symbols in first-use order, data (literals, failure messages with
        `file:line:column`), the runtime registry (`runtime.nz`, generated from
        `crates/nazm-runtime`'s `IMPORTS` by `tests/runtime_table.py`, checked by `run.sh`)
  - [x] 6.3.3 Functions: locals in variables, slots or promoted values; integer arithmetic and
        comparison with their guards; direct calls and their failure bits; strings
        (`str_concat`, `num_to_str`, `print`), drops; the stack check; plain entries. `fib` and
        `hello` identical, the whole `nazm lir` document (text and digests) included
  - [x] 6.3.4 Records and enums (construction, fields owned and borrowed, payloads, `switch`),
        `exit_with`, the type helpers that retain and release (records, enums, arrays, a
        vector's elements), simple string and sequence primitives. `run.sh`: 68 of 161 inputs
        with LIR identical, document and digests included; every other stops at a construct
        named as not ported, and none produces different text
  - [x] 6.3.5 The remaining primitives (sequences' push/get/set/pop, channels, files, bytes and
        text, contracts, `numeric.rs`, `arrays.rs`, `atomics.rs`), conversions, the helpers that
        compare, order and hash, closures and indirect calls, scopes and spawn, foreign calls
        and exports, statics and sections, large frames. `run.sh` reads the front end's test
        programs again (`toolchain/frontend-tests/inputs`) and prints its summary over the base
        set and the full set: lir ops 158 of 161 (base) and 981 of 1000 (full) identical,
        document and digests included; the 19 others all stop at a generic channel's helper
        (6.3.6), and none produces different text
  - [x] 6.3.6 A generic channel's helper: LIR names it by its element's key (`nz.h.vel.chan.KEY`,
        as `vec.KEY`) instead of the checker's channel id, the coordinator's decision, in
        `crates/nazm-lir` (`Lw::type_name`); `ops.nz` derives it. `run.sh`: lir ops 161 of 161
        (base) and 1002 of 1002 (full) identical, document and digests included

## 7. Task 4.1's later items

- [x] 7.1 The runtime generator (`rtgen.nz`; `rtext.nz` from the runtime crate by
      `tests/runtime_text.py`): the hosted runtime, the entry, the per-target units and the
      counted heap, for what the program's LIR reaches (`pipeline runtime`). `run.sh`: runtime
      units 160 of 160 (base) and 977 of 977 (full) identical to `nazm build --emit-ir`'s, by
      default and with `--stack-size` and `--heap-limit` (25 inputs with LIR `nazm build`
      refuses on the host: board, WebAssembly and C-library programs)
- [x] 7.2 The object cache (`objcache.nz`): framed digests, the LLVM object key with the
      backend identity and configuration derived from the build's `clang -###` probe
      (`tests/probe.sh`, `pipeline backend`), the runtime digest over every target's units, and
      the store's entries read, refused and published as Rust's. `run.sh`: 4521 units of 977
      inputs found under the key computed here, each entry written again byte for byte the one
      `nazm build` wrote; the runtime digest equal to the one `--provenance` records; 9 of 9
      broken entries read as expected. The Cranelift key is left to R9, whose own replaces it
- [x] 7.3 Debug information: `pipeline ops-debug`, as `nazm lir --ops --debug` — positions,
      the variables a debugger shows with their scopes, the files the positions name — with
      every MIR statement's span Rust's; `nazm core-ir --with-sources` gives each module's file
      index and path, a call's `callee_at` and a binding's `at`. `run.sh`: lir ops --debug 161
      of 161 (base) and 1002 of 1002 (full) identical

## 8. After 4.1

- [x] 8.1 Failures by unwinding (performance-leadership 4.2): `rtext.nz` regenerated — runtime
      ABI 34, `nz.exc` among the one-thread globals; LIR's text and digests unchanged, and no LIR
      validator here to take the reference's new rule (design.md). `run.sh` against the reference
      built at `ce24bcd0`: MIR 1453 of 1453, LIR tables and ops 1007 of 1007 (document, digests
      and `--debug` included), runtime units 982 of 982, object cache 982 of 982 inputs (4546
      units), the runtime digest equal, 39924 of 39924 mutants caught
