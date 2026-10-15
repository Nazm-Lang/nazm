## Context

The Rust stage, at `c73dab27`: `crates/nazm-mir` (lowering 2.4k lines, instance planner 0.5k,
validator 1.0k, text/print/digest 0.5k), `crates/nazm-lir` (lowering, layout, symbols, ABI, debug
info, 3.9k) and the drivers in `crates/nazm-core`. MIR is printed by `nazm mir` as `nazm.mir/1`:
per function its identity, effects, parameter count, result type, executable digest (BLAKE3 over a
canonical text), locals and blocks, every statement and terminator as one canonical text line.

The input is `nazm core-ir --json` (`nazm.core-ir/3`; `/2` until `dcb43aab`): per function its definition key, module,
type-parameter count, parameter count, result type, effects, locals, constants and body, with every
expression typed by its spelled type (`Int`, `Vec[Str]`, `enums.nz::enum Shape`,
`@core/prelude::enum Option[Int]`, `$0`).

## Goals / Non-Goals

**Goals:** MIR and LIR identical to the Rust toolchain's for every input both can process; every
difference either fixed or shown meaningless; the port reads as the rules of §7.42, not as Rust
transliterated.

**Non-goals:** a faster pipeline than Rust's (budgets are R2/R3 work at integration); the runtime
generator, the object cache and debug information (task 4.1's later items); targets beyond the
host's (task 4.2).

## The data model

The language has no recursive types through `Vec` (`N0359`), so a tree is either `Box`/`List` or an
arena. Decisions:

1. **Core IR stays in its JSON arena.** `@std/json` already parses the document into an arena of
   nodes; an expression, statement or block is a node index, read through typed accessors in
   `cir.nz` (`op(c, e)`, `ty(c, e)`, `kid(c, e, "lhs")`, …). Function headers — definition, module,
   name, type parameters, parameters, result, published effects, locals, constants, body — are
   decoded once into a `Vec[CirFn]`, with a `HashMap[Str, Int]` from definition key to position.
   Converting the whole tree into a second arena would copy every node for no rule that needs it.
2. **Types are their Core IR spellings** (`Str`), parsed on demand by `types.nz` into a head and
   arguments. The MIR key (`E.m.enums_2Enz.Shape`, `Vec(Int)`), substitution of `$N`, and the
   *managed* classification are functions of the spelling and the type table.
3. **MIR is a real data model** (`mir.nz`), not text: `Operand`, `MConst`, `Rvalue`, `Stmt` and
   `Term` are enums; a `Block` holds a `Vec[Stmt]`, its terminator and its origin; a `MirFn` holds
   its identity, locals (`Vec[LocalDecl]`) and blocks. The validator (slice 5) and LIR (slice 6) read
   it. Text (`text.nz`) is one rendering shared by the printed form and the digest, as in Rust, so
   the two cannot disagree.
4. **The lowering state** is one record of handles (`Vec`, `Ints`, `HashMap`) — a record is a value,
   so scalar state (the current block, the shared unwind block) lives in an `Ints` register file.
   The cleanup stack is two parallel `Ints` (kind, local); the open scopes an `Ints`; the
   failure-cleanup blocks a `HashMap[Str, Int]` keyed by the open scopes' spelling.

## The rules, as ported

The same as §7.42 and `crates/nazm-mir/src/lower.rs`; listed so a reviewer can check each:
functions numbered non-generic first in Core IR order, then instances sorted by identity, then
thunks; parameters, then Core IR's locals, then temporaries named `what#N`; the stack check first in
a function in a call cycle (Tarjan over direct, unspawned calls; any indirect call counts) or with a
frame of 4096 bytes or more; the result temporary; operands borrowed where they are unless a later
sibling could reassign them (a region in a later operand), a field of a local borrowed into an
unowned temporary; everything else through an owned temporary dropped after its use; a value `if`
as then/else/join, a statement `if` without `else` branching to the join; a loop as head, body,
exit; `match` through an owned scrutinee temporary, payload bindings owned in each arm, the
scrutinee dropped at the join; `scope` as open, body, join, then a failure check; `break` and
`continue` dropping down to their loop's marker; `return` and fall-through dropping everything; a
failure edge through the joins of the scopes open at it to one unwind block that drops every managed
non-parameter local except borrowed temporaries; blocks nothing reaches removed and the rest
renumbered in creation order.

## Core IR /3, as this stage reads it

The stage reads `nazm.core-ir/3` only (main at `dcb43aab`); the reconstructions it needed under `/2`
are gone. What each `/3` fact replaced:

| Fact | `/3` member | Replaced |
| --- | --- | --- |
| Records' and enums' fields and variants | `types.records` / `types.enums`: the entries whose `type` is their `definition`, fields over the definition's own parameters `$?J` | The table recovered from constructions, field reads and `match` bindings (`types.nz`) |
| Function and array types | spelled (`fn(Int) -> Str`, `[Int; 4]`) | `refine.nz`'s array fixpoint (deleted). `read_ty` reads a function type as `?` and an array as `?array[ELEM, LEN]`, the spellings MIR's text needs: `?` for a local or result of either, `Array(ELEM,LEN)` inside a key (`tkey`, Rust's `type_key`) |
| A closure's captures | `captures` (the locals they arrive in) | "the locals after the parameters" |
| The impl a `call_trait` reaches | `impls`: the row of the method's trait whose `self` is the type or its definition; its `function`, and `type_args` read with `self` as the concrete type (a default body) or bound from the function's `Self` parameter (a generic impl) | Matching the impl functions' names `Trait/Self/method` |
| Default bodies at every concrete impl | `impls[].methods[].default` | Not recoverable before: now planned as Rust plans them, from every impl of a non-generic type |
| Derived `Hash` | `derived_hash` | "a trait named `Hash` with no impl" |
| `@export` | `attributes.export` | The digest's `export SYM` line: closes `desktop/counter` and `check/export-numbers` |

Two facts Core IR does not carry and the stage derives, both shown equal to Rust's on every input:
a function value written as `callback_address("SYM")` is a use of the `@export`ed function `SYM`
(the checker records it as a function value, so MIR makes it a thunk); and a closure of a generic
function is planned at its owner's arguments wherever the owner's instance makes it.

## What LIR needs beyond `/3`

`nazm lir` (`nazm.lir/2`) prints, per unit, its functions' symbols, linkage and signatures, its
externals, the layouts of the records and enums it mentions — and its instruction-level text with
that text's digest (`lir`). The tables are functions of MIR and `/3` (`modules`, `types` ids and
`display`, `pub`, `attributes`, `impls`), with one exception and one open point:

- **Source positions.** The instruction text embeds each failure's message with its
  `file:line:column` (`nazm: error[N0400]: … at fib.nz:2:33`). Core IR carries byte spans
  (`"at": "fib.nz:313..317"`), not lines and columns: the stage needs each module's source text
  (or a line table) to convert them. Requested: a `source` member per `/3` module, or a read-only
  `--with-sources` option on `nazm core-ir`. Decided: `--with-sources`, read-only, giving each
  `/3` module a `source` member only on request (`crates/nazm-cli`, `crates/nazm-cir`). The MIR
  this stage builds carries each statement's span beside it (`Block.ats`, `term_at`; not printed):
  the span of the innermost Core IR node being lowered, which is what Rust's MIR gives the
  statements whose failures LIR names. A stack check names the function's name, which Core IR
  does not span: found as `fn NAME` within the function's span in its source.
- **Types the checker never interned.** A unit lists its records and enums by the checker's ids.
  A concrete type that appears only once MIR substitutes an instance's arguments (a `Pair[T]`
  inside `f[T]`, called at `Int`) has no `/3` entry and so no id; the Rust lowering interns it then.
  Until a difference shows which order that is, such a type sorts after every listed one, in the
  order MIR first meets it. No input of the 1009 with LIR shows one: every unit's records and
  enums agree, ids included.

The tables are compared now (`run.sh`, *lir tables*): `nazm lir`'s document with each unit's
`lir` member — the instruction text and its digest, 6.3 — taken out, every other byte compared.
Function types are spelled inside the stage as `?fn(PARAMS) -> RET` (MIR's text still names them
`?`), so a `Vec[fn(Int) -> Int]` is named as the checker shows it.

## Instruction-level LIR (6.3)

`ops.nz` builds each unit's module as text, in the order Rust's `Lw` builds it — functions
declared in MIR order, then every runtime import, datum and message in first-use order — because
the printed order of symbols is that order. A function is built as Rust's `Fb` builds it: block 0
initialises the locals, one block per MIR block follows, and every guard opens its two blocks
where it stands. Values are numbered in emission order. Nothing is guessed: a construct not ported
yet stops the unit with a message naming it, and `run.sh` counts it as a difference.

`run.sh` compares three things for an input with LIR: the tables, the instruction text against
`nazm lir --ops`, and — when the text agrees — the whole `nazm lir` document, the text's BLAKE3
digests included. Its inputs are the base set (the brief's, `compiler/`, the pipeline itself)
and the front end's test programs (`toolchain/frontend-tests/inputs`), which moved out of
`compiler/tests/inputs`; it prints the summary over both, and a text that differs without a
stop is counted apart and always shown.

What 6.3.5 found, and decided:

- **`Ints` and `Strs` are vectors.** Since 5b.18 the checker interns `Ints` and `Strs` as
  `Vec[Int]` and `Vec[Str]` (Core IR spells them so), and LIR releases a `Vec[Str]` as any
  vector of owned elements (`nz.vec_release` with `nz.h.vel.Str`), never `nz.strs_release`.
  The stage's earlier rule (a `Vec[Str]` released as `Strs`) agreed only on inputs that never
  dropped one; it made two front-end inputs' text differ, and is gone.
- **One helper per symbol.** A `Box`, `Chain` or `Cell` is its `Vec`, so their helpers share a
  symbol; the stage keys helpers by symbol, as Rust's keying by `Ty` comes to.
- **A thunk's spans are its target's** (Rust's `thunk_of`): its stack check names the target
  function, found from the thunk's name `NAME#value@HOME`.
- **A static atomic constant** is spelled by its name alone in Core IR (`static NAME`): the stage
  takes the static of that name in the function's module, else the only one of that name, and
  stops if there are several.
- **Compile-time constants** are computed here: a float literal's IEEE bits (`float_parse_raw`,
  then its bits), an integer of any kind as its class's two's complement (decimal arithmetic
  past 64 bits), FNV-1a of a variant's name for an enum's hash (`UInt64` wrapping), and the
  powers of two `float_fits` compares with.
- **A generic channel's helper** was named by the checker's channel id (`nz.h.vel.chan.N`).
  Core IR does not carry it, and it is not a function of the type: `Chan[Int]` was `chan.0` in
  `select-004` and `chan.6` in `select-001` (the built-ins' `Chan[T]`s are interned first, in a
  session-dependent order). Decided by the coordinator (6.3.6): LIR names it by its element's
  key, as a `Vec`'s is (`chan.KEY`, `KEY` encoded as `vec.KEY`'s: `nz.h.vel.chan.Int`), a
  change to `crates/nazm-lir` (`Lw::type_name`, from MIR's `Types::chans[id].elem_key`). The
  symbol is now durable across sessions. No revision moves: an object's key is its unit's LIR
  digest (§7.106), which covers the symbol's text, and the `nazm.lir/2` document's shape is
  unchanged; no mutant's `find` text named the old line. Held by `lir::a_generic_channels_helper_is_named_by_its_element`
  (`crates/nazm-cli/tests/lir.rs`) and its mutant `r3-a-generic-channel-s-helper-named-by-the-checker-s-id`, caught.

## Order of work

1. This proposal.
2. Reading Core IR JSON into the model of decision 1 and 2 (`cir.nz`, `types.nz`).
3. MIR for integer functions, calls, branches and loops; `fib` and the arithmetic corpus identical,
   digests included.
4. Ownership: strings, sequences, records, enums, `match`, drops and failure edges, scopes and
   spawn; generic instances.
5. The MIR validator (`verify.nz`): structure and typing, then the ownership dataflow.
6. LIR: data layout, units, symbols and linkage, record and enum layouts, instructions.

## Risks / Trade-offs

- **Speed:** measured on the largest input, this pipeline's own Core IR (3.9 MB, ~500 functions):
  2.3 s against `nazm mir`'s 0.09 s, which includes checking. Several whole-document walks (the
  type table, closures, the plan, call cycles, the array fixpoint) over `@std/json`'s linear key
  lookups are the likely cost; a budget is R3 work at integration. Core IR /3 removed the type
  table and array walks.
