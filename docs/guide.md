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
  return types — there is no inference across a function boundary. `main` may take
  capabilities, `fn main(io: IoCap) -> Int`, and nothing else.
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
| `IoCap`, `SpawnCap` | **Capabilities**: authority to reach the outside world, and to start a task. Only `main` receives them; nothing constructs one |

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
- **`a == b` compares fields**, when every field's type has equality — see *Equality*
  below. A record holding an `Ints` cannot be compared: that field would have to mean the
  same storage or the same contents.

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
- **`a == b` compares the variant and then its payload**, when every payload field of
  every variant has equality — see *Equality* below.

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
  It cannot add two `T`s, read a field of one or pass one to a task — there is no way to say
  "only for types that can", so none of those is offered. Comparing two `T`s with `==` is the
  one exception, and only when the function declares `T: Equality` (see *Equality*).
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

## Equality

`==` and `!=` take two values of **one** type. `Int`, `Bool` and `Str` compare as they always
have — a `Str` by its bytes. For your own types equality is worked out from what they
contain; there is nothing to declare:

- **A record** can be compared when every field's type can, and two records are equal when
  every field is. It compares values, never identity: two records built separately with
  equal fields are equal.
- **An enum** can be compared when every payload field of **every** variant can — decided
  for the whole type, not for the variant a value happens to hold. Different variants are
  unequal and their payloads are not looked at; the same variant compares its own payload
  and nothing else.
- **Two types of one shape** are still two types and cannot be compared.
- **A generic type** is answered after its arguments are filled in: `Box[Int]` compares,
  `Box[Vec[Int]]` does not. `Option[T]` and `Result[T, E]` are ordinary enums and follow the
  same rule.
- **Not comparable:** `Vec[T]`, `Ints`, `Strs` and `Chan`, anything that contains one, and an
  unconstrained `T` inside a generic function — `fn same[T](a: T, b: T) -> Bool { a == b }`
  is refused, because `T` might be any of those. Declaring the requirement is the way out.

A generic function that needs `==` on `T` **says so** where `T` is declared:

```nazm
fn same[T: Equality](a: T, b: T) -> Bool { a == b }
```

Inside it `T` can be compared, and so can `Box[T]` or `Option[T]`. Every caller must pass a
type that has equality — `same(1, 2)`, `same(Point(…), Point(…))` and `same(Box[Int](…), …)`
are fine, `same(v, v)` for a `Vec[Int]` is refused, and so is an `Option[Vec[Int]]` even when
it holds `None`. A generic caller can pass its own `T` on only if its `T` says
`: Equality` too. `Equality` is the only requirement there is, it adds nothing at run time —
the compiled `same` at `Int` is the same code as one written for `Int` — and it is not a way
to define your own `==`: what `==` means is still worked out from the type.

`docs/spec.md`, *Equality is derived* and *A type parameter may require equality*, have the
full rules.

## Errors as values: `Result`, `Option`, and `?`

Something that can fail says so in its return type. `Result[T, E]` is either a success
holding a `T` or a failure holding an `E` — a **typed error value**, which you return, pass
and store like any other value. There are no exceptions: nothing is thrown, nothing is
caught, and nothing leaves a function unless the function returns.

```nazm
struct ParseError {
    at: Int,
}

fn digit(s: Str, i: Int) -> Result[Int, ParseError] {
    let d = str_byte(s, i) - 48;
    if d < 0 {
        return Result[Int, ParseError].Err(error: ParseError(at: i));
    }
    if d > 9 {
        return Result[Int, ParseError].Err(error: ParseError(at: i));
    }
    Result[Int, ParseError].Ok(value: d)
}

fn parse(s: Str) -> Result[Int, ParseError] {
    let mut i = 0;
    let mut n = 0;
    while i < str_len(s) {
        n = n * 10 + digit(s, i)?;
        i = i + 1;
    }
    Result[Int, ParseError].Ok(value: n)
}

fn main() -> Int {
    match parse("4x2") {
        Result.Ok(value: n) => n,
        Result.Err(error: e) => 100 + e.at,
    }
}
```

That prints `101`. Three separate things are going on, and each is worth knowing on its own:

- **`Result` is an ordinary enum.** It and `Option` are declared by the **core prelude**,
  which every module sees without a `use`, so you build one as you build any generic enum —
  `Result[Int, ParseError].Ok(value: d)` — and you cannot declare your own type called
  `Result` or `Option`.
- **`match` handles one explicitly.** `main` above looks at both variants and decides what
  each means. That is always available, and it is how `main` handles errors: `main` still
  returns `Int`.
- **`?` passes a failure on.** `digit(s, i)?` is the digit when `digit` succeeded; when it
  failed, `parse` returns that same error at once, in its own `Result`. The rule is exact:
  `?` works on the core `Result` only, in a function that itself returns a `Result`, and
  the two **error types must be the same type** — `?` never converts one error into
  another. The success types may differ: `?` gives you the value, and only the error
  leaves.

When `?` leaves, it leaves the way `return` does: anything later in the same expression is
not evaluated, every `scope` in between is joined, and everything the function was holding
is released. The error value arrives intact.

**`Option[T]`** is `None` or `Some(value: T)`, for a value that may be absent — there is no
null. In this version `?` does not work on an `Option`; take it apart with `match`:

```nazm
fn first_or(xs: Vec[Int], fallback: Int) -> Int {
    let o = if vec_len(xs) > 0 { Option[Int].Some(value: vec_get(xs, 0)) } else { Option[Int].None() };
    match o {
        Option.None() => fallback,
        Option.Some(value: n) => n,
    }
}
```

There are no methods on either — no `unwrap`, no `map` — and runtime failures such as an
index out of bounds are still runtime failures, not `Err` values.

## Effects and capabilities

A function may say what it does — `! { io }` reaches the outside world, `! { spawn }` starts
tasks, `! {}` does neither — and the checker holds its body to that. Leaving the set out is
not the same as `! {}`: an undeclared function's effects are inferred, and to another module
it may do anything.

A function that declares a set must also **hold the authority** for what it does: an
`IoCap` to call `print`, `read_file` and the other outside-world built-ins, a `SpawnCap` to
`spawn`. Authority is a value. `main` is handed it, and passes it to what needs it:

```nazm
fn log(io: IoCap, n: Int) -> Int ! { io } {
    print(int_to_str(n));
    print(str_from_byte(10))
}

fn double(n: Int) -> Int ! {} { n * 2 }

fn main(io: IoCap) -> Int ! { io } {
    log(io, double(21));
    0
}
```

Declaring `! { io }` without holding an `IoCap` is refused, and so is holding one while
declaring `! {}` — two different mistakes, two different errors. A function that declares
nothing uses its caller's authority, which is how programs written without any of this keep
working; one that declares a set may call it only while holding what it needs. The checking
is static: a capability costs nothing at runtime, and it does not sandbox the program.
`docs/spec.md`, *Effects* and *Capabilities*, is the law.

## Where values come from

The checker records where each value's contents came from: the command line (`arg`,
`args_count`), a file (`read_file`, `file_exists`), `main`'s capabilities, or storage it does
not follow (anything read out of a sequence, a `Vec` or a channel). Computing over input keeps its
origin, and needs no effect:

```nazm
fn shout(s: Str) -> Str ! {} { str_concat(s, "!") }

fn main(io: IoCap) -> Int ! { io } {
    let text = read_file("in.txt");
    write_file(arg(0), shout(text));
    0
}
```

`text`, and `shout(text)`, are from a file; `shout` is still pure. A function that returns a
constant passes nothing on, whatever it is given.

One flow is refused: in a function that declares its effects, the path `write_file` writes to
may not come from a file's contents — nor from a sequence or channel — directly or through a
function that writes to a path it is handed. `write_file(read_file("manifest"), "x")` is
`N0372`, with the chain from the read to the write. A path from the command line, or one the
program built, is fine. This is explicit data flow only: a value is not tainted by the condition
that chose it. `docs/spec.md`, *Provenance*, is the law.

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

## Seeing what a program became

```
nazm core-ir prog.nz
```

prints the program's Core IR — what each function became after checking, and what both
`nazm run` and `nazm build` start from: every value with its type, every call naming the
definition or built-in it reaches, every `break` and `continue` naming the loop it leaves, and
each `?` written out as the `match` it means. Definitions and types are named by their durable
keys (`main.nz::fn helper`), so the output is the same from any checkout; `--no-spans` leaves
out source positions, and `--json` gives one line. A program that does not check prints
nothing but its diagnostics. It is a view for a person asking *what did this source become*,
not a syntax to write or a format to depend on: `docs/architecture.md` §7.41 says what its
version, `nazm.core-ir/1`, promises and what it does not.

## The syntax

<!-- generated from docs/grammar.ebnf: begin -->

Every rule below is checked against the parser by
`crates/nazm-syntax/tests/grammar.rs`, and every example is a program that parser
accepts.

| Rule | Definition |
| --- | --- |
| `program` | `{ use_item } , { function \| record \| enumeration }` |
| `use_item` | `"use" , STRING , ";"` |
| `function` | `[ visibility ] , "fn" , IDENT , [ type_parameters ] , "(" , [ parameters ] , ")" , "->" , type , [ effect_set ] , block` |
| `effect_set` | `"!" , "{" , [ effect_name , { "," , effect_name } ] , "}"` |
| `effect_name` | `IDENT \| "spawn"` |
| `type_parameters` | `"[" , type_parameter , { "," , type_parameter } , "]"` |
| `type_parameter` | `IDENT , [ ":" , IDENT ]` |
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
| `primary` | `atom , { "." , IDENT \| "?" }` |
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
function = [ visibility ] , "fn" , IDENT , [ type_parameters ] , "(" , [ parameters ] , ")" , "->" , type , [ effect_set ] , block ;
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

### `effect_set`

```ebnf
effect_set = "!" , "{" , [ effect_name , { "," , effect_name } ] , "}" ;
```

The effects a function declares it may exercise: after the return type, a `!` and a
set in braces. `! {}` is the empty set — the function is pure, and its body may call
nothing that is not. With no set written the checker infers one from the body, and
another module calling the function must assume every effect — so `fn f() -> Int` and
`fn f() -> Int ! {}` are not the same: only the second promises purity. A declared set
says what a function does, not what allows it: its body holds only the authority of
the capability values in its scope (`IoCap`, `SpawnCap`). The names live in their
own namespace: `io` and `spawn` are the only effects, a name that is not one is refused,
and so is one written twice. Order means nothing. See `docs/spec.md`, *Effects*.

```nazm
fn show(io: IoCap, s: Str) -> Int ! { io } { print(s) } fn main(io: IoCap) -> Int { show(io, "hi") }
```

```nazm
fn add(a: Int, b: Int) -> Int ! {} { a + b } fn main() -> Int { add(1, 2) }
```

```nazm
fn w(c: Chan) -> Int { 0 } fn run(t: SpawnCap) -> Int ! { spawn } { let c = chan_new(1); scope { spawn w(c); } 0 } fn main(t: SpawnCap) -> Int { run(t) }
```

### `effect_name`

```ebnf
effect_name = IDENT | "spawn" ;
```

`spawn` is a keyword, and the effect of starting a task is named by it.

```nazm
fn w(c: Chan) -> Int { 0 } fn run(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { let c = chan_new(1); scope { spawn w(c); } print("done") } fn main(io: IoCap, t: SpawnCap) -> Int { run(io, t) }
```

### `type_parameters`

```ebnf
type_parameters = "[" , type_parameter , { "," , type_parameter } , "]" ;
```

A generic declaration's parameters: at least one, no trailing comma. A parameter
is a **position** — renaming one changes nothing — and it is a type and nothing else.
Which names may be one (not a built-in type, not `Vec`, not twice) is the checker's
question. See `docs/spec.md`, *Generics*.

```nazm
struct Pair[A, B] { first: A, second: B, } fn main() -> Int { 0 }
```

```nazm
enum Maybe[T] { None, Some(value: T), } fn main() -> Int { 0 }
```

### `type_parameter`

```ebnf
type_parameter = IDENT , [ ":" , IDENT ] ;
```

A parameter may require a capability of every type it stands for: one name after a
`:`, and no list. `Equality` is the only capability, only a function's parameter may
require it, and inside the function the parameter then has `==`. The name is not a
type and nothing declares it. See `docs/spec.md`, *A type parameter may require
equality*.

```nazm
fn same[T: Equality](a: T, b: T) -> Bool { a == b } fn main() -> Int { if same(1, 1) { 1 } else { 0 } }
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
`Str`, `Ints`, `Strs`, `Chan`, the capabilities `IoCap` and `SpawnCap`, `Vec`, a
declared record or enum, a type parameter in scope — is a *checking* question, not a grammatical one: the parser accepts any
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

```nazm
fn say(io: IoCap, s: Str) -> Int ! { io } { print(s) } fn main(io: IoCap) -> Int { say(io, "hi") }
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
primary = atom , { "." , IDENT | "?" } ;
```

`?` is a postfix beside projection, in any order and any number: `r?`, `make()?.x`,
`r??`. It binds tighter than negation, so `-r?` is `-(r?)`. Whether the operand is a
`Result` — and the core prelude's one — is the checker's question, not the grammar's.
See `docs/spec.md`, *Propagation*.

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

```nazm
fn f(r: Result[Int, Str]) -> Result[Int, Str] { Result[Int, Str].Ok(value: r? + 1) } fn main() -> Int { 0 }
```

```nazm
struct P { x: Int, } fn f(r: Result[P, Str]) -> Result[Int, Str] { Result[Int, Str].Ok(value: -r?.x) } fn main() -> Int { 0 }
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
