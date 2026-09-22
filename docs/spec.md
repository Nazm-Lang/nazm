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


### Records — the first user-defined type

Settled by N9, 2026-09-23. A **record** is a nominal product type with named fields. The
source keyword is `struct`; the word *record* names the category everywhere else.

```nazm
pub struct Point {
    x: Int,
    y: Int,
}
```

#### One concept, two words, and why that is not two spellings

`struct` was reserved by name from the beginning — `crates/nazm-syntax/src/parser.rs`
refused it with *"there are no user-defined types yet"* — so the keyword was chosen before
this milestone and N9 only stopped refusing it. There is no `record` keyword and never
will be: a language with two spellings for one thing has two things to explain. The word
*record* is used for the semantic category (`DefKind::Record`, `docs/architecture.md`,
this section) precisely because it is **not** a keyword, so it can name the idea without
being mistaken for syntax.

#### Nominal, always

Two records with identical fields are different types.

```nazm
struct A { x: Int, }
struct B { x: Int, }
```

There is no conversion between `A` and `B`, implicit or explicit, and no structural
subtyping. A record's identity is its **declaration**: the module it is declared in and
the name it is declared under, which is the same rule a function's identity follows and is
spelled the same way — `model.nz::record Point`. Two modules may each declare `Thing`, and
they are two types.

#### Types and functions are separate namespaces

A module may declare a record `Point` and a function `Point`. Which one a name means is
decided by where it is written: a type position (`: T`, `-> T`) means the type, and a call
means the function. The grammar always knows, and that is the condition under which two
namespaces are worth having.

Built-in type names are **not** in the type namespace and cannot be shadowed: `struct Int`
is `N0331`. `Int` means the same thing in every module of every program.

Collisions between two imported types, or between an imported type and a local one, are
refused where they are created — the same rule and the same codes as the function
namespace, `N0207` and `N0208`. A name is never silently shadowed.

#### A record is a value, and its fields keep their own semantics

**Assignment creates another record value.** A record has no object identity: there is no
address-of, no reference equality, and no operation through which two bindings of one
record could be observed to be the same object. What each field does when the record is
copied is **that field type's existing law**, unchanged:

```nazm
struct Box {
    count: Int,
    name: Str,
    values: Ints,
}

let mut a = Box(count: 1, name: "n", values: ints_new());
let b = a;
```

| Field | After `let b = a` |
| --- | --- |
| `count` | two independent `Int`s. `a.count = 7` does not change `b.count` |
| `name` | two `Str` values with the same bytes. Whether they share a backing is unobservable, exactly as *Strings are bytes* already promised |
| `values` | **two references to one sequence.** `ints_push(a.values, 7)` is visible through `b.values` |

That last row is the design, not a concession. A record is a value; an `Ints` is an owned
mutable handle; a record *containing* one contains the handle, so copying the record copies
the handle and both name one storage. Making `Ints` copy because it sits inside a record
would have given the same type two meanings depending on where it was written.

Consequently the memory constitution needs **no fifth kind of value** for records. A record
is a composition of kinds, and every rule below is derived from its fields rather than
declared for it:

| | |
| --- | --- |
| **copy** | copy or share each field, by that field's rule |
| **destroy** | release each field, by that field's rule, in an order no program can observe |
| **needs cleanup** | some field does, recursively |
| **may cross into a task** | every field may, recursively |

#### Construction names every field

```nazm
let p = Point(x: 1, y: 2);
```

Named, never positional, so field order is never an implicit API. Every field is given
exactly one value: a missing one is `N0334`, an unknown one `N0333`, a repeated one
`N0332`. There are no default values, no positional fallback, and no update or spread form.

**Why the parenthesised form and not `Point { x: 1 }`.** The braced form is more familiar
and it collides with the language's own grammar: `if p { … }` is an `if` whose condition is
a bare name, and `p { … }` would be a record literal, and no amount of parser lookahead
turns that into a decision rather than a guess. The parenthesised form is told from a call
by two tokens — `IDENT "(" IDENT ":"` — and that *is* a decision, because no argument
expression can begin with a name followed by a colon. The smallest grammar with the fewest
contextual exceptions won.

**Initialisers evaluate in the order written.** `Point(x: f(), y: g())` calls `f` before
`g`, whatever order the fields were declared in and wherever the backend puts them. Physical
layout is unrelated to evaluation order, and a backend that filled fields in layout order
would have made field order observable through side effects.

**A construction that fails part way through cleans up.** If an initialiser fails after an
earlier one has produced a heap string or a sequence, that value is released. A record is
either fully initialised or it does not exist; there is no partially initialised record a
program can reach.

**A record with no fields is refused** (`N0101`), because `Marker()` is exactly how a call
to a function named `Marker` is written, and types and functions are separate namespaces,
so neither reading could be preferred. If a later milestone gives constructions a form that
cannot be a call, the refusal can be lifted without changing anything else.

#### Field order is not part of what a record means

Moving two field declarations changes nothing: not the type, not what constructs it, not
the module's interface fingerprint, and not the native layout. Construction is by name and
the persisted interface sorts fields by name, so the fingerprint cannot move; the backend
lays fields out in **canonical field-name order**, so the object file cannot either. An
edit the language says means nothing also *does* nothing — which is the only honest way to
make the claim, since the alternative is an ABI difference produced by an edit the compiler
itself calls meaningless.

#### Reading a field

`p.field` produces a value according to the field type's own law: an `Int` copies, a `Str`
is a value with the same bytes, an `Ints` or a `Chan` is another reference to the same
storage, and a nested record is another record value whose fields follow these rules again.

Field lookup starts from the **resolved type of the expression it is read from**, and goes
nowhere else. There is no global field namespace: `value.y` where `value` is an `A` that
has no `y` is `N0333`, whatever other record has a `y`.

**A projection out of a temporary is valid.** In

```nazm
let s = make().name;
```

`s` is a valid `Str` after the record `make()` returned has been destroyed. The value read
out holds whatever reference it needs before the record it came from is released; this is
rule 3 of the memory constitution applied to a projection.

#### Binding mutability reaches every field

```nazm
let p = Point(x: 1, y: 2);
p.x = 2;                      // N0310 — `p` is not mutable
```

```nazm
let mut p = Point(x: 1, y: 2);
p.x = 2;                      // fine
```

Mutability is checked at the **root binding** of the place being written, at any depth:
`a.inner.count = 3` needs `let mut a`. A record is a value, so replacing part of what a
binding holds is changing the binding — the same act as `p = q`, and it needs the same
permission.

Parameters remain immutable bindings, so a `struct` parameter's field cannot be replaced
either. There is no `inout`.

**Mutating a field and mutating what a field refers to are different things.** Given
`w: Work` with a field `values: Ints`, `w.values = other` is refused and
`ints_push(w.values, 1)` is not — the second changes the sequence, not the record. This
distinction already existed for sequence parameters; records inherit it rather than
introduce it.

**Replacement secures the new value before releasing the old.** `p.name = e` evaluates `e`,
takes whatever reference the new value needs, releases the old field, and stores. So
`p.name = p.name` is safe, a new value backed by the same allocation as the old one is
safe, and a failing `e` leaves the old field intact and valid.

#### Parameters, returns and tasks

A record parameter is **borrowed**, like every other parameter: calling `size(p)` neither
consumes nor duplicates the caller's record, and the callee cannot keep it. Returning a
record **transfers**, like every other return: each owning field survives into the caller,
on both the tail-expression edge and the explicit `return` edge.

Whether a record may cross into a task is **derived, not declared**: a record may cross
exactly when every field may, recursively. There is no marker to write and none to forget.

```nazm
struct Message { id: Int, text: Str, reply: Chan, }   // may cross
struct Work { values: Ints, }                          // may not — N0321
```

A record containing an `Ints` at any depth may not cross, and the diagnostic names the
field path that decided it — `Outer.inner.values` rather than "this record cannot cross".
Wrapping a sequence in a record does not launder it past `N0321`.

#### Visibility, and the API being closed

Record declarations follow the existing law: private by default, `pub` exports, imports
non-transitive. N9 records are **transparent** — a module that can see a record can
construct it and project its fields — so there are no private fields, no opaque records and
no accessors.

Because there are no opaque types, a public item may not name a private one:

```nazm
struct Secret { value: Int, }
pub fn get() -> Secret { … }              // N0337
pub struct Public { secret: Secret, }     // N0337
```

An importer would have to name the type to use the item and cannot, so the interface would
be one nobody could consume. The diagnostic names both the public item and the private type
it exposes.

A module's persisted interface carries every record definition its public surface mentions,
closed over field types — including one declared in a third module, because an importer has
to understand a shape it can project through and cannot go and read a module it never
imported. Carrying the *shape* is not carrying the *name*: nothing extra becomes writable,
and the non-transitive import rule is unchanged.

#### Recursion is refused as an impossible layout

```nazm
struct Node { next: Node, }        // N0336
struct A { b: B, }                 // N0336, including across modules
struct B { a: A, }
```

A record contains its fields **by value**, so a chain of record-typed fields that returns to
where it started describes a value that contains itself, and no finite layout exists. This
is not a memory-safety refusal and it is not converted into a pointer behind the reader's
back: it is an impossible layout, stated as one.

The rule is written over **inline value containment**, deliberately, so that a future type
which holds its contents somewhere else makes the same shape expressible without this rule
changing. A module import cycle remains legal and is a different thing entirely; only a
layout cycle is refused.

Records nested inside records are bounded at 32 levels (`N0338`). The bound is on the
emitted program rather than on the compiler — each level multiplies the copy and cleanup
code generated for the outer record — and it is stated rather than discovered.

#### What records are not, in N9

No variants, no pattern matching, no generics, no traits, no methods, no `impl`, no
closures, no `Option` or `Result`, no user-defined destructors, no operator overloading, no
inheritance, no field-level visibility, no default values, no update syntax.

**No derived equality.** `a == b` on records is `N0304`. Equality would have to decide what
a sequence field means — the same storage or the same contents — and the language has not
chosen; sequences and channels are refused by `==` for the same reason and records join
them rather than get an exception.

**`pub` still promises nothing about a binary.** A record's layout, its field offsets, its
size and its calling convention are compiler-private and may change between builds. `pub`
says which *Nazm modules* may name the type. There is no `repr`, no FFI layout, no
serialisation format and no storage layout; each of those needs an explicit contract and
none is made here.

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

#### Where a loop target binds

**A loop's condition is not inside that loop.** A `break` in a `while`'s condition leaves
the *enclosing* loop; a `continue` there starts the enclosing loop's next iteration. In the
outermost loop's condition there is no enclosing loop, so it is `N0312`.

*Moved here from `roadmap.md` on 2026-09-21: it is a rule about what a program means, so a
plan was the wrong owner — and three source files were citing the plan for it.* The rule
was read out of the implementations rather than assumed: `check.rs` raises `loop_depth`
around the body only, and `eval.rs` evaluates the condition outside the loop's own flow
handling, so a `Flow::Break` raised there is consumed by the enclosing loop. Two probe
programs pin it — a `break` in an inner loop's condition prints `102` and a `continue`
there prints `204`, both only explicable if the target is the outer loop. The native
backend pushes a loop's targets around its **body and not its condition**, and asserts the
same two numbers.

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

#### Why native recursion is refused rather than counted

*Condensed here from `roadmap.md` on 2026-09-21.* A call-depth counter alone **does not**
guarantee a diagnostic before the native stack is exhausted: a counter bounds the number of
frames, and whether that many frames fit is a question about frame size and about how much
stack the process actually has. A budget of 512 says nothing until both are known.

The interpreter never relied on a counter alone. `--max-depth` is one half of a pair —
`nazm_core::eval::on_interpreter_stack` runs the pipeline on a thread whose stack it sizes
*from the budget it is about to enforce*, with `N0404` when the host will not grant the
reservation. It is the **combination** that makes `N0403` arrive before the stack runs out.
Copying only the counter into generated code would copy the half that does not carry the
guarantee, and neither quantity — the platform's main-thread stack, or a generated
function's frame size — has been measured for native code. Emitting a bound derived from
neither would be a guarantee with no basis. So recursion is **refused by name** until the
contract exists, by both compilers.

**The general rule, of which these are instances: a runtime resource limit is an
implementation contract, never a silently invented language rule.** Where an
implementation imposes one it is stated, named in diagnostics, and recorded as that
implementation's choice — not promoted into the semantics because it happens to hold.

**`nazm build` is that different implementation, and it has chosen none of them.** A
compiled program has no iteration budget and no call-depth budget: a loop whose condition
never becomes false runs until something kills it, with no `N0402`. This is stated here so
that the difference is part of the record rather than a discovery. The two implementations
agree on what a *terminating* program produces; they do not agree on what a runaway one
does, and no test claims otherwise. Recursion is refused outright by **both** compilers for
the related reason — `nazm build` since the native path existed, and the compiler written
in Nazm since 2026-09-21, when a measured divergence between them was closed. The reason
it is a refusal rather than a counter is below, under *Why native recursion is refused
rather than counted*.

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

That is a deliberate bootstrap trade (`docs/bootstrap.md` §1, *the memory model*): a
compiler reads input, writes output and exits, and reclaiming inside that lifetime buys
nothing while costing an ownership system the bootstrap does not otherwise need. It is a
**stated limitation with a cost that must be measured**, not a design anybody should
inherit. A later allocator with reclamation is intended to change none of the meanings
above.

> **Superseded in part by N7, 2026-09-22.** *"Never freed"* and *"a sequence lives until
> the process exits"* described every implementation up to that milestone. Sequence storage
> is reclaimed now, under the rule in *The memory constitution* below, and **none of the
> meanings above changed** — which is what the last sentence promised. Everything else in
> this section still holds: sequences are still shared, still mutable, still aliasable, and
> a push through one binding is still visible through every other.
>
> **Extended by N8, 2026-09-22.** The same rule now covers `Str` and `Chan` storage, so
> *nothing* in the current type universe is left to the process's exit. Two entries in the
> table above are worth reading again in that light. *Destruction* is no longer "there is
> none": a sequence's storage goes when the last reference to it dies, and a `Strs`
> releases every string in it first. And *element copying* — "changing the sequence
> afterwards does not change a `Str` already read out of it" — used to be true because
> nothing was ever freed. It is true now because `strs_get` hands back its own reference to
> the bytes, which is the same promise resting on something that will go on being true.

#### An expression may be a statement

`ints_push(a, 1);` is an expression evaluated for its effect, with its value discarded.
This is not new syntax — a block has always been statements and an optional tail — but
it is newly *useful*: without it, a language with no unit type would force
`let _unused = ints_push(a, 1);` at every mutation, which is a workaround for a missing
type rather than a rule worth having.

### The memory constitution: four kinds of value, and when storage is reclaimed

Settled by N7, 2026-09-22, from an audit of what the two implementations actually do
rather than from what a memory model is usually shaped like. `docs/architecture.md` §7.7
carries the implementation and the evidence; this section is the law.

The whole of it is four kinds of value and five rules. Nothing here is new syntax, nothing
here is an annotation, and **no program that was legal before this section existed changed
its meaning because of it.** That was a requirement rather than an outcome: a memory model
that had to break aliasing to be implementable would have been the wrong model for a
language whose specification already promised aliasing.

#### The four kinds

| Kind | Types | Assignment produces | Storage |
| --- | --- | --- | --- |
| **Immediate** | `Int`, `Bool` | an independent copy | none |
| **Shared immutable value** | `Str` | a value with the same bytes | reclaimed when the last value naming it dies; shared or copied **unobservably** until then |
| **Owned mutable handle** | `Ints`, `Strs` | another reference to the same storage | reclaimed when the last reference dies |
| **Runtime-managed handle** | `Chan` | another reference to the same channel | the runtime's, reclaimed when the last reference dies |

There are four because the language has four kinds of thing, not because four was a good
number. A fifth would need a value that is none of: a number, an immutable byte string, a
mutable container, or a synchronisation primitive.

#### The five rules

**1. Assignment never copies a handle, and never observably copies a `Str`.**
`let b = a;` makes `b` refer to what `a` refers to. For `Ints` and `Strs` that sharing is
observable and specified above. For `Str` it is not observable at all — *Strings are bytes*
already says passing, returning, binding and copying produce a string with the same bytes
and there is no way to tell whether a copy was made — so an implementation may share the
bytes, copy them, or intern them. The reference interpreter copies and the native backend
shares, and both conform.

**2. A parameter is borrowed.** A function may read a handle, mutate what it refers to, and
pass it on. It may not keep it after it returns, and the language gives it nowhere to keep
one: a handle can be stored only in a local binding, and a local binding dies with its
frame. Parameters are not mutable bindings (*Mutation is opt-in*), so a callee cannot even
rebind its own parameter. The caller's reference is unaffected by the call and is still
valid after it.

**3. Returning transfers.** A function that returns a handle hands its caller a reference
that is independent of anything the callee still held. The caller's binding is then one of
the references that keeps the storage alive.

**4. Storage is reclaimed when the last reference to it dies, and never before.** A
reference is a binding, an argument in a call that has not returned, or a value on its way
out of a `return`. An implementation may reclaim later than this and may not reclaim
earlier. It is deliberately not stated *how*: reference counting, an escape analysis, and a
region scheme all satisfy it, and `architecture.md` §7.7 records which one is implemented
and why.

This is where the milestone's scope is stated rather than implied:

| | |
| --- | --- |
| `Ints`, `Strs` storage | reclaimed |
| `Str` storage | reclaimed **since N8**. The line above read *"not reclaimed"* and gave the reason: a `Str` may point into a literal, into the process's arguments, into a heap buffer, or into the middle of any of those, and the value did not say which. It says which now — `architecture.md` §7.8 — and the four origins are classified rather than guessed at |
| `Chan` storage | reclaimed **since N8**, including its queue, its mutex and its condition variable |
| the interpreter | reclaims under the same rule, and models the same backing rather than inheriting the host's |

**5. What crosses a task boundary is decided by kind.** An immediate crosses by copy. A
`Str` crosses by immutable sharing, which is safe precisely because nothing can mutate it.
A `Chan` crosses by runtime-managed sharing, which is what it is for. A handle **does not
cross at all** — `N0321` refuses it, and that is unchanged.

That refusal is load-bearing rather than incidental. Because a handle cannot reach a second
task, its reference count is never touched by two threads at once, so reclaiming one needs
no atomic operation and no lock. A language rule that existed for data-race safety pays for
itself a second time here.

**And the converse, which N8 had to settle.** A `Str` and a `Chan` *do* cross, so whatever
keeps their storage alive is reached by more than one thread and must be safe under that. A
task can be handed a string, slice it, and bind the slice; two sibling tasks doing that to
the same bytes are two threads adjusting one thing. So the implementation pays for a
thread-safe mechanism there and does not pay for one on a sequence. Neither half is a
judgement about how likely sharing is — each follows from a rule the language already had,
and if `N0321` were ever relaxed the sequence half would have to change in the same commit.

What the *language* says is only this: **immutable string bytes remain valid for every live
`Str` value, and a channel's storage remains valid while any reference to it exists.** That
is a rule about validity, not about counting. `architecture.md` §7.8 records that counting
is what is implemented and why; a future arena, an interning pass, a small-string
optimisation or an escape analysis would satisfy the same sentence.

#### What this costs, stated so it can be checked

Nazm does not call something automatic and leave the reader to find out what it costs.

| Statement | May allocate | May copy | May adjust a reference count | May synchronise | May block |
| --- | --- | --- | --- | --- | --- |
| `let n = a + b;` | no | no | no | no | no |
| `let s = str_concat(a, b);` | **yes** | **yes** | no | no | no |
| `let xs = ints_new();` | **yes** | no | no | no | no |
| `let ys = xs;` | no | no | **yes** | no | no |
| `f(xs)` | no | no | no | no | no |
| `return xs;` | no | no | **yes** | no | no |
| `ints_push(xs, v)` | **yes**, on growth | no | no | no | no |
| a binding going out of scope | no | no | **yes**, if it holds one | no | no |
| `chan_send(c, v)` | no | no | no | **yes** | **yes** |
| `let t = str_slice(s, 1, 3);` | no | no | **yes** | no | no |
| `let b = a;` on a `Str` | no | **permitted, and not done** | **yes** | no | no |
| `strs_push(xs, s)` | **yes**, on growth | no | **yes** | no | no |
| `strs_get(xs, i)` | no | no | **yes** | no | no |
| `let c2 = c;` on a `Chan` | no | no | **yes** | no | no |
| `spawn f(s)` | **yes**, for the task's arguments | no | **yes** | **yes** | no |
| `let p = Point(x: 1, y: 2);` | no | no | no | no | no |
| `let q = p;` on a record with owning fields | no | no | **yes, one per owning field** | no | no |
| `p.x` where `x` is an `Int` | no | no | no | no | no |
| `p.name` where `name` is a `Str` | no | no | **yes** | no | no |
| `p.name = s;` | no | no | **yes**, one taken and one released | no | no |

Two entries in that table deserve their emphasis. **Passing a value to a function costs
nothing**, which is rule 2 turned into a number: the caller keeps its reference across the
call, so nothing has to be counted. And **the work happens only where something takes a
reference or lets one go** — a binding, a return, a container that keeps what it is given —
which is a place a reader can see and a tool can list.

Two rows say "yes" where a reader might expect "no". `str_slice` does no copying and still
adjusts a count, because the view it produces has to keep the bytes it points into alive.
And a `spawn` argument is held for the task rather than borrowed from the statement that
started it, because the task outlives that statement: a binding reassigned inside the scope
would otherwise release storage the task is still reading.

#### Cycles

A reference cycle is impossible in the language as it stands, and not by luck: a `Ints`
holds `Int`, a `Strs` holds `Str`, `Str` is a leaf, and a `Chan` carries `Int`. **No value
can refer to another value that can refer back.** So counting references reclaims
everything, and the leak freedom claimed for every current heap-backed type is a claim
about this type system rather than about counting in general.

A recursive user-defined type would end that, and the constitution says in advance what
admitting one requires: either the type system refuses ownership cycles, or the value kind
that can form them arrives with its own policy — a weak reference, a region, or a
collector for that kind alone. **What may not happen is a recursive type being added and
the reclamation claim left standing.**

**N9 took the first option, 2026-09-23.** A record holds its fields by value, so a
containment cycle has no finite layout and is refused outright (`N0336`) — which means the
paragraph above is satisfied by the type system rather than by a new policy, and the
argument is unchanged: a record adds no new *kind* of value, only a composition of existing
ones, and a composition of leaves is a leaf. The claim therefore now covers `Int`, `Bool`,
`Str`, `Ints`, `Strs`, `Chan` **and every record built from them**. What would end it is a
type that can refer to another value indirectly — a pointer, a reference, or a generic
container able to hold a record — and none of those exists.

#### What is not promised

- **Leak freedom for a whole program, as a permanent property.** Every heap-backed type
  the language *currently has* is reclaimed, which is a claim about `Int`, `Bool`, `Str`,
  `Ints`, `Strs`, `Chan` and records composed of them, and about nothing else. It rests on the cycle argument below,
  which is a fact about this type system rather than about counting in general: the first
  recursive user-defined type ends it, and the paragraph after next says in advance what
  admitting one requires. `capability-matrix.md` states the scope per type rather than as
  one word.
- **Reclamation of storage the program did not allocate.** A string literal lives in the
  object's read-only data and a process argument belongs to the operating system. Neither
  is freed, neither is counted, and neither is a leak.
- **Deadlock freedom.** Unchanged, and still not promised.
- **A destructor.** There is no user-visible destruction, no RAII and no ordering a program
  can observe, because nothing a program can write runs at reclamation. A record is
  destroyed by releasing each of its fields, and the order between fields is deliberately
  unspecified — no program can observe it, and fixing it in the specification would be
  promising something the language has no way to let anyone use.
- **Zero-cost record copying.** A record with many owning fields costs one reference
  adjustment per owning field, per copy. The design leaves that optimisable — a copy whose
  source dies immediately after needs no traffic at all — and until it is measured, nothing
  here claims it is free. `docs/performance.md` carries the numbers.

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
| **Loading** | Every reachable file is read once, keyed by its canonical path, and parsed on its own text. The result is a **graph**: modules, and which modules each one imports |
| **Order** | The root first, then its imports depth-first in source order. Deterministic, and a function of the import graph alone |
| **Diamonds** | A file imported twice is **one module**, not two. Its definitions exist once and have one identity |
| **Cycles** | **Allowed.** See below |
| **Missing file** | `N0205`, pointing at the quoted path |
| **Declarations** | There is no `mod`. A file is the module |

#### One file is one module is one compilation unit

A module is not a namespace bolted onto a program that was really one file all along.
**It is the unit of meaning**: a module owns its definitions, decides which of them
anyone else may name, and is checked against the *interfaces* of the modules it imports
rather than against their bodies.

That property is the point, and it is worth stating as a test rather than as an intention:
*given module A's source and the interfaces of the modules A imports, A can be checked
with no access to those modules' bodies.* Everything below follows from wanting that to
be true, and `architecture.md` §7.2 says where each half of it lives.

What one module is has a boundary today and will move: a package will eventually be able
to present several files as one unit, and a generated or virtual source has no file at
all. So the semantics below are written in terms of **modules**, and "one file is one
module" is how modules currently come into existence rather than what a module is.

#### Visibility: private by default, `pub` exports

```nazm
// lib.nz
pub fn parse(s: Str) -> Int { digits(s) }   // exported
fn digits(s: Str) -> Int { str_len(s) }     // private to lib.nz
```

| | |
| --- | --- |
| **Default** | A top-level definition is **private to its module**. Nothing outside the module may name it |
| **`pub`** | Written before `fn`, it exports that definition: modules that directly import this one may name it. That is all it does — it does not change the definition's meaning, its type, or how its own module sees it |
| **Granularity** | One bit. There is no `private`, no package visibility, no friend module, no export list, and no re-export |
| **Within a module** | Private and public definitions are alike: a module sees everything it defines, in any order |
| **Built-ins** | Unaffected. They are not a module's definitions and `pub` has nothing to say about them |

**`pub` is about names, not about safety.** It says which names another module may write,
and nothing about capabilities, effects, or what a definition may do. Confusing the two is
how a visibility keyword ends up load-bearing for security in a language that never
promised it.

*Changed on 2026-09-22.* Until then every top-level function in a program was visible to
every file in it. The old text recorded the consequence honestly — "adding private-by-default
later would break every multi-file program that relies on today's behaviour" — and that is
what happened: the multi-file programs in this repository, including the self-hosted
compiler, were migrated by marking their genuine cross-module APIs `pub`. A program written
against the old rule does not silently change meaning; it stops compiling, naming the
definition and the module that cannot see it.

#### `use` makes an interface visible, not a body

`use "lib.nz";` means: **the names `lib.nz` exports may be written in this module,
unqualified**. It is not textual inclusion, and it is not a request to compile anything —
a module is part of the compilation because it is reachable, and `use` decides what this
module may *see*, which is a different question.

| | |
| --- | --- |
| **What it brings in** | Exactly the exported definitions of the named module. Its private definitions are not brought in, and neither is anything *it* imported |
| **Spelling** | Unqualified. An imported `parse` is written `parse`, the same as a local one. There is no qualified path syntax and no alias |
| **Direction** | One way. Importing a module does not let it see the importer |
| **Self-import** | A module that imports itself already sees all its own definitions, so the import adds nothing and is not an error |
| **Duplicate import** | Importing the same module twice adds its exports once |

##### Imports are not transitive

If `A` imports `B` and `B` imports `C`, then **`A` cannot name `C`'s exports.** `B`
depending on `C` is `B`'s business; it is not an interface `B` offers, and a language in
which it were would make every dependency's dependency part of every module's vocabulary.

To use `C`, `A` imports `C`. That is one line, and it makes the dependency true in the
source instead of accidental.

This is the rule with the largest effect on how much context reading one module requires —
the *reason* Nazm has module interfaces at all rather than one program-wide namespace. See
`master-architecture.md` on context routing.

#### Collisions are errors, not silent choices

Import order decides nothing. Every one of these is refused, with both sides named:

| Situation | Code | Where it points |
| --- | --- | --- |
| A module defines the same name twice | `N0203` | The second definition, with the first as a secondary label |
| A module defines a name and imports a module exporting that name | `N0208` | The `use`, with the local definition as a secondary label |
| Two directly imported modules export the same name | `N0207` | The second `use`, with the first as a secondary label |
| A module names a definition that exists in an imported module but is private there | `N0206` | The name, with its private definition as a secondary label |
| A definition uses the name of a built-in | `N0203` | The definition |

**Two modules may each define a private `helper`.** They are different definitions and
neither is in the other's way — that is what owning your names means, and a module system
in which it were not true would be a naming convention.

`N0207` is reported where the ambiguity is created, not where an ambiguous name is used.
The alternative — allowing the import and refusing only a mention of the contested name —
is defensible in a language with qualified paths, because there the program can say which
one it means. This one cannot: with no way to write the distinction, a deferred error only
moves the same dead end further from its cause.

`N0206` exists so that privacy does not present as absence. "`digits` is not defined" is
true from the caller's position and useless: the reader can see it defined in the file they
just imported. Saying that it is defined and private names the actual repair, which is to
export it or to stop calling it.

#### Cycles remain legal

`A` may import `B` while `B` imports `A`. Checking is two stages, and that is what makes it
well defined:

1. **Interfaces**, for every module in the graph: each module's declarations, their
   signatures, and which are exported. Reading a declaration requires no other module.
2. **Bodies**, each module against its own definitions plus the interfaces of the modules
   it directly imports.

Neither stage needs a module ordering, so there is no ordering for a cycle to violate. The
language has no top-level initialisation — no globals, no static constructors, nothing that
runs before `main` — so there is no initialisation order a cycle could make ambiguous
either. A cycle would become a real question the moment either of those changed; until then
outlawing it would be an implementation's convenience written into a language.

#### The root module owns `main`

The program's entry point is **`main` in the root module** — the file named on the command
line. `N0204` is reported when *that module* has no `main`, whatever other modules contain.

An imported module's `main` is an ordinary definition of that module. It is private unless
exported, it does not become a second entry point, and it does not collide with the root's.
A helper file with a `main` of its own — a module that is also runnable on its own — is a
normal thing to write, and until 2026-09-22 it was not expressible: two `main`s in one
compilation were a duplicate definition, so imported files had to rename theirs.

`main` need not be `pub`. Being the entry point is not the same as being importable, and
requiring the keyword would suggest the runtime resolves it by name from outside.

### Durable identity: what makes this the same module, or the same definition

*Added by N3, 2026-09-22.* Everything above is about one compilation. This is about two.

A compiler that wants to reuse work has to answer a question no single compilation ever
asks: **is this the same thing it was last time?** Load order cannot answer it — the ids a
compilation mints are positions in that compilation — and neither can a file's path, which
says where a checkout happens to live. So the language settles what continuity means, and
an implementation encodes it.

Identity is not content. *Which* definition this is and *what it currently says* are two
questions, and collapsing them would make every edit create a new definition, which is the
opposite of what durable identity is for.

#### The source root

A compilation has a **source root**, and every module's durable identity is its path
**relative to that root**, normalised: `/` between components, `.` removed, `..` folded
away where it can be, and no leading or trailing separator. A checkout moved from one
directory to another has the same root and the same relative paths, so it has the same
identities. The shell's working directory is not one of them: a relative path is resolved
against it to make the comparison possible, and what remains after the root is removed is
all that survives.

There are two ways a compilation gets one.

> **Declared.** A file named `nazm.root` marks the directory that contains it as a source
> root. The nearest one at or above the root module wins. `nazm check` and `nazm interface`
> also accept `--source-root DIR`, which names one without writing a file.
>
> **Derived.** With neither, the root is the directory containing the root module — the
> file named on the command line.

*Declared roots were added by N4, 2026-09-22. Derived ones are unchanged, and a project
with no marker behaves exactly as it did.*

**The marker must be empty.** It answers one question and owns nothing else. The moment it
is allowed to carry a field it is a manifest, and a manifest is a package system's front
door — versions, dependencies, a registry, a resolver. None of those is needed to say where
a project begins, and a marker with content is refused rather than read.

**Why the distinction matters.** Under a derived root the identity a module gets depends on
which file the compilation started from: `lib/util.nz` when it started at `main.nz` beside
`lib/`, and `util.nz` when it started at `lib/util.nz` itself. Both are correct within their
own compilation, and neither can be compared with the other. Under one declared root they
are the same key, which is what lets two compilations of one project agree about what their
modules are. Anything comparing keys across compilations compares them within one declared
root or not at all.

#### Module identity

> **Two modules are the same module when their normalised source-root-relative paths are
> equal.**

That is all of it. Not the absolute path, which differs per machine; not the canonical
path, which differs per checkout and leaks the layout above the root; not a content hash,
which would make every edit a different module.

Two situations have **no** durable module identity, and are refused one rather than given a
guess:

| Situation | Why |
| --- | --- |
| The module sits **outside** the source root — its relative path still begins with `..` after folding | Its identity would depend on the directory the root happens to sit in, which is the thing a durable identity must not depend on |
| Two different import paths reach **one file** through a symbolic link | The implementations disagree about how many modules that is (see below), and an identity the two cannot both compute is not an identity |

Such a compilation is correct and complete — session-local identity is untouched, the
program checks, runs and compiles exactly as before. What it does not get is a durable key,
and anything built on durable keys declines rather than guesses.

#### Symbolic links, and the one place the two implementations differ

The reference loader reads each file once keyed by its **canonical** path, so two spellings
that reach one file through a symbolic link are one module. The compiler written in Nazm
keys by a **textually normalised** path — its runtime has no `realpath` — so the same two
spellings are two modules to it. That difference predates durable identity and is recorded
in `bootstrap.md`.

Durable identity does not resolve it in favour of either. It **declines** where they could
disagree: a module reached under two different normalised paths has no `ModuleKey`, in
either implementation. Where no alias exists the two agree by construction, because textual
normalisation and canonicalisation give the same answer for a path with no symbolic link in
it.

Making them agree in the aliased case would require the Nazm-written compiler to obtain
filesystem identity — a `realpath` built-in and a runtime symbol it does not have. That is
a language and runtime change, deliberately not made for a case no real program has, and
`roadmap.md` records it as a known consequence rather than a plan.

#### Definition identity

> **A definition is identified by its module, its kind, and the name it is declared
> under.**

Top-level functions do not overload, so a name identifies at most one definition in a
module, and `N0203` is what keeps that true. What follows is therefore decidable:

| The edit | Same definition? |
| --- | --- |
| The body changes | **Yes.** A function whose body changed is that function, changed |
| The signature changes | **Yes.** Its contract changed; the definition did not become another one |
| `pub` is added or removed | **Yes.** What may name it changed; which definition it is did not |
| It moves within its module | **Yes.** Identity is a position in a namespace, not a position in a file |
| It is renamed | **No.** A different name is a different definition, and the old one was removed |
| It is deleted, and later a definition of that name is added | **The same identity.** This models continuity of a *name in a module*, not the history of an author's intent |

The last row is the honest limitation and is stated rather than hidden. Identity here
answers "which definition does this name in this module refer to", which is what a cache
key and a dependency edge need. It does not answer "is this the same definition the author
was thinking of", which is lineage — a different question, for a tool that tracks edits
rather than for the language.

**Built-ins are not definitions under this model.** They belong to no module, so they have
no module to be identified by; they keep the separate, stable identity they already have.
Nothing about a module's exported interface mentions them, because a call to `str_len`
inside a body is not part of what that module offers.

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

fn feeder(jobs: Chan) -> Int {
    let mut i = 0;
    while i < 10 {
        chan_send(jobs, i);
        i = i + 1;
    }
    chan_close(jobs);
    0
}

fn main() -> Int {
    let jobs = chan_new(4);
    let results = chan_new(4);
    let buf = ints_new();
    let mut total = 0;
    scope {
        spawn feeder(jobs);
        spawn worker(jobs, results);
        spawn worker(jobs, results);
        let mut got = 0;
        while got < 10 {
            chan_recv(results, buf);
            total = total + ints_pop(buf);
            got = got + 1;
        }
    }
    chan_close(results);
    total
}
```

This is `examples/worker-pool.nz`, and a test runs it under both implementations and
checks it prints 90 — the sum of 0..9 doubled.

Its shape is the point. With a **bounded** channel, production and consumption have to
progress at the same time. An earlier version of this example had the parent send all ten
jobs and only then receive, which deadlocks: the workers fill the four-slot results
channel, block in `chan_send`, stop taking jobs, the jobs channel fills behind them, and
the parent blocks sending — with nothing left running to drain anything. Widening the
buffers only moves the job number at which it happens. Feeding is therefore a task of its
own, and the parent drains while the workers run.

#### Scopes and tasks

| | |
| --- | --- |
| **Creation** | `spawn f(args);` starts a task running `f`. It is a statement, and `f` must be a named function — there are no function values, so there is nothing else it could be |
| **Where** | Only inside a `scope { … }` block. `spawn` outside one is `N0320` |
| **Joining** | A `scope` block does not finish until **every** task spawned inside it has finished. There is no way to leave a task running past its scope, and no way to detach one. That is the whole point of the word |
| **Nesting** | A `scope` may contain another. The inner one joins first, because it closes first |
| **The scope's value** | None. `scope { … }` is a statement |
| **Result propagation** | A task's return value is **discarded**. Results travel through channels, which is the one mechanism, rather than two |
| **Error propagation** | A task that fails at run time — overflow, a bad index, a failed allocation — ends the **whole program** with its diagnostic and exit status 2. Tasks do not have private failure modes. It ends **at the scope's closing brace, not the instant the task failed**: the scope joins every task first, so the diagnostic is reported once nothing is still running. Both implementations do this. What follows says what it costs, and how the compiled one manages it |

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
hangs rather than reporting. This is the price of the paragraph above, and it is the same
mechanism seen from the other side: the diagnostic exists, but every failure leaves through
the join, and a join that never returns never delivers it. Closing the channels a task
waits on is the program's responsibility, and the `while chan_recv(c, buf)` loop is the
idiom that makes it work.

##### How the compiled program does this

A compiled failure does not end the process where it happens. `@nz.fail` records the
diagnostic in **thread-local** state and returns; each generated function gains one extra
exit that a failure leaves by, and every call site checks the flag afterwards, so a
failure walks back out through the calls that led to it. On the way it passes each open
`scope`, joining it — innermost first, which is the nesting. Only the entry wrapper
writes a diagnostic and exits, by which time nothing is still running.

A task that fails leaves its message in its own argument block rather than reporting it.
The scope's join reads that block after `pthread_join` has returned for it, which is a
happens-before edge, so no lock is involved; the earliest failure by spawn position is
the one taken, so which task is reported does not depend on which thread finished first.
Only the first failure a thread records is kept, which is what makes the body's failure
win: a task's is adopted at the join, and the join happens after the body's was recorded.

Verified at `-O0` and `-O2` against the interpreter, for a failing body, a failing task,
a failure two calls below the body, and nested scopes — and for the case where a task
fails *strictly before* the body does, where the body's diagnostic is still the one
reported.

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

A task's arguments are values or channels, so nothing it holds can dangle. Joining is
guaranteed by the scope's closing brace, and there are no destructors to run — nothing a
program can write runs at reclamation.

> **Superseded in part by N8, 2026-09-22.** This section used to continue: *"a channel
> outlives every scope that uses it because nothing is ever freed … cleanup is therefore
> about joining, not about freeing"*. Freeing happens now. A channel's storage — its queue,
> its mutex and its condition variable — goes when the last reference to it dies, under
> rule 4 like everything else, and **closing a channel is still not destroying one**:
> `chan_close` changes what the channel does and touches no storage at all. Nothing a
> program can observe changed. What makes the two safe to keep apart is that a thread
> blocked in `chan_send` or `chan_recv` reached it from a frame that holds the handle, so a
> waiter implies a live reference and no reference implies no waiter.

#### What this model does not promise

**The runtime is one operating-system thread per task, and that is all it is.** That is
true of both implementations: `nazm run` uses a thread per task and a mutex and condition
variable per channel, and `nazm build` emits `pthread_create`, `pthread_join` and the
same mutex-and-condition-variable ring. The two are written the same way on purpose, so
that they agree by construction rather than by testing — though they are tested against
each other as well, at both optimisation levels.

Every one of the following is *not* claimed, because none of it is implemented:

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

Valueless `return`, labelled break, `for`, `loop`, mutable parameters, effects, generics,
floating point. A program using any of them is rejected by name rather than misparsed into
something else.

**Corrected 2026-09-23.** This list also named **user-defined types**, which N9 added:
`struct` declares a record, and the section *Records* above is the law. Variants, pattern
matching, generics, traits and methods remain absent and remain refused by name.

**Corrected 2026-09-21.** This list also named **I/O, modules and string concatenation**,
and the paragraph below it said there was deliberately no `print` — while all three are
specified two to four hundred lines earlier in this same file. `print` and `eprint` exist;
see *What the outside-world built-ins do*, *A file is a module*, and `str_concat`. The
reasoning that was true, and still is, is that output is an effect **the language does not
track**; what shipped is ambient authority, labelled as a limitation rather than designed.
The original paragraph, unedited:

> `nazm run` prints the value of `main`. There is deliberately no `print`: output is an
> effect, effects are the design's central mechanism, and a builtin that bypassed the
> effect system before it exists would be the wrong shape to unpick later.

## Open — later phases must settle these

### Source identity and definition identity

*Added 2026-09-21. These are the questions the next milestone — `roadmap.md` N1 — has to
answer, and they are semantic rather than merely structural, which is why they are here.*

- ~~**What is a compilation unit?**~~ **Settled by N2, 2026-09-22: one module, and today
  one module is one file.** A module owns its definitions, exports the ones marked `pub`,
  and is checked against the interfaces of the modules it directly imports rather than
  against their bodies. What remains open is not what a unit *is* but what else may become
  one: a package presenting several files as a single unit, and a generated or virtual
  source with no file behind it. Both are additions to how a module comes into existence,
  not revisions of what one means.
- ~~**What does a span identify?**~~ **Settled by N1, 2026-09-21: a span identifies a file
  and a byte range within it.** `Span` is `{ file, start, end }`, offsets are file-local,
  and the file is carried from the lexer rather than recovered from a merged offset. The
  published diagnostic shape did not change — `nazm.diagnostic/1` still reports offsets
  into the concatenation of all files, because `diagnostics.md` promises the shape will not
  move inside a version. That a program's *meaning* is unaffected is why this was an
  implementation change and not a language one.
- ~~**What is the stable identity of a definition, and what changes it?**~~ **Settled by
  N3, 2026-09-22.** A definition is identified by the module that owns it, the kind of
  definition it is, and the name it is declared under; a module is identified by where its
  source sits relative to the compilation's source root. Both survive the process. See
  **Durable identity** below.
- **Do built-ins share that identity model?** They live in the ordinary function namespace
  and may not be redefined, but they are declared in a compiler table rather than in
  source. Whether they are definitions with identities, or something else the resolver
  knows about, is **still undecided**. What N1 settled is narrower and separate: there is
  now **one** built-in inventory rather than two, and resolution answers "built-in or user
  function" once for every call site. Neither makes a built-in a definition.

*Updated 2026-09-22.* This paragraph used to end "the first answer that *does* change a
meaning — visibility, most likely — has to be given deliberately rather than discovered."
It was given deliberately: see **Visibility: private by default, `pub` exports** above.
What is left in this section changes no current program's meaning.

### Memory and values

*Copy semantics, aliasing, parameter and return conventions, reclamation, cross-task
transfer and cycles were settled by N7 — see **The memory constitution** above. What
remains open is what the four kinds do not yet cover.*

- **`Str` storage.** A `Str` may point into a literal, into `argv`, into a heap buffer, or
  into the interior of any of them, and the value does not record which. Reclaiming one
  needs a representation that does. Which representation, and what it costs on a type that
  is copied in every hot loop, is unsettled.
- **Channel storage.** A channel outlives the tasks that use it and is bounded only by its
  `scope`. Whether that bound is enough to reclaim it, or whether the runtime needs its own
  count, is unsettled.
- **User-defined and recursive types.** The constitution says what admitting a recursive
  type requires. It does not say which of those routes the language takes.
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
- ~~A machine-readable grammar~~ — **delivered.** `docs/grammar.ebnf`, checked against the
  real lexer and parser by `crates/nazm-syntax/tests/grammar.rs`, with `docs/guide.md`
  generated from it. Struck through rather than deleted, because the reason it was wanted
  — a spec artefact regardless of whether grammar-constrained decoding is ever used — is
  why it is maintained.
- A cheatsheet under ~3k tokens, to be injected by `nazm init`.

## Non-goals for the current interpreter milestone

Deliberately out of scope:

macros · comptime beyond constant folding · general effect handlers · whole-program taint ·
layout selection · a package manager · a module system beyond files

**Self-hosting was on this list and has been moved off it.** It is now a stated long-term
objective — recorded in the 2026-09-20 decision record — and the staged path was
[`roadmap.md`](roadmap.md) M3.

**Achieved 2026-09-21.** The sentence that closed this section — *"the language has no
collections, no user-defined types, no pattern matching, no string concatenation and no
I/O, so a compiler cannot be written in it yet at all"* — was true when written and is now
false in four of five clauses. Collections, string concatenation, modules and I/O exist,
and the compiler written in Nazm reaches a C2 ≡ C3 fixpoint. User-defined types and pattern
matching are still absent; the compiler was written around them with parallel `Ints`/`Strs`
arrays rather than waiting. The heading above — *"the current interpreter milestone"* — no
longer describes the project either. What this list still correctly records is what that
milestone chose not to do.
