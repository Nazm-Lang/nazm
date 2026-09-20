# Nazm language specification

**Status: this is not yet a specification — it is the list of decisions a specification
has to make, with the ones already settled marked as such.**

**The current executable language is v0.3.** Sections are versioned and kept in order, so
a *Settled for v0.1* entry records what v0.1 decided, not what is true now; where a later
version changed one, the earlier entry says so and points forward. Everything under *Open*
is undecided.

Publishing the open questions before the answers is deliberate. Every one of them was
either wrong or unstated in an earlier draft of the design, and the cost of discovering
that after 150k lines of compiler is the entire reason this document exists first.

---

## Settled

These are design commitments. Changing one is a re-founding decision, not an edit.

1. **Source of truth is plain UTF-8 `.nz` files in git.** Content addressing applies to
   derived artifacts only. (`architecture.md` §3)
2. **Field access is a symbolic projection through Mono MIR.** Byte offsets exist only at
   LIR. Without this, automatic layout selection is unrecoverable. (`architecture.md` §2)
3. **No lifetime syntax in the surface language.** Mutable value semantics with parameter
   conventions `let` / `inout` / `sink` / `set`, checked intraprocedurally. The IR carries
   a full region calculus anyway, as a hedge. (`architecture.md` §5)
4. **Effect rows are inferred and polymorphic.** Colouring is prevented by polymorphism
   over the effect variable, not by hiding the distinction.
5. **Taint is position-sensitive.** Untrusted data in a bound-parameter position is legal;
   in a query-construction position it is an error.
6. **The structured-concurrency borrow region is `[spawn point, scope end]`**, with parent
   and sibling access rejected throughout — not merely at the join.
7. **Purity is a prerequisite, not a licence.** An empty effect row does not by itself
   authorise dropping, reordering, memoizing, or parallelising a call.

## Settled for v0.1 — the executable subset

These are the decisions the first interpreter needs and no more. Each is chosen for the
same reason: **a reader should be able to predict what a line does from the line and the
compiler's response to it.** Where two designs are defensible, the one that removes a
question wins over the one that adds an option.

### Integer overflow — traps, identically in every build

`Int` is 64-bit signed. Overflow is an error that stops the program, in development and in
release alike.

Wrapping silently is the classic source of a bug nobody reads: the arithmetic looks
correct and the number is wrong much later. Differing between debug and release is worse
— it makes a class of bug reproducible only where it cannot be debugged. Trapping costs a
check per operation, which matters for a language claiming performance, and the escape
hatch when it does is an explicit wrapping operator, not a build flag.

### Division by zero — an error, not a value and not undefined

`/` and `%` by zero stop the program with a diagnostic. Not a NaN-like sentinel, which
propagates silently; not undefined behaviour, which is unrepresentable in a language with
no `unsafe` yet.

### Shadowing — allowed in a nested scope, refused in the same one

```nazm
let x = 1;
let x = 2;        // error: `x` is already defined in this scope
if c { let x = 3; }   // fine: a different scope
```

Rust permits same-scope shadowing and it is genuinely convenient. It is refused here
because of what it does to *reading a change*: when a later line mentions `x`, telling
which binding it means requires scanning upward for every intervening `let`. A diff that
inserts one silently changes the meaning of code it does not touch.

### Argument evaluation order — left to right, specified

Specified, not "unspecified for optimisation". Unspecified order is a licence for two
correct compilers to disagree, and the resulting bug appears only after a toolchain
upgrade. v0.1 has no side effects for the order to be observable through, which is
precisely why it costs nothing to fix it now and would be a breaking change later.

### Function returns — the final expression, and nothing else

A function body is a block; the block's value is its final expression; that is the return
value. There is no `return` statement in v0.1.

One exit point means the value a function produces is always in the same place. `return`
earns its keep with guard clauses, and it is not ruled out — it is deferred until there is
something to guard against.

> **Superseded in v0.3.** `return` exists and is implemented — see [`return`](#return)
> below. This entry is kept rather than edited because it records what v0.1 committed to
> and why, and the reasoning ("deferred until there is something to guard against") is
> what the later section answers. **The current executable language is v0.3**; a function's
> value is still its final expression, and `return` is for the paths that leave early.

### Types — `Int`, `Bool`, `Str`, and no inference across function boundaries

Parameter and return types are always written. Local `let` bindings infer from their
initialiser. Both branches of an `if` must have the same type, and a condition must be
`Bool` — there is no truthiness.

## Settled for v0.2 — mutation and iteration

### Mutation is opt-in and visible at the binding

```nazm
let x = 1;        // immutable
let mut n = 0;    // mutable
n = n + 1;        // assignment is a statement
x = 2;            // error: `x` is not mutable
```

Immutable by default, `mut` to opt in. The alternative — mutable by default with a `const`
opt-out — puts the annotation on the rarer case and makes the common one silent, so a
reader has to check every binding to know whether a later line can change it.

Assignment is a **statement**, not an expression. It has no value, so `if (x = 1) { … }`
does not parse. That is one of the oldest C bugs and there is nothing to gain by
reproducing it.

**Parameters are not mutable**, and `fn f(mut x: Int)` is refused by name. A mutable
parameter is a local copy, which looks like an out-parameter to anyone who has not checked
the calling convention. A `let mut` inside the body says the same thing without the
ambiguity.

Assignment does not change a binding's type: `n` declared from an `Int` stays `Int`.

### `while` is a statement, and there is no loop value

```nazm
fn sum_to(n: Int) -> Int {
    let mut total = 0;
    let mut i = 1;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    total
}
```

A loop has no natural value. The two ways to give it one are a unit type or an arbitrary
rule, and a unit type is a real decision with consequences for function returns and block
tails — too large to make in passing for the sake of `while`. So a block is *statements
followed by a tail expression*, and `while` is a statement. The tail is still required.

The condition must be `Bool`. There is still no truthiness.

### One block, and completion decided by checking

A block is **statements followed by an optional tail expression**. There is one `Block`
and one `if`; whether either produces a value is determined when checking, not by which
syntactic form was parsed.

The tail is simply *an expression not followed by `;`*:

```nazm
{ let x = 1; x }      // tail: the block's value is `x`
{ let x = 1; x; }     // no tail: `x;` is a statement, the block has no value
{ if c { a() } }      // tail: the `if`'s value is the block's
{ if c { a(); } d }   // the `if` is a statement; `d` is the tail
```

A block-like expression — `if`, `while`, a bare `{ … }` — needs no `;` in statement
position, so `if c { x = 1; }` followed by more statements reads naturally.

An earlier version had two of everything: an expression block and a statement block, an
`if` expression and an `if` statement, and a function that scanned forward balancing braces
to guess which `if` it was looking at. That guess had a case it got wrong, and every
construct added afterwards would have needed its own place in it. **This is the replacement
and the scanner is gone.**

Checking gives each block one of three completions, which are internal — the language has
no `unit` or `never` type to write:

| completion | meaning |
| --- | --- |
| **value** | falls off the end with a value of some type |
| **unit** | falls off the end with no value |
| **diverges** | never falls off the end: every path `return`s, `break`s or `continue`s |

A function body must complete with a value of the declared type, or diverge. An `if` used
where a value is wanted must have both branches, and they must agree — unless one diverges,
in which case the other decides. Unreachable code is a statement after one that diverges.

### `break` and `continue`, and the `if` they force

```nazm
while i < n {
    if skip(i) {
        i = i + 1;
        continue;
    }
    if done(i) { break; }
    total = total + i;
    i = i + 1;
}
```

Both are **statements**, they affect the **innermost** enclosing loop, and there are no
labels. `continue` re-evaluates the condition.

They are only legal inside a loop body, and **only in statement position**. `break` is not
an expression and has no value, so `let x = break;` and `if c { break } else { 1 }` do not
parse. Rust gives `break` the never type and makes this work; a never type is a real
addition with consequences for every other inference rule, and it is not worth adding so
that `break` can sit in an expression it has no business being in.

`while c { if d { break; } }` works because a block's tail is optional: `{ break; }`
completes by diverging, which is not the same as completing with no value, and neither is
an error unless something wanted a value from it.

### `return`

```nazm
fn classify(n: Int) -> Str {
    if n < 0 {
        return "negative";
    }
    if n == 0 {
        return "zero";
    }
    "positive"
}
```

`return expression;` leaves the enclosing **function**, not merely the enclosing loop, and
its value must match the declared return type. A function whose every path returns needs no
tail expression.

**`return;` with no value is not supported.** It would need a unit type to mean anything,
and that is a decision this language has not made. Refused by name.

A function's value is still its final expression; `return` is for the paths that leave
early, which is the case it earns its keep on.

**`return` is a statement, but a block is an expression**, so a `return` may sit inside
anything used as a value:

```nazm
let x = if n > 0 { return 1; } else { 2 };
let y = g(if n > 0 { return 7; } else { 2 });
while (if n > 0 { return 4; } else { i < 3 }) { … }
```

Each of these leaves the function. The enclosing expression has no value and never will,
so evaluation of it stops where the `return` was raised: in `1 + if c { return 5; } else
{ 2 }`, the left operand has already been evaluated and the addition never happens. The
same applies to `break` and `continue` in a value position, and to a `return` nested
inside another — the inner one leaves first, so the outer never gets a value to return.

This follows from the checker rather than being a separate rule: a diverging branch takes
the other branch's type, which is what makes these programs well-typed in the first place.
An evaluator that refused them would be rejecting programs the type system accepts.

### Code after `break`, `continue` or `return` is an error

```nazm
while c {
    break;
    i = i + 1;   // error: this can never run
}
```

An error rather than a warning, because this language has no warning severity yet and
silence is the wrong default for a statement that provably cannot execute. Relaxing an
error to a warning later is a compatible change; tightening a warning to an error is not,
so the strict choice is the reversible one.

### The iteration and depth limits are the interpreter's, not the language's

With no `break` and no I/O, a non-terminating program produces nothing at all. A tool that
hangs is worse than one that reports, so `nazm run` stops and says so — after a
configurable number of loop iterations (`--max-iterations`, default 10,000,000) or nested
calls (`--max-depth`, default 512).

Both are needed, and one does not imply the other: `fn f(n: Int) -> Int { f(n) }` never
loops, so the iteration budget never sees it. Unbounded, it exhausts the host stack and
kills the process with no diagnostic at all — the one failure a tool cannot explain.

**A bound only means something against a known stack**, and a library cannot know its
caller's: macOS gives the main thread 8 MiB, a spawned thread 2 MiB, and the test harness
2 MiB per test. So the interpreter runs the whole pipeline — parsing, checking, evaluating
and dropping the tree — on a thread whose stack it sets itself, and **sizes that stack from
the budget it is about to enforce**: `24 MiB + max_depth × 32 KiB`. Raising `--max-depth`
raises the reservation with it, so the guarantee holds above the default rather than only
at it. A machine that will not grant the reservation says so (`N0404`) instead of failing
part-way in, and a budget past `1,000,000` is refused with the figure it would have cost.

The per-call figure is measured, not guessed: about 6.6 KB per nested call for ordinary
code on a debug build and about 17 KB for deliberately heavy frames, doubled to 32 KiB. A
test recurses to the bound and requires a diagnostic, so a construct that made frames dear
enough to close the gap fails the suite rather than surfacing as an abort in a terminal.

### Nesting is bounded at parse time, which bounds every pass after it

Call depth is not the only way to run out of stack. `((((1))))`, `- - - - 1` and
`{ { { 1 } } }` recurse through the parser, the checker, the evaluator **and the destructor
of the tree itself**, without a single call frame in the program — so a call-depth budget
never sees them, and `nazm check` never reaches the evaluator at all.

**The syntax tree may be 512 deep** (`N0102`), measured as it is built: every node
records `1 + max(children)`, so the bound is on the tree the later passes walk rather than
an estimate of it. Parsing is the only pass that can report rather than abort, so that is
where the limit lives. Reported once per item, not once per level.

A *second* bound, the same size, applies to the parser's own recursion, because the parser
is a stack consumer too and the two are not the same measurement. Parentheses build no
node — `((((1))))` is a tree of depth 2 however many there are — so only the recursion
bound constrains them. Conversely `1 + 1 + …` is parsed by a loop that never recurses, and
only the tree bound constrains it.

Getting this wrong is easy and was got wrong first. Counting parser recursion alone looked
sufficient, and was not: in `- - - 1 + 1 + 1` the operand is parsed and its levels released
*before* the chain of `+` starts counting, so the peak count is the larger of the two while
the tree is as deep as both together. A test that measured the tree rather than the counter
found one accepted at 1022 against a limit of 512, and composing the shapes would have
compounded it further. The bound is now exact because it is taken from the nodes
themselves; the tests check the deepest tree accepted for every pairing of seven shapes,
each pair probed at its own edge rather than somewhere comfortably inside it.

512 is far past anything written by hand. It is not far past what a generator emits: Nazm
has no `else if`, so a dispatch chain written as `else { if … }` costs two levels an arm.
A limit tight enough for a 2 MiB stack would have been about 64, which is a real constraint
on real code — so the stack is raised instead, and `parse_source` states the requirement it
cannot enforce. The worst shape measured is nested blocks, at about 14 KB per level; the
tests parse the deepest tree the parser accepts, for four shapes, on exactly the documented
stack, finding each shape's edge by asking the parser rather than by counting levels.

Neither budget is a language rule. Nazm bounds neither iteration nor recursion, and the
nesting limit is the parser's; this implementation has all three so that a runaway program
fails visibly instead of consuming a machine. A different implementation may choose
differently.

**`nazm build` is that different implementation, and it has chosen none of them.** A
compiled program has no iteration budget and no call-depth budget: a loop whose condition
never becomes false runs until something kills it, with no `N0402`. This is stated here so
that the difference is part of the record rather than a discovery. The two implementations
agree on what a *terminating* program produces; they do not agree on what a runaway one
does, and no test claims otherwise. Recursion is refused outright by that backend for the
related reason — see `docs/roadmap.md`, *Recursion: pending a native execution contract*.

### Strings are bytes, and what that commits to

`Str` is an **immutable sequence of bytes**. Not of characters, and not of Unicode scalar
values. The decision is forced rather than chosen: Nazm has no character type, and
inventing one to index strings with would be a second text abstraction added for the sake
of the first.

The commitments, each stated because leaving one to the host language's behaviour is how
two implementations end up disagreeing:

| | |
| --- | --- |
| **Length** | `str_len(s)` is the number of **bytes**. `str_len("é")` is 2 |
| **Indexing** | `str_byte(s, i)` is the byte at offset `i`, as an `Int` in `0..=255`. Never negative, never a code point |
| **Bounds** | `str_byte` fails with `N0405` unless `0 <= i < str_len(s)`. There is no wrapping, no clamping, and no sentinel return |
| **Slicing** | `str_slice(s, a, b)` is the bytes in `[a, b)`, and fails with `N0405` unless `0 <= a <= b <= str_len(s)`. `a == b` is the empty string, and `a == str_len(s)` is allowed |
| **UTF-8 boundaries** | A slice **may** split a multi-byte sequence. The result is a `Str` whose bytes are not valid UTF-8, and that is not an error. Validity is a property of *some* strings, not of the type |
| **Embedded NUL** | A `Str` may contain zero bytes, and they are ordinary bytes. Length comes from the length, never from a terminator |
| **Equality** | `==` and `!=` compare byte sequences. Equal length and equal bytes. No normalisation, no case folding, no collation |
| **Ordering** | `< <= > >=` do **not** apply to `Str`. Ordering text needs a collation decision this language has not made |
| **Concatenation** | `str_concat(a, b)` is the bytes of `a` followed by the bytes of `b` |
| **Conversion** | `int_to_str(n)` is the decimal form, with a leading `-` when negative. `int_to_str(0)` is `"0"`, and the full `Int` range round-trips |
| **Construction** | `str_from_byte(b)` is a one-byte `Str`, and fails with `N0405` unless `0 <= b <= 255`. v0.1 literals have **no escapes**, so this is the only way to write a byte you cannot type — a newline, a NUL, anything above 127 |
| **Allocation** | `str_concat`, `int_to_str` and `str_from_byte` allocate; `str_slice` does not. A compiled program reports `N0406` if the allocator refuses. The interpreter cannot: its host aborts on allocation failure, and that difference is stated rather than papered over |
| **Ownership** | Values, not references. Passing, returning, binding and copying a `Str` produce a string with the same bytes, and there is no way to observe whether a copy was made |
| **Mutation** | There is none. No operation changes an existing `Str` |

Source positions stay byte offsets throughout — `Span` is a byte range, diagnostics point
at byte ranges, and `str_len` counts the same units. A compiler written in Nazm therefore
reports positions that mean the same thing as the ones this implementation reports,
without a conversion step that could be got wrong.

#### What an implementation may not infer from its host

The reference implementation is written in Rust, whose `String` cannot hold invalid UTF-8.
That is a property of *that host*, not of `Str`, and the specification above is written to
what a byte sequence can do. Where the two differ — a slice that splits a code point — the
specification governs and the Rust interpreter's representation is the thing that has to
accommodate it.

This is not hypothetical. The interpreter's `Value::Str` was a Rust `String` when this
section was written, and `str_byte(str_slice("é", 0, 1), 0)` produced **239** — the first
byte of the replacement character the lossy conversion substituted — where the
specification requires **195**. The test that asserts 195 was written from this section
and found the defect immediately; the representation is now `Vec<u8>`. A host type
standing in for an unstated specification is the trap, and it had already been walked
into.

#### Built-ins share the ordinary function namespace

`str_len`, `str_byte`, `str_slice`, `str_concat` and `int_to_str` are called like any
other function and checked by the ordinary call rule. **They may not be redefined**: a
program declaring `fn str_len` is rejected (`N0203`) rather than shadowing the built-in,
because a call whose meaning depends on what happens to be in scope is exactly what this
language is trying not to have.

### Sequences are shared, mutable, and never freed

`Ints` and `Strs` are growable sequences of `Int` and of `Str`. They are the first
**reference** types in the language, and every consequence of that is stated here rather
than discovered.

Named `Ints` and `Strs` rather than `[Int]` and `[Str]` on purpose: the lexer has no
bracket tokens and the type grammar is a bare name, so concrete names commit to nothing
about how a future generic `[T]` will look. A half-built `[T]` syntax would.

| | |
| --- | --- |
| **Creation** | `ints_new()` and `strs_new()` produce a new, empty sequence |
| **Identity** | A sequence value is a **handle**. Binding, passing and returning one share the storage; nothing is copied. `let b = a;` makes `b` and `a` the same sequence, and a push through either is visible through both |
| **Mutation** | `ints_push`, `ints_set` and their `Strs` counterparts change shared storage. There is no immutable sequence and no copy operation |
| **Length** | `ints_len` / `strs_len`, in elements |
| **Indexing** | `ints_get(a, i)` / `strs_get(a, i)`, failing with `N0405` unless `0 <= i < len`. `ints_set(a, i, v)` has the same bound and returns **the index written** — the language has no unit type, and the index is the answer that is always meaningful |
| **Appending** | `ints_push(a, v)` returns the **new length** |
| **Growth failure** | Reported as `N0406` where the implementation can detect it. A failed growth leaves the sequence exactly as it was, never half-resized |
| **Equality** | `==` and `!=` are **refused** on a sequence (`N0304`). They would have to mean either "the same storage" or "the same contents", and the language has chosen neither; offering one silently is how a program comes to depend on the wrong answer |
| **Ordering** | Not applicable, for the same reason it is not applicable to `Str` |
| **Destruction** | **There is none.** A sequence lives until the process exits |
| **Element copying** | `strs_get` yields a `Str`, which is a value — changing the sequence afterwards does not change a `Str` already read out of it |

#### What this is not

It is **not an ownership system, and no safety is claimed from it.** Nothing prevents
aliasing, nothing tracks lifetimes, nothing detects a sequence used after the code that
built it has finished, and nothing is ever reclaimed. Peak memory grows with everything
a program ever allocates.

That is a deliberate bootstrap trade (`docs/bootstrap-design.md`, *Memory model*): a
compiler reads input, writes output and exits, and reclaiming inside that lifetime buys
nothing while costing an ownership system the bootstrap does not otherwise need. It is a
**stated limitation with a cost that must be measured**, not a design anybody should
inherit. A later allocator with reclamation is intended to change none of the meanings
above.

#### An expression may be a statement

`ints_push(a, 1);` is an expression evaluated for its effect, with its value discarded.
This is not new syntax — a block has always been statements and an optional tail — but
it is newly *useful*: without it, a language with no unit type would force
`let _unused = ints_push(a, 1);` at every mutation, which is a workaround for a missing
type rather than a rule worth having.

### What the outside-world built-ins do and do not promise

A program can read its arguments, read and write files, write to standard output and
standard error, and end the process. That is enough for a compiler, and it is all.

| | |
| --- | --- |
| **Arguments** | `args_count()` and `arg(i)`, **excluding the program's own name**. `arg(i)` fails with `N0405` outside `0..args_count()` |
| **Reading** | `read_file(path) -> Str` — the file's bytes, whatever they are. Fails with `N0407` |
| **Writing** | `write_file(path, data) -> Int` — truncating; returns the byte count. Fails with `N0407` |
| **Existence** | `file_exists(path) -> Bool` — whether it can be *opened for reading*, which is the question a program actually asks |
| **Output** | `print(s)` and `eprint(s)` write bytes to descriptors 1 and 2 and return the count. Neither adds a newline: `str_from_byte(10)` is how you ask for one |
| **Ending** | `exit_with(code)` ends the process. It never returns, which the type system cannot express, so it is written `Int` and the value never arrives |

Excluding the program name from `arg(i)` is deliberate: a compiled program's `argv[0]` is
a path and an interpreted one has no equivalent, so including it would make the same
program mean different things under `nazm run` and `nazm build`. Excluded, they agree
exactly.

#### Authority is ambient, and that is a limitation

**Nothing in the language restricts which functions may reach the file system.** Any
function can call `read_file`. There is no capability parameter, no effect annotation,
and no enforcement of any kind.

This is stated rather than implied because the alternative — shipping the built-ins and
describing the design as capability-based because a capability design exists in
`docs/architecture.md` — would be a claim the implementation does not support. Partial
enforcement labelled as full enforcement is worse than none.

The growth path is a capability parameter on `main`, threaded to whatever needs it. The
built-ins above are shaped so that adding one changes *what may call them*, not what they
mean, so no program's behaviour changes when it arrives. That design is **not settled**,
and in particular `main(io: IO)` is not chosen merely because an earlier note suggested
it.

#### There is no process spawning, deliberately

A Nazm program cannot run another program. A compiler written in Nazm therefore emits
LLVM IR to a file and a **script** invokes `clang` — which is the documented arrangement,
and much narrower than a language-level `run_command` would be. Adding one would be the
single largest authority grant available and it is not needed.

#### Errors are values only where they can be

There is no error type, no `Result`, and no exceptions. A failed read stops the program
with `N0407`. A program that wants to *handle* a missing file asks `file_exists` first
and reports it itself, which is what a compiler wants to do anyway. The gap between the
check and the read is a real race, and for a compiler reading its own inputs it is an
accepted one; closing it needs an error type, which is a later decision.

### A file is a module, and `use` names one by path

```nazm
use "util.nz";
use "sub/deep.nz";

fn main() -> Int { quad(5) }
```

| | |
| --- | --- |
| **Syntax** | `use "path";`, at the top level only. The path is a **quoted string**, not an identifier chain: a path is data, so there is no module-path grammar to design and nothing that could be mistaken for one |
| **Resolution** | Relative to the **importing file's** directory, always. Never to the working directory, never to a search path, never to an environment variable — a build that depends on where it was started is not reproducible |
| **Loading** | Every reachable file is read once, keyed by its canonical path, and concatenated into one buffer that is then checked as a single program |
| **Order** | The root first, then its imports depth-first in source order. Deterministic, and a function of the import graph alone |
| **Diamonds** | A file imported twice is loaded once |
| **Cycles** | **Allowed.** The second visit finds the file already loaded and stops. Nothing depends on order — signatures are collected before any body — so a cycle has no meaning to get wrong |
| **Names** | All top-level functions from every loaded file share one namespace. A name defined twice is `N0203`, and the diagnostic names **both files** |
| **Missing file** | `N0205`, pointing at the quoted path |
| **Declarations** | There is no `mod`. A file is the module |

#### Visibility is not expressible yet, and that has a cost

**Every top-level function is visible to every file in the program.** There is no `pub`,
no privacy, and no way to keep a helper to its own file.

This is enough for the compiler this subset exists to bootstrap, which has no secrets from
itself. It is recorded here rather than left implicit because the consequence is real:
**adding private-by-default later would break every multi-file program that relies on
today's behaviour.** Whatever visibility eventually arrives has to be added before this
part of the language is called stable, and a program written now should not assume
otherwise.

#### One buffer, and what that means for positions

The merged program is one byte buffer, so a `Span` stays a plain pair of offsets and
means the same thing everywhere. Diagnostics name the file an offset falls in and report
**line numbers within that file**, so a reader opens the file the message names and finds
the line it gives. A cross-file secondary label names its file too; without that,
"first defined here" would give a line number with no file to look it up in.

### Structured concurrency: scopes, tasks and channels

This is the **initial** model. It is deliberately small, and what it does not claim is as
much a part of the specification as what it does — see *What this model does not promise*
below.

```nazm
fn worker(jobs: Chan, results: Chan) -> Int {
    let buf = ints_new();
    while chan_recv(jobs, buf) {
        chan_send(results, ints_pop(buf) * 2);
    }
    0
}

fn main() -> Int {
    let jobs = chan_new(4);
    let results = chan_new(4);
    scope {
        spawn worker(jobs, results);
        spawn worker(jobs, results);
        let mut i = 0;
        while i < 10 { chan_send(jobs, i); i = i + 1; }
        chan_close(jobs);
    }
    chan_close(results);
    let buf = ints_new();
    let mut total = 0;
    while chan_recv(results, buf) { total = total + ints_pop(buf); }
    total
}
```

#### Scopes and tasks

| | |
| --- | --- |
| **Creation** | `spawn f(args);` starts a task running `f`. It is a statement, and `f` must be a named function — there are no function values, so there is nothing else it could be |
| **Where** | Only inside a `scope { … }` block. `spawn` outside one is `N0320` |
| **Joining** | A `scope` block does not finish until **every** task spawned inside it has finished. There is no way to leave a task running past its scope, and no way to detach one. That is the whole point of the word |
| **Nesting** | A `scope` may contain another. The inner one joins first, because it closes first |
| **The scope's value** | None. `scope { … }` is a statement |
| **Result propagation** | A task's return value is **discarded**. Results travel through channels, which is the one mechanism, rather than two |
| **Error propagation** | A task that fails at run time — overflow, a bad index, a failed allocation — ends the **whole program** with its diagnostic and exit status 2, exactly as the same failure would in `main`. Tasks do not have private failure modes |

#### When a task starts, and when a scope waits

**A task starts when its `spawn` runs**, not when the scope closes. It runs *alongside*
the rest of the scope's body.

This is not an implementation detail, and it is written down because the first
implementation got it wrong. Collecting the spawned tasks and starting them all at the
closing brace reads like a harmless reordering — every task still runs, and every task is
still joined. It deadlocks the example at the top of this section: the body sends ten jobs
into a channel of capacity four before any worker exists to take them, so the fifth send
blocks forever and the closing brace that would have started the workers is never reached.
That is the shape of every worker pool, so the property is part of the language.

The closing brace waits for every task, **including when the body itself failed**. A
failure that returned immediately would leave tasks running past the scope that promised
to contain them, which is the one thing a scope is for. When both the body and a task
fail, the body's failure is the one reported: a task that failed because the body stopped
feeding it is a consequence, not a cause.

There is no cancellation beyond closing a channel. A scope whose body fails still waits
for tasks that may be blocked on a channel nobody will now close, and such a program
hangs rather than reporting. Closing the channels a task waits on is the program's
responsibility, and the `while chan_recv(c, buf)` loop is the idiom that makes it work.

#### What may cross into a task

**`spawn` arguments may be `Int`, `Bool`, `Str` or `Chan`, and nothing else.** Passing an
`Ints` or a `Strs` is rejected with `N0321`.

The reason is not stylistic. `Ints` and `Strs` are handles to shared mutable storage with
no synchronisation of any kind (*Sequences are shared, mutable, and never freed*). Letting
one cross into a task would be a data race the language had invited, and no amount of
documentation would make it safe. `Int`, `Bool` and `Str` are values — a task gets its own
copy of what it was given, and nothing it does is visible to anyone else. `Chan` is the
exception on purpose: it is the primitive *designed* to be shared, and its operations are
the synchronisation.

A task also cannot see its caller's locals. It runs a named function with the arguments it
was given, and a function has never been able to see its caller's bindings.

#### Channels

`Chan` is a **bounded** queue of `Int`.

| | |
| --- | --- |
| `chan_new(capacity)` | A channel holding at most `capacity` values. `capacity` must be at least 1, or `N0405` |
| `chan_send(c, v)` | Blocks while the channel is full. Returns `true` when the value was queued, `false` if the channel is closed — in which case **the value was not sent** |
| `chan_recv(c, out)` | Blocks while the channel is empty. On success pushes one value onto `out` and returns `true`. Returns `false` when the channel is closed **and** drained |
| `chan_close(c)` | Closes it. Idempotent, and wakes every blocked sender and receiver |

Closing is the cancellation mechanism, and `chan_send` and `chan_recv` are the
**observation points**: a task that loops on `while chan_recv(c, buf)` stops when the
channel closes, without needing a separate cancellation flag. Values already queued when
a channel closes are still received — closing ends a channel, it does not discard it.

`Chan` is a reference, like a sequence, and is not comparable with `==`.

#### Lifetime and cleanup

A task's arguments are values or channels, so nothing it holds can dangle: values are
copied, and a channel outlives every scope that uses it because nothing is ever freed.
There are no destructors to run and nothing to reclaim — the same bootstrap memory model
the rest of the language has (`docs/bootstrap-design.md`).

Cleanup is therefore about *joining*, not about freeing, and joining is guaranteed by the
scope's closing brace.

#### What this model does not promise

**The runtime is one operating-system thread per task, and that is all it is.** Every one
of the following is *not* claimed, because none of it is implemented:

- No M:N scheduling, no green threads, no work stealing, no task density. A program that
  spawns ten thousand tasks will try to create ten thousand OS threads.
- **No deadlock freedom.** Two tasks that each wait for the other wait forever. Nothing in
  the capture rules prevents it, and no ownership discipline implies it. Neither does a
  scope: it waits for its tasks unconditionally, so a body that blocks on a task that
  cannot proceed hangs the program.
- **No cancellation** other than closing a channel. There is no cancellation token, no
  interruption, no timeout, and a failure elsewhere does not unblock anybody.
- No fairness guarantee between tasks blocked on the same channel.
- No memory-ordering guarantees beyond what the channel operations themselves provide.
  There is no shared mutable state to order, because the capture rule removes it.
- No priorities, no timeouts, no `select`.

The interface is deliberately separable from the runtime: `scope`, `spawn` and the four
channel operations say nothing about threads, so a different runtime strategy could
replace this one without changing what a program means.

### The integer boundaries, stated rather than inherited

Two's complement is asymmetric — there is one more negative `Int` than positive — and
every language that leaves the consequences to the hardware gets a rule with an exception
in it. Nazm states all four cases:

| expression | result | why |
| --- | --- | --- |
| `-9223372036854775808` | the smallest `Int` | The sign folds into the literal. The magnitude alone is out of range, so lexing the digits and negating afterwards would make the smallest `Int` unwritable — an arbitrary hole in the range |
| `-9223372036854775809` | error, out of range | The fold checks the magnitude rather than assuming any digits after `-` will fit |
| `-x` where `x` is the smallest `Int` | **traps** | The result has no representation. Traps in every build, like all overflow |
| `x / -1` where `x` is the smallest `Int` | **traps** | The quotient is one past the largest `Int`. On x86 this faults in hardware; a checked division is what turns it into a diagnostic instead of a dead process |
| `x % -1` where `x` is the smallest `Int` | **0** | Mathematically zero, and zero is representable. The same hardware instruction faults on it and Rust's `checked_rem` reports nothing, but inheriting that would make the rule *"`%` fails on a zero divisor, and also on one exact pair of operands"* — a wart to memorise for no gain |

The through-line: `%` has exactly one failure case, a zero divisor. Overflow traps
wherever the result genuinely cannot be represented, and nowhere else.

### One mistake, one error

A parse error inside a function body does not delete the function. Its signature is kept,
so calls to it still check against real parameter and return types instead of reporting
that it does not exist — which would be several errors for one mistake, the exact failure
this language is meant to avoid. Nothing is said about the body itself: statements that
were never built would be claims about the parser's guess, not about the program.

### Not in v0.3, and unsupported rather than half-present

Valueless `return`, labelled break, `for`, `loop`, mutable parameters, effects, I/O,
generics, user-defined types, modules, string concatenation, floating point. A program using any of them is
rejected by name rather than misparsed into something else.

`nazm run` prints the value of `main`. There is deliberately no `print`: output is an
effect, effects are the design's central mechanism, and a builtin that bypassed the effect
system before it exists would be the wrong shape to unpick later.

## Open — later phases must settle these

### Memory and values

- Exact copy semantics: when is a value copied, when projected, when moved?
- Closure capture: which conventions may a closure capture under, and what escapes?
- Copy elision: what does the compiler *guarantee*, versus what does it merely usually do?
  MVS is unusable if this is folklore.
- Arena/region assignment rules, and how region inference interacts with the exclusivity
  check when they are (by design) the same solve.

### Effects

- **Is divergence an effect?** The `forever()` counterexample in `architecture.md` §5 shows
  that without an answer, "drop an unused pure call" is unsound. Options: treat divergence
  as an effect; track termination separately (`willreturn`-style); or refuse the
  transformation entirely.
- Is allocation failure an effect, a panic, or a return value?
- Is panic an effect, an unwind, or both, and what is observable about it?
- Evaluation order: specified, or deliberately unspecified? If unspecified, differential
  testing across backends cannot compare it (`architecture.md` §6).
- Which handlers lower statically, and which require one-shot continuation capture?

### Provenance and information flow

- Implicit flows: is branching on a `Secret` a leak? Full non-interference is usually
  unusable in practice; what is the chosen approximation, and what does it miss?
- Sanitizer functions: how are they declared, and what stops a no-op being declared one?
- Aliasing: how does a label propagate through a projection?
- Declassification: which capability, granted where, audited how?
- The FFI boundary: what label does a value returned from C carry?

### Concurrency

- Captured variables, globals/statics, and foreign pointers are **not** covered by channel
  ownership transfer. Each needs a rule.
- Preemption: cooperative at back-edges and calls, or signal-based?
- What exactly does cancellation cancel, and what cleanup is guaranteed to run?

### Layout selection

- The eligibility predicate: which types may the compiler re-lay-out?
- Behaviour under address identity, escaping references, FFI, serialization, atomics, and
  separate compilation (`architecture.md` §4).

### Backend semantics

Required before `nazm test --backend=all` means anything:

- integer overflow behaviour
- floating-point semantics (fast-math? reassociation? NaN payloads?)
- panic and unwind behaviour
- what counts as an observable output

### Syntax

- The surface syntax itself, which is the **largest lever against the training-data risk**
  and is nearly free to change now and effectively unchangeable later. Novelty is a budget:
  spend it only where it buys semantics unobtainable otherwise.
- A machine-readable grammar (needed as a spec artifact regardless of whether
  grammar-constrained decoding is ever used).
- A cheatsheet under ~3k tokens, to be injected by `nazm init`.

## Non-goals for the current interpreter milestone

Deliberately out of scope:

macros · comptime beyond constant folding · general effect handlers · whole-program taint ·
layout selection · a package manager · a module system beyond files

**Self-hosting was on this list and has been moved off it.** It is now a stated long-term
objective — see [`direction.md`](direction.md) — and it remains outside this milestone,
which is a different thing from being a non-goal. The staged path is
[`roadmap.md`](roadmap.md) M3, and it is deliberately preceded by M2: the language has no
collections, no user-defined types, no pattern matching, no string concatenation and no
I/O, so a compiler cannot be written in it yet at all.
