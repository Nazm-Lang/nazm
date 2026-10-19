# The general-purpose foundation — Gate 2 decision record

*Gate 2 of the v1 programme, 2026-10-07, from the accepted baseline `b5f7506` (toolchain 1.0.0,
semantic epoch 30, `nazm.interface/11`, runtime ABI 14, standard library API 1.0). This file is the
design written before the code. Each mechanism below states the problem, the mechanism Nazm already
has, why that is not enough, what the extension costs the language, where it lives, and what will
count as evidence. [`capability-matrix.md`](capability-matrix.md) decides what exists; where this
record and the matrix disagree, the matrix is right and this record is history.*

## What Gate 2 starts from

A survey of the tree at `b5f7506`, recorded so that every decision below can be checked against it:

| Area | At `b5f7506` |
| --- | --- |
| Numbers | one `Int`, 64-bit two's complement, overflow traps (`N0400`); no floating point (`1.5` is `N0101`), no other widths, no bitwise operators (`&`, `\|`, `^`, `~` do not lex; `<<` is two `<`) |
| Storage | records and enums inline, `Vec[T]`, `Ints`, `Strs` and `Chan[T]` counted handles; no fixed-size aggregate, no non-owning view in the language (`str_slice` already shares storage, as a value) |
| Text | `Str` is bytes (*Strings are bytes*); string literals are UTF-8 because `\x` escapes stop at `0x7f`; nothing validates or decodes UTF-8 |
| Collections | `Vec`, `StrMap` (a sorted vector: O(n) insert), no hashing, no set, no deque |
| Outside world | whole-file `read_file` / `write_file` / `file_exists` that trap on failure, arguments, `print`/`eprint`, `exit_with`, a monotonic millisecond clock; no handles, seek, metadata, directories, rename, stdin, environment, child processes, signals, network or randomness |
| Concurrency runtime | M:N pool by default (N83, N106): stackful tasks pinned to workers, parking at four hooked waits; no reactor, so a blocking call holds its worker |
| Errors | `Result`/`Option`/`?` (N12), `Str` errors in the standard library, `c_errno()` behind `ForeignCap` |
| Metaprogramming | `nazm comptime` evaluates a pure nullary function on request; no constants, no attributes, no conditional compilation |
| Testing | `nazm test`: golden `.expected` files, interpreted and native; `@std/test` helpers |
| Packages | `nazm.package/1`, `nazm.lock/1`: exact or caret/tilde requirements, deterministic resolution, no features |
| C | `Int`/`Bool`/`Str`/handles/C structs across, exports of `Int`/`Bool`, `--lib`/`--shared`; no floats or other widths |

## The rule every mechanism below follows

**One semantic language.** Every mechanism is part of the one language and means the same in the
interpreter, under LLVM and under Cranelift; profiles may refuse it, and none reinterprets it.
**Prefer the mechanism that exists.** A new built-in function before new syntax, a library module
before a built-in, an existing handle discipline before a new one. **Authority is a capability**,
**what a function does is an effect**, and the two stay separate problems (`architecture.md` §5).
**Nothing silently changes meaning:** every program that checks at `b5f7506` checks and means the
same after Gate 2 — the compatibility corpus (`tests/compat/v1/`) is the proof, and is append-only.

---

## 1. Floating point — `Float64`, `Float32`

**Problem.** Numeric, graphics, simulation, statistics, signal and most scientific work is written
in binary floating point, and so is every C interface that carries a `double`. Without it Nazm is
excluded from those workloads outright.

**Existing mechanism considered.** `Int`, with fixed-point by hand. It is exact and portable, and it
is not what any library, file format or C API in those domains exchanges.

**Why extension is necessary.** Floating point cannot be built from `Int` at acceptable cost or
fidelity; it is a hardware type every target has.

**Semantics.**

| | |
| --- | --- |
| **Types** | `Float64` (IEEE 754 binary64) and `Float32` (binary32). No `Float` alias: one type, one name |
| **Literals** | `1.5`, `0.25`, `1e9`, `2.5e-3`, `6.02e23` — digits, a `.` and digits, and/or an exponent. A literal is a `Float64`, or a `Float32` where one is expected (*Numeric literals*, below). `1.` and `.5` are not literals |
| **Operators** | `+ - * /` correctly rounded, round-to-nearest-ties-to-even, never trapping: overflow gives an infinity, invalid operations a NaN. `%` is the remainder of truncated division (C's `fmod`). Unary `-` negates, including zero and NaN |
| **Comparison** | `< <= > >=` and `==`/`!=` are IEEE comparisons: every comparison with a NaN is false except `!=`, and `-0.0 == 0.0`. A record holding a float derives this equality — which is therefore not reflexive for a NaN. Floats have no `Hash` (§7) |
| **Conversions** | `Float64(i)` from any integer type rounds to nearest-even; `Float32(x)` from `Float64` rounds to nearest-even (overflow to infinity); `Float64(f32)` is exact. To an integer: `Int(x)`, `Int32(x)` and the rest truncate toward zero and **trap** (`N0400`) on a NaN, an infinity or a result out of range; `try_convert[T](x)` gives `None` there instead; `saturate_convert[T](x)` clamps, NaN to 0 |
| **Functions** | `float_sqrt`, `float_abs`, `float_floor`, `float_ceil`, `float_trunc`, `float_round` (half away from zero), `float_min`/`float_max` (a NaN loses), `float_is_nan`, `float_is_infinite`, `float_is_finite`, `float_bits`/`float_from_bits` (to and from `UInt64`/`UInt32`), `float_to_str`, `float_parse`; and from the C library, `float_sin`, `float_cos`, `float_tan`, `float_atan2`, `float_exp`, `float_ln`, `float_log2`, `float_log10`, `float_pow` |
| **Formatting** | `float_to_str` writes the shortest `%g`-style decimal that reads back to the same value (`1`, `0.1`, `1e+21`, `-0`, `inf`, `-inf`, `nan`), identically in every implementation |

**Conformance scope, stated rather than inherited.** The basic operations, `float_sqrt`, the
conversions, comparisons and `float_to_str`/`float_parse` are exactly specified by IEEE 754 and are
**bit-identical** in the interpreter, under LLVM and under Cranelift on every supported host. The
compiler never folds a float operation into anything a different rounding could produce, never
contracts `a * b + c` into a fused multiply-add, and never enables fast-math. **Not promised:** the
transcendental functions come from the host's C library and may differ in the last place between
platforms; a NaN's sign and payload are not observable through any operation except `float_bits`,
and are not promised; subnormals are IEEE subnormals (no flush to zero) on every supported host. No
floating-point exception flags or rounding modes are exposed.

**Constant folding.** Folding a float expression is permitted only for the basic operations and
uses the same IEEE operation, so a folded result is the run-time result bit for bit.

**FFI.** `Float64` crosses as `double` and `Float32` as `float`, in both directions and as fields of
C-layout structs.

**Semantic cost.** Two built-in types; a literal form that `N0101` refused since Gate 1 — no
program that checked changes meaning. Equality that is not reflexive for one value.

**Placement.** Language: types, literals, operators. Built-in functions for conversions and math.
Runtime: formatting and parsing.

**Evidence plan.** Differential tests (interpreter, LLVM `-O0`/`-O2`, Cranelift) over the special
values — NaN, ±infinity, ±0, the largest and smallest normal and subnormal values, rounding ties,
conversions at every integer boundary; a refusal per misuse; a compatibility run case; a numeric
reference application (§24).

## 2. Fixed-width integers

**Problem.** Binary formats, network protocols, hashing, device registers, C interfaces and dense
numeric storage are written in specific widths and signedness.

**Existing mechanism considered.** `Int` with manual masking. It cannot represent a `UInt64` above
`2^63 - 1`, carries no width in the type, and makes every protocol field a masking exercise.

**Semantics.**

| | |
| --- | --- |
| **Types** | `Int8`, `Int16`, `Int32`, `UInt8`, `UInt16`, `UInt32`, `UInt64`. `Int` remains the ordinary signed 64-bit type and the type of an unannotated integer literal; there is no `Int64` alias |
| **Arithmetic** | `+ - * / %` and unary `-` on two operands of **one** integer type, with `Int`'s rules at that width: overflow traps (`N0400`), division by zero traps (`N0401`), `MIN / -1` traps, `MIN % -1` is 0, unsigned `-x` traps unless `x` is 0. Mixed types are refused (`N0302`): nothing converts implicitly |
| **Explicit wrapping and saturation** | `wrapping_add`, `wrapping_sub`, `wrapping_mul`, `saturating_add`, `saturating_sub`, `saturating_mul`, generic over every integer type — the "explicit wrapping operator" *Integer overflow* promised, as functions rather than new operators |
| **Conversions** | `T(x)` from any integer or float type: exact or trap (`N0400`). `try_convert[T](x) -> Option[T]`, `wrap_convert[T](x)` (two's complement modulo `2^n`, integers only), `saturate_convert[T](x)` |
| **Literals** | Decimal, `0x`, `0o` and `0b` forms with `_` separators. An unannotated literal is an `Int`; where an integer type is expected it is that type, and a literal out of that type's range is refused at compile time (`N0003`, the code a literal too large for `Int` always had) |
| **Equality, order, hashing** | `==`, `!=`, `< <= > >=` within one type; every integer type has `Equality` and `Hash` |
| **FFI and layout** | `int8_t` … `uint64_t`, with the C size and alignment, in calls and in C-layout structs |
| **Formatting** | `num_to_str(x)` for every numeric type; `int_to_str` keeps its `Int` signature |

**Semantic cost.** Seven built-in types and an expected-type rule for literals. A literal's type was
always `Int`; it still is wherever nothing else is expected, so no program changes meaning.

**Placement.** Language and built-in functions. MIR/LIR already have `I8`/`I32` classes; `I16` is
added.

**Evidence plan.** Differential boundary tests at every width (overflow traps, wrapping, saturation,
conversions both ways, unsigned division, `UInt64` above `2^63`); literal range refusals; FFI round
trip through C at every width.

## 3. Bit operations

**Problem.** Flags, masks, hashing, compression, checksums, protocol fields, cryptography and device
registers need bitwise operations. The spec records "No bitwise operators" (N49) as a gap.

**Semantics.**

| | |
| --- | --- |
| **Operators** | `&`, `\|`, `^` (two operands of one integer type), prefix `~` (complement), `<<` and `>>`. On `Bool`, `&&`/`\|\|`/`!` stay the only operators |
| **Precedence** | Unary, then `* / %`, then `+ -`, then `<< >>`, then `&`, then `^`, then `\|`, then comparisons, then `&&`, then `\|\|` — so `a & MASK == 0` is `(a & MASK) == 0`, not C's `a & (MASK == 0)` |
| **Shifts** | The amount is an `Int`. `>>` is arithmetic for signed types and logical for unsigned. A shift by a negative amount or by the width or more **traps** (`N0412`, new — `N0408` was already the exhausted stack): no masking of the amount, no "implementation-defined" result. A constant amount out of range is refused when checked (`N0617`). `<<` discards the bits shifted out — it is a bit operation, not multiplication, and does not trap on them |
| **Bit functions** | `count_ones`, `leading_zeros`, `trailing_zeros` (returning `Int`), `rotate_left`, `rotate_right`, `swap_bytes`, and the endian conversions `to_big_endian`, `from_big_endian`, `to_little_endian`, `from_little_endian` — generic over the integer types |

**Semantic cost.** Five operators and one prefix operator; `<<`/`>>` lexed as tokens of their own
(no program contained `<<`, since `<` cannot follow `<` in any expression that checked).

**Evidence plan.** Differential tests over every width and both signednesses; shift traps; constant
shift refusals; precedence tests; checksum and hash code in the reference applications.

## 4. Fixed-size arrays — `[T; N]`

**Problem.** Deterministic, allocation-free storage of a known size: buffers, matrices, lookup
tables, protocol frames, embedded and real-time state.

**Existing mechanism considered.** `Vec[T]` — a counted, growable, heap-allocated, shared handle.
Its sharing and allocation are exactly what this storage must not have; a record with N fields does
not scale and cannot be indexed.

**Semantics.**

| | |
| --- | --- |
| **Type** | `[T; N]`, `N` an integer literal or a constant (§15) of type `Int`, `0 < N`, and the array at most 65,536 bytes; larger is refused (`N0618`, new) with the advice to use `Vec` or `Bytes` |
| **Construction** | `[a, b, c]` (every element of one type; `N` is the count) and `[x; N]` (N copies of `x`) |
| **Indexing** | `a[i]` with `i: Int`; out of range traps (`N0405`), as every index does. A constant index out of range is refused |
| **Mutation** | `a[i] = v` where `a` is a `let mut` binding or a field path of one, like a field assignment |
| **Ownership** | A value, held inline, like a record: assignment and passing copy it (parameters are borrowed, as for every value), and it owns exactly what its elements own |
| **Equality, hashing, task transfer** | Derived from `T`, like a record's |
| **Length** | `array_len(a)`, a constant |
| **Layout** | `N` elements of `T`'s layout, contiguous, in index order, with `T`'s alignment — so an array of C-compatible elements is C's array. Arrays do not cross the C boundary in Gate 2 |
| **Not** | A slice, a view or a growable buffer; there is no conversion to `Vec`. `fs[0](x)` — indexing then calling — is written `let f = fs[0]; f(x)`, because `name[...](` is a generic call |

**Semantic cost.** A type form, two literal forms, an index expression and an index assignment.
`name[...]` followed by `(` remains an explicit generic call, so no program changes meaning.

**Evidence plan.** Differential tests: construction, indexing, mutation, copy independence, nested
arrays, arrays of records and of strings with reclamation counts at `live=0`, bounds traps, size
refusals; MIR validation of the copies.

**As built (2026-10-08).** Narrower than designed above, and `spec.md`, *Arrays*, is the rule:

- **Plain data only.** The element is a number, `Bool`, or a record, enum or array of them
  (`N0621`, new), so an array owns nothing and is copied by its bytes. An array of strings would
  need per-element reference counting at every copy, drop and task crossing in three
  implementations; a collection of strings is a `Vec`, and nothing in the use cases above needs
  one inline. Widening the element class later is an addition.
- **No `array_len`.** The length is the type's `N`, written where the type is — a literal or a
  constant (§15) — so the constant is the length's name.
- **Calling an element** is moot (a function value is not plain data); a method call on an
  element is written `(xs[i]).m()`, since `name[T](` stays type arguments.
- The bound counts each element's scalars without padding (`data_bytes`), so `[UInt8; 65536]` is
  an array and `[UInt8; 65537]` is not.

## 5. Zero-copy views — `Bytes`, and `Str` slices as they already are

**Problem.** Parsing a packet, a file region or a text without copying it.

**Existing mechanism considered.** `str_slice` already returns a `Str` that shares the original's
storage and keeps it alive by its reference count (`Ty::Str` is `{ptr, len, owner}`): an immutable
zero-copy view whose validity the runtime owns, with no lifetime anywhere in the language. That is
the design, and it extends.

**The mechanism.** **`Bytes`, a counted handle to a fixed-length mutable byte buffer, whose slices
are views of the same storage.**

| | |
| --- | --- |
| **Creating** | `bytes_new(n)` (zeroed), `bytes_from_str(s)` (a copy) |
| **Reading and writing** | `bytes_len`, `bytes_get(b, i) -> UInt8`, `bytes_set(b, i, v)`, and fixed-width loads and stores in either byte order (`bytes_get_u16_le`, … `bytes_set_u64_be`, and the signed and float forms) |
| **Views** | `bytes_slice(b, lo, hi) -> Bytes` shares `b`'s storage: a write through either is seen through both, exactly as two references to one `Vec` see each other. Bounds are the view's own |
| **Converting** | `bytes_to_str(b) -> Str` (a copy, so a `Str` stays immutable), `bytes_copy(dst, src)` |

**Why this is sound without lifetimes.** A view holds a counted reference to the storage, so the
storage lives while any view does: nothing can dangle. A `Bytes` never changes length, so a view's
bounds stay inside the storage for as long as the view exists: nothing can be invalidated by growth.
Mutation through one view being visible through another is the language's existing meaning of a
counted handle (*Sequences are shared*), not a new aliasing rule. A `Bytes` holds no values, so it
closes no ownership cycle (*Cycles*).

**What is not offered, and why.** A view of a fixed-size array or of a record field: an array is an
inline value, and a non-owning reference to an inline value needs exactly the lifetime reasoning the
constitution excludes. Data meant to be viewed lives in a `Bytes` or a `Str`.

**Evidence plan.** Differential tests over views sharing writes, out-of-range traps through views,
views outliving their source binding, reclamation counted (`bytes` joins the memory report), task
transfer refused (a `Bytes` is mutable shared state and does not cross, like `Vec`).

**As built (2026-10-08).** As designed, with three settlements; `spec.md`, *Bytes*, is the rule.
The fixed-width loads and stores are four generic built-ins — `bytes_read_le[T]`,
`bytes_read_be[T]`, `bytes_write_le`, `bytes_write_be`, over every numeric type — rather than
thirty-two named ones: one rule, the class mechanism the numeric built-ins already use.
`bytes_copy` moves, so two views of one buffer may overlap. A `Bytes` is `Str`'s triple over a
buffer the program allocated, so the memory report counts buffers with the strings rather than in
a class of their own, in both implementations alike.

## 6. Bytes and text — `Text`

**Problem.** Applications exchange Unicode text, and need to know whether a byte string is valid
UTF-8 and to iterate it by scalar value.

**Existing mechanism considered.** `Str`, which is bytes by a 1.0 commitment (*Strings are bytes*)
that Gate 1 froze. Changing what `Str` means is a 2.0 change.

**The mechanism.** Keep `Str` as bytes; add **`Text`**, a built-in type whose values are always
valid UTF-8, with `Str`'s representation, so conversion to `Str` is free and from `Str` is a
validation, never a copy.

| | |
| --- | --- |
| **Creating** | `text_from_str(s) -> Option[Text]` (validates: shortest form, no surrogates, at most U+10FFFF); a string literal where a `Text` is expected (literals are UTF-8 by construction) |
| **Using** | `text_as_str(t) -> Str` (free), `text_len_bytes`, `text_concat`, `text_slice(t, lo, hi) -> Option[Text]` (`None` unless both ends are scalar boundaries), `text_eq` via `==`, ordering by `text_compare` (scalar order = byte order for UTF-8) |
| **Scalars** | `text_scalar_at(t, i) -> Int` decodes the scalar starting at byte `i`, and `text_next(t, i) -> Int` is the byte index after it; iteration is a `while` over byte indices. `text_scalar_count`, `text_from_scalar(c) -> Option[Text]` |
| **Not** | Graphemes, normalisation, case mapping beyond ASCII, collation and locale — ecosystem |

**Evidence plan.** The UTF-8 validation table (overlongs, surrogates, out-of-range, truncated
sequences) agreeing in every implementation; scalar iteration over multi-byte text; a JSON pipeline
that validates its input (§24).

**As built (2026-10-08).** A built-in cannot return an `Option` — it lives in the prelude, as
source — so the validating conversion is the conversion mechanism §1 already has: `Text(s)` (`s` or
`N0413`, as `UInt8(300)` is `N0400`) and `try_convert[Text](s) -> Option[Text]`, beside
`utf8_valid`. `text_from_str` is the conversion's internal name, not callable. `text_slice` and
`scalar_text` stop on a bad boundary or value (`N0413`) and are paired with `text_is_boundary` and
`scalar_valid`, rather than returning `Option`. A literal is a `Text` where one is expected — a
parameter, an operand beside a `Text`, a conversion's argument — as a numeric literal takes the
type expected. Ordering is `@std/text`'s `text_compare` over `text_as_str`, since `text_compare`
was already that module's name and byte order is scalar order; `text_from_scalar` is
`scalar_text`.

## 7. Collections — hashing, `HashMap`, `HashSet`, `Deque`

**Problem.** A practical map with expected constant-time operations, a set, and a queue.

**Existing mechanism considered.** `Vec` and `StrMap` (sorted vector, `Str` keys, O(n) insert). The
library can build any structure on `Vec`; what it cannot build is a hash, because nothing in the
language turns an arbitrary key into an integer.

**The mechanism.** A second built-in requirement, **`Hash`**, alongside `Equality`: `[K: Hash]`
means `K` has equality and a hash. It is derived exactly as equality is (*Equality is derived*):
every integer type, `Bool`, `Str`, `Text`, and records, enums and arrays whose components all have
it. Floats, handles, function values and type parameters without the requirement do not. The
built-in `hash(x) -> UInt64` is defined for every such type. Then, in the standard library, written
in Nazm:

| Module | Type | Complexity |
| --- | --- | --- |
| `@std/hashmap` | `HashMap[K, V]`, open addressing with linear probing | insert, get, remove: expected O(1); growth amortised O(1) |
| `@std/hashset` | `HashSet[K]` over `HashMap[K, Bool]` | as above |
| `@std/deque` | `Deque[T]`, a ring buffer over `Vec` | push and pop at either end amortised O(1), index O(1) |

**Determinism and hostile keys.** `hash` is a fixed function (64-bit FNV-1a over bytes, mixed by the
SplitMix64 finaliser), the same in every implementation and run, so iteration order is reproducible.
A map facing keys chosen by an adversary takes a seed — `hashmap_with_seed(seed)` — and a seed from
`@std/random` (§21) makes it unpredictable. Reproducibility is the default and unpredictability is
explicit, the rule §21 states for randomness.

**Evidence plan.** Library tests (insert/remove/grow/collide), the `Hash` derivation and refusal
tests, `hash` agreeing in every implementation, and the KV application (§24).

**As built (2026-10-09).** `Hash` is an ordinary trait in `@std/hash`, not a built-in
requirement, and there is no built-in `hash`: the traits of N78 already give a bound, static
dispatch and coherence, so a second derivation mechanism would duplicate them. The library
implements it for every integer type, `Bool` and `Str`, with two methods — `hash` and `same` (the
equality a map compares with); a program implements it for its own key types, composing the
library's (`hash_combine(self.x.hash(), self.y.hash())`). What the language changed is one rule:
an impl may be for any numeric type, as it was for `Int` (`N0604`, epoch 35). Records, enums and
arrays are keys by their impl, not by derivation; floats have none. `spec.md`, *Standard library*,
is the rule.

## 8. Filesystem

**Problem.** Real programs open files, stream them, seek, inspect metadata, walk directories,
rename, delete, and replace files atomically — and handle failure as a value.

**Existing mechanism considered.** Three whole-file built-ins that trap on failure, and `@std/fs`
wrapping them in `Result[_, Str]`.

**The mechanism.** One new counted handle, **`OsHandle`**, owning an operating-system descriptor:
it is closed when its last reference goes, or earlier by `os_close`, after which every operation on
it fails with a "closed" error and never touches a reused descriptor. Built-in operations return
their outcome as a status (a count, or a negative error code), and the standard library turns that
into `Result[T, IoError]`. `IoError` is a standard-library record:

```nazm
pub enum IoErrorKind { NotFound, PermissionDenied, AlreadyExists, WouldBlock, InvalidInput,
    TimedOut, Interrupted, UnexpectedEof, ConnectionRefused, ConnectionReset, AddressInUse,
    AddressNotAvailable, BrokenPipe, NotConnected, IsADirectory, NotADirectory, DirectoryNotEmpty,
    Closed, Cancelled, Other, }
pub struct IoError { kind: IoErrorKind, code: Int, message: Str, context: Str, }
```

`code` is the operating system's error number (POSIX `errno`), `message` its text (`strerror`), and
`context` what the program was doing (`io_context(e, "reading config")`).

`@std/file` offers `file_open(io, path, mode)`, `file_create`, `file_read(f, buf)`,
`file_read_all`, `file_write(f, bytes)`, `file_write_str`, `file_seek(f, from, offset)`,
`file_sync`, `file_close`, `file_metadata(io, path)` (`size`, `kind`, `modified_ms`, `permissions`),
`dir_list`, `dir_create`, `dir_create_all`, `dir_remove`, `file_remove`, `file_rename`, and
`file_replace_atomic(io, path, contents)` — write a temporary file in the same directory, flush it
to storage, and `rename` it over the target, which POSIX makes atomic on one file system. All take
an `IoCap` and have the `io` effect.

**Semantic cost.** One built-in handle type and its reclamation (the memory report gains a class);
no change to the existing built-ins or to `@std/fs`, whose API is frozen.

**Evidence plan.** Differential tests over every operation and every error kind reachable on the
host; atomic replacement observed; reclamation of handles; the KV application (§24).

**As built (2026-10-09).** `spec.md`, *Files and handles*, is the rule; three settlements.

- **Not closed when the last reference goes.** An `OsHandle` is a word — `generation << 32 |
  descriptor` — not a counted handle: it may cross into a task (a server hands a connection to a
  task), which a counted handle cannot, and the memory report is unchanged. It is closed by
  `os_close` or when the process ends. What the counted design was for, a close that can never
  reach a reused descriptor, is kept by the generation: the table remembers each descriptor's
  current generation, a closed generation answers `Closed`, and a close while another task is
  using the handle takes effect when that use ends.
- **Reading and writing open separately.** `os_open_read` and `os_open_write(path, mode)`, so the
  restricted sink (`N0372`) is where a file is written — as `write_file`'s path is — and a path to
  read may come from anywhere.
- **The number-to-kind table is the library's.** A status is `-errno`, the platform's number;
  `@std/ioerror` holds each platform family's table, chosen by the pure built-in `os_family()`,
  so what a number means is decided once, in Nazm, for the interpreter and every target. A closed
  handle's status is `-65536`, which no system gives, so `Closed` is never confused with the
  system's `EBADF` (a file written that was opened to read, `Other`).

Naming a path needs an `IoCap` held; using a handle already open needs only the handle — so a
function handed one `File` can use it and open nothing else, and `@std/file`'s functions on a
`File` take no `IoCap`. A `File` that carried one would have let whoever is handed a file take
the authority to open any other.

## 9. Networking

**Problem.** A server language needs TCP and UDP sockets, addresses, timeouts and name resolution.

**Existing mechanism considered.** None — the architecture records networking as undesigned
(`architecture.md`, §7 *Networking*), and C through `ForeignCap` cannot express it safely.

**The mechanism.** A capability, **`NetCap`**, separate from `IoCap` so that a program can be given
files without the network or the network without files; network operations have the `io` effect.
Sockets are `OsHandle`s. `@std/net`:

| | |
| --- | --- |
| **Addresses** | `SocketAddr { host: Str, port: Int }` with `addr_parse("127.0.0.1:8080")`, IPv4 and IPv6 literal hosts; `net_resolve(net, name, port) -> Result[Vec[SocketAddr], IoError]` |
| **TCP** | `tcp_listen(net, addr) -> Result[TcpListener, IoError]`, `tcp_accept`, `tcp_connect(net, addr, timeout_ms)`, `tcp_read(s, buf)`, `tcp_write(s, bytes)`, `tcp_read_timeout`, `tcp_shutdown`, `tcp_local_addr`, `tcp_peer_addr`, `tcp_close` |
| **UDP** | `udp_bind`, `udp_send_to`, `udp_recv_from`, with timeouts |
| **Timeouts** | Every wait takes or has a deadline; expiry is `IoErrorKind.TimedOut` |

HTTP and TLS are ecosystem.

**Evidence plan.** Loopback tests in every implementation: echo over TCP, many concurrent clients,
UDP round trip, refused connection, timeout, address in use, resolution of `localhost`; the server
reference application (§24); refusals without `NetCap`.

## 10. Scheduler-aware I/O

**Problem.** A task waiting for a socket must not hold a worker thread — and with it every task
pinned to that worker — on the M:N pool.

**Existing mechanism considered.** The pool already parks a task at four hooked waits and resumes it
when woken (§7.84, §7.107): a task waiting on a channel releases its worker. The design reuses that
park, rather than adding a second suspension mechanism or any `async` syntax.

**The mechanism.** Sockets are non-blocking. An operation that would block registers its descriptor
with a **reactor** — one runtime thread waiting in `kqueue` on macOS and `epoll` on Linux — and then
waits on its own condition with the existing hooked wait, so on the pool it parks and its worker
runs other tasks. The reactor wakes it, with the existing wake, when the descriptor is ready or its
deadline passes. In the thread scheduler the same wait blocks only the task's own thread. In the
interpreter every task is a thread and the operation simply blocks with its deadline. Operations
look ordinary: `tcp_read(s, buf)` is a call, not an `await`. `time_sleep(t, ms)` parks the same way.
Cancellation is the language's existing structured kind: closing a socket wakes any task waiting on
it with `Closed`, and a deadline bounds every wait.

**What still holds a worker, stated:** regular-file I/O (`read`/`write` on files is not pollable on
these hosts) and name resolution (`getaddrinfo` has no non-blocking form). Both are bounded by the
storage or the resolver; neither waits on a peer. Windows (IOCP) is later host-port work.

**Evidence plan.** A pool test that holds every worker's tasks in network waits while other tasks
still make progress (one worker, many parked readers, a busy task completing); reactor tests in the
contained Linux run (epoll) and on the macOS host (kqueue).

**As built (2026-10-09), §9 and §10.** `spec.md`, *Networking*, is the rule; settlements:

- **Built-ins over the files part's handles**, not a module of their own: a socket is an
  `OsHandle` in the same table, with the same generations and the same `os_close`, read and
  written with `os_read` and `os_write`. `@std/net` gives the `Result` APIs the record names
  (`tcp_listen`, `tcp_accept`, `tcp_connect`, `tcp_read`, `tcp_write`, …, `net_resolve`), and
  addresses are `HOST:PORT` text rather than a `SocketAddr` record.
- **A deadline belongs to the handle** (`os_set_timeout`), so a read is `os_read` and not a
  second `tcp_read_timeout`; a connect takes its own.
- **Using a socket needs only the socket** — as using a file does (§8) — and naming an address
  needs a `NetCap` held.
- **The reactor's wake is a sequence number** in a wait record kept per descriptor: a waiter
  arms the descriptor once and waits with the existing hooked wait until the number moves, its
  deadline passes or its handle is closing. On Linux a descriptor's registration carries every
  direction its waiters have asked for, since `epoll` keeps one per descriptor; `kqueue` keeps
  one per direction.
- **Name resolution** is the system's resolver and holds the task's worker while it answers,
  as the record says; a name it does not answer is `-65537`, `NotFound`, in both
  implementations, since the interpreter cannot see the resolver's own code.
- **Signals for a peer that has gone are never raised**: `MSG_NOSIGNAL` on Linux, `SO_NOSIGPIPE`
  on macOS, and the interpreter's host ignores `SIGPIPE`.

## 11. Time

**Existing mechanism considered.** `time_now_ms` (monotonic, `TimeCap`), `chan_select_until`,
`@std/time` in `Int` milliseconds. Kept; extended.

**The mechanism.** Built-ins `time_now_ns` (monotonic), `time_wall_ns` (Unix epoch, wall clock,
may jump) and `time_sleep_ms` (parks on the pool), all behind `TimeCap`. `@std/time` gains
`Duration { nanos: Int }` and `Instant { nanos: Int }` with arithmetic, comparison, `instant_now`,
`instant_elapsed`, `deadline_after`, `deadline_passed`, `duration_from_ms` and friends, and
`wall_now() -> WallTime { unix_nanos: Int }`. A timer is a sleep in a task or a select deadline.

**As built (2026-10-09).** As designed, with `@std/time`'s names prefixed `time_` as the module's
were (`time_now`, `time_elapsed`, `time_after`, `time_reached`, `time_wall`, `time_sleep`), and
the clocks and the sleep needing a `TimeCap` held and performing no effect, as `time_now_ms` did.
`time_sleep_ms` parks on the pool through the hooked timed wait; the monotonic clock is the one
`time_now_ms` already read.

## 12. Process and environment

**The mechanism.** `IoCap`: `stdin_read(buf)`, `stdin_read_line()`, `env_get(name) ->
Option[Str]`, `env_vars()`. A new capability, **`ProcessCap`**, for running other programs —
running a program is every authority that program has, so it must be granted explicitly:
`process_run(p, program, args, stdin) -> Result[ProcessOutput, IoError]` (exit status, signal,
stdout, stderr) via `posix_spawnp`, waiting on its pipes through the reactor. Signals: a program
may ask for `SIGINT`/`SIGTERM` to be delivered as a value it can poll or wait on (`signal_watch`,
`signal_wait`), behind `ProcessCap`; handlers that run arbitrary code are not offered. `exit_with`
and arguments are unchanged.

**As built (2026-10-09).** `spec.md`, *Process and environment*, is the rule; settlements:

- **Built-ins over the handle table**: `os_env_*`, `os_spawn`, `os_child_pipe`, `os_wait` and
  `os_kill`, and `@std/env`, `@std/io`'s `io_stdin_*` and `@std/process`'s `process_spawn`,
  `process_wait`, `process_kill` and `process_run` over them. A child's pipes are handles that wait
  as sockets do, so reading a child's output parks on the pool; `process_run` moves the input and
  the errors in tasks of its own and so takes a `SpawnCap` beside the `ProcessCap`.
- **`ProcessCap` is id 9**, as the table below records; `RandomCap`'s 8 is §21's.
- **Waiting for a child's exit holds the worker** (`waitpid`); its pipes do not.
- **Signals are not offered.** Watching for `SIGINT` or `SIGTERM` needs a handler installed in
  the process, which the interpreter cannot do without `unsafe` code the workspace forbids, and a
  native-only mechanism would be one program meaning two things. A native program that has
  started a child ignores `SIGPIPE`, as the interpreter's host always does.

## 13. Errors

`Result`/`Option`/`?` unchanged; no exceptions. `IoError` (§8) is the one error record for the
operating system, the network and child processes; `io_context` adds context; cancellation and
timeouts are kinds. `@std/error` adds `Error { message: Str, context: Vec[Str] }` with `error_new`,
`error_context`, `error_from_io` and `error_show`, so applications have one type to propagate with
`?`. FFI errors remain `c_errno()`, now mappable with `io_error_from_code`.

**As built (2026-10-09).** As designed, library only. `io_error_from_code` is
`io_error_from_errno`, and `error_io_result` turns a `Result[T, IoError]` into a
`Result[T, Error]` so one `?` serves both. No checking rule moved, so no epoch did.

## 14. Serialization

`@std/json` gains numbers with fractions and exponents (`json_add_float`, `json_float`), and pretty
printing (`json_encode_pretty`); its frozen API is extended, not changed. `@std/binary` adds a
deterministic binary encoding over `Bytes`: fixed-width little- and big-endian integers and floats,
unsigned and zig-zag varints, length-prefixed strings and bytes, with a `Writer`/`Reader` pair whose
reads return `Result`.

**As built (2026-10-09).** Library only. `@std/json`'s 1.0 `json_parse` keeps refusing a fraction
— its documented behaviour is frozen — and `json_parse_floats` reads one, beside `json_add_float`,
`json_float` and `json_encode_pretty`; JSON has no infinity or NaN, so those are written `null`.
`@std/binary`'s `Writer` and `Reader` are as designed, the reads `Result[_, Str]`.

## 15. Compile-time evaluation — constants

**Problem.** Array sizes, table sizes, masks and configuration need named compile-time values; the
`comptime` command evaluates on request but nothing in a program can use the result.

**Existing mechanism considered.** `nazm comptime` (a command), and contract `requires` evaluated
when literal (N87). Neither puts a value into a program.

**The mechanism (as designed; see *As built*).** `const NAME: T = expr;` at module level, `pub` to export. `T` is a numeric type,
`Bool` or `Str`; `expr` is evaluated by the checker, using the same evaluator `comptime` uses, under
its budget; it may use literals, other constants, operators and pure built-in calls, and nothing
else. A constant is usable anywhere an expression is, and where an array size is required. The
smallest mechanism that solves the use cases; textual macros and declaration generation are not
added — no use case in the tree needs them, and they are the uncontrolled mechanism the programme
forbids.

**As built (2026-10-08).** Narrower, and `spec.md`, *Constants*, is the rule. The value is
literals, other constants, operators and numeric conversions — no built-in calls, so nothing a
constant computes depends on a library's implementation; it is computed with
`nazm_sema::numeric`, the semantics every implementation runs, so a constant and the same
expression at run time cannot differ. `pub const` is refused (`N0101`): an importer would need
the value, and the persisted interface carries none, so a cold check and a cached one could
disagree. Exporting constants comes with the interface carrying values.

## 16. Attributes

**Problem.** Deprecation, test and benchmark registration, conditional compilation, and (in Gate 3)
layout, section placement, interrupts and exports all need to annotate a declaration.

**The mechanism.** One syntax: `@name` or `@name(args)` before a declaration, with a **closed,
compiler-owned vocabulary**; an unknown attribute is refused (`N0619`, new). Gate 2 defines
`@deprecated("why")` (a warning at each use, `N0620`), `@test`, `@test(fails)`, `@bench`, `@fuzz`,
and `@cfg(...)`. Attributes are part of a declaration's identity where they change its meaning
(`@cfg`) and of its interface where importers can observe them (`@deprecated`). No keyword per
domain, and nothing user-defined.

**As built (2026-10-09).** The syntax, the closed vocabulary and `N0619` as designed (`spec.md`,
*Attributes*): an attribute is written before a top-level declaration — not before a `use`, and
not yet before a method inside an `impl` or a `trait` — and a malformed one is a single syntax
error, after which it is not checked again. Narrower in one respect: **`@deprecated` is
metadata-only deprecation, and warns nowhere** (settled by G2-C1). `nazm.diagnostic/1` has one
severity, `error`, and a warning is a new severity every consumer and exit status would have to
learn; 1.0 does not add one for one attribute. Instead `@deprecated` is what `nazm doc` publishes:
the item's `deprecated` in `nazm.api-doc/2` (the reason, or empty) and a line in the Markdown — so
it marks only what `nazm doc` publishes, a `pub` function, `struct` or `enum`, and is refused
(`N0619`) anywhere it would be read by nothing. `N0620` is not assigned; a warning at each use
comes with a warning severity, if one ever does. The attributes are kept out of `compiler/`, the corpus and `library/core`: the
self-hosted lexer, which 1.0 carries frozen, has no `@`.

## 17. Conditional compilation

`@cfg(os = "macos")`, `@cfg(os = "linux")`, `@cfg(arch = "aarch64")`, `@cfg(arch = "x86_64")`,
`@cfg(target = "TRIPLE")`, `@cfg(feature = "NAME")`, `@cfg(profile = "NAME")`, combined with
`all(...)`, `any(...)` and `not(...)`. A declaration whose condition is false is parsed and then
dropped before name resolution; two declarations of one name are allowed only when their conditions
cannot both hold for the build. Conditions are decided by the build's target, its enabled features
and its profiles — never by the environment — and all three are part of the check key and the build
identity, so a cached result is never reused across configurations. `nazm run` decides them for the
host.

**As built (2026-10-09).** As designed (`spec.md`, *Conditional compilation*). The configuration
is a `BuildConfig` on the module graph — the target's operating system, architecture and triple,
the features and the profiles — and the checker drops every declaration whose `@cfg` does not hold
before anything resolves a name, an `impl` with its methods. `nazm build --target T` decides for
`T`; `check`, `run` and a build with no target for the host. The profiles are `--profile`'s and
every package manifest's. A module whose text writes `@cfg` is fingerprinted with the
configuration, so its check is filed under it; a module that does not write one is shared across
configurations, as before. A build's later stages need nothing new: what they see is what the
check kept. The features are empty until §22 gives a package a way to enable one.

## 18. User testing

`@test fn name() -> Result[Int, Str]` declares a test: `Ok` passes, `Err` fails with its message, a
trap fails. `@test(fails)` passes only if the test traps or returns `Err`. `nazm test` runs a
module's `@test` functions — interpreted and as one native build — alongside the existing golden
files, reports each by name (`nazm.test/1` gains test-function rows; additive), and filters with
`--filter`. `@std/test` gains `assert_eq`, `assert_true`, `assert_some`, `assert_ok`, and a property
helper, `property(seed, cases, gen, check)`, using `@std/random`'s seeded generator. `@fuzz fn
f(input: Bytes) -> Result[Int, Str]` registers a fuzz target; `nazm test --fuzz N` drives it with N
deterministic generated inputs.

**As built (2026-10-09).** As designed, with the outcome carried by the process (`spec.md`, *Tests
and benchmarks*). A test function runs through a generated entry appended to its file's text as
the loader serves it — the file's own `main` renamed in place to a name of the same length, so no
offset moves — under `nazm run` and as an executable, **one build per test** rather than one per
module: each test is then its own process, and a trap fails one test, not the run. `Ok` ends `0`; an
`Err` prints its message and ends `2`, a trap's status, so `@test(fails)` is told a failing test from
a program that does not check (`1`). The shapes are refused otherwise (`N0619`). The assertions are
`assert_true`, `assert_eq_int`, `assert_eq_str`, `assert_some` and `assert_ok` — typed, as the
language has no generic equality — and `property(seed, cases, check)`. A fuzz target is given `--fuzz
N` inputs (100 by default) of 0–64 bytes from a fixed xorshift sequence, the same in every
implementation; there is no coverage feedback and no corpus. `nazm.test/1` gained `kind`, additive.
Tests run for the host's configuration only.

## 19. User benchmarking

`@bench fn name() -> Int` and `nazm bench [PATH] [--json]`: builds the benchmarks natively, runs
each until a time budget is met, and reports nanoseconds per iteration (`nazm.bench/1`,
EXPERIMENTAL). The returned `Int` is consumed so the work cannot be optimised away. The repository's
`xtask bench` is unchanged.

**As built (2026-10-09).** `@bench fn NAME(capabilities…) -> Int`; `nazm bench PATH… [--iterations
N] [--json]` builds each through the same generated entry at `-O2` and reports one run's
nanoseconds per iteration (`nazm.bench/1`, EXPERIMENTAL); every answer is folded into one value
the entry prints, so the work is not optimised away. A fixed count, not a time budget; no warm-up, repetition or statistics:
one measurement, stated as one. The performance register (`docs/internal/performance-log.md`) stays the place a
claim is filed.

## 20. Logging and observability

`@std/log`: leveled, structured records (`log_info(out, "msg", fields)`) written as one JSON object
per line to standard error, needing only `OutCap`; `task_id()` (a built-in, the current task's
number, 0 for `main`); `runtime_stats()` (tasks spawned, live, channel waits, from the runtime's
existing counters); and `span_begin`/`span_end` records with timings. Transport-neutral: lines on
standard error, for any collector.

**As built (2026-10-09).** `@std/log` as designed, with a `Logger` carrying a level and fields of
its own, and spans under a `TimeCap`. **`task_id()` and `runtime_stats()` are not built**: both
need the task runtime to number tasks and expose its counters, and that runtime's text is held line
for line to the self-hosted compiler's (`runtime parity`), so they wait for a change that carries
both; a program numbers its own tasks by passing an id to `spawn`. The scheduler's counters remain
`NAZM_SCHEDULER_REPORT`'s.

## 21. Randomness

Two things that must never be confused: `@std/random`'s **seeded generator** — `Rng` (xoshiro256**
seeded through SplitMix64), pure, reproducible, needing no authority — and **entropy**, a new
capability **`RandomCap`**: `random_bytes(r, buf)` and `random_u64(r)` from the operating system
(`getentropy`), suitable for keys and seeds. A seed from entropy makes a generator unpredictable;
reproducibility is the default and is explicit.

**As built (2026-10-09).** As designed, with one built-in rather than two:
`os_random_bytes(buf)` behind `RandomCap` (id 8), `getentropy` natively and the random device in
the interpreter; `@std/random`'s `random_bytes` and `random_seed` over it, and its `Rng`
(`random_new`, `random_u64`, `random_int`, `random_float`, `random_shuffle`) ordinary pure code.

## 22. Package features

`[features]` in `nazm.toml`: `name = ["other-feature", "dep/feature"]`, `default = [...]`, and a
dependency's `features = [...]`. Features are additive; the enabled set is the union of what is
requested, resolved in name order, recorded in the lock file, and nothing about the environment
enters it. `@cfg(feature = "x")` reads it. This is an additive manifest key, so `nazm.package/1`
stays `/1` for manifests without it; the lock file records features as `nazm.lock/2`, and `/1` locks
still read.

**As built (2026-10-09).** As designed, narrower in three places (`spec.md`, *Package
features*). Each package's set is its own, and each module's `@cfg(feature = …)` reads its own
package's: the configuration carries the root's set and each dependency's by `NAME@VERSION`, and a
module's key says which it is. `--features` (on `check`, `run` and `build`) asks the root only, and
the lock records what the manifests enable, not what a command asked; there is no way to switch a
`default` off. `nazm.lock/2` is written only when a feature is enabled, so every lockfile written
before Gate 2 is still the one written now. `nazm.package/1` stays `/1`: the keys are optional.

## 23. The C foundation for other languages

Python (`ctypes`/`cffi`), Swift, Kotlin/JNI, Node-API and C# P/Invoke all bind to a C ABI: exported
functions over scalars, pointers to opaque handles, and C strings. With Gate 2, a Nazm `--lib` or
`--shared` library exports `Int`, `Bool`, every fixed-width integer and both floats. Evidence: a
shared library built by `nazm build --shared` and called from Python through `ctypes` with integers
and floats. The ecosystems themselves are later.

**As built (2026-10-09).** As designed: an export's C entry takes and returns each number in its
own C type, and the header names `int8_t` … `uint64_t`, `float` and `double`. Python's `ctypes`
calls a shared library built by `nazm build --shared` with every width and both floats, under both
backends (`c_foundation.rs`). Swift, Kotlin, Node and C# bind the same header; their ecosystems are
later.

## 24. Reference applications

`examples/apps/`: `nwc` (a CLI word, line and byte counter with flags and stdin), `kvd` (a concurrent
TCP key-value server), `jsonpipe` (a JSON-lines filter and aggregator over files), `nbody` (a
floating-point simulation) and `kvstore` (a persistent log-structured store with atomic compaction).
Each has an expected output, runs interpreted and under both backends where its operations allow,
records its resource use and time, and is held by tests.

**As built (2026-10-09).** As designed, in `examples/apps/`, held by `tests/apps.rs` under the
interpreter, LLVM `-O0` and `-O2` and Cranelift, with real arguments, files and standard input; each
directory's `README` table records time and memory of one native run. Writing them found two
things about the language as it is, recorded rather than worked around in the compiler: there is
no `else if` (a chain is `else { if … }`), and `Vec[Str]` is not `Strs`, so a map's keys are copied
before the library sorts them. `kvd` has no shared mutable table — one task owns it and the
connections ask it over channels — which is the language's rule, not a limitation of the program.

## 25. Competitor map

`docs/internal/v1-competitor-map.md`, one row per language, with no superiority claim without a measurement.

**As built (2026-10-09).** `docs/internal/v1-competitor-map.md`: fifteen languages, each with why it is
chosen, the problem underneath, Nazm's mechanism, the 1.0 evidence by test or document, and the gap
that remains. It claims no superiority: no measurement against another language is filed.

## Capabilities after Gate 2

| Kind | Id | Grants |
| --- | --- | --- |
| `IoCap` | 0 | files, standard streams, environment |
| `SpawnCap` | 1 | `spawn` |
| `ForeignCap` | 2 | C calls |
| `VouchCap` | 3 | `str_vouch` |
| `TimeCap` | 4 | clocks, deadlines, sleep |
| `MmioCap` | 5 | device registers |
| `OutCap` | 6 | `print`/`eprint` |
| `NetCap` | 7 | sockets and name resolution |
| `RandomCap` | 8 | operating-system entropy |
| `ProcessCap` | 9 | running programs, signals |

The set widens from eight bits to sixteen. Ids never change.

## Versions

| | Before | After |
| --- | --- | --- |
| Semantic epoch | 30 | 42 — bumped by each semantic commit of Gate 2 (and once by M1, 34); `stability.md` lists each step |
| `nazm.interface` | 11 | 11 so far — numeric types and arrays are new type names and shapes, which a reader from before Gate 2 refuses rather than misreads (`wire.rs`, the `IoCap` precedent); the major moves when the interface carries something an older reader would misread |
| Runtime ABI | 14 | 20 — floats, handles and files, networking and the reactor, time, processes, entropy; `stability.md` lists each step |
| Standard library API | 1.0 | 1.1 — additions only, every 1.0 line of `API-1.0` unchanged; twenty-nine modules |
| Language | 1.0 | 1.0 — Gate 2 lands before the first 1.0.0 release; the compatibility corpus is the check that no 1.0 program changed meaning |
| Lockfile | `nazm.lock/1` | `nazm.lock/1`, and `nazm.lock/2` when a feature is enabled (§22) |
| Machine outputs | — | `nazm.test/1` gained `kind`, additive; `nazm.bench/1` new, EXPERIMENTAL |

What a program written before Gate 2 meets is `docs/user/migration-gate-2.md`.
