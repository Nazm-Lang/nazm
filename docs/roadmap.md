# Roadmap: native compilation, then self-hosting

**Status: M1a is implemented; M1b onward, M2 and M3 are plan.** The direction this serves
is recorded in [`direction.md`](direction.md); the design it must stay consistent with is
[`architecture.md`](architecture.md); the semantics it must preserve are in
[`spec.md`](spec.md).

Ordering principle: **a trustworthy core before a bootstrap.** Self-hosting is the
long-term objective and it is also the easiest possible excuse for a rewrite. It appears
here as M3, after the language can actually express a compiler (M2), which is after
anything at all can be compiled (M1).

---

## Where the line is today

Established by inspection and execution on 2026-09-20, not by document:

| | |
| --- | --- |
| `nazm run` | **Interpreted.** `source → tokens → AST → check → tree-walking eval`. The whole language below |
| `nazm build` | **Native, for arithmetic only (M1a).** `source → tokens → AST → check → LIR → LLVM IR text → clang → executable` |
| Language | Functions, recursion, `let`/`let mut`, assignment, `while` + `break`/`continue`, `return`, `if`/`else`, arithmetic and comparison over `Int`, `Bool`, `Str` |
| Absent | Collections, user-defined types, pattern matching, string concatenation, modules, I/O, generics, concurrency, floats |
| Toolchain | Rust 1.98.1 pinned, host `aarch64-apple-darwin`; Apple clang 21.0.0; `cranelift` is **not** a dependency |

The two commands cover different amounts of the language, deliberately: `nazm build`
compiles a narrow subset and refuses the rest **by name**, while `nazm run` continues to
execute everything. A program `nazm build` refuses is not a wrong program.

---

## M1 — First native vertical slice

> **M1a and M1b are implemented (2026-09-20).** A `.nz` program in the subset below
> compiles to a native executable that runs with no interpreter present and agrees with
> the interpreter on every case tested. M1a was integer arithmetic; M1b added `Bool`, the
> six comparisons, and value-position `if`/`else` with branch-local bindings and nesting.
> **M1 as a whole is not complete**: there are still no user functions, no loops, no
> mutation, and no `Str`. See *M1b subset* below for the accepted/rejected table and
> *M1a/M1b status* at the end of this section.

### Objective

A `.nz` program in a defined subset compiles to a native executable that runs with no
interpreter present, and produces observably the same result as the interpreter.

### Existing components reused

Everything in front of the AST, unchanged:

| Reused | Path |
| --- | --- |
| Lexer, parser, two depth bounds | `crates/nazm-syntax/src/{lexer,parser}.rs` |
| AST and spans | `crates/nazm-syntax/src/ast.rs`, `crates/nazm-span` |
| Checker, `Completion::{Value,Unit,Diverges}` | `crates/nazm-core/src/check.rs` |
| Diagnostics, stable codes, `--json` | `crates/nazm-diag` |
| **The interpreter, as the differential oracle** | `crates/nazm-core/src/eval.rs` |
| **The 98-case behavioural suite, as the conformance corpus** | `crates/nazm-core/tests/language.rs` |

The interpreter is not replaced and not deprecated. It becomes the reference that the
backend is tested against, which is the role `architecture.md` §1 already assigns to a
reference interpreter.

### The representation

**One** new IR. Not eight.

`nazm-lir` — a typed, flattened, control-flow-explicit form:

- functions with typed parameters and a return type;
- **slot-indexed locals**, so name resolution is finished before lowering and the backend
  never sees a string;
- basic blocks with explicit terminators (`Br`, `CondBr`, `Ret`);
- instructions over `Int` / `Bool` / `Str` only.

What is deliberately *not* built: HIR, Core, Core′, MIR, Mono MIR. There are no generics
to monomorphise, no traits to turn into dictionaries, no regions to infer, and no layouts
to select — scaffolding those layers now would produce five empty pass-throughs and a
diagram that matches. `architecture.md` §2 stays the target shape; M1 does not pretend to
have reached it.

Lowering `AST → LIR` lives in `nazm-lir`, because it needs the checker's output and
nothing below it. Structured control flow becomes blocks here: `while` becomes
header/body/exit, `if` becomes then/else/join, and `break`/`continue`/`return` become
branches to blocks already on a stack — which is exactly the information `Completion`
already computes.

### Backend and target

**One backend: LLVM IR emitted as text, assembled by `clang`. One target:
`aarch64-apple-darwin`.**

Justified against the existing decisions rather than chosen by taste:

- `architecture.md` §1 says *"Do not link against LLVM in year one. Emit LLVM IR as text
  and shell out to `clang`/`opt`."* This is that, and nothing more.
- It adds **zero Rust dependencies**. `Cargo.toml` already states the rule — *"adding a
  dependency before the crate that needs it exists is how a workspace acquires a build it
  cannot explain"* — and `xtask check`'s codegen-reach gate exists to keep backends behind
  the LIR contract. Text emission needs no entry on that allowlist.
- It avoids an MSRV question. `cranelift` 0.135 declares 1.95.0 against a 1.98.1 pin, so
  it would fit — but `architecture.md` §8 is explicit that declared MSRVs are *"not that
  the set resolves and builds together"*, and M1 does not need to find out.
- `architecture.md` §1 also says *"two backends is two sets of semantics bugs"* and
  *"release is **always** LLVM"*. Starting with the backend that release must use anyway
  means the semantics being nailed down are the ones that ship.

Cranelift arrives when the jobs only it does — JIT, comptime evaluation, hot reload —
actually exist. That is a Stage 7 question, not an M1 one.

**Verified locally, 2026-09-20**, before committing to the path: `clang` 21 compiles a
hand-written `.ll` using `@llvm.sadd.with.overflow.i64`, branches to a trap block, writes
a diagnostic to fd 2, and exits 2. No `llc` or `opt` binary is needed; `clang` consumes IR
text directly. **A hand-written probe establishes only that the tool works**; what M1a is
tested on is `.nz` files compiled through `nazm build` and executed.

The adviser's ordering decision, recorded: **LLVM IR text → `clang` is the initial
backend, not a permanent rejection of Cranelift.** Cranelift arrives when the jobs only it
does — JIT, comptime evaluation, hot reload — exist to justify a second set of semantics
to keep in agreement.

### Semantics that must survive lowering

These are commitments in `spec.md`, and each has a way of being got wrong quietly:

| Commitment | The wrong way to lower it |
| --- | --- |
| Overflow **traps** in every build | `add nsw` — which makes overflow *undefined*, the exact opposite. Must be `llvm.sadd/ssub/smul.with.overflow.i64` plus a branch |
| Division by zero is an error | Emitting bare `sdiv`, which faults |
| `Int::MIN / -1` traps | Also a hardware fault on `sdiv`; needs its own check |
| **`Int::MIN % -1` is `0`, not a trap** | Inheriting `srem`'s fault, which would give `%` a second failure case the language says it does not have |
| `-Int::MIN` traps | `sub 0, x` without a check |
| Argument order is left to right | Letting the emitter reorder operand evaluation |
| `return`/`break`/`continue` leave from value position | Requiring a value from a block that diverges |
| Overflow checks in release too | `[profile.release]` already sets `overflow-checks = true` for the *host*; the emitted program needs the same property by construction |

`Str` in M1 is deliberately tiny: there is no concatenation and no I/O in the language, so
no operation produces a new string. A `Str` is a pointer and a length to a constant in the
binary; `==`/`!=` is a length check and `memcmp`; `main` returning `Str` writes it to fd 1.
No allocator, no ownership question, no cleanup. That is why `Str` is affordable now and
growable collections are not.

### CLI

`nazm run` **does not change.** It stays the tree-walking interpreter, with the same exit
codes (`0` ran, `1` did not type-check, `2` failed at runtime, `3` no such file) and the
same `--max-iterations` / `--max-depth` budgets.

Added:

```sh
nazm build <file> [-o <path>]   # → native executable; same exit codes for check failure
```

A native program has no interpreter budgets, so `--max-depth` and `--max-iterations` are
rejected on `build` by name rather than accepted and ignored. Whether `nazm run` ever
gains a `--backend` flag is deferred; two ways to run one program is a decision to make
once there is something to compare.

### Prerequisites

- None in the language. M1 compiles the subset that already runs.
- `clang` on `PATH`. `nazm build` must detect its absence and say so, with the same
  discipline as `N0404` (*"could not start the evaluator"*) — an environment failure
  reported as one, not blamed on the program.
- A placement for the new crates in `xtask/src/rules.rs`, or the layering gate fails —
  which is the gate working.

### Explicit exclusions

Scaffolding the other IR layers · a second backend · any target but `aarch64-apple-darwin`
· optimisation passes (`-O0`, and the emitted IR is not expected to be fast) · separate
compilation · a linker crate · `nazm-rt` · incremental compilation · `salsa` · `cstree` ·
replacing the interpreter · any new language feature.

### Acceptance criteria

1. `nazm build examples/max.nz -o /tmp/max && /tmp/max` exits 0 and prints `25` — the same
   as `nazm run examples/max.nz`, with no `nazm` process involved.
2. All four `examples/*.nz` do the same.
3. **Differential conformance:** for every case in `crates/nazm-core/tests/language.rs`
   inside the native subset, the native binary and the interpreter agree on the observable
   result — value printed, exit code, and for a runtime failure the diagnostic code.
   Disagreement fails the suite.
4. The trapping cases are checked natively and individually, not as a group: overflow on
   `+`, `-`, `*`, unary negation of `Int::MIN`, `/` by zero, `%` by zero, `Int::MIN / -1`
   traps, and **`Int::MIN % -1` is `0`**.
5. Anything outside the subset is refused by name at `build` time, in the style the parser
   already uses for `loop` and `struct` — never silently miscompiled.
6. `cargo lint`, `cargo xtask check`, `cargo test --workspace` stay green.

### Evidence required for completion

- The differential runner's output, naming how many cases ran natively and how many were
  excluded from the native subset **and why** — an exclusion list is part of the result,
  not a footnote.
- The emitted `.ll` for one arithmetic function, in the commit message or a doc, so a
  reviewer can see that the overflow path is an intrinsic and a branch rather than `nsw`.
- Catalogued mutations for the traps: at minimum, one that replaces a checked add with a
  wrapping one and one that drops the `Int::MIN / -1` check. Both must be caught.
- A statement of what M1 does **not** establish: no performance claim, one target, one
  backend, no optimisation.

### M1d subset — the accepted/rejected table

Written before implementation, for the same reason as M1b's and M1c's: a backend broadens
by accident when an AST node is handled generically. Anything refused **still runs under
`nazm run`**.

| Construct | `nazm build` | Note |
| --- | --- | --- |
| everything in M1a, M1b and M1c | accept | unchanged |
| **`let mut` bindings of `Int` and `Bool`** | **accept** | M1d |
| **assignment to such a local** | **accept** | M1d. A statement with no value, exactly as `docs/spec.md` has it |
| **`while` with a `Bool` condition** | **accept** | M1d |
| **`let`, `let mut`, assignment and nested `while` inside a loop body** | **accept** | M1d |
| **all of the above inside any user-defined function** | **accept** | M1d. Not only `main` |
| **a loop condition or body that calls a supported function** | **accept** | a call is an expression like any other; the cycle refusal is unchanged |
| **an assignment whose value is an `if`/`else` expression** | **accept** | value-position `if` is M1b's, and assignment does not restrict it |
| immutable parameters | unchanged | the language has no mutable parameter; `fn f(mut x: Int)` is refused by the parser |
| existing scope and shadowing rules | unchanged | a loop body is a scope; an inner `let` shadows and does not overwrite |
| existing final-expression function results | unchanged | a function still produces its body's tail expression |
| checked arithmetic and runtime diagnostics | unchanged | the same guards, in a loop as anywhere else |
| **`break`** | **refuse** (`N0101`) | M1d compiles the loop, not the loop exits |
| **`continue`** | **refuse** (`N0101`) | same |
| **`return`** | **refuse** (`N0101`) | unchanged from M1c |
| **statement-position `if`** | **refuse** (`N0101`) | unchanged. It arrives as `Stmt::Expr`, which is refused; a loop body that *ends* in one arrives as the body block's tail, and that is refused too |
| **a loop body ending in an expression** | **refuse** (`N0101`) | a loop body produces nothing, so a tail there is either a discarded value (the checker's `N0300`) or a statement-position `if` (above) |
| recursion, direct or mutual | refuse (`N0101`) | unchanged, and see *Recursion* below |
| `Str`, bare blocks as expressions, expression statements | refuse | unchanged |
| assignment to an immutable binding; assigning the wrong type; a non-`Bool` loop condition | **not the backend's business** | the checker reports `N0310`, `N0300`, `N0301`. A backend limitation must never be reported as a language error, and the reverse is equally wrong |

#### The native execution contract for loops

**A compiled Nazm program can loop forever.** `nazm build` emits a back edge and nothing
else: there is no iteration counter in the generated code, no budget, and no diagnostic
for a loop whose condition never becomes false. Such a program runs until it is killed.

This is a *difference from `nazm run`*, and it is stated rather than inherited.
`docs/spec.md` already says the iteration and depth limits are the interpreter's and not
the language's — "Nazm bounds neither iteration nor recursion… A different implementation
may choose differently." The native backend is that different implementation, and the
choice it has made for M1d is **no budget at all**.

What that forbids, for this increment:

- **No claim that the two implementations have the same resource behaviour.** They agree
  on the *value* a terminating program produces. They do not agree on what a
  non-terminating one does: `nazm run` reports `N0402` after 10,000,000 iterations;
  the native program does not stop.
- **No silent inheritance of `--max-iterations`.** The flag belongs to the interpreter and
  is not read by `nazm build`.
- **No new public flag introduced in passing.** A native execution budget is a language-
  and CLI-visible decision with its own diagnostic, and it gets its own increment or none.
- **Every test that runs a compiled program gets a deadline**, and a test that hits the
  deadline is a *failure* — or, for the one mutation case that expects it, a specifically
  named expected outcome. A timeout is never recorded as successful execution.

Differential testing therefore compares only programs that terminate inside the
interpreter's budget. That is a real restriction on what M1d's evidence covers, and it is
the honest boundary of it.

#### Recursion: pending a native execution contract, not a counter

This supersedes the three-candidate framing in `review/m1c/HANDOFF.md`, which presented a
call-depth counter as the recommended answer. The correction, point by point:

1. **A call-depth counter alone does not guarantee a diagnostic before the native stack is
   exhausted.** The counter bounds the number of frames. Whether that number of frames
   fits is a question about frame size and about how much stack the process actually has,
   and a counter answers neither. A budget of 512 says nothing until both are known.
2. **The interpreter does not rely on a counter alone, and never did.** `--max-depth` is
   one half of a pair: `nazm_core::eval::on_interpreter_stack` runs the whole pipeline on
   a thread whose stack it sizes *from the budget it is about to enforce* — `24 MiB +
   max_depth × 32 KiB`, with the per-call figure measured rather than guessed, and with
   `N0404` when the host will not grant the reservation. It is the combination that makes
   `N0403` arrive before the stack runs out. Copying only the counter into generated code
   would copy the half that does not carry the guarantee.
3. **Neither quantity has been established for native code.** `nazm build` produces a
   process whose main-thread stack is the platform's (8 MiB on macOS, typically 8 MiB and
   configurable on Linux), which this project has not measured, and the frame size of a
   generated Nazm function has not been measured either. Emitting a bound derived from
   neither would be a guarantee with no basis.
4. **The specification already separates these.** `docs/spec.md`, *The iteration and depth
   limits are the interpreter's, not the language's*: the limits are an implementation's
   guards, not language semantics. So the native backend is not obliged to reproduce
   `N0403`; it is obliged not to promise a diagnostic it cannot deliver.

**Therefore:** the call-cycle refusal stays exactly as it is, and recursion is recorded as
**pending a native execution contract with supporting evidence** — measured stack
availability, measured frame requirements, and a decided answer to what a compiled program
does when it exhausts the stack. It is *not* recorded as "remove the cycle detection and
add a counter". No runtime crate and no stack-probing mechanism is added now.

### M1e subset — the accepted/rejected table

Written before implementation. Anything refused **still runs under `nazm run`**.

| Construct | `nazm build` | Note |
| --- | --- | --- |
| everything in M1a–M1d | accept | unchanged |
| **`break`** | **accept** | M1e |
| **`continue`** | **accept** | M1e |
| **statement-position `if`, with `else`** | **accept** | M1e |
| **statement-position `if`, without `else`** | **accept** | M1e. Always has a fall-through path: the condition may be false |
| **an `if` as a loop body's tail** | **accept** | M1e. This *is* statement-position `if` — the parser puts a trailing `if` in the block's `tail`, and `check_while` accepts it when its completion is Unit or Diverges |
| **`break`/`continue` inside a value-position `if` branch** | **accept** | M1e: a `let` initialiser, an assignment right-hand side, an arithmetic operand, a call argument, a nested `if` condition. All five check and run today |
| **both branches of a statement `if` transferring** | **accept** | no continuation is emitted, and the statements after are already `N0313` |
| **one branch transferring, one falling through** | **accept** | only the falling-through branch reaches the continuation |
| **a value-position `if` with one transferring branch** | **accept** | the value comes from the branch that reaches the join; the `phi` has one incoming |
| **`break`/`continue` in a loop's condition** | **accept**, targeting the **enclosing** loop | not this loop's. See *Where a loop target binds* below |
| `return` | **refuse** (`N0101`) | unchanged, and still refused inside an otherwise supported conditional |
| recursion, direct or mutual | refuse (`N0101`) | unchanged; still pending a native execution contract |
| bare blocks as expressions; expression statements other than `if` | refuse | unchanged. `{ … };` and `f(1);` are separate constructs with their own questions |
| `Str`, logical operators, collections, another backend | refuse / absent | out of scope by instruction |
| `break`/`continue` outside a loop; unreachable statements; a non-`Bool` condition | **not the backend's business** | the checker reports `N0312`, `N0313`, `N0301` |

There is still **one** `if` in the source language. The backend does not add a second: a
single `AstExpr::If` is lowered either as `Stmt::If` or as `Expr::If` depending on the
position it is *found in*, which is the same thing checking does. What differs between the
two is completion behaviour, and that is represented explicitly rather than by having two
syntaxes.

#### Where a loop target binds, and how it was established

**A loop's condition is not inside that loop.** A `break` there leaves the *enclosing*
loop; a `continue` there starts the enclosing loop's next iteration. This was read out of
the existing implementations rather than assumed:

- `nazm-core/src/check.rs` increments `loop_depth` around `check_block(&w.body)` **only**,
  so a `break` in the outermost loop's condition is `N0312` — confirmed by compiling one.
- `nazm-core/src/eval.rs` evaluates the condition through `take!`, so a `Flow::Break`
  raised there returns straight out of `while_loop` and is consumed by the enclosing loop.

Two probe programs pin the behaviour: with a `break` in an inner loop's condition the
interpreter prints `102`, and with a `continue` there it prints `204` — both only
explicable if the target is the outer loop. The backend therefore **pushes a loop's
targets around its body and not around its condition**, and the tests assert the same two
numbers natively.

#### Completion, represented explicitly

Emission carries three results, matching the checker's `Completion` one for one, so that
no path has to invent a value it does not have:

| Emission type | Meaning | The checker's name |
| --- | --- | --- |
| `Val::Of { value, ty }` | the expression produced a value, of this type, in the current block | `Completion::Value` |
| `Reach::FallsThrough` | a statement sequence reached its end; more may be appended | `Completion::Unit` |
| `Val::Transferred` / `Reach::Transferred` | control left along this path; there is no value and the current block already has its terminator | `Completion::Diverges` |

`Incoming` pairs a branch's value **with the block it was produced in**, because those two
are the halves of one `phi` incoming and naming a block that did not produce the value is
exactly the defect the type exists to make unrepresentable. A branch that transferred
contributes no `Incoming` at all.

`Expr::ty` becomes `Option<Ty>`: `None` means "never produces a value". Lowering then
treats `None` as fitting whatever the context wants — which is not a concession, it is the
checker's own rule, `value_type` returning `expected` for a `Completion::Diverges`.

### M1f subset — the accepted/rejected table

Written before implementation. Anything refused **still runs under `nazm run`**.

| Construct | `nazm build` | Note |
| --- | --- | --- |
| everything in M1a–M1e | accept | unchanged |
| **`return e;`** | **accept** | M1f |
| **early `return` from anywhere a statement may appear** | **accept** | including inside a loop, inside a statement `if`, and inside a nested loop |
| **a body every path of which returns** | **accept** | such a body has no tail, and needs none: `check_function` accepts `Completion::Diverges` without one |
| **`return` in a value position** | **accept** | `let x = if c { return 1; } else { 2 };` and the other four positions M1e established |
| **a `return` whose operand itself transfers** | **accept** | `return if c { break; } else { 1 };` — the `break` wins, and no `ret` is written, matching the interpreter's `take!` |
| `return` from `main` | accept | `main` is an ordinary `@nz.f<N>`; **only the platform wrapper prints** |
| recursion, direct or mutual | refuse (`N0101`) | unchanged, and still pending a measured native execution contract |
| `Str`, bare blocks as expressions, non-`if` expression statements | refuse | unchanged |
| `return` with the wrong type; `return` outside a function | **not the backend's business** | the checker reports `N0300`; the parser rejects the latter |

#### One `ret` per `return`, not a shared exit block

**Direct typed returns.** Each `return` emits its own `ret <ty> %v`, which LLVM allows and
which needs no target stack, no `phi`, and no epilogue.

A single exit block collecting every return through a `phi` is the shape a language with
destructors needs, because cleanup has to run on every path and wants one place to live.
Nazm has no cleanup mechanism yet — no destructors, no `defer`, no owned heap values — so
that shape would be scaffolding for a requirement that does not exist, and the contract
(§5) forbids adding an IR layer to match an aspirational diagram. When cleanup arrives it
is a localised change: the `ret` sites become branches to one block.

#### A body with no tail stops being a defect

M1e's `emit_function` wrote `unreachable` when a body produced no value, as a terminator
for a case lowering refused. `return` makes the case real, and the right answer is not
`unreachable` — it is **nothing at all**, because the block already holds the `ret` that
transferred. Appending a second terminator is invalid IR, and the structural check
(exactly one terminator per block, at its last instruction) is what fails if this is got
wrong.

### M1f status — implemented, and the scalar conformance table

**Added:** native `return`, as one `ret` per return site. A body every path of which
returns now legitimately has no tail, and emission writes **no** terminator for it,
because the `ret` is already there — the `unreachable` M1e wrote for that arm was correct
only while the case could not arise, and has been removed.

**Front end:** unchanged. `N0300` for a wrongly typed `return` stays the checker's.

**The one subtlety:** a `return` whose operand itself transfers. `return if c { break; }
else { 1 };` raises the `break` and writes no `ret`, matching the interpreter's `take!`
exactly, and it falls out of the existing `Val::Transferred` discipline rather than
needing a rule.

#### Scalar conformance — what `nazm build` compiles as of M1f

This table is the current answer to "what is native". Everything not listed is refused by
name with `N0101` and still runs under `nazm run`.

| Area | Native | Notes |
| --- | --- | --- |
| Types | `Int` (signed 64-bit), `Bool`, `Str`, `Ints`, `Strs` | `Str` is an immutable byte sequence; `Ints`/`Strs` are mutable references |
| Literals | integer, `true`, `false` | full `Int` range including `Int::MIN` |
| Arithmetic | `+ - * / %`, unary `-` | checked at every optimisation level; intrinsic + branch, never `nsw` |
| Arithmetic failures | overflow `N0400`, division by zero `N0401` | `Int::MIN / -1` fails; **`Int::MIN % -1` is `0`** |
| Comparison | `== != < <= > >=` | signed; ordering is `Int` only |
| Bindings | `let`, `let mut`, immutable parameters | one slot per binding, allocated in the entry block |
| Assignment | `name = value;` | a statement, no value; an interrupted right-hand side writes nothing |
| Conditionals | `if`/`else` as a value; `if` with and without `else` as a statement | one source `if`; position decides |
| Loops | `while` | no iteration budget in compiled code |
| Transfers | `break`, `continue`, `return` | a transfer in a loop *condition* targets the enclosing loop |
| Functions | several, forward references, direct calls, `Int`/`Bool` parameters and returns | `fn main() -> Int` is the entry point; only the wrapper prints |
| Evaluation order | left to right, operands and arguments | observable through which failure fires and which transfer wins |
| Strings | `Str` literals, `str_len`, `str_byte`, `str_slice`, `str_concat`, `str_from_byte`, `int_to_str`, `==`/`!=` | bytes, not characters; `{ptr, len}` by value; slices share storage; `N0405` on every out-of-range index |
| Sequences | `Ints`, `Strs`; `_new`, `_len`, `_get`, `_set`, `_push` | **reference** types: a handle to a `{len, cap, data}` header. Shared, mutable, never freed; `==` refused |
| Statements | a discarded expression (`ints_push(a, 1);`) | the language has no unit type, so a mutation would otherwise need a binding nobody wants |
| The outside world | `args_count`, `arg`, `read_file`, `write_file`, `file_exists`, `print`, `eprint`, `exit_with` | **ambient authority, enforced nowhere** — stated in `docs/spec.md` rather than implied. No process spawning |
| **Not native** | recursion, bare blocks as expressions, records, modules, concurrency | recursion pends a measured native execution contract |

This is a *scalar* conformance statement. It does not say the language is complete; it
says which programs over `Int` and `Bool` the backend compiles and with what semantics.

### M1e status — implemented

**Added:** `break`; `continue`; statement-position `if` with and without `else`,
including as a loop body's tail; and `break`/`continue` inside the value positions the
frontend accepts — a `let` initialiser, an assignment right-hand side, an arithmetic
operand, a call argument, and a nested `if` condition.

**Front end:** unchanged. `N0312` (outside a loop), `N0313` (unreachable statement),
`N0301`, `N0300` and the interpreter's `Flow` are all as they were; M1e added no language
rule and widened neither the parser nor the checker.

**Completion:** three emission results, matching the checker's `Completion` one for one —
`Val::Of { value, ty }`, `Reach::FallsThrough`, and `Val`/`Reach::Transferred`. The type
travels *with* the value because every producer knows it locally, so nothing has to be
re-derived later. `Incoming` pairs a branch's value with the block it was produced in,
which makes "a `phi` naming a block that did not produce the value" unrepresentable rather
than merely avoided. An `operand!` macro stops emission of an enclosing expression the
moment a sub-expression transfers — the emitter's counterpart to the interpreter's
`take!`, for the same reason and with the same consequences: earlier operands keep their
effects, later ones are never emitted, an interrupted assignment writes no slot, and a
call with an interrupted argument is never written at all.

`Expr::ty` became `Option<Ty>`, and `fits` is the rule the checker already applies to a
`Completion::Diverges`: an expression that always transfers stands in for whatever type
the context wanted. `Block::result` became `Option`, `None` meaning the statements
transfer. Neither is a place where a value is invented.

**Loop targets:** an explicit stack of `Targets { head, exit }` on the emitter, pushed
around a loop's **body** and popped before its exit block opens — so a nested loop cannot
overwrite the enclosing one's, and a transfer in a *condition* reaches the enclosing loop,
which is the existing language's rule. `continue` targets `head`, never the body, so the
condition is re-evaluated including any call or checked arithmetic in it. The stack is
cleared at every function boundary. A back edge is emitted only when the body's path still
falls through.

**Statement `if`:** with no `else` the false edge goes straight to the continuation, so the
statement always falls through. With an `else`, the continuation is opened only if at
least one branch still falls through; when both transfer, none is fabricated.

**Not added, and refused by name:** `return` — including inside a fully supported
conditional and loop — recursion, `Str`, bare blocks as expressions, and every expression
statement that is not an `if`.

### M1d status — implemented

**Added:** `let mut` bindings of `Int` and `Bool`; assignment as a statement; `while` with
a `Bool` condition; declarations, assignments and nested loops inside a loop body; all of
it inside any user-defined function, not only `main`.

**Front end:** unchanged. The parser, checker and interpreter already had every one of
these constructs — `N0310` for assignment to an immutable binding, `N0300` for a wrong
assignment type or a loop body that produces a value, `N0301` for a non-`Bool` condition —
so M1d added no language rule and widened nothing to suit the backend.

**IR:** two new statements. `Stmt::Assign { slot, value }`, which is the same store as
`Stmt::Let` once the name is resolved and is routed through one emitter path so the two
cannot drift; and `Stmt::While { cond, body }`, whose body is a statement sequence with no
value, because a loop produces nothing and there is no type to carry. `Stmt::Let` is now
documented as executing *every time it is reached*, which is what a declaration inside a
loop means.

**Lowering:** `lower_stmts` is shared; `lower_block` requires a tail and `lower_loop_body`
refuses one. Mutability is not re-decided — the checker has already rejected an illegal
assignment, and a second mutability rule here would be a second chance to disagree. A
loop's condition is lowered *outside* the body's scope, so a binding made in the body is
invisible to it.

**Emission:** a condition block, a body block, a back edge to the **condition**, and an
exit block. Not one `alloca` is emitted on the loop path: every slot is allocated in the
function's entry block, because a slot belongs to a binding and not to an execution of
one. A body-local declaration still stores on every iteration, so reusing the storage does
not reuse the value. The conditional branch and the back edge are appended to whatever
block emission has reached rather than to the label opened for it, which is what keeps
terminators and predecessors right when a condition or body contains checked arithmetic, a
call, or a nested `if`.

**Not added, and refused by name:** `break`, `continue`, `return`, statement-position `if`
(including as a loop body's tail), recursion, `Str`, bare blocks as expressions.

### M1c subset — the accepted/rejected table

Written before implementation, for the same reason as M1b's: a backend broadens by
accident when an AST node is handled generically. Anything refused **still runs under
`nazm run`**.

| Construct | `nazm build` | Note |
| --- | --- | --- |
| everything in M1a and M1b | accept | unchanged |
| **several top-level functions** | **accept** | M1c |
| **`Int` / `Bool` parameters and returns** | **accept** | M1c |
| **immutable parameters** | **accept** | the language has no mutable parameter; `fn f(mut x: Int)` is refused by the parser already |
| **direct named calls** | **accept** | M1c |
| **forward references** | **accept** | signatures are collected before any body is lowered, so declaration order does not matter |
| **nested calls; calls in conditions, branches, comparisons and arithmetic** | **accept** | M1c |
| `fn main() -> Int` as the entry point | required | unchanged |
| **direct recursion `f → f`** | **refuse** (`N0101`) | the language allows it and the interpreter runs it. This backend has no plan for the stack a recursive call needs, and emitting one anyway would be a guarantee it cannot keep |
| **mutual recursion `f → g → f`** | **refuse** (`N0101`) | same reason; detected as a cycle, not as a special case |
| indirect calls, function values, closures | refuse | no such construct in the language yet |
| loops, `let mut`, assignment, `return`, `break`, `continue` | refuse | unchanged |
| `Str`, bare blocks as expressions, expression statements | refuse | unchanged |
| unknown function, wrong arity, wrong argument type, duplicate definition | **not the backend's business** | the checker reports `N0201`, `N0303`, `N0300`, `N0203`. A backend limitation must never be reported as a language error, and the reverse is equally wrong |

#### Cycles in functions nothing calls

**Policy: every declared function is validated, whether or not it is reachable from
`main`.** A cycle between two functions `main` never calls is still refused.

The alternative — checking only what is reachable — would mean introducing dead-code
reachability rules as a side effect of a backend limitation, so that deleting a call
elsewhere silently changes whether a function is diagnosed. That is a language-visible
rule arriving by accident, and this increment does not make it. Validating everything is
also the behaviour that will need no migration when recursion is supported and the
diagnostic disappears entirely.

#### Symbols

Nazm functions are emitted as `@nz.f<N>`, numbered from the order they were collected.
**No source name is ever concatenated into an LLVM identifier**: a name needs escaping
rules to be safe there, and the escaping is the bug. The source name appears in a comment
beside the definition, where it cannot collide with anything.

The platform entry point `@main` is a **wrapper** that calls Nazm's `main` and prints its
result. Nazm's `main` is an ordinary `@nz.f<N>` like any other function, so a program
cannot collide with the C entry point, with the runtime helper `@nz.fail`, or with
`printf` / `write` / `exit`. **Only the wrapper prints**; a helper function returns a
value and writes nothing.

### M1b status — implemented

**Added:** `true`/`false`; immutable `Bool` locals; `< <= > >=` on `Int, Int`; `== !=` on
`Int, Int` and on `Bool, Bool`; value-position `if`/`else` with branch-local bindings,
shadowing inside a branch, and arbitrary nesting in conditions and branches.

**IR:** `Ty::{Int, Bool}` carried on `Load`, `If` and `Compare`, so emission never infers
a type from context. `BinOp` was split into `ArithOp` (five, all fallible) and `CmpOp`
(six, none fallible) — one enum would let a `match` arm written for arithmetic silently
start accepting comparisons. `Block { stmts, result }` is shared by `main`'s body and by
each branch, because `docs/spec.md` has one `Block`.

**The phi-predecessor rule:** a `phi`'s incoming labels name the block each branch
*ended* in, read after lowering that branch — not the label it started at. A branch
containing a nested `if`, or any checked arithmetic (whose guard opens a continuation
block), ends somewhere other than where it began, and an entry-label `phi` would name a
block that does not jump to the join. `clang` rejects that, so the error is loud, but the
fix is structural rather than a patch.

**Evaluation semantics:** the condition is emitted once, before either branch; only the
selected branch's block is entered, so a failure in the branch not taken cannot run; and
left-to-right order is preserved in enclosing arithmetic and comparisons. A guard's
failure block ends in `unreachable`, so a terminated path is never a `phi` predecessor.

**Two corrections to earlier claims, both about the same mistake.**

1. It was suggested that comparisons would give the arithmetic something the optimiser
   could not fold. **Wrong.** `-O2` may evaluate an `if` on constants at compile time,
   comparisons and branches included.
2. It was then suggested that **function parameters** would fix that. **Also wrong**, and
   for the same reason: a parameter is only a value unknown to *its own body*. A whole
   program whose `main` calls everything with constants is still a closed term, so an
   optimiser may inline, specialise and fold the entire call graph away. Nothing short of
   a genuinely external input — I/O, a command-line argument, a clock — makes a value
   unknown at compile time, and this language has none of those.

What the `-O0`/`-O2` pairs establish is that **the specified outcome does not depend on
the optimisation level**. They do *not* establish that any branch, call or arithmetic
operation survived optimisation, and nothing here should be read as claiming it.

**So native functions are next for a different reason.** They are how programs compose,
and a compiler is a few hundred functions calling one another — M2's whole feature list
is unreachable without them. That is the justification, not an optimisation-survival
argument that does not hold.

### M1a status — implemented

**Subset compiled:** a single `fn main() -> Int`; integer literals including
`-9223372036854775808`; immutable `let` bindings of `Int` and references to them; unary
`-`; `+ - * / %`; parentheses; a final result expression.

**Excluded, and refused by name with `N0101` after ordinary checking:** user functions and
calls, `if`, `while`, `break`, `continue`, `return`, `let mut` and assignment, `Bool`,
`Str`, comparison operators, nested blocks, expression statements, and any `main` with
parameters or a non-`Int` return type. Nothing is partially compiled.

**Crates:** `crates/nazm-lir` (`ir.rs`, `lower.rs`, `emit.rs`) and
`crates/nazm-cli/src/build.rs`. One IR level, as planned — no HIR, Core, MIR or Mono MIR.

**Target:** `aarch64-apple-darwin`, Apple clang 21.0.0, host `rustc` 1.98.1. Other targets
are **unverified rather than unsupported**: nothing in the emitted IR is target-specific,
but nothing has been run anywhere else, and `nazm-lir::TARGET_NOTE` says so in the
generated file's header.

**Not done in M1a:** catalogued mutations for the new checked-arithmetic paths. The
catalogue was not re-run this increment (see *Verification*), and adding entries that have
not been verified to be caught would weaken it rather than strengthen it. This is the
first item of the next task.

### M1b subset — the accepted/rejected table

Written before implementation, because the way to broaden a backend by accident is to
handle an AST node generically and discover afterwards what else that node covers.
Anything `nazm build` refuses **still runs under `nazm run`**: a backend limitation is not
a statement that the program is invalid.

| Construct | `nazm build` | Note |
| --- | --- | --- |
| `fn main() -> Int`, the only function | accept | M1a |
| `Int` literals, incl. `-9223372036854775808` | accept | M1a |
| **`true` / `false`** | **accept** | M1b |
| immutable `let` of `Int` | accept | M1a |
| **immutable `let` of `Bool`** | **accept** | M1b |
| reference to a local | accept | M1a |
| unary `-` on `Int` | accept | M1a |
| `+ - * / %` on `Int, Int` | accept | M1a |
| **`< <= > >=` on `Int, Int`** | **accept** | M1b; the checker admits `Int` only |
| **`== !=` on `Int, Int`** | **accept** | M1b |
| **`== !=` on `Bool, Bool`** | **accept** | M1b |
| `== !=` on `Str, Str` | **refuse** | Well-typed — the checker allows equality on any single type — but `Str` is not a native value type yet. Refused by name, not miscompiled |
| parentheses | accept | build no node |
| **`if c { a } else { b }` in value position** | **accept** | M1b; both branches, same type, `Int` or `Bool` |
| **`let` inside a branch, and shadowing there** | **accept** | M1b; branch-local scope |
| **`if` nested in a condition or in a branch** | **accept** | M1b |
| `if` with no `else` | refuse | the checker rejects it in value position anyway |
| `if` in **statement** position | refuse | it arrives as an expression statement, which stays refused — value-position `if` is the *only* `if` this backend handles |
| bare `{ … }` as an expression | refuse | only an `if` branch may be a block |
| `let mut`, assignment | refuse | |
| `while`, `break`, `continue`, `return` | refuse | |
| user functions and calls | refuse | |
| `Str` values | refuse | |
| `main` with parameters or a non-`Int` return | refuse | |
| expression statements | refuse | |

There are no logical operators in the language (`&&`, `||`, `!` do not exist), so there is
nothing for `if` to broaden into by way of one.

### Risks and effort

| Risk | Note |
| --- | --- |
| The trap paths are where the bugs will be | Mitigated by criterion 4 being per-case, and by the interpreter already being right about them |
| `Str` scope creeps | It is affordable only because nothing produces a new one; the moment concatenation appears it needs an allocator and a cleanup rule, which is M2's job |
| Apple clang's IR dialect drifting from upstream LLVM | Pin nothing yet, but record the clang version with any generated artifact, as `performance.md` §5 already requires |

Rough effort: **2–4 weeks**, assuming the subset stays exactly as it is and the
differential runner reuses the existing test corpus rather than growing a new one.

---

## M2 — The facilities needed to write a compiler

### Objective

Grow the language, in dependency order, until a compiler for a Nazm subset can be
*written* in Nazm. Not until it is bootstrapped — that is M3.

### Derived from what a compiler actually does

Each row states its semantic prerequisite, what the interpreter needs, what the backend
needs, and a small program that demonstrates it. The ordering is forced by the
prerequisites, not chosen.

| # | Compiler operation | Semantic prerequisite | Interpreter | Native lowering | Acceptance program |
| --- | --- | --- | --- | --- | --- |
| 1 | **Value/copy/move behaviour and cleanup** | The MVS parameter conventions (`let`/`inout`/`sink`/`set`) decided intraprocedurally, and a drop rule | Value clone at binding; drop at scope end | Stack slots, drop calls at block exits | A value passed `inout`, mutated, observed by the caller; a `sink` value not usable afterwards |
| 2 | **Bytes and strings** | Owned `Str` with concatenation, slicing, length, indexing by byte, `Int`↔`Str`, UTF-8 decode | Heap-backed `String` | An allocator interface; drop for `Str` | A program that reads a byte, builds a string, and compares it |
| 3 | **Tagged alternatives and products** | `struct` and `enum` with exhaustive `match` | Tagged runtime values | Tagged unions with explicit discriminants; **field access stays symbolic** (`architecture.md` §2) | A `Token` enum with a payload, matched exhaustively |
| 4 | **Growable collections** | Generics, or one monomorphic escape hatch | `Vec<Value>` | Monomorphised per element type | `List[Token]` built by a loop and iterated |
| 5 | **Recursive structures** | **Arena indices**, not heap-recursive values | An index into a `List` | Nothing new — an index is an `Int` | An AST as `List[Node]` where a node holds `u32` child ids |
| 6 | **Symbols and diagnostics** | A map, or a sorted list with binary search; string interning | — | — | Interning 10k identifiers and resolving them |
| 7 | **Reading source files** | The I/O authority contract below | A real read | `read(2)` behind the runtime | `wc -c` in Nazm |
| 8 | **Multiple source files** | File-as-module with explicit imports and a visibility rule | Multi-file loading | Separate compilation, or whole-program for now | A two-file program |
| 9 | **Writing output and errors** | Same authority contract | Real writes | `write(2)` | A program that writes a file and reports a failure to stderr |

**Rows 1 and 2 are in that order deliberately.** Growing a mutable collection before the
copy/move and cleanup rules are settled is how a language acquires a memory model by
accident. `spec.md` already lists exact copy semantics, closure capture, and copy elision
as *Open*; row 1 is where they stop being open.

**Row 5 is the load-bearing simplification.** An AST is the recursive structure a compiler
is made of, and heap-recursive values under mutable value semantics is the hardest problem
on this list. Arena indices sidestep it entirely: a node holds `u32` children, the arena is
a `List`, and recursion is an index lookup. It is also what the existing design already
leans toward — `architecture.md` §2 has `DefId` as *"path-based, never positional"* and
every IR level indexed rather than pointer-chased. Heap-recursive values can come later, or
never.

### The I/O authority contract

Narrow, and deliberately not the general effect system.

**I/O requires an unforgeable capability value, passed explicitly.**

```nazm
fn main(io: IO) -> Int {
    let text = read_file(io, "input.nz");
    ...
}
```

- `IO` has no constructor in the language. Only the runtime makes one, and it makes
  exactly one, at entry to `main`.
- A function that performs I/O takes it as a parameter. A function that does not, cannot —
  which is checkable today, with the type checker that already exists, and needs no effect
  rows, no inference, and no handlers.
- It composes forward rather than being thrown away: when effect rows arrive, holding an
  `IO` is what discharges the `io` effect, and the row can be *inferred* from the
  parameter. `architecture.md` §5 already treats effects, capabilities and provenance as
  three separate problems; this is the capability one, alone.
- It does not bypass the planned model. There is no ambient `print`, no global handle, and
  no `unsafe` escape. The thing `spec.md` refuses — *"a builtin that bypassed the effect
  system before it exists would be the wrong shape to unpick later"* — is exactly what an
  ambient `print` would be, and a capability parameter is not that shape.

Finer-grained authority (`FileRead` vs `FileWrite` vs `Net`) is a later split of one type,
not a redesign.

### Existing components reused

The checker's scope handling and `Completion` analysis extend to the new forms rather than
being replaced. The diagnostic codes and `--json` schema are unchanged. The parser gains
constructs; its two depth bounds and its refuse-by-name list already have the right shape
for them.

### Explicit exclusions

General effect handlers · one-shot continuations · whole-program taint · layout selection ·
closures (until row 1 settles capture) · traits beyond what row 4 needs · a package manager ·
floats · concurrency of any kind.

### Acceptance criteria

Per row: the acceptance program runs **identically under the interpreter and natively**,
and `spec.md` gains a *Settled* entry stating the rule and why the alternative was refused.
A row is not done when its feature parses; it is done when its semantics are written down
and differentially tested.

Milestone-level: a **lexer for Nazm, written in Nazm**, that reads a `.nz` file and emits
its token stream — running natively. That is the smallest artefact that exercises rows 1–9
together and is genuinely part of the eventual compiler rather than a demo.

### Evidence required

The Nazm lexer's token stream, on every file in `examples/`, byte-identical to the Rust
lexer's on the same input. A disagreement is a specification bug, and finding one is the
point.

### Effort

**Months, not weeks**, and the estimate is weak because rows 1, 3 and 4 each contain a
real design decision. Row 1 alone is the mutable-value-semantics question the project has
deferred since the beginning. Treat the row boundaries as the review points.

---

## M3 — Self-hosted compiler and bootstrap

### Three states, kept apart

The founder's distinction, restated as the thing that must not be blurred in a report:

| Stage | What is true | What is **not** |
| --- | --- | --- |
| **M3.0** | A compiler for a Nazm subset, *written in Nazm*, runs **under the Rust interpreter** and compiles a test program | Not self-hosting. Not the bootstrap. A useful intermediate, and reportable only as one |
| **M3.1** | That compiler, compiled natively by the Rust implementation, runs as a binary | Still not self-hosting — the Rust implementation built it |
| **M3.2** | That binary compiles its own source | Self-hosted. Reproducibility not yet established |
| **M3.3** | The result compiles its own source again, and the two agree | The bootstrap, with its fixed point checked |

### The chain

```
compiler.nz ──[ Rust implementation ]──► C1     stage 1
compiler.nz ──[ C1 ]─────────────────► C2       stage 2
compiler.nz ──[ C2 ]─────────────────► C3       stage 3
```

- **C1** is produced by the retained Rust toolchain from the same Nazm source.
- **C2** is produced by C1 from that *same* source.
- **C3** is produced by C2 from that *same* source.

C1 differs from C2 legitimately: different compilers built them. **C2 and C3 were built by
compilers that should be behaviourally identical**, so that is where a fixed point is
expected, and it is the only place equality may be asserted.

### Conformance and reproducibility checks

**Conformance — required, and the strong check:**

- C2 and C3 both pass the whole conformance corpus: the 98 cases in
  `crates/nazm-core/tests/language.rs` plus everything M1 and M2 added.
- Differential against the retained Rust implementation on the same corpus. Where they
  disagree, the specification decides, and if the specification is silent that is the
  finding.

**Reproducibility — stated carefully:**

- **Byte equality between C2 and C3 is asserted only once determinism has been
  established**, which means: no timestamps, no absolute paths, no build-host identifiers,
  and no hash-map iteration order in emitted output. None of that exists today, and
  nothing here should be read as claiming it.
- Until then the comparison is: **identical emitted LLVM IR text** for the corpus. That is
  a stronger check than identical binaries anyway, because it is diffable, and it isolates
  the compiler's determinism from `clang`'s and the linker's.
- Any remaining difference between C2 and C3 is **enumerated and explained**, or the
  bootstrap is not complete. "They behave the same" is not a substitute for knowing why
  the bytes differ.

### What stays outside Nazm, legitimately

Self-hosting does not mean eliminating these, and a report that implies otherwise is
wrong:

| Dependency | Why it stays |
| --- | --- |
| LLVM, via `clang` | Codegen is the reused layer by design (`architecture.md` §1) |
| The system linker | Same |
| libc | `write`, `read`, `exit`, `malloc` until `nazm-rt` exists |
| The OS and its loader | — |
| **The Rust implementation** | **Retained until a replacement is justified** — it is the bootstrap seed, the differential oracle, and the thing that still works when C1 does not |

### Explicit exclusions

Deleting the Rust implementation · a second backend · reproducible-build infrastructure
beyond determinism of the compiler's own output · distributing a binary seed ·
cross-compilation.

### Acceptance criteria

C3 exists; C2 and C3 emit identical IR for the whole corpus; both pass conformance; every
external dependency is listed; every difference that remains is named.

### Evidence required

The three build logs, the corpus results for C2 and C3, the IR diff (empty, or explained),
and an explicit statement of which of M3.0–M3.3 was reached. **A compiler running under
the Rust interpreter is M3.0 and must be reported as M3.0.**

---

## Concurrency track (separate, and not now)

Runs alongside, intersecting the compiler roadmap in exactly two places. **No scheduler is
to be started now**, and a compiler does not need one — a single-threaded compiler is a
perfectly good compiler.

| Stage | Content | Depends on |
| --- | --- | --- |
| C-1 | **Specify** the open questions `spec.md` already lists: what cancellation cancels, what cleanup is guaranteed, captured variables and globals under channel transfer, preemption at back-edges vs signals | Nothing. It is writing |
| C-2 | **Ownership across tasks.** The borrow region `[spawn point, scope end]` with parent and sibling access rejected throughout — already a *Settled* decision (`spec.md` §6) | **M2 row 1.** Ownership across tasks is the same exclusivity question as ownership across calls; solving it twice is how the two answers diverge |
| C-3 | Structured lifetimes and cancellation, as a checked property with compile-fail tests | C-2 |
| C-4 | `nazm-rt`: scheduler, stacks, channels, reactor. **The one crate allowed `unsafe`**, which means the workspace's `unsafe_code = "forbid"` becomes `deny` with a narrow allowance, and `xtask check`'s unsafe gate enforces the narrowness | C-3, and a backend that can call it |
| C-5 | Measurement: spawn-to-first-instruction latency, context-switch cost, task memory as **RSS not virtual reservation** with page size recorded, against Go | C-4 |

**Where it intersects:** at M2 row 1 (shared memory model) and at M1's backend (C-4 needs
something to compile against). Nowhere else. Concurrency features in the surface language
before C-1 is written would be syntax without semantics.

---

## AI-native track (start here, and keep it narrow)

`direction.md` separates three things that the phrase blurs. Only the first is in scope
now.

**(a) Agent-facing tooling and predictable semantics — in scope, and largely already
built.**

| Capability | State |
| --- | --- |
| Stable structured diagnostics: error codes, spans, `--json`, machine-applicable fixits | **Implemented** (`nazm-diag`, `--json`) |
| One mistake, one error — a broken body does not delete the function | **Implemented** (`ast.rs` `Function::body: Option<Block>`) |
| Predictable semantics: specified evaluation order, trapping overflow, no truthiness, refuse-by-name | **Implemented** (`spec.md` *Settled*) |
| **Safe source edits**: a lossless CST and a canonical formatter with no degrees of freedom to churn over | **Absent.** Needs `cstree`; the current parser produces a plain AST |
| **Reproducible checks**: the same input gives the same diagnostics, orderable and diffable | Partly — needs the determinism work M3 requires anyway |
| A cheatsheet under ~3k tokens, and a machine-readable grammar | **Absent** (`spec.md` *Open*) |

**(b) Libraries for writing AI applications in Nazm — out of scope.** Not until there is a
standard library and an I/O story, which is M2 at the earliest.

**(c) Tensors, GPUs, model providers, distributed execution — out of scope, and not
planned.** `architecture.md` §1 already says MLIR should be revisited *"only if Nazm pivots
toward tensor/accelerator codegen"*, and nothing here proposes that pivot.

**No language keywords will be added to support the label.** If a construct earns its place
on semantics, it earns it; "AI-native" is not an argument for syntax.

---

## Ordering summary

```
M1  native vertical slice        ← next; no language change
M2  facilities for a compiler    ← the long one; row 1 is the real decision
M3  bootstrap                    ← C1 → C2 → C3, byte equality only where earned
```

with the concurrency track joining at M2 row 1, and the AI-native track's one open item
(lossless CST + canonical formatter) schedulable any time after M1.
