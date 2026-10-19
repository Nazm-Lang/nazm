## Why

`rewrite-in-nazm` (R2, tasks 3.1–3.4) needs the compiler written in Nazm (`compiler/*.nz`) to carry
**the whole language** with the Rust front end's diagnostics, so that R3 (pipeline), R4
(interpreter) and R5 (commands) can be fed by a front end written in Nazm. Today it compiles itself
(C2 ≡ C3) and agrees with the reference on 21 parity probes (`compiler/parity.json`), but that is
parity on the probes, not on the language: it refuses string escapes, `::`, `as`, packages, every
Gate 2 type and built-in, and every R1 form (`docs/user/limitations.md`, *Self-hosted compiler*;
`crates/nazm-cli/tests/selfhost.rs`, `NOT_CARRIED`).

## What Changes

The front end of `compiler/` is completed in five slices, each compared with the Rust toolchain
before it is called done:

1. **This proposal** — the missing constructs in dependency order (below), the comparison forms and
   the test harness (`toolchain/frontend-tests/run.sh`, inputs in `toolchain/frontend-tests/inputs/`).
2. **Lexer and parser complete** — every token and every tree the Rust `nazm-syntax` produces, with
   its syntax diagnostics and recovery; compared token by token and tree by tree.
3. **Names and types** — modules and packages, every type the language has, generics, traits,
   closures and effects resolved and typed as `nazm_sema` does.
4. **The checker's rules, group by group** — every diagnostic the Rust checker gives, identical in
   code, span, message, help, secondary labels and fixes, text and JSON, with its recovery.
5. **Core IR identical** — `nazm.core-ir/2` as `nazm core-ir` prints it, digests included.

### What `compiler/*.nz` refuses today, in dependency order

Each line is a unit of work; a later group uses the earlier ones. "L" lexer, "P" parser,
"N" names and types, "C" checker, "I" Core IR.

**A. Tokens (L)** — string escapes (`\n \t \r \0 \\ \" \xHH \u{…}`, N0004); `::`; `..`; `&&`/`||`
(have); bit operators `& | ^ ~ << >>`; compound assignment `+= -= *= /= %= &= |= ^= <<= >>=`; `@`;
hexadecimal, octal and binary integers; integers too large for `Int` (kept as digits); floats with
fraction and exponent; raw strings `r#"…"#`; multi-line strings `"""` (N0005); interpolated strings
`f"…"`/`f"""` with holes and formats (N0006); nested block comments (N0007); every lexing error
collected rather than the first one fatal.

**B. Declarations (P, then N)** — `use "p" as m;` and `m::item` (qualifiers on calls, types,
constructions, variants and patterns); `pub use` re-exports with name lists; package imports and
manifests; attributes `@name(args)` on every declaration; `const` and `static`; `extern "C"`
records (opaque and laid out), functions and exports; traits with default methods; impls over
generic types (`impl[T: R]`); inherent impls (methods by name, R1 5b.16); effect parameters
(`effects E`); contracts (`requires`/`ensures`, have) .

**C. Types (P, then N)** — every integer width (`Int8`…`Int128`, `UInt8`…`UInt128`) and `Byte`;
`Float32`/`Float64`; fixed arrays `[T; N]`; tuples `(A, B)`; function types `fn(A) -> R ! {e}`;
`Bytes` and `Text`; `Box[T]`, `List[T]`, `Cell[T]`; `Option`/`Result` short variants (`Some(x)`,
`Ok(x)`, R1 5b.14); `Chan[T]` of every element type; `Atomic`; capability types (`TimeCap`,
`MmioCap`, `ForeignCap`, …); `let x: T` annotations (5b.17).

**D. Statements (P)** — `else if` (5b.11); compound assignment and indexed places `v[i] = e`;
`for x in e` and `for i in a..b` (R1a); `defer` (R1b); `while let`, `loop`, match guards (5b.18);
`let … else`; `if let`; semicolon-free lines (5b.19); `scope`/`spawn` (have).

**E. Expressions (P)** — closures `fn(x: T) -> R { … }` and captured state; method calls
`v.m(a)` and `Trait.m(x)`; arrays `[a, b]`, `[v; n]` and indexing `v[i]`; tuples and `.0`; wide
integer and float literals; `~` and the bit/shift operators with their precedence; interpolation
and `print(e)`'s rewrite to `print(f"{e}")` (5b.13); patterns — literal, wildcard, binding, or,
tuple, nested variant; error recovery inside expressions (area 1).

**F. Built-ins (N, C)** — the `NOT_CARRIED` list of `selfhost.rs`: the Gate 2 numeric built-ins,
`Bytes`/`Text`, files and handles, networking, processes, time, MMIO, `c_errno`, channels of `T`,
`select`, `Box`/`List`/`Cell`, `checked_*`, R1's library built-ins (BLAKE3, SHA-256, file walks,
`os_stdout`, `os_spawn_in`, `os_lock`, string building), interpolation's formatting built-ins.

**G. Checking (C)** — profiles (`--profile`), provenance and information flow, capability and
effect inference across closures and effect parameters, exhaustiveness over nested patterns,
ownership/transfer rules for every new type, the recursion and layout refusals (have, extend),
`--deny-warnings` and every warning.

**H. Output (I)** — Core IR for every construct, per-module output (task 3.3), the performance
budgets measured (3.3), and `NAZM_BIN` black-box runs (3.4).

**R1.5 forms last** — `:` result types, `= e` bodies, `|a, b|` closures, map literals, positional
payloads: after the coordinator declares them final.

## Capabilities

### New Capabilities
- `front-end-in-nazm`: what the Nazm front end must agree with the Rust one on. Archived with
  `--skip-specs`; the normative text lands in `docs/internal/bootstrap.md` and `docs/user/limitations.md` when R2
  closes.

### Modified Capabilities
None.

## Impact

`compiler/*.nz` (lexer, parser, checker, emitter and their drivers), `toolchain/frontend-tests/` (new: the
comparison harness, extracted inputs, and a small Rust reference dumper outside the workspace),
`compiler/parity.json` when the probes change. Asked of the coordinator, not changed here:
`crates/nazm-cli/tests/selfhost.rs` (its token mapping and tree dump name the old subset),
`docs/user/limitations.md`, `docs/internal/bootstrap.md`, and the runbook's containment rule for running the
comparison (see `design.md`, D6).
