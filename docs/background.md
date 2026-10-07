# Background: why Nazm exists

These sections were the top-level README until R1 (2026-10-07), when the README was rewritten for
a reader who knows nothing of the project's history. They are kept here verbatim because they are
still the project's argument and the design it was built toward. Figures and status remarks in
them are as of when each was written; `capability-matrix.md` is the current status, and
`docs/releases/` holds the frozen evidence.

The agent-cost harness they describe — `crates/nazm-bench`, `crates/nazm-agent-bench` and
`docs/research/evaluation.md` — is maintained, not extended, and nothing that ships depends on it.
It still has no task corpus or agent driver for its own benchmark, so it produces no benchmark
number; `docs/performance.md` records the one agent-task measurement that exists.

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

