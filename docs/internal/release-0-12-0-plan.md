# Release 0.12.0: the plan, by phase

Same shape as [the 0.11.0 plan](release-0-11-0-plan.md): fixed IDs, a status and what each step waits on.
The scope is what the copyright holder assigned to 0.12.0 on 2026-11-05 and 2026-11-07
(`decisions-log.md`). Nothing starts before 0.11.0 is published.

Status as of 2026-11-08 (update it with every change).

## A. More targets: Linux, WebAssembly and WASI

| ID | Step | Old task | Status | Waits on |
|---|---|---|---|---|
| A1 | The Nazm-written pipeline builds for Linux (`aarch64-` and `x86_64-unknown-linux-gnu`), `wasm32-unknown-unknown` and WASI (`wasm32-wasip1`); today it refuses them by name | R3 8.3 | Not started | 0.11.0 published |
| A2 | The code generator's output runs on `x86_64-unknown-linux-gnu`, which needs an amd64 contained image | R9 6.3b | Not started | A1 |

## B. Retire the Rust bootstrap chain

| ID | Step | Old task | Status | Waits on |
|---|---|---|---|---|
| B1 | `parse`, `analyse`, `check`, `emit` and `module` Rust-reference stages replaced by their Nazm versions | R2 2.8b | Not started | 0.11.0 published and proven |
| B2 | The self-host test's list of built-ins not yet carried (about 90) emptied | R2 4.3b | Not started | B1 |

## C. The command-line tools

| ID | Step | Old task | Status | Waits on |
|---|---|---|---|---|
| C1 | The remaining commands (`bindgen`, `contract`, `accel`, `bench`, `profile`, `explain-cost`, `explain-flow`, `obligations`, `trace` and others) written in Nazm | R5 11.2 | Not started | 0.11.0 published |
| C2 | A statement's `;` dropped in both formatters, with the repository's own sources reformatted | R5 2.6 | Not started | C1 (it reformats `compiler/` and `library/`) |

## D. Faster builds

| ID | Step | Old task | Status | Waits on |
|---|---|---|---|---|
| D1 | Mutation runs group leaf crates so consecutive mutants rebuild less (measure first; only if the gain is real) | R1.5 8.3b | Not started | 0.11.0 published |
| D2 | Make the whole-repository `context` test fast: one test, `every_definition_of_the_repository_has_a_packet_every_layer_agrees_with`, takes about 1000 s (744 s even spread over every core, because one large document dominates). Find why a packet for each definition costs so much in that document and fix the cost in the service, not the test; it is the longest job in CI | Measured 2026-11-09, not started | 0.11.0 published |

## E. Housekeeping

| ID | Step | Status | Waits on |
|---|---|---|---|
| E1 | Retitle the headings of the `openspec` task lists with the plain step names and IDs | Not started | 0.11.0 published |
| E2 | Delete the old branches on GitHub that still carry the pre-rewrite history | Done 2026-11-08: 41 merged or redundant branches deleted (all backed up in a local bundle); kept `main`, `cla-signatures`, the two open-PR branches and four branches with no PR (`r8-s14a-patch`, `r8-s14b-misc`, `r8-s14m`, `r8-slice14b`) | |
| E3 | Require the Windows job in branch protection. The job runs only on pushes to `main`, never on pull requests, so requiring it as it is would block every merge. First make the job run on code pull requests (a CI change, three operating systems times four shards of GitHub time), then require it | Not started; approved by the holder 2026-11-08, needs the CI change first | The CI change |

## F. Gate and ship

The same gate and release steps as 0.11.0 (its phases A, D and E), run again on the 0.12.0 tree:
contained selfhost, bootstrap, selfbuild and tests; fuzz; the mutation catalogue; the `build.sh` modes;
bench last and alone; then version bump, archives for every supported target, provenance, tag, release,
nazm.dev and homebrew-tap.

| ID | Step | Status | Waits on |
|---|---|---|---|
| F1 | Gate | Not started | A to D |
| F2 | Bump, archives, provenance, tag, release, site, formula | Not started | F1 |
