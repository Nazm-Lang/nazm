# Nazm

**An AI-native programming language: fast, memory-efficient, concurrent — with a compiler
eventually written in Nazm itself.**

> نظم — *systematic order, well-formed arrangement.*

Nazm starts from a claim that is popular and, as usually stated, wrong: that because AI now
writes most code, languages should be redesigned to save tokens. The evidence says
otherwise. What the evidence *does* show is that agents waste enormous effort fighting
compilers and churning over code that already works — and that is attackable.

So this repository contains two things. **The language** — an interpreter and a native
compiler over an overlapping subset, which since 2026-09-21 compiles itself to a fixpoint.
And **an agent-cost harness** built to measure specific claims about it honestly, including
the uncomfortable one: that better tooling on an existing language might capture much of
the benefit.

The language is the primary track. That was decided on 2026-09-20 and recorded in
a decision record Git retains; what comes next, in dependency order, is
[`docs/roadmap.md`](docs/roadmap.md). The harness keeps its protocol, its thresholds and
its control arm unchanged — it is now evidence for claims rather than a gate on the
project's existence.

## What is actually here

**A `.nz` program runs, compiles, and — for the compiler's own subset — compiles
itself.**

```sh
$ cargo run -p nazm-cli --bin nazm -- run examples/max.nz
25

$ cargo run -p nazm-cli --bin nazm -- build arithmetic.nz -o arithmetic
arithmetic
$ ./arithmetic          # no interpreter, no nazm process
-54

$ cargo xtask contained bootstrap   # the compiler written in Nazm, compiling itself
bootstrap complete: C2 and C3 are byte-identical, 5 conformance cases agree
```

`compiler/*.nz` is a compiler for Nazm **written in Nazm** — lexer, parser, checker and
LLVM emission. The bootstrap source is five files and **5,041 lines**; `compiler/*.nz` is
5,272 across eight, the difference being three thin command-line drivers, one per stage.
Since 2026-09-22 it states its own module boundaries — 84 of its 224 functions are `pub`
and 140 are private — and **enforces them**: fifteen module and visibility rules are run
through both compilers and required to give the same answer. Every component is differentially verified against the Rust
implementation rather than merely run. It compiles itself to a fixpoint;
[`docs/bootstrap.md`](docs/bootstrap.md) says what that does and does not establish,
and the answer is narrower than it sounds.

`nazm build` is the native path, and its subset is deliberate:
several functions over `Int` and `Bool` with `fn main() -> Int` as the entry point,
literals, `let` and `let mut` bindings, immutable parameters, assignment, `while` with
`break` and `continue`, `return`, unary `-`, `+ - * / %`, the comparisons
`== != < <= > >=`, `if` in **both statement and value position**, and non-recursive
direct calls — plus `Str` as a byte sequence, the `Ints` and `Strs` growable sequences,
and the outside-world built-ins (arguments, files, output, exit), and structured
concurrency — `scope`, `spawn` and the bounded `Chan`, compiled to one POSIX thread per
task. `docs/roadmap.md` carries the conformance table. What is left outside — recursion,
bare blocks used as expressions, records, modules — is **refused by name** after
ordinary checking, with no partial compilation, and still runs under `nazm run`. The arithmetic the specification
promises is preserved rather than inherited from the hardware: overflow is a checked
intrinsic and a branch (never `nsw`, which would let the optimiser delete the check),
`Int::MIN / -1` fails, and `Int::MIN % -1` is `0`. See
[`docs/roadmap.md`](docs/roadmap.md).

**A compiled program has no iteration budget.** `nazm run` stops a runaway loop after
`--max-iterations` and says so (`N0402`); a `nazm build` executable does not stop. The
limits are the interpreter's and never were the language's — `docs/spec.md` says so — and
the native backend has not adopted a replacement. The two implementations agree on what a
*terminating* program produces, which is the claim the test suite makes and the only one
it is entitled to.

```nazm
fn first_pair_with_product(target: Int, limit: Int) -> Int {
    let mut a = 2;
    while a < limit {
        let mut b = a;
        while b < limit {
            if a * b == target {
                return a * 100 + b;
            }
            b = b + 1;
        }
        a = a + 1;
    }
    0 - 1
}
```

An executable subset — functions, `let` and `let mut`, assignment, `while` with `break`
and `continue`, `return`, `if`/`else`, arithmetic and comparison over `Int`, `Bool` and
`Str`, the `Ints` and `Strs` sequences, file and output built-ins, modules, and structured
concurrency — lexed, parsed, type-checked and evaluated. **Recursion runs here and is
refused by `nazm build`**; that asymmetry is the one place the two paths disagree, and it
is reported by name. Still absent: user-defined types, pattern matching, generics and
floats. (Effects were in this list until N36: a function may now declare `! { io }` or
`! {}` — see [`docs/spec.md`](docs/spec.md), *Effects*. Since N37 one that declares a set must
also hold the authority for it, an `IoCap` or `SpawnCap` handed down from `main` — *Capabilities*;
the check is static and is not a sandbox.) A program using one of those is **refused by name** rather than misparsed into
something else.

The semantics it commits to are in [`docs/spec.md`](docs/spec.md): overflow traps
identically in every build, division by zero is an error, bindings are immutable unless
written `let mut`, parameters are never mutable, assignment is a statement so `if (x = 1)`
does not parse, shadowing is allowed in a nested scope and refused in the same one,
arguments evaluate left to right, and a function's value is its final expression.

There is **one** block and **one** `if`. A block is statements followed by an optional
tail expression — an expression with no `;` after it — and whether a block or an `if`
produces a value is decided when checking, not by its syntax. So `break`, `continue` and
`return` need no second form of anything: a branch that leaves takes the other branch's
type, and a body where every path returns needs no tail. Code after a `break` is an error
rather than a warning — relaxing an error later is a compatible change, tightening a
warning is not.

One mistake produces one error. A syntax error inside a function body does not delete the
function: the signature survives, so calls to it still check against real types rather
than reporting that it does not exist.

`nazm run` stops a non-terminating program after a configurable number of loop iterations
(`--max-iterations`) or nested calls (`--max-depth`), and the parser refuses a syntax tree
more than 512 deep — measuring the tree as it builds it, node by node, not guessing from
how deeply it recursed. Those are different numbers: `((((1))))` recurses four times and
builds a tree of depth 2, while `1 + 1 + …` is parsed by a loop that never recurses and
builds one level per operator. Both are bounded, separately.

Three guards because there are three ways to run out and none implies the others:
`fn f(n: Int) -> Int { f(n) }` never loops, and a deep expression adds no call frames at
all while recursing through the parser, the checker, the evaluator and the destructor of
the tree.

The pipeline runs on a thread whose stack the interpreter sets itself, **sized from the
depth budget it is about to enforce**, so raising `--max-depth` raises the reservation with
it and the bound holds above the default rather than only at it. A machine that will not
grant the reservation reports it; a budget no machine would grant is refused with the
figure. The tests parse the deepest tree the parser accepts — four shapes, each edge found
by asking the parser — on exactly the stack the contract documents.

Alongside it, a benchmark harness. `crates/nazm-bench` measures what it costs an agent to
build and maintain small backend services, with a validated execution path for its
development fixtures — containers, a restricted database role, verified cleanup. It cannot
produce a benchmark number yet: there is no task corpus, no agent driver, and no provider
adapter.

The harness was built first, and that was the right order: an instrument has to be shown
to work before anything it measures means anything. It is now maintained rather than
extended, until the claims in the roadmap need it.

## The argument

The "terse languages save tokens" figure that motivates most AI-native language projects
comes from Rosetta Code problems — tasks solvable in 70–109 tokens. On real work it
inverts:

- [Dan Luu](https://danluu.com/pl-tokens/) ran agents at full effort on a zstd decoder and
  on Pandoc. The variable that predicted cost and correctness was language **popularity**,
  not density.
- ["The Best Programming Language for Tokenmaxxing"](https://arxiv.org/html/2607.22807v1)
  — 2,000 trajectories, 5 models, 4 languages — found terse OCaml consumed **1.3–1.7×
  Python's output tokens**. Problem identity explained 73–97% of cost variance; language
  was second-order.
- The Stack v2 holds roughly 200GB each of Java and Python, and **12GB of Rust**. A new
  language starts at zero, which is the worst possible position on the variable that
  actually predicts cost.

The diagnosis underneath is still right, just misattributed. Agents burn budget on compile
errors in unfamiliar languages, on rewriting code that already passes, and on cosmetic
churn. None of that is fixed by shorter keywords. All of it is attackable by making invalid
programs unrepresentable, closing the check loop in milliseconds, and giving formatting no
degrees of freedom to churn over.

**Whether that is worth a new language, or just better tooling for TypeScript, is a real
question and the harness is built to keep answering it honestly** — see
[`docs/research/evaluation.md`](docs/research/evaluation.md) for what changed about how the answer is used.

## The experiment

Three arms. The third is the hypothesis; the second is why the experiment is honest. It
measures specific claims — it no longer decides whether the language gets built
([`docs/research/evaluation.md`](docs/research/evaluation.md)).

| Arm | Contents |
| --- | --- |
| A1 | TypeScript, baseline tooling |
| **A2** | **TypeScript + improved agent tooling** — structured diagnostics, canonical formatter, repeat-edit detection |
| A3 | Nazm + comparable tooling |

If A2 captures most of the benefit, that is a finding about the claim under test and it
stays in the record. The decision rule, its thresholds and the cost policy are unchanged,
and are written down **before** the evaluation runs — see
[`docs/research/evaluation.md`](docs/research/evaluation.md).

First workload: small HTTP/JSON services on PostgreSQL — validation, transactions,
authorization, error handling, cancellation. First user: a backend developer using coding
agents to build and maintain services, so debugging and operations stay in scope rather
than just "generated programs pass tests".

## The design it is being built toward

Documented in [`docs/architecture.md`](docs/architecture.md), and staged in
[`docs/roadmap.md`](docs/roadmap.md). In brief: memory safety with no lifetime annotations
and no garbage collector; effect rows, capabilities, and position-sensitive taint so
untrusted data cannot reach a code-construction site; structured concurrency where data
races are a compile error; and a four-tier compilation strategy where `nazm check` — no
codegen at all — is the loop agents actually live in.

*That first clause read "mutable value semantics so memory safety needs no lifetime
annotations" until N7 (2026-09-22), when the memory model was settled from an audit rather
than from the hypothesis. It is not mutable value semantics: sequences alias, observably
and by specification, and `spec.md`'s memory constitution reclaims their storage without
changing that. `research-register.md` R1 records what happened to the hypothesis and where
it is still live. **N8, the same day, finished the job for the types that exist**: strings
and channels are reclaimed too, with no annotation, no collector and no source change to
the compiler written in Nazm. It is not whole-language leak freedom and
`capability-matrix.md` area 4 says exactly what it is.*

The answer to "does this need a new compiler design?" is **new front end, new IR, new
middle end, reused back end**. Code generation is decades of work with no novelty available
in it.

**The design it was built toward, and where that stands.** M1, M2 and M3 below are all
complete as of 2026-09-21; the table is kept as the record of the order they were done in.
What exists now, area by area with evidence, is
[`docs/capability-matrix.md`](docs/capability-matrix.md):

| | |
| --- | --- |
| **M1** | A native vertical slice. The existing lexer, parser, checker and diagnostics, one new typed IR, LLVM IR emitted as text and assembled by `clang`, one target. A compiled program runs with no interpreter, and is differentially compared against the interpreter on the existing 98-case corpus (114 cases today) |
| **M2** | The facilities a compiler actually needs — value/copy/move and cleanup first, then strings, tagged unions, collections, arena-indexed recursive structures, modules, and a narrow capability-based I/O contract |
| **M3** | The bootstrap: `Rust → C1`, `C1 → C2`, `C2 → C3`, with byte equality asserted only where determinism has been established |

**Superseded 2026-09-21.** This paragraph read: *"Self-hosting is the long-term objective
and it is M3, not next. Nazm today has no collections, no user-defined types, no pattern
matching, no string concatenation and no I/O, so a compiler cannot be written in it in any
form."* Four of those five are now false — collections, string concatenation, modules and
I/O all exist — and the compiler has been written in it. Only user-defined types and
pattern matching remain absent, and the compiler was written around them with parallel
`Ints`/`Strs` arrays rather than waiting for them. The Rust implementation is still
retained as the bootstrap seed and the differential oracle, which has not changed.

## What this does not establish

Stated up front, because these are easy to blur:

| Mechanism | Does **not** establish |
| --- | --- |
| Ownership / exclusivity | Business-logic correctness. Two valid transactions can still oversell inventory |
| Provenance / taint | Authorization. Correctly-parameterised SQL can still read another tenant's rows |
| Effect rows / purity | That an edit preserves behaviour — and not, alone, that a call can be dropped or reordered |
| Capabilities (static, N37) | Isolation at runtime. Nothing sandboxes the process, and a function with no declared effects still uses its caller's authority |
| Provenance (static, N38) | Non-interference or confidentiality. Only explicit data flow is tracked, one sink is restricted, and containers are coarse |
| Definition-hash equality | All behaviour-preserving rewrites. Only one class: normalised-Core alpha-equivalence |
| Foreign functions (N42) | Anything about the C side. Only `Int` and `Bool` cross and authority is required, but a C function can corrupt memory, block or never return |
| Restriction profiles (N47) | Certification, sandboxing or a security property beyond the rule. Each rule refuses a stated fact and nothing else |
| Reproducible builds (N46) | Identity across hosts, operating systems or toolchain versions. Two builds on one host are byte-identical |
| Passing the test suite | Correctness |
| Reproducible bootstrap | Compiler trustworthiness |
| Differential testing | Absence of bugs shared by all implementations |

"Verified-correct" does not appear anywhere in this project. The phrase is **"passes
independent acceptance tests and specified static checks."**

## Build and check

```sh
cargo build --workspace
cargo xtask contained tests
cargo lint              # clippy; CI runs this same alias, so they cannot drift
cargo xtask check       # architectural gates, and the guide against the grammar
cargo xtask guide       # rewrite docs/guide.md from docs/grammar.ebnf

# Not in the default set: these drive nested cargo builds and kill real process
# groups, and `cargo xtask mutate` would otherwise pay their cost once per entry.
cargo test -p xtask --test lifecycle -- --ignored --test-threads=1

# Each catalogued defect is injected and must be caught. One full build and test run
# per entry, so `--only NAME` selects a subset by substring.
cargo xtask contained mutate
```

The toolchain is pinned to 1.98.1 in `rust-toolchain.toml`. That pin is load-bearing:
`cranelift` 0.135 declares MSRV 1.95.0, so the 1.93.1 an earlier draft assumed could not
have built the eventual backend. The workspace's declared MSRV floor is 1.85; `xtask`
declares 1.89 for itself, because it uses std's file locking and a development tool has no
business raising the floor for the crates that ship.

## The commands

| | |
| --- | --- |
| `nazm init DIR [--template T]` | A new project from a template — `cli`, `library`, `server`, `embedded`, `wasm` or `contract` — whose README names the commands that check, test, build and document it; an existing non-empty directory is refused (N74) |
| `nazm run FILE` | Check and evaluate, printing the value of `main` |
| `nazm check FILE` | Check without evaluating. Reuses the result of checking a module whose source and dependencies' interfaces are unchanged; `--no-cache` checks everything, `--cache-report` says what it did, `--summary-json` prints one compact `nazm.command-summary/1` — status, exit status and a reference to each diagnostic, no prose |
| `nazm build FILE` | Compile to a native executable. Each module becomes its own object, and an object whose exact LLVM input, backend and codegen configuration were compiled before is reused instead; `--no-cache` compiles everything, `--build-report` says what it did, `--summary-json` prints one `nazm.command-summary/1` with the artifact written. The link runs every time. `--backend llvm\|cranelift` picks the code generator (N41), `--target` must be the host, `--link FILE` adds C objects (N42), `--locked` and a package directory build a package against its lockfile (N46), `--profile NAME` holds it to a restriction profile (N47), `--debug` writes DWARF — under either backend since N91 — and `--timings` reports each phase (N48) |
| `nazm test PATH…` | Run every `NAME.nz` with a `NAME.expected` beside it, interpreted **and** compiled. `--summary-json` prints one `nazm.test-summary/1`: counts, and only the cases that did not pass, with how each leg ended |
| `nazm fmt FILE…` | Lay files out canonically. `--check` writes nothing and fails if any would change |
| `nazm fix FILE` | Apply the repairs the compiler already knows about. Nothing is written without `--apply`, and only fixes marked `automatic` are ever applied |
| `nazm capabilities` | What this build supports, read from the compiler's own tables |
| `nazm obligations FILE` | Every obligation of a checked program, `nazm.obligations/1` (N87): each contract clause, call of a function with a precondition, overflow, division, index, allocation, call through a function value and call into C — proved (with its witness), checked as the program runs, or unknown. Compiler evidence, not certification; `--json` |
| `nazm core-ir FILE` | What a checked program became — its Core IR, `nazm.core-ir/1`: typed, every name an identity, every transfer explicit. A debugging view of what `nazm run` and `nazm build` both start from; `--no-spans`, `--json` |
| `nazm explain-cost FILE` | Each function's allocation, retain, block, channel, foreign and task sites, read off MIR, with a count per call — `at-most-once` or `unknown` (N52); `--json` for `nazm.cost/1` |
| `nazm explain-flow FILE` | Every sink — a path written to, standard output, a file's data — and every declassification, with the origins that reach it (N52); `--json` for `nazm.flow/1` |
| `nazm inspect FILE` | One program's facts as `nazm.inspect/1`, one deterministic line: the toolchain's identities, the package and its lock, and per definition its identity, types, effects, capabilities, provenance, Core IR and MIR digests (N48); `--def NAME` for one |
| `nazm lock DIR` | Resolve the package in `DIR` (its `nazm.toml`) and write `nazm.lock`, `nazm.lock/1`: every package's exact version, source and BLAKE3 digest, in name order, rewritten only when its bytes change. `nazm check`, `build` and `run` take a package's directory, and `--locked` holds them to its lockfile (N46) |
| `nazm update DIR [NAME…]` | Re-resolve the package in `DIR` with its lockfile's registry versions no longer preferred — only the named packages', or every one's — print each change as `name old → new`, and write `nazm.lock`. Resolution backtracks and explains a refusal (N90) |
| `nazm profile FILE` | Build the program and run it once with its memory and scheduler counters on; print `nazm.profile/2`: exit status, wall time, output size and the counts (N58). `--sample [--interval-ms N]` also samples the run with macOS's `sample` and attributes each sample to a Nazm function and `.nz` line, the runtime, foreign code or the system, waits counted as blocked (N91) |
| `nazm publish DIR --registry REG` | Copy the package in `DIR` into the local registry `REG` and record its version and digest in `REG/index/NAME.toml`, `nazm.registry-index/1`; a published version never changes (N56) |
| `nazm publish … --dry-run` | Every check a publish makes, and what it would record — `nazm.publish-plan/1`: name, version, digest and files — writing nothing (N74) |
| `nazm doc PATH [-o DIR]` | A program's or package's public API from its semantic definitions, `nazm.api-doc/1`: each exported function, record and enum with its signature, effects, capabilities, foreign symbol or export and a link to its source line; `-o` also writes `api.md`, anchored by identity (N74) |
| `nazm bindgen HEADER [-o FILE]` | The safe subset of a C header — `int64_t`, `bool` and `const char *` arguments — as `extern "C"` declarations the checker has held; everything else refused with the reason, never widened; `nazm.bindgen/1` with `-o` (N74) |
| `nazm build … --provenance FILE` | After a successful build, `nazm.provenance/1`: sources, packages, toolchain, runtime, backend options, profiles and output digests; `--attest-with CMD` keeps a local signer's output beside it (N73) |
| `nazm yank NAME@VERSION --registry REG` | Withdraw a published version: never chosen anew, still used where a lockfile names it; `--undo` restores it (N56) |
| `nazm runtime build -o DIR` | Compile the whole runtime for a target and profile once, into `DIR/libnazmrt.a` with a `nazm.runtime/1` manifest; `nazm build --runtime DIR` links it instead of generating one, and refuses one that does not fit; `nazm runtime verify DIR` checks it (N82) |
| `nazm lir FILE` | What LIR decided for a native build — `nazm.lir/1`: the target's data layout, every unit's symbols and linkage, every record's and enum's byte layout on that target, and which types own anything; the tables both backends read, validated. `--target TRIPLE` (N81) |
| `nazm mir FILE` | What `nazm build` lowers a checked program to — its MIR, `nazm.mir/1`: each concrete function (generic instances included) as basic blocks with every copy, move, drop, scope join and failure edge written out, after the validator accepted it. A debugging view; `--spans`, `--json` |
| `nazm contract FILE [--txs FILE]` | A contract under the `web3` profile: its state, entrypoints and read/write sets as `nazm.contract/1`, or a deterministic run of transactions on the interpreter as `nazm.contract-run/1` — reverts, a step meter, and conservation of a declared quantity (`N0396`) (N69); `--evm DIR` compiles it to EVM bytecode with its ABI, storage layout and a py-evm harness, `--upgrade-check OLD` checks a storage upgrade (N70) |
| `nazm accel FILE KERNEL --input FILE` | Map a pure `(Int) -> Int` function over `Int`s on the GPU through OpenCL (macOS); `nazm.accel/1` names every transfer, the synchronisation, timings and the kernel's identity, and holds the results to the interpreter's; `--print-kernel` shows the OpenCL C (N67) |
| `nazm repl` | Definitions and expressions a line at a time, checked as one program on every edit; a redefinition that breaks another definition is refused by its name (N65) |
| `nazm comptime FILE FUNCTION` | Evaluate a parameterless pure function under the interpreter's budgets; anything that could reach the host is refused (`N0393`) (N65) |
| `nazm reload-check OLD NEW` | `nazm.reload-check/1`: per definition, whether a running image could take the change or needs a restart, and why (N65) |
| `nazm interface FILE` | Write what that module offers, durably — `nazm.interface/1`, or its fingerprint with `--hash`. Not a cache: it reuses nothing |
| `nazm resolve FILE` | Each module's resolved unit, `nazm.resolved/1`, one per line: its imports, definitions and every resolved occurrence by durable identity — what its check entry stores; a reused module's is read, not recomputed (N77) |
| `nazm references DEF FILE` | Every occurrence of one entity — `DEF` a durable key, `module::kind name`, with `.member` for a variant, field or payload field — read from the modules' resolved units, `nazm.references/1`; a module whose check is reused is not checked again, and the output says how many were (N77) |
| `nazm context --root FILE --file FILE --byte N` | One definition's semantic context packet, `nazm.context/1`: the exact source of the function, record or enum declared or used at that byte, and compact links to what it uses, what uses it, what it calls and what calls it — in the root's compilation, read from disk. `--json` prints the canonical compact form |
| `nazm snapshot --root FILE` | The root compilation's semantic snapshot, `nazm.snapshot/1`: every durable function, record and enum by identity, with one digest per section the compiler records — source, shape, dependencies, related types, references, callers, callees, diagnostics. Equal digests are not equal behaviour. `--json` prints the canonical compact form |
| `nazm delta --root FILE --baseline FILE` | What changed since a kept snapshot, `nazm.delta/1`: definitions added, removed, and changed, naming which recorded sections changed — a comment or a literal is `source` alone, a rename is a removal and an addition. A baseline of another root or compiler, or not exactly `nazm.snapshot/1`, is refused. `--json` prints the canonical compact form |
| `nazm diagnostics --root FILE` | The root's current diagnostics compactly, `nazm.diagnostic-index/1`: each one's id, code, severity, file and byte range, owning definition and fix counts — no message or help text. `--detail ID --state DIGEST` prints one of them in full, `nazm.diagnostic-detail/1`: the diagnostic exactly as `nazm.diagnostic/1` has it, its places in their files, and each fix's `nazm patch fix` selector where one exists. `nazm check --json` is unchanged |
| `nazm docs --index` | Nazm's own documentation as stable sections, `nazm.docs-index/1`: every document with its authority — normative spec, grammar, architecture, evidence, planning, goal, and the rest — and every section's id, title, parent and digest, no body. `--section ID` prints that section's own canonical text byte for byte (`nazm.docs-section/1` with `--json`); `--state` refuses it if the documentation changed since the index. Ids are exact; there is no search. Reads the repository's `docs/`, not a project's |
| `nazm repo --map` | The repository's context map, `nazm.repository-map/1`: every package and target, source root, compilation root, module and durable definition, document, schema and mutation profile, with its authority, its structural relationships and where to retrieve more — no body. `--task --seed ID [--intent understand\|edit\|diagnose\|test]` prints the minimum context for a task, `nazm.task-context/1`: each item with why it is there, the seed's exact source and the selected sections' exact text, within `--max-entities`/`--max-source-bytes`/`--max-doc-bytes`; a cut makes it `partial`, and `--state` refuses a repository that changed since the map. Seeds are ids, never paths |
| `nazm patch rename --root FILE --file FILE --byte N --new-name NAME` | A planned rename, `nazm.patch/1`: the compiler's validated rename of the local or private definition at that byte — and, within a declared workspace (a `nazm.root` marker, `--source-root` or an application package), of an exported one across every module under the root (N89); never a library package's API — as minimal exact-byte edits bound to the root's snapshot, each file's digest and each edit's expected bytes. Plans; never applies. `--json` prints the canonical compact form |
| `nazm patch apply PLAN [--base DIR]` | Applies a `nazm.patch/1` plan to disk, all or nothing (N89): every file must hash to the plan's digest and hold each edit's expected bytes, or nothing is written and the differing file is named; each file written through a temporary file and a rename |
| `nazm patch fix --root FILE --file FILE --byte N --fix I` | A planned fix, `nazm.patch/1`: fix `I` of those the current diagnostics at that byte carry, with its applicability and precondition, bound the same way. Plans; never applies |
| `nazm lsp` | Serve the Language Server Protocol to an editor over stdin and stdout: diagnostics, definition, hover, references, safe rename of locals and private definitions, completion of values, call heads and types (invoked on an identifier), completion of a field after `.`, a variant after `E.` and a construction's or pattern's label — also with nothing written yet, right after the `.` or `(`, where the compiler can tell what it belongs to, and triggered by `.`, signature help for calls of functions and built-ins and for record and variant constructions, document symbols (one file's declarations, nested, in source order) workspace symbols (the declarations of every file the open documents' programs load — not a directory scan) full-document semantic tokens (every identifier the checker resolved, by kind and whether it declares; not keywords, comments or literals) quick fixes (the fixes the compiler attaches to current diagnostics, as versioned edits the editor applies) and call hierarchy (a function's direct callers and callees, from the calls the checker recorded in the program the request came from — not every caller in a project), over unsaved buffers synchronised by ranged edits — only the item an edit reaches is reparsed (N76) — from the same loader and checker as every other command, across a package's dependencies. Nothing else is offered. `editors/vscode` is a client for it (N74) |

An empty `nazm.root` file marks the directory it sits in as the project's source root, so a
module keeps one identity whichever file a compilation started from. `--source-root DIR`
says the same thing without writing a file. With neither, the root is the directory of the
file you named. The marker must be empty: it says where a project begins and owns nothing
else.

`--json` on any of them emits machine-readable output under a **named, versioned
schema**; the schemas are in [`schema/`](schema) and the compatibility rules are in
[`docs/diagnostics.md`](docs/diagnostics.md). `scripts/repair.sh` is the whole repair
loop as one command: apply every automatic fix until a pass changes nothing, lay the
result out, and report what is left as JSON.

### An MCP server, read-only

`nazm-mcp` (a separate binary: `cargo build --release -p nazm-mcp`) serves eleven Model Context
Protocol tools over stdio, each answering from the compilation of the root it was started with,
read from disk at every call: `nazm.semantic_context`, the same `nazm.context/1` packet as
`nazm context` for a file and byte offset; `nazm.semantic_snapshot`, the same `nazm.snapshot/1` as
`nazm snapshot`; and `nazm.semantic_delta`, the `nazm.delta/1` against a snapshot the client kept
and passes back as data — the server keeps none; and `nazm.semantic_patch`, the same
`nazm.patch/1` as `nazm patch`, a planned rename or fix it never applies; and `nazm.diagnostics`
and `nazm.diagnostic_detail`, the same index and detail as `nazm diagnostics`; and
`nazm.command_summary`, the `nazm check --summary-json` of its root (it neither builds nor runs
tests); and `nazm.docs_index` and `nazm.docs_section`, Nazm's own documentation by section, as
`nazm docs` gives it — by id, never by path; and `nazm.repository_map` and `nazm.task_context`,
the repository's entities and the minimum context for a task, as `nazm repo` gives them — seeds
are ids, never paths. It writes nothing and reads no file that compilation does not load, beyond
the fixed documentation corpus and the repository's fixed inputs. An MCP client starts it as a stdio command:

```json
{ "command": "nazm-mcp", "args": ["--root", "/absolute/path/to/root.nz"] }
```

[`docs/mcp.md`](docs/mcp.md) says what it answers and what it does not.

### Token counts under several tokenizers

`nazm-tokens` (a separate tool: `cargo build --release -p nazm-tokens`) counts the exact text on
standard input under four pinned tokenizers of three families — OpenAI byte-level BPE
(`cl100k_base`, `o200k_base`), SentencePiece BPE (Mistral 7B v0.1) and SentencePiece Unigram
(T5-small) — offline, as `nazm.token-cost/1`: `nazm repo --task --seed ID --json | nazm-tokens
measure`. It measures; it decides nothing, and nothing that ships depends on it
([`docs/architecture.md`](docs/architecture.md) §7.36).

### An agent task benchmark

`nazm-agent-bench` (evaluation tooling: `cargo build --release -p nazm-agent-bench`) puts eight
repository tasks — understand, edit, diagnose, documentation, test selection — to a real model
twice, once with the naive files and once with `nazm repo --task`'s context, and scores each
answer fact by fact against the repository's own authorities, with usage and dated prices kept
apart. On `claude-haiku-4-5-20251001` through the Claude Code client, two trials, the N33 context
solved 12 of 16 requests to the naive arm's 6 with 94 % fewer input tokens; the 123-byte
diagnostic stays the one task where raw source is cheaper
([`docs/performance.md`](docs/performance.md)). The OpenAI and Anthropic adapters bring their own
key; the steps are in [`docs/runbook.md`](docs/runbook.md). Nothing that ships depends on it.

## The documents

| | |
| --- | --- |
| [`docs/capability-matrix.md`](docs/capability-matrix.md) | **Start here for "does X exist?"** Every area, with evidence — the only status authority, and the one with a gate under it |
| [`docs/master-architecture.md`](docs/master-architecture.md) | The durable constitution: the laws, the evidence vocabulary, and **context routing** — which documents to read for which task |
| [`docs/NAZM_LANGUAGE_GOALS.md`](docs/NAZM_LANGUAGE_GOALS.md) | Where Nazm is ultimately going and why: the historical barriers, the long-term goals, the token-efficiency constitution. **Goals, not evidence** |
| [`docs/guide.md`](docs/guide.md) | The language, for someone writing a program in it. Its syntax section is generated from the grammar |
| [`docs/spec.md`](docs/spec.md) | What each construct means, and the decision behind it |
| [`docs/architecture.md`](docs/architecture.md) | The compiler design record: IR levels, type system, layout boundary, toolchain choices |
| [`docs/roadmap.md`](docs/roadmap.md) | What comes next, and why in that order |
| [`docs/research-register.md`](docs/research-register.md) | Hypotheses, each with what would invalidate it. Not plans |
| [`docs/diagnostics.md`](docs/diagnostics.md) | The machine interface: schemas, fixes, capabilities, exit statuses |
| [`docs/mcp.md`](docs/mcp.md) | `nazm-mcp`, the read-only MCP server: its tools, its root, and what it reads |
| [`docs/bootstrap.md`](docs/bootstrap.md) | Self-hosting: how it was designed, what it does, and what it does **not** establish |
| [`docs/performance.md`](docs/performance.md) | What has been measured, and the noise floor under it |
| [`docs/runbook.md`](docs/runbook.md) | **Containment and resource safety. Read before running anything heavy** |
| [`docs/releases/`](docs/releases/) | Frozen evidence, one file per tested commit |
| [`docs/research/evaluation.md`](docs/research/evaluation.md) | The comparative agent-cost protocol, with frozen thresholds |

## What is not built yet

Almost everything. Specifically:

- **The language is an interpreter and a native path over *almost* the same subset** —
  since N49 recursion compiles too, and what separates them is the interpreter's iteration
  budget and the native-only foreign call (`nazm capabilities` lists both). Lexer, parser,
  type checker, one lowering to Core IR (`nazm-cir`), and an evaluator over Core IR;
  `nazm build` compiles everything in the list above from the same Core IR. Two IR levels
  (Core IR and `nazm-lir`), one backend (LLVM IR as text → `clang`), two verified
  targets (`aarch64-apple-darwin` and `aarch64-unknown-linux-gnu`, the latter inside the
  contained runner); other targets are **unverified rather than unsupported**, and there
  is no cross-compilation at all. Run `nazm capabilities` for what this build actually has,
  read from the compiler's own tables rather than from this paragraph. `-O0` is still the
  default. Measurements now exist — one profile-driven optimisation pass and two recorded
  baselines, of which only the Linux one is current — and
  [`docs/performance.md`](docs/performance.md) states what they do and do not support.
  `docs/spec.md` keeps its sections
  versioned, so a *Settled for v0.1* entry records what v0.1 decided rather than what is
  true now. Everything under *Open* is undecided.
- **Concurrency is an initial model, and only that.** `scope`, `spawn` and a bounded
  `Chan` of `Int`, specified in `docs/spec.md` before being implemented, running
  interpreted and compiled. **One operating-system thread per task and nothing more**:
  no M:N scheduling, no work stealing, no task density, no fairness, no deadlock
  freedom, and no cancellation beyond closing a channel. **Superseded 2026-09-21:** this
  entry used to end *"The compiler written in Nazm does not accept it — concurrency is
  outside the bootstrap subset, and says so by name."* It does accept it. The Nazm-written
  compiler parses, checks and lowers `scope`, `spawn` and `Chan`, emitting the same task
  and channel runtime, and the exclusion list that enforced the old statement is now
  asserted by test to be **absent**.
- **The harness is partial.** Real, tested, and mutation-checked (`cargo xtask mutate`,
  **51** catalogued defects, 48 caught with 0 survivors and 2 not injected **as of
  `524cd24`** — plus two defects
  documented in `mutations.toml` as *uncovered*, which a passing run says nothing about):
  the accounting and comparison path (`nazm-bench selftest`, 19 cases), per-attempt
  workspace management, the submission freeze, and the independent acceptance runner.
  **Not written:** agent drivers, a provider adapter, and all but one task fixture — so
  the harness cannot produce a benchmark number, and `run` stays disabled. The corpus has
  **never accepted a known-good solution or rejected a known-bad one**: there is no
  `reference/` or `broken/` directory anywhere, so it has not been shown to discriminate
  at all.
- **Confinement works and is tested against a real daemon.** Each attempt gets a private
  internal network, its own PostgreSQL container, and a candidate container with all
  capabilities dropped, no published ports, and no host namespaces. Four tests under
  `NAZM_REQUIRE_DOCKER=1` establish what the fake engine cannot: a candidate cannot read a
  host sentinel or route off its network, killing it reaps a detached grandchild, teardown
  is verified clean, and a separate process recovers what a killed harness left behind.
  Run them with `NAZM_REQUIRE_DOCKER=1 cargo test --test docker_boundary -- --test-threads=1`.
- **One complete attempt runs end to end.** Candidate work → stop → freeze the submission
  and record its digest → reset the database so the candidate's own writes are not what it
  is graded against → grade the frozen snapshot against tests it never saw → record the
  outcome and costs → verified cleanup. Demonstrated with three submissions: accepted,
  wrong answer, and a timeout that outranks a correct answer written before the deadline.
  A grade is **provisional until cleanup verifies**, because a leak means the next attempt
  starts from this one's leftovers and that cannot be detected afterwards.
- **`run` is still disabled.** There is no task corpus, no agent driver, and no provider
  adapter — the path is validated for the development fixtures, not for a benchmark.
- **Holdout isolation is a workflow control, not a boundary.** The API refuses holdout
  fixtures without a verified freeze; it does nothing about filesystem access. Real
  isolation for the gate run is operational — see `docs/research/evaluation.md` §3.
- **No task corpus.** The six task families in `docs/research/evaluation.md` are described, not
  written.
- **The decision rule is not pre-registered.** It is a draft until committed with its hash
  recorded, and it is not binding before then. Registration is bound by git ancestry plus a
  runner preflight record; neither is an independent timestamp, so third-party-verifiable
  chronology would need an external registration event.
- **The spec is part specification, part list of open questions** — see
  [`docs/spec.md`](docs/spec.md). Its *Settled* strata are real and versioned; its *Open*
  section is the register of what is not. Notably unsettled: whether divergence is an
  effect, which determines whether an unused pure call may be dropped at all.
- **No performance targets**, and the numbers that exist are narrow.
  [`docs/performance.md`](docs/performance.md) carries one optimisation pass measured
  against its own baseline, an interpreter-against-native comparison, and a single external
  reference (`clang` and `python3` on one program). There are still no targets, because a
  target set from one program on one host would be a number pretending to be a goal. The
  Darwin baseline is **stale and not a valid comparison**; the Linux one is current.
- **No literature review** on automatic layout selection, so this project is not entitled
  to claim the idea is novel. Phase 1 deliverable.

## Licence

Apache-2.0. See [`LICENSE`](LICENSE).

**Open question:** whether the runtime and standard library need an exception clause — as
Swift and LLVM use — so that programs compiled with Nazm are unambiguously unencumbered.
Compiler and runtime licensing are different problems, and this is a Phase 1 decision.
