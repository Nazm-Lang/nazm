## Context

The Rust stage, at `c73dab27`: `crates/nazm-mir` (lowering 2.4k lines, instance planner 0.5k,
validator 1.0k, text/print/digest 0.5k), `crates/nazm-lir` (lowering, layout, symbols, ABI, debug
info, 3.9k) and the drivers in `crates/nazm-core`. MIR is printed by `nazm mir` as `nazm.mir/1`:
per function its identity, effects, parameter count, result type, executable digest (BLAKE3 over a
canonical text), locals and blocks, every statement and terminator as one canonical text line.

The input is `nazm core-ir --json` (`nazm.core-ir/2`): per function its definition key, module,
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

## Gaps in `nazm.core-ir/2`, and what the stage does until they close

| Needed by MIR | In Core IR /2? | Until `nazm.core-ir/3` |
| --- | --- | --- |
| Whether a record or enum is managed; its fields' types (temporaries, frame size) | No type table | Recovered from uses: every `record`/`variant` construction, `field` read and `match` binding gives a field's type; a generic definition's fields are kept as templates over its parameters when the construction's arguments are its function's parameters. Correct whenever every type the program uses is constructed somewhere in it, which holds for the inputs so far |
| Function and array types | Printed `?` | MIR prints both `?` too, so only ownership needs them. A `?` is taken as a function type (managed) unless `refine.nz` recovers an array from how it is made (`[…]`, `[v; n]`, an index, a binding of one), carried to a fixpoint into parameters (from what callers pass) and results (from what bodies return) — spelled internally `?array[ELEM, LEN]`. Every input agrees on this since, `@std/blake3` included |
| A closure's captures and `closure` flag | No | Recovered: a closure's code is a function `NAME#K`, its captures the locals right after its parameters, as many as the `closure` expression lists |
| Thunks for function values | Not in Core IR (Rust synthesises them) | Synthesised the same way, as Core IR nodes added to the document: `env`, the target's parameters, a body that is the call |
| `@export` symbol | No | The canonical text names it (`export SYM`), so the digest differs: 2 inputs differ (`desktop/counter`, `check/export-numbers`). `@section`, `@interrupt`, `@task`, `@on_failure` are not in the MIR text; LIR needs them |
| Statics (`Atomic` cells) | No | Matter for LIR only |
| The impl a `call_trait` resolves to at a type, and derived `Hash` | No | Recovered from the impl functions' names (`Trait/Self/method`, first parameter unified with the concrete type); no impl and the trait `Hash` means derived. A trait's *default* body at a type (L11a) is not recoverable; no input uses one |
| Which calls close a call cycle | Calls yes; a call through a bound is not marked | A `call_trait` counts as an indirect call, as the Rust facts treat it (every generic body making one checks its stack) |

LIR (`nazm lir`, `nazm.lir/2`) needs more than MIR does, and none of it is recoverable from Core IR
/2: a function's visibility (`pub` makes it `hidden` rather than `internal`); the type table **in the
checker's own order** (a unit lists its records and enums by the checker's type ids, which interning
decides — `Point`, then `@std/json`'s records, then a tuple, then `HashSet[Point]`, in one input),
with each type's display name (`HashMap[Point, Bool]`, as `Types::show` writes it) and its fields'
names, whose order by name is the layout order; the modules in load order with each one's source
name (`@std/hashset` has a unit with no functions); the attributes; the statics; and the source
text of every module, which the instruction-level text needs for the `file:line:column` of each
failure message. The instruction-level text itself (`crates/nazm-lir/src/op/`, ~6k lines) is the
bulk of slice 6.

The request to the coordinator is `nazm.core-ir/3`: a `types` table in the checker's id order (each
record and enum: definition key, display name, type arguments, fields in declaration order with
their names and types, canonical order; each variant likewise), function and array types spelled
(`fn(Int) -> Str`, `[Int; 4]`), a function's visibility, `closure`, `captures` and attributes
(`@export` symbol, `@section`, `@interrupt`, `@task`, `@on_failure`), the statics, the modules in
load order with their source names, and the impl table (trait, self type, method, target
definition and type arguments, defaults included). These are facts the checker already has; printing them changes no decision.

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

- **The type table recovered from uses** is a workaround, not a design: it is replaced by Core IR
  /3's table the day it exists, and every input it cannot serve is counted as a difference.
- **Speed:** measured on the largest input, this pipeline's own Core IR (3.9 MB, ~500 functions):
  2.3 s against `nazm mir`'s 0.09 s, which includes checking. Several whole-document walks (the
  type table, closures, the plan, call cycles, the array fixpoint) over `@std/json`'s linear key
  lookups are the likely cost; a budget is R3 work at integration, once Core IR /3 removes the
  recovery walks.
