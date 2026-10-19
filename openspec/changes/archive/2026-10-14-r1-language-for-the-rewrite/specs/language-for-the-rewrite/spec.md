## ADDED Requirements

### Requirement: A definition may refer to itself through an immutable indirection
A record or enum SHALL be accepted when every cycle through it in the ownership graph of definitions
consists of inline edges and edges through `Box` or `List`, with at least one of the latter
(`docs/user/spec.md`, *Generics* and *Cycles*). A cycle with an edge through a mutable handle SHALL stay
refused with `N0359`, and an all-inline cycle with `N0336`.

#### Scenario: an expression tree
- **WHEN** a program declares `enum Expr { Int(v: Int), Add(l: Box[Expr], r: Box[Expr]) }` and builds and evaluates a tree of it
- **THEN** it is accepted, and the interpreter and every native backend print the same result with every allocation reclaimed in the memory report

#### Scenario: a cycle through a vector stays refused
- **WHEN** a program declares `struct A { kids: Vec[B] }` and `struct B { up: Box[A] }`
- **THEN** it is refused with `N0359` and the path that closes the cycle

#### Scenario: a deep list is released without overflowing the stack
- **WHEN** a program builds a one-million-element recursive list of `Box` values and drops it
- **THEN** it ends normally and its memory report shows nothing live

### Requirement: Growing a string in a loop is linear
`s = str_concat(s, t)` on a `let mut` binding SHALL cost amortised time and allocation proportional
to the length of `t`, and SHALL NOT change any other value's bytes.

#### Scenario: a long string grown by appends
- **WHEN** a program appends one byte 10,000,000 times to a `let mut` string
- **THEN** it finishes within a time linear in the count, measured against 1,000,000 appends

#### Scenario: a shared value is never mutated
- **WHEN** `s` shares its bytes with a slice, a copy or a `Strs` element and `s = str_concat(s, t)` runs
- **THEN** the slice, the copy and the element are byte-for-byte unchanged

### Requirement: `for` is defined by the `while` loop it stands for
`for x in e { … }` over `Ints`, `Strs`, `Vec[T]`, `List[T]` and `Bytes`, and `for i in a..b { … }`,
SHALL behave exactly as the `while` loop `docs/user/spec.md` gives for each, in every implementation; a
program using `in` as a name elsewhere SHALL keep its meaning.

#### Scenario: pushing during a loop
- **WHEN** a loop over a `Vec` pushes an element on its first step
- **THEN** the loop visits that element too, as the corresponding `while` loop does

#### Scenario: `in` as a name
- **WHEN** a valid 1.0 program binds a variable named `in`
- **THEN** it is accepted and means what it meant

### Requirement: The library supplies what the toolchain needs
`@std/blake3` and `@std/sha256` SHALL produce the official test vectors' digests; `@std/toml` SHALL
parse and re-encode `nazm.toml` and `nazm.lock` files the toolchain writes; `file_walk`,
`os_stdout`/`os_stderr`, `os_spawn_in` and `os_lock`/`os_unlock` SHALL behave as `docs/user/spec.md`
states, under the interpreter and native builds.

#### Scenario: hashing
- **WHEN** `blake3` is given each input of the official BLAKE3 test vectors
- **THEN** it returns each published digest

#### Scenario: a child's environment
- **WHEN** `os_spawn_in` starts `env` in a directory with exactly `A=1`
- **THEN** the child prints `A=1` and nothing else, and its working directory is the one given

### Requirement: A heap limit ends a program instead of the machine
A native program built with `--heap-limit SIZE` SHALL end with a run-time failure, a fixed message
and status, when an allocation would take its live heap past `SIZE`; a program built without it SHALL
allocate exactly as before.

#### Scenario: a runaway allocation
- **WHEN** a program built with `--heap-limit 64MiB` allocates without bound
- **THEN** it ends with the heap-limit failure before its live heap passes 64 MiB
