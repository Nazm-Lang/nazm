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

**Non-goals:** a faster pipeline than Rust's (budgets are R2/R3 work at integration); targets
beyond the host's (task 4.2). The runtime generator, the object cache and debug information —
task 4.1's later items — were non-goals of the proposal and follow LIR (7).

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
- **A static atomic constant** carries its static's durable key in Core IR (`static`, added within
  `/3` on main at `ef1bf70a`), the `def` of its `statics` entry: `MConst.Cell` keeps the name its
  module spells (the MIR text prints `static NAME`, as Rust's) and that key, and LIR places the
  `statics` entry whose `def` is the key. A constant without the key is refused; the earlier
  rule (the function's module's static of that name, else the only one) is gone.
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

## The runtime generator (7.1)

`nazm build` links beside the program's own units the runtime's: the runtime itself (the parts
the program reaches), the platform entry, and per target the task switch, the task stacks and
stack budget, `errno`'s location, the files part's C facts, Windows' start-up, and under
`--heap-limit` the counted heap (`crates/nazm-cli/src/build/lower.rs`). They are LLVM text, the
runtime crate's (`crates/nazm-runtime`): some 10,000 lines of text constants and the rules that
choose and join them.

Decided: **the texts are data, the rules are ported.** `tests/runtime_text.py` writes `rtext.nz`
from the crate — every `const NAME: &str` of the hosted runtime as `rt_FILE_NAME()` (a raw
string, the 0.10.0 form), the declarations every unit carries (`os::declarations`, `GLOBALS`, the
declared `INVENTORY`), `SINGLE_THREAD_STATE`, the ABI revision, the `os` unit's format string,
and each inventory entry as Rust's `Debug` writes it (what the runtime digest covers, 7.2) — and
`run.sh` checks it is current, as `runtime.nz` is checked against `IMPORTS`. `rtgen.nz` is the
rules: `Needs` and `settled`, `emit_runtime` with the pool's hooks into the thread model's texts,
the entry, `emit_switch`, `emit_errno`, `emit_os` (the C facts per target, the network part),
`emit_stack`, `emit_init`, `count_heap` and `emit_heap`, and the units' order. While Rust is the
reference the texts cannot drift; once it is frozen (R8) `rtext.nz` is the source.

What the program reaches is noted where LIR imports a runtime entry, as `Lw::runtime` does: its
part (`runtime.nz` now carries each entry's part), the shims, the stack check, the string part
for `nz.strs_release`, I/O for `nz.argc`/`nz.argv`; `single` when `main` holds no `SpawnCap`,
`NetCap` or `ForeignCap`; the entry's `foreign` when any function calls C; a word per capability
`main` takes.

In a program of one thread (`single`) the reference counts are plain too (performance-leadership,
*Plain reference counts in a program of one thread*): `emit_runtime` applies
`plain_reference_counts` to the whole unit after every part is pushed — inside the functions of
`PLAIN_IN_ONE_THREAD` only (carried as data, `rt_plain_in_one_thread()`, extracted by
`runtime_text.py` from `core.rs`), from the `define` line to its `}`, each
`%r = atomicrmw add|sub ptr P, i64 1 monotonic|release` becomes `%r = load` / `%r.plain = add|sub`
/ `store`, and `  fence acquire` goes; every other line byte for byte. The pipeline prints no hosted
LLVM of its own, so `inline_reference_counting`'s copies have no counterpart here.

`pipeline runtime CORE.json [STACK HEAP]` prints the units, each after `;;; unit NAME`; `run.sh`
builds every input with LIR with `nazm build --emit-ir`, by default and with `--stack-size` and
`--heap-limit`, and compares every runtime unit byte for byte and that the build wrote exactly
those (`tests/units.py`). Only the host's units can be compared so: the other targets' are held
by the runtime digest (7.2), which covers every target's.

## The object cache (7.2)

`crates/nazm-cache`'s second store, as `objcache.nz`: an object's key is the backend's actual
input, never a summary of the front end's — `nazm.object-key/1` over the unit's bytes
(`nazm.llvm-input/1`), the backend's identity (`nazm.backend-tool/1`: the driver and its banner,
`InstalledDir` left out) and the configuration it resolved (`nazm.codegen-config/1`: the `-###`
job lines' arguments, this invocation's own paths and names, `-o`, `-main-file-name` and the
compilation directories left out, then `nazm-runtime-abi=REVISION+DIGEST`). Every digest is
BLAKE3 over length-framed fields, as `digest::Framed`. The runtime digest is BLAKE3 over the ABI
revision's four little-endian bytes, the whole runtime, the entry, the counted heap, every
target's per-target units and each inventory entry as Rust's `Debug` prints it — the generator
of 7.1 at every target, which the host's builds alone could not show. An entry is the header as
one JSON line (`nazm.object/1`, its fields in Rust's order) and the object; it is read whole, and
refused, every refusal a miss, by the first of Rust's checks it fails: no header line, not JSON,
another schema, a field not 64 lower-case hexadecimal digits, a key its recorded inputs do not
make, a key not the one asked for, a length, an object digest. It is published as Rust publishes
it, beside its final name and renamed over it. A key is a digest, so the paths made from it are
vouched for (`str_vouch`): the checker sees them as made from a unit's contents.

The probe the build makes (`clang -###` for an empty `probe.ll` at `-O0`, with only the
variables it passes) is `tests/probe.sh`; `pipeline backend` derives the identity and the
configuration from its report. `run.sh` keys every unit `nazm build --emit-ir` compiled from the
unit's bytes and that identity and configuration, and requires the store `nazm build` filled to
hold it under that key, the entry written again here byte for byte the one found; it compares the
runtime digest with the one `nazm build --provenance` records; and it breaks an entry eight ways
and reads it as asked for another key, each refused as expected.

Not ported: the Cranelift backend's key (its input is the unit's LIR digest with Cranelift's
version and printed settings), which R9's code generator replaces with its own; and the check
store (`nazm.check/5`), which is R2's.

## Debug information (7.3)

A `--debug` build's LIR (`nazm lir --ops --debug`, what both backends make line tables and
variables from): each function's `at FILE LINE`; before each MIR statement and terminator a
`loc FILE LINE COLUMN` of its span, a block switched to before anything is in it restating the
last one (N108); a borrowed parameter's address kept in a variable of its own; a `debug` line per
parameter or source binding of an integer word, `Bool` or string with a plain name — where it is
kept, its parameter position, its line, its kind and its lexical scope (its block's first and
last positions, or none in the function's body); and after each unit the files its positions
name, `; file INDEX PATH`. The DWARF the LLVM emitter attaches to LLVM text (`debug.rs`) is the
LLVM emitter's, not this stage's.

Every MIR statement's span now matters, not only a failing one's. MIR keeps Rust's: a call at
its callee (`callee_at`), a `match` binding's payload at the binding, a region's drops at its
block, a `match` arm's end at the arm, a `scope`'s join at its body, the function's end at its
body, a discarded value's drop at its expression; the shared unwind block, a failure cleanup
block and an orphan's `unwind` at no position; a thunk's spans its target's name. A local keeps
its declaring span and (a binding) its block's span, which MIR's text does not print.

Three facts Core IR did not give, now given only with `--with-sources` (read-only, additive, so
the default document and every digest are unchanged; `crates/nazm-cir/src/print.rs`): each
module's `file` (the index a position names) and `path` (the loader's name for it), a call's
`callee_at` (calls, trait calls, indirect calls) and a `match` binding's `at`. Without them the
positions, file names and those statements' spans could only be guessed.

## Failures by unwinding (8.1)

The reference crosses a call's failure by unwinding on the hosted targets (performance-leadership
4.2, `docs/architecture.md` §7.115). LIR's text is unchanged — a call still names its `failed`
value, now only the condition of the branch ending its block, which `ops.nz`'s lowering already
obeys — so no digest here moves. This pipeline prints no LLVM of its own and does not validate
LIR (its validator is MIR's, `verify.nz`), so the validator's new rule has no place here yet; it
belongs with the LIR validator when one is ported. What does move is the runtime: ABI revision 34
(an image now links the system's unwinder) and `nz.exc`, the exception object a unit defines, an
ordinary global in a one-thread program (`SINGLE_THREAD_STATE`). `rtext.nz` is regenerated from
the runtime crate (`tests/runtime_text.py`); the runtime digest every object key carries follows.

## Every target, narrowed to macOS (8.2)

`rewrite-in-nazm` task 4.2 asks for every target in `docs/support.md`. The copyright holder
narrowed it (2026-10-15) to macOS: `aarch64-apple-darwin` and `x86_64-apple-darwin` (built and
run under Rosetta on an Apple silicon host). Linux, Windows, WebAssembly, WASI, Android, iOS and
the boards are **deferred until after 1.0**, not ported now; the pipeline refuses them by name.

`pipeline --target TRIPLE MODE ...` takes the target as `nazm lir --target` and `nazm build
--target` do; without it, `aarch64-apple-darwin`. The two targets share LIR's data layout and
instruction text; the target is named in the LIR document and chooses the runtime's per-target
units (`x86_64-darwin`'s task switch and stack entry). `run.sh` with `TARGET=x86_64-apple-darwin`
passes `--target` to both sides and compares everything but the object cache (whose backend
probe is the host's).

## Order of work

1. This proposal.
2. Reading Core IR JSON into the model of decision 1 and 2 (`cir.nz`, `types.nz`).
3. MIR for integer functions, calls, branches and loops; `fib` and the arithmetic corpus identical,
   digests included.
4. Ownership: strings, sequences, records, enums, `match`, drops and failure edges, scopes and
   spawn; generic instances.
5. The MIR validator (`verify.nz`): structure and typing, then the ownership dataflow.
6. LIR: data layout, units, symbols and linkage, record and enum layouts, instructions.
7. Task 4.1's later items: the runtime generator, the object cache, debug information.

## Risks / Trade-offs

- **Speed:** measured on the largest input, this pipeline's own Core IR (3.9 MB, ~500 functions):
  2.3 s against `nazm mir`'s 0.09 s, which includes checking. Several whole-document walks (the
  type table, closures, the plan, call cycles, the array fixpoint) over `@std/json`'s linear key
  lookups are the likely cost; a budget is R3 work at integration. Core IR /3 removed the type
  table and array walks.
