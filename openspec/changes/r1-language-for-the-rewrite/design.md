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

### D1. Immutable indirection: `Box[T]` and `List[T]` — for approval

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

### D2. Linear-time string building — for approval

A guarantee added to the cost table, not a new type:

| Statement | May allocate | May copy |
| --- | --- | --- |
| `s = str_concat(s, t);` where `s` is a `let mut` binding | **yes**, amortised | **`t` only**, amortised |

The implementation appends in place when the old value of `s` dies at that assignment, its bytes
are a heap buffer it alone owns, and the buffer has room; otherwise it copies into a buffer with
room to grow. Rule 1 already makes the difference unobservable. A `@std/text` `StrBuf` is not
added: the guarantee makes the existing idiom right instead of adding a second one.

### D3. `for` loops — for approval

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

### D4. Library additions — for approval

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

### D5. A bounded heap — for approval

`nazm build --heap-limit SIZE` (and a manifest key) builds a native program whose runtime counts
live heap bytes and ends the program with a run-time failure — a fixed message and status, as other
run-time failures have — when an allocation would pass `SIZE`. Without the flag nothing changes. The
Nazm-built toolchain is built with a limit; this is the mechanism R6 needs to argue host execution
safe, and what keeps a runaway compiler stage from exhausting the machine meanwhile.

### D6. Closures in generic functions — recommended deferred

`N0387` refuses a closure or a named function value in a generic function's body. The rewrite can
be written without it (pass a function value in; write the closure at the call site). Recommended:
not in R1; reconsider if R2–R5 show a real cost.

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
