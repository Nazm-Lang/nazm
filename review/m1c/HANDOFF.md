# M1c handoff — multiple functions and non-recursive direct calls

**Base commit:** `8d67a7dcf89b6c4d4a8f3a6ad371e10e57ef173d` (`fix: submission identity covers what the grader can see, and`)
**Working tree:** uncommitted. 23 paths modified or added.
**Prepared:** 2026-09-20T18:03:51Z · local handoff artifact, nothing published.

## What M1c adds

Several top-level functions; `Int` and `Bool` parameters and returns; immutable
parameters; direct named calls including **forward references**; nested calls, and calls
in conditions, branches, comparisons and arithmetic. `fn main() -> Int` remains the entry
contract.

## What it refuses, and why that is not a language error

**Recursion — direct and mutual — is refused by the backend** (`N0101`) and still runs
under `nazm run`. Cycles are found by one depth-first search over the call graph, so
`f → f` and `f → g → f` are the same condition rather than one being a special case.

**Every declared function is validated, reachable or not.** A cycle between two functions
`main` never calls is still refused. Checking only reachable functions would introduce a
dead-code reachability rule as a side effect of a backend limitation, so that deleting a
call elsewhere silently changed whether a function was diagnosed — a language-visible rule
arriving by accident.

Arity, argument types, unknown callees and duplicate definitions stay the **checker's**
business (`N0303`, `N0300`, `N0201`, `N0203`). A backend limitation is never reported as
a language error, and the reverse would be worse.

## Symbols

Nazm functions are `@nz.f<N>`, numbered from collection order. **No source name is ever
concatenated into an LLVM identifier** — a name would need escaping rules there, and the
escaping is the bug. The source name appears only in a trailing comment. The runtime
helper is `@nz.fail`, in the same reserved space.

The platform entry point `@main` is a **wrapper** that calls Nazm's `main` and prints its
result; Nazm's `main` is an ordinary `@nz.f<N>`. So a program cannot collide with the C
entry point, the runtime helper, or `printf`/`write`/`exit`, and **only the wrapper
prints** — a test asserts a helper produces no output.

## Files

| | |
| --- | --- |
| `HANDOFF.md` | this file |
| `direction.md`, `roadmap.md` | current, complete. The roadmap carries the M1c accepted/rejected table and the cycle policy |
| `tracked.diff` | ordinary unified diff of all tracked files (excludes `Cargo.lock`, below) |
| `cargo-lock.diff` | the `Cargo.lock` change on its own |
| `src/` | complete current copies of every file that matters, including the new untracked ones |
| `verification.md` | every command, its result, and what was **not** exercised |
| `multi-function.nz` / `.ll` | the representative program and its emitted IR |

## Next increment — proposed, not started

**M1d: recursion.** It is the one refusal M1c adds, and the only thing between this
backend and a compiler written in Nazm — a recursive-descent parser is recursion.

The work is not in the emitter: LLVM emits a self-call without complaint, and the
representation already supports it. It is in **deciding what happens when a recursive
program exhausts the stack**, which is a specification question this project has already
answered once for the interpreter (`--max-depth`, a stack sized from the budget, `N0403`).
The native answer must be chosen deliberately, and the candidates differ in what they
promise:

1. **Do nothing** — let the OS deliver `SIGSEGV`. Cheapest and wrong: `docs/user/spec.md`
   promises a program *stops with a diagnostic*, and a signal is not that. It is the same
   mistake `llvm.trap` would have been.
2. **A depth counter** in the generated code, checked on entry, reporting `N0403`. Honest,
   matches the interpreter, costs an increment and a branch per call.
3. **A stack-limit probe** against a recorded bound. Cheaper per call, needs a runtime
   that knows the stack's extent — which is `nazm-rt`, and out of scope.

Removing the cycle diagnostic is then a deletion, and the tests that assert the refusal
become tests that assert the recursive program *runs*. Deciding between (2) and (3) is the
founder's call, and (2) is the one that keeps the two implementations reporting the same
diagnostic for the same program.
