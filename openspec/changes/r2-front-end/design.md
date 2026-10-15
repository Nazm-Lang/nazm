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

**D6 — What runs on this host (revised 2026-10-15 by the coordinator).** Generated compiler
stages — C1, C2, C3, and the old `nzlex`/`nzparse`/`nzcheck` drivers — run only contained (runbook,
*Mandatory containment*; `compiler/AGENTS.md`); the coordinator runs every stage comparison, the
bootstrap and selfhost there. On the host this track runs only the reference `nazm` and what the
comparisons with it need: the R2 drivers (`tokens`, `tree`, `diagnose`), built by the reference
with `--heap-limit 2G` and run under a deadline (`run.sh`), and `host-check.sh`, which is the
reference checking the compiler and the drivers and `nazm fmt --check`.

**D7 — The reference dumper lives in `toolchain/frontend-tests/refdump/`**, a Rust binary with its own
`[workspace]` and path dependencies on `nazm-syntax`, `nazm-diag` and `nazm-span` of the same
checkout, built into `$HOME/.cache/nazm-target-r2`. It is the only Rust this track writes, it
ships nowhere, and at integration its two functions belong in `selfhost.rs` beside the existing
dump.

**D8 — Inputs.** `tests/compat/v1/`, `corpus/`, `examples/`, the compiler's own sources, and the
programs written inside `crates/nazm-cli/tests/*.rs`, extracted by
`toolchain/frontend-tests/extract-inputs.py` into `toolchain/frontend-tests/inputs/rs/` (2,286 distinct programs at
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

**D12 — Budgets are measured, not chased, in R2.** `toolchain/frontend-tests/budget.sh` times the Nazm
lexer and parser drivers (built by the reference at `-O0`, heap-limited) against the reference
dumper on the three largest sources. First measurement (2026-10-15, this host): lexing
`analyse.nz` (7.4k lines) 0.04 s and 24 MB against the reference's 0.02 s and 11 MB; lexing and
parsing it and printing its tree 0.11 s and 65 MB, where the reference takes 1.01 s and 66 MB — but
the reference's time there is almost all Rust's `{:#?}` and the span compaction, so only the lexing
row compares like with like. The budgets `rewrite-in-nazm` 1.4 asks for are set from these when
the checker exists to measure (3.3).

**D13 — The checker is written against `syntax.nz`'s tree and knows where it stops.** Agreed
with the coordinator (2026-10-15): the new checker reads `syntax.nz`'s tree, and `parse.nz` and
`analyse.nz` retire together once it carries what they do. It is a port of `nazm-core/src/check`
and `nazm-sema`, stage for stage as `check_reusing` runs them (type names, field types, layouts,
trait methods, declarations, re-exports, constants, bodies, provenance, the `main` rules). While
it is partial it must never report a diagnostic the reference would not: a compilation that uses
anything the Nazm checker does not carry yet gets none of its diagnostics, and `diagnose` says on
standard error what it stopped at. `run.sh check` then counts the inputs the checker carries, and
among them every difference is a defect, not a gap; coverage grows group by group (`tasks.md` 3,
4), most frequent codes first — on 2026-10-15 N0304, N0201, N0300, N0619, N0369, N0372 by inputs.

The checker is `compiler/sema.nz`, driven by `diagnose.nz`. Standard error is kept apart from the
diagnostics: `run.sh check` writes each input's stop reason beside its result and reports how many
inputs the checker carries whole and how many of those agree. After a syntax error the checker
checks the trees the parser recovered, as `analyse` does — their `Debug` dumps agree with the
reference's on every input (D3) — and publishes its errors after the parser's, never the missing
`main` and never a warning. The loader's refusal of a package is a stop: it does not resolve
packages yet.

**D14 — The built-in table is generated, never transcribed.** `compiler/builtins.nz` is printed by
the reference dumper's `builtins` mode from `nazm_sema::Intrinsic::ALL` — each built-in's name,
parameter and result shapes, type parameters, numeric class, whether it is a prelude name, effect
and the authority it needs (`CapabilityKind::of_intrinsic`), and which arguments are sinks
(`SinkClass::of_intrinsic`) — and `toolchain/frontend-tests/builtins.sh`
regenerates it (`--check` says whether the tree's copy is current). One table, as `intrinsic.rs` says
of its own two readers: the Nazm checker cannot drift from the Rust one by a hand-copied row.

**D15 — Generics as the reference settles them, instance by interned application.** A type
parameter is an interned type naming its owner (a record, an enum, a function, or a built-in's
`T`) and its position; an application is its definition's id with an argument run, and a field's
type in it is the definition's with the arguments substituted, so the reference's instances need
no table of their own. A call of a generic callee is `check_generic_call` step for step: written
arguments, or each argument unified left to right with its declared type, then the wanted result
type; one mistake one error (`excuse`). A generic built-in's types are made per call from the
table's shapes (D14), its `T` named `T` and declared at no place (`0..0`, as the reference prints
a synthetic span). Layout cycles are searched over definitions rather than instances, which ends
and over-approximates: any cycle through a generic definition stops the checker rather than risk
a refusal it does not word yet. The `check` comparison's reference is `nazm` built from this
checkout (the checker ports `main`, R1.5 included): run with `NAZM` set to that build, not the
0.10.0 release, which lacks R1.5 D8 and N0636 and so differs on ten inputs the checker carries.

**D16 — The module graph is the loader's, and every stage runs over every module.** The loader
(`load.nz`) records, for each file, the identity each of its `use` items names; the driver hands the
checker each module's targets by load order. The checker builds the edges as
`ModuleGraph::import_as` does — one per module named, in the order of the first `use` of it, plain
when any `use` of it is, with every name it is imported under, none for a module naming itself — and
the core prelude last on every other module. Then `check_reusing`'s stages run module by module in
load order (`graph.ids()`): type names, each module's field types against its imports' nameable
types (collisions reported there), layouts, declarations with each module's `pub` exports, then each
module's environment (N0209, N0208, N0207 for functions) and bodies against its direct imports only.
A standard module is checked as the reference checks it (`--no-cache` reuses nothing), and holds
no output authority it is not given (R1.5, D8); an imported function with no declared effect set
is assumed to do every bridged effect. A `pub use` re-export stops the checker until N103 is
carried. R1.5 D9 (on `main` since `511f77d6`) gives a program module `@std/hash`, `@std/hashmap`,
`@std/ordmap`, `@std/sort` or `@std/text` where it names one of their items: the loader ports
`prelude_named` to find where it does and loads those modules after the written imports, and the
checker adds each as `import_implicit` does — plain, after the written edges and before the core
prelude, only where no `use` names the module — whose names yield, silently, to the module's own
and to a written import's (types, functions, traits). A map literal, whose calls the parser
writes, still stops the checker.

**D17 — Numbers are computed exactly, floats not at all.** The checker is written in `Int`, so a
constant's integer value of any type is a `numeric::Wide` of its own: a sign and sixteen 16-bit
limbs, enough for the product of two 128-bit values, so `checked_add`, `checked_mul`, the
truncating quotient and remainder, two's-complement bit operators and shifts give the reference's
answer and its refusal (N0003, N0622, N0623) without passing `Int`'s range; an `Int` constant
keeps its value as an `Int`, any other as its decimal digits. A float is never computed: a float
constant has its type and no value, and where a value would decide something — a float compared,
converted to an integer, or a shift's amount — the checker stops rather than guess IEEE 754.

**D18 — Provenance is carried where it cannot refuse, before its flows are.** `N0372`
(`provenance::solve`, `FlowRule::LANGUAGE`) is the only diagnostic provenance gives, and only in a
function that declares its effects — a closure and an impl's or a trait's default method always do
— at a path it writes to, or at a call, a call through a value or a bound, or a closure's capture
that hands a value to what writes to one, when that value may carry a file's contents or what the
checker does not follow. So a compilation is carried, with no provenance diagnostic, when no
built-in of a restricted origin (`Origin::of_intrinsic`'s `File` and `Unknown` sources) is called
anywhere in it, when no restricted sink (`SinkClass::Path`) is, or when no function that declares
its effects reaches one — calling one, calling a function that reaches one, making a closure that
does, or calling through a value or a bound while anything reaches one. Anywhere else the checker
stops until the flows themselves are ported. The table (D14) gains each built-in's flow and
container use, generated, for that port.

**D19 — Attributes are configured over the whole compilation, before the checker.**
`compiler/configure.nz` ports `attributes::configure`: the reference runs it on the one program
`parse_map` makes of every module — each kind of item the modules' in load order — so its
refusals come kind by kind (records, enums, `@codec` inside them, traits, constants and statics,
impls, functions, then the second failure hook and each second handler), not module by module.
Every attribute is held to the vocabulary (`N0619`) and the board's functions to their shapes
(`N0627`); a declaration whose `@cfg` does not hold is removed from its module's tree, an impl
with its methods, so nothing later sees it. The conditions are asked of the host, as `nazm check`
asks them (`BuildConfig::host`): `run.sh` hands the driver the host's operating system and
architecture as Rust's `std::env::consts` names them — not `@cfg` in the driver, which the old
parser the selfhost suite runs over `compiler/*.nz` does not read — and without them a condition
on either stops the checker; outside a package no feature or profile is enabled. Its
refusals count as a syntax error's do for the missing `main` and the warnings, but a function
without a body after them is not one whose body failed to parse. A derived encoding is generated
into a module's text before it is parsed (`derive::expand`), which this front end does not do: a
record or enum that writes `@derive` stops the checker.

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
