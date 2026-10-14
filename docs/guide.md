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

`nazm build` has two code generators. The default, `--backend llvm`, writes LLVM IR as text and
has clang compile it; `--backend cranelift` compiles every function in process with Cranelift,
which built the compiler's own source at `-O0` in 0.74 s against LLVM's 1.21 s on the measured host
(`docs/performance.md`, N41). They produce programs that behave identically — the same output,
the same failures, the same memory report — and neither ever stands in for the other: a program
one cannot compile is refused, never quietly built by the other. `--target` names the target
triple: one of nine (`docs/support.md`), built to an executable where this host can link it and to
objects with `--objects` elsewhere; an unknown triple is refused rather than built for this one.

A native program can call C. `extern "C" fn c_add(a: Int, b: Int) -> Int = "c_add";` declares a
C function by its signature and its symbol, and `nazm build --link add.o` hands the object that
defines it to the linker. Only `Int` (`int64_t`) and `Bool` (`bool`) cross; every call needs a
`ForeignCap` in scope and is the effect `foreign`; and `nazm run` refuses such a program rather
than pretend to run C. See `docs/spec.md`, *Foreign functions*.

### Writing a program the way v1 prefers

Declare what each function does and hand it the authority it needs, rather than relying on a
caller's: `fn save(io: IoCap, path: Str, data: Str) -> Int ! { io } { write_file(path, data) }`.
A function that declares `! {}` is pure, and a profile such as `critical` or `cyber` refuses any
function that declares nothing. Reach for the standard library before writing helpers —
`use "@std/text";` gives `text_split`, `text_trim` and `text_parse_int`, which returns a `Result`
rather than failing. A program that grows past one directory becomes a package: a `nazm.toml`
beside `src/`, dependencies as `use "NAME:path.nz";`, and `nazm lock` before a `--locked` build.
`nazm build --debug` makes a program a debugger can stop in, `nazm build --timings` says where a
build's time went, and `nazm inspect` prints everything the compiler knows about a program.
`docs/limitations.md` lists what v1 does not do.

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
| `IoCap`, `OutCap`, `SpawnCap` | **Capabilities**: authority to reach the outside world, only its standard streams, and to start a task. Only `main` receives them; nothing constructs one, and an `IoCap` is accepted where an `OutCap` is expected. Four more — `ForeignCap`, `VouchCap`, `TimeCap`, `MmioCap` — authorise calling C, vouching for a string, reading the clock and device registers (`docs/spec.md`, *The capabilities*) |

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
  It cannot add two `T`s, read a field of one or pass one to a task. What a bound grants is
  the exception: `T: Equality` gives `==` (see *Equality*), and `T: Trait` gives that trait's
  methods (N78, `docs/spec.md` *Traits and methods*). Nothing can say "`T` may be passed to a
  task".
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
`: Equality` too. `Equality` is the one built-in requirement — a trait is the other kind — it adds nothing at run time —
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

Neither has methods: the prelude gives them no impls, and an impl for a generic type is
refused. `@std/option` and `@std/result` offer functions instead — `option_unwrap_or`,
`result_ok` and the like — and there is no `map` on either. Runtime failures such as an index
out of bounds are still runtime failures, not `Err` values.

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
declaring `! {}` — two different mistakes, two different errors. A function holds what
it uses, whether it declares a set or not: one that prints and is given nothing is refused, so
authority is threaded from `main`'s parameters to every function that needs it.

Hand a function less than everything: an `OutCap` authorises `print` and `eprint` and nothing
else, and an `IoCap` is accepted wherever one is expected —
`fn log(o: OutCap, n: Int) -> Int ! { io }`, called as `log(io, 42)`, cannot read or write a file. The checking
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
version, `nazm.core-ir/2`, promises and what it does not.

```
nazm mir prog.nz
```

goes one level down, to what `nazm build` compiles: each concrete function — every instance of a
generic one included — as MIR, a graph of basic blocks in which every copy, move and drop of a
string, sequence, channel, record or enum is written out, every `scope` is joined on every way
out, and every operation that can fail names the block its failure goes to. Each function comes
with its executable digest, which a comment, a renamed local or an effect annotation does not
change. `--spans` names each statement's source position and `--json` gives one line. The same
promise as Core IR's: a debugging view, `nazm.mir/1`, internal and unstable
(`docs/architecture.md` §7.42).

```
nazm lir --target wasm32-unknown-unknown prog.nz
```

goes one level further, to what both native backends read before they write any code: the
target's data layout, every record's and enum's byte layout on that target, and which types own
anything, and (since N105) every unit's instructions, the one lowering both backends translate —
`nazm.lir/2`, validated as every build validates it (`docs/architecture.md` §7.82, §7.106);
`nazm lir --ops` prints the instructions alone, and `nazm lir --run` runs them on LIR's interpreter.

## The syntax

<!-- generated from docs/grammar.ebnf: begin -->

Every rule below is checked against the parser by
`crates/nazm-syntax/tests/grammar.rs`, and every example is a program that parser
accepts.

| Rule | Definition |
| --- | --- |
| `program` | `{ use_item } , { { attribute } , ( function \| foreign_function \| record \| foreign_struct \| enumeration \| trait_item \| impl_item \| constant ) }` |
| `attribute` | `"@" , IDENT , [ "(" , [ attribute_arg , { "," , attribute_arg } , [ "," ] ] , ")" ]` |
| `attribute_arg` | `STRING \| IDENT , [ "=" , STRING \| "(" , [ attribute_arg , { "," , attribute_arg } , [ "," ] ] , ")" ]` |
| `constant` | `[ visibility ] , ( "const" \| "static" ) , IDENT , ":" , type , "=" , expression , ";"` |
| `trait_item` | `[ visibility ] , "trait" , IDENT , "{" , { trait_method } , "}"` |
| `trait_method` | `"fn" , IDENT , "(" , [ parameters ] , ")" , result , [ effect_set ] , ( ";" \| block )` |
| `impl_item` | `"impl" , [ type_parameters ] , type , IDENT , type , "{" , { function } , "}"` |
| `use_item` | `"use" , STRING , [ IDENT , IDENT ] , ";" \| visibility , "use" , STRING , [ "{" , [ reexport_name , { "," , reexport_name } , [ "," ] ] , "}" ] , ";"` |
| `reexport_name` | `IDENT , [ IDENT , IDENT ]` |
| `qualifier` | `IDENT , "::"` |
| `function` | `[ visibility ] , "fn" , IDENT , [ type_parameters ] , "(" , [ parameters ] , ")" , [ result ] , [ effect_set ] , { contract } , ( block \| expression_body )` |
| `expression_body` | `"=" , expression , [ ";" ]` |
| `result` | `( ":" \| "->" ) , type` |
| `contract` | `( "requires" \| "ensures" ) , expression` |
| `foreign_function` | `[ visibility ] , IDENT , STRING , "fn" , IDENT , "(" , [ parameters ] , ")" , result , "=" , STRING , ";"` |
| `effect_set` | `"!" , "{" , [ effect_name , { "," , effect_name } ] , "}"` |
| `effect_name` | `IDENT \| "spawn"` |
| `type_parameters` | `"[" , type_parameter , { "," , type_parameter } , "]"` |
| `type_parameter` | `IDENT , [ ":" , IDENT ] \| "effects" , IDENT` |
| `visibility` | `"pub"` |
| `record` | `[ visibility ] , "struct" , IDENT , [ type_parameters ] , "{" , [ fields ] , "}"` |
| `foreign_struct` | `[ visibility ] , IDENT , STRING , "struct" , IDENT , ( ";" \| "{" , [ fields ] , "}" )` |
| `fields` | `field , { "," , field } , [ "," ]` |
| `field` | `IDENT , ":" , type` |
| `enumeration` | `[ visibility ] , "enum" , IDENT , [ type_parameters ] , "{" , [ variants ] , "}"` |
| `variants` | `variant , { "," , variant } , [ "," ]` |
| `variant` | `IDENT , [ "(" , payload , ")" ]` |
| `payload` | `field , { "," , field } , [ "," ]` |
| `parameters` | `parameter , { "," , parameter }` |
| `parameter` | `IDENT , ":" , type` |
| `type` | `function_type \| array_type \| tuple_type \| [ qualifier ] , IDENT , [ type_arguments ]` |
| `tuple_type` | `"(" , type , "," , type , { "," , type } , [ "," ] , ")"` |
| `array_type` | `"[" , type , ";" , ( INTEGER \| IDENT ) , "]"` |
| `function_type` | `"fn" , "(" , [ type , { "," , type } ] , ")" , result , [ effect_set ]` |
| `type_arguments` | `"[" , [ type , { "," , type } ] , "]"` |
| `block` | `"{" , { statement } , [ expression ] , "}"` |
| `statement` | `let_statement \| assignment \| while_statement \| for_statement \| let_else_statement \| return_statement \| break_statement \| continue_statement \| scope_statement \| defer_statement \| spawn_statement \| expression_statement` |
| `let_statement` | `"let" , [ "mut" ] , IDENT , "=" , expression , ";"` |
| `let_else_statement` | `"let" , pattern , "=" , expression , [ "else" , block ] , ";"` |
| `assignment` | `place , ( "=" \| compound_operator ) , expression , ";"` |
| `compound_operator` | `"+=" \| "-=" \| "*=" \| "/=" \| "%=" \| "&=" \| "\|=" \| "^=" \| "<<=" \| ">>="` |
| `place` | `IDENT , { "." , ( IDENT \| INTEGER ) \| "[" , expression , "]" }` |
| `while_statement` | `"while" , expression , block` |
| `for_statement` | `"for" , ( IDENT \| tuple_pattern ) , "in" , ( expression , ".." , expression \| expression ) , block` |
| `return_statement` | `"return" , expression , ";"` |
| `break_statement` | `"break" , ";"` |
| `continue_statement` | `"continue" , ";"` |
| `scope_statement` | `"scope" , block` |
| `defer_statement` | `"defer" , block` |
| `spawn_statement` | `"spawn" , IDENT , "(" , [ arguments ] , ")" , ";"` |
| `expression_statement` | `expression , ";"` |
| `expression` | `disjunction` |
| `disjunction` | `conjunction , { "\|\|" , conjunction }` |
| `conjunction` | `comparison , { "&&" , comparison }` |
| `comparison` | `bit_or , { ( "==" \| "!=" \| "<" \| "<=" \| ">" \| ">=" ) , bit_or }` |
| `bit_or` | `bit_xor , { "\|" , bit_xor }` |
| `bit_xor` | `bit_and , { "^" , bit_and }` |
| `bit_and` | `shift , { "&" , shift }` |
| `shift` | `sum , { ( "<<" \| ">>" ) , sum }` |
| `sum` | `product , { ( "+" \| "-" ) , product }` |
| `product` | `unary , { ( "*" \| "/" \| "%" ) , unary }` |
| `unary` | `[ "-" \| "!" \| "~" ] , primary` |
| `primary` | `atom , { "." , ( IDENT , [ "(" , [ arguments ] , ")" ] \| INTEGER ) \| "?" \| "[" , expression , "]" }` |
| `atom` | `INTEGER \| FLOAT \| STRING \| FSTRING \| "true" \| "false" \| IDENT \| qualifier , IDENT \| call \| construction \| variant_construction \| if_expression \| if_let_expression \| match_expression \| closure \| array \| tuple \| "(" , expression , ")"` |
| `tuple` | `"(" , expression , "," , expression , { "," , expression } , [ "," ] , ")"` |
| `array` | `"[" , expression , ( { "," , expression } , [ "," ] \| ";" , expression ) , "]"` |
| `closure` | `"fn" , "(" , [ parameters ] , ")" , result , [ effect_set ] , block` |
| `call` | `[ qualifier ] , IDENT , [ type_arguments ] , "(" , [ arguments ] , ")"` |
| `construction` | `[ qualifier ] , IDENT , [ type_arguments ] , "(" , initialisers , ")"` |
| `initialisers` | `initialiser , { "," , initialiser } , [ "," ]` |
| `initialiser` | `IDENT , ":" , expression` |
| `variant_construction` | `[ qualifier ] , IDENT , [ type_arguments ] , "." , IDENT , "(" , [ initialisers ] , ")"` |
| `arguments` | `expression , { "," , expression }` |
| `if_expression` | `"if" , expression , block , [ "else" , ( block \| if_expression \| if_let_expression ) ]` |
| `if_let_expression` | `"if" , "let" , pattern , "=" , expression , block , [ "else" , ( block \| if_expression \| if_let_expression ) ]` |
| `match_expression` | `"match" , expression , "{" , [ match_arms ] , "}"` |
| `match_arms` | `match_arm , { "," , match_arm } , [ "," ]` |
| `match_arm` | `pattern , [ "if" , expression ] , "=>" , expression` |
| `pattern` | `pattern_one , { "\|" , pattern_one }` |
| `pattern_one` | `"_" \| IDENT \| [ "-" ] , INT \| STRING \| "true" \| "false" \| variant_pattern \| tuple_pattern` |
| `tuple_pattern` | `"(" , pattern , "," , pattern , { "," , pattern } , [ "," ] , ")"` |
| `variant_pattern` | `[ qualifier ] , IDENT , "." , IDENT , [ "(" , [ pattern_fields ] , ")" ] \| ( "Some" \| "Ok" \| "Err" ) , "(" , pattern , ")" \| "None"` |
| `pattern_fields` | `pattern_field , { "," , pattern_field } , [ "," ]` |
| `pattern_field` | `IDENT , ":" , pattern` |

### `program`

```ebnf
program = { use_item } , { { attribute } , ( function | foreign_function | record | foreign_struct | enumeration | trait_item | impl_item | constant ) } ;
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

```nazm
trait Show { fn show(self: Self) -> Str; } impl Show for Int { fn show(self: Int) -> Str { int_to_str(self) } } fn main() -> Int { str_len(7.show()) }
```

```nazm
const SIZE: Int = 4; fn main() -> Int { SIZE }
```

```nazm
@cfg(os = "linux") fn f() -> Int { 1 } @cfg(not(os = "linux")) fn f() -> Int { 0 } fn main() -> Int { f() }
```

### `attribute`

```ebnf
attribute = "@" , IDENT , [ "(" , [ attribute_arg , { "," , attribute_arg } , [ "," ] ] , ")" ] ;
```

An attribute (Gate 2): `@` and a name from a closed vocabulary — `cfg`, `test`, `bench`,
`fuzz`, `deprecated` — before a top-level declaration, never before a `use`. Which names
exist and what each takes is the checker's (`N0619`), not the grammar's. See
`docs/spec.md`, *Attributes* and *Conditional compilation*.

```nazm
@test fn checks() -> Int { 0 } fn main() -> Int { 0 }
```

```nazm
@cfg(all(os = "macos", any(arch = "aarch64", arch = "x86_64"))) fn f() -> Int { 0 } fn main() -> Int { 0 }
```

### `attribute_arg`

```ebnf
attribute_arg = STRING | IDENT , [ "=" , STRING | "(" , [ attribute_arg , { "," , attribute_arg } , [ "," ] ] , ")" ] ;
```

```nazm
@deprecated("use g") fn f() -> Int { 0 } fn main() -> Int { 0 }
```

```nazm
@test(fails) fn f() -> Int { 0 } fn main() -> Int { 0 }
```

### `constant`

```ebnf
constant = [ visibility ] , ( "const" | "static" ) , IDENT , ":" , type , "=" , expression , ";" ;
```

A constant (Gate 2): a value the checker computes from literals, other constants,
operators and numeric conversions. `const` is a word only here, before a name and a `:`,
so it remains an ordinary name everywhere else. See `docs/spec.md`, *Constants*.
A static (Gate 3) is written the same way with `static`: storage with one address, a
number or a `Bool` read-only, or an `Atomic` started by `atomic_new` of a constant. `static`
is a word only where `const` is. See `docs/spec.md`, *Statics*.
R1a (L8): a constant may be a record, enum value, tuple or array built of constant parts, and
`pub const` exports one; an importer under a name reads it as `m::NAME`. See `docs/spec.md`,
*Constant tables and exported constants*.

```nazm
const MASK: UInt8 = 0xF0; fn main() -> Int { Int(MASK) }
```

```nazm
static TICKS: Atomic = atomic_new(0); fn main() -> Int { atomic_add(TICKS, 1) }
```

```nazm
struct K { s: Str, n: Int, } pub const TABLE: [K; 2] = [K(s: "a", n: 1), K(s: "b", n: 2)]; fn main() -> Int { TABLE[1].n }
```

### `trait_item`

```ebnf
trait_item = [ visibility ] , "trait" , IDENT , "{" , { trait_method } , "}" ;
```

A trait (N78): a named set of methods a type may implement. Each method is a signature
ended by `;`, or since R1b a default body an impl that writes none gets, whose first
parameter is `self: Self`. A trait
is in the type namespace and is not a type: it is written as a type parameter's bound,
`[T: Show]`, and never as the type of a value. See `docs/spec.md`, *Traits and methods*.

```nazm
pub trait Show { fn show(self: Self) -> Str; fn shout(self: Self, n: Int) -> Str ! {}; } fn main() -> Int { 0 }
```

### `trait_method`

```ebnf
trait_method = "fn" , IDENT , "(" , [ parameters ] , ")" , result , [ effect_set ] , ( ";" | block ) ;
```

```nazm
trait Size { fn size(self: Self) -> Int; } fn main() -> Int { 0 }
```

```nazm
trait Size { fn size(self: Self) -> Int; fn big(self: Self) -> Bool { self.size() > 9 } } fn main() -> Int { 0 }
```

### `impl_item`

```ebnf
impl_item = "impl" , [ type_parameters ] , type , IDENT , type , "{" , { function } , "}" ;
```

`impl Trait for Type`: every method of the trait without a default body, once, with
`Self` written as the type. `for` is a word here and nowhere else. At most one impl of a
trait for a type, in the trait's module or the type's. Since R1b, `impl[T…] Trait for
D[T…]` implements it for every instance of a generic record or enum.

```nazm
trait Size { fn size(self: Self) -> Int; } struct P { x: Int, } impl Size for P { fn size(self: P) -> Int { 1 } } fn main() -> Int { 0 }
```

```nazm
trait Size { fn size(self: Self) -> Int; } struct W[T] { v: T, } impl[T] Size for W[T] { fn size(self: W[T]) -> Int { 1 } } fn main() -> Int { 0 }
```

### `use_item`

```ebnf
use_item = "use" , STRING , [ IDENT , IDENT ] , ";" | visibility , "use" , STRING , [ "{" , [ reexport_name , { "," , reexport_name } , [ "," ] ] , "}" ] , ";" ;
```

`as NAME` imports the module under a name (N49): its exports are written `NAME::item`
and are not brought into scope. `as` is a word here and nowhere else. See `docs/spec.md`,
*Qualified names*.
`pub use` re-exports (N103): every name the module offers, or the names in braces, each
optionally offered under another name with the word `as`. A re-export is also an ordinary
import, and is never under a name: `pub use "PATH" as m;` is refused. See `docs/spec.md`,
*`pub use` offers another module's definitions*.

```nazm
use "lib/helpers.nz"; fn main() -> Int { 0 }
```

```nazm
use "other.nz" as other; fn main() -> Int { 0 }
```

```nazm
pub use "shapes.nz"; fn main() -> Int { 0 }
```

```nazm
pub use "shapes.nz" { double, Sq as Square }; fn main() -> Int { 0 }
```

### `reexport_name`

```ebnf
reexport_name = IDENT , [ IDENT , IDENT ] ;
```

```nazm
pub use "shapes.nz" { Sq as Square }; fn main() -> Int { 0 }
```

### `qualifier`

```ebnf
qualifier = IDENT , "::" ;
```

`NAME ::` before the name of a definition: a function, a record, an enum. Nazm has no
module-level values, so a qualified name is never a variable.

```nazm
use "geometry.nz" as g; fn main() -> Int { g::area(g::Point(x: 1, y: 2)) }
```

```nazm
use "shapes.nz" as s; fn f(x: s::Shape) -> Int { match x { s::Shape.Dot() => 0, } } fn main() -> Int { 0 }
```

### `function`

```ebnf
function = [ visibility ] , "fn" , IDENT , [ type_parameters ] , "(" , [ parameters ] , ")" , [ result ] , [ effect_set ] , { contract } , ( block | expression_body ) ;
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

```nazm
fn add(a: Int, b: Int): Int { a + b } fn main(): Int { add(1, 2) }
```

```nazm
fn double(n: Int): Int = n * 2 fn add(a: Int, b: Int) = a + b fn main() -> Int { add(double(1), 2) }
```

### `expression_body`

```ebnf
expression_body = "=" , expression , [ ";" ] ;
```

An expression body (R1.5, D2): the function's value is the expression. Written with no result
type, the result type is the expression's — not for a `pub` function, a method, or one whose
body reaches itself (`N0634`).

### `result`

```ebnf
result = ( ":" | "->" ) , type ;
```

A result type is written after `:` (R1.5, D1), as a parameter's is; `->` is still read until
1.0. A function that writes none returns nothing (R1, 5b.2).

```nazm
fn add(a: Int, b: Int): Int { a + b } fn main() -> Int { add(1, 2) }
```

### `contract`

```ebnf
contract = ( "requires" | "ensures" ) , expression ;
```

A contract clause (N87): `requires` is checked as the function is entered, `ensures` as it
returns, on every path, with `result` the value returned. Both are IDENTs recognised only
here, between the signature and the body; every `requires` comes before every `ensures`. A
clause is a `Bool` of literals, names, operators, field reads and calls of pure functions.
See `docs/spec.md`, *Contracts*.

```nazm
fn half(n: Int) -> Int ! {} requires n >= 0 ensures result * 2 <= n { n / 2 } fn main() -> Int { half(8) }
```

### `foreign_function`

```ebnf
foreign_function = [ visibility ] , IDENT , STRING , "fn" , IDENT , "(" , [ parameters ] , ")" , result , "=" , STRING , ";" ;
```

A foreign function (N42): a C function this program calls, declared by its signature and
its C symbol, with no body. `extern` is not a keyword — it is recognised only where a
declaration starts, before a string — so it is an IDENT here, and the string before `fn`
is the ABI, which must be `"C"`. Only `Int` and `Bool` cross; no type parameters and no
written effect set: calling one is always the effect `foreign`, and needs a `ForeignCap`.
`nazm run` refuses a program that calls one; `nazm build --link FILE` links it. See
`docs/spec.md`, *Foreign functions*.

```nazm
extern "C" fn c_add(a: Int, b: Int) -> Int = "c_add"; fn main() -> Int { 0 }
```

```nazm
pub extern "C" fn is_even(n: Int) -> Bool = "c_is_even"; fn main() -> Int { 0 }
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
own namespace: `io`, `spawn` and `foreign` are the only effects, a name that is not one is refused,
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
type_parameter = IDENT , [ ":" , IDENT ] | "effects" , IDENT ;
```

`effects E` declares the function's effect parameter (N51): `effects` is a word only
here, before a name. At most one, only on a function that declares its effect set, and
never on a record or an enum (`N0388`); its name may then be written in the function's
effect sets. See `docs/spec.md`, *Effect parameters and captured authority*.
A parameter may require a capability of every type it stands for: one name after a
`:`, and no list: `Equality`, which gives the parameter `==`, or a trait (N78), which
gives it the trait's methods. Only a function's parameter may require one. `Equality` is
not a type and nothing declares it. See `docs/spec.md`, *A type parameter may require
equality* and *Traits and methods*.

```nazm
fn same[T: Equality](a: T, b: T) -> Bool { a == b } fn main() -> Int { if same(1, 1) { 1 } else { 0 } }
```

```nazm
fn apply[effects E](f: fn() -> Int ! { E }) -> Int ! { E } { f() } fn main() -> Int { apply(fn() -> Int { 1 }) }
```

### `visibility`

```ebnf
visibility = "pub" ;
```

One bit, and one spelling for it. Absent means private to this module; `pub` exports
it to the modules that directly import this one. There is no `private` keyword,
because the default needs no word, and no package or friend visibility: a package offers
what its modules export, and nothing sits between. See `docs/spec.md`.

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

### `foreign_struct`

```ebnf
foreign_struct = [ visibility ] , IDENT , STRING , "struct" , IDENT , ( ";" | "{" , [ fields ] , "}" ) ;
```

A struct C sees (N85): `extern "C" struct Name;` is an opaque handle — a C pointer only a
foreign function returns, never built, read or compared — and `extern "C" struct Name { … }`
a record laid out as C's struct, its fields in the order written, which here is meaning.
`extern` is an IDENT recognised before a string, as for a foreign function. See `docs/spec.md`,
*Foreign functions v3*.

```nazm
extern "C" struct Db; fn main() -> Int { 0 }
```

```nazm
extern "C" struct Point { x: Int, flag: Bool, y: Int } fn main() -> Int { Point(x: 1, flag: true, y: 2).y }
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
type = function_type | array_type | tuple_type | [ qualifier ] , IDENT , [ type_arguments ] ;
```

A name, applied to type arguments or not. Which names are types — `Int`, `Bool`,
`Str`, `Ints`, `Strs`, `Chan` (and `Chan[T]`, N53), the seven capabilities (`IoCap`, `OutCap`, `SpawnCap`, `ForeignCap`, `VouchCap`, `TimeCap`, `MmioCap`), `Vec`, a
declared record or enum, a type parameter in scope — is a *checking* question, not a grammatical one: the parser accepts any
identifier here and the checker reports `N0305` for one that is not a type. A grammar
that listed them would be claiming the parser rejects `fn f(x: Widget)`, which it does
not — it parses it and then says what is wrong with it, which is how one mistake stays
one error. How many arguments a name takes is the checker's too (`N0351` – `N0353`).

```nazm
fn f(c: Chan) -> Int { 0 } fn main() -> Int { f(chan_new(1)) }
```

```nazm
fn f(c: Chan[Str]) -> Bool { chan_send_of(c, "hi") } fn main() -> Int { if f(chan_new_of[Str](1)) { 0 } else { 1 } }
```

```nazm
fn f(xs: Vec[Vec[Int]]) -> Int { vec_len(xs) } fn main() -> Int { f(vec_new[Vec[Int]]()) }
```

```nazm
fn say(io: IoCap, s: Str) -> Int ! { io } { print(s) } fn main(io: IoCap) -> Int { say(io, "hi") }
```

```nazm
fn f(xs: [Int; 4]) -> Int { xs[0] } fn main() -> Int { f([1, 2, 3, 4]) }
```

### `tuple_type`

```ebnf
tuple_type = "(" , type , "," , type , { "," , type } , [ "," ] , ")" ;
```

R1a: a tuple type, two to eight elements; `(T)` is `T`, parenthesised, and `(T,)` or a ninth
element is refused (`N0101`). See `docs/spec.md`, *Tuples*.

```nazm
fn f(p: (Int, Str)) -> (Str, Int) { (p.1, p.0) } fn main() -> Int { f((1, "a")).1 }
```

### `array_type`

```ebnf
array_type = "[" , type , ";" , ( INTEGER | IDENT ) , "]" ;
```

A fixed-size array type (Gate 2): its element type and its length, an integer literal or a
constant's name. See `docs/spec.md`, *Arrays*.

```nazm
const N: Int = 3; fn f(xs: [UInt8; N]) -> Int { Int(xs[2]) } fn main() -> Int { f([UInt8(1), 2, 3]) }
```

### `function_type`

```ebnf
function_type = "fn" , "(" , [ type , { "," , type } ] , ")" , result , [ effect_set ] ;
```

A function type (N50): its parameters' types, its result's, and the effects a call
performs — none when unwritten. Structural: two are one type exactly when all three are
equal. See `docs/spec.md`, *Function values and closures*.

```nazm
fn twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) } fn main() -> Int { 0 }
```

```nazm
fn run(c: IoCap, f: fn(IoCap) -> Int ! { io }) -> Int ! { io } { f(c) } fn main() -> Int { 0 }
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
statement = let_statement | assignment | while_statement | for_statement | let_else_statement | return_statement | break_statement | continue_statement | scope_statement | defer_statement | spawn_statement | expression_statement ;
```

R1, 5b.19: a statement's closing ";" may be left out where a line break or its block's
"}" follows it; see spec.md, "else if, and a line break for ;".

```nazm
fn main() -> Int { let a = 1; a }
```

```nazm
fn main() -> Int { let mut n = 0; while n < 5 { n = n + 1; if n == 3 { break } } n }
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

### `let_else_statement`

```ebnf
let_else_statement = "let" , pattern , "=" , expression , [ "else" , block ] , ";" ;
```

R1a: `let P = e else { … };` — the `else` must leave; `P`'s names are in scope after it.
Without `else` (L5), `P` must match every value (`N0343`): `let (a, b) = pair;`.

```nazm
fn f(o: Option[Int]) -> Int { let Option.Some(value: x) = o else { return 0; }; x } fn main() -> Int { f(Option[Int].Some(value: 2)) }
```

```nazm
fn main() -> Int { let (a, _) = (1, "b"); a }
```

### `assignment`

```ebnf
assignment = place , ( "=" | compound_operator ) , expression , ";" ;
```

The left-hand side is a **place**: a binding, then a path of fields through it.
Mutability is checked at the binding, so `p.x = 2` needs `let mut p` — see
`docs/spec.md`, *Binding mutability reaches every field*.
R1, 5b.12: `place op= e` is `place = place op e`; an index in the place is a name or a number.

```nazm
fn main() -> Int { let mut a = 1; a = 2; a }
```

```nazm
struct P { x: Int, } fn main() -> Int { let mut p = P(x: 1); p.x = 2; p.x }
```

```nazm
fn main() -> Int { let mut n = 1; n += 2; n *= 3; n <<= 1; n }
```

### `compound_operator`

```ebnf
compound_operator = "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" ;
```

```nazm
fn main() -> Int { let mut n = 100; n -= 1; n /= 3; n %= 7; n &= 6; n |= 1; n ^= 2; n >>= 1; n }
```

### `place`

```ebnf
place = IDENT , { "." , ( IDENT | INTEGER ) | "[" , expression , "]" } ;
```

```nazm
struct I { n: Int, } struct O { i: I, } fn main() -> Int { let mut o = O(i: I(n: 1)); o.i.n = 2; o.i.n }
```

```nazm
fn main() -> Int { let mut g = [[0; 2]; 2]; g[1][0] = 5; g[1][0] }
```

```nazm
fn main() -> Int { let mut t = ((1, 2), 3); t.0.1 = 5; t.0.1 }
```

### `while_statement`

```ebnf
while_statement = "while" , expression , block ;
```

```nazm
fn main() -> Int { let mut i = 0; while i < 3 { i = i + 1; } i }
```

### `for_statement`

```ebnf
for_statement = "for" , ( IDENT | tuple_pattern ) , "in" , ( expression , ".." , expression | expression ) , block ;
```

R1a. `in` is a word only here; `..` only between a range's bounds. Each form means the
`while` loop `docs/spec.md` (*`for` loops*) gives for it.

```nazm
fn main() -> Int { let xs = ints_new(); ints_push(xs, 2); let mut t = 0; for x in xs { t = t + x; } for i in 0..3 { t = t + i; } t }
```

```nazm
fn main() -> Int { let ps = vec_new[(Int, Int)](); vec_push(ps, (2, 3)); let mut t = 0; for (a, b) in ps { t = t + a * b; } t }
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
fn w(c: Chan) -> Int { 0 } fn main(t: SpawnCap) -> Int { let c = chan_new(1); scope { spawn w(c); } 0 }
```

### `defer_statement`

```ebnf
defer_statement = "defer" , block ;
```

R1b: a block run when the enclosing block is left — at its end, or by `return`, `?`,
`break` or `continue` — the last registered first. `defer` is a word only here, before
`{`. Its body leaves nothing: no value, and no `return`, `?`, `break` or `continue` out of
it (`N0631`).

```nazm
fn main() -> Int { let mut n = 0; { defer { n = n + 1; } n = 10; } n }
```

### `spawn_statement`

```ebnf
spawn_statement = "spawn" , IDENT , "(" , [ arguments ] , ")" , ";" ;
```

```nazm
fn w(c: Chan, n: Int) -> Int { 0 } fn main(t: SpawnCap) -> Int { let c = chan_new(1); scope { spawn w(c, 1); } 0 }
```

### `expression_statement`

```ebnf
expression_statement = expression , ";" ;
```

```nazm
fn main(out: OutCap) -> Int { print("hi"); 0 }
```

### `expression`

```ebnf
expression = disjunction ;
```

```nazm
fn main() -> Int { 1 + 2 * 3 }
```

### `disjunction`

```ebnf
disjunction = conjunction , { "||" , conjunction } ;
```

`||` and `&&` (N49): `Bool` operands, left-associative, and short-circuit — the right
operand is evaluated only when the left does not decide. `||` binds loosest. Each is the
`if` it lowers to; see `docs/spec.md`, *Logical operators*.

```nazm
fn main() -> Int { if 1 < 2 || 2 < 1 { 1 } else { 0 } }
```

### `conjunction`

```ebnf
conjunction = comparison , { "&&" , comparison } ;
```

```nazm
fn main() -> Int { if 1 < 2 && 2 < 3 { 1 } else { 0 } }
```

### `comparison`

```ebnf
comparison = bit_or , { ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) , bit_or } ;
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

### `bit_or`

```ebnf
bit_or = bit_xor , { "|" , bit_xor } ;
```

The bit operators (Gate 2): two integers of one type, or for a shift an integer and an
`Int` amount. They bind tighter than comparison — `a & m == 0` is `(a & m) == 0`, not C's
reading — and looser than arithmetic. See `docs/general-purpose.md` §3.

```nazm
fn main() -> Int { 6 | 1 }
```

### `bit_xor`

```ebnf
bit_xor = bit_and , { "^" , bit_and } ;
```

```nazm
fn main() -> Int { 6 ^ 3 }
```

### `bit_and`

```ebnf
bit_and = shift , { "&" , shift } ;
```

```nazm
fn main() -> Int { 6 & 3 }
```

### `shift`

```ebnf
shift = sum , { ( "<<" | ">>" ) , sum } ;
```

```nazm
fn main() -> Int { 1 << 4 >> 2 }
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
unary = [ "-" | "!" | "~" ] , primary ;
```

`!` is logical negation, as tight as `-`: `!a && b` is `(!a) && b`. It is never the `!` of
an effect set, which follows a result type, where no expression is.
`~` is the bit complement of an integer (Gate 2).

```nazm
fn main() -> Int { -1 }
```

```nazm
fn main() -> Int { if !false { 1 } else { 0 } }
```

```nazm
fn main() -> Int { ~0 }
```

### `primary`

```ebnf
primary = atom , { "." , ( IDENT , [ "(" , [ arguments ] , ")" ] | INTEGER ) | "?" | "[" , expression , "]" } ;
```

`?` is a postfix beside projection, in any order and any number: `r?`, `make()?.x`,
`r??`. It binds tighter than negation, so `-r?` is `-(r?)`. Whether the operand is a
`Result` — and the core prelude's one — is the checker's question, not the grammar's.
See `docs/spec.md`, *Propagation*.
`.name(…)` after an operand is a method call (N78): `p.show()`, `make().size()`. With a
trait's name as the operand, `Show.show(p)`, the trait chooses the method and the first
argument is the receiver.
`[index]` after an operand reads an array's element (Gate 2): `xs[i]`, `grid[r][c]`,
`points[i].x`. After a name, a `[` followed by types and then `(` is type arguments instead,
as it always was: `vec_new[Int]()`; a method call on an element is written `(xs[i]).m()`.
`.0` after an operand is a tuple's element (R1a, L5); `t.0.1` is two of them.

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

```nazm
trait Size { fn size(self: Self) -> Int; } fn f[T: Size](x: T) -> Int { x.size() + Size.size(x) } fn main() -> Int { 0 }
```

```nazm
fn main() -> Int { let xs = [1, 2, 3]; xs[1] }
```

```nazm
fn main() -> Int { let t = ((1, 2), 3); t.0.1 + t.1 }
```

### `atom`

```ebnf
atom = INTEGER | FLOAT | STRING | FSTRING | "true" | "false" | IDENT | qualifier , IDENT | call | construction | variant_construction | if_expression | if_let_expression | match_expression | closure | array | tuple | "(" , expression , ")" ;
```

A `FLOAT` is digits, `.` and digits, and/or an exponent: `1.5`, `2e9` (Gate 2). An
`INTEGER` may be written `0x`, `0o` or `0b`, with `_` between digits.
A `STRING` is an ordinary literal, or since R1a a raw one — `r"…"`, `r#"…"#` — or a multi-line
one between `"""` lines (`docs/spec.md`, *Multi-line and raw strings*): one token class, as the
lexer reads them.
An `FSTRING` (R1a, L4) is `f"…"` or `f"""…"""` whose `{expr}` and `{expr:spec}` holes are
expressions (`docs/spec.md`, *String interpolation*).

```nazm
fn main() -> Int { 1 }
```

```nazm
fn main() -> Int { Int(2.5e1) + 0xff + 0b1_0 + 0o7 }
```

```nazm
fn main() -> Int { str_len(r#"say "hi""#) + str_len(r"C:\path") }
```

```nazm
fn main() -> Int { let n = 4; str_len(f"n = {n:>3}, half = {Float64(n) / 2.0:.1}") }
```

### `tuple`

```ebnf
tuple = "(" , expression , "," , expression , { "," , expression } , [ "," ] , ")" ;
```

R1a: a tuple, two to eight elements (`docs/spec.md`, *Tuples*).

```nazm
fn main() -> Int { let t = (1, "two", true); t.0 }
```

### `array`

```ebnf
array = "[" , expression , ( { "," , expression } , [ "," ] | ";" , expression ) , "]" ;
```

An array (Gate 2): its elements, or one value and a count.

```nazm
fn main() -> Int { let a = [1, 2, 3]; let b = [0; 4]; a[2] + b[3] }
```

### `closure`

```ebnf
closure = "fn" , "(" , [ parameters ] , ")" , result , [ effect_set ] , block ;
```

A closure (N50): a function written where a value is, its parameters and result in
full, its effects declared or none. Its body may name the bindings around it, each
copied in when the closure is made; a `let mut` binding is shared instead (R1b), unless
it can hold a function value (`N0386`). A capability may be (N51): the closure holds
it, and its type says what it does. One may be written in a generic function (R1b).

```nazm
fn main() -> Int { let k = 2; let f = fn(x: Int) -> Int { x * k }; f(3) }
```

### `call`

```ebnf
call = [ qualifier ] , IDENT , [ type_arguments ] , "(" , [ arguments ] , ")" ;
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
construction = [ qualifier ] , IDENT , [ type_arguments ] , "(" , initialisers , ")" ;
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
variant_construction = [ qualifier ] , IDENT , [ type_arguments ] , "." , IDENT , "(" , [ initialisers ] , ")" ;
```

Always qualified by the enum, and always parenthesised — including for a variant with
no payload, because `E.V` alone is exactly the shape of a field projection. Told apart
from one by three tokens, `IDENT "." IDENT "("`. Since N78 a method call has that shape
too: `x.m(…)` with an unlabelled argument is always a call, and `x.m()` is a call when the
checker finds `x` names a value rather than an enum — an enum keeps every meaning it had.
See `docs/spec.md`, *Variants are named by their enum* and *Traits and methods*.

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
if_expression = "if" , expression , block , [ "else" , ( block | if_expression | if_let_expression ) ] ;
```

R1, 5b.11: "else if" is "else { if … }".

```nazm
fn main() -> Int { if true { 1 } else { 2 } }
```

```nazm
fn main() -> Int { let mut n = 0; if true { n = 1; } n }
```

```nazm
fn main() -> Int { let n = 50; if n < 10 { 1 } else if n < 100 { 2 } else { 3 } }
```

### `if_let_expression`

```ebnf
if_let_expression = "if" , "let" , pattern , "=" , expression , block , [ "else" , ( block | if_expression | if_let_expression ) ] ;
```

R1a: `if let P = e { … } else { … }` means `match e { P => …, _ => … }`.

```nazm
fn main() -> Int { if let 3 = 3 { 1 } else { 0 } }
```

```nazm
fn main() -> Int { if let 3 = 4 { 1 } else if true { 2 } else { 0 } }
```

### `match_expression`

```ebnf
match_expression = "match" , expression , "{" , [ match_arms ] , "}" ;
```

The one elimination form for an enum. An expression, so it composes wherever one may
appear, and its arms reuse the existing block and completion rules rather than
introducing a second set.
Every value is matched by some arm and no arm is unreachable (`docs/spec.md`, *Patterns*).
Since R1a an arm's pattern may be `_`, a name, a literal or several alternatives; a `match`
written variant by variant still breaks when a variant is added, as *Exhaustiveness* says.

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
match_arm = pattern , [ "if" , expression ] , "=>" , expression ;
```

A pattern names the enum and never its type arguments: the value being matched already
has one concrete type, and `Maybe.Some(value: v)` matches a `Maybe[Int]` and a
`Maybe[Str]` alike.
R1, 5b.17: a guard after the pattern; the arm is chosen only where it is true.

```nazm
enum E { V(x: Int), } fn main() -> Int { match E.V(x: 1) { E.V(x: n) => n, } }
```

```nazm
fn main() -> Int { match 7 { n if n < 0 => 0, n if n % 2 == 1 => 1, _ => 2, } }
```

### `pattern`

```ebnf
pattern = pattern_one , { "|" , pattern_one } ;
```

R1a: a pattern is one alternative or several separated by `|`.

```nazm
fn main() -> Int { match 2 { 0 => 10, 1 | 2 => 20, n => n, } }
```

### `pattern_one`

```ebnf
pattern_one = "_" | IDENT | [ "-" ] , INT | STRING | "true" | "false" | variant_pattern | tuple_pattern ;
```

`_`, a name, a literal (an integer, perhaps negated, a string, `true` or `false`), or a
variant pattern.

```nazm
enum E { A, B(x: Int), } fn main() -> Int { match E.B(x: 3) { E.B(x: 3) => 1, E.B(x: _) => 2, _ => 0, } }
```

### `tuple_pattern`

```ebnf
tuple_pattern = "(" , pattern , "," , pattern , { "," , pattern } , [ "," ] , ")" ;
```

R1a: a tuple pattern, as many elements as the tuple; `(p)` is `p`, parenthesised.

```nazm
fn main() -> Int { match (1, true) { (0, _) => 0, (n, true) => n, (_, false) => 2, } }
```

### `variant_pattern`

```ebnf
variant_pattern = [ qualifier ] , IDENT , "." , IDENT , [ "(" , [ pattern_fields ] , ")" ] | ( "Some" | "Ok" | "Err" ) , "(" , pattern , ")" | "None" ;
```

R1, 5b.14: a variant with no payload needs no `()`; `Some(p)`, `Ok(p)`, `Err(p)` and `None` name the
prelude's variants.

```nazm
enum E { V(x: Int, y: Bool), } fn main() -> Int { match E.V(x: 1, y: true) { E.V(x: 1, y: _) => 1, E.V(x: n, y: _) => n, } }
```

```nazm
enum C { R, G, } fn main() -> Int { match C.G { C.R => 1, C.G => 2, } }
```

```nazm
fn main() -> Int { match Some(3) { Some(n) => n, None => 0, } }
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
pattern_field = IDENT , ":" , pattern ;
```

A payload field matches a pattern — a name binds it, `_` discards it. `_` declines to name
the value; it does not mean the field is absent and does not mean it is not released.

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
