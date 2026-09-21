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

- **A file is a module.** `use "other.nz";` loads another file relative to this one;
  every top-level function in it becomes visible, because there is no visibility control
  yet.
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
| `Ints` | A growable sequence of `Int`, **by reference** and never freed |
| `Strs` | The same, of `Str` |
| `Chan` | A bounded queue of `Int`, by reference. The one type designed to be shared between tasks |

`Ints`, `Strs` and `Chan` are handles: two bindings of one sequence are the same storage,
and `==` is not offered on them, because it would have to mean either identity or
contents and the language has not chosen. A sequence may **not** be passed to a task; a
channel may, and its operations are the synchronisation.

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
| `program` | `{ use_item } , { function }` |
| `use_item` | `"use" , STRING , ";"` |
| `function` | `"fn" , IDENT , "(" , [ parameters ] , ")" , "->" , type , block` |
| `parameters` | `parameter , { "," , parameter }` |
| `parameter` | `IDENT , ":" , type` |
| `type` | `IDENT` |
| `block` | `"{" , { statement } , [ expression ] , "}"` |
| `statement` | `let_statement \| assignment \| while_statement \| return_statement \| break_statement \| continue_statement \| scope_statement \| spawn_statement \| expression_statement` |
| `let_statement` | `"let" , [ "mut" ] , IDENT , "=" , expression , ";"` |
| `assignment` | `IDENT , "=" , expression , ";"` |
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
| `primary` | `INTEGER \| STRING \| "true" \| "false" \| IDENT \| call \| if_expression \| "(" , expression , ")"` |
| `call` | `IDENT , "(" , [ arguments ] , ")"` |
| `arguments` | `expression , { "," , expression }` |
| `if_expression` | `"if" , expression , block , [ "else" , ( block \| if_expression ) ]` |

### `program`

```ebnf
program = { use_item } , { function } ;
```

```nazm
fn main() -> Int { 0 }
```

```nazm
use "other.nz"; fn main() -> Int { 0 }
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
function = "fn" , IDENT , "(" , [ parameters ] , ")" , "->" , type , block ;
```

```nazm
fn main() -> Int { 0 }
```

```nazm
fn add(a: Int, b: Int) -> Int { a + b } fn main() -> Int { add(1, 2) }
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
type = IDENT ;
```

A name, and only a name. Which names are types — `Int`, `Bool`, `Str`, `Ints`,
`Strs`, `Chan` — is a *checking* question, not a grammatical one: the parser accepts
any identifier here and the checker reports `N0305` for one that is not a type. A
grammar that listed them would be claiming the parser rejects `fn f(x: Widget)`,
which it does not — it parses it and then says what is wrong with it, which is how
one mistake stays one error.

```nazm
fn f(c: Chan) -> Int { 0 } fn main() -> Int { f(chan_new(1)) }
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
assignment = IDENT , "=" , expression , ";" ;
```

```nazm
fn main() -> Int { let mut a = 1; a = 2; a }
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

```nazm
fn main() -> Int { if 1 < 2 { 1 } else { 0 } }
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
primary = INTEGER | STRING | "true" | "false" | IDENT | call | if_expression | "(" , expression , ")" ;
```

```nazm
fn main() -> Int { (1 + 2) * 3 }
```

### `call`

```ebnf
call = IDENT , "(" , [ arguments ] , ")" ;
```

```nazm
fn main() -> Int { str_len("abc") }
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


<!-- generated from docs/grammar.ebnf: end -->

## Where to go next

| | |
| --- | --- |
| `docs/spec.md` | What each construct means, and the decisions behind it |
| `docs/grammar.ebnf` | The grammar this page's syntax section is generated from |
| `docs/diagnostics.md` | The machine interface: diagnostics, fixes, capabilities, exit statuses |
| `docs/bootstrap.md` | What the self-hosting claim does and does not establish |
