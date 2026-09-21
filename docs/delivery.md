# Delivery report

What was built under `docs/NAZM_AUTONOMOUS_GOAL.md`, what verifies it, and what it does
not establish. Written to be checkable: every claim here names the command or file that
supports it, and the limitations are in the same document as the results rather than in
a different one.

## Outcome

Nazm is a small, specified language with two implementations that agree — a tree-walking
interpreter and a native compiler through LLVM IR — **and a compiler for it written in
Nazm that reproduces itself byte for byte**. It has structured concurrency, a canonical
formatter, machine-applicable fixes under a versioned JSON schema, and measured
performance baselines with one profile-driven optimisation pass behind them.

It is an experimental release. It has been verified on one target, it has no package
ecosystem, no visibility control, no recursion in native code, and no cross-language
performance comparison of any kind.

## The checklist, and the evidence for each line

| Definition of done (§13) | State | Evidence |
| --- | --- | --- |
| Scalar control flow, including `return` and transfers inside values | done | `crates/nazm-cli/tests/build.rs`, every case at `-O0` and `-O2` against the interpreter |
| A bootstrap subset with text, structured data, collections, memory behaviour, file I/O, errors and modules | done | `docs/bootstrap-design.md`; the subset is what `compiler/*.nz` is written in |
| Compiler frontend and lowering/emission in Nazm, external dependencies documented | done | `compiler/lex.nz`, `parse.nz`, `analyse.nz`, `emit.nz`; dependencies in `docs/bootstrap.md` |
| Native C1 → C2 → C3 bootstrap, conformance and reproducibility | done | `compiler/bootstrap.sh`; C2 and C3 byte-identical in IR and executable, 5 conformance cases |
| Ordinary independent programs compile with the Nazm-written compiler | done | `crates/nazm-cli/tests/selfhost.rs` |
| Initial structured concurrency: transfer/capture rules, cancellation, cleanup | done | `docs/spec.md`; 15 interpreter tests and 11 native tests, all deadline-bounded |
| Structured developer interfaces, grammar/guide, formatting, repair workflow | done | `docs/diagnostics.md`, `schema/`, `docs/grammar.ebnf`, `docs/guide.md`, `scripts/repair.sh` |
| Performance baselines, a profiling/optimisation pass, no unsupported speed claims | done | `docs/performance.md`, `bench/baseline.json` (Darwin), `bench/baseline-linux-aarch64.json` |
| A clean rebuild of the integrated final state on a supported target | done | a fresh copy, `--offline`, full suite — see *Reproducing this* |
| A final local review/release package | done | `scripts/release-candidate.sh`, this document |

## What is verified, and what that means

Six different kinds of evidence are involved, and they are not interchangeable.

| Claim | Established by | **Not** established |
| --- | --- | --- |
| The two implementations agree | every native test runs the same program under `nazm run` and compares | that either matches the specification where both are wrong the same way |
| The compiler is self-hosting | `bootstrap.sh`: C2 and C3 byte-identical | that the compiler is *correct*. A compiler that miscompiles its own source in a way that reproduces the miscompilation is also a fixpoint — stated in `docs/bootstrap.md` |
| Concurrency works | 26 tests, each with a deadline so a hang fails by name; invariants and totals, never a schedule | freedom from deadlock, which the specification explicitly does not promise |
| The formatter is safe | it re-lexes to the same token kinds, is idempotent, and keeps every comment | that its layout is the one you would choose |
| Fixes are applicable | preconditions on every fix; `--apply` refuses to write a file that would not parse | that a `needs_review` fix is right — that is what the label means |
| The optimisation helped | a profile before, a measurement against a recorded baseline after, same machine minutes apart | anything about another machine, or about any other language |

## Supported platform and dependencies

| | |
| --- | --- |
| Verified targets | `aarch64-apple-darwin` (Darwin arm64) and `aarch64-unknown-linux-gnu` (Linux aarch64, inside the contained runner). Other targets remain **unverified rather than unsupported** |
| Toolchain | rustc 1.98.1, pinned in `rust-toolchain.toml`. Workspace MSRV floor 1.85; `xtask` declares 1.89 for itself |
| External at build time | `clang`, to assemble the emitted LLVM IR and link |
| External at run time | libc — `malloc`, `realloc`, `write`, `exit`, stdio, and pthreads for concurrency |
| Rust dependencies | `clap`, `serde`, `serde_json`, `tempfile` (dev). The benchmark harness has its own set; see `Cargo.toml` |

The bootstrap needs the Rust implementation to exist: C1 is built by it. Nazm does not
build itself from nothing, and `docs/bootstrap.md` says so at length.

## Known limitations

These are properties of the release, not a backlog.

- **No visibility control.** Every top-level function in a file is visible to anything
  that `use`s it. Adding private-by-default later will break existing multi-file
  programs; it already forced `compiler/lex.nz` to be split from `compiler/lexer.nz`,
  because a library carrying a `main` collides with its importer's.
- **No recursion in native code.** Refused by name, pending a stack contract. The
  compiler written in Nazm is iterative throughout, which is why it compiles natively.
- **Ambient authority.** `read_file`, `write_file` and the rest are available to any
  function with no capability to pass. Labelled as such in `docs/spec.md`; a capability
  parameter is the growth path and `main(io: IO)` was considered and not chosen.
- **A compiled program has no iteration budget.** `nazm run` stops a runaway loop and
  reports `N0402`; the executable does not stop.
- **Concurrency is one OS thread per task.** No M:N scheduling, no work stealing, no
  fairness, no cancellation beyond closing a channel, and no deadlock freedom. At
  roughly 4.5 µs per channel hand-off it is suited to coarse-grained work and not to
  fine-grained message passing.
- **The bootstrap compiler does not accept concurrency**, and refuses it by name rather
  than being silently unable to.
- **`use` is resolved by the driver, not by the compiler written in Nazm.** The
  bootstrap concatenates the four sources.
- **No cross-language performance comparison.** `docs/performance.md` measures this
  implementation against itself and says nothing about C, Rust, Zig, Go or Python.
- **The benchmark harness (`nazm-bench`) cannot produce a benchmark number.** Its
  agent drivers, provider adapter and task corpus are unwritten, and `run` is disabled.
  It is unrelated to the language work above; see `README.md`.

## Results

`cargo test --workspace`, run through `cargo xtask contained` on Linux:
**524 passed, 0 failed, 6 ignored.** What they cover, grouped by the question each group
answers:

| | count | what it establishes |
| --- | ---: | --- |
| Language semantics (`nazm-core/tests/language.rs`) | 114 | each rule in `docs/spec.md` that a refactor could quietly reverse |
| Native end-to-end (`nazm-cli/tests/build.rs`) | 126 | every case compiled at `-O0` **and** `-O2`, and compared against the interpreter |
| Self-hosting differentials (`nazm-cli/tests/selfhost.rs`) | 15 | the Nazm lexer, parser and checker against the Rust ones over every `.nz` in the tree, and the full bootstrap chain |
| Concurrency, interpreted (`nazm-core/tests/concurrency.rs`) | 15 | joins, capture, cancellation, cleanup, contention — every one with a deadline |
| Concurrency, native (`nazm-cli/tests/concurrent.rs`) | 11 | the same programs compiled, at `-O0` and `-O2`, against a written expectation *and* the interpreter |
| Machine interface (`nazm-cli/tests/schemas.rs`) | 5 | real command output validated against `schema/*.json` |
| Formatter (`nazm-syntax/tests/format.rs`) | 11 | token preservation, idempotence, comment preservation, over every source in the tree |
| Grammar (`nazm-syntax/tests/grammar.rs`) | 4 | every lexer token written down, every rule reachable, every example parsed by the real parser |
| Repair workflow (`nazm-cli/tests/workflow.rs`) | 10 | `nazm fix`, `nazm fmt`, `nazm test` and `scripts/repair.sh` end to end |
| Coverage of the guarantees themselves (`codes.rs`, `builtins.rs`, `docs.rs`, `counts.rs`, `resource_regression.rs`) | 10 | every diagnostic code asserted or named unreachable; every built-in exercised where an answer is asserted; every example run by a test; the README's command table, the published schemas and the verified target checked against the build; and this table's own rows required to sum to the total above; and the stage drivers required to assemble their output in one allocation, measured under a ceiling |
| The rest | 203 | unit tests across the six crates, the CLI's own behaviour, and the benchmark harness — which is not part of this goal |

The bootstrap: **C2 and C3 byte-identical** in IR and executable, with 5 conformance
cases agreeing across C2, C3 and the interpreter. `compiler/bootstrap.sh` writes the
record, including the source digest and line count.

Benchmarks: `docs/performance.md`, with the noise floor of the method stated alongside
the numbers.

### Mutation coverage

`cargo xtask mutate` injects each catalogued defect into a **disposable copy** and
requires the suite to catch it. Run at final integration against `524cd24`, which is the
integrated state minus `nazm test` — an addition that touches none of the code the
catalogue mutates.

**48 caught, 0 survived, 0 unusable.** Every catalogued defect that could be injected
was detected by the suite.

**Two were not injected**, and both were already known: the run confirmed, in the
runner's own words, what checking the catalogue against the tree had found an hour
earlier.

| | the runner said | why |
| --- | --- | --- |
| `the-conditional-selects-the-wrong-branch` | *pattern occurs 2 times; it must be unique* | its anchor was the `br i1` line alone, which appears in both `statement_if` and `conditional` |
| `the-loop-reuses-its-first-condition` | *pattern not found* | its anchor predated `break` and `continue`, which made the back edge conditional |

Neither is a survivor, and both are worse than one: a mutation that does not apply is a
problem only if somebody reads the log, and reads as coverage if they do not. Both are
repaired, and the ambiguous one is split into two entries, because the statement and
value forms of `if` are different code paths. `cargo xtask mutate --only NAME` exists so
the repairs could be verified without re-running fifty.

A caught mutation establishes that the suite detects **that** defect, and nothing
broader. Fifty entries is fifty facts, not a coverage percentage.

## Skipped and conditional coverage

Reported rather than folded into a total, because a skipped test that looks like a
passing one is the most misleading thing a suite can contain.

| | |
| --- | --- |
| Two diagnostic codes, `N0404` and `N0406` | asserted by no test, and **named as unprovokable** in `crates/nazm-cli/tests/codes.rs` with the reason: one needs the machine to run out of threads, the other out of memory. That test requires every other code to be asserted somewhere, so the list is the only way out and it costs a sentence that has to be true |
| 6 ignored tests in `cargo test --workspace` | `xtask` lifecycle tests. They spawn nested cargo builds and kill real process groups, so they run separately: `cargo test -p xtask --test lifecycle -- --ignored --test-threads=1` |
| 10 `docker_boundary` tests | exercise nothing without `NAZM_REQUIRE_DOCKER=1`. They belong to the benchmark harness, not the language |
| The mutation catalogue | `cargo xtask mutate` is not in the default gate set: it injects defects into a copy and costs a full build per entry |

## The exact source state

| | |
| --- | --- |
| Repository | local only; no remote is configured, which is not a blocker to anything here |
| Integrated commit | the tip of `main` at delivery; `scripts/release-candidate.sh` records it in `MANIFEST.txt` and refuses to run on a dirty tree without `--allow-dirty` |
| Remaining diff | none. The candidate is assembled only from a clean tree unless that flag is passed, and the manifest says which |

## Reproducing this

From a clean checkout on Darwin arm64, with `clang` available:

```sh
cargo build --workspace
cargo xtask contained tests          # the whole suite
cargo lint                       # clippy, the same alias CI runs
cargo xtask check                # architectural gates and the guide against the grammar
cargo xtask contained bootstrap         # C1 -> C2 -> C3, byte-identical
cargo xtask contained bench      # measurements; --check compares to this host's baseline
cargo xtask contained run \'sh scripts/release-candidate.sh\'  # every gate, then the local artefact

# Not in the default set, and each says why in its own output:
cargo xtask contained mutate                       # one build and test run per catalogued defect
cargo test -p xtask --test lifecycle -- --ignored --test-threads=1
NAZM_REQUIRE_DOCKER=1 cargo test --test docker_boundary -- --test-threads=1
```

The compiler also runs a corpus of its own:

```sh
cargo run -p nazm-cli -- test compiler/conformance   # 5 cases, interpreted and compiled
```

Independent example programs are in `examples/`: `max.nz`, `sum.nz`, `primes.nz`,
`search.nz` and `pipeline.nz` (structured concurrency). Each runs identically under
`nazm run` and as a `nazm build` executable, and `crates/nazm-cli/tests/build.rs`
asserts exactly that.

A repair workflow anyone can run:

```sh
printf 'fn main()->Int{\nlet x=1;\nx=2;\nx}\n' > broken.nz
NAZM=./target/debug/nazm sh scripts/repair.sh broken.nz
nazm run broken.nz     # 2
```

## The archive

`scripts/release-candidate.sh` assembles `target/release-candidate/`: the binary, the
documentation, the JSON schemas, the examples, the recorded baseline, the bootstrap
record, and a `MANIFEST.txt` carrying the commit, the toolchain, and a SHA-256 of every
file. It builds nothing that has not passed every gate, and it does not tag, push or
publish — that is a decision for a person.
