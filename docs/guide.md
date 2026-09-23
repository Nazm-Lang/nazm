# The Nazm language guide

Nazm is small on purpose. This page is what a person — or an agent — needs to write a
program in it, and nothing more; `docs/spec.md` says what each construct *means* and why,
and is the document to read when two implementations could differ.

## A whole program

```nazm
fn max(a: Int, b: Int) -> Int {
    if a > b { a } else { b }
}

fn main() -> Int {
    let x = 17;
    let y = 25;
    max(x, y)
}
```

`nazm run` evaluates `main` and prints its value. `nazm build` compiles the same program
to a native executable that prints the same value. `nazm check` type-checks without
running anything, and `nazm test` runs a directory of programs against recorded outputs
— interpreted and compiled, requiring both to agree.

## The shape of the language, in six sentences

- **A file is a module.** `use "other.nz";` makes the definitions that file marked `pub`
  nameable here. Everything else in it stays private to it, and what *it* imports does not
  come with it.
- **`fn main() -> Int` is the entry point**, and every function writes its parameter and
  return types — there is no inference across a function boundary.
- **Bindings are immutable** unless written `let mut`, and a parameter is never mutable.
- **A block's last expression, with no `;`, is its value.** That is how `if` produces one.
- **Arithmetic is checked.** Overflow, division by zero and `Int::MIN / -1` fail with a
  diagnostic rather than wrapping or trapping silently; `Int::MIN % -1` is `0`.
- **Tasks are scoped.** `spawn f(args);` only appears inside `scope { … }`, and that block
  does not finish until every task started in it has.

## The types

| | |
| --- | --- |
| `Int` | A 64-bit signed integer. Every operation on one is checked |
| `Bool` | `true` or `false`. Not a number, and not convertible to one |
| `Str` | Immutable **bytes**, by value. Not text: `str_len` counts bytes, and an embedded zero is an ordinary byte |
| `Ints` | A growable sequence of `Int`, **by reference** |
| `Strs` | The same, of `Str` |
| `Chan` | A bounded queue of `Int`, by reference. The one type designed to be shared between tasks |
| a `struct` you declare | A **record**: a value with named fields, each keeping its own type's rules |
| an `enum` you declare | One of a fixed set of named **variants**, each with its own named fields |
| `Vec[T]` | A growable sequence of **any** type — records, enums, strings, other `Vec`s — by reference |

`Ints`, `Strs`, `Vec[T]` and `Chan` are handles: two bindings of one sequence are the same storage,
and `==` is not offered on them, because it would have to mean either identity or
contents and the language has not chosen. A sequence may **not** be passed to a task; a
channel may, and its operations are the synchronisation.

## Records

```nazm
pub struct Point {
    x: Int,
    label: Str,
}

fn main() -> Int {
    let mut p = Point(x: 1, label: "here");
    p.x = 41;
    p.x + str_len(p.label)
}
```

Six things to know, and then you know records:

- **Every field is named when you build one.** There is no positional form, no default
  value and no partial record. A field you forget is an error that names it.
- **A record is a value.** `let q = p;` gives you another record. Changing `q.x` does not
  change `p.x` — and if a field is an `Ints`, both records still point at the *same*
  sequence, because that is what an `Ints` is. Each field keeps its own rules; the record
  does not override them.
- **`p.x = 2` needs `let mut p`**, at any depth: `a.inner.count = 3` asks permission of
  `a`. Replacing part of what a binding holds is changing the binding.
- **Reading a field gives you a value you can keep.** `let s = make().label;` is valid
  after the record `make()` returned is gone.
- **Records are nominal.** Two records with the same fields are two types, and there is no
  conversion between them. A record and a function may share a name.
- **`a == b` is not offered**, for the same reason it is not offered on a sequence: it
  would have to decide whether an `Ints` field means the same storage or the same contents.

Field order is presentation. Moving two field declarations changes nothing at all — not
the type, not what constructs it, not the compiled program. Construction is by name.

Whether a record can be passed to a task is **worked out from its fields**: one made of
`Int`, `Str` and `Chan` can, one holding an `Ints` anywhere inside it cannot, and the error
tells you which field decided it.

A record cannot contain itself, directly or in a circle. That is refused as a layout with
no size rather than as a safety problem — there is nowhere for such a value to fit.

## Enums, and `match`

A record has all of its fields at once. An enum has **one of its variants** at a time.

```nazm
pub enum State {
    Ready,
    Done(code: Int),
    Failed(why: Str, code: Int),
}

fn describe(s: State) -> Int {
    match s {
        State.Ready() => 0,
        State.Done(code: c) => c,
        State.Failed(why: w, code: c) => str_len(w) + c,
    }
}

fn main() -> Int {
    describe(State.Failed(why: "disk", code: 5))
}
```

Six things to know, and then you know enums:

- **A variant is always written with its enum, and always with brackets.** `State.Ready()`,
  not `Ready` and not `State.Ready`. That is why two enums may both have a `Ready`, and why
  importing a module brings in no new bare names.
- **`match` is the only way to get at a payload.** There is no `s.code`: which fields a
  value has depends on which variant it is, so the question only has an answer inside an
  arm that has established one.
- **Every variant gets exactly one arm.** Leave one out and the compiler names it; write
  one twice and it says so. There is no `_ => …` arm — deliberately, so that when somebody
  adds a variant, every `match` that thought it had handled the whole enum is told.
- **A pattern names every payload field**, binding it or discarding it with `_`. Same
  reason: add a field and the old patterns stop compiling instead of quietly ignoring it.
  `_` only declines to *name* the value — it is still there and still cleaned up.
- **A binding is a borrowed view that lives in its arm.** You can return it, store it or
  pass it on, and it stays valid after the value you matched is gone. If it names an `Ints`,
  you can still push to the sequence: the binding is immutable, the storage is not.
- **`a == b` is not offered**, for the reason it is not offered on a record.

Variant order and payload-field order are presentation, exactly as a record's field order
is. Moving two of either changes nothing at all.

Whether an enum can be passed to a task is worked out from **every** variant, not the one
it happens to hold: `enum Work { None, Values(xs: Ints), }` may not cross even while it is
`None`, because what a task is given is a value of a type.

An enum and a record can contain one another, and an enum can contain an enum. What is
refused is a circle — the same layout-with-no-size refusal a record gets.

## Generics, and `Vec[T]`

A record, an enum or a function may take **type parameters**, written in square brackets
after its name, and a type is applied to arguments the same way.

```nazm
struct Pair[A, B] {
    first: A,
    second: B,
}

enum Maybe[T] {
    None,
    Some(value: T),
}

fn or_else[T](m: Maybe[T], fallback: T) -> T {
    match m {
        Maybe.None() => fallback,
        Maybe.Some(value: v) => v,
    }
}

fn main() -> Int {
    let pairs = vec_new[Pair[Int, Str]]();
    vec_push(pairs, Pair[Int, Str](first: 7, second: "nazm"));
    let p = vec_get(pairs, 0);
    p.first + or_else(Maybe[Int].Some(value: 35), 0)
}
```

Six things to know, and then you know generics:

- **A generic function is checked once, for every `T` at once.** So its body can only do
  what every type can: bind a `T`, pass it, return it, store it, push it into a `Vec[T]`.
  It cannot add two `T`s, compare them with `==`, read a field of one or pass one to a task
  — there is no way to say "only for types that can", so none of those is offered.
- **A call's type arguments come from its arguments, or you write them.** `or_else(m, 0)`
  works out `T` from `m`; `vec_new[Int]()` has no argument to work it out from, so it is
  written. What the result is *used for* never decides it.
- **Building a generic value names its arguments.** `Pair[Int, Str](first: …)` and
  `Maybe[Int].None()`, never `Pair(…)`. A **pattern** is the other way round —
  `Maybe.Some(value: v)` — because the value being matched already has one type.
- **Parameter order means something; parameter names do not.** `Pair[B, A]` is a different
  definition from `Pair[A, B]`, and renaming `T` to `Value` everywhere changes nothing at all.
- **`Vec[T]` is a sequence, like `Ints`.** `let b = a;` shares it, and `==` is not offered.
  `vec_get(xs, i)` gives you **your own copy** of an element; `vec_set(xs, i, v)` replaces
  one; `vec_push` appends and `vec_pop` removes the last. There is no `xs[i]` — to change a
  field of a record in a `Vec`, get it, change the copy, and set it back.
- **A type cannot own itself through a `Vec`.** `struct Node { children: Vec[Node], }` is
  refused: a value of it could hold itself, and nothing would ever reclaim it. `Vec[Vec[Int]]`
  and a record holding a `Vec[Str]` are fine — the refusal is only for a circle.

A `Vec` may not be passed to a task, whatever it holds, for the reason an `Ints` may not.

## Memory: what you do not have to do

**Nothing.** There is no `free`, no `new`, no ownership annotation and no lifetime to
write. Storage is reclaimed when the last reference to it dies — a binding going out of
scope, a value being replaced, a sequence being released along with everything in it — and
it happens on every way out of a function, including the one a runtime failure takes.

Four things follow, and they are worth knowing because they are what makes the rest
predictable:

- **`let b = a;` shares.** For `Ints`, `Strs` and `Chan` that is visible: a push through
  `b` is a push through `a`. For `Str` it is not visible at all — a `Str` is immutable, so
  whether the bytes were shared or copied is something no program can ask.
- **Passing a value to a function costs nothing.** The caller keeps it; the callee borrows
  it and cannot keep it after it returns. A value you pass in is still yours afterwards.
- **A slice does not copy.** `str_slice(s, 1, 3)` points into `s`'s bytes and keeps them
  alive for as long as the slice lives — so a function may build a string, slice it, and
  return the slice.
- **What you put into a `Strs` stays valid**, independently of the binding it came from,
  and what you read back out of one stays valid even if you then overwrite the element.

`str_concat` in a loop is still quadratic in **time**, and `str_join(parts, sep)` is one
pass and one allocation. That is now a performance note rather than the memory warning it
used to be.

## Finding out what a build supports

```
nazm capabilities
```

reports the types, the built-ins and the constructs this build has, read from the
compiler's own tables — and for each one, whether `nazm run` has it and whether
`nazm build` compiles it. `--json` gives the same thing under a versioned schema; see
`docs/diagnostics.md`.

## The syntax

<!-- generated from docs/grammar.ebnf: begin -->

Every rule below is checked against the parser by
`crates/nazm-syntax/tests/grammar.rs`, and every example is a program that parser
accepts.

| Rule | Definition |
| --- | --- |
| `program` | `{ use_item } , { function \| record \| enumeration }` |
| `use_item` | `"use" , STRING , ";"` |
| `function` | `[ visibility ] , "fn" , IDENT , [ type_parameters ] , "(" , [ parameters ] , ")" , "->" , type , block` |
| `type_parameters` | `"[" , IDENT , { "," , IDENT } , "]"` |
| `visibility` | `"pub"` |
| `record` | `[ visibility ] , "struct" , IDENT , [ type_parameters ] , "{" , [ fields ] , "}"` |
| `fields` | `field , { "," , field } , [ "," ]` |
| `field` | `IDENT , ":" , type` |
| `enumeration` | `[ visibility ] , "enum" , IDENT , [ type_parameters ] , "{" , [ variants ] , "}"` |
| `variants` | `variant , { "," , variant } , [ "," ]` |
| `variant` | `IDENT , [ "(" , payload , ")" ]` |
| `payload` | `field , { "," , field } , [ "," ]` |
| `parameters` | `parameter , { "," , parameter }` |
| `parameter` | `IDENT , ":" , type` |
| `type` | `IDENT , [ type_arguments ]` |
| `type_arguments` | `"[" , [ type , { "," , type } ] , "]"` |
| `block` | `"{" , { statement } , [ expression ] , "}"` |
| `statement` | `let_statement \| assignment \| while_statement \| return_statement \| break_statement \| continue_statement \| scope_statement \| spawn_statement \| expression_statement` |
| `let_statement` | `"let" , [ "mut" ] , IDENT , "=" , expression , ";"` |
| `assignment` | `place , "=" , expression , ";"` |
| `place` | `IDENT , { "." , IDENT }` |
| `while_statement` | `"while" , expression , block` |
| `return_statement` | `"return" , expression , ";"` |
| `break_statement` | `"break" , ";"` |
| `continue_statement` | `"continue" , ";"` |
| `scope_statement` | `"scope" , block` |
| `spawn_statement` | `"spawn" , IDENT , "(" , [ arguments ] , ")" , ";"` |
| `expression_statement` | `expression , ";"` |
| `expression` | `comparison` |
| `comparison` | `sum , { ( "==" \| "!=" \| "<" \| "<=" \| ">" \| ">=" ) , sum }` |
| `sum` | `product , { ( "+" \| "-" ) , product }` |
| `product` | `unary , { ( "*" \| "/" \| "%" ) , unary }` |
| `unary` | `[ "-" ] , primary` |
| `primary` | `atom , { "." , IDENT }` |
| `atom` | `INTEGER \| STRING \| "true" \| "false" \| IDENT \| call \| construction \| variant_construction \| if_expression \| match_expression \| "(" , expression , ")"` |
| `call` | `IDENT , [ type_arguments ] , "(" , [ arguments ] , ")"` |
| `construction` | `IDENT , [ type_arguments ] , "(" , initialisers , ")"` |
| `initialisers` | `initialiser , { "," , initialiser } , [ "," ]` |
| `initialiser` | `IDENT , ":" , expression` |
| `variant_construction` | `IDENT , [ type_arguments ] , "." , IDENT , "(" , [ initialisers ] , ")"` |
| `arguments` | `expression , { "," , expression }` |
| `if_expression` | `"if" , expression , block , [ "else" , ( block \| if_expression ) ]` |
| `match_expression` | `"match" , expression , "{" , [ match_arms ] , "}"` |
| `match_arms` | `match_arm , { "," , match_arm } , [ "," ]` |
| `match_arm` | `IDENT , "." , IDENT , "(" , [ pattern_fields ] , ")" , "=>" , expression` |
| `pattern_fields` | `pattern_field , { "," , pattern_field } , [ "," ]` |
| `pattern_field` | `IDENT , ":" , IDENT` |

### `program`

```ebnf
program = { use_item } , { function | record | enumeration } ;
```

```nazm
fn main() -> Int { 0 }
```

```nazm
use "other.nz"; fn main() -> Int { 0 }
```

```nazm
struct Point { x: Int, } fn main() -> Int { Point(x: 1).x }
```

```nazm
enum State { Ready, } fn main() -> Int { match State.Ready() { State.Ready() => 0, } }
```

```nazm
fn identity[T](value: T) -> T { value } fn main() -> Int { identity(7) }
```

### `use_item`

```ebnf
use_item = "use" , STRING , ";" ;
```

```nazm
use "lib/helpers.nz"; fn main() -> Int { 0 }
```

### `function`

```ebnf
function = [ visibility ] , "fn" , IDENT , [ type_parameters ] , "(" , [ parameters ] , ")" , "->" , type , block ;
```

```nazm
fn main() -> Int { 0 }
```

```nazm
fn add(a: Int, b: Int) -> Int { a + b } fn main() -> Int { add(1, 2) }
```

```nazm
pub fn add(a: Int, b: Int) -> Int { a + b } fn main() -> Int { add(1, 2) }
```

```nazm
fn first[A, B](a: A, b: B) -> A { a } fn main() -> Int { first(1, "x") }
```

### `type_parameters`

```ebnf
type_parameters = "[" , IDENT , { "," , IDENT } , "]" ;
```

A generic declaration's parameters: at least one name, no trailing comma. A parameter
is a **position** — renaming one changes nothing — and it is a type and nothing else.
Which names may be one (not a built-in type, not `Vec`, not twice) is the checker's
question. See `docs/spec.md`, *Generics*.

```nazm
struct Pair[A, B] { first: A, second: B, } fn main() -> Int { 0 }
```

```nazm
enum Maybe[T] { None, Some(value: T), } fn main() -> Int { 0 }
```

### `visibility`

```ebnf
visibility = "pub" ;
```

One bit, and one spelling for it. Absent means private to this module; `pub` exports
it to the modules that directly import this one. There is no `private` keyword,
because the default needs no word, and no package or friend visibility, because there
are no packages and no friends. See `docs/spec.md`.

```nazm
pub fn helper() -> Int { 1 } fn main() -> Int { helper() }
```

### `record`

```ebnf
record = [ visibility ] , "struct" , IDENT , [ type_parameters ] , "{" , [ fields ] , "}" ;
```

A record: a nominal type whose fields are named. `struct` is the keyword; *record* is
what the semantics call the category. One concept, one spelling in each place — see
`docs/spec.md`, *Records*.
Declaration order is written down and means nothing: construction names every field,
and reordering two fields changes neither the type, nor its interface fingerprint, nor
its native layout.

```nazm
struct Point { x: Int, y: Int, } fn main() -> Int { Point(x: 1, y: 2).x }
```

```nazm
pub struct User { name: Str, } fn main() -> Int { str_len(User(name: "a").name) }
```

```nazm
struct Box[T] { value: T, } fn main() -> Int { Box[Int](value: 1).value }
```

### `fields`

```ebnf
fields = field , { "," , field } , [ "," ] ;
```

```nazm
struct P { x: Int, y: Int, } fn main() -> Int { P(x: 1, y: 2).y }
```

### `field`

```ebnf
field = IDENT , ":" , type ;
```

```nazm
struct P { x: Int, } fn main() -> Int { P(x: 7).x }
```

### `enumeration`

```ebnf
enumeration = [ visibility ] , "enum" , IDENT , [ type_parameters ] , "{" , [ variants ] , "}" ;
```

An enum: a closed set of named variants, exactly one of which is active in any value.
`enum` is the keyword and *enum* is what the semantics call the category — unlike
`struct`/record there is no second word, because none was needed.
Variant order is written down and means nothing: the discriminant is derived from the
names, so reordering two variants changes neither the type, nor its interface
fingerprint, nor its native representation. See `docs/spec.md`, *Enums*.

```nazm
enum State { Ready, Done, } fn main() -> Int { match State.Ready() { State.Ready() => 1, State.Done() => 0, } }
```

```nazm
pub enum E { V(x: Int), } fn main() -> Int { match E.V(x: 1) { E.V(x: n) => n, } }
```

```nazm
enum Maybe[T] { None, Some(value: T), } fn main() -> Int { match Maybe[Int].Some(value: 1) { Maybe.None() => 0, Maybe.Some(value: v) => v, } }
```

### `variants`

```ebnf
variants = variant , { "," , variant } , [ "," ] ;
```

```nazm
enum E { A, B, } fn main() -> Int { match E.A() { E.A() => 1, E.B() => 0, } }
```

### `variant`

```ebnf
variant = IDENT , [ "(" , payload , ")" ] ;
```

A variant with no payload is written with **no parentheses**: there is no field list,
exactly as a record with no fields has none. Construction and patterns always write
them, because there the parentheses are what separates a variant from a projection.

```nazm
enum E { A, } fn main() -> Int { match E.A() { E.A() => 0, } }
```

```nazm
enum E { V(x: Int, y: Int), } fn main() -> Int { match E.V(x: 1, y: 2) { E.V(x: a, y: b) => a + b, } }
```

### `payload`

```ebnf
payload = field , { "," , field } , [ "," ] ;
```

```nazm
enum E { V(x: Int,), } fn main() -> Int { match E.V(x: 1) { E.V(x: n) => n, } }
```

### `parameters`

```ebnf
parameters = parameter , { "," , parameter } ;
```

```nazm
fn f(a: Int, b: Str) -> Int { str_len(b) + a } fn main() -> Int { f(1, "x") }
```

### `parameter`

```ebnf
parameter = IDENT , ":" , type ;
```

```nazm
fn f(n: Int) -> Int { n } fn main() -> Int { f(1) }
```

### `type`

```ebnf
type = IDENT , [ type_arguments ] ;
```

A name, applied to type arguments or not. Which names are types — `Int`, `Bool`,
`Str`, `Ints`, `Strs`, `Chan`, `Vec`, a declared record or enum, a type parameter in
scope — is a *checking* question, not a grammatical one: the parser accepts any
identifier here and the checker reports `N0305` for one that is not a type. A grammar
that listed them would be claiming the parser rejects `fn f(x: Widget)`, which it does
not — it parses it and then says what is wrong with it, which is how one mistake stays
one error. How many arguments a name takes is the checker's too (`N0351` – `N0353`).

```nazm
fn f(c: Chan) -> Int { 0 } fn main() -> Int { f(chan_new(1)) }
```

```nazm
fn f(xs: Vec[Vec[Int]]) -> Int { vec_len(xs) } fn main() -> Int { f(vec_new[Vec[Int]]()) }
```

### `type_arguments`

```ebnf
type_arguments = "[" , [ type , { "," , type } ] , "]" ;
```

Square brackets, and one spelling: there are no angle brackets and no indexing, so
`name[` can only begin type arguments. Empty brackets parse — `Vec[]` is a type written
with the wrong number of arguments, which is `N0353` rather than a syntax error.

```nazm
struct Pair[A, B] { a: A, b: B, } fn f(p: Pair[Int, Str]) -> Int { p.a } fn main() -> Int { f(Pair[Int, Str](a: 1, b: "x")) }
```

### `block`

```ebnf
block = "{" , { statement } , [ expression ] , "}" ;
```

```nazm
fn main() -> Int { let a = 1; a }
```

```nazm
fn main() -> Int { 1 }
```

### `statement`

```ebnf
statement = let_statement | assignment | while_statement | return_statement | break_statement | continue_statement | scope_statement | spawn_statement | expression_statement ;
```

```nazm
fn main() -> Int { let a = 1; a }
```

### `let_statement`

```ebnf
let_statement = "let" , [ "mut" ] , IDENT , "=" , expression , ";" ;
```

```nazm
fn main() -> Int { let a = 1; a }
```

```nazm
fn main() -> Int { let mut a = 1; a = 2; a }
```

### `assignment`

```ebnf
assignment = place , "=" , expression , ";" ;
```

The left-hand side is a **place**: a binding, then a path of fields through it.
Mutability is checked at the binding, so `p.x = 2` needs `let mut p` — see
`docs/spec.md`, *Binding mutability reaches every field*.

```nazm
fn main() -> Int { let mut a = 1; a = 2; a }
```

```nazm
struct P { x: Int, } fn main() -> Int { let mut p = P(x: 1); p.x = 2; p.x }
```

### `place`

```ebnf
place = IDENT , { "." , IDENT } ;
```

```nazm
struct I { n: Int, } struct O { i: I, } fn main() -> Int { let mut o = O(i: I(n: 1)); o.i.n = 2; o.i.n }
```

### `while_statement`

```ebnf
while_statement = "while" , expression , block ;
```

```nazm
fn main() -> Int { let mut i = 0; while i < 3 { i = i + 1; } i }
```

### `return_statement`

```ebnf
return_statement = "return" , expression , ";" ;
```

```nazm
fn f() -> Int { return 1; } fn main() -> Int { f() }
```

### `break_statement`

```ebnf
break_statement = "break" , ";" ;
```

```nazm
fn main() -> Int { let mut i = 0; while true { break; } i }
```

### `continue_statement`

```ebnf
continue_statement = "continue" , ";" ;
```

```nazm
fn main() -> Int { let mut i = 0; while i < 3 { i = i + 1; continue; } i }
```

### `scope_statement`

```ebnf
scope_statement = "scope" , block ;
```

```nazm
fn w(c: Chan) -> Int { 0 } fn main() -> Int { let c = chan_new(1); scope { spawn w(c); } 0 }
```

### `spawn_statement`

```ebnf
spawn_statement = "spawn" , IDENT , "(" , [ arguments ] , ")" , ";" ;
```

```nazm
fn w(c: Chan, n: Int) -> Int { 0 } fn main() -> Int { let c = chan_new(1); scope { spawn w(c, 1); } 0 }
```

### `expression_statement`

```ebnf
expression_statement = expression , ";" ;
```

```nazm
fn main() -> Int { print("hi"); 0 }
```

### `expression`

```ebnf
expression = comparison ;
```

```nazm
fn main() -> Int { 1 + 2 * 3 }
```

### `comparison`

```ebnf
comparison = sum , { ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) , sum } ;
```

Each operator gets an example. Listing six and demonstrating one would be a grammar
that is right about the alternatives and silent about five of them, which the
examples exist to prevent.

```nazm
fn main() -> Int { if 1 < 2 { 1 } else { 0 } }
```

```nazm
fn main() -> Int { if 1 == 1 { 1 } else { 0 } }
```

```nazm
fn main() -> Int { if 1 != 2 { 1 } else { 0 } }
```

```nazm
fn main() -> Int { if 1 <= 2 { 1 } else { 0 } }
```

```nazm
fn main() -> Int { if 2 > 1 { 1 } else { 0 } }
```

```nazm
fn main() -> Int { if 2 >= 2 { 1 } else { 0 } }
```

### `sum`

```ebnf
sum = product , { ( "+" | "-" ) , product } ;
```

```nazm
fn main() -> Int { 1 + 2 - 3 }
```

### `product`

```ebnf
product = unary , { ( "*" | "/" | "%" ) , unary } ;
```

```nazm
fn main() -> Int { 2 * 3 / 4 % 5 }
```

### `unary`

```ebnf
unary = [ "-" ] , primary ;
```

```nazm
fn main() -> Int { -1 }
```

### `primary`

```ebnf
primary = atom , { "." , IDENT } ;
```

```nazm
fn main() -> Int { (1 + 2) * 3 }
```

```nazm
fn main() -> Int { if true { 1 } else { 0 } }
```

```nazm
fn main() -> Int { if false { 1 } else { 0 } }
```

```nazm
fn main() -> Int { str_len("a name") }
```

```nazm
struct P { x: Int, } fn main() -> Int { P(x: 1).x }
```

### `atom`

```ebnf
atom = INTEGER | STRING | "true" | "false" | IDENT | call | construction | variant_construction | if_expression | match_expression | "(" , expression , ")" ;
```

```nazm
fn main() -> Int { 1 }
```

### `call`

```ebnf
call = IDENT , [ type_arguments ] , "(" , [ arguments ] , ")" ;
```

Type arguments are written where inference cannot find them from the arguments —
`vec_new[Int]()` — or where a reader wants them visible. The expected result type
never supplies one.

```nazm
fn main() -> Int { str_len("abc") }
```

```nazm
fn identity[T](v: T) -> T { v } fn main() -> Int { identity[Int](7) }
```

### `construction`

```ebnf
construction = IDENT , [ type_arguments ] , "(" , initialisers , ")" ;
```

Named, never positional, so field order is never an implicit API. Parenthesised rather
than `Point { x: 1 }` because `IDENT "{"` is exactly what an `if` whose condition is a
bare name looks like, and no amount of parser lookahead makes that a decision rather
than a guess. See `docs/spec.md`, *Why construction is parenthesised*.
Told apart from `call` by two tokens: `IDENT "(" IDENT ":"`. No argument expression can
begin with a name followed by a colon, so the two forms cannot be confused.

```nazm
struct P { x: Int, y: Int, } fn main() -> Int { P(x: 1, y: 2).x }
```

```nazm
struct Box[T] { value: T, } fn main() -> Int { Box[Int](value: 7).value }
```

### `initialisers`

```ebnf
initialisers = initialiser , { "," , initialiser } , [ "," ] ;
```

```nazm
struct P { x: Int, } fn main() -> Int { P(x: 1,).x }
```

### `initialiser`

```ebnf
initialiser = IDENT , ":" , expression ;
```

```nazm
struct P { x: Int, } fn main() -> Int { P(x: 1 + 1).x }
```

### `variant_construction`

```ebnf
variant_construction = IDENT , [ type_arguments ] , "." , IDENT , "(" , [ initialisers ] , ")" ;
```

Always qualified by the enum, and always parenthesised — including for a variant with
no payload, because `E.V` alone is exactly the shape of a field projection. Told apart
from one by three tokens, `IDENT "." IDENT "("`, which is a decision rather than a
guess: the language has no methods and no function values, so nothing else can put a
`(` after a projection. See `docs/spec.md`, *Variants are named by their enum*.

```nazm
enum E { A, } fn main() -> Int { match E.A() { E.A() => 0, } }
```

```nazm
enum E { V(x: Int), } fn main() -> Int { match E.V(x: 1 + 1) { E.V(x: n) => n, } }
```

```nazm
enum Maybe[T] { None, Some(value: T), } fn main() -> Int { match Maybe[Int].None() { Maybe.None() => 0, Maybe.Some(value: v) => v, } }
```

### `arguments`

```ebnf
arguments = expression , { "," , expression } ;
```

```nazm
fn main() -> Int { ints_set(ints_new(), 0, 1) }
```

### `if_expression`

```ebnf
if_expression = "if" , expression , block , [ "else" , ( block | if_expression ) ] ;
```

```nazm
fn main() -> Int { if true { 1 } else { 2 } }
```

```nazm
fn main() -> Int { let mut n = 0; if true { n = 1; } n }
```

### `match_expression`

```ebnf
match_expression = "match" , expression , "{" , [ match_arms ] , "}" ;
```

The one elimination form for an enum. An expression, so it composes wherever one may
appear, and its arms reuse the existing block and completion rules rather than
introducing a second set.
Every variant gets exactly one arm — there is no `_` arm, deliberately, so that adding
a variant breaks the matches that believed they had handled the whole enum. See
`docs/spec.md`, *Exhaustiveness*.

```nazm
enum E { A, B(x: Int), } fn main() -> Int { match E.A() { E.A() => 1, E.B(x: n) => n, } }
```

```nazm
enum E { A(text: Str), } fn main() -> Int { match E.A(text: "hi") { E.A(text: s) => { let n = str_len(s); n }, } }
```

### `match_arms`

```ebnf
match_arms = match_arm , { "," , match_arm } , [ "," ] ;
```

```nazm
enum E { A, B, } fn main() -> Int { match E.A() { E.A() => 1, E.B() => 0, } }
```

### `match_arm`

```ebnf
match_arm = IDENT , "." , IDENT , "(" , [ pattern_fields ] , ")" , "=>" , expression ;
```

A pattern names the enum and never its type arguments: the value being matched already
has one concrete type, and `Maybe.Some(value: v)` matches a `Maybe[Int]` and a
`Maybe[Str]` alike.

```nazm
enum E { V(x: Int), } fn main() -> Int { match E.V(x: 1) { E.V(x: n) => n, } }
```

### `pattern_fields`

```ebnf
pattern_fields = pattern_field , { "," , pattern_field } , [ "," ] ;
```

```nazm
enum E { V(x: Int, y: Int), } fn main() -> Int { match E.V(x: 1, y: 2) { E.V(x: a, y: _) => a, } }
```

### `pattern_field`

```ebnf
pattern_field = IDENT , ":" , IDENT ;
```

A payload field is bound to a name, or discarded with `_`. `_` declines to name the
value; it does not mean the field is absent and does not mean it is not released.

```nazm
enum E { V(x: Int), } fn main() -> Int { match E.V(x: 1) { E.V(x: _) => 0, } }
```


<!-- generated from docs/grammar.ebnf: end -->

## Where to go next

| | |
| --- | --- |
| `docs/spec.md` | What each construct means, and the decisions behind it |
| `docs/grammar.ebnf` | The grammar this page's syntax section is generated from |
| `docs/diagnostics.md` | The machine interface: diagnostics, fixes, capabilities, exit statuses |
| `docs/bootstrap.md` | What the self-hosting claim does and does not establish |
