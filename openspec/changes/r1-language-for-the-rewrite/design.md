## Context

What the rewrite needs and the language lacks (survey of 2026-10-11, `rewrite-in-nazm/design.md`):

- **Trees.** `N0359` refuses every definition that reaches itself through what it owns
  (`docs/spec.md`, *Generics* — *What may not be expressed: an ownership cycle*; *The memory
  constitution* — *Cycles*). The reason is sound: a `Vec` is a shared **mutable** handle, so a
  `Node` could be pushed into its own `children`, and counting would never reclaim it. The cost is
  that the compiler written in Nazm keeps ~45 parallel arrays in one table (`compiler/analyse.nz`),
  and `@std/json` is an index arena.
- **Strings.** `str_concat` allocates and copies (*What this costs*); a loop that grows a string is
  quadratic, and the self-hosted stages' memory history (`docs/bootstrap.md`) is partly this.
- **Loops.** Only `while`; every traversal is an index and a `vec_len` written out.
- **Library.** `@std/hash` is FNV-1a, not cryptographic; no TOML; `dir_list` but no walk; no handle
  for standard output; `os_spawn` takes the process's environment and directory (*Processes*);
  no file locks; environment variables can be read, not set.
- **Memory exhaustion.** A Nazm program that allocates without bound takes the machine with it; the
  runbook confines self-hosted stages to a container for that reason.

## Goals / Non-Goals

**Goals:** each addition is a 1.x-compatible extension (`docs/stability.md`: new syntax that was
refused before, new built-ins and library items); the reclamation claim of *Cycles* — every value
of every type a program can write is reclaimed by counting — stays true; each addition is specified
before it is built, and built in the interpreter, both native backends (until cut-over) and the
Nazm compiler (R2).

**Non-goals:** a garbage collector, weak references, mutation through `Box`/`List`, trait objects,
exceptions, a general iterator protocol, methods.

## Decisions

Each marked **for approval** is a language decision the copyright holder makes before code.

### D1. Immutable indirection: `Box[T]` and `List[T]` — approved 2026-10-11 (as L1)

Two built-in generic types of a new value kind, **shared immutable**, the kind `Str` already is:

| | `Box[T]` | `List[T]` |
| --- | --- | --- |
| Holds | one `T` | a sequence of `T` |
| Made by | `box_new(v)` | `list_from_vec(xs)` (a snapshot: later changes to `xs` do not reach it), `list_empty[T]()`, `list_push_new(l, v)` (a new list) |
| Read by | `box_get(b) -> T` | `list_len(l)`, `list_get(l, i) -> T` (`N0405`-style failure outside `0..len`), `for` |
| Mutated by | nothing | nothing |
| Assignment | shares (a reference count) | shares |
| Equality | structural, when `T: Equality` | structural, when `T: Equality` |
| Crosses into a task | no (`N0321`), as a handle does — see *Open Questions* | no |

**The ownership rule changes in one place.** An edge of the ownership graph through `Box` or
`List` is labelled *immutable*. Then:

- a cycle whose every edge is inline stays an impossible layout — `N0336`;
- a cycle with **at least one edge through a mutable handle** (`Vec`, `Ints`, `Strs`, `Bytes`, a
  closure's captured handle) stays an ownership cycle — `N0359`;
- a cycle whose edges are inline or immutable, with at least one immutable, is **accepted**.

So `enum Expr { Int(v: Int), Add(l: Box[Expr], r: Box[Expr]) }` and
`struct Node { kids: List[Node] }` are legal; `struct Node { kids: Vec[Node] }` stays refused, and so
does `struct A { kids: Vec[B] } struct B { up: Box[A] }`, whose cycle passes through a `Vec`.

**Why counting still reclaims everything** (the argument *Cycles* requires before a recursive type
is admitted). A value cycle maps to a closed walk in the definition graph, and a closed walk through
an edge contains a simple cycle through that edge; so a value cycle through a mutable handle needs a
definition cycle through one, which stays refused. A value cycle of inline and immutable edges only
is impossible: inline containment is finite, and an immutable edge points at a value that existed
before the `Box` or `List` holding it was made and can never be changed to point anywhere else —
the same fact that makes closure environments safe (*Cycles*, Gate 1-C1, fact 1). So every value is
reclaimed by counting, recursive ones included; release of a deep structure is iterative, not
recursive, so a long list does not overflow the stack.

Closures keep `N0616` for **mutable** handles only: a closure may capture a `Box` or `List` that can
hold a function value, because what either holds was made before it and cannot be replaced — the
closure that captures one cannot be inside it.

*Names:* `Box` appears in the grammar's examples as a user struct; a project type named `Box` or
`List` is refused once they are the language's, as `Result` and `Option` are (*Its names are
reserved*). Alternatives, if the names are wanted for users: `Rc[T]`/`Seq[T]`, `Frozen[T]`/`FrozenList[T]`.

### D2. Linear-time string building — approved 2026-10-11

A guarantee added to the cost table, not a new type:

| Statement | May allocate | May copy |
| --- | --- | --- |
| `s = str_concat(s, t);` where `s` is a `let mut` binding | **yes**, amortised | **`t` only**, amortised |

The implementation appends in place when the old value of `s` dies at that assignment, its bytes
are a heap buffer it alone owns, and the buffer has room; otherwise it copies into a buffer with
room to grow. Rule 1 already makes the difference unobservable. A `@std/text` `StrBuf` is not
added: the guarantee makes the existing idiom right instead of adding a second one.

### D3. `for` loops — approved 2026-10-11

```nazm
for x in xs { … }        // xs: Ints, Strs, Vec[T], List[T], Bytes (each Int byte)
for i in a..b { … }      // a, b: Int; half-open; a..b evaluated once
```

Each is **defined by the `while` loop it stands for**, so nothing new is said about evaluation:
the collection expression is evaluated once into a hidden binding; the index starts at 0; the
length is read **before every step**, so pushing during the loop visits the new elements and a
`vec_pop` that shortens it ends the loop early — exactly what the `while` loop does; `x` is a
fresh immutable binding per step; `break` and `continue` behave as in `while`. `for` is already a
reserved word (`RESERVED` in `parser.rs`); `in` is recognised only after a `for` pattern and stays
an ordinary name elsewhere; `..` was a lexical error before. So no valid program changes meaning.

### D4. Library additions — approved 2026-10-11

| Module or built-in | What | Notes |
| --- | --- | --- |
| `@std/blake3`, `@std/sha256` | `blake3(Bytes) -> Bytes`, incremental hasher; SHA-256 the same | Nazm source, using the existing wrapping arithmetic and rotations; held to the official test vectors |
| `@std/toml` | parse to and encode from a tree of D1 types | TOML 1.0; what `nazm.toml` and `nazm.lock` use is tested first |
| `file_walk(io, dir) -> Result[Strs, IoError]` | every path under a directory, sorted | in Nazm over `dir_list`; symbolic links not followed |
| `os_stdout()`, `os_stderr()` | handles for `os_write` | raw bytes out; `print` unchanged |
| `os_spawn_in(program, args, dir, env: Strs)` | a child in `dir` with exactly `env` (`NAME=value`) | `ProcessCap`; the existing `os_spawn` unchanged |
| `os_lock(h, exclusive: Bool) -> Int`, `os_unlock(h)` | advisory whole-file locks | `flock`/`LockFileEx` |

**Not added:** setting the process's own environment (`setenv` is unsafe beside the task pool's
threads); a child's environment is given through `os_spawn_in` instead.

### D5. A bounded heap — approved 2026-10-11

`nazm build --heap-limit SIZE` (and a manifest key) builds a native program whose runtime counts
live heap bytes and ends the program with a run-time failure — a fixed message and status, as other
run-time failures have — when an allocation would pass `SIZE`. Without the flag nothing changes. The
Nazm-built toolchain is built with a limit; this is the mechanism R6 needs to argue host execution
safe, and what keeps a runaway compiler stage from exhausting the machine meanwhile.

### D6. Closures in generic functions — superseded by L10 (approved 2026-10-11)

`N0387` refuses a closure or a named function value in a generic function's body. The rewrite can
be written without it (pass a function value in; write the closure at the call site). Recommended:
not in R1; reconsider if R2–R5 show a real cost.

### D7. Capable first — the copyright holder's direction, 2026-10-11

"Before re-write by nazm, nazm should be capable of doing everything." R1 is therefore sized by an
audit, not by this list alone: every idiom the Rust toolchain relies on (data structures,
polymorphism, closures and iteration, errors, formatting, serialisation, concurrency, the platform)
is classified as expressible, expressible only by a pervasive workaround, or not expressible; each
of the last two becomes a proposed decision here, D1 among them, and R1 closes when the audit's
blockers and heavy workarounds are closed or explicitly accepted.

### D8. The capability audit's findings — approved 2026-10-11

Three read-only audits of the Rust toolchain (2026-10-11, at `18d1c19`): the compiler crates
(~62k lines), the tool and service crates (~128k with tests), and the runtime, code generator and
harness. Each idiom the Rust relies on was classed BLOCKER (no sensible expression in Nazm), HEAVY (a
workaround repeated hundreds of times) or CONVENIENCE. Counts are grep counts over source.

**Approved by the copyright holder, 2026-10-11:** the whole list, with the direction "first make
Nazm mature, to rewrite everything". Where a choice remained, the recommended one was taken: L1's
names are `Box` and `List`; L3 includes `_` with a warning when it covers variants; S5 builds
`@std/regex`.

**Language — each changes what a program can say; spec first, then both implementations:**

| # | Proposal | Evidence | Class |
| --- | --- | --- | --- |
| L1 | D1: `Box[T]`/`List[T]`, recursive types through immutable indirection | 48 `Box<` in the AST and Core IR; 239 tree matches in the service | BLOCKER |
| L2 | Derived serialisation: `@derive(json)` (and TOML) emitting encode, decode and JSON Schema, structural like derived `==`, with a closed set of field and type attributes (`rename`, `default`, `omit_none`, `tag`, `untagged`, `rename_all`, `deny_unknown`) | 176 serde types, 70 versioned schemas, 118 shape attributes; 81 more in the harness | BLOCKER |
| L3 | Patterns: a `_` arm, `A \| B` or-patterns, nested and literal patterns, `if let` and `let … else`. **Reverses a recorded decision** (*Enums*: no wildcard, so adding a variant forces every `match` to be revisited); proposed with a warning when `_` covers variants, so the evolution argument keeps a voice | 333 or-patterns, 301 `_` arms, 526 `if let`, 428 `let … else`, 585 nested patterns | HEAVY |
| L4 | String interpolation `"…{x}…"` over `Str`, numbers and `Show`, with width and precision; float formatting | 1,318 `format!`, 161 `write!`, 651 inline captures | HEAVY |
| L5 | Tuples: `(A, B)` types, `let (a, b) = …`, usable as map keys | 134 tuple types, 322 destructurings, 26 tuple-keyed maps | HEAVY |
| L6 | Derived `Hash` and `Ord` for plain records, enums and tuples | 92 Hash, 74 Ord derives; 257 ordered-map uses | HEAVY |
| L7 | `Cell[T]`, a mutable handle of one value (the kind `Vec` is, so `N0359` covers it unchanged), for context records updated by callees | 631 `&mut self`, 199 inherent impls | HEAVY |
| L8 | `pub const`, constant arrays of `Str` and records | 257 consts, 87 static string tables; kinds written as `fn k() -> Int` | HEAVY |
| L9 | Multi-line and raw string literals | the runtime's ~225 IR templates; `emit.nz` has 423 one-line literals | HEAVY |
| L10 | Closures in generic functions (lifts `N0387`, was D6) and non-escaping capture of a `let mut` (lifts `N0386` where the closure cannot outlive the binding) | 1,894 closures, 1,491 iterator chains; `for` absorbs most | HEAVY |
| L11 | Impls for generic types (`impl[T: Show] Show for Option[T]`) and default trait methods | ~40 generic impls | CONVENIENCE |
| L12 | `defer` — scope-exit cleanup that runs on `?`, `return` and `break` | locks, watchdogs, temp dirs in the harness | CONVENIENCE, a correctness risk |
| L13 | `checked_add/sub/mul -> Option[T]`, a widening multiply, `Int128`/`UInt128` | 160 checked/overflowing calls; constant folding | HEAVY (local) |
| L14 | Stack size: `--stack-size` for the main thread and tasks | 13 deep-stack threads (up to 1 GiB) | CONVENIENCE |

**L10 as carried out (2026-10-13).** The approved wording lifts `N0386` "where the closure cannot
outlive the binding". Nazm cannot say that of a parameter — a callee may store any closure it is
given — so a non-escaping rule would need an escape analysis over every function value. L10 shares
the binding instead (the design Swift uses): it lives in a `Cell` the function and its closures
name, so a closure outliving its scope leaves nothing dangling, and the cycle argument holds by
refusing a binding that can hold a function value (`N0386`), as `N0616` refuses such a `Vec`.

**Found while carrying it out (2026-10-12):**

| # | Proposal | Why |
| --- | --- | --- |
| L15 | A **warning** severity for diagnostics, with `--deny-warnings` | L3's `_` is to warn when it covers enum variants, and Nazm has no warning severity (Q1-D-15 found the same gap for deprecation). Until L15, `_` is accepted without the warning, recorded in `limitations.md`; L15 adds it, and the deprecation warning `N0620` was reserved for |

**Platform built-ins — runtime ABI:**

| # | Proposal | Evidence | Class |
| --- | --- | --- | --- |
| P1 | Process control beyond D4's `os_spawn_in`: `os_pid`, `os_child_pid`, a new process group, `os_kill_group(pgid, sig)`, `os_try_wait`, `os_wait_until(h, deadline)`, a null standard input, `os_child_rusage` (peak memory) | the harness's watchdog and contained runner | BLOCKER |
| P2 | Filesystem: `os_lstat` with a symlink kind and device/inode, `os_chmod`, `os_realpath`, a temporary directory, `os_try_lock` beside D4's `os_lock` | bench workspace checks; the mutation lock | HEAVY |
| P3 | Streaming standard input: read into a buffer, read a line | LSP framing, the MCP line protocol | HEAVY |

**Library, in Nazm — no language change:**

| # | Proposal | Class |
| --- | --- | --- |
| S1 | D4 as approved: `@std/blake3`, `@std/sha256`, `@std/toml`, `file_walk` | BLOCKER |
| S2 | `ByteBuf`: amortised growth, `put_le`/`put_be`, `patch_at`, `align_to`, SLEB128 and ULEB128 — the object writer's and DWARF's buffer | HEAVY |
| S3 | `@std/ordmap` over `Ord` keys, in-order iteration; `@std/bitset`; in-place sort | HEAVY |
| S4 | `@std/args`: subcommands, options, conflicts and requirements, generated help — for `nazm`'s 42 subcommands | HEAVY |
| S5 | `@std/regex`, linear-time, with what `nazm-tokens`' pre-tokenisers need, or a decision to keep tokenisation an external oracle | BLOCKER for `nazm-tokens` only |

**Not proposed, and why:** trait objects (4 real uses; enums and function values serve), lifetimes
and borrowed slices (reference counting and `str_slice` serve), async (a synchronous stdio loop
serves the MCP server), a mutex (the interpreter keeps its own heap of counted objects — the right
design for it, and what makes its memory report exact), leaking cycles (the reclamation claim stays).

**Design notes for later phases:** the fuzzer cannot catch a crash in process with no unwinding, so
it becomes a server re-spawned after a crash or hang, with coverage emitted by Nazm's code generator
(R7, R9); the runtime keeps its context switch as `module asm` in LLVM text, so clang remains the
assembler and no assembler is written; 1,388 of the 1,538 mutation entries and the layering, reach
and module checks read Rust and are redesigned in R7.

**Sequencing, if approved:** R1a — `for`, L1, L3, L5, L6, L8, L9, L4; R1b — L7, L10, L11, L12, L13,
L14; R1c — L2 with S1's TOML; R1d — D5, P1–P3, D4's built-ins; R1e — S1–S5. Each a commit series
with its gates. Every addition is also owed to the Nazm compiler in R2.

## Risks / Trade-offs

- **D1 is a memory-model change.** Its argument is above; it is written into *Cycles* before the
  code, and the reclamation tests (memory report compared across the interpreter, LLVM and
  Cranelift) cover recursive values — deep, wide and shared.
- **Two new reserved type names** (D1) refuse a project type of that name: a 1.x-compatible
  addition by `stability.md`'s rule for new built-ins, but a user-visible one.
- **In-place append (D2)** must never mutate bytes another value can see; its tests compare every
  sharing shape (a slice, a copy, a literal, a value in a `Strs`) before and after.
- **D5** changes allocation in every program built with the flag; without it the runtime is
  unchanged, which its tests check.

## Open Questions

- Should `Box`/`List` cross into tasks (with atomic counts), for a parallel compiler? Recommended
  no in R1: crossing is a separate decision with a cost on every count.
- The default `--heap-limit` the toolchain is built with (measured in R2).
