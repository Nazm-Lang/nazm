## Context

`compiler/*.nz` (~16.9k lines) is a lexer (`lex.nz`), a parser into a post-order arena of parallel
`Ints`/`Strs` columns (`parse.nz`), a checker (`analyse.nz`), a module resolver (`module.nz`) and an
LLVM emitter (`emit.nz`). It compiles itself: the reference builds C1, C1 builds C2, C2 builds C3,
and C2 ≡ C3 (`compiler/bootstrap.sh`). Whatever it carries, it may then use in its own source —
and only that, because stage 2 compiles `compiler/` with C1.

The Rust front end it must agree with is `nazm-syntax` (lexer 1.8k lines, parser 6k),
`nazm-sema` and `nazm-core`'s checker, and `nazm-cir`'s Core IR.

## Goals / Non-Goals

**Goals:** every program the Rust front end accepts, the Nazm one accepts with the same tree, types
and Core IR; every program it refuses, the Nazm one refuses with the same diagnostics; the chain
keeps reaching its fixpoint after every slice.

**Non-goals:** the native pipeline (R3), the interpreter (R4), the commands (R5); a faster front
end than the reference (budgets are measured in 3.3, not chased here); R1.5's syntax before it is
final.

## Decisions

**D1 — Extend the arena compiler; do not start another.** The existing stages already self-host,
and the emitter, checker and bootstrap read the arena. New node kinds and columns are added to it;
a construct becomes usable in `compiler/` itself once C1 carries it and the chain still reaches its
fixpoint. A second compiler beside it would have to re-earn the bootstrap from nothing.

**D2 — Tokens are compared as `Kind start end payload`.** `Kind` is the Rust `TokenKind`'s variant
name, so the comparison needs no numbering agreement; the payload is the decoded value (`Int` in
decimal, `IntTooLarge` and `Float` as their digits, `Str` and `Ident` as text, `FStr` as its pieces —
`T:text` or `H:at:align:width:precision:start:end:text`, joined by `|`). After the tokens, each
lexing diagnostic is `error CODE start end message`. Payloads and messages are written byte by
byte with every byte outside `0x20..0x7e`, `\` and `|` as `\xHH`, so neither side needs a Unicode
table to produce the same bytes.

**D3 — Trees are compared as the Rust parser's `{:#?}`.** The reference dumper prints
`nazm_syntax::parse_source(text)`'s `Program` with Rust's derived pretty `Debug`, then each parse
diagnostic as in D2. `Debug` is exhaustive by construction — every field of every node, spans
included, in declaration order — so the comparison cannot drift from the tree the way a hand-written
dump does (the `selfhost.rs` dump collapses closures, method calls and every R1 node to a
placeholder). The Nazm parser's driver prints the same text from its arena. The diagnostics' spans
are what the checker will point at, so they are compared from the start. Both sides write a span
`start..end` rather than `Debug`'s seven lines (`Span { file: FileId(0), … }`): the dumper rewrites
exactly that shape and nothing else, and a span of another file would stay whole and show.

**D11 — The parser is a port, and its tree is shaped as `Debug` shows it.** `compiler/syntax.nz`
ports `nazm-syntax/src/parser.rs` function by function (recursive descent, Pratt binary operators,
the reference's recovery, cascade muting, nesting bound and every desugaring — `else if`,
`while let`, `loop`, `let … else`, `for` over a pattern, `print(e)`, `Some(x)`, method names of
inherent impls, trait default methods). Its arena's nodes are structs with named fields, tuples,
lists, atoms, strings and spans — the shapes `{:#?}` prints — so the printer is one function and the
field names are written once, where each node is built. It does not replace `parse.nz` yet: the
checker and emitter still read the post-order arena (task 2.8). The concrete syntax tree the
reference builds beside the abstract one is not ported; nothing downstream of the parser reads it.

**D4 — Diagnostics are compared as `nazm check --json` objects, whole.** Code, severity, message,
primary span and label, secondary labels, help and fixes, in the reference's order. The text form
is a rendering of the same objects and is compared on a sample once the JSON agrees.

**D5 — Core IR is compared byte for byte with `nazm core-ir`.** Digests included: they are BLAKE3
over the reference's semantic encoding, and `library/std/blake3.nz` (R1, D4) supplies the hash.

**D6 — Running the comparison on this host.** The runbook makes every generated compiler stage
contained work, and `bootstrap.sh` refuses to start without containment. This track may not run
contained workloads or set `NAZM_CONTAINED`/`NAZM_ALLOW_UNCONTAINED`. The comparison therefore
builds each driver with the reference and **`--heap-limit 2G`** (R1, D5: an allocation past it
ends the program with `N0406`), and runs it under a wall-clock deadline. The bootstrap's own chain
(C2 and C3 are linked from IR the Nazm compiler emitted, which carries no heap limit) is not run
here: the track's host check is the reference checking the compiler source, C1 — built with the
heap limit — emitting C2's IR from it, and C1 compiling and running the conformance corpus. C2 ≡ C3
stays the coordinator's contained `bootstrap`. Whether a heap-limited stage is enough to run
compiler stages on a host is R6's decision, not this track's.

**D7 — The reference dumper lives in `compiler/tests/refdump/`**, a Rust binary with its own
`[workspace]` and path dependencies on `nazm-syntax`, `nazm-diag` and `nazm-span` of the same
checkout, built into `$HOME/.cache/nazm-target-r2`. It is the only Rust this track writes, it
ships nowhere, and at integration its two functions belong in `selfhost.rs` beside the existing
dump.

**D8 — Inputs.** `tests/compat/v1/`, `corpus/`, `examples/`, the compiler's own sources, and the
programs written inside `crates/nazm-cli/tests/*.rs`, extracted by
`compiler/tests/extract-inputs.py` into `compiler/tests/inputs/rs/` (2,286 distinct programs at
`c73dab2`), and the `.nz` corpora that live under `crates/nazm-cli/tests` (refusals, transfers,
domains, …) and `crates/nazm-cli/templates` — 2,710 inputs in all. Many of the extracted programs
are meant to be refused; they are the diagnostic corpus. No string literal in `compiler/*.nz` holds
a line break (`selfhost.rs` dumps a string's value on one line of its tree dump), so the sources
write `nl()`.

**D9 — Every lexing error is collected.** The Rust lexer is lossless and keeps going after an error,
so the Nazm lexer records every diagnostic (`lex_all`) and continues exactly as it does. `lex`, which
the parser and checker call today, keeps its fail-at-the-first behaviour until the parser recovers
(slice 2), so no caller changes meaning before it can.

**D10 — Token payloads in the arena.** `texts` holds a token's decoded value: an integer's decimal
digits with `_` removed (a hexadecimal, octal or binary literal converted to decimal, as the
reference's value), a float's text with `_` removed, a string's decoded bytes, an interpolation's
pieces in D2's form. Whether an integer fits `Int` is a separate kind, `IntTooLarge`, as in Rust.

**D12 — Budgets are measured, not chased, in R2.** `compiler/tests/budget.sh` times the Nazm
lexer and parser drivers (built by the reference at `-O0`, heap-limited) against the reference
dumper on the three largest sources. First measurement (2026-10-15, this host): lexing
`analyse.nz` (7.4k lines) 0.04 s and 24 MB against the reference's 0.02 s and 11 MB; lexing and
parsing it and printing its tree 0.11 s and 65 MB, where the reference takes 1.01 s and 66 MB — but
the reference's time there is almost all Rust's `{:#?}` and the span compaction, so only the lexing
row compares like with like. The budgets `rewrite-in-nazm` 1.4 asks for are set from these when
the checker exists to measure (3.3).

## Risks / Trade-offs

- **Size.** The Rust parser alone is 6k lines and the checker several times that; written in the
  arena style without recursive types, the Nazm side is larger. Mitigation: the slices each leave a
  self-hosting compiler and a comparison that says exactly how much agrees.
- **Self-hosting constraint.** New code in `compiler/` may only use what C1 carries, so early slices
  are written in the old subset (no escapes, no `else if`, no `for`). Mitigation: each feature is
  adopted in the source only after the comparison and the host check pass with it.
- **`Debug` couples the comparison to field order.** A field added to the Rust AST changes the
  expected text. That is the point — the Nazm tree must follow — but it means the reference dumper is
  rebuilt from the same checkout as the Nazm sources it is compared with.
- **Host execution** (D6): a heap limit bounds the Nazm program's heap, not its stack or the
  process's other memory; the deadline bounds the rest in time only.

## Open Questions

- Whether the coordinator wants the R2 comparisons folded into `selfhost.rs` (contained) or kept as
  a host test under the heap limit (R6).
- The Core IR digests' encoding is not documented outside `nazm-cir`; slice 5 reads the code.
