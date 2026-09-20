# The bootstrap compiler, sketched before the language is extended

**Status:** design record for Milestone B of `docs/NAZM_AUTONOMOUS_GOAL.md`. Written
first, on purpose: the point of this document is that the language features added next
are *derived from a compiler that has been designed*, not chosen from a wish list and
justified afterwards.

## What the compiler has to do

`nazmc` reads one or more `.nz` files and writes an LLVM IR text file, then hands it to
`clang`. That is the same pipeline `nazm build` already runs, so the target is known
exactly rather than imagined: the Rust implementation is the reference, the emitted IR is
the artefact, and `docs/spec.md` is the specification both must meet.

## The data structures it needs, concretely

The whole compiler is expressible with **`Int`, `Bool`, `Str`, and growable arrays of
`Int` and `Str`**. Nothing here needs generics, traits, closures, user-defined types,
pattern matching, or a shared-pointer graph. The technique is the one bootstrap compilers
have always used: *structures of parallel arrays, addressed by index*.

### Source text

```
source : Str            # the whole file, read once
```

Byte-addressed. A token is a half-open byte range `[start, end)`, which is also exactly
what `nazm-span::Span` already is, so positions stay consistent with the existing
diagnostics by construction rather than by translation.

### Tokens — five parallel `[Int]`

```
tok_kind  : [Int]   # a small integer tag
tok_start : [Int]   # byte offset
tok_end   : [Int]
tok_value : [Int]   # integer literals; unused otherwise
tok_text  : [Str]   # identifiers and string literals; "" otherwise
```

### Syntax — six parallel arrays, an arena

```
node_kind : [Int]
node_a    : [Int]   # child index, or -1
node_b    : [Int]
node_c    : [Int]
node_span_start : [Int]
node_span_end   : [Int]
node_text : [Str]   # names
```

A node is an index. Children are indices. A list of children is a linked chain through
`node_b`, or a contiguous run recorded as `(first, count)` — both are `Int`s. **There is
no pointer graph and nothing to free**, which removes the entire ownership question from
the compiler's own data structures.

### Symbols, types, diagnostics, output

```
sym_name : [Str]   sym_kind : [Int]   sym_type : [Int]   sym_scope : [Int]
diag_code : [Str]  diag_start : [Int]  diag_end : [Int]  diag_message : [Str]
out : [Str]        # emitted IR, joined once at the end
```

A scope is an index into `sym_scope`; lookup walks outward by index. A hash map is not
needed for a bootstrap compiler and is not proposed.

### Traversal is iterative, always

Parsing, checking, lowering and emission all walk a tree. **They do so with an explicit
work stack — a `[Int]` — and never by recursion.** This is the B5 decision, and it is
what lets self-hosting proceed while native recursion remains unsupported:

> A counter alone is not a native stack-safety guarantee.
> — `docs/NAZM_AUTONOMOUS_GOAL.md` §7 B5

The Rust implementation already does this where it matters (`report_cycles` walks the
call graph with an explicit stack for exactly this reason), so the technique is not new
to the project. It also removes the "no AST nesting limit bounds runtime call depth"
trap: with no recursion there is no call depth to bound.

## The language features this implies, and nothing more

| Needed for | Feature | Milestone |
| --- | --- | --- |
| reading source, names, literals, building output | `Str` with length, byte access, slicing, concatenation, comparison, and `Int` conversion | **B1** |
| every table above | growable `[Int]` and `[Str]` with `push`, indexed get/set, `len` | **B2** |
| reading input, writing the `.ll`, reporting, exit status | file read/write, arguments, stderr, exit codes | **B3** |
| a compiler in more than one file | modules with deterministic multi-file resolution | **B4** |
| every traversal | nothing — iterative algorithms use the `[Int]` stack from B2 | **B5** |

**Not needed, and therefore not being added:** generics, traits, closures, user-defined
records or tagged unions, pattern matching, a hash map, reference types, a borrow
checker, destructors, exceptions, iterators, operator overloading, a package registry.
Each may be a good idea later; none is a prerequisite for the first compiler, and the
contract forbids adding them to make a feature list longer.

## Memory model for the bootstrap

**Strings and arrays are immutable in identity and allocated from a process-lifetime
region that is never freed.** A compiler process reads input, produces output and exits;
reclaiming memory inside that lifetime buys nothing and costs an ownership system that
the bootstrap does not otherwise need.

This is a stated limitation, not an oversight, and it has consequences that must be
tested rather than assumed: peak memory grows with input size and with the number of
intermediate strings, so string building uses an array plus one join rather than repeated
concatenation. A real allocator with reclamation is future work, and the interface here
is chosen so that adding one does not change any program's meaning.

## What this design does *not* settle

- Concurrency (Milestone E) shares none of this and is designed separately.
- The public ABI and value layout stay unfrozen. `Str` and arrays are compiler-internal
  representations for now, and no program can observe their layout.
- Whether the self-hosted compiler eventually uses records and pattern matching is a
  question for after it exists. Writing it with arrays first is a claim about the
  shortest sound path, not about the language's eventual shape.
