# M1b verification results

All commands run against the final edited working tree on **aarch64-apple-darwin**
(macOS 27.2.0), `rustc`/`cargo` **1.98.1**, Apple **clang 21.0.0**. Nothing committed,
nothing pushed.

## Executed

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo lint` (`clippy --workspace --all-targets --locked -- -D warnings`) | **0 diagnostics** |
| `cargo test --workspace` | **334 passed, 0 failed, 6 ignored** |
| `cargo test -p nazm-cli --test build` | **43 passed** (17.3 s) |
| `cargo test -p nazm-lir` | **8 passed** |
| `cargo test -p xtask --test lifecycle -- --ignored --test-threads=1` | **6 passed** (13.4 s) |
| `cargo xtask check` | **5/5** — layering, codegen reach, **emission reach** (new), unsafe policy, doc claims |
| `cargo run -p nazm-bench -- selftest` | 19/19 |

Every arithmetic and conditional case in `tests/build.rs` compiles and executes at
**`-O0` and `-O2`**, and is checked against an explicitly written expected value *and*
against the interpreter.

## Not exercised — stated rather than implied

| | |
| --- | --- |
| `docker_boundary` (10 of the 334) | Report `ok` **without exercising anything**: they return early without `NAZM_REQUIRE_DOCKER=1`. Subtract them from any coverage claim |
| `NAZM_REQUIRE_NONUTF8` | Cannot run on APFS at all — the filesystem refuses to create the filename |
| `cargo xtask mutate` (full catalogue) | **Not run.** The two new entries were each verified individually (below); re-running 46 entries was not justified by that |
| CI | **Never executed** — the repository still has no git remote |
| Targets other than `aarch64-apple-darwin` | **Unverified, not unsupported.** Nothing in the emitted IR is target-specific, but nothing has been run elsewhere |

## Mutation outcomes — the two new entries

Each verified through the **real single-entry mutation path** (`xtask mutate-run`) against
an **isolated disposable copy** of the checkout in `/tmp`, never the working tree. Both
copies were removed afterwards.

| Mutation | File | Verdict | Failing tests |
| --- | --- | --- | --- |
| `checked-add-loses-its-sign` (`sadd` → `uadd`) | `crates/nazm-lir/src/emit.rs` | **caught** | 4 |
| `the-conditional-selects-the-wrong-branch` (branch labels swapped) | `crates/nazm-lir/src/emit.rs` | **caught** | 10 |

**Discrimination, checked rather than assumed.** The arithmetic mutant was applied by hand
in the copy and the relevant test run alone:

```
thread 'addition_overflow_traps' panicked at crates/nazm-cli/tests/build.rs:153:9:
assertion `left == right` failed: -O0: a specified runtime failure must exit 2,
not Some(0). stderr:
  left: Some(0)
 right: Some(2)
```

The mutant exits **0** — silently wrong arithmetic — so a test asserting merely "non-zero
exit" would have missed it entirely. The assertions require the exact status **2**, the
`N0400` code, the message, and a source position, which is the diagnostic contract rather
than a liveness check.

**Toolchain absence is kept separate.** `the_native_toolchain_is_available` asserts
`clang --version` succeeds, on its own. Without it, a machine with no `clang` fails every
compiling test and a mutation run reads that as *every mutant caught* — coverage
manufactured by a missing tool. With it, the baseline check refuses to start instead.

## What the `-O0` / `-O2` pairs do and do not establish

They establish that **the specified outcome does not depend on the optimisation level**.

They do **not** establish that any branch survived optimisation. Every program in the M1b
subset has constant inputs — there are no parameters and no I/O — so `-O2` may evaluate
the whole program at compile time, comparisons and branches included. An earlier
suggestion that comparisons would defeat folding was wrong and has been corrected in
`docs/roadmap.md`. A value unknown at compile time arrives with function parameters.
