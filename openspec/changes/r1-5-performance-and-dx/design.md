## Context

R1 closed and 0.10.0 was published on 2026-10-15. The copyright holder chose R1.5's goals on
2026-10-14 (`proposal.md`) and delegated the detail-level choices; on 2026-10-15 they also granted
the recommended choice for any decision that comes up while they are away. This file records those
choices before the code, as the governance asks. Each is a recommendation taken, not a reopening of
anything the copyright holder decided.

Nazm is pre-1.0: a change of meaning is allowed, and is listed in the next release notes and in
`stability.md`'s epoch history. Every syntax change keeps the old form readable until 1.0, and
`nazm fmt` rewrites old to new, unless the decision below says otherwise.

## Decisions

### Syntax (section 6, with 4.4)

**D1 — `:` for result types (6.1).** `fn fib(n: Int): Int { … }`; function types `fn(Int): Int`;
function expressions `fn(n: Int): Int { … }`. The effect clause follows the type as before:
`fn read(io: IoCap): Str ! {io} { … }`. `->` is still read everywhere it was and is removed at 1.0.
No ambiguity: after a parameter list's `)`, `:` can only begin a type. Until R2, `nazm fmt` keeps
whichever of the two is written: the tree's own sources (`compiler/`, `library/`, `examples/`) must
stay in the forms the Nazm-written compiler reads, and the test that holds them formatted would
otherwise fail on every one. From R2, when that compiler reads `:`, `nazm fmt` writes `:` and the
tree is rewritten in one commit.

**D2 — expression bodies (4.4).** `fn f(n: Int): Int = n * 2`, ended where a statement ends (a line
break or `;`). A private function may leave the result type out — `fn add(a: Int, b: Int) = a + b` —
and its type is its expression's, checked before any caller; a `pub` function writes it (its
signature is its interface), and so does a recursive one (`N0634`: "write the result type of a
recursive function"), because its body needs its type first. A block body with no result type keeps
its R1 meaning: it returns nothing.

**D3 — short closures (6.4).** `|a, b| a < b` and `|| work()`; a parameter may be annotated,
`|n: Int| n + 1`. Parameter types come from the type expected where the closure is written — a
function-typed parameter or a `let` with a type; where nothing is expected, an unannotated
parameter is refused (`N0635`: "write this parameter's type"). The result type is the body's. A
block body is `|x| { … }`. Captures and their rules are a function expression's. `||` at the start
of an expression begins a closure with no parameters; elsewhere it is still `or`.

**D4 — positional payloads and unprefixed variants (6.5).** A variant declared with named fields,
`Circle(r: Int)`, may be built and matched positionally in declaration order — `Circle(2)`,
`Circle(r)` — or by name as before, not mixed in one use. A variant name is usable without its
enum's prefix wherever exactly one enum in scope has a variant of that name; where two do, the
prefix is required (`N0636`, naming both). (Implemented 2026-10-15:) the parser keeps an unlabelled
`Name(args)` a call, so a function of a capitalised name — and the language service, which finds
calls by their syntax — is unchanged; the checker reads it as the construction only where no
function of the name is in scope, and Core IR and provenance follow what the checker recorded.

**D5 — map literals and `Map[K, V]` (6.3).** `{"a": 1, "b": 2}` builds a `Map[K, V]`: a `{` whose
first element is an expression followed by `:` is a map literal, since no block begins that way.
`{}` is an empty map where a `Map` is expected (`let m: Map[Str, Int] = {}`, an argument, a field);
elsewhere `{}` keeps its meaning as an empty block, and using such a value as a map is refused with
help that writes the type. `Map[K, V]` is the standard hash map (`@std/hashmap`'s `HashMap`, the
same type under a shorter name), so `m[k]`, `m[k] = v` and `m[k] += 1` work on it as in R1.

**D6 — `[…]` is a list (6.2).** `[1, 2, 3]` is a `List[Int]` wherever no fixed array is expected; a
fixed array is written only where its type says `[Int; 3]`. **This changes meaning**: a
`let a = [1, 2, 3]` that was an array (a value, copied on assignment) is now a list (shared, mutable
through any name). Listed as a breaking change for the next release; `nazm fix` offers the `[Int; 3]`
annotation where a program copies such a value and then writes either copy.

**D7 — `List[T]` and `use std.json` (6.6).** `List[T]` names the growable list, `Vec[T]`, and is the
name taught; `Vec`, `Ints` and `Strs` keep working. R1a's immutable linked list, which held the name
until now, becomes `Chain[T]` (`chain_*` built-ins); a use of the old name for it is refused with
help naming `Chain` (`N0637`), and `@std/toml` moves with it. A breaking change, listed. `use
std.json` means `use "@std/json"`: a dotted path of identifiers names a standard module, and a quoted
path is still a file. (Implemented 2026-10-15:) `List[T]` is a prelude name, as `Box` is — a
module's own type `List` keeps it — resolved to the `Vec[T]` it names, so messages keep showing
`Ints`, `Strs` and `Vec[T]` as before and never `List`: showing `List[Int]` consistently would
touch every message and test that names a vector, for no change of meaning. `Chain` replaces
`List` in every spelling a layer writes (messages, Core IR, MIR keys, symbols, the Nazm-written
pipeline's type spellings, derived code); the checker's Rust identifiers (`Type::List`,
`Intrinsic::List*`, `Shape::ListT`) keep their names, to keep the change out of the checker files
other work is editing. `N0637` is raised where resolution has already failed, so a module's own
`list_len` keeps its meaning. The interface schema's `{"list": T}` spelling is unchanged.

**D8 — printing in your own program (6.7).** Inside the program's own package — every module of the
root package being built or run, never a dependency or `@std` — a function may print without being
passed `out: OutCap`: printing there needs the output capability the program's `main` already holds
(option C). A library or package function still takes `out`, so what a dependency can touch stays in
its signature. Files, the network, processes, spawning, randomness and time stay declared in
`main` and passed, as before. `nazm inspect` and `nazm build` report it as the program's authority,
as R1 does.

### Libraries and lines (4.2, 4.3)

**D9 — the prelude imports common modules (4.2).** `@std/hash`, `@std/hashmap` (as `Map`) and
`@std/text` are imported into every module of a program; a module's own declaration of a name hides
the prelude's, as built-ins already are. `@std/json` and the rest are still imported by `use`.

**D10 — `top(n)` and `sort_by(key)` on maps (4.3).** `m.top(n)` is the `n` entries with the largest
values, ties by key ascending — the word-frequency program's ten lines in one call; `m.sort_by(f)` is
the entries ordered by `f(key, value)`. Both return a `List[(K, V)]`.

### Performance (sections 1–3, 4.1)

**D11 — `nazm run` compiles (2.1).** `nazm run` builds the program with Cranelift in process, runs
the executable and passes its exit status through; `nazm run --interpret` is the interpreter, which
stays the oracle every equivalence test compares against. A program Cranelift refuses is refused by
`run` with that diagnostic and help naming `--interpret`: neither path falls back silently. Tests
that mean the interpreter say `--interpret`. Settled in the code: the build is `--opt-level 2` and
uses the project's object store as `nazm build` does, prints nothing of its own (no path, authority
line or warnings), and the help names `--interpret` only for refusals a native build alone makes
(`N0101`, `N0357`, `N0358`); `--max-iterations` and `--max-depth` require `--interpret` rather
than being silently ignored, and a signal ends `run` with `128 +` its number.

**D12 — build caches (1.1).** The runtime is compiled once per target and profile into the user's
cache directory (`~/Library/Caches/nazm` on macOS, `$XDG_CACHE_HOME/nazm` elsewhere, or
`NAZM_CACHE_DIR`'s), keyed by its content digest, and linked from there; `@std` units' objects are
cached there too, by the same keys the project store uses. A project's own objects stay in its
`.nazm/`. Deleting the cache changes no program. *Taken while the copyright holder was away
(2026-10-15):* the runtime's text is specialised to the services a program reaches (N82), so the
cache holds one object per distinct runtime text, keyed by the existing `nazm.object/1` key over its
exact bytes, the compiler's identity and its resolved configuration — never the whole-runtime
archive `nazm runtime build` makes, whose thread-local failure state would make a cached build a
different executable from an uncached one; `NAZM_USER_CACHE_DIR` names the user cache on its own
(empty turns it off, and `.cargo/config.toml` turns it off for every cargo-started test, so no test
writes to a real user cache), and objects are copied out of the cache into the build's working
directory rather than linked in place, since an entry is a header followed by the object.

**D13 — word frequency (3.1–3.5).** A size-class allocator in the runtime for blocks up to 512 bytes
(per-thread free lists, falling back to `malloc`); a native string-hash built-in that computes
exactly the values `@std/hash` computes, so every map's order is unchanged; a `split`/`words`
iterator that yields slices without a list; the runtime's LLVM IR optimised together with the
program's (`-O2` builds); and `file_lines` reading in chunks rather than the whole file.

**D14 — dead-stripping (4.1).** Native builds link with `-dead_strip` (macOS) or `--gc-sections` with
function sections (ELF), so unused standard functions are left out. *Taken while the copyright
holder was away (2026-10-15):* executables only, on the hosted macOS, iOS, Linux and Android
targets; a static or shared library keeps every function for its C callers, and Windows, WebAssembly
and the boards link as before.

### Security and verification (sections 7, 8)

**D15 — release builds (7.1).** `nazm build --release` strips symbols and debug information, links
position-independent with stack protection, and on macOS signs ad hoc with the hardened runtime.
**D16 — signed, reproducible releases (7.2):** minisign, whose key is the copyright holder's to make
and hold; until then the release checklist builds twice and compares, and the signing step is named
as waiting for the key. **D17 — 8.2's eight-CPU profile** is calibrated like P4T4 and adopted only for
the workloads it measurably speeds up within a justified memory ceiling.

## Order

Syntax first, since every later example and test is written in it: D1, D2, D3, D4, D5 with D9, D6,
D7, D8; then `run` (D11), the caches (D12), word frequency (D13), D14 and D10; then security and
verification (D15–D17); the comparison re-run (5.1) last. One commit per decision, each with its spec
text, its tests, its mutants and its epoch where the checker changes; a full gate every 4–6 changes.

## Breaking changes for the next release

- **D6 — `[…]` is a list** (2026-10-15, semantic epoch 84). `[1, 2, 3]` where no `[T; n]` is
  expected is a `Vec[Int]`, shared by every name for it; before, it was a fixed array, a value
  copied on assignment. A program that copies such a value and writes either copy now sees the
  write through both names, and one that compares two such literals with `==` or passes one where
  a `[T; n]` parameter is declared through an untyped binding is refused. The fix is to write the
  array type where the literal is bound, `let a: [Int; 3] = [1, 2, 3]`. An empty `[]` with no type
  to take is refused (`N0618`); `let v: Vec[Int] = []` is the form. The compatibility corpus's
  `run/arrays.nz`, `refuse/constant-index-out-of-range.nz` and `refuse/array-of-strings.nz` were
  edited this way and re-pinned, their expectations unchanged.

## Risks

- **D6 and D7 change meaning.** Mitigated by the diagnostics and `nazm fix`, and listed in the next
  release notes; the compatibility corpus's pinned cases are updated with the change, never deleted.
- **D11 touches most tests.** The `--interpret` rewrite is mechanical and done in one commit, before
  the default changes.
- **The Nazm-written compiler lags** until R2, so `compiler/`, `library/` and the top-level examples
  keep the 0.10.0 forms for now.

## Breaking changes for the next release

- **D7 (2026-10-15): R1a's `List[T]` is `Chain[T]`.** `List[T]` now names the growable list,
  `Vec[T]`, and R1a's immutable sequence — stable since 0.10.0 — is `Chain[T]`, with built-ins
  `chain_from_vec`, `chain_empty`, `chain_len` and `chain_get` for `list_*`. A program using the
  old type gets `Vec[T]` where it wrote `List[T]` (a mismatch against a `chain_` value, `N0300`); a
  call of an old built-in is refused, `N0637`, naming its replacement. `@std/toml`'s `Toml` holds
  `Chain`s. Semantic epoch 82.
