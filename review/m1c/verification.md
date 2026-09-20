# M1c verification results

Host: **aarch64-apple-darwin** (macOS 27.2.0) · `rustc`/`cargo` **1.98.1** · Apple
**clang 21.0.0**. Nothing committed, nothing pushed.

## Executed

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo lint` (`clippy --workspace --all-targets --locked -- -D warnings`) | **0 diagnostics** |
| `cargo test --workspace` | **352 passed, 0 failed, 6 ignored** |
| `cargo test -p xtask --test lifecycle -- --ignored --test-threads=1` | **6 passed** (13.2 s) |
| `cargo xtask check` | **5/5** — layering, codegen reach, emission reach, unsafe policy, doc claims |
| `cargo run -p nazm-bench -- selftest` | 19/19 |

**352 is the workspace total.** Per-suite figures below are *components of it*, not
additional tests, and must not be summed with it:

| Suite | Of the 352 |
| --- | --- |
| `nazm-cli` `tests/build.rs` (the compiler) | 58 |
| `nazm-core` `tests/language.rs` (the interpreter, unchanged) | 98 |
| `nazm-lir` unit tests | 11 |
| `nazm-bench` lib | 52 |
| everything else | 133 |

The 6 ignored are `xtask/tests/lifecycle.rs`, run separately by the command above.

Every arithmetic, conditional and call case in `tests/build.rs` is compiled and executed
at **`-O0` and `-O2`**, and checked against an explicitly written expected value *and*
against the interpreter.

## Not exercised — stated rather than implied

| | |
| --- | --- |
| `docker_boundary` — 10 of the 352 | Report `ok` **without exercising anything**: they return early without `NAZM_REQUIRE_DOCKER=1`. Subtract them from any coverage claim |
| `NAZM_REQUIRE_NONUTF8` | Cannot run on APFS — the filesystem refuses to create the filename |
| `cargo xtask mutate` (full catalogue, now 47 entries) | **Not run.** The new entry was verified individually; re-running 47 was not justified by adding one |
| CI | **Never executed** — the repository still has no git remote |
| Targets other than `aarch64-apple-darwin` | **Unverified, not unsupported** |

## Mutation outcome — the new entry

Verified through the **real single-entry mutation path** (`xtask mutate-run`) against an
**isolated disposable copy** of the checkout in `/tmp`, removed afterwards.

| Mutation | Verdict | Failing tests |
| --- | --- | --- |
| `two-same-typed-arguments-are-transposed` | **caught** | 3 |

Against the four conditions asked for:

1. **Baseline passed.** The runner checks the unmutated tree builds and the suite is green
   *before* injecting, and refuses to start otherwise. It produced a verdict, so it got
   past that check.
2. **The mutant compiled.** `caught` is only reachable after a successful
   `cargo build --workspace --tests`; a malformed module would have been reported
   `UNUSABLE`. The mutation is deliberately guarded on the two argument types being equal
   for exactly this reason — transposing differently typed arguments would emit
   `call i64 @nz.fN(i1 %a, i64 %b)`, clang would reject it, and a build failure is not
   evidence that an argument-order test discriminates.
3. **A semantic assertion caught it.** Applied by hand in the copy and run alone:

   ```
   thread 'arguments_are_not_swapped' panicked at crates/nazm-cli/tests/build.rs:1222:9:
   assertion `left == right` failed: -O0
     left: "-7"
    right: "7"
   ```

   `sub(10, 3)` produced **-7**. Noncommutative on purpose; a commutative helper would
   have been satisfied by the transposition.
4. **The source was restored exactly.** `diff -q` against the working tree reported no
   difference, and no `target/.nazm-mutation-backup` was left behind.

## What the `-O0` / `-O2` pairs do and do not establish

They establish that **the specified outcome does not depend on the optimisation level**.

They do **not** establish that any call, branch or arithmetic operation survived
optimisation. Every program here has constant inputs — **including the ones with
parameters**: a parameter is only unknown to *its own body*, and a `main` that calls
everything with literals is still a closed term an optimiser may inline, specialise and
fold entirely away. Nothing short of a genuinely external input makes a value unknown at
compile time, and this language has none. Both earlier claims to the contrary are
corrected in `roadmap.md`.
