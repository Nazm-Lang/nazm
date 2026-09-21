# Nazm

**An AI-native programming language: fast, memory-efficient, concurrent — with a compiler
eventually written in Nazm itself.**

> نظم — *systematic order, well-formed arrangement.*

Nazm starts from a claim that is popular and, as usually stated, wrong: that because AI now
writes most code, languages should be redesigned to save tokens. The evidence says
otherwise. What the evidence *does* show is that agents waste enormous effort fighting
compilers and churning over code that already works — and that is attackable.

So this repository contains two things. **The language** — today an interpreter, not a
compiler, which is why nothing here speaks to the performance claims yet. And **an
agent-cost harness** built to measure specific claims about it honestly, including the
uncomfortable one: that better tooling on an existing language might capture much of the
benefit.

The language is the primary track. That was decided on 2026-09-20 and recorded in
[`docs/direction.md`](docs/direction.md); what comes next, in dependency order, is
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

$ sh compiler/bootstrap.sh   # the compiler written in Nazm, compiling itself
bootstrap complete: C2 and C3 are byte-identical, 5 conformance cases agree
```

`compiler/*.nz` is a compiler for Nazm **written in Nazm** — lexer, parser, checker and
LLVM emission, about 3 500 lines, every component differentially verified against the
Rust implementation rather than merely run. It compiles itself to a fixpoint;
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

An executable subset — functions, recursion, `let` and `let mut`, assignment, `while` with
`break` and `continue`, `return`, `if`/`else`, and arithmetic and comparison over `Int`,
`Bool` and `Str` — lexed, parsed, type-checked and evaluated. No collections, user-defined
types, effects or I/O yet, and a program using them is **refused by name** rather than
misparsed into something else.

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
[`docs/direction.md`](docs/direction.md) for what changed about how the answer is used.

## The experiment

Three arms. The third is the hypothesis; the second is why the experiment is honest. It
measures specific claims — it no longer decides whether the language gets built
([`docs/direction.md`](docs/direction.md)).

| Arm | Contents |
| --- | --- |
| A1 | TypeScript, baseline tooling |
| **A2** | **TypeScript + improved agent tooling** — structured diagnostics, canonical formatter, repeat-edit detection |
| A3 | Nazm + comparable tooling |

If A2 captures most of the benefit, that is a finding about the claim under test and it
stays in the record. The decision rule, its thresholds and the cost policy are unchanged,
and are written down **before** the evaluation runs — see
[`docs/evaluation.md`](docs/evaluation.md).

First workload: small HTTP/JSON services on PostgreSQL — validation, transactions,
authorization, error handling, cancellation. First user: a backend developer using coding
agents to build and maintain services, so debugging and operations stay in scope rather
than just "generated programs pass tests".

## The design it is being built toward

Documented in [`docs/architecture.md`](docs/architecture.md), and staged in
[`docs/roadmap.md`](docs/roadmap.md). In brief: mutable value
semantics so memory safety needs no lifetime annotations; effect rows, capabilities, and
position-sensitive taint so untrusted data cannot reach a code-construction site;
structured concurrency where data races are a compile error; and a four-tier compilation
strategy where `nazm check` — no codegen at all — is the loop agents actually live in.

The answer to "does this need a new compiler design?" is **new front end, new IR, new
middle end, reused back end**. Code generation is decades of work with no novelty available
in it.

**What comes next**, in dependency order and no further:

| | |
| --- | --- |
| **M1** | A native vertical slice. The existing lexer, parser, checker and diagnostics, one new typed IR, LLVM IR emitted as text and assembled by `clang`, one target. A compiled program runs with no interpreter, and is differentially compared against the interpreter on the existing 98-case corpus |
| **M2** | The facilities a compiler actually needs — value/copy/move and cleanup first, then strings, tagged unions, collections, arena-indexed recursive structures, modules, and a narrow capability-based I/O contract |
| **M3** | The bootstrap: `Rust → C1`, `C1 → C2`, `C2 → C3`, with byte equality asserted only where determinism has been established |

Self-hosting is the long-term objective and it is **M3, not next**. Nazm today has no
collections, no user-defined types, no pattern matching, no string concatenation and no
I/O, so a compiler cannot be written in it in any form. The Rust implementation is retained
as the bootstrap seed and the differential oracle.

## What this does not establish

Stated up front, because these are easy to blur:

| Mechanism | Does **not** establish |
| --- | --- |
| Ownership / exclusivity | Business-logic correctness. Two valid transactions can still oversell inventory |
| Provenance / taint | Authorization. Correctly-parameterised SQL can still read another tenant's rows |
| Effect rows / purity | That an edit preserves behaviour — and not, alone, that a call can be dropped or reordered |
| Definition-hash equality | All behaviour-preserving rewrites. Only one class: normalised-Core alpha-equivalence |
| Passing the test suite | Correctness |
| Reproducible bootstrap | Compiler trustworthiness |
| Differential testing | Absence of bugs shared by all implementations |

"Verified-correct" does not appear anywhere in this project. The phrase is **"passes
independent acceptance tests and specified static checks."**

## Build and check

```sh
cargo build --workspace
cargo test  --workspace
cargo lint              # clippy; CI runs this same alias, so they cannot drift
cargo xtask check       # architectural gates, and the guide against the grammar
cargo xtask guide       # rewrite docs/guide.md from docs/grammar.ebnf

# Not in the default set: these drive nested cargo builds and kill real process
# groups, and `cargo xtask mutate` would otherwise pay their cost once per entry.
cargo test -p xtask --test lifecycle -- --ignored --test-threads=1

# Each catalogued defect is injected and must be caught. One full build and test run
# per entry, so `--only NAME` selects a subset by substring.
cargo xtask mutate
```

The toolchain is pinned to 1.98.1 in `rust-toolchain.toml`. That pin is load-bearing:
`cranelift` 0.135 declares MSRV 1.95.0, so the 1.93.1 an earlier draft assumed could not
have built the eventual backend. The workspace's declared MSRV floor is 1.85; `xtask`
declares 1.89 for itself, because it uses std's file locking and a development tool has no
business raising the floor for the crates that ship.

## The commands

| | |
| --- | --- |
| `nazm run FILE` | Check and evaluate, printing the value of `main` |
| `nazm check FILE` | Check without evaluating |
| `nazm build FILE` | Compile to a native executable |
| `nazm test PATH…` | Run every `NAME.nz` with a `NAME.expected` beside it, interpreted **and** compiled |
| `nazm fmt FILE…` | Lay files out canonically. `--check` writes nothing and fails if any would change |
| `nazm fix FILE` | Apply the repairs the compiler already knows about. Nothing is written without `--apply`, and only fixes marked `automatic` are ever applied |
| `nazm capabilities` | What this build supports, read from the compiler's own tables |

`--json` on any of them emits machine-readable output under a **named, versioned
schema**; the schemas are in [`schema/`](schema) and the compatibility rules are in
[`docs/diagnostics.md`](docs/diagnostics.md). `scripts/repair.sh` is the whole repair
loop as one command: apply every automatic fix until a pass changes nothing, lay the
result out, and report what is left as JSON.

## The documents

| | |
| --- | --- |
| [`docs/delivery.md`](docs/delivery.md) | **Start here.** What was built, what verifies it, what it does not establish, and how to reproduce all of it |
| [`docs/guide.md`](docs/guide.md) | The language, for someone writing a program in it. Its syntax section is generated from the grammar |
| [`docs/spec.md`](docs/spec.md) | What each construct means, and the decision behind it |
| [`docs/diagnostics.md`](docs/diagnostics.md) | The machine interface: schemas, fixes, capabilities, exit statuses |
| [`docs/bootstrap.md`](docs/bootstrap.md) | What the self-hosting claim does and does not establish |
| [`docs/performance.md`](docs/performance.md) | What has been measured, and the noise floor under it |

## What is not built yet

Almost everything. Specifically:

- **The language is an interpreter and a native path over the same subset.** Lexer,
  parser, type checker and tree-walking evaluator; `nazm build` compiles everything in
  the list above. One IR level (`nazm-lir`), one backend (LLVM IR as text → `clang`),
  one verified target (`aarch64-apple-darwin`); other targets are **unverified rather
  than unsupported**. Run `nazm capabilities` for what this build actually has, read
  from the compiler's own tables rather than from this paragraph. Nothing here says
  anything yet about the performance claims: `-O0` is the default, no optimisation work
  has been done, and no measurement has been taken. `docs/spec.md` keeps its sections
  versioned, so a *Settled for v0.1* entry records what v0.1 decided rather than what is
  true now. Everything under *Open* is undecided.
- **Concurrency is an initial model, and only that.** `scope`, `spawn` and a bounded
  `Chan` of `Int`, specified in `docs/spec.md` before being implemented, running
  interpreted and compiled. **One operating-system thread per task and nothing more**:
  no M:N scheduling, no work stealing, no task density, no fairness, no deadlock
  freedom, and no cancellation beyond closing a channel. The compiler written in Nazm
  does not accept it — concurrency is outside the bootstrap subset, and says so by
  name.
- **The harness is partial.** Real, tested, and mutation-checked (`cargo xtask mutate`,
  44 runnable catalogued defects, all caught **as of `8d67a7d`** — plus two defects
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
  isolation for the gate run is operational — see `docs/evaluation.md` §3.
- **No task corpus.** The six task families in `docs/evaluation.md` are described, not
  written.
- **The decision rule is not pre-registered.** It is a draft until committed with its hash
  recorded, and it is not binding before then. Registration is bound by git ancestry plus a
  runner preflight record; neither is an independent timestamp, so third-party-verifiable
  chronology would need an external registration event.
- **The spec is a list of open questions**, not a specification — see
  [`docs/spec.md`](docs/spec.md). Notably unsettled: whether divergence is an effect, which
  determines whether an unused pure call may be dropped at all.
- **No performance numbers.** [`docs/performance.md`](docs/performance.md) contains claims
  to be tested, and deliberately no targets, because there are no baselines to set them
  against.
- **No literature review** on automatic layout selection, so this project is not entitled
  to claim the idea is novel. Phase 1 deliverable.

## Licence

Apache-2.0. See [`LICENSE`](LICENSE).

**Open question:** whether the runtime and standard library need an exception clause — as
Swift and LLVM use — so that programs compiled with Nazm are unambiguously unencumbered.
Compiler and runtime licensing are different problems, and this is a Phase 1 decision.
