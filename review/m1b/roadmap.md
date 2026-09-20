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

**Correction to an earlier claim.** It was suggested that comparisons would give the
arithmetic something the optimiser could not fold. **That is wrong.** Every program in
the M1b subset has constant inputs — there are no parameters and no I/O — so `-O2` may
evaluate the whole thing at compile time, comparisons and branches included. What the
`-O0`/`-O2` pairs establish is that **the specified outcome does not depend on the
optimisation level**. They do *not* establish that any branch survived optimisation, and
nothing here should be read as claiming it. A value unknown at compile time arrives with
function parameters.

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
