# The bootstrap: how it was designed, what it does, and what it establishes

**One document for the whole subject.** It absorbed `bootstrap-design.md` on 2026-09-21 —
the pre-implementation design record — because a reader asking "why is the self-hosted
compiler shaped like this?" and one asking "what does the fixpoint prove?" were being sent
to two files that each answered half.

---

## 1. The design, and why it came first — *historical, 2026-09*

*This section is the design record as written **before** the language was extended, kept
because the reasoning is the point. It describes intent at the time, not current status;
`capability-matrix.md` is the authority on what exists.*

The compiler was designed first, on purpose: **the language features added next were
derived from a compiler that had been designed**, not chosen from a wish list and justified
afterwards. `nazmc` reads `.nz` files and writes LLVM IR text, then hands it to `clang` —
the same pipeline `nazm build` already ran, so the target was known exactly rather than
imagined.

**The whole compiler is expressible with `Int`, `Bool`, `Str`, and growable arrays of `Int`
and `Str`.** No generics, traits, closures, user-defined types, pattern matching, or a
shared-pointer graph. The technique is the one bootstrap compilers have always used:
*structures of parallel arrays, addressed by index*, with every traversal iterative and
every work stack an explicit `Ints`.

That analysis produced the feature list, and nothing beyond it:

| Needed for | Feature | Milestone |
| --- | --- | --- |
| reading source, names, literals, building output | `Str` with length, byte access, slicing, concatenation, comparison, `Int` conversion | B1 |
| every table above | growable `Ints` and `Strs` with `push`, indexed get/set, `len` | B2 |
| reading input, writing the `.ll`, reporting, exit status | file read/write, arguments, stderr, exit codes | B3 |
| a compiler in more than one file | modules with deterministic multi-file resolution | B4 |
| every traversal | nothing — iterative algorithms use the `Ints` stack from B2 | B5 |

**Deliberately not added, and still absent:** generics, traits, closures, user-defined
records or tagged unions, pattern matching, a hash map, reference types, a borrow checker,
destructors, exceptions, iterators, operator overloading, a package registry. Each may be a
good idea later; none was a prerequisite for the first compiler. This list is the source
for the *deferred rather than delivered* distinction in `capability-matrix.md`.

**The memory model was stated as a limitation, not discovered as a bug.** Strings and
arrays are immutable in identity and allocated from a process-lifetime region that is never
freed: a compiler process reads input, produces output and exits, so reclaiming memory
inside that lifetime buys nothing and costs an ownership system the bootstrap does not
otherwise need. The consequence was named at design time and required testing rather than
assuming — *peak memory grows with input size and with the number of intermediate strings,
so string building uses an array plus one join rather than repeated concatenation.*

That sentence is the one this project paid for. Three stage drivers were written with
`str_concat` in a loop anyway, and on 2026-09-21 the quadratic retention that follows from
never freeing took the machine down three times. `runbook.md` carries the incident and the
containment rules it produced.

> **Partly superseded by N7, 2026-09-22.** Sequence storage is reclaimed now, under
> `docs/spec.md`'s *memory constitution*, and a workload whose live set is constant no
> longer grows. **The sentence above still stands for this document's subject**, twice
> over: `Str` storage is still not reclaimed, so the quadratic `str_concat` shape is
> exactly as dangerous as it was; and the compiler written in Nazm emits no reclamation at
> all, so a program *it* compiles behaves as this paragraph describes in every respect. The
> containment rules are unchanged and are not a milestone's to relax.
>
> **Both halves retired by N8, 2026-09-22.** `Str` storage is reclaimed — a `Str` carries
> the allocation that owns its bytes, `architecture.md` §7.8 — and `compiler/emit.nz` emits
> the same model, held to the Rust emitter's runtime by the `runtime parity` gate and to
> its *decisions* by a differential test that compares what each compiler's programs
> reclaimed. Measured on the shape this document is about: the accumulator loop at 8,000
> iterations fell from **70.66 MiB to 2.23 MiB**.
>
> **The advice above is still the advice.** `str_concat` in a loop is quadratic in *time*
> whatever happens to the memory, and `str_join` is still one pass and one allocation. What
> changed is that getting it wrong now costs time rather than the machine.
>
> **The containment rules do not change, and the reason is not that they might still be
> needed.** A leak that has been fixed is not evidence that the incident of 2026-09-21
> cannot recur: an unbounded loop, a runaway recursion in a future language version, or a
> defect in this very reclamation reaches the machine exactly as the original did.
> Containment is operational law — `runbook.md` owns it — not a workaround with an expiry.

### What the design did not settle

- The public ABI and value layout stay unfrozen. `Str` and arrays are compiler-internal
  representations, and no program can observe their layout.
- Whether the self-hosted compiler eventually uses records and pattern matching is a
  question for after it exists. Writing it with arrays first was a claim about the shortest
  sound path, not about the language's eventual shape.
- Concurrency was designed separately and was expected to share none of this. It is now
  **compiled by this compiler** — `compiler/emit.nz` carries the task and channel runtime.

---

## 2. What the chain does

`compiler/bootstrap.sh` runs, from one compiler source **S**:

1. the retained Rust implementation compiles S into **C1**;
2. C1 compiles S into **C2**;
3. C2 compiles S into **C3**;
4. C2 and C3 are compared, and both compile the conformance corpus.

Each stage writes to its own directory, so no stage can read another's artefacts and a
stale binary cannot stand in for one that was never built. Every stage is checked and
failure propagates — a script that prints success after a failed stage is worse than no
script.

**The source is imported, not concatenated.** Every stage compiles `compiler/emit.nz` and
resolves its `use` lines itself, with the resolver written in Nazm (`compiler/module.nz`),
so what is bootstrapped is the compiler as it is actually written. The script used to strip
the `use` lines and paste the files together, which worked but meant the chain never
exercised the thing it was meant to prove.

## 3. The result, on the verified configuration

| | |
| --- | --- |
| Host | **Linux aarch64**, inside `nazm-contained:1.98.1` — the configuration of record since containment became mandatory. Darwin arm64 remains *a* verified configuration, historically |
| Toolchain | rustc 1.98.1 (pinned); Debian clang 19.1.7 in the container, Apple clang 21.0.0 on Darwin |
| S | `compiler/{lex,parse,analyse,module,emit}.nz`, **5,041 lines** |
| **C2 vs C3, emitted IR** | **byte-identical** |
| **C2 vs C3, executables** | **byte-identical**, including the runnable form |
| Conformance | 10 cases, each agreeing across C2, C3 **and the reference** — 5 of them multi-module |

The IR being identical is the fixpoint: compiling S with a compiler built by Rust and with
a compiler built by itself produces the same output. That is what self-hosting means, and
it is checked rather than asserted.

### The reference compiler, and a correction — 2026-09-22

This script does not build `$nazm`; it requires it to exist. Inside the contained runner
`CARGO_TARGET_DIR` is `/work/target`, and `docker/contained.Dockerfile` warms that
directory with a build of the whole workspace when the **image** is built. So every
contained bootstrap from the image's build date until this one used that binary — for the
reference check at the top, and for stage 1 — regardless of what was in the tree mounted at
`/src`, and said nothing about the substitution. `cargo xtask contained bootstrap` now runs
`cargo build -p nazm-cli` first.

What this does and does not change: the fixpoint is C1 → C2 → C3, and C2 vs C3 is a
statement about the compiler written in Nazm, which the substitution does not touch. What
was wrong is narrower and still worth correcting — *which* front end produced C1, and
whether the reference check at the top of the script was checking the language the tree
currently defines. It was not.

### Visibility, since N2 — 2026-09-22

The compiler source states its own module boundaries. **84 of its 224 top-level functions
are `pub`; 140 are private**, and the split is computed from which names are actually
called across a file boundary rather than applied to everything: `lex.nz` exports 40,
`parse.nz` 23, `analyse.nz` 18, `module.nz` 3, and `emit.nz` — the largest at 58 functions
— exports nothing at all, because nothing imports it. `emit.nz`, `check.nz` and `parser.nz`
gained `use` lines for modules they were using and naming nowhere; `emit.nz` was calling
into `lex.nz` and `parse.nz` through `analyse.nz`'s imports, which stopped working when
imports stopped being transitive.

That the compiler can express those boundaries cleanly, and that fewer than half its
functions are part of any interface, is the useful evidence: a visibility model the real
multi-file workload cannot express is a model that would have been worked around.

### Semantic parity, and why it is a separate claim — N2.1, 2026-09-22

**C2 = C3 is a fixpoint.** It says the compiler written in Nazm reproduces itself, and a
compiler that consistently accepts a program the specification forbids reaches a fixpoint
too. It is not evidence that the two implementations agree about the language.

For one day they did not. N2 taught the Nazm-written compiler to *parse* `pub` and nothing
else: it still resolved every name across the whole program, so it accepted eight kinds of
program the reference refuses and **miscompiled** a ninth — two modules each defining a
private `helper` both emitted `@nz.f0`, and clang rejected the module as an invalid
redefinition. The fixpoint was undisturbed throughout, which is the point.

The evidence that replaced it has three parts, kept apart on purpose:

| Claim | What runs | What it establishes |
| --- | --- | --- |
| **C2 = C3** | `compiler/bootstrap.sh` stages 2 and 3 | a fixpoint: the Nazm compiler reproduces itself, byte for byte |
| **reference = C2 = C3** | the conformance corpus, `compiler/conformance/` including `modules/` | the three agree on what a program **means** |
| **accept = accept, refuse = refuse** | `the_two_compilers_agree_on_every_module_and_visibility_rule` | the two agree on which programs are in the language at all, comparing the diagnostic code where both produce one |

The third is the one that would have caught the divergence, and it is fifteen cases: every
rule `spec.md` settles about modules, run through both compilers and required to give the
same answer. The multi-module conformance cases are the second; they are directories rather
than files because each compiler resolves `use` for itself, and a single-file corpus cannot
exercise a module boundary.

**How the Nazm compiler knows.** `compiler/module.nz` returns the merged text **and** the
module graph: where each file's text begins, and which module each `use` names. That the
text is concatenated is a representation detail of this compiler; the boundaries travel
beside it, and every question about ownership is answered from those rather than from the
text. `compiler/analyse.nz` reads a definition's owner from where its declaration sits and
its `pub` bit from the function node, collects every module's interface before any body is
checked — which is why an import cycle has no ordering to violate here either — and records
what each call resolved to. Emission reads that and never looks a name up again. The last
part is not tidiness: it is why two modules' private `helper` now get two symbols.

**The remaining difference from the reference is not about visibility.** `load.rs` keys a
file by its canonical path and this keys by a textually normalised one, so a file reached
through a symbolic link is one module to the reference and two here. Recorded above, and
unchanged by N2.1.

### What is normalised, and why it is narrow

The macOS linker stamps an `LC_UUID` and embeds the output *filename* in the ad-hoc code
signature. Neither is anything the compiler produced. The script therefore assembles each
stage **twice**: once runnable, and once with `-Wl,-no_uuid` purely to compare — a binary
without an `LC_UUID` is one macOS refuses to load, so the normalised copy is never
executed. Each stage links to the same filename in its own directory, which removes the
second difference.

Nothing about code or data is touched. On this configuration the runnable binaries turn out
to be byte-identical too (`runnable-bytes-differing: 0`), because the UUID clang computes
is a function of the content — so the normalisation removes a difference that did not
arise. It is kept because it *can* arise, and finding out by having the check fail would be
worse.

### The cache is not part of this chain — N4, 2026-09-22

N4 gave `nazm check` a store of semantic results. **The bootstrap neither reads nor writes
one**, and that is a decision rather than an omission.

`compiler/bootstrap.sh` runs `nazm check` on the compiler source as its reference leg, and
the stages it builds are `nazm build` invocations, which reuse nothing. Each stage works in
its own directory, so no `.nazm` store from one run is visible to another, and the fixpoint
is a comparison of emitted IR — a thing no cache is involved in producing. **A cache hit is
therefore not a prerequisite for self-host success, and could not become one.**

Nor is the cache ported into the compiler written in Nazm. It is toolchain architecture,
not language semantics: no Nazm program can observe a `CheckKey`, and a second store
written in a language with no `realpath` would be a second thing to keep correct in
exchange for symmetry nobody needs. What the two implementations must agree on is the
language, which is what §3 measures. `nazm.root` changes nothing for either — the
Nazm-written compiler computes no durable identity at all, so there is nothing for a
declared root to change.

## 4. What this does **not** establish

- **It is not evidence that the compiler is correct.** A compiler that miscompiles S in a
  way that reproduces the same miscompilation is a fixpoint too. Reproducibility and
  correctness are different claims; the conformance corpus and the differential tests in
  `crates/nazm-cli/tests/selfhost.rs` are what speak to the second, and they compare
  against the Rust implementation and against explicit expectations rather than against the
  Nazm compiler itself.
- **It is one target per run.** Nothing here says what an unverified target would do.
- **It says nothing about archive reproducibility.** That is a separate claim with a
  separate answer; see the release report.
- **The Rust implementation is still required**, to build C1 and to run `nazm check` inside
  the script. It is the bootstrap toolchain and the semantic oracle, and deleting it after
  the first successful build is exactly what must not happen.
- **External dependencies remain**: `clang` assembles the IR and links, and libc supplies
  `malloc`, `realloc`, `memcmp`, `memcpy`, `snprintf`, `write`, `exit` and the `stdio` file
  functions. Emitting LLVM and invoking an external assembler is permitted and documented;
  it is not hidden.

## 5. Current limitations

- **Module resolution keys files by a textually normalised path, not a canonical one.**
  `./x.nz`, `x.nz` and `a/../x.nz` are one file; two names reaching one file through a
  symbolic link are two, and it would be read twice. The Rust driver keys by the canonical
  path instead, because it can — the emitted runtime has no `realpath`.
- **Only relative imports.** The reference resolves `@std/NAME` and a package's `NAME:path`
  (`load.rs`); this loader refuses both **by name** since N77 ("the compiler written in Nazm does not
  resolve `@std/text`: it names a standard library module", exit 2), where until then it looked for
  a file of that name and reported it missing. Aliases (`as`) and qualified names (`::`) are refused
  by its parser.
- **Nothing is freed** — *until N8, which closed it*. See §1; the design stated it, and
  `capability-matrix.md` area 4 carries the current status and the acceptance condition.
- **A record's or an enum's native type name is its index**, `%nz.t<n>`, exactly as a
  function's symbol is `@nz.f<n>`. Sound because this compiler emits one LLVM module for a
  whole program, and the same limitation N5 removed for the reference compiler and
  deliberately not for this one. It is one limitation, not three: whatever fixes the symbols
  fixes the type names.
- **A bare block is not an expression here**, so a `match` arm's body may not be one. See
  §5's N10 note; the limitation is N2's and predates enums — *closed by N12.1, below*.
- **The chain cannot run uncontained**, by design. See §6.

### Records, and what the chain now carries — N9, 2026-09-23

The compiler written in Nazm understands records: `struct` and `.` in the lexer,
declarations and named construction and projection-path assignment in the parser, a record
table with three declaration stages in the analyser, and named LLVM aggregates with
generated retain and release helpers in the emitter. **There is no divergence to record** —
it accepts and rejects the same record language the reference does, which is the standard
N2.1 set and which this document exists partly to keep.

It also **uses** one. `Place { line, column }` replaced `line_of` and `column_of` in
`compiler/emit.nz`, which were two functions because a function returns one value. So S —
the source this chain compiles with itself — now contains a record, and C1, C2 and C3 each
compile one.

The corpus grew with it: `compiler/conformance/records.nz` covers construction, copy,
replacement, nesting, both return edges, a projection out of a temporary and two hundred
short-lived records with heap fields; `modules/exported-record` sends a record with an
owning field across a module boundary and back. Both are compiled by C2 and C3 and run
against the reference.

**The digest moved, twice, and that is expected.** `spec.md` and the milestone directive
both say so: a compiler that gained a language feature emits different bytes. The fixpoint
is what matters, and it held at every step —
`1ce9aafb…` (N8) → `3ecf0921…` (records) → `3491dfed…` (a record in the compiler's own
source). Preserving an old digest by declining the change would have been preserving the
number rather than the property it stands for.

### Enums, and the one divergence this document has to record — N10, 2026-09-23

The compiler written in Nazm understands enums and `match`: `enum`, `match` and `=>` in the
lexer; declarations, variant construction and arms in the parser; a data model in the
analyser that is **one flag** on the table N9 already had — an enum is a record whose fields
are its variants, and a variant is a record whose fields are its payload — and, in the
emitter, tagged aggregates with generated helpers that switch on the discriminant and then
call the variant's own.

Parity is compared on **codes rather than spans**, and that is a limitation of the
comparison rather than of either compiler: the self-hosted parser records one span per arm,
so a diagnostic about a payload field inside a pattern points at the arm where the reference
points at the field. Forty cases compare what each compiler *decides*, which is what N2.1 is
about.

**There is one divergence, and it is N2's.** A bare block used as an expression is still
refused by name in the self-hosted parser, so an arm body there may be `n + 1` but not
`{ let n = str_len(s); n }`. The reference accepts both. N10 did not introduce the
limitation and did not remove it; it made it reachable in one more position, and
`compiler/*.nz` and the corpus are written without it.

The corpus grew again: `compiler/conformance/enums.nz` covers every variant shape, both
directions of composition with records, nesting, a payload returned from an arm,
replacement across variants and within one, two hundred short-lived values with heap
payloads and an inactive owning variant that must cost nothing — and it declares its
variants in **none** of the orders that matter, so a compiler that took the parser's order
for the canonical one would build one value and read another. `modules/exported-enum` sends
an enum with an owning payload across a module boundary, inside a record, and back.

**It also uses one.** `enum Owning { Nothing, Sequence, Strings, Backing, Channel,
Record(at: Int), Enum(at: Int) }` replaced the small integer `owns` returned in
`compiler/emit.nz`, so S — the source this chain compiles with itself — now contains an enum
as well as a record, and C1, C2 and C3 each compile one. `architecture.md` §7.11 says why
that structure and not another: it is the case where the value is returned and never stored,
which is the only case a container is not needed for.

**And making that dispatch exhaustive found an N9 defect in this compiler.** A record passed
to a task produced a wrong answer and leaked its fields: the argument block's offsets were
arithmetic that predated records, and the trampoline released four kinds of value and not a
record's. The block is a named LLVM type now, as the reference emitter's always was. The
divergence was real, it was a year of milestones old in compiler time, and **no N9 test
passed a record to a task through this compiler** — the gap was in the corpus.

**And a second one, found the same way.** A surviving mutation asked why zeroing a
variant's assembly slot mattered, and the answer was that a `break`, a `continue` or a
`return` out of a half-evaluated expression gave back nothing it was holding — the
arguments already produced, and the value a `match` was holding while an arm ran. That was
N8's, in this compiler and in the reference both, and `architecture.md` §7.11 writes it up.
The fix here is `leave_loop`, and the differential memory case that carries it abandons a
record, an enum and an arm in one program.

**The digest moved a third and a fourth time**, for the same reason and with the same
property intact: `1ce9aafb…` (N8) → `3ecf0921…` → `3491dfed…` (N9) → `8030ecbb…` (enums and
`match`, an enum in the compiler's own source, and the task-block fix) → `ba8615e4…` (what
a `break` and a `continue` give back). `ir: identical`, executables byte-identical once the
linker UUID is removed, and C2 = C3 over 14 conformance cases, 7 of them multi-module, at
8,724 source lines. Preserving the old number by declining the change would have been
preserving the number rather than the property it stands for.

### A transfer in a value position, the divergence N10 left open — N10.1, 2026-09-23

N10's evidence exposed one divergence older than N10, and it is recorded here rather than
written out of the history above:

```nazm
let v = pick(int_to_str(i), if i == 1 { continue; } else { i });
```

The reference checked, interpreted and compiled this to `7` with every string reclaimed.
The compiler written in Nazm — interpreted, as C1, as C2 and as C3, identically, because
they are one program — emitted `call i64 @nz.f0({ ptr, i64, ptr } %19, void %24)`, which
clang refuses. Its neighbours were worse: with the other branch leaving, or both, it failed
**inside itself** with `N0405 this sequence is empty` at `compiler/emit.nz:2887`, exit 2. A
`let`, a `return`, a comparison and an assignment of the same shape each produced a
different `void` operand, and a `match` over one reported "`s` is not defined". One string
comparison compiled, ran, answered correctly, and leaked three strings.

**The exit status 133 recorded against an intermediate state of this work did not reproduce
at `2f0cba9`** in any of those forms, under the selfhost suite's 512 MiB address-space ceiling
or without it. 133 is 128 + `SIGTRAP` on the contained runner's aarch64 Linux — the signal
`llvm.trap` raises there, measured — and neither emitter's runtime contains `llvm.trap`.
Reaching an `unreachable` at `-O0` was measured too, and it does *not* trap: control falls
into whatever follows. So the most that can be said of the unreproduced 133 is that it
belongs to no committed tree, and that the class of defect it would have been — emitted code
running on past a point that was never meant to be reached — is the class this correction
removes: every accepted case now emits valid IR, and the reachable paths through it are the
ones the reference has.

**The cause was one representation missing twice.** The checker folded "this never
completes" into `unknown`, which also meant a statement and an error, and typed an `if` by
its then-branch; the emitter opened every join whether any branch reached it. Records
survived by accident — their field types came from the declaration — which is why N9's and
N10's corpora, full of record and variant transfers, never saw it. `architecture.md` §7.11
has the repair.

**And the audit found three more, all of them rules the reference already kept.** A
`break`, a `continue` or a `return` out of a `scope` did not join it, so a task that failed
there was never waited for and the program printed `1` where the reference stopped with
`N0401`. A statement after one that leaves was compiled rather than refused as `N0313`. And
a value that never arrives was `unknown` where the reference makes it `Int`, so
`match (if c { break; } else { continue; }) { … }` was accepted. A fifth finding was not
this compiler's alone: `==` on two strings leaked its operands in **both** native backends.

**What is still not the same, stated.** The self-hosted checker has no `Unit` rule — the
reference's `N0300`, *this produces no value* — and does not compare the types of an `if`'s
two branches (`N0302`). Each concerns only programs the reference refuses, so neither is a
divergence in the accepted language; but for those programs this compiler emits IR clang
refuses, or fails with `N0405`, rather than refusing by name. That, and N2's bare block,
are the known differences, and the second of them is still the only one a program the
reference accepts can reach. *The first two were closed by N10.2, below.*

The corpus carries all of it: `crates/nazm-cli/tests/transfers/` holds 32 programs, the
original among them byte for byte, run five ways; and six of them are conformance cases
here, so C2 and C3 compile them too.

**The digest moved a fifth time**, and the property held: `ba8615e4…` (N10) → `ce458be3…`.
`ir: identical`, executables byte-identical once the linker UUID is removed, and C2 = C3
with the reference over 20 conformance cases, 7 of them multi-module, at 8,959 source
lines.

### No value, as a third completion — N10.2, 2026-09-23

The two refused-program differences recorded above are closed. The Nazm checker now has the
reference's three completions — a value, no value, never finishing — in one array beside
the type, and `unknown` means only "already reported". A value position given nothing is
`N0300`; an `if` whose branches disagree, or one with a value and one without, is `N0302`;
a body and a `return` are held to the signature, which this checker had never done at all.

`crates/nazm-cli/tests/refusals/` holds forty programs, and each is checked by the reference
and by the Nazm checker compiled and interpreted, on code **and** span. All agree, and the
Nazm compiler refuses every refused one before it emits anything. One pair crosses a module
boundary: two records laid out alike from two modules are two types, `N0302` at the same
line and column in the reference and in the Nazm compiler, compiled and interpreted.

**Accepted programs did not move.** The Nazm compiler's IR for all 58 accepted programs in
the transfer corpus, the conformance corpus and `examples/` is byte-identical to N10.1's.

**What is still not the same.** When an expression has *already* failed, the two checkers
recover differently: the reference gives it the expected type, or `Int`, and may then report
a consequence; this checker gives it `unknown`, which agrees with everything, and reports
none. Every comparison above is of programs whose first error is the one being tested, and
that is the scope of the claim. The bare block of N2 is unchanged.

**The digest moved a sixth time**, with the property intact: `ce458be3…` (N10.1) →
`37785cef…`. `ir: identical`, executables byte-identical once the linker UUID is removed,
C2 = C3 with the reference over 20 conformance cases, 7 of them multi-module, at 9,194
source lines.

### Generics, and a refusal corpus — N11, 2026-09-23

The compiler written in Nazm implements N11: type parameters and applications in its parser,
parametric checking and inference in its checker, and an instance planner and `Vec[T]` in its
emitter (`architecture.md` §7.12, *The compiler written in Nazm*). It remains a program with
no recursion — substitution is a post-order walk with an explicit stack, and interning only
enqueues, because `apply → settle → complete → substitute → apply` is a call cycle the
native backend refuses and did refuse, the first time.

**What the chain now carries.** Two generic programs (`compiler/conformance/generics.nz` and
`vectors.nz`) and two generic module cases — a library exporting a generic record, enum and
function that only the importer instantiates, and one instance requested from two modules —
join the corpus. And the chain gained a half it never had: **refusals**.
`compiler/conformance/refused/` holds programs every compiler must refuse, each naming its
code on its first line — an ownership cycle through a `Vec` (`N0359`), an impossible inline
layout through a generic record (`N0336`), conflicting inference (`N0355`) and an instance
family that never ends (`N0357`) — and C2, C3 and the reference must each refuse each one with
that code. Agreement about what is accepted says nothing about agreement about what is not.

**The compiler uses what it compiles.** Its diagnostics were four parallel arrays and are a
`Vec[Diag]` now, so the chain compiles a `Vec` of records at every stage — C1 built by the
reference, C2 and C3 by the compiler itself.

**The digest moved a seventh time**, twice within the milestone: `37785cef…` (N10.2) →
`4e473d4a…` with generics → `c58b91dd…` with the dogfood migration. `ir: identical`,
executables byte-identical once the linker UUID is removed, 24 conformance cases (9 of them
multi-module) and 4 refusals agreeing across C2, C3 and the reference, at 11,877 source
lines. Fixpoint is not semantic proof; the parity tests in `crates/nazm-cli/tests/selfhost.rs`
are what compare the two compilers on meaning, and they are recorded in
`capability-matrix.md`.

**What is still not the same.** The Nazm compiler emits one LLVM module and numbers its
instances `@nz.g<n>` by the planner's worklist; the reference emits one object per instance
named from the canonical key. That is a difference in decomposition, not in meaning. Spans of
declaration-level diagnostics that are not compared — a record declared twice, a built-in
type name redeclared — still point at the whole declaration in this checker and at the name
in the reference.

### Typed errors, and a prelude every stage loads — N12, 2026-09-24

The compiler written in Nazm implements N12: `?` in its lexer and parser, the core `Result`
recognised by definition in its checker, and propagation in its emitter as the `match` it
means, leaving by `return`'s exact sequence (`architecture.md` §7.13, *The compiler written
in Nazm*).

**The prelude is part of the chain.** `library/core/prelude.nz` is the toolchain's source,
not the compiler's, so it is not one of S's files: every stage is **given** it —
`nazmc FILE CORE` — and loads it as the last module of every compilation, the compiler's own
included. C1 is built by the reference, which carries the same file's bytes; C2 and C3 read
the file. There is no stage-local definition of `Result` anywhere, and no warm cache is
involved. The record gains a `core-prelude:` line with the file's digest and length, beside
each source's.

**What the chain now carries.** A propagation program (`compiler/conformance/results.nz` —
`?` out of a loop, an argument, an initialiser, a scrutinee, a generic function and two
layers of `Result`), a module exporting a `Result` API that its importer propagates
through, and five more refusals: `?` on an `Option` and on a lookalike enum (`N0360`), `?`
in `main` (`N0361`), two error types with identical fields (`N0362`), and a project
`Result` (`N0363`). C2, C3 and the reference must each accept the first two with the same
output and refuse each refusal with its code.

**The compiler uses what it compiles.** The front end is one stage returning
`Result[Checked, Vec[Diag]]`, and the emitter's driver begins with `check_program(…)?` — so
every stage of the chain compiles, and runs, a `?` over a record of twenty handles with a
`Vec[Diag]` error.

**The digest moved an eighth time**: `c58b91dd…` (N11) → `08895900…`. `ir: identical`,
executables byte-identical once the linker UUID is removed — and on this run the runnable
forms too, with no byte differing — 26 conformance
cases (10 of them multi-module) and 9 refusals agreeing across C2, C3
and the reference, at 12,046 source lines.

**What is still not the same.** This compiler's modules are session indices, so the prelude
is identified as *the last module* rather than by a durable key; the reference finds it by
`@core/prelude`. Both are exact within a compilation and neither can be confused with a
project module — *N12.1 closed this, below*. The earlier differences — one LLVM module and `@nz.g<n>` instance names, and
whole-declaration spans for the declaration diagnostics that are not compared — stand.

### One block, and the prelude by its key — N12.1, 2026-09-24

**Both differences N12 recorded as this compiler's own are closed.**

*A block used as a value.* The parser takes `{` where an operand is expected — suspending
the expression exactly as an `if` does — and wraps the block in a `blockexpr` node, the
reference AST's `Expr::Block` and the encoding `selfhost.rs` had already written down for
it. The checker gives it its block's completion and type; the emitter treats it as
structure, like `block`. So a `match` arm, a `let` initialiser, an argument, a field and an
operand may each be a block, a statement may be a bare block, and a statement `match` may
have arms that produce nothing. The same programs compile in `nazm build`, which refused
all of them until N12.1 as well.

*The prelude's identity.* A module's `mpath` entry is its key, and `attach_core` keys the
prelude `@core/prelude`. `core_module` finds that key — once, in `check_program` — and both
consumers, the reserved-name rule and `?`'s `Result`, read the one answer. A project path in
the `@core` namespace is re-keyed out of it, so no project file can hold the key; such a
file still compiles, as it does in the reference. A harness drives `check_program` with the
prelude where the loader puts it, between two project modules, and absent, and the answers
are the same wherever it is.

**The compiler uses what it compiles.** `record_owned` returns `Option[Int]`: its four
consumers `match` on it, and four `>= 0` guards are gone — the guards whose job was to keep a
`-1` meaning *no declaration* from equalling the `-1` `definition_of` answers for a type that
is not a record. `performance.md` has the before and after.

**What the chain now carries.** `compiler/conformance/blocks.nz` — arm blocks over `Option`
and a generic `T`, a three-arm `match` where one arm returns, one produces a value and one
propagates with `?`, a block argument and a block field initialiser that leave half-way,
an all-leaving `match`, `break` and `continue` out of arm blocks, `Unit` arms in a statement
`match`, and a block producing a `Result` for `?` — a module case whose importer takes
another module's enum and an `Option` apart with arm blocks, and three refusals: arm blocks
of two types (`N0302`), a `Unit` arm used as a value and a `Unit` block used as an argument
(`N0300`).

**The digest moved a ninth time**: `08895900…` (N12) → `a9ead258…`. `ir: identical`,
executables byte-identical once the linker UUID is removed and the runnable forms too, 28
conformance cases (11 of them multi-module) and 12 refusals agreeing across C2, C3 and the
reference, at 12,164 source lines.

**What is still not the same.** One LLVM module and `@nz.g<n>` instance names;
whole-declaration spans for the declaration diagnostics that are not compared; modules
keyed by normalised rather than canonical path. The prelude is the only module whose key
is not a path.

### Derived equality, and the first enum comparison in the compiler — N13, 2026-09-25

*The checker.* `equality_blocker` in `analyse.nz` is the reference's `UserTypes::equality`
over the same table — an enum's fields are its variants and a variant's are its payload, so
one walk answers both kinds — and `check_binary` refuses `==` exactly when the reference
does, at the same span with the same code. `the_nazm_checker_agrees_with_the_reference_on_equality`
compares the two on 30 programs, 20 of them refusals.

*The emitter.* A comparison of two records or two enums calls `@nz.eq<entry>`, generated
after every function for each entry a comparison reached. A record's helper compares fields
in layout order and leaves at the first difference; an enum's compares tags, then `switch`es
to the variant's slot and hands it to the variant's own helper. Both operands are released
after the answer, as a `Str` comparison's already were.

*The compiler uses what it compiles.* A node's completion was `0`, `1` or `2` in an `Ints`,
with `-1` for "no arm yet" in a `match`'s fold. It is an `enum Completion` in a
`Vec[Completion]`, the fold is an `Option[Completion]`, and every test of one is an enum
`==` — so every stage of the chain compiles enum equality in the compiler itself.

**What the chain now carries.** `compiler/conformance/equality.nz` — records with a heap
field built separately, different and equal variants, `Option[Int]`, `Result[Int, Str]` and
`Box[Shape]` — a module case comparing an imported record, enum and `Option[Point]`, and
seven refusals, all `N0304`: a `Vec`, an `Ints`, a `Chan`, a record holding a `Vec`, an enum
with one variant holding one, a generic body over `Box[T]`, and two records of one shape.

**The digest moved a tenth time**: `a9ead258…` (N12.1) → `76598c43…`. `ir: identical`,
executables byte-identical once the linker UUID is removed and the runnable forms too, 30
conformance cases (12 of them multi-module) and 19 refusals agreeing across C2, C3 and the
reference, at 12,412 source lines.

### A type parameter that requires equality — N14, 2026-09-25

*The parser.* `T: Equality` is written into the parameter list's text after the parameter's
position — `[T@5=Equality@8]` — where every existing reader of that text stops, and the
reference parse-tree dump carries it the same way, so parser parity covers it.

*The checker.* The table's new `req` column marks a parameter whose declaration requires
equality; `has_no_equality` reads it for a parameter, which is the reference's single
assumption point; `check_generic_call` checks every argument by `equality_blocker` and reports
`N0364` at the reference's spans; an unknown capability is `N0365` and a requirement on a
record's or enum's parameter `N0101`. The emitter needed no change: instances are concrete.

*Dogfood:* none. The compiler has no generic helper that needs a capability — its searches are
over `Ints` and `Strs` — and none was invented.

**What the chain now carries.** `compiler/conformance/requirements.nz` — bounded bodies at
`Int`, `Str`, a record, an enum, `Box[T]`, `Option[T]`, `Result[T, E]`, and a forwarded call —
a module case that calls and forwards to an imported bounded function, including a
`count_same[T: Equality]` over a `Vec[T]`, and six refusals: an unconstrained `T` compared
(`N0304`), a `Vec`, a `Chan` and an `Option[Vec[Int]]` holding `None` given to a bounded
parameter, an unconstrained wrapper forwarding (`N0364`), and two bounded parameters compared
(`N0304`).

**The digest moved an eleventh time**: `76598c43…` (N13) → `0e1a40e6…`. `ir: identical`,
executables byte-identical once the linker UUID is removed and the runnable forms too, 32
conformance cases (13 of them multi-module) and 25 refusals agreeing across C2, C3 and the
reference, at 12,543 source lines.

## 6. Reproducing it

```sh
cargo xtask contained bootstrap      # builds the image if needed, writes the record
cat target/bootstrap/record.txt
```

`./compiler/bootstrap.sh` on its own **refuses to run**: the script requires an external
memory ceiling, because a stage written in Nazm allocates without bound and reaches the
machine rather than the process. Setting `NAZM_CONTAINED=1` on the host to satisfy it would
be exactly the mistake the refusal exists to prevent. `runbook.md` owns that rule.

The record names the date, host, toolchain versions, commit, whether the tree was clean,
each source with its own digest and line count, both compiler digests, and the conformance
count.
