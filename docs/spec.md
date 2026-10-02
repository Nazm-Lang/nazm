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
<!-- id: settled -->

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
<!-- id: v0-1 -->

These are the decisions the first interpreter needs and no more. Each is chosen for the
same reason: **a reader should be able to predict what a line does from the line and the
compiler's response to it.** Where two designs are defensible, the one that removes a
question wins over the one that adds an option.

### Integer overflow — traps, identically in every build
<!-- id: overflow -->

`Int` is 64-bit signed. Overflow is an error that stops the program, in development and in
release alike.

Wrapping silently is the classic source of a bug nobody reads: the arithmetic looks
correct and the number is wrong much later. Differing between debug and release is worse
— it makes a class of bug reproducible only where it cannot be debugged. Trapping costs a
check per operation, which matters for a language claiming performance, and the escape
hatch when it does is an explicit wrapping operator, not a build flag.

### Division by zero — an error, not a value and not undefined
<!-- id: division -->

`/` and `%` by zero stop the program with a diagnostic. Not a NaN-like sentinel, which
propagates silently; not undefined behaviour, which is unrepresentable in a language with
no `unsafe` yet.

### Shadowing — allowed in a nested scope, refused in the same one
<!-- id: shadowing -->

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
<!-- id: evaluation-order -->

Specified, not "unspecified for optimisation". Unspecified order is a licence for two
correct compilers to disagree, and the resulting bug appears only after a toolchain
upgrade. v0.1 has no side effects for the order to be observable through, which is
precisely why it costs nothing to fix it now and would be a breaking change later.

### Function returns — the final expression, and nothing else
<!-- id: returns -->

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
<!-- id: types -->

Parameter and return types are always written. Local `let` bindings infer from their
initialiser. Both branches of an `if` must have the same type, and a condition must be
`Bool` — there is no truthiness.


### Records — the first user-defined type
<!-- id: records -->

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

**Equality is derived, since N13.** `a == b` on two records of one type compares their
fields, and is refused (`N0304`) when any field has no equality — a sequence field would
force the language to decide whether equality means the same storage or the same contents,
and it has not chosen. *Equality is derived* below owns the rule.

**`pub` still promises nothing about a binary.** A record's layout, its field offsets, its
size and its calling convention are compiler-private and may change between builds. `pub`
says which *Nazm modules* may name the type. There is no `repr`, no FFI layout, no
serialisation format and no storage layout; each of those needs an explicit contract and
none is made here.

### Enums — the second user-defined type, and the first sum
<!-- id: enums -->

Settled by N10, 2026-09-23. An **enum** is a nominal sum type: a closed set of named
variants, exactly one of which is active in any value, each carrying its own named payload
fields.

```nazm
pub enum Token {
    Eof,
    Number(value: Int),
    Name(text: Str),
    Pair(left: Int, right: Int),
}
```

`enum` and `match` were both reserved by name from the beginning —
`crates/nazm-syntax/src/parser.rs` refused them with *"there are no variant types yet"* and
*"there is no pattern matching yet"* — so the keywords were chosen before this milestone
and N10 only stopped refusing them. There is no `variant`, `union`, `case` or `switch`: one
concept gets one spelling, the rule `struct`/record already follows.

#### Exactly one variant is active, and that is a runtime fact

A record owns every field it declares. An enum value owns **the fields of its active
variant and nothing else**. That difference is the whole of what N10 adds to the memory
constitution, and it is a runtime fact rather than a static one: which fields exist is
determined by a discriminant the value carries.

An enum has a nominal identity, an active variant identity, and the payload values of that
variant. It has no object identity of its own — no address-of, no identity comparison, no
way to observe that two bindings came from one value; the `==` N13 derives compares values,
not identities. Two separately declared enums are different types
however identical their variants, exactly as two records are.

#### Declaration

A zero-payload variant is written with no parentheses; a variant with payload fields names
each one and its type, exactly as a record field is written.

```nazm
enum State {
    Ready,
    Failed(code: Int, why: Str),
}
```

The declaration has no parentheses for `Ready` because there is no field list to write, and
a record with no fields is refused for the same reason. Construction and patterns always
write them — see below — because there the parentheses are what separates a variant from a
field projection.

**An enum with no variants is refused** (`N0339`). A type with no values raises
`Never`/uninhabited semantics — what a function returning one means, whether a `match` on
one is vacuously exhaustive — that this language has not settled, and inventing an answer
as a side effect of allowing `enum Never {}` would settle it by accident.

**An enum with one variant is allowed.** It is a nominal sum type with one family of
inhabitants, and it is not silently a record: `match` still names the variant, and adding a
second variant later is an ordinary edit rather than a change of kind.

#### Variants are named by their enum

```nazm
Token.Eof()
Token.Number(value: 42)
```

A variant is never injected into the ordinary value namespace. Three consequences, all of
them the point:

- the same variant name may exist in many enums, so `enum A { Ready, }` and
  `enum B { Ready, }` coexist and `A.Ready()` and `B.Ready()` are different values of
  different types;
- importing a module brings in no constructor names;
- a reader and the parser both learn the nominal owner from the same two tokens.

**The parentheses are not optional, including for a zero-payload variant.** `Token.Eof`
alone is exactly the shape of a field projection out of a binding called `Token`, and the
language would then need to know which it was before it knew what `Token` referred to.
`IDENT . IDENT (` is a decision the parser can make from three tokens, and it is a decision
rather than a guess: there are no methods and no function values, so nothing else in the
language can put a `(` after a projection.

#### Construction names every payload field

```nazm
Token.Pair(
    left: 1,
    right: 2,
)
```

Every payload field exactly once, by name, with no unknown field, no duplicate, and a value
of the declared type. There is no positional form, no default value and no update syntax —
the same law records follow, for the same reason: declaration position is not an API.

Initialisers are evaluated **exactly once, in the order they are written**, whatever order
the payload is physically stored in:

```nazm
Token.Pair(
    right: f(),
    left: g(),
)
```

evaluates `f()` and then `g()`. Evaluation order, semantic field identity and physical
layout are three different things and this specification keeps them apart.

**If an initialiser fails, no enum value ever becomes live.** The values already produced
are released, the failing one never becomes a field, and the initialisers after it are never
evaluated. The discriminant is written only once every payload field has been established,
so cleanup after a partial construction never dispatches on a variant the value does not
yet have.

#### An enum is a value, and its active payload keeps its own semantics

```nazm
enum Data {
    Text(value: Str),
    Numbers(value: Ints),
    Channel(value: Chan),
}

let a = Data.Numbers(value: ints_new());
let b = a;
```

After `let b = a` both values have the same active variant and the same payload values, and
each payload value obeys **its own type's existing law** — unchanged, and unchanged by being
inside an enum:

| Active variant | After `let b = a` |
| --- | --- |
| `Text` | two `Str` values with the same bytes; sharing unobservable |
| `Numbers` | **two references to one sequence**; a push through either is visible through both |
| `Channel` | two references to one channel |

**The inactive variants own nothing.** `Text` does not hold a `Str` while `Numbers` is
active; there is no inactive payload, semantically, and no copy or release operation is
performed for one. That is not an optimisation, it is what a sum type means.

So the memory constitution still needs no new kind of value. Like a record, an enum is a
composition of existing kinds — the difference is only *which* composition applies:

| | Record | Enum |
| --- | --- | --- |
| **copy** | copy or share every field, by that field's rule | copy the discriminant, then copy or share **the active variant's** fields |
| **destroy** | release every field | release **the active variant's** fields |
| **needs cleanup** | some field does, recursively | some field of **some** variant does, recursively |
| **may cross into a task** | every field may, recursively | every field of **every** variant may, recursively |

The last row is deliberately not symmetric with the first two. Copy and destroy are runtime
operations and ask about the value; task safety is a **type-level** guarantee and asks about
the type. `enum Work { None, Values(xs: Ints), }` may not cross into a task even while it
holds `None`, because what a `spawn` is given is a value of a type, and no flow-sensitive
escape is offered.

#### Variant and payload-field order are not part of what an enum means

Moving two variant declarations, or two payload-field declarations, changes nothing: not
what the program means, not the interface fingerprint, and not the native representation.
The persisted interface sorts variants and fields by name and the backend assigns
discriminants and lays payloads out in the same canonical order, so the claim is made true
rather than asserted.

Initialiser evaluation order is a separate question and is the order written, as above.

#### `match` is an expression

```nazm
let n = match token {
    Token.Number(value: n) => n,
    Token.Name(text: _) => 0,
    Token.Pair(left: a, right: _) => a,
    Token.Eof() => -1,
};
```

It composes wherever an expression may appear, including as a statement, as a `return`
operand and as an initialiser, and it reuses the existing block and completion rules rather
than introducing a second set. An arm body is an expression, and a block is an expression,
so `=> { let n = str_len(s); n },` is the ordinary block, not a statement grammar of its own.

Every arm of a value-producing `match` produces the same type. There is no union inference
and no implicit conversion. An arm that transfers control — `return`, `break`, `continue` —
produces no value and constrains nothing, exactly as a branch of an `if` does.

**The scrutinee is evaluated exactly once**, before any arm is selected, and the `match`
holds that value for as long as the selected arm runs. The scrutinee is a value the `match`
owns, not the caller's storage: `match make_token() { … }` has a clear owner for the
temporary, and `match t { … }` leaves `t` untouched and independent. A `match` does not
consume the binding it reads.

The discriminant selects exactly one arm. There is no fallthrough, no backtracking and no
guard, so an expression in an unselected arm never runs.

#### Patterns name every payload field

```nazm
Token.Pair(
    left: x,
    right: _,
)
```

A variant pattern names **every payload field of that variant, exactly once**, either
binding it to a name or discarding it with `_`. There is no `..` rest syntax, and that is an
evolution property rather than pedantry: adding a payload field makes existing patterns
fail to check instead of silently ignoring new state.

`_` means *introduce no binding*. It does not mean *forget this field*: the value is still
the enum's, and it is still released when the `match`'s scrutinee dies. Two payload fields
may both be `_`; two bindings may not share a name.

Bindings are **borrowed, immutable views of the payload held by the match's scrutinee**,
live only inside the arm that introduced them. Matching does not consume the payload. A
binding that names an owned mutable handle borrows the handle, so `E.Values(items: xs) =>
ints_push(xs, 1)` mutates the referenced sequence — *immutable binding* is a statement about
the name, never about the storage it refers to.

If an arm's value is a payload — `Token.Name(text: s) => s` — the result takes its own
reference to whatever backs it **before** the arm's bindings end and before the scrutinee is
released. That is the same rule *A projection produces a value* already states for records,
applied to a value whose fields are a runtime fact.

A pattern's variant must belong to the scrutinee's enum. `B.Ready(…)` against an `A` is
refused (`N0345`) even if both enums have a `Ready`; there is no structural variant
matching.

#### Exhaustiveness

Every `match` must name **every variant of its enum, exactly once**.

- a missing variant is `N0343`, and the diagnostic lists the variants that are missing;
- a repeated variant is `N0344`, reported at the second arm;
- there is **no `_ => …` catch-all arm** in N10.

No runtime trap substitutes for the check. A valid program's discriminant always has a
statically known arm, so an unreachable default exists in the generated code as a structural
requirement of the instruction set and never as language semantics.

The absent wildcard is a decision about evolution, not an oversight. While the language is
young, adding a variant *should* break every importer that believed it had handled the whole
enum. A `non_exhaustive`-style opt-out, and the wildcard that goes with it, is a later
design that must be made deliberately.

#### `match` is the only elimination form

There is no `is`, no `instanceof`, no `tag()` and no `variant_name()`. The discriminant is
representation, not a value a program can read.

There is also **no payload projection outside a pattern**: `value.payload` is not written,
because which fields exist depends on the active variant. To change an enum value, assign a
whole new one.

#### Assignment and replacement

```nazm
let mut x = E.Text(value: "a");
x = E.Values(items: ints_new());
```

The new value is fully evaluated and owned first, then the old value's **active** payload is
released, then the discriminant and payload are replaced. Self-assignment, and a new value
backed by the same allocation as the old one, are safe consequences of that order rather
than special cases.

#### Records and enums compose, in both directions

A record field may be an enum, an enum payload field may be a record, and an enum payload
field may be another enum. Copy, release, task safety and layout all recurse through both
kinds. Nothing is derived from which kind happens to be on the outside.

#### Recursion is refused as an impossible layout

```nazm
enum List { End, Next(rest: List), }     // N0336
struct A { b: B, }                       // N0336
enum B { Value(a: A), }
```

The rule N9 stated over records is stated over **every inline user-defined value type**: a
chain of by-value containment that returns to where it started describes a value that
contains itself, and no finite layout exists. Records and enums form one containment graph
and the cycle is refused wherever it runs.

It is not secretly boxed, and it is not forbidden forever: the refusal is of an **inline
infinite layout**, so a later type that holds its contents somewhere else — a box, an arena
handle, a generic container — makes the same shape expressible without this rule changing.

Enums and records nested inside one another are bounded at 32 levels (`N0338`), for the
reason records already were: each level multiplies the copy and cleanup code generated for
the outer value.

#### Visibility, and the API being closed

An enum is private to its module unless it is written `pub`, exactly as a record is. For a
public enum, **every variant and every payload shape is part of the public type** — there is
no per-variant visibility and no private variant inside a public enum, because either would
be an opaque or open enum and neither has been designed.

A public type may not expose a private one (`N0337`), and the check runs recursively through
records and enums together: a public enum whose payload is a public record whose field is a
private enum is refused, because an importer needs that type to understand the shape it was
given.

#### Adding a variant is a breaking change, deliberately

For an exported enum, adding, removing or renaming a variant — or adding, removing or
retyping a payload field — changes the module's interface fingerprint, which re-checks every
importer, which rejects every `match` that is no longer exhaustive. A rename is a removal and
an addition; no lineage is invented.

That is the intended cost of a closed enum with no wildcard arm, and it is why the
discriminant is not a stable identity: the *meaning* of a variant is its enum and its name,
and the number the backend gives it is chosen fresh for each build.

#### What enums are not, in N10

No generics, so no `Option`, no `Result` and no generic enum of any kind — a monomorphic
`enum MaybeInt { None, Some(value: Int), }` is an ordinary user declaration and the language
offers no built-in like it. No typed errors, no `throw`, no `?`, no `try`: an enum can model
a result by hand, and that is not an error-handling feature. *(N12 added `Result`, `Option`
and `?` — as ordinary generic enums and a `match` — in* Typed error values *below; there is
still no `throw` and no `try`.)*

No literal, record, tuple, range or or-patterns; no guards; no top-level wildcard; no `let`
destructuring; no by-move or mutable patterns; no match ergonomics inferred from ownership.
Pattern matching in N10 selects a variant and binds its named payload fields, and nothing
else.

**Equality is derived, since N13.** `a == b` on two enum values of one type compares their
variants and then the active variant's payload, and is refused (`N0304`) when a payload field
of **any** variant has no equality. *Equality is derived* below owns the rule.

**`pub` still promises nothing about a binary.** An enum's discriminant values, its payload
layout, its size and its calling convention are compiler-private and may change between
builds. There is no `repr`, no C ABI, no FFI layout, no serialisation format and no storage
layout.


### Generics — first-order parametric types, and `Vec[T]`
<!-- id: generics -->

Settled by N11, 2026-09-23. A record, an enum and a function may declare **type parameters**,
and a type may be **applied** to type arguments. There is one kind — *type* — and nothing
else: no constraints, no traits, no methods, no higher-kinded parameter, no type-level
computation, no defaults, no variance, no specialisation. What N11 adds is first-order
parametric polymorphism and one generic container, `Vec[T]`, and every law below is written
so that the ones before it — records, enums, the memory constitution — survive substitution
unchanged.

```nazm
struct Pair[A, B] {
    first: A,
    second: B,
}

enum Maybe[T] {
    None,
    Some(value: T),
}

fn identity[T](value: T) -> T {
    value
}

fn main() -> Int {
    let xs = vec_new[Pair[Int, Str]]();
    vec_push(xs, Pair[Int, Str](first: 7, second: "nazm"));
    let p = vec_get(xs, 0);
    p.first
}
```

#### One syntax for each thing

| | |
| --- | --- |
| **Parameter list** | `[A, B]` after the declared name: `struct Pair[A, B]`, `enum Maybe[T]`, `fn identity[T](…)`. At least one name, no trailing comma |
| **Type application** | `Pair[Int, Str]`, `Maybe[Point]`, `Vec[Vec[Int]]`, anywhere a type is written |
| **Explicit function arguments** | `identity[Int](7)`, `vec_new[Token]()` |
| **Generic construction** | `Pair[Int, Str](first: 1, second: "x")` — the type arguments are written |
| **Generic variant** | `Maybe[Int].Some(value: 42)`, `Maybe[Int].None()` — the type arguments are written |
| **Pattern** | `Maybe.Some(value: x)` — the enum's **name**, never its arguments; the scrutinee's type supplies them |

Square brackets, and no angle brackets as a second spelling. `[` and `]` are new tokens in
N11 and mean this and nothing else, and there is no indexing expression for an application
to be confused with: `name[` in an expression can only begin type arguments — where `<`
would have been a comparison until the parser guessed otherwise. A pattern takes no
type arguments because it cannot choose them — the value being matched already has one
concrete type, and writing it again in every arm would be a second place to get it wrong.

#### A type parameter is a type and nothing else

Inside the declaration that owns it, a parameter may be written wherever a type is. It is
not a value, not an integer, not runtime metadata and not something a program can inspect.
Its names are unique within one list (`N0350`), may not be a built-in type name or `Vec`
(`N0331`) — and neither may a declared record or enum, since a type written `Vec` is always
the built-in — and are resolved lexically: a parameter belongs to its own declaration only, so
one function's `T` is never another's, and a parameter may shadow a user-defined type of the
same name inside the declaration that introduces it.

#### Order is semantic; names are not

`Pair[A, B]` and `Pair[B, A]` with the same field declarations are different definitions:
the first argument of an application fills the first position. **Renaming** a parameter —
`[A, B]` to `[X, Y]` with every use renamed — changes nothing: not what the definition
means, not its interface fingerprint, not any object. A parameter's identity is its
**position** in its owner's list, and that is what a persisted interface writes.

#### Identity

A generic definition is one definition — its `DefKey` and kind, exactly as before — and its
**arity** is part of its interface. An **applied type** is identified by the definition and
its ordered type arguments, recursively: `Pair[Int, Str]` is the same type in every module
of a compilation that resolves the same `Pair`, `Pair[Str, Int]` is a different one, and
`a.nz`'s `Box[Int]` is not `b.nz`'s `Box[Int]`, because the definitions differ. Nothing about
the shape is compared: two generic records with identical fields are still two types after
substitution, for the reason two ordinary records are.

#### Generic definitions are checked once, parametrically

A generic function is checked **once, as a generic definition**, with each parameter an
unknown type. It is never re-checked for a particular use, and whether it is valid does not
depend on how it is called. An unconstrained `T` supports exactly what every value in the
language supports: it may be bound, copied by its own law, passed, returned, stored in a
field or a payload of a generic type, pushed into a `Vec[T]` and taken out of one. It may
**not** be added, ordered, compared with `==` (`N0304`), projected (`N0335`), matched
(`N0342`), used as a condition (`N0301`) or passed to a task (`N0321`), because none of those
is valid for every type. `fn equal[T](a: T, b: T) -> Bool { a == b }` is refused at its
definition. Since N14 one constraint exists: a parameter declared `T: Equality` may be
compared with `==` — *A type parameter may require equality* owns it. Nothing else can be
required, so the rest of this list is unchanged.

#### Calling a generic function

The type arguments are either **written** or **inferred from the arguments**, and nothing
else:

- Inference unifies each parameter's declared type with the argument's type, recursing
  through applications and `Vec`: passing a `Vec[Point]` for `Vec[T]` gives `T = Point`.
- **The expected result type never participates.** `fn make[T]() -> T` cannot be called as
  `make()`; it must be `make[Int]()` (`N0354`). Local, left-to-right, from the arguments —
  so a call's meaning is visible in the call.
- Two arguments that give one parameter different types are **refused** (`N0355`) —
  `same(1, "x")` for `fn same[T](a: T, b: T)` does not choose the first.
- Explicit arguments fix every parameter, and an argument that then disagrees with the
  signature is an ordinary mismatch (`N0300`).
- The wrong number of type arguments is `N0353` wherever it is written — `Pair[Int]`,
  `Vec[Int, Str]`, `identity[Int, Str](…)`; type arguments on a definition that has no
  parameters are `N0351`; a generic type written without its arguments where a concrete type
  is needed — a field, a parameter, a construction — is `N0352`.
- An argument that never produces a value (every path `return`s, `break`s or `continue`s)
  contributes nothing to inference.

Inference is not overload resolution. A name still resolves to exactly one definition, and
generics add no overloading: `fn f[T]` and `fn f` in one module are two definitions of one
name, which is `N0203` as it always was. A generic `main` is refused (`N0356`): nothing
could choose its arguments. A `spawn` runs a non-generic function (`N0101`).

#### Generic records and enums

Every law in *Records* and *Enums* holds **after substitution**. `Box[Int]` and `Box[Str]`
are two concrete types of one definition; each is nominal, a value, constructed by naming
every field, copied and destroyed field by field according to the field types it has *after*
substitution. A generic enum's variants are its definition's, for every instantiation:
`Maybe[Int]` and `Maybe[Str]` have the same variant set, exhaustiveness is decided over it,
and only the active variant's payload owns anything.

Visibility is unchanged, and the closure rule recurses through applications: a public
signature or shape may not mention a private type **anywhere in a type argument** either
(`N0337`).

#### `Vec[T]` — a mutable handle, by the sequence law

`Vec[T]` is the generic counterpart of `Ints` and `Strs` and follows **their** law, not a
value law: it is an *owned mutable handle* in the memory constitution's table.

| | |
| --- | --- |
| **Creation** | `vec_new[T]()` — explicit, because nothing else could say what `T` is |
| **Identity** | a handle. `let b = a;` makes `b` the same vector, and a push through either is visible through both. Nothing is copied |
| **Length** | `vec_len(xs)` |
| **Reading** | `vec_get(xs, i)` returns **its own copy** of the element by `T`'s copy law, `N0405` unless `0 <= i < len` |
| **Replacing** | `vec_set(xs, i, v)` stores a copy of `v` and returns the index written. The new value is taken before the old one is released |
| **Appending** | `vec_push(xs, v)` stores a copy of `v` and returns the new length. A failed growth (`N0406`) leaves the vector exactly as it was |
| **Removing** | `vec_pop(xs)` **transfers** the last element out; `N0405` on an empty vector |
| **Equality** | refused (`N0304`), for the reason it is refused on `Ints` |
| **Tasks** | a `Vec[T]` **never** crosses into a task, whatever `T` is (`N0321`). It is shared mutable storage with an unsynchronised count, and a record or enum containing one inherits that |
| **Destruction** | when the last reference dies, every element is released by `T`'s law, then the storage |

There is **no indexing syntax**. `xs[i].field = v` would need a place calculus — what is
evaluated once, what is copied, what happens on failure part way through — and nothing in
N11 needs one. Updating a record element is written as what it is: get a copy, change it,
set it back.

`Ints` and `Strs` are unchanged and are not aliases of `Vec[Int]` and `Vec[Str]`.

#### What may not be expressed: an ownership cycle

A `Vec` holds its elements **somewhere else**, so a type can now contain a handle to storage
holding values of that same type — `struct Node { children: Vec[Node], }`. Its layout is
finite, and a program could still build a `Node` whose `children` contain itself, which no
reference count ever reclaims. The memory constitution said in advance what admitting such a
type requires; **N11 takes the first option again, and refuses it.**

The rule is over the **ownership graph of definitions**: an edge runs from a record or enum
to every definition its fields or payloads mention, labelled *inline* or *through a
reference-counted container* by the path that reaches it — a field is inline, a `Vec`
element is not, and an argument of a generic type inherits how that type uses the
corresponding parameter. Then:

- a cycle whose every edge is inline is an **impossible layout** — `N0336`, as before;
- any other cycle is an **ownership cycle** — `N0359`, with the path that closes it.

So `Node { children: Vec[Node] }`, `A → Vec[B] → B → Vec[A]`, `enum Tree { Leaf, Node(kids: Vec[Tree]) }`
and `Tree[T] { kids: Vec[Tree[T]] }` are refused, and `Vec[Vec[Int]]`, `Vec[Pair[Int, Str]]`,
a record holding a `Vec[Int]` and an enum holding a `Vec[Str]` are not: an acyclic ownership
graph is a graph whose values cannot own themselves. A parameter that no field uses
contributes no edge. The rule also bounds every instance family: with no cycle among the
definitions, substitution can only nest types a finite way deep, so it never runs forever.

#### How much a generic value costs

Nothing about generics is free and nothing claims to be. Copying a `Pair[Str, Chan]` is two
reference adjustments; copying a `Maybe[Record]` is a branch on the discriminant and then the
active payload's copy; assigning a `Vec[T]` is one adjustment, never a copy of its elements.
The same table as *What this costs* applies to every instantiation, with the field types it
has after substitution.

#### What a program cannot observe

Which native code implements a generic function, whether one copy of it serves two
instantiations, what size `T` has and what a specialisation is called are not part of the
language. `nazm check` and `nazm run` are parametric; `nazm build` currently
**monomorphises** — one native instance per concrete argument list — and that is a property
of the current LLVM backend, not of the language (`architecture.md` §7.12). The native
backend refuses polymorphic recursion (`N0357`) — recursion through which an instance would
need another instance without end — and bounds how many instances one program may need (`N0358`);
ordinary recursion through a generic function compiles (N49).

#### What generics are not, in N11

No traits, interfaces or protocols; no `impl` and no methods; no constraints or `where`; no
associated types or constants; no higher-kinded parameters; no const generics; no defaults;
no variance or subtyping; no specialisation or overloading; no reflection. No closures, no
function values, no `Never`, no unit type. Construction infers nothing: its type arguments
are written.

**Corrected 2026-09-24, N12.** This paragraph said there was no built-in `Option` or
`Result`. There still is none built into the checker: both are now ordinary generic enums
declared by the core prelude — the next section — and nothing about generics changed to
admit them.

### Typed error values — `Result`, `Option`, and `?`
<!-- id: errors -->

*Settled by N12, 2026-09-24.* **A typed failure is a value, and propagation is structured
control flow over that value.** There is no exception, no `throw` or `catch`, no unwinding,
no hidden error object and no conversion between error types. `Result` and `Option` are
two ordinary generic enums, and `?` is one postfix operator that means a `match` with a
`return` in one arm.

#### The core prelude

One module belongs to the toolchain rather than to a project: **the core prelude**, whose
source is `library/core/prelude.nz` and which declares exactly

```nazm
pub enum Result[T, E] {
    Ok(value: T),
    Err(error: E),
}

pub enum Option[T] {
    None,
    Some(value: T),
}
```

It is ordinary source, checked by the ordinary checker, and it is part of **every**
compilation — including one given as a string. Its rules:

- **Every module imports it**, with no `use` written. Its two types are therefore in every
  module's type namespace exactly as a direct import's exported types are, and by the same
  rule; nothing is copied into a module, and every module names the same two definitions.
- **Its identity is the toolchain's.** Its `ModuleKey` is `@core/prelude` in every project,
  from every working directory and every checkout, so `Result` is
  `@core/prelude::enum Result` everywhere. No project path produces a key beginning
  `@core`: a project file that happens to sit under a directory of that name compiles, and
  has no durable identity (*Durable identity*, below).
- **It is a dependency like any other.** A module's check key includes the prelude's key
  and interface hash, as it includes every import's. A change to the prelude's public
  shape reaches every module; one that moves no public shape reaches none. Every module
  depends on the whole prelude — there is no per-name slicing — which costs nothing while
  it declares two types.
- **Its names are reserved.** A project module may not declare a type named `Result` or
  `Option` (`N0363`, at the declaration). Otherwise ordinary lookup would find the
  project's while `?` meant the prelude's. Only the **type** namespace: a function called
  `Result` is legal, as it always was.
- **It is not a package system.** There is one such module, with two declarations; nothing
  can add a second, and there is no registry, version, manifest or namespace mechanism
  behind it.

#### `Result` and `Option` are ordinary enums

Everything *Enums* and *Generics* say applies, unchanged, and nothing is added:

- construction is qualified and carries its arguments — `Result[Int, Str].Ok(value: 1)`,
  `Option[Str].None()` — and a pattern names the definition, `Result.Ok(value: v)`;
- `match` is exhaustive over both variants;
- layout, copying and release are an enum's: one active variant, released by its own law;
- task safety is derived: `Result[Int, Str]` and `Result[Int, Chan]` may cross into a task,
  `Result[Int, Vec[Diag]]` and `Option[Vec[Int]]` may not (`N0321`);
- the ownership graph sees through them: `struct Node { next: Option[Vec[Node]] }` is an
  ownership cycle (`N0359`), and `struct Node { next: Option[Node] }` has no layout
  (`N0336`).

**There is no null.** `Option[Str].None()` is a variant, not a null pointer, and
`Option[Int].None()` is not a magic integer. **There are no methods** — no `unwrap`, `map`,
`is_ok` — and no panicking accessor: a program takes a value apart with `match`.

#### `?` — propagation

`?` is a postfix operator beside projection, in any order and any number: `r?`,
`make()?.x`, `r??`. It binds tighter than every operator, including negation, so `-r?` is
`-(r?)`. `??` is two `?` tokens and removes two layers; `(r?)?` is the same thing written
out.

**The rule.** If `expr : Result[T, E]` — an application of the core prelude's `Result`,
recognised by **definition**, never by name or by the shape of its variants — and the
enclosing function returns `Result[U, E]` with the **identical** `E`, then `expr? : T`. `U`
is free: `?` yields the operand's success value where it is written, and only the error
leaves. `?` means exactly

```nazm
match expr {
    Result.Ok(value: v) => v,
    Result.Err(error: e) => return Result[U, E].Err(error: e),
}
```

with `expr` evaluated **once**. Everything below follows from that sentence.

**What is refused**, each at the `?`:

| code | meaning |
| --- | --- |
| `N0360` | the operand is not the core `Result` — an `Option`, a project enum with `Ok` and `Err` variants, anything else |
| `N0361` | the enclosing function does not return the core `Result` — including `main`, and including a function returning a lookalike |
| `N0362` | the operand's error type is not exactly the function's: two records with identical fields are two error types, and two type parameters `E1`, `E2` are not known to be equal |

There is no conversion: to change an error type, `match` and construct the other.

**Completion.** A propagation completes as `Value(T)` when its operand does; the `Err`
path is a `return` and adds no completion state of its own. An operand that never
produces a value makes the propagation transfer too.

**Evaluation order.** The operand is evaluated once, where the left-to-right rules put it.
When it is an `Err`, nothing after it in the enclosing expression is evaluated: no later
argument and no call, no arithmetic on it, no store of an assignment — the destination
keeps its value — no later field initialiser and no completed record or variant, whose
earlier fields are released, and no inspection of a `match` whose scrutinee it is.

**Cleanup is exactly `return`'s.** Every `scope` the propagation leaves is joined,
innermost first, before the function returns — a task still running is waited for, and a
task's failure is taken on as it is for `return`. Every temporary the enclosing
expressions hold and every local of the function are released once.

**Ownership.** On `Ok`, the payload takes its own reference before the temporary `Result`
is released, so a `Str` that came out of one outlives it. On `Err`, the error value is
owned by the returned `Result` before anything it came out of is released. A `Vec` error
is a handle: propagated through three functions, it is one vector with one logical
reference at each return boundary, never a copy.

**What `?` is not.** Not an exception: no unwinding, no landing pad, no catch table, no
runtime failure and no message — a propagated `Err` is a normal return. Not an effect: a
function's error type is visible in its signature because it is inside its return type,
and a caller may store, pass or ignore the `Result` like any value. Runtime failures —
`N0405`, `N0406`, division by zero, overflow — are unchanged and never become an `Err`.
Not available on `Option`, which a program takes apart with `match`: a second carrier
would need a propagation protocol, and traits do not exist.

**`main` is unchanged.** It returns `Int`, so a program handles its errors itself with a
`match` and decides what they mean; nothing maps an `Err` to an exit status.

### The standard library — `use "@std/…"`
<!-- id: std -->

A **standard module** is a toolchain-owned Nazm module a program imports by name. There are seven,
each ordinary source checked like any module, and none is visible unless a module writes its `use`:

```nazm
use "@std/text";
use "@std/io";

fn main(io: IoCap) -> Int ! { io } {
    let words = text_split("alpha,beta,,gamma", ",");
    io_println(io, int_to_str(strs_len(words)));
    match text_parse_int(text_trim("  42 ")) {
        Result.Ok(value: n) => n,
        Result.Err(error: e) => 0,
    }
}
```

**Resolution.** An import path beginning with `@std/` names a module the toolchain provides and
is never looked for on disk: `@std/NAME` is the standard module `NAME`, and any other `@std/` path
is `N0205`. Every other path, including one beginning with another `@`, is relative to the
importing file as before. A standard module's durable identity is `@std/NAME`, which no project file can have. Its
exported functions enter the importer's namespace like any module's, so each is named after its
module — `text_split`, not `split` — and a program may not define a function of the same name in a
module that imports it (`N0208`).

**Contracts.** Every standard function declares its effect set. The pure ones are `! {}`; the ones
that reach the outside world take the `IoCap` they need and declare `! { io }`, so a caller's
authority is as visible as with the built-ins. What they read carries the built-ins' origins:
`io_read`'s result is `file`.

**Errors.** Ordinary failure is a value: `Err` or `None`, never a trap. A standard function fails
only where a built-in it calls would — `ints_sum` overflows as `+` does, and `io_read` of a file
that exists but cannot be read is `N0407`.

| Module | Functions |
| --- | --- |
| `@std/text` | `text_starts_with(s, prefix) -> Bool`, `text_ends_with(s, suffix) -> Bool`, `text_find(s, needle) -> Option[Int]`, `text_contains(s, needle) -> Bool`, `text_repeat(s, n) -> Str`, `text_split(s, sep) -> Strs`, `text_is_space(byte) -> Bool`, `text_trim(s) -> Str`, `text_parse_int(s) -> Result[Int, Str]` |
| `@std/option` | `option_is_some[T](o) -> Bool`, `option_is_none[T](o) -> Bool`, `option_unwrap_or[T](o, fallback) -> T` |
| `@std/result` | `result_is_ok[T, E](r) -> Bool`, `result_is_err[T, E](r) -> Bool`, `result_unwrap_or[T, E](r, fallback) -> T`, `result_ok[T, E](r) -> Option[T]`, `result_err[T, E](r) -> Option[E]` |
| `@std/seq` | `ints_index_of(xs, x) -> Option[Int]`, `ints_contains(xs, x) -> Bool`, `ints_sum(xs) -> Int`, `ints_max(xs) -> Option[Int]`, `ints_min(xs) -> Option[Int]`, `strs_index_of(xs, x) -> Option[Int]`, `strs_contains(xs, x) -> Bool`, `vec_index_of[T: Equality](xs, x) -> Option[Int]`, and (N50) `vec_map[T, U](xs, f: fn(T) -> U) -> Vec[U]`, `vec_filter[T](xs, keep: fn(T) -> Bool) -> Vec[T]`, `vec_fold[T, A](xs, init: A, f: fn(A, T) -> A) -> A` — each, since N51, polymorphic in the effects of the function it takes (`effects E`), and doing exactly what that function does |
| `@std/io` | `io_println(io, s) -> Int`, `io_eprintln(io, s) -> Int`, `io_read(io, path) -> Result[Str, Str]` — each `! { io }` |
| `@std/process` | `process_args(io) -> Strs` — `! { io }` |
| `@std/chan` | `chan_take(c) -> Option[Int]`: the next value, or `None` once closed and drained |

**Stability.** Pre-1.0. A standard module may change between milestones; its interface hash says
when, and no semantic epoch moves for it.

### Equality is derived — `==` on the value types that can have it
<!-- id: equality -->

Settled by N13, 2026-09-25. Built-in `==` and `!=` apply to two operands of **one** type,
and only when the language defines equality for that type. For a user-defined type it is
not declared, written or opted into: it is **derived** from what the type contains.

| type | built-in equality |
| --- | --- |
| `Int`, `Bool` | yes — the same value |
| `Str` | yes — the same bytes (*Strings are bytes*). A shared backing makes nothing equal, and separate backings make nothing unequal |
| a record | yes exactly when **every field's** type has equality |
| an enum | yes exactly when **every payload field of every variant** has equality |
| a concrete instance of a generic record or enum — `Box[Int]`, `Option[Str]`, `Result[Int, Str]` | derived as above, over its fields **after substitution** |
| `Ints`, `Strs`, `Vec[T]`, `Chan` | **no** — a handle's equality would have to mean the same storage or the same contents, and the language has chosen neither |
| an unconstrained type parameter `T` | **no** — `==` is not valid for every type, and there is no constraint that could say "for these types only" |

A type with no equality is refused where it is compared, with `N0304`, and the message names
the first component responsible: `Outer`'s `inner.values` is a `Vec[Int]`.

**One type, nominally.** Two records with identical fields are two types and cannot be
compared, nor can `Box[Int]` and `Box[Str]`, `Option[Int]` and `Option[Str]`, or `A[Int]`
and `B[Int]`. Nothing converts, implicitly or otherwise.

**What it means.** Two records are equal when every corresponding field is equal under that
field type's equality — two records built separately with equal fields are equal, because
a record has no identity. Two enum values are equal when they hold the **same variant** and
that variant's payload fields are pairwise equal; different variants are unequal, and their
payloads are not compared. A zero-payload variant is equal to itself, and an inactive
variant is never inspected. `!=` is the negation of `==`, always.

**Decided from the type, never from the value.** An enum with one variant that cannot be
compared cannot be compared at all: given `enum Work { Empty, Values(value: Vec[Int]), }`,
`Work.Empty() == Work.Empty()` is refused, because `==` is checked where the active variant
is not known and a guarantee about a type is about every one of its values. This is the
rule task safety follows, for the same reason.

**Generic bodies need a requirement.** A concrete `Box[Int]` compares, and without one
`fn same[T](a: T, b: T) -> Bool { a == b }` is still refused at its definition, as is
`fn same_box[T](a: Box[T], b: Box[T]) -> Bool { a == b }`: inside the body `Box[T]` has a
field of an unconstrained type, so it has no equality either. Nothing is inferred, deferred to
an instantiation, or accepted because some caller would pass a comparable type. *(N14: a
definition that declares `T: Equality` may compare both — see* A type parameter may require
equality *below.)*

**`Option` and `Result` have no rule of their own.** They are the core prelude's ordinary
generic enums, so `Option[Int]` and `Result[Int, Str]` compare and `Option[Vec[Int]]` does
not, by the rule above.

**Order is not meaning.** Declaration order of fields, of variants and of payload fields is
not part of what a type means, and not part of what its equality means either.

**Evaluation.** The left operand is evaluated, then the right, each once, and then they are
compared. An operand that leaves — returns, breaks, continues or propagates — leaves no
comparison behind, and what the other side already built is released on the way out.
Comparing observes: a bound operand is still usable afterwards, and a temporary operand is
released once the answer exists. Comparing is pure, so the order in which fields are
compared is not observable, and an implementation may stop at the first unequal one.

**What equality is not.** Not declared: there is no `derive`, no `Eq` and no trait. Not
customisable: a type cannot define its own `==`. Not identity: no value's address, owner or
handle is compared. Not ordering: `< <= > >=` remain `Int`-only. Not hashing. And not
defined on sequences, vectors or channels, deliberately, by the rule that stops a record
containing one from comparing.

### A type parameter may require equality
<!-- id: equality-requirement -->

Settled by N14, 2026-09-25. A generic function may state that one of its type parameters
ranges only over types that have equality:

```nazm
fn same[T: Equality](a: T, b: T) -> Bool {
    a == b
}
```

**The law.** A type parameter may carry an explicit equality requirement, written
`T: Equality`. Inside that definition the parameter has built-in equality, and so does every
composite whose equality derives from it by the rule above — `Box[T]`, `Option[T]`,
`Result[T, E]` when both `T` and `E` require it. Every type argument, written or inferred,
must satisfy the requirement by the same rule: a concrete type satisfies it exactly when it
has equality, and a caller's own type parameter exactly when *its* declaration requires it.
Requirements are checked statically, and they create no user-defined equality: `==` means
what *Equality is derived* says it means, for every type, with or without one.

| | |
| --- | --- |
| **Syntax** | `[T: Equality]` — one requirement per parameter, no list. `Equality` is the only one. It is not a type and nothing declares it: it means something only after the `:`, so no record, enum or import can shadow it, and a record named `Equality` is an ordinary record |
| **Identity** | A requirement belongs to the parameter — its owner and position — never to its name. Renaming `T` changes nothing, in the program or in its interface |
| **The body** | Checked once, against what the requirements guarantee. Nothing is inferred from what the body does: an unconstrained `T` still has no equality, and `fn same[T](a: T, b: T) -> Bool { a == b }` is still refused (`N0304`) |
| **The arguments** | Each is checked when the call is: written or inferred, `N0364` names the argument and the component responsible. `same(v, v)` for a `Vec[Int]`, `same(Work.Empty(), …)` for an enum with a `Vec` in any variant, and `same(o, o)` for an `Option[Vec[Int]]` holding `None` are all refused — a requirement is satisfied by a type, never by a value |
| **Forwarding** | A generic caller may pass its own parameter to a bounded callee only if its own declaration requires the same: `fn wrap[T: Equality](a: T, b: T) -> Bool { same(a, b) }` is accepted, the same without `: Equality` is `N0364` |
| **Nominality** | Unchanged. Two parameters that both require equality are still two types, and `a: A == b: B` is refused |
| **Where** | Only on a function's parameters. A requirement on a record's or an enum's parameter is refused (`N0101`, not supported yet) rather than parsed and ignored |
| **Interfaces** | A public function's requirements are part of what it offers. They are persisted by position (`nazm.interface/5`), an importer is held to them when checking against the interface alone, and adding or removing one moves the interface's fingerprint |
| **At run time** | Nothing. A requirement is a fact for the checker: no dictionary, no descriptor and no test is emitted, and an instance of `same` at `Int` is the same instructions as a hand-written `same_int` |

**What this is not.** Not a trait: there is no declaration of a capability, no `impl`, no
method, and nothing a program can implement or override. Not an inference: the requirement is
written where the parameter is declared, so a body edit cannot change which callers are
valid. Not a second equality: a bound proves that an operand's type is in `==`'s domain and
says nothing about what `==` does there.

## Settled for v0.2 — mutation and iteration
<!-- id: v0-2 -->

### Mutation is opt-in and visible at the binding
<!-- id: mutation -->

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
<!-- id: while -->

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
<!-- id: blocks -->

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
<!-- id: break-continue -->

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
<!-- id: return -->

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
<!-- id: unreachable-code -->

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
<!-- id: limits -->

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
<!-- id: nesting -->

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

#### Native recursion, and the stack it runs on — N49
<!-- id: native-recursion -->

*Until N49 this section was "Why native recursion is refused rather than counted", and its
argument still holds:* a call-depth counter alone does not guarantee a diagnostic before the
native stack is exhausted, because whether that many frames fit depends on frame size and on
how much stack a thread actually has. The interpreter pairs its counter with a stack it sizes
from the counter (`--max-depth`, `N0403`). **`nazm build` now pairs a measurement with the stack
it measures**, and recursion — direct and mutual, across modules and packages — compiles under
both native backends.

| | |
| --- | --- |
| **What is checked** | The stack itself, not a count. On entry to every function that is in a call cycle (a cycle of calls; `spawn` starts a task on its own stack and closes none), the frame's address is compared with the thread's limit |
| **The limit** | Computed once per thread, at its first check: that frame's address minus three quarters of the smaller of `RLIMIT_STACK` and 8 MiB. Task threads are created with 8 MiB, so no thread's budget exceeds its stack |
| **What a program sees** | `N0408`, *the native call stack is exhausted*, at the function entered, and then the failure unwinds like any other: what the frames own is released, an enclosing `scope` joins its tasks, and the process exits 2 with the diagnostic |
| **What is guaranteed** | Unbounded recursion meets a check. A chain of non-recursive calls between two checks is bounded by the program's call graph but its stack is not measured: the guarantee holds while such a chain fits in the quarter kept as headroom (2 MiB at 8 MiB), and a program whose non-recursive frames are larger can still be killed by the operating system |
| **What is not** | Tail calls are ordinary calls: no tail-call optimisation is performed or promised. Recursion depth is not bounded by the language, and the depth at which `N0408` arrives depends on frame sizes, the backend and the optimisation level |
| **The interpreter** | Unchanged: its limit is its call-depth budget, `N0403`. The two implementations agree on what a terminating program produces; each reports its own limit, and neither limit is a language rule |
| **The compiler written in Nazm** | Still refuses recursion by name: its emitter has no stack check, so the reason this section gave for refusing is still its reason |

**The general rule, of which these are instances: a runtime resource limit is an
implementation contract, never a silently invented language rule.** Where an
implementation imposes one it is stated, named in diagnostics, and recorded as that
implementation's choice — not promoted into the semantics because it happens to hold.

**`nazm build` is that different implementation, and it has chosen none of them.** A
compiled program has no iteration budget and no call-depth budget: a loop whose condition
never becomes false runs until something kills it, with no `N0402`. This is stated here so
that the difference is part of the record rather than a discovery. The two implementations
agree on what a *terminating* program produces; they do not agree on what a runaway one
does, and no test claims otherwise. Recursion was refused outright by both compilers until N49;
`nazm build` now compiles it with a measured stack guard, and the compiler written in Nazm still
refuses it — see *Native recursion, and the stack it runs on*, below.

### Strings are bytes, and what that commits to
<!-- id: strings -->

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
| **Construction** | `str_from_byte(b)` is a one-byte `Str`, and fails with `N0405` unless `0 <= b <= 255`. Since N49 a literal may write a byte below 128 with an escape (*String escapes*, below); `str_from_byte` remains the way to make one above 127 that UTF-8 source does not already spell |
| **Declassification** | `str_vouch(s)` is `s` with no origin, needing a `VouchCap` held (N52, *Declassification*) |
| **Allocation** | `str_concat`, `int_to_str` and `str_from_byte` allocate; `str_slice` and `str_vouch` do not. A compiled program reports `N0406` if the allocator refuses. The interpreter cannot: its host aborts on allocation failure, and that difference is stated rather than papered over |
| **Ownership** | Values, not references. Passing, returning, binding and copying a `Str` produce a string with the same bytes, and there is no way to observe whether a copy was made |
| **Mutation** | There is none. No operation changes an existing `Str` |

Source positions stay byte offsets throughout — `Span` is a byte range, diagnostics point
at byte ranges, and `str_len` counts the same units. A compiler written in Nazm therefore
reports positions that mean the same thing as the ones this implementation reports,
without a conversion step that could be got wrong.

#### String escapes — N49
<!-- id: string-escapes -->

| Escape | Byte | | Escape | Byte |
| --- | --- | --- | --- | --- |
| `\n` | 10 | | `\0` | 0 |
| `\t` | 9 | | `\\` | 92 |
| `\r` | 13 | | `\"` | 34 |
| `\xHH` | `0xHH`, for `HH` in `00`–`7F`, either case | | | |

Each escape is **one byte**. Everything else in a literal is its own bytes, so `"é"` is still the
two bytes of its UTF-8 encoding. A backslash followed by anything else, `\x` with fewer than two
hexadecimal digits, and `\x80`–`\xFF` are refused (`N0004`) at the escape. `\xHH` stops at `7F`
because a literal's bytes are UTF-8 text throughout this implementation and `str_from_byte` already
makes any byte; there is no `\u{…}`, because `Str` is bytes and a Unicode escape would be a text
encoding decision this specification has not made. `"\0"` is a one-byte string, as *Embedded NUL*
above says any NUL is. Diagnostics point at the bytes written, and the formatter keeps a literal
exactly as written. **Changed meaning:** before N49 a backslash in a literal was itself, so
`"a\nb"` was four bytes; it is now three.

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
<!-- id: sequences -->

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
<!-- id: memory -->

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

**4a. An expression that never finishes still gives back what it had produced.** There are
four ways to leave one part way through — a run-time failure, `return`, `break` and
`continue` — and every one of them is an ordinary death for the values already in hand:
the arguments already evaluated, the fields already stored into a half-built record, the
payload already stored into a half-built variant, and the value a `match` is holding while
an arm runs. None of them is reachable by the program again, so each is reclaimed then and
not later. `E.Value(a: f(), b: if done { continue; } else { g() })` reclaims what `f()`
produced, on the turn that abandons it, and a value produced **before** a loop and wanted
after it is untouched by a `break` inside that loop.

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

**N11 added the third, and took the first option again, 2026-09-23.** `Vec[T]` holds its
elements through a reference-counted handle, so a definition can reach itself with a finite
layout — `struct Node { children: Vec[Node], }` — and a value of it could own itself. The
type system refuses every such definition (`N0359`) over the ownership graph of definitions
described in *Generics*, beside the impossible layouts `N0336` already refused. So the
argument holds with one more step: no definition can reach itself through what it owns, so
no value can, and counting still reclaims every value of every type a program can write —
now including every `Vec` and every generic record or enum applied to such types.

**N10 extended the refusal rather than the argument.** An enum holds its active payload by
value too, so records and enums form **one** inline-containment graph and a cycle anywhere
in it is refused by the same rule and the same code. An enum adds no new kind of value
either: it is a composition of existing kinds of which one is live at a time, and a choice
between leaves is a leaf. The claim now covers every record **and every enum** built from
the six built-in types. What would end it is unchanged and still absent.

#### What is not promised

- **Leak freedom for a whole program, as a permanent property.** Every heap-backed type
  the language *currently has* is reclaimed, which is a claim about `Int`, `Bool`, `Str`,
  `Ints`, `Strs`, `Chan` and the records and enums composed of them, and about nothing else. It rests on the cycle argument below,
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
  destroyed by releasing each of its fields, an enum by releasing the fields of its active
  variant, and the order between fields is deliberately unspecified — no program can observe
  it, and fixing it in the specification would be promising something the language has no
  way to let anyone use.
- **Zero-cost record or enum copying.** A record with many owning fields costs one reference
  adjustment per owning field, per copy; an enum costs a branch on its discriminant and then
  the same, for the active variant alone. The design leaves that optimisable — a copy whose
  source dies immediately after needs no traffic at all — and until it is measured, nothing
  here claims it is free. `docs/performance.md` carries the numbers.

### What the outside-world built-ins do and do not promise
<!-- id: outside-world -->

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

#### Authority is explicit where a contract is written, and ambient elsewhere

**Since N37 a function that declares an effect set may reach these built-ins only while it
holds an `IoCap`** — a capability value its scope reaches, handed down from `main`, which is
the one place authority enters a program (*Capabilities: what allows a function to act*). A
function that declares no set is still ambient: it exercises its caller's authority, which
is how every program written before N37 is accepted unchanged.

*Corrected 2026-09-30, N37.* This section was titled *Authority is ambient, and that is a
limitation*, and said any function could call `read_file` and there was no capability
parameter. That is no longer true for a function with a declared set, and it is still true
for one without — which is now a **compatibility bridge**, bounded so that code with a
contract never reaches authority it does not hold by calling code without one. The
limitation that remains is stated there, not hidden here: the checking is static, v1
capabilities are coarse, and nothing at runtime sandboxes a Nazm program.

*Corrected 2026-09-29, N36.* This said there was no effect annotation. There is one now
(*Effects: what a function may do*): a function may declare `! { io }`, and one declaring
`! {}` may not reach these built-ins, directly or through anything it calls. That restricts
**what a function does**, not **who may do it** — which is what N37 then added.

This is stated rather than implied because the alternative — describing the design as
capability-based wherever a capability design exists — would be a claim the implementation
does not support. Partial enforcement labelled as full enforcement is worse than none.

The growth path this section once left open — *a capability parameter on `main`, threaded to
whatever needs it* — is the one N37 took, and for a stated reason rather than because an
earlier note suggested it: `main` is the only function the runtime calls, so it is the only
place a value can come from that no expression produced. The built-ins were shaped so that
the capability changes *what may call them*, not what they mean, and no program's
behaviour changed when it arrived.

#### There is no process spawning, deliberately

A Nazm program cannot run another program. A compiler written in Nazm therefore emits
LLVM IR to a file and a **script** invokes `clang` — which is the documented arrangement,
and much narrower than a language-level `run_command` would be. Adding one would be the
single largest authority grant available and it is not needed.

#### Errors are values only where they can be

There are no exceptions, and the built-ins return no `Result`. A failed read stops the
program with `N0407`. A program that wants to *handle* a missing file asks `file_exists`
first and reports it itself, which is what a compiler wants to do anyway. The gap between
the check and the read is a real race, and for a compiler reading its own inputs it is an
accepted one.

*Corrected 2026-09-24, N12.* This said there was no error type and that closing the race
needed one, which was a later decision. N12 made it: `Result` exists (*Typed error values*).
It did **not** change any built-in's signature — a runtime failure stays a runtime failure,
and never becomes an `Err` — so the race is still open, and a `read_file` returning a
`Result` is a separate decision about the outside-world built-ins.

### A file is a module, and `use` names one by path
<!-- id: modules -->

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
| **The prelude** | Every module also imports the core prelude, with no `use` written, and it is loaded after every project module. N12; see *Typed error values* |

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

### Packages — `nazm.toml`, `use "NAME:path"` and `nazm.lock`
<!-- id: packages -->

A **package** is a directory holding a manifest, `nazm.toml` (`nazm.package/1`): its name, its
exact version, its source root (`src` unless it says otherwise), an optional entry module, and
its dependencies — each a path and the exact version that package must state.

```toml
schema = "nazm.package/1"

[package]
name = "app"
version = "0.1.0"
main = "main.nz"

[dependencies]
geo = { path = "../geo", version = "1.2.0" }
```

`use "geo:lib.nz";` imports the module `lib.nz` of the package `app` declares as `geo`, resolved
against `geo`'s source root. A package sees its own dependencies and nothing else — not its
dependencies' dependencies (`N0508`) — and a relative import stays inside its package's source
root. A dependency's module is identified as `NAME@VERSION/path` (`geo@1.2.0/lib.nz`), which no
project path can be.

`nazm check|build|run DIR` builds the package in `DIR` from its `main` (`N0509` without one).
Resolution is exact and deterministic: packages are read in name order; a declared version its
manifest does not state is `N0502`, two packages of one name `N0503`, a cycle `N0504`, a missing
dependency `N0501`, an invalid manifest — an unknown key included — `N0500`. Every source is a
path, so nothing is ever downloaded.

`nazm lock DIR` writes `nazm.lock` (`nazm.lock/1`): each package's name, version, source relative
to the root package, and a BLAKE3 digest of its manifest and sources, in name order — the same
bytes for the same packages anywhere. `--locked` holds `check`, `build` or `run` to it before
anything is compiled: no lockfile is `N0505`, other packages than it records `N0506`, and a package
whose bytes changed since it was locked `N0507`.

**Reproducible builds.** The same sources, lockfile, toolchain, target, backend and settings give
byte-identical executables on one host, from empty caches, in any directory.

### Restriction profiles — `--profile`
<!-- id: profiles -->

A **profile** refuses some programs and changes nothing about the ones it accepts. It is chosen
with `--profile NAME` on `check`, `build` and `run`, or required by a package's manifest
(`profile = "NAME"` under `[package]`); every profile in force applies to the whole program.

| Profile | Rules |
| --- | --- |
| `general` | none |
| `embedded` | `no-io`, `no-spawn` |
| `critical` | `declared-effects`, `no-spawn`, `no-foreign`, `locked-build` |
| `cyber` | `declared-effects`, `no-foreign`, `locked-build` |

`no-io`, `no-spawn` and `no-foreign` refuse a program whose `main` requires that effect, through
any call, standard functions included. `declared-effects` refuses any function of the program that
writes no `! { … }`, so no function exercises its caller's authority. `locked-build` refuses a
build that is not a package build whose lockfile verifies. A refusal is `N0510`, naming the profile,
the rule and where it comes from; an unknown profile is `N0511`. `nazm check --profile-report`
prints `nazm.profile-report/1`. A profile is compiler-enforced evidence about these rules and
nothing more: not a certification, not a sandbox.

### Durable identity: what makes this the same module, or the same definition
<!-- id: durable-identity -->

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
| The relative path's first component is `@core` | That name is the toolchain's. Added by N12, so that no project module can ever be keyed as the core prelude is |

**One module's identity is not a path at all.** The core prelude's `ModuleKey` is
`@core/prelude`, fixed by the toolchain and the same in every compilation of every project
(*Typed error values*, above). It is disjoint from every project key by the row above, and
it is the one key a persisted interface may name that no source root computed.

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
<!-- id: concurrency -->

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

### Effects: what a function may do
<!-- id: effects -->

An **effect** is an externally observable power a function exercises beyond computing its
result from its arguments — one a caller could observe, and one someone may later want to
forbid. The checker computes every function's effects from its resolved calls and holds each
declared set to its body. Effects restrict which programs are accepted; they change nothing
about what an accepted program does, and add nothing to what runs.

```nazm
fn area(w: Int, h: Int) -> Int ! {} { w * h }

fn show(io: IoCap, n: Int) -> Int ! { io } { print(int_to_str(n)) }

fn worker(c: Chan) -> Int ! {} { chan_send(c, 1); 0 }

fn run(io: IoCap, tasks: SpawnCap) -> Int ! { io, spawn } {
    let c = chan_new(1);
    scope { spawn worker(c); }
    show(io, area(2, 3))
}
```

#### The effects

There are exactly two, and they are the compiler's. An effect is identified by the compiler,
never by where or how it is written, and there are **no user-defined effects**.

| Effect | Exercised by |
| --- | --- |
| `io` | the eight built-ins that reach the outside world — `args_count`, `arg`, `read_file`, `write_file`, `file_exists`, `print`, `eprint`, `exit_with` — and nothing else |
| `spawn` | a `spawn` statement: starting a task |

**Not effects**, and why:

- **Allocation.** Every string operation and every sequence allocates, and nothing the
  language can express restricts it; an `alloc` effect would be on nearly every function and
  forbid nothing anyone could act on. It is not tracked, rather than tracked unsoundly.
- **Mutation.** A `let mut` is local state and is nobody else's business. Writing into a
  sequence or a vector a caller handed over is visible to that caller — sequences are shared
  handles — but it is a property of the value passed, not a power exercised on the world, and
  an empty effect set does **not** promise it does not happen: pure here means *no effect*,
  not *referentially transparent*.
- **Channels.** A channel is a value too; sending, receiving and closing act on it. The power
  to run anything alongside is `spawn`'s, and a receive that blocks forever is divergence,
  which this model does not track (below).
- **Traps.** Overflow, division by zero, an index out of range and a failed read end the
  program; so can almost any arithmetic, so marking them would mark everything.
- **`Result` and `?`.** A failure written in the return type is a value, and `?` is a
  `return`. Neither is an effect, and an `! {}` function may propagate one freely.

#### A function's effect set

After the return type, a function may declare the effects it exercises: `! {}`, `! { io }`,
`! { io, spawn }`. The names live in their **own namespace** — `io` is not a variable, a type
or a function, and a function or parameter named `io` is no conflict. A set means the same in
any order and the formatter writes it in the canonical one; a name that is not an effect is
`N0367`, and one written twice is `N0368`, with a fix that removes the repetition.

**Pure is the empty set**: `! {}` asserts that the function exercises no effect and calls
nothing that does, directly or through any number of calls. There is no separate `pure`
word.

#### Declared and inferred

- **Every function's effects are inferred**: the effects of what its body calls — a
  built-in's own effect, a `spawn`'s `spawn` and everything its task does, and each callee's
  effects — through any number of calls. For recursion and mutual recursion the answer is the
  **least fixed point**: a cycle of functions none of which reaches `io` does not require
  `io`, and one `print` anywhere on it reaches every function of the cycle. Recursion adds no
  effect of its own, and declaration order changes nothing.
- **A declared set is a contract.** The body may require no effect outside it: requiring
  more is `N0366`, at the first call or `spawn` that needs the missing effect, with one
  shortest chain of calls from there to the built-in or `spawn` that exercises it. Declaring
  **more** than the body needs is allowed: the contract is wider than the implementation, and
  a caller reads the contract.
- **A caller reads a callee's contract when it has one**, and what its body requires when it
  has none — but only within the callee's own module.

**No set is not the empty set.** `fn f() -> Int` and `fn f() -> Int ! {}` are different
declarations, and only the second promises anything:

| Written | Contract | Its own module's callers see | Another module's callers see | Authority (N37) |
| --- | --- | --- | --- | --- |
| `fn f() -> Int` | none | what its body requires, inferred | every effect, `{ io, spawn }` | its caller's: the compatibility bridge |
| `fn f() -> Int ! {}` | pure | `{}` | `{}` | only the capabilities its scope holds |
| `fn f(io: IoCap) -> Int ! { io }` | may reach the outside world | `{ io }` | `{ io }` | the `IoCap` it is handed |

Omitting the set is never a way to say *pure*: to another module an undeclared function may
do anything, and its effects are inferred only for its own module's callers.

#### Across modules

**A caller never needs another module's body to know its effects.** What crosses a module
boundary is the declared set, which is part of the module's published interface. An exported
function that declares **no** set promises nothing, so a caller in another module must
assume it may exercise **every** effect: a pure function may call an import only if the import
declares `! {}`. Declaring is how an exported function becomes callable from pure code; a
program that declares nothing anywhere is accepted exactly as before N36.

Changing a declared set changes the interface, so every importer is checked again. Changing a
body within its declared set changes nothing an importer sees.

#### Generics, and what is not here

A generic function's effects are its body's, whatever its type arguments: there are no
function values and no methods, so nothing a type argument brings can call anything. For
the same reason there is no effect polymorphism — no effect variables, no handler, nothing
that removes an effect once it is required. Those are later work, and a later milestone
adds them rather than N36 pretending concrete sets cover them.

**Effects are not authority.** A function declaring `! { io }` says what kind of power it
uses, not that anything granted it. Since N37 who may exercise a power is a separate
question with a separate check: *Capabilities: what allows a function to act*, next.

### Capabilities: what allows a function to act
<!-- id: capabilities -->

An effect describes **behaviour**: `! { io }` says a function may reach the outside world. A
**capability** describes **authority**: a value of a compiler-owned type whose presence in a
function's scope is what allows it to. The two are separate facts, checked separately and
reported under separate codes, and neither implies the other.

```nazm
fn save(io: IoCap, path: Str, data: Str) -> Int ! { io } { write_file(path, data) }

fn log(io: IoCap, s: Str) -> Int ! { io } { print(s) }

fn keep(io: IoCap) -> IoCap ! {} { io }

fn main(io: IoCap) -> Int ! { io } {
    log(keep(io), "saving");
    save(io, "out.txt", "data");
    0
}
```

#### The capabilities

There are exactly three, and they are the compiler's. There are **no user-defined
capabilities**.

| Kind | Type | Authorises | Effect it authorises |
| --- | --- | --- | --- |
| `Io` | `IoCap` | the eight outside-world built-ins — `args_count`, `arg`, `read_file`, `write_file`, `file_exists`, `print`, `eprint`, `exit_with` | `io` |
| `Spawn` | `SpawnCap` | a `spawn` statement: starting a task | `spawn` |
| `Foreign` | `ForeignCap` | a call to a foreign function: running C (N42) | `foreign` |
| `Vouch` | `VouchCap` | `str_vouch`: declassifying a string (N52), in every function | none |

A kind is identified by the compiler, never by how its type is spelled. `IoCap`,
`SpawnCap`, `ForeignCap` and `VouchCap` are **built-in type names**, like `Chan`: they mean the same in every module, and a
module may not declare a type of either name (`N0331`). Which authority a built-in needs is a
property of the built-in's identity, decided by an exhaustive table the checker reads — a
program's own function cannot be named `print`, so a call spelled `print` is the built-in.

**v1 capabilities are coarse.** An `IoCap` authorises the arguments, every file, both
standard streams and ending the process alike: it is not least privilege, and it does not
distinguish a read from a write, one file from another, or output from exit. A capability
authorises its **kind**; a resource-specific capability — one file, one directory — would be
a kind with a type argument, and none exists yet.

#### Authority is a value in scope

**A function that declares an effect set holds exactly the authority of the capability values
its scope reaches**: at each point in its body, every parameter or `let` of a capability type
whose name is visible there. A binding hidden by a later one of the same name holds nothing,
since no expression can reach it; a record, `Vec` or variant that contains a capability holds
nothing until the capability is bound; a type parameter holds nothing whatever it is
instantiated with, since nothing is known about it. Possession is read from bindings a reader
can see, and from nothing else — there is no flag and no hidden grant.

At each point, what is needed:

- **A built-in** needs the authority its identity requires.
- **A `spawn`** needs a `SpawnCap`, to start the task. What the task then does is its function's
  business: a function with a declared set has only what the `spawn` hands it as arguments, so
  authority enters a task only by being **passed**, like any value. A task never gains its
  spawner's authority by being started.
- **A call to a function with a declared set** needs nothing more here. Its authority is in
  its parameters, and the caller supplies them as it supplies any argument — ordinary type
  checking, with a missing or wrong capability being a missing or wrong argument.
- **A call — or a `spawn` — of a function with no declared set** needs whatever that function's
  effects need: in its own module, its inferred effects; from another module, every effect.

Lacking any of these is `N0369`, once per missing kind, at the first point that needs it, with
the chain through undeclared functions to what needs the authority. It is never fixed
automatically: threading authority through parameters changes every caller.

#### Effects and authority are checked apart

| The function… | holds the capability | does not hold it |
| --- | --- | --- |
| **declares the effect** | accepted | `N0369` — authority |
| **does not declare it** | `N0366` — effect | `N0369`, then `N0366` |

A body wrong in both gets both, authority first, in that order every time. Holding a
capability is not exercising it: receiving, passing, returning and storing one adds no effect,
so `fn keep(io: IoCap) -> IoCap ! {}` is pure. Holding authority a body never uses is allowed,
as declaring an effect it never exercises is.

#### Where authority comes from

**`main` is the root of authority.** The runtime calls it, and hands it one capability of each
kind it declares a parameter of — any number, in any order. A `main` parameter of any other
type is refused (`N0371`): the runtime has nothing else to hand over. Every capability in a
running program is one of those, or a copy of one.

**Nothing a program writes constructs one.** `IoCap()` and `IoCap(x: 1)` are refused
(`N0370`); there is no literal, no conversion from another type and no default value, and a
generic function cannot produce a value of a type parameter it was not given. A capability is
unforgeable because no expression but a parameter of `main` has one to give.

#### A value like any other, with two exceptions

A capability is **shareable**: it is copied freely, and a copy is the same authority — there
is no linear or affine use, and so no revocation. It may be stored in a record, a `Vec` or an
enum payload, returned, passed through a generic function, and handed to a task as a `spawn`
argument; matching a payload binds the capability that was stored, and creates none. The two
exceptions:

- **No equality.** `==` on a capability is `N0304`, and a record or enum containing one derives
  none: two authority values are not data to compare.
- **No representation a program can read.** No built-in takes one, and nothing converts one
  to an `Int` or a `Str`, so a capability cannot be printed, written to a file or persisted.

#### The compatibility bridge: a function with no declared set

A function that declares no effect set is **ambient**: it has no authority of its own and
exercises its **caller's**. That is how every program written before N37 is accepted unchanged
— a `main` with no set is the bridge's root and is handed all of it, and every undeclared
function it calls exercises it in turn.

The bridge is bounded, and the bound is the point: a function that declares a set may call or
spawn an ambient one **only while holding what that callee's effects need** — every effect, for
an import it cannot read. So code that states its contract never reaches authority it does not
hold by calling code that states none, in its own module or another. Declaring a set is how a
function leaves the bridge; nothing makes a function leave it silently, and nothing lets one
re-enter it.

#### Across modules

**A caller never needs another module's body to know what authority it requires.** A
capability parameter is an ordinary parameter of a built-in type, so it is in the published
interface as any parameter is, and an importer checks its call against it. Changing which
capability a function takes — adding one, removing one, or trading `IoCap` for `SpawnCap` —
changes the interface, and every importer is checked again.

#### What is checked, and what is not

**The checking is static, and nothing at runtime enforces it.** A capability erases: each
lowers to a machine word holding 0, which nothing reads, and a program behaves identically
interpreted and compiled, with its capabilities or with `Int`s in their place. Erasure permits
no forgery, because forgery is a question the checker answers before anything runs: no
expression produces a capability, so there is nothing at runtime to fake.

There is **no sandbox**. A Nazm program can affect its machine exactly as much as the process
running it can; an operating-system boundary, a broker or a resource quota is not part of the
language. What N37 establishes is narrower and exact: **Nazm v1 statically represents and
checks explicit compiler-owned authority** — for every function that declares its effects —
and labels everywhere else as the ambient bridge it is.

Not in v1, and not implied by anything above: attenuation (there is no narrower capability to
derive one into), resource-specific authority, revocation, linear or affine capabilities,
capability polymorphism, user-defined capabilities, restriction profiles, and authority for a
foreign function or unsafe code (*Open*, *Capabilities*).

### Foreign functions: calling C
<!-- id: foreign -->

A **foreign function** is a C function a program calls. It is declared by its signature and its
C symbol, and it has no body: the body is C's, compiled by a C compiler and handed to the linker
with `nazm build --link FILE`. Nazm does not compile C, and does not check it.

```nazm
extern "C" fn c_add(a: Int, b: Int) -> Int = "c_add";
extern "C" fn c_is_even(n: Int) -> Bool = "c_is_even";

fn classify(f: ForeignCap, n: Int) -> Int ! { foreign } {
    if c_is_even(n) { c_add(n, 1000) } else { c_add(n, 100) }
}

fn main(io: IoCap, f: ForeignCap) -> Int ! { io, foreign } {
    print(int_to_str(classify(f, 3)));
    0
}
```

**The boundary is the C convention of the target, and only two types cross it.** `Int` is C's
`int64_t` and `Bool` is C's `bool`, both by value. Nothing else is FFI-safe in v1 (`N0381`): a
`Str` is not a C string (it is counted, not terminated, and reference-counted); a record or an
enum has no C layout Nazm promises; a sequence or a channel is the runtime's; and a capability is
authority, never a word C could forge or keep. No pointer crosses, so C cannot retain, free or
alias anything Nazm owns, and nothing Nazm owns needs pinning across a call. No callback crosses
either: Nazm has no function values.

**The declaration.** `extern` begins it, before the ABI string — `"C"`, the only ABI (`N0380`);
`extern` is recognised only there, and is not otherwise reserved. The symbol after `=` is the C
identifier the linker resolves, exactly as written: ASCII letters, digits and `_`, not starting
with a digit (`N0382`). A foreign declaration takes no type parameters and writes no effect set
(`N0384`): its effect is always `foreign`. Two declarations of one symbol must agree about its
signature, and none may name a symbol the native runtime itself declares, such as `malloc` or
`main` (`N0383`). A `pub` foreign declaration is exported like any function, and the persisted
interface carries its symbol.

**Calling one is an effect and needs authority.** A foreign function's effect is `foreign`: a
function whose declared set leaves it out cannot call one (`N0366`). And **every** call to one
needs a `ForeignCap` in scope, whether or not the calling function declares an effect set
(`N0369`): C can do anything, so the compatibility bridge that lets an undeclared function
exercise its caller's `io` does not reach it. A function in another module that declares no
effects is assumed to call C only if one of its parameters can carry a `ForeignCap`.

**Failure and ownership.** C cannot fail the Nazm way: after a foreign call nothing is checked,
and a C function that aborts, loops or corrupts memory does so outside every Nazm guarantee. An
integer overflow inside C is C's. What Nazm owns stays Nazm's across the call: values held in
the calling function are released on every path exactly as around any call.

**Provenance.** What C returns carries the origin `unknown`, the worst there is, together with
whatever its arguments carried: it may be anything, and may be derived from what it was given. So
in a function that declares its effects, a `write_file` path computed from a C result is refused
(`N0372`), exactly as one read from a file is. What C does with an argument is not a Nazm sink —
it is authorised by the `ForeignCap` the call needed, and outside what a restriction can see.

**Where it runs.** Only in a native build. `nazm run` refuses a program that calls a foreign
function before running any of it (`N0385`); one that only declares one runs. `nazm build`
declares each foreign function in every unit that calls it and defines it in none; a missing
definition is the linker's refusal, reported as a missing `--link` naming the symbols. Both
backends call C the same way.

### Provenance: where a value came from, and where it may go
<!-- id: provenance -->

A value's **provenance** is the set of **origins** its contents were derived from by explicit
data flow. It is a checker fact, computed from resolved bodies and erased before anything
runs: not an effect (reading a file is `io`; the text read is not), not a capability (knowing
a value came from the root authority grants none), not a type, and not a runtime tag.

```nazm
fn lines(text: Str) -> Int ! {} {
    let mut n = 0;
    let mut i = 0;
    while i < str_len(text) {
        if str_byte(text, i) == 10 { n = n + 1; }
        i = i + 1;
    }
    n
}

fn main(io: IoCap) -> Int ! { io } {
    let text = read_file("notes.txt");      // from a file's contents
    write_file("count.txt", int_to_str(lines(text)));   // a path the program chose: allowed
    0
}
```

#### The origins

Four, and the compiler's; there are no user-defined labels. The absence of every origin is
**local**: a literal, or anything computed only from literals.

| Origin | Where it enters |
| --- | --- |
| `argument` | `arg` and `args_count`: the command line, supplied by whoever ran the program |
| `file` | `read_file`'s contents and `file_exists`'s answer: supplied by whoever could write that file |
| `authority` | a capability `main` is handed (N37): the runtime's root authority |
| `unknown` | a value read out of a sequence, a `Vec` or a channel, which the checker does not follow |

An origin is identified by the compiler, and a built-in's by an exhaustive table on its
identity — never by spelling, and no program can write a label. Output is not input: `print`,
`eprint` and `write_file` are not sources. v1 is coarse: an origin is a *kind* of source, not a
particular argument or file, and a diagnostic names where each one was first met.

`unknown` is explicit and never trusted. Sequences and channels are shared handles, and a value
read out of one is whatever any alias wrote, which a local analysis cannot see; so it may be any
of the others, and a restriction treats it as the worst of them.

#### How provenance flows

**Explicit data flow only.** A value carries what it was computed from:

- a binding, everything ever assigned to it — flow-insensitively, to a fixed point, so a loop is
  not a special case;
- an operator, both operands'; a built-in that computes, its arguments';
- a record or a variant, its fields'; a field read, a pattern's binding and `?`, the whole value's
  — records, variants, `Result` and `Option` are tracked as a whole, not field by field;
- a call, its callee's summary applied to its arguments;
- an `if` or a `match`, its arms'. **The condition contributes nothing**: branching on a value is
  implicit flow, which v1 does not track.

A **join** is a set union: commutative, associative and idempotent, so the order values are
combined in changes nothing they carry.

#### Function summaries

A function's provenance is summarised, over the whole program, as three facts:

- the **origins** its result carries whatever it is given;
- the **parameters it passes on**: those its result is computed from. `fn id(x: Str) -> Str { x }`
  passes on `x`; `fn len(x: Str) -> Int { str_len(x) }` does too; `fn one(x: Str) -> Int { 1 }`
  passes on nothing, and its result is local whatever it is handed;
- the **parameters that reach a restricted sink** (below).

Summaries are the **least fixed point** over every call, `spawn`, recursion, mutual recursion and
import cycle: they start empty and grow until nothing changes, over a finite set of origins and
parameter positions, so they always settle, and source order changes nothing. A caller reads its
callee's summary, never its body.

#### Across modules

A module's bodies are reduced to **facts** — each function's flows in terms of its parameters and
the calls it makes — which depend only on the module's source and its imports' declarations, as
its checking does. A cache keeps them beside the module's other results, so a module whose bodies
are not checked again still contributes them. Every run then solves the whole program's summaries
from all modules' facts: a callee's changed body changes what its caller is told in the same run,
though the caller was not checked again. An importer never needs another module's body.

#### Provenance, effects and authority are three facts

External data is not an effect: once read, computing over it is pure, and `lines` above declares
`! {}`. Provenance never adds an effect, never removes one, and never grants authority — a value
known to have come from an `IoCap` is not an `IoCap` in scope. Passing, storing and spawning with a
capability keep its `authority` origin: a parameter records the origins its callers hand it, so a
task's capability is recorded as the spawner's.

#### The restricted flow

**In a function that declares an effect set, the path given to `write_file` must not derive from
a file's contents or from what the checker does not follow** (`N0372`). It may come from the
command line — whoever ran the program already holds every authority it has — or be computed in
the program. The rule reaches through calls: a function whose parameter reaches `write_file`'s
path, however many calls deep and whether it declares effects or not, carries the rule to its
callers, and the call that hands it a file's contents is refused there. The diagnostic shows one
chain, to the `write_file` and to where the contents were read. There is no automatic fix and no
way to vouch for such a value in v1: declassification and sanitisers are later work.

A function that declares no effect set is the bridge N37 describes, and its own writes are not
checked; but explicit code cannot launder a file's contents through it.

Authority needs no restriction of its own here: no built-in accepts a capability and none
converts one to data, so authority reaching output, a file or a channel is already impossible by
type, and provenance does not claim to enforce what types already do.

#### What this is not

**Explicit information-flow provenance**, statically, for one sink. Not non-interference — implicit
flow through conditions is not tracked — and not taint tracking at runtime, confidentiality
enforcement, side-channel or constant-time guarantees, or a sandbox. Records, variants and
containers are coarse, shared storage is `unknown`, and there are no user labels, no
declassification and no policy language.

### The integer boundaries, stated rather than inherited
<!-- id: integer-boundaries -->

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
<!-- id: one-error -->

A parse error inside a function body does not delete the function. Its signature is kept,
so calls to it still check against real parameter and return types instead of reporting
that it does not exist — which would be several errors for one mistake, the exact failure
this language is meant to avoid. Nothing is said about the body itself: statements that
were never built would be claims about the parser's guess, not about the program.

Since N15 the parser resumes after a malformed statement, so a second, independent syntax
error in the same body is reported too; what follows from the first — a stray token left
behind by a missing brace — is not reported again. The body is still withheld from checking.

### Logical operators — `&&`, `||`, `!` — N49
<!-- id: logical-operators -->

| | |
| --- | --- |
| **Syntax** | `!e` is a prefix, beside unary `-`. `a && b` and `a \|\| b` are infix and left-associative |
| **Precedence** | `\|\|` binds loosest, then `&&`, then the comparisons, then `+ -`, then `* / %`, then the prefixes: `a == b && c < d \|\| !e` is `((a == b) && (c < d)) \|\| (!e)` |
| **Types** | Every operand is `Bool`, and so is the result. Anything else is `N0304`, as for every operator |
| **Short-circuit** | The left operand is evaluated first. `a && b` evaluates `b` only when `a` is `true`; `a \|\| b` only when `a` is `false`. So the right operand's effects, failures and non-termination happen only then |
| **Meaning** | `a && b` is `if a { b } else { false }`, `a \|\| b` is `if a { true } else { b }`, and `!e` is `if e { false } else { true }` — not as an analogy: that is what they are, in every analysis and every implementation |
| **Not** | No bitwise operators, no `and`/`or`/`not` words, and no truthiness: `!0` is a type error |

### Qualified names — `use "PATH" as NAME;` and `NAME::item` — N49
<!-- id: qualified-names -->

```nazm
use "@std/text" as t;
use "geometry:shapes.nz" as g;

fn main() -> Int { str_len(t::text_trim(" a ")) + g::area(g::Point(x: 2, y: 3)) }
```

| | |
| --- | --- |
| **Importing under a name** | `use "PATH" as NAME;` loads the module as `use "PATH";` does, and binds `NAME` to it in the importing module. **Its exports are not brought into scope unqualified**: they collide with nothing, and only `NAME::item` names them |
| **Where a qualified name is written** | Wherever a definition is named: a call `NAME::f(…)`, a construction `NAME::R(…)`, a variant `NAME::E.V(…)`, and a type `NAME::R` or `NAME::E[Int]`. Nazm has no module-level values, so a qualified name is never a variable |
| **What it means** | Exactly the definition the unqualified import would have named — the same identity, signature, effects and authority. Qualification chooses a definition; it does not create one |
| **Scope** | An alias belongs to the module that writes it, as an import does. Two aliases may name one module. A module imported both plainly and under a name is one module, seen both ways |
| **Refused** | An alias that is also one of the module's own definitions, a type, or another alias (`N0209`); `NAME::item` where `NAME` is no alias or the module does not export `item` (`N0210`) |
| **`as`** | Recognised only after a `use` path. It takes no identifier from programs |
| **Built-ins and the prelude** | Not modules a program imports, so never qualified: `str_len`, `Result` and the rest are written as before |

### Function values and closures — N50
<!-- id: function-values -->

```nazm
fn double(n: Int) -> Int { n * 2 }
fn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }

fn main() -> Int {
    let step = 3;
    let add = fn(x: Int) -> Int { x + step };
    twice(double, 5) + twice(add, 1)
}
```

| | |
| --- | --- |
| **Type** | `fn(T1, …, Tn) -> R`, with `! { e… }` after `R` for a function that performs effects. Unwritten effects are none. Two function types are one exactly when their parameters, result and effects are equal; there is no subtyping between them |
| **Equality** | None. `==` on a function value is refused, as on a capability |
| **A named function as a value** | A non-generic function that is not foreign, written without a call: `double`. Its type is its signature's. A generic function, a built-in and a foreign function are not values (`N0387`); write a closure that calls one |
| **A closure** | `fn(x: Int) -> Int { … }` in expression position, its parameters and result written in full, its effects `! { … }` declared or none. Its body is checked against its declared effects as a declared function's is |
| **Capture** | A closure may name the bindings of the functions and closures around it. Each it names is **copied** into the closure when the closure is created: a value is a copy, a handle (`Ints`, `Vec`, `Chan`) is the same storage. A `let mut` binding and a capability may not be captured (`N0386`) |
| **Call** | `f(a, b)`, where `f` is a parameter or binding of function type: the arguments must match its parameters, the result is its result, and the call performs its type's effects. Such a binding hides a function of the same name for calls in its scope. A function stored in a field is called by binding it first: `let g = r.f; g(1)` |
| **Ownership** | A function value is a handle to its closure; copies share it, and what it captured is released when the last copy goes |
| **Tasks** | A function value may not be passed to a task (`N0321`) |
| **In a generic function** | Neither a closure nor a named function as a value may be written in the body of a generic function (`N0387`); a function value may be passed into one as an argument, and called there |
| **Recursion through a value** | A native function that calls through a function value checks its stack first, as a recursive one does (`N0408`, *Native recursion*) |
| **Not here** | Traits, methods, generic function values, a closure that names itself, effect-polymorphic function types — see `architecture.md` §7.52 |

### Effect parameters and captured authority — N51
<!-- id: effect-parameters -->

```nazm
use "@std/seq";

fn main(io: IoCap) -> Int ! { io } {
    let xs = vec_new[Int]();
    vec_push(xs, 1);
    vec_push(xs, 2);
    let shown = vec_map(xs, fn(x: Int) -> Int ! { io } { print(int_to_str(x)); x * 10 });
    vec_len(shown)
}
```

| | |
| --- | --- |
| **Declaring one** | `fn name[T…, effects E](…)`: at most one effect parameter, after the type parameters. Its name may be written in the effect sets of the function's own signature and of closures in its body. `main` may not have one |
| **In the body** | Opaque. Calling something whose type has `E` performs `E`, so the declared set must include it; `E` needs no capability |
| **At a call** | Bound from the arguments: a parameter typed `fn(…) -> R ! { k…, E }` given a value of type `fn(…) -> R ! { a… }` binds `E` to `a…` without `k…` (and `k…` must be within `a…`). Bound to two different sets is `N0389`; bound by nothing, it is empty. The call performs the callee's declared set with `E` replaced |
| **Capture** | A closure may capture a capability; its body then holds that authority as well as its own capability parameters'. Its type still declares what it does. A `let mut` binding still may not be captured (`N0386`) |
| **Values of undeclared functions** | Making one needs, where it is made, the capabilities its type's effects need, as calling the function would (`N0369`) |
| **Not here** | Two effect parameters, effect subtyping, attenuated capabilities, revocation — see `architecture.md` §7.53 |

### Declassification — N52
<!-- id: declassification -->

```nazm
fn main(io: IoCap, v: VouchCap) -> Int ! { io } {
    let secret = read_file("token.txt");
    let shown = str_vouch(str_slice(secret, 0, 4));
    print(shown)
}
```

| | |
| --- | --- |
| **`str_vouch(s: Str) -> Str`** | Its argument, with no origin. Needs a `VouchCap` held where it is written, in every function (`N0369` otherwise) |
| **`VouchCap`** | A capability kind, minted only for `main`'s parameters; no effect is authorised by it |
| **Evidence** | Every vouch is recorded with the origins it cleared; `nazm explain-flow` prints them beside every sink |
| **Policy** | The language refuses one flow (`N0372`); profiles refuse more: `cyber`'s `contents-stay-in-files`, `critical`'s `no-declassification` |

### Generic channels — N53
<!-- id: generic-channels -->

```nazm
struct Job { name: Str, size: Int, }

fn worker(jobs: Chan[Job], done: Chan[Str]) -> Int ! {} {
    let got = vec_new[Job]();
    while chan_recv_of(jobs, got) {
        let j = vec_pop(got);
        chan_send_of(done, str_concat(j.name, "!"));
    }
    chan_close_of(done)
}
```

| | |
| --- | --- |
| **Type** | `Chan[T]`, for a `T` that may cross into a task — the rule a `spawn` argument follows (`N0390` otherwise). `Chan` alone is the `Int` channel, unchanged |
| **Built-ins** | `chan_new_of[T](capacity: Int) -> Chan[T]`, `chan_send_of(c, v: T) -> Bool`, `chan_recv_of(c, out: Vec[T]) -> Bool`, `chan_close_of(c) -> Int` — `Chan`'s semantics exactly: blocking, closing, draining |
| **Ownership** | A send copies; a value a closed channel refuses is released; a receive moves the value into `out`; a channel destroyed with values queued releases each |
| **Equality, tasks** | As `Chan`: no equality; may cross into a task |

### Select, deadlines and cancellation — N54
<!-- id: select-deadlines -->

```nazm
fn worker(stop: Chan[Int], jobs: Chan[Int], out: Chan[Int]) -> Int ! {} {
    let cs = vec_new[Chan[Int]]();
    vec_push(cs, stop);
    vec_push(cs, jobs);
    let got = vec_new[Int]();
    let mut running = true;
    while running {
        let i = chan_select_of(cs, got);
        if i == 1 && vec_len(got) > 0 {
            chan_send_of(out, vec_pop(got) * 2);
        } else {
            running = false;
        }
    }
    chan_close_of(out)
}
```

| | |
| --- | --- |
| **`chan_select_of(cs: Vec[Chan[T]], out: Vec[T]) -> Int`** | Waits until a channel in `cs` is ready — holds a value, or is closed and drained — and returns the lowest ready index; a value is moved onto `out`, a closed channel gives nothing. `-1` for an empty `cs`. Schedule-dependent, deterministically tie-broken |
| **`chan_select_until(cs, out, ms: Int) -> Int`** | As `chan_select_of`, or `-2` once at least `ms` milliseconds pass; `ms <= 0` looks once. No effect; needs a `TimeCap` held where it is written, in every function (`N0369` otherwise) |
| **`time_now_ms() -> Int`** | A monotonic clock, in milliseconds. No effect; needs a `TimeCap` |
| **`TimeCap`** | A built-in capability type, like `IoCap`; `critical`'s `no-ambient-time` refuses any use |
| **Cancellation** | Cooperative: close a channel the task selects on. Observed only where a task waits; a sibling's failure cancels nothing (N44 unchanged) |
| **Counters** | `NAZM_SCHED_REPORT=1`: `nazm-sched: spawned=… joined=… selects=… timeouts=…` on standard error at exit |

### Foreign functions v2: strings in, exports out — N55
<!-- id: ffi-v2 -->

```nazm
extern "C" fn c_strlen(s: Str) -> Int = "strlen_of";

pub extern "C" fn nazm_triple(x: Int) -> Int ! {} = "nazm_triple" { x * 3 }
```

| | |
| --- | --- |
| **`Str` argument** | A NUL-terminated copy for the call's duration; C borrows it. A `Str` holding a NUL byte fails the call before C runs (`N0405`). A `Str` result is still refused (`N0381`) |
| **Export** | `pub extern "C" fn … ! {} = "symbol" { … }`: a C-callable function of `Int`s and `Bool`s, public, effect-free and not generic (`N0391` otherwise) |
| **Failure in an export** | Reported on standard error; the process exits with status 2. Nothing unwinds into C |
| **Library** | `nazm build --lib FILE -o OUT.a` writes the static archive and `OUT.h`; no `main` needed |

### Packages v2: a registry and version requirements — N56
<!-- id: registry -->

```toml
schema = "nazm.package/1"

[package]
name = "app"
version = "1.0.0"
main = "main.nz"

[registry]
path = "../registry"

[dependencies]
shapes = { version = "^1.2.0" }
geometry = { path = "../geometry", version = "0.3.1" }
```

| | |
| --- | --- |
| **Requirement** | `1.2.3` / `=1.2.3` exactly; `^1.2.3` the same left-most non-zero component; `~1.2.3` the same minor |
| **Choice** | per name, the lockfile's version while it satisfies every requirement, else the highest non-yanked one that does; one version per name, no backtracking (`N0513` otherwise) |
| **Store** | `REGISTRY/index/NAME.toml` and an immutable copy per version, checked against its digest (`N0507`), refused if it holds a link (`N0514`) or its index is malformed (`N0512`) |
| **Commands** | `nazm publish DIR --registry REG`; `nazm yank NAME@VERSION --registry REG [--undo]`; `nazm lock`, `--locked` as N46 |
| **Imports** | unchanged: `use "shapes:polygon.nz";` |

### Not in v0.3, and unsupported rather than half-present
<!-- id: not-in-v0-3 -->

Valueless `return`, labelled break, `for`, `loop`, mutable parameters, floating point. A
program using any of them is rejected by name rather than misparsed into something else.

**Corrected 2026-10-01, N50.** Function values and closures exist, specified above; traits and
methods remain absent and are refused.

**Corrected 2026-10-01, N49.** String escapes, the logical operators `&&`, `||` and `!`, and
qualified names exist, each specified above; native recursion compiles.

**Corrected 2026-09-29, N36.** This list named **effects**, which N36 added: a function may
declare the effects it exercises and the checker infers and holds them — *Effects: what a
function may do*, above, is the law. Handlers, effect variables and user-defined effects
remain absent.

Exceptions, `throw`, `catch` and unwinding are not in the language and are not planned:
N12's typed error values are values, and `?` is a `return` (*Typed error values*, above).

**Corrected 2026-09-23, N11.** This list named **generics**, which N11 added: records, enums
and functions may declare type parameters, and `Vec[T]` is the one generic built-in — the
section *Generics* above is the law. **Traits, methods, constraints and every other item in
*What generics are not, in N11*** remain absent and remain refused.

**Corrected 2026-09-23.** This list also named **user-defined types**, which N9 added:
`struct` declares a record, and the section *Records* above is the law. It then named
**variants and pattern matching**, which N10 added: `enum` declares a closed sum type and
`match` eliminates one, and the section *Enums* above is the law. Generics, traits and
methods remain absent and remain refused by name.

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
<!-- id: open -->

### Source identity and definition identity
<!-- id: open-identity -->

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
<!-- id: open-memory -->

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
<!-- id: open-effects -->

*Settled by N36*: what an effect is, the two built-in effects, declared and inferred sets,
purity as the empty set, and what crosses a module (*Effects: what a function may do*). Still
open:

- **Is divergence an effect?** The `forever()` counterexample in `architecture.md` §5 shows
  that without an answer, "drop an unused pure call" is unsound. Options: treat divergence
  as an effect; track termination separately (`willreturn`-style); or refuse the
  transformation entirely.
- Is allocation failure an effect, a panic, or a return value?
- Is panic an effect, an unwind, or both, and what is observable about it?
- Evaluation order: specified, or deliberately unspecified? If unspecified, differential
  testing across backends cannot compare it (`architecture.md` §6).
- Which handlers lower statically, and which require one-shot continuation capture?

### Capabilities
<!-- id: open-capabilities -->

*Settled by N37*: what a capability is, the two kinds, lexical possession, `main` as the root,
unforgeability, separate authority and effect checks, the ambient bridge and its bound, and
static erasure (*Capabilities: what allows a function to act*). Still open:

- **Attenuation and resource-specific authority.** A read-only `IoCap`, one file's, one
  directory's: each needs a kind with a resource argument, and the rule for deriving a
  narrower capability from a wider one. v1 has no narrower kind to derive into.
- **Linearity.** Whether some authority should be affine — usable once, or movable but not
  copyable — needs move semantics the language does not have.
- **Revocation and lifetime.** A copied capability lives as long as any copy; a scoped grant
  needs a lifetime analysis.
- **Retiring the bridge.** When a function with no declared set stops being ambient, and what
  a program declaring nothing is migrated to.
- **Runtime enforcement.** Whether a later milestone backs the static check with an
  operating-system boundary for a program that crosses into foreign code.
- **Foreign and unsafe authority.** A foreign-function call or unsafe code would need
  authority of its own kind.

### Provenance and information flow
<!-- id: open-provenance -->

*Settled by N38*: the four origins, explicit data flow, joins, summaries and their fixed point,
facts across modules, the separation from effects and authority, and one restricted sink
(*Provenance: where a value came from*). Still open:

- Field-, element- and alias-precise tracking, instead of whole values and `unknown` shared storage.

- Implicit flows: is branching on a `Secret` a leak? Full non-interference is usually
  unusable in practice; what is the chosen approximation, and what does it miss?
- Sanitizer functions: how are they declared, and what stops a no-op being declared one?
- Aliasing: how does a label propagate through a projection?
- Declassification: which capability, granted where, audited how?
- The FFI boundary: what label does a value returned from C carry?

### Concurrency
<!-- id: open-concurrency -->

- Captured variables, globals/statics, and foreign pointers are **not** covered by channel
  ownership transfer. Each needs a rule.
- Preemption: cooperative at back-edges and calls, or signal-based?
- What exactly does cancellation cancel, and what cleanup is guaranteed to run?

### Layout selection
<!-- id: open-layout -->

- The eligibility predicate: which types may the compiler re-lay-out?
- Behaviour under address identity, escaping references, FFI, serialization, atomics, and
  separate compilation (`architecture.md` §4).

### Backend semantics
<!-- id: open-backend -->

Required before `nazm test --backend=all` means anything:

- integer overflow behaviour
- floating-point semantics (fast-math? reassociation? NaN payloads?)
- panic and unwind behaviour
- what counts as an observable output

### Syntax
<!-- id: open-syntax -->

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
<!-- id: non-goals -->

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
