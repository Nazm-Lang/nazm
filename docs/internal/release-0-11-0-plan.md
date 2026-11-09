# Release 0.11.0: the plan, by phase

The build stages (B1 to B14) are the steps of making the language; they are in
[`how-nazm-was-built.md`](how-nazm-was-built.md) and are all done. This page is the plan for
shipping 0.11.0, in five phases with fixed IDs. A step starts only when the steps it waits on are done.

Status as of 2026-11-09 23:00 (update it with every change).

## A. Gate: prove the code

| ID | Step | Status | Waits on |
|---|---|---|---|
| A1 | Contained selfhost, bootstrap, selfbuild and tests | Done | |
| A2 | Fuzz, 5 targets, 600 s each | Done | |
| A3 | Host nextest | Done (the 41 selfhost refusals are expected on the host) | |
| A4 | Dell Windows rerun | Done: host 70 agree, 0 differ; toolchain 94 agree, 2 differ (httpd baseline) | |
| A5a | Targeted mutation campaign, the 2,081 mutants the fast pass decides | Done, 0 survivors | |
| A5b | The 32 mutants (11 catalogue entries) that need the baseline suite, run as a second campaign from main with a 7,200 s deadline | Running: 3 entries decided, one survived and was fixed (B5); the other 8 and that one are rerunning, in 6-hour sessions | |
| A6 | `build.sh` modes (llvm, cached, report-cached, lib, summary, timings) with BUILD_ARGS=1, and the `host_run` integration tests | Done: 899 agree, 0 differ; 161 agree, 0 differ; 17 of 17 | |
| A7 | Bench: no regression against `bench/baseline`, run last and alone | Not started | A5b |

## B. Repair: what the gate found

| ID | Step | Status | Waits on |
|---|---|---|---|
| B1 | Windows CI: `host_run` and ten Unix-host tests compiled only on Unix (#39, #41) | Done | |
| B2 | Green CI run on the rewritten main | Done | |
| B3 | Arm shard flake: the third-stage lock test waits out a forking neighbour (#42) | Done | |
| B4 | `--broad-deadline-secs` reaches the campaign through the contained passthrough and `mutate` itself; pass 2 skips the baseline suite once the budget is spent (#43, #44) | Done | |
| B5 | A test that did not test: the Cranelift cache-key test now checks the backend version names its optimisation level (mutant `n41-the-object-key-forgets-the-optimisation-level`), with a seed-freeze exemption (#45) | Done; the mutant is rerun in A5b | |

## C. Readability

| ID | Step | Status | Waits on |
|---|---|---|---|
| C1 | History rewritten with plain titles, force-pushed, trees identical | Done | |
| C2 | The how-Nazm-was-built guide, this plan and the naming rule (#40) | Done | |

## D. Ship

| ID | Step | Status | Waits on |
|---|---|---|---|
| D1 | Version bump 0.10.0 to 0.11.0 through a PR with green CI, a seed-freeze exemption and the release notes (`docs/user/releases/0.11.0.md`, drafted with `TODO(gate)` marks). A dry run is draft PR #46, not to be merged; its trap list is in the resume memory | Dry run done; the real bump starts after A | A, B |
| D2 | Contained selfhost, bootstrap and selfbuild on the bumped commit | Not started | D1 |
| D3 | Archives: `aarch64-apple-darwin`, `x86_64-apple-darwin` (macOS host selfbuilds) and `x86_64-pc-windows-gnu` (the Dell). The Dell must be reachable | Not started; the Dell was unreachable at 22:38 on 2026-11-09 | D2 |
| D4 | Sign `SHA256SUMS` with Sigstore (`scripts/sign-release.sh`). This is the holder's own step: it opens a browser login and is never run by an agent | Needs the holder; see `decisions-log.md` | D3 |
| D5 | Provenance, tag v0.11.0, GitHub release | Not started | D4 |
| D6 | nazm.dev deploy (`suhail/deploy.sh`; builds locally, reaches the server) | Not started; a local build and the server login were rehearsed on 2026-11-09 | D5 |
| D7 | homebrew-tap formula for 0.11.0: it is arm64-only today and must carry both Mac archives | Not started | D5 |

## E. After the release

| ID | Step | Status | Waits on |
|---|---|---|---|
| E1 | Tick the task-list items this release closes (R8 11.1 and 14.1, R1.5 8.1 and 8.4) | Not started | D7 |

Every gate rule still applies: no weak or deleted test, no relaxed safety rule, nothing published unless
every A step is green.
