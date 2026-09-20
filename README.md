# Nazm

**An AI-native programming language — or, first, an experiment to find out whether one is
worth building.**

> نظم — *systematic order, well-formed arrangement.*

Nazm starts from a claim that is popular and, as usually stated, wrong: that because AI now
writes most code, languages should be redesigned to save tokens. The evidence says
otherwise. What the evidence *does* show is that agents waste enormous effort fighting
compilers and churning over code that already works — and that is attackable.

So this repository does not yet contain a language. It contains the instrument that decides
whether the language should exist.

## What is actually here

**A `.nz` program runs.**

```sh
$ cargo run -p nazm-cli --bin nazm -- run examples/max.nz
25
```

```nazm
fn sum_to(n: Int) -> Int {
    let mut total = 0;
    let mut i = 1;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    total
}
```

An executable subset — functions, `let` and `let mut`, assignment, `while` with `break`
and `continue`, `if`/`else` as a statement or an expression, and arithmetic and comparison
over `Int`, `Bool` and `Str` — lexed, parsed, type-checked and evaluated. No `return`,
effects or I/O yet, and a program using them is **refused by name** rather than misparsed
into something else.

The semantics it commits to are in [`docs/spec.md`](docs/spec.md): overflow traps
identically in every build, division by zero is an error, bindings are immutable unless
written `let mut`, parameters are never mutable, assignment is a statement so `if (x = 1)`
does not parse, shadowing is allowed in a nested scope and refused in the same one,
arguments evaluate left to right, and a function's value is its final expression.

`break` and `continue` affect the innermost loop and there are no labels. They are
statements with no value, which is why `if` exists in two forms: an **expression** whose
branches are expression blocks and whose `else` is required, and a **statement** whose
branches hold statements and whose `else` is optional. Which one a given `if` is follows
from where it appears, so nothing needs annotating. Code after a `break` is an error
rather than a warning — relaxing an error later is a compatible change, tightening a
warning is not.

`nazm run` stops a non-terminating program after a configurable number of iterations. That
is the interpreter's guard, not a language rule — with no `break` and no I/O such a program
produces nothing at all, and a tool that hangs is worse than one that reports.

Alongside it, a benchmark harness. `crates/nazm-bench` measures what it costs an agent to
build and maintain small backend services, with a validated execution path for its
development fixtures — containers, a restricted database role, verified cleanup. It cannot
produce a benchmark number yet: there is no task corpus, no agent driver, and no provider
adapter.

The harness came first on purpose. The decisive experiment sat at month 30 in the first
version of this plan; it now sits at roughly month 13, and the instrument has to be shown
to work before anything it measures means anything.

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

**Whether that is worth a new language, or just better tooling for TypeScript, is the
question this repository is built to answer** — and the answer may well be the second one.

## The experiment

Three arms. The third is the hypothesis; the second is why the experiment is honest.

| Arm | Contents |
| --- | --- |
| A1 | TypeScript, baseline tooling |
| **A2** | **TypeScript + improved agent tooling** — structured diagnostics, canonical formatter, repeat-edit detection |
| A3 | Nazm + comparable tooling |

If A2 captures most of the benefit, the right deliverable is better TypeScript tooling and
Nazm should not be built. The decision rule is written down and committed **before** the
evaluation runs — see [`docs/evaluation.md`](docs/evaluation.md).

First workload: small HTTP/JSON services on PostgreSQL — validation, transactions,
authorization, error handling, cancellation. First user: a backend developer using coding
agents to build and maintain services, so debugging and operations stay in scope rather
than just "generated programs pass tests".

## The design, if it gets built

Documented in [`docs/architecture.md`](docs/architecture.md). In brief: mutable value
semantics so memory safety needs no lifetime annotations; effect rows, capabilities, and
position-sensitive taint so untrusted data cannot reach a code-construction site;
structured concurrency where data races are a compile error; and a four-tier compilation
strategy where `nazm check` — no codegen at all — is the loop agents actually live in.

The answer to "does this need a new compiler design?" is **new front end, new IR, new
middle end, reused back end**. Code generation is decades of work with no novelty available
in it.

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
cargo lint              # clippy, mirrors CI exactly
cargo xtask check       # architectural gates
```

The toolchain is pinned to 1.98.1 in `rust-toolchain.toml`. That pin is load-bearing:
`cranelift` 0.135 declares MSRV 1.95.0, so the 1.93.1 an earlier draft assumed could not
have built the eventual backend.

## What is not built yet

Almost everything. Specifically:

- **No language.** No lexer, parser, type checker, IR, interpreter, or backend.
- **The harness is partial.** Real, tested, and mutation-checked (`cargo xtask mutate`,
  7 catalogued defects, all caught): the accounting and comparison path (`nazm-bench
  selftest`, 15 cases), per-attempt workspace management, the submission freeze, and the
  independent acceptance runner. **Not written:** agent drivers, a provider adapter, and
  all but one task fixture — so the harness cannot produce a benchmark number, and `run`
  stays disabled.
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
