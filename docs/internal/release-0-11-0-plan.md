# Release 0.11.0: the plan, by phase

The build stages (B1 to B14) are the steps of making the language; they are in
[`how-nazm-was-built.md`](how-nazm-was-built.md) and are all done. This page is the plan for
shipping 0.11.0, in five phases with fixed IDs. A step starts only when the steps it waits on are done.

Status as of 2026-11-08 (update it with every change).

## A. Gate: prove the code

| ID | Step | Status | Waits on |
|---|---|---|---|
| A1 | Contained selfhost, bootstrap, selfbuild and tests | Done | |
| A2 | Fuzz, 5 targets, 600 s each | Done | |
| A3 | Host nextest | Done (the 41 selfhost refusals are expected on the host) | |
| A4 | Dell Windows rerun | Done: host 70 agree, 0 differ; toolchain 94 agree, 2 differ (httpd baseline) | |
| A5 | Full targeted mutation campaign, 2113 mutants, 0 survivors | Running, 6-hour sessions | |
| A6 | `build.sh` modes (llvm, cached, report-cached, lib, summary, timings) with BUILD_ARGS=1, and the 9 `host_run` integration tests | Queued for the gap between campaign sessions | A5 session end |
| A7 | Bench: no regression against `bench/baseline`, run last and alone | Not started | A5, A6, B |

## B. Repair: what the gate found

| ID | Step | Status | Waits on |
|---|---|---|---|
| B-1 | Windows CI fix: `host_run` tests only on Unix (PR #39) | CI running | |
| B-2 | Green CI run on the rewritten main | Running | B-1 |

## C. Readability

| ID | Step | Status | Waits on |
|---|---|---|---|
| C1 | History rewritten with plain titles, force-pushed, trees identical | Done | |
| C2 | The how-Nazm-was-built guide, this plan and the naming rule (PR #40) | CI running | |

## D. Ship

| ID | Step | Status | Waits on |
|---|---|---|---|
| D1 | Version bump 0.10.0 to 0.11.0 through a PR with green CI (needs a seed-freeze exemption) | Not started | A, B |
| D2 | Contained selfhost, bootstrap and selfbuild on the bumped commit | Not started | D1 |
| D3 | Archives: aarch64 and x86_64 macOS, Windows on the Dell | Not started | D2 |
| D4 | Provenance, tag v0.11.0, GitHub release | Not started | D3 |
| D5 | nazm.dev deploy and homebrew-tap formula | Not started | D4 |

## E. After the release

| ID | Step | Status | Waits on |
|---|---|---|---|
| E1 | Tick the task-list items this release closes | Not started | D5 |

Every gate rule still applies: no weak or deleted test, no relaxed safety rule, nothing published unless
every A step is green.
