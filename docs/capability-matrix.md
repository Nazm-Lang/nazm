# Capability matrix

**Status: source-backed against `4fb15542dfbf9af0bf34c42fe52783637b3daed4`, the commit the
retained candidate was built and tested at.**

**The capability claims remain grounded in the tested `4fb1554` implementation.** Changes
since that tested tree are documentation, test and reference wiring, comments, or other
non-capability changes. Specifically: every change under `compiler/`, `crates/nazm-lir/src`
and `crates/nazm-core/src` is comment-only — verified by diffing with comment lines removed;
the only shipping-code changes are two `evidence:` strings in
`crates/nazm-cli/src/capabilities.rs` and the `--help` description in
`crates/nazm-cli/src/main.rs`, both correcting stale prose rather than altering behaviour;
the rest is test wiring and a new `cargo xtask check` rule. No behavioural change has been made
to the compiler, and the workspace suite reports the same result. Where a row below rests on
something newer, it says so.

This is one table serving two purposes. The directive asked for a capability matrix and a
gap map; they are the same information at the same granularity, so the five fields of a
gap map are the columns of the matrix and there is one document instead of two.

**Vocabulary:** VERIFIED · PARTIAL · DESIGNED · RESEARCH · BLOCKED · MISSING, defined in
[`master-architecture.md`](master-architecture.md) §1. A VERIFIED row cites a path; a
`cargo xtask check` rule fails if it does not, or if a cited path stops existing.

**What this document does not cover.** Construct-level support — whether `while` or
`spawn` or a given built-in runs interpreted, compiles natively, or is refused — is
answered by the compiler itself:

```sh
cargo run -p nazm-cli --bin nazm -- capabilities --json
```

That output is read from the compiler's own dispatch tables (`crates/nazm-cli/src/capabilities.rs`),
so it cannot drift from the implementation the way a written table can. Ask it, not this
file. The schema is `schema/nazm.capabilities-1.json`.

---

## Summary

**Current suite: 720 passed, 0 failed, 6 ignored, across 50 suites**, run through
`cargo xtask contained tests`. The frozen count at the tested release commit is in
[`releases/4fb1554.md`](releases/4fb1554.md) and does not move.


| # | Area | Status |
| --- | --- | --- |
| 1 | Syntax, parser, CST | PARTIAL |
| 2 | Name resolution and modules | PARTIAL |
| 2a | Incremental semantic checking | VERIFIED |
| 3 | Type system | PARTIAL |
| 4 | Ownership and resource semantics | MISSING |
| 5 | Effects | MISSING |
| 6 | Capabilities (language) | MISSING |
| 7 | Provenance and information flow (language) | MISSING |
| 8 | Core IR | MISSING |
| 9 | MIR | MISSING |
| 10 | LIR and the backend contract | PARTIAL |
| 11 | LLVM backend | VERIFIED |
| 12 | Cranelift backend | DESIGNED |
| 13 | Runtime | PARTIAL |
| 14 | Structured concurrency | VERIFIED |
| 15 | Advanced scheduler | RESEARCH |
| 16 | Standard library | MISSING |
| 17 | FFI and ABI | MISSING |
| 18 | Embedded support | MISSING |
| 19 | Critical profile | DESIGNED |
| 20 | Cybersecurity profile | DESIGNED |
| 21 | AI/HPC | RESEARCH |
| 22 | Diagnostics | VERIFIED |
| 23 | Formatter | VERIFIED |
| 24 | Machine-applicable fixes | PARTIAL |
| 25 | LSP and MCP | MISSING |
| 26 | Package and build tooling | PARTIAL |
| 27 | Debugger and profiler | MISSING |
| 28 | Performance measurement | PARTIAL |
| 29 | Differential testing | VERIFIED |
| 30 | Mutation testing and fuzzing | PARTIAL / MISSING |
| 31 | Bootstrap | VERIFIED |
| 32 | Reproducibility and provenance | PARTIAL |
| 33 | Platform and target matrix | PARTIAL |

Seven VERIFIED, eleven PARTIAL, three DESIGNED, two RESEARCH, ten MISSING. The shape is
worth reading directly: **everything verified is either a front-end capability, a tooling
interface, or an evidence mechanism.** Nothing in the semantic core — ownership, effects,
capabilities, provenance — exists at all, and that is the accurate summary of where the
project stands after the first era.

---

## Front end

### 1. Syntax, parser, CST — PARTIAL

**Evidence** · Hand-written lexer and recursive-descent/Pratt parser,
`crates/nazm-syntax/src/lexer.rs` and `crates/nazm-syntax/src/parser.rs`. The grammar is a
tested artefact, not prose: `crates/nazm-syntax/tests/grammar.rs` requires every lexer
token to appear in `docs/grammar.ebnf`, every nonterminal to be reachable from `program`,
and every `(* @example *)` in it to parse with zero diagnostics. `docs/guide.md`'s syntax
section is generated from the same file by `xtask/src/guide.rs`.

*Since N1, each file is parsed on its own text through `nazm_syntax::parse_map`, so the
parser no longer requires a concatenated buffer. Since N2 the grammar has one visibility
bit — `function = [ visibility ] , "fn" , ...` — and the AST records `pub_span`, which is
where the word was written and nothing about what it means.*

**Limitation** · There is **no CST**. `crates/nazm-syntax/src/ast.rs` is a plain owned
AST; comments are returned as a parallel span list consumed only by the formatter, and
never reach the tree. Error recovery is one strategy at one granularity — skip to the next
`fn` — so a malformed statement loses its whole function body. Parse nesting is bounded at
512 with a 24 MiB parse stack.

**Next dependency** · A lossless green tree (`cstree` is the recorded choice,
`architecture.md:344`), which is also what safe structured edits and any language server
need.

**Accepted when** · A round trip through the tree preserves every byte including trivia,
and the parser produces a usable tree for a file with an error in the middle of a function.

### 2. Name resolution and modules — PARTIAL

**Evidence** · Lexical scoping with shadowing rules and a separate function namespace,
`crates/nazm-core/src/check.rs`. `use "relative/path.nz";` resolved by the driver,
`crates/nazm-cli/src/load.rs` — canonical-path keyed, read-once, depth-first, cycles
terminate rather than erroring. A second resolver written in Nazm, `compiler/module.nz`,
serves the bootstrap stages. Codes `N0200`–`N0208` in `crates/nazm-diag/src/lib.rs`.

**Resolution is singular as of N1 (2026-09-21).** `nazm_core::check` produces a
`nazm_sema::Resolution` — function definitions, call targets, and the slot every name
mention refers to — and both the evaluator and `crates/nazm-lir/src/lower.rs` read it.
Since N2 that is true of the **evaluator** as well: its `HashMap<String, usize>` over every
function, and its own copy of the built-in-first rule, are gone. The backend's last two
name lookups went with them — `crates/nazm-lir/src/lower.rs` pairs definitions with
declarations through `FnDef::decl` and finds the entry point through `Resolution::entry`,
so neither consumer knows the spelling `main`.

**Source identity is real as of N1.** `nazm_span::Span` is `{ file, start, end }` with
file-local offsets, and `crates/nazm-cli/src/load.rs` produces a `SourceMap` whose files
are parsed separately. The merged buffer, and the binary search that recovered a file from
an offset, are gone.

**There is a compilation unit as of N2 (2026-09-22).** One module is one file.
`crates/nazm-sema/src/module.rs` holds `ModuleId` and the `ModuleGraph` the driver builds
while loading; `crates/nazm-sema/src/interface.rs` holds the `UnitInterface` a module
offers. Checking is two stages, `declare_unit` then `check_unit`
(`crates/nazm-core/src/check.rs`), and the second takes its dependencies as **interfaces**.
`crates/nazm-core/tests/unit_checking.rs` checks a module against an interface for a module
whose source does not exist, and refuses the same module when that interface is empty.

**Visibility exists as of N2.** Top-level definitions are private to their module; `pub`
exports; imports are not transitive; every collision is a diagnostic naming both sides
(`N0206`–`N0208`). Twenty cases in `crates/nazm-cli/tests/visibility.rs`, most of them run
interpreted and compiled and required to agree. The self-hosted compiler was migrated
rather than exempted: 84 of its 224 functions are `pub`, 140 are private, and since N2.1 it enforces the rule as well as expressing it.

**Durable identity and persisted interfaces exist as of N3 (2026-09-22).** A module's
`ModuleKey` is its normalised path relative to the compilation's source root, and a
definition's `DefKey` is that key plus its kind and name
(`crates/nazm-sema/src/key.rs`). `nazm interface FILE` writes a module's interface as
`nazm.interface/1` — durable keys, canonical type names, no spans, no session ids
(`crates/nazm-iface/`) — and `--hash` fingerprints it with BLAKE3. The session ids are
unchanged and were not renamed; both layers coexist.

`crates/nazm-cli/tests/durable_identity.rs` establishes it across real boundaries: the same
project gives identical bytes in two processes and in two different directories, a module
whose private body changed gives the same fingerprint, and **a dependent checks against a
deserialised interface with the dependency's source deleted.** A module outside the source
root, or one a symbolic link gave two names, is refused a durable key rather than given a
guess.

**A project can declare where it begins, as of N4 (2026-09-22).** An empty `nazm.root`
marker, or `--source-root DIR`, fixes the root a module's key is relative to, so
`lib/common.nz` is the same module whichever entry file named it
(`crates/nazm-cli/src/root.rs`, ten cases in `crates/nazm-cli/tests/source_root.rs`). With
neither, the root is derived from the named file exactly as before. The marker must be
empty and is refused otherwise: it says where a project begins and owns nothing else.

**Limitation** · **Nothing is separately *compiled*.** Lowering and code generation still
take the whole program and emit one LLVM module (area 10). A module still cannot export a
name under another one, re-export, or be named by anything but a relative path.

**Next dependency** · Aliasing and re-export are language decisions; separate code
generation needs a linking model (area 10).

**Accepted when** · Met for identity, interfaces and a declared root.

### 2a. Incremental semantic checking — VERIFIED

**Evidence** · `nazm check` reuses the result of checking a module whose semantic inputs
are unchanged, as of N4 (2026-09-22). `crates/nazm-cache/` computes what those inputs are —
`SourceFingerprint` over the exact source bytes, `CheckerIdentity` over the checker's own
tables plus an explicit `SEMANTIC_EPOCH`, and `CheckKey` over both plus each direct
import's `(ModuleKey, InterfaceHash)` — and stores a `nazm.check/1` entry per module in
`.nazm/check/`, published by `rename` and validated on read.
`crates/nazm-core/src/check.rs` gained one hook, `Reuse`; `check_unit` is still the only
thing that decides what a body means.

Twenty-nine tests in `crates/nazm-cli/tests/incremental.rs`, each running the binary
repeatedly, establish the model: a private body change re-checks its own module and leaves
its importer alone; an exported signature change re-checks the importer and finds the error
it now has; in `A → B → C`, a change to C that leaves B's interface unchanged stops at B.
Sixteen more in `crates/nazm-cache/tests/store.rs` break one thing about a stored entry —
truncation, a future schema, an edited fingerprint, an edited interface, a valid entry
under the wrong name — and every one is a miss.

**What is reused, exactly** · The second stage of checking a module: its bodies. Every
module is still read, parsed and declared from source on every run, which is why a skipped
module is invisible — the interface other modules' keys depend on, and whether the
compilation has a `main`, are recomputed rather than recalled.

**Limitation** · **This is not incremental native compilation, and no such claim is made.**
`nazm run` and `nazm build` reuse nothing: both need the resolution that only checking a
body produces, and lowering is still whole-program. There is no object, IR or LIR cache, no
query database, no scheduler and no eviction policy. A module with no durable identity is
checked every time.

**And the measured gain is about a millisecond.** On the compiler's own five modules,
`performance.md` records 40.5 ms with the cache disabled against 38.9 ms warm — 18% of the
work above a 31.7 ms process floor, and 3.9% of the wall clock. The first run is **24 ms
slower**, because publishing five entries costs an `fsync` each. The decisions are right;
the saving is small because parsing and declaring happen every run and are most of the
cost. Recorded rather than averaged away: a reader deciding whether to depend on this
should know it buys correctness of invalidation, not speed.

**Next dependency** · A `CodegenKey` — implementation fingerprint, target, CPU features,
backend, optimisation level, ABI — and a per-unit code-generation boundary to key on
(area 10). `CheckKey` is not it and must not become it.

**Accepted when** · Met for semantic checking. Native reuse is a separate row that does not
exist.

### 3. Type system — PARTIAL

**Evidence** · Six types, closed, `crates/nazm-sema/src/types.rs`: `Int`, `Bool`, `Str`,
`Ints`, `Strs`, `Chan`. Local inference for `let`; parameters and returns always written.
Completion analysis decides whether a block produces a value, which is what lets one `if`
serve both statement and value position. Codes `N0300`–`N0314`. 114 behavioural tests in
`crates/nazm-core/tests/language.rs`.

**Limitation** · No generics, no traits, no user-defined types, no pattern matching, no
closures or function values, no unit type, no `Option`, no `Never`. Eight keywords are
reserved and refused by name rather than silently absent
(`crates/nazm-syntax/src/parser.rs`). This is law 12 working as intended, not an oversight
— but it means the type system has never been asked the questions that make type systems
hard.

**Next dependency** · Nothing forces the order. Records, tagged unions and generics were
**bypassed rather than delivered** — the compiler was written with parallel `Ints`/`Strs`
arrays instead. `docs/roadmap.md`, *What M2 bypassed*, records why that matters.

**Accepted when** · Chosen by the milestone that needs it, not by completeness.

---

## Semantic core

### 4. Ownership and resource semantics — MISSING

**Evidence of absence** · No ownership, borrow, move, region or lifetime machinery exists
in any language crate. The source says so: `crates/nazm-core/src/eval.rs` — *"This is
emphatically **not** a claim of ownership safety. Nothing prevents aliasing, nothing tracks
lifetimes, and nothing is ever freed."* Sequences are shared, mutable and never freed
(`docs/spec.md`). `==` on a sequence is refused (`N0304`) because it would have to mean
identity or contents and the language chose neither.

**Limitation** · Compiled programs allocate and never release: `@free` appears **zero**
times in `crates/nazm-lir/src/emit.rs` and zero times in `compiler/emit.nz`, against 11
`@malloc` and 2 `@realloc` sites in each. This is the defect behind the 2026-09-21
resource incident and the reason `compiler/bootstrap.sh` refuses to run without an
external memory ceiling. `str_join` was added to allocate *less*
(`crates/nazm-sema/src/intrinsic.rs`); nothing reclaims.

**Next dependency** · The surface model decision — mutable value semantics against the
full region calculus `architecture.md` §5 hedges with. It is the largest open design
question in the project.

**Accepted when** · A compiled program's peak RSS is bounded by live data rather than by
total allocation, demonstrated on the self-hosting compiler compiling itself.

### 5. Effects — MISSING

**Evidence of absence** · No effect row, annotation, inference or purity analysis. The
only occurrences of the word in the language crates are the English phrase "evaluated for
its effects" and `crates/nazm-lir/src/lib.rs`'s statement that there are *"no effects to
track"* — given as a reason there is one IR level rather than eight.

**Limitation** · The blocking question is open and named: **is divergence an effect?**
Without an answer, "drop an unused pure call" is unsound, so the optimiser cannot be given
that transformation at all. `docs/spec.md` *Open* carries it, along with whether allocation
failure and panic are effects.

**Next dependency** · A typed core IR to carry a row on a function type.

**Accepted when** · A function's row is inferred, checked, and a compile-fail test shows an
effectful call refused where a pure one is accepted.

### 6. Capabilities, as a language feature — MISSING

**Evidence of absence** · Authority is **ambient and labelled as such** in the source:
`crates/nazm-sema/src/intrinsic.rs` — *"These are callable from anywhere, and nothing in
the language restricts which functions may reach the file system or the process arguments.
That is a real limitation, not a design."*

**Do not confuse this with `nazm capabilities`.** `schema/nazm.capabilities-1.json` is a
*toolchain descriptor* — what one build of the compiler supports. It is unrelated to
object capabilities or capability-safety, and it is VERIFIED under area 22.

**Limitation** · `fn main(io: IO)` was written down as the plan of record and then **not**
adopted: I/O shipped as ambient authority, labelled as a limitation in `docs/spec.md`. It
remains the most likely growth path, and it remains unchosen.

**Next dependency** · Effects, or an independent capability-parameter design — the two are
separable and `architecture.md` §5 insists they stay separate concerns.

**Accepted when** · A function without the authority cannot reach the filesystem, and the
refusal is a compile error with a span.

### 7. Provenance and information flow, as a language feature — MISSING

**Evidence of absence** · No labels, lattice, sanitizers or declassification anywhere in
the language crates. `architecture.md` §5 designs a lattice-valued qualifier
(`Untrusted`, `Secret`, `PII`) and records that an earlier draft calling provenance
*"nearly free once effects exist"* was overclaiming.

**Limitation** · Every question that makes information flow usable is open in
`docs/spec.md`: implicit flows, sanitizer declaration, label propagation through a
projection, declassification authority, and what label a value from C carries.

**Next dependency** · Effects and a MIR that tracks places.

**Accepted when** · A `Secret` reaching an output is refused, with a stated and documented
approximation of what the analysis misses.

---

## IR and backend

### 8. Core IR — MISSING · ### 9. MIR — MISSING

**Evidence of absence** · Neither exists; no HIR either. `crates/nazm-lir/src/lib.rs`
states the reason directly: the levels in between *"exist to carry information M1a has
none of"* — no generics to monomorphise, no traits to lower, no regions to infer, no
effects to track. Monomorphisation is absent and cannot be otherwise, since nothing is
polymorphic.

**Limitation** · Not the absence of the levels — that is correct restraint.

**Still MISSING after N1 and N2, and deliberately.** N1 produced a `Resolution`, and a
resolution is **not an IR**: it holds no expressions and no control flow, only the answers
to "what does this name refer to", keyed by where the name was written. N2 added a
`ModuleGraph` and a `UnitInterface`, and neither holds a body — an interface is a module's
signatures, which is less than an AST rather than more. Lowering still lowers from the AST.
`nazm-sema` is not `nazm-hir`: the crate on §7's table is a graph of definitions *with*
bodies. Reading either new type as an HIR would be exactly the progress-bar mistake
`master-architecture.md` §3 warns about.

**Next dependency** · A responsibility no existing level owns. The duplicated one is gone;
the next candidate has to be argued for on its own evidence.

**Accepted when** · Introducing the level **removes** code rather than adding a
pass-through.

### 10. LIR and the backend contract — PARTIAL

**Evidence** · `crates/nazm-lir/src/ir.rs` and `crates/nazm-lir/src/lower.rs`. The
distinguishing information it carries over the AST is which operations can fail and where
the source said so; every fallible node carries a span. Signatures are lowered before
bodies, so forward references resolve without a second walk, and all refusals are
collected rather than the first. Layering is gated: `xtask/src/rules.rs` forbids
`nazm-lir` depending on `nazm-core`.

**Limitation** · It is not level 7 as `architecture.md` §2 defines it — a structured typed
tree over numbered slots, not SSA over a CFG of places. No optimisation, inlining or
constant folding happens here.

*The duplicate built-in table and the second resolver this row used to name were removed by
N1; the forbidden dependency on `nazm-core` still holds, and is satisfied by both crates
depending on `nazm-sema`, which owns the shared vocabulary since N2.*

**Whole-program, and still whole-program after N2.** `lower` takes the entire `Program` and
one `Resolution` and emits one LLVM module, numbering functions `@nz.fN` — which is why two
modules each defining a private `helper` needed no change here at all. Modules exist in the
front end and nowhere in the backend: no per-module object file, no linker symbol, no
separate LLVM module. Anything describing this as separate compilation would be wrong.

**Next dependency** · Only whatever a second backend or a real optimisation pass demands.

**Accepted when** · A second consumer exists that the contract serves unchanged.

### 11. LLVM backend — VERIFIED

**Evidence** · `crates/nazm-lir/src/emit.rs` emits LLVM IR as text; `crates/nazm-cli/src/build.rs`
hands it to `clang`. No LLVM is linked, so there is no build-time LLVM dependency. 127
end-to-end tests in `crates/nazm-cli/tests/build.rs`, each at `-O0` **and** `-O2`, against
a written expectation and against the interpreter.

**Limitation** · The arithmetic contract is the load-bearing part and is deliberately
conservative: `nsw` is not used, because it would promise overflow cannot happen and
license an optimiser to delete the trap. No debug info is emitted and `clang` is invoked
without `-g`, so a compiled program cannot be debugged symbolically.

**Next dependency** · None for correctness.

**Accepted when** · Already met for the current subset.

### 12. Cranelift backend — DESIGNED

**Evidence** · `architecture.md` §1 and `roadmap.md`, *Backends* — one component doing four jobs
(JIT, comptime evaluator, REPL, hot reload), and an explicit ordering decision that it
comes after, not instead. `xtask/src/main.rs` gates against a codegen dependency appearing
unannounced.

**Limitation** · None of the four jobs exists. A second backend now would be a second set
of semantics bugs to keep in agreement for no present benefit.

**Next dependency** · One of the four jobs becoming real.

**Accepted when** · `nazm test --backend=all` means something — which first requires
`docs/spec.md`'s backend-semantics questions to be settled.

---

## Runtime and concurrency

### 13. Runtime — PARTIAL

**Evidence** · There is no runtime crate; the runtime is LLVM IR generated into each
program, emitted lazily only when reached — `crates/nazm-lir/src/emit.rs` carries the
join, I/O, sequence, task and channel runtimes, and `compiler/emit.nz` mirrors them. The
external surface is 18 libc/pthread symbols plus three overflow intrinsics and `memcpy`.

**Limitation** · No deallocation (area 4). No `nazm-rt` crate, so no scheduler, no
allocator, no reactor.

**Next dependency** · The memory model.

**Accepted when** · `nazm-rt` exists with a narrow `unsafe` allowance that `xtask check`
enforces, per `architecture.md` §7.

### 14. Structured concurrency — VERIFIED

**Evidence** · Specified in `docs/spec.md` before implementation, then interpreted
(`crates/nazm-core/src/eval.rs`, 18 tests in `crates/nazm-core/tests/concurrency.rs`), then
compiled (17 tests in `crates/nazm-cli/tests/concurrent.rs` at `-O0` and `-O2`, each
against a written expectation *and* the interpreter), **then compiled by the compiler
written in Nazm** — `compiler/emit.nz` carries the task and channel runtime. Every test
carries a deadline, so a hang fails by name. Static rules are real: `N0320` for `spawn`
outside a `scope`, `N0321` for a sequence crossing into a task.

**Limitation** · One OS thread per task and nothing more. No M:N, no work stealing, no
fairness, no task density, no timeouts, and **no cancellation beyond closing a channel**.
`Chan` carries `Int` only. Ownership prevents data races; **that is not deadlock freedom**
and is not claimed as such (`architecture.md` §5).

**Next dependency** · None for the current model; area 15 for anything beyond it.

**Accepted when** · Met for the model as specified.

### 15. Advanced scheduler — RESEARCH

**Evidence** · `architecture.md` §5 and `roadmap.md`'s C-4/C-5 rows: M:N tasks, work
stealing, growable stacks, an event reactor, blocking-FFI handoff.

**Limitation** · None of it exists and none of it is measured. `architecture.md` records
that the "1M live tasks under 2GB" target was **withdrawn** as arithmetically unsupported —
the right precedent for anything proposed here.

**Next dependency** · `nazm-rt`, which needs the `unsafe` allowance, which needs the memory
model.

**Accepted when** · Spawn-to-first-instruction latency, context-switch cost and task memory
as **RSS, not virtual reservation, with page size recorded**, measured against a named
implementation.

---

## Libraries and interop

### 16. Standard library — MISSING

**Evidence of absence** · No `.nz` library, no prelude. Thirty-one compiler built-ins in
one table, `crates/nazm-sema/src/intrinsic.rs`, mirrored by the backend in
`crates/nazm-lir/src/ir.rs`. Every built-in the front end has, the backend lowers — there
is no coverage gap today, and a test enforces that.

**Limitation** · The table is the library. Names live in the ordinary function namespace
and cannot be redefined. There are no string escapes, so `str_from_byte` is the only way to
write a newline.

**Next dependency** · User-defined types, or the library stays a table.

**Accepted when** · A `.nz` library ships, is versioned, and is not special-cased by the
compiler.

### 17. FFI and ABI — MISSING

**Evidence of absence** · No `extern`, no ABI annotation, no such token in
`crates/nazm-syntax/src/lexer.rs`. The 18 libc symbols are emitted *by* the compiler to
implement built-ins; a user program cannot declare or call anything else. `docs/spec.md`
lists the FFI boundary under *Open*, so it is not designed either.

**Limitation** · Total. This is the single largest gap between the language and any
systems, embedded or cybersecurity use.

**Next dependency** · A settled ABI and layout story, which `docs/bootstrap.md`
deliberately left unfrozen.

**Accepted when** · A C function is declared and called, with the safety boundary stated
and the provenance question (what label a returned value carries) answered.

---

## Domain profiles

`NAZM_LANGUAGE_GOALS.md` §10 owns what each profile is *for*. `master-architecture.md`
§4 owns the invariant that makes them profiles rather than dialects, and the ladder a
profile has to climb to become real. **This section owns what exists**, which is: one
profile, unnamed, and five ambitions.

**General — PARTIAL.** What `nazm run` and `nazm build` accept today: ambient authority,
checked arithmetic that traps, structured concurrency over OS threads, no reclamation. It
is the widest profile and every other is a restriction of it. PARTIAL rather than VERIFIED
for one reason: **the interpreter's subset and the native subset are not the same** —
recursion runs under `nazm run` and is refused by both compilers. A profile whose two
implementations accept different programs is not yet a profile. *Accepted when* the two
subsets coincide, or the difference is itself declared as a profile boundary rather than
left as a gap.

**Systems — MISSING.** Would need explicit control over allocation and layout, no hidden
allocation where none is declared, and a stated ABI; would grant raw memory access and
foreign calls inside a named, narrow boundary. Nothing profile-specific exists. Blocked on
area 17 (FFI and ABI, MISSING) and area 4 (ownership, MISSING). The architectural
commitment that keeps it reachable — field access stays symbolic until the lowest level, so
layout selection remains possible — is already honoured and must be preserved rather than
undone when this profile arrives.

### 18. Embedded support — MISSING

**What it would restrict** · No allocation after startup, or none at all; a stack bound
that is computed; no I/O except through declared capabilities; a freestanding target with
no libc.

**Evidence of absence** · No cross-compilation, no target triple handling, no freestanding
target, no `no_std` equivalent, no control over allocation. `crates/nazm-cli/src/build.rs`
emits a module with no triple and lets `clang` supply the host's. The emitted runtime
declares 18 libc and pthread symbols and cannot currently be built without them.

**Next dependency** · Cross-compilation (area 33), then a freestanding runtime — which
means area 13 and therefore area 4. **A language that never frees cannot run on a device
with kilobytes.**

**Accepted when** · A program runs on a target with no libc, within a stack bound that was
computed rather than hoped for. Native frame counts do not bound stack bytes, and that
obligation is inherited here.

### 19. Critical profile — DESIGNED

**Read this before the rest.** This profile is an architectural intention. It is **not** a
claim that Nazm is suitable for any safety-related, airborne, medical, automotive or
otherwise regulated use; it is not a certification path; and no artefact in this repository
has been produced under a safety standard or assessed against one. **The architecture
anticipating a Critical profile is not evidence for one.**

**What it would restrict** · No dynamic allocation, no recursion, bounded loops with an
established bound, total functions, and every failure mode enumerated rather than trapped;
contracts and invariants checked rather than asserted.

**What exists that is genuinely relevant** · Three things, and they are real. Arithmetic is
**specified rather than inherited from the hardware** — overflow traps, and
`crates/nazm-lir/src/emit.rs` refuses `nsw` so no optimiser may delete the check. Recursion
is already **refused by both compilers**, which is the restriction this profile would want,
arrived at for an unrelated reason. Everything outside a backend's coverage is **refused by
name with a span**, never silently approximated or partially compiled.

**Missing prerequisites** · A memory model; effects; contracts; a bounded-stack story; and
a qualified toolchain, which the bootstrap explicitly does **not** provide — `docs/bootstrap.md`
§4 puts compiler trustworthiness on the list of things a fixpoint does not establish.

**Accepted when** · A proof obligation is discharged on a real Nazm program and assessed
against a named standard by someone qualified to make that assessment. Not a test count.

### 20. Cybersecurity profile — DESIGNED

**What it would restrict** · Untrusted input tracked to its uses; declassification
explicit and audited; no ambient authority; constant-time obligations honoured where they
are declared.

**Evidence** · The one real ingredient present is refuse-by-name-with-a-span rather than
silent degradation, and failures that are diagnostics rather than undefined behaviour.

**Limitation** · Against that: authority is **ambient and labelled as such in the source**
— `crates/nazm-sema/src/intrinsic.rs` says nothing in the language restricts which
functions may reach the filesystem.

**Next dependency** · Areas 6 and 7 — capabilities and information flow — both MISSING,
with every design question still open in `docs/spec.md`.

**Accepted when** · Untrusted input is tracked to its uses and declassification is explicit
and audited, with the approximation's blind spots stated.

### 21. AI/HPC — RESEARCH

**Evidence of absence** · No tensors, no GPU path, no vectorisation work, no measurement.

**Next dependency** · A reason. `architecture.md` §1 says MLIR should be revisited *only
if* Nazm pivots toward tensor/accelerator codegen, and nothing proposes that pivot.
Treating this as a profile rather than a declined direction is itself the open question,
tracked as R7 in `research-register.md`.

**Accepted when** · A workload the project actually has, that a CPU backend cannot serve.

---

## Tooling

### 22. Diagnostics — VERIFIED

**Evidence** · `crates/nazm-diag/src/lib.rs` — 33 stable codes, byte-offset spans, a
versioned JSON schema `schema/nazm.diagnostic-1.json` emitted per object. Conformance is
tested against real binary output in `crates/nazm-cli/tests/schemas.rs`, and
`crates/nazm-cli/tests/codes.rs` requires every declared code to be provoked by a test or
listed as unprovokable with a written reason. The capability inventory
(`crates/nazm-cli/src/capabilities.rs`) is read from the compiler's own tables, and
`docs/diagnostics.md` states the compatibility contract.

**Limitation** · Two codes are unprovokable by construction. Spans are offsets into a
merged buffer, so file identity is recovered rather than carried (area 2).

**Next dependency** · None.

**Accepted when** · Met.

### 23. Formatter — VERIFIED

**Evidence** · `crates/nazm-syntax/src/format.rs`, token-driven so comments survive, with
no options by design. `crates/nazm-syntax/tests/format.rs` asserts on every input that
tokens are unchanged, that `format(format(x)) == format(x)`, that comments are identical,
and that output ends in exactly one newline — applied to every `.nz` in the tree and to 15
hand-written ugly inputs.

**Limitation** · No reflow, no reordering, no insertion. Per file, not per merged program.
Canonical in the sense of "one output", not yet in the sense of a lossless tree round trip.

**Next dependency** · A CST, for edits rather than formatting.

**Accepted when** · Met for formatting.

### 24. Machine-applicable fixes — PARTIAL

**Evidence** · `crates/nazm-cli/src/fix.rs`, schema `schema/nazm.fix-1.json`. Only
`Automatic` fixes apply; nothing is written without `--apply`; edits apply right-to-left;
overlaps are skipped; and a result that no longer parses is rolled back wholesale. Ten
workflow tests in `crates/nazm-cli/tests/workflow.rs`.

**Limitation** · Stale-edit protection exists but is **by expected-bytes comparison, not a
file hash or document version**, and it is **untested** — none of its four skip paths is
asserted anywhere. The named test for it exercises a different path.

**Next dependency** · None; this is a test to write.

**Accepted when** · Each of the four skip reasons has a test that reaches it.

### 25. LSP and MCP — MISSING

**Evidence of absence** · No crate, no dependency, no protocol code. `architecture.md`
lists `nazm-lsp` and `nazm-mcp` as planned and records the intended libraries.

**Next dependency** · A CST and incremental resolution — both area 1 and area 2.

**Accepted when** · An adapter shares the compiler's semantic engine rather than
re-implementing it.

### 26. Package and build tooling — PARTIAL

**Evidence** · Multi-file builds work through path-relative `use`, two independent
implementations: `crates/nazm-cli/src/load.rs` and `compiler/module.nz`.

**Limitation** · There is no project manifest, no dependency resolution, no lockfile and no
registry. `nazm build` takes one root file, and since N3 the **source root is derived from
it** — so a module's durable identity is relative to whichever file was named. A declared
root is what would make a key independent of that, and it is a build-system feature this
project does not have.

The two resolvers still differ deliberately — the Nazm one normalises paths textually
because the emitted runtime has no `realpath`, so a symlinked file is read twice there and
once here. N3 settled what that means for identity rather than closing it: a module reached
under two names gets **no** durable key, in either implementation, so the two cannot
disagree about one (`spec.md`, *Durable identity*).

**Next dependency** · A declared source root, which is the same decision as a project
manifest.

**Accepted when** · A project builds from a manifest and the two resolvers agree on
identity.

### 27. Debugger and profiler — MISSING

**Evidence of absence** · No DWARF, no `!dbg` metadata, no `-g`; `--emit-ir` writing the
`.ll` beside the executable is the only debugging affordance
(`crates/nazm-cli/src/build.rs`). Source positions in compiled programs are baked in as
string constants at compile time. No profiler integration exists.

**Next dependency** · Line tables in the emitted module.

**Accepted when** · A native backtrace names a `.nz` line.

---

## Evidence and infrastructure

### 28. Performance measurement — PARTIAL

**Evidence** · `xtask/src/bench.rs` times five stages per program plus the compiler's own
source, with a correctness gate that refuses to time anything not producing its `.expected`
output first. One discarded warm-up, then N runs, median compared. Peak RSS captured where
readable. A regression must clear **both** 25% and 5 ms. External references are built and
timed from `bench/reference/sieve.c` and `bench/reference/sieve.py`, and a reference whose
answer differs from the Nazm program's is not timed.

**Limitation** · `bench/baseline.json` (Darwin) is **stale** and not a valid comparison;
re-recording it means running generated programs on the host, which the resource contract
forbids. `bench/baseline-linux-aarch64.json` is current. Cross-host comparison is refused
rather than attempted. Every measurement under ~15 ms is below the method's noise floor.

**Next dependency** · A Darwin machine on which running generated programs is contained.

**Accepted when** · Both baselines are current and recorded under containment.

### 29. Differential testing — VERIFIED

**Evidence** · Five independent mechanisms. `crates/nazm-cli/tests/build.rs` — two oracles
per case, both opt levels. `crates/nazm-cli/tests/concurrent.rs` — the same for
concurrency. `crates/nazm-cli/src/test.rs` — `nazm test` runs every case interpreted and
native and requires agreement. `crates/nazm-cli/tests/selfhost.rs` — the Nazm-written
lexer, parser and checker against the Rust ones, token-for-token and span-for-span, over
every `.nz` in the tree including their own sources. `compiler/bootstrap.sh` — five
conformance cases under C2, C3 and the interpreter.

**Limitation** · The interpreter is the oracle, so a shared specification error is
invisible to all five. `docs/spec.md` names what must be settled before
`nazm test --backend=all` would mean anything.

**Next dependency** · None.

**Accepted when** · Met.

### 30. Mutation testing — PARTIAL · Fuzzing — MISSING

**Evidence** · `xtask/mutations/mutations.toml` holds **68** catalogued defects;
`xtask/src/mutate.rs` injects each under a kernel advisory lock with a write-ahead backup,
requires the pattern to match exactly once, rebuilds, runs the suite and restores. It
distinguishes seven verdicts and refuses to fold a non-result into "survived" — a
distinction added after an incident that produced 19 false survivors. Lifecycle behaviour
is tested in `xtask/tests/lifecycle.rs`.

*N4 added six on 2026-09-22 and ran all six contained: four remove one input each from a
check key — the module's own source, its dependencies' interfaces, the checker's identity,
and which module it is — one stops an entry's recorded inputs being checked against the key
it is filed under, and one publishes an entry for a module that did not check cleanly. Each
is a wrong answer rather than a slow build, which is why they were worth the run.*

*N2 added two and ran four contained on 2026-09-22 — `every-definition-is-exported` and
`imports-become-transitive`, plus the two existing entries in the files N2 rewrote most, to
confirm their patches still apply. All four were caught, by 8 to 18 tests each. N2.1 added
four more against the **self-hosted** path — the `pub` bit ignored, resolution across every
module, the first ambiguous import winning, and any `main` accepted as the entry point —
and all four were caught by the parity suite. Both are targeted runs, not catalogue runs;
the row below still stands.*

**Limitation** · The **whole-catalogue run timed out unexplained** at its 3600 s deadline
without reaching injection, and is recorded as incomplete rather than as a result; the
first hypothesis (a cold build cache) was measured and is wrong. It must not be restarted
until the hour is understood. **Fuzzing does not exist** — no fuzz directory at the repository
root, no targets, no corpus.

**Next dependency** · For mutation, an explanation. For fuzzing, a decision to start.

**Accepted when** · A full catalogue run completes with every verdict accounted for.

### 31. Bootstrap — VERIFIED

**Evidence** · `compiler/bootstrap.sh` runs C1 → C2 → C3 from one source and compares two
things: the emitted IR byte for byte, and the linker-normalised executables byte for byte.
Five conformance cases under `compiler/conformance/` must agree across C2, C3 and the
interpreter. The script refuses to run without an external memory ceiling. `docs/bootstrap.md`
states what this does and does not establish, and the answer is narrower than it sounds.

**Limitation** · A fixpoint establishes self-consistency, not correctness — a compiler with
a bug that reproduces itself reaches a fixpoint too.

**The reference compiler was stale until 2026-09-22.** `compiler/bootstrap.sh` requires
`$nazm` to exist and does not build it, and inside the contained runner `CARGO_TARGET_DIR`
is `/work/target` — which `docker/contained.Dockerfile` warms with a binary at *image build
time*. Every contained bootstrap between the image's build and this date therefore used
that binary for the reference check and for stage 1, whatever the tree said, and reported
nothing about the discrepancy. The workload now runs `cargo build -p nazm-cli` first. The
fixpoint claim itself is unaffected — C1 → C2 → C3 is about the compiler written in Nazm,
and the reference check is explicitly not part of the chain — but *which front end built
C1* was not what the record said it was.

**Since N2** the compiler source states its own module boundaries: 84 of its 224 functions
are `pub`, 140 are private, and `emit.nz`, `check.nz` and `parser.nz` gained the `use` lines
for modules they were reaching through someone else's import.

**Since N2.1 the two implementations accept the same language.** For one day they did not:
the Nazm-written compiler parsed `pub` and resolved every name across the whole program, so
it accepted eight kinds of program the reference refuses and emitted an invalid module for a
ninth. It now carries a module graph, a per-definition owner and visibility, per-module
interfaces collected before any body, and a recorded resolution that emission consumes.
Fifteen rules are compared through both compilers
(`the_two_compilers_agree_on_every_module_and_visibility_rule`), and five multi-module
conformance cases agree across the reference, C2 and C3 — which is a **different** claim
from C2 = C3 and is recorded separately in `bootstrap.md`.

**Next dependency** · None.

**Accepted when** · Met.

### 32. Reproducibility and provenance — PARTIAL

**Evidence** · `scripts/release-candidate.sh` runs every gate, writes `PROVENANCE.txt` and
`MANIFEST.txt`, and validates the package against its own manifest with counts compared
both ways so a file copied in but not listed fails.

**Limitation** · Three claims, kept apart. Archive **entry metadata is normalised** — tar
sorted with zeroed owner and mtime, gzip with no timestamp. Whole-archive
**bit-reproducibility is not established and cannot be** while the provenance and bootstrap
records embed their own run date. **C2 and C3 are byte-identical within one run** after the
build-id is stripped. Separately, the source digest covers a **declared file set**, not the
tree — it omits every Cargo manifest, `Cargo.lock`, `rust-toolchain.toml`,
`docker/contained.Dockerfile`, the mutation catalogue and every `.expected` fixture, so it
cannot detect a changed dependency pin.

**Next dependency** · Moving the run date out of the digested set, if bit-reproducibility
is wanted.

**Accepted when** · Two runs at one commit produce identical archives, or the claim is
permanently retired.

### 33. Platform and target matrix — PARTIAL

**Evidence** · Two verified targets: `aarch64-apple-darwin` and `aarch64-unknown-linux-gnu`
(the latter inside `docker/contained.Dockerfile`). The emitter states its own scope, and
`crates/nazm-cli/tests/docs.rs` asserts the documented target and the emitter agree.

**Limitation** · **No cross-compilation and no target handling** — `crates/nazm-cli/src/build.rs`
has no `--target`, and the emitted module carries no triple, so `clang` supplies the host's.
Other targets are unverified rather than unsupported.

**BLOCKED, and it is a real finding** · `.github/workflows/ci.yml` runs
`cargo test --workspace` and sets neither `NAZM_CONTAINED` nor `NAZM_ALLOW_UNCONTAINED`,
while `crates/nazm-cli/tests/common/mod.rs` turns that into a panic for the four selfhost
stage builders. The workflow was last changed before the containment gate existed, so by
reading, those tests cannot pass in CI. CI also runs on `ubuntu-latest` x86_64, which is
**neither** verified target. Two candidate fixes — run the selfhost stages through
`cargo xtask contained`, or exclude them from the CI test step and say so — and choosing
between them is outside this task.

**Next dependency** · Deciding what CI is for: a gate on the verified targets, or a smoke
test on a third.

**Accepted when** · CI runs green on a stated target and the stated target is one the
project verifies.
