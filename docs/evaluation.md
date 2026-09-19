# Evaluation protocol

**Status: draft. Not yet pre-registered.**
The decision rule in §6 becomes binding when it is committed and its commit hash is
recorded in `docs/evaluation-preregistration.md`. Until then it is a proposal and may
change freely. After that, it may not.

---

## 1. The question

Does a new language reduce the cost of building and maintaining backend services with
coding agents, **over and above what better tooling on an existing language achieves?**

That last clause is the whole experiment. Without it, any improvement gets attributed to
the language by default, which is the mistake this protocol exists to prevent.

## 2. Arms

| Arm | Contents | Isolates |
| --- | --- | --- |
| **A1** | TypeScript, baseline tooling (`tsc`, `eslint`, `prettier`, stock agent loop) | starting point |
| **A2** | TypeScript **+ improved agent tooling**: structured diagnostics with machine-applicable fixits, canonical formatter, repeat-edit detection, explicit completion rules | **the tooling contribution** |
| **A3** | Nazm + tooling of comparable capability | the **additional** language contribution |

Go is run as a **runtime performance and resource reference only**. It is not an arm:
runtime performance and agent development efficiency are different questions and are
reported separately, never combined into one score.

The primary comparison is **A3 against A2**. A3-against-A1 is reported but decides nothing.

## 3. Tasks

### Backend (primary domain)

| Task | What it tests |
| --- | --- |
| Validated CRUD API | Basic language usability, JSON, diagnostics |
| Transactional inventory reservation | Correctness across concurrent requests |
| Tenant-isolated API | Authorization and data-access rules |
| Idempotent webhook handler | Retries, duplicate delivery, persistence |
| Cancellable background job | Resource cleanup and task lifecycle |
| **Feature change in an existing service** | Agent comprehension and regression avoidance |

Each is a family, instantiated into multiple concrete tasks. Target 30–50 total.

A smaller ETL and CLI sample runs alongside to test whether another domain shows a
stronger advantage. Backend remains the working choice; it is revised if the evidence
warrants, and that revision is reported.

The maintenance task matters most and is the one most benchmarks omit. Short greenfield
algorithm problems are the easiest thing to measure and the least like the work.

### Three sets, and a freeze

Hiding acceptance tests from the agent is necessary but not sufficient. If Nazm, its
wrappers, and its diagnostics are tuned against the same tasks for a year, the final
benchmark rewards specialisation to that set rather than language quality.

| Set | Permitted use |
| --- | --- |
| **Development** | Language design, diagnostics, wrappers, debugging. Opened freely |
| **Calibration** | Variance estimation, power analysis, protocol rehearsal |
| **Holdout** | Gate evaluation only, **after implementation freeze**. Never opened before |

**Holdout isolation is operational, not type-level.** The harness refuses to hand back a
holdout fixture without a `Freeze`, and a fabricated `Freeze` fails when verified against
the repository — but that guards *the harness API*, not the filesystem. Anything with read
access to the corpus directory can bypass it, and repository history, logs, and CI
artifacts can all leak fixture content. For the gate run the holdout corpus must live
outside the agent's accessible environment entirely: a separate host or credential-gated
store, fetched after the freeze and never mounted where the agent can reach it.

The freeze is a git tag. Everything under evaluation — compiler, tooling, prompts,
wrappers — is fixed at that tag, and the holdout run uses only that tag.

## 4. Fairness conditions

**Held equivalent:** task requirements, evaluation rules, and the independent acceptance
tests, which the agent never sees and which execute **outside the agent's writable
environment**.

**Allowed to differ, and recorded:** idiomatic implementation style, language-specific
diagnostics. Forcing one language to imitate another's idioms measures the wrong thing.

**Comparable starting codebases.** For maintenance tasks the TypeScript and Nazm services
must be functionally equivalent and of comparable quality. A crufty TypeScript repo
against a freshly simplified Nazm one measures codebase quality, not language.

**Aligned semantic assistance.** If Nazm ships `reserveInventorySafely()` while TypeScript
must hand-roll the transaction, the experiment measures library abstraction. The helper
surface available to each arm is inventoried, aligned, and published with the results.

**"Same budget" is pinned down.** Dollar, token, turn, and wall-clock caps are four
different constraints. The pre-registration names which bind and fixes model versions,
reasoning-effort settings, tool access, dependency availability, and context configuration.

**Arms are interleaved** within each trial block, so drifting service conditions do not
land disproportionately on one arm.

## 5. Metrics

Primary:

```
cost per accepted solution = total cost of all attempts / number of accepted solutions
```

**This metric is perverse alone and is never reported alone.** An arm accepting 40/100 at
$40 scores $1.00, beating an arm accepting 90/100 at $100 scoring $1.11 — while failing
most of the work. Always reported with it:

- acceptance rate (absolute, and delta against A2)
- per-task-category breakdown
- wall clock
- regressions introduced (maintenance tasks)
- human interventions required
- unsuccessful repair cycles

**Cost** is three separate lines, never silently summed into one:
model charges · execution infrastructure · logged human intervention.

**All attempts count, including failures.** Post-success activity is *categorised*
(cosmetic / revert / performance / security / edge case) to explain where cost went — the
categories are analysis labels, never a licence to exclude activity from the denominator.
Some reverts repair real regressions.

## 6. The decision rule

> **Not yet binding.** See the status note at the top.

Let `p` be acceptance rate and `C` cost per accepted solution:

```
Δp  = p₃ − p₂
R_C = C₃ / C₂
```

Uncertainty is estimated **on the difference and the ratio themselves**, not by checking
whether two separate confidence intervals overlap — those are different questions and the
second is not a test.

**Repeated runs of the same task are not independent samples.** The Tokenmaxxing study
found problem identity explains 73–97% of cost variance. Task enters the model as a random
effect, with model (the LLM) as a second grouping factor. Aggregation across models and
categories is fixed in the pre-registration, not chosen after seeing results.

### Cost policy

Two defensible standards exist. **The second is chosen:**

| Policy | Requirement | Status |
| --- | --- | --- |
| Evidence of ≥20% savings | upper confidence bound of `R_C` ≤ 0.80 | **rejected** — too strong for a 30–50 task benchmark with agent-level noise; would reject a real effect |
| Estimated 20% savings, with evidence of some savings | point `R_C` ≤ 0.80 **and** upper CB of `R_C` < 1.00 | **chosen** — appropriate for a research gate; trial count is powered for it |

### Continue — requires all

| Condition | Threshold |
| --- | --- |
| Cost | point `R_C` ≤ 0.80 **and** upper CB of `R_C` < 1.00 |
| Non-inferiority | lower CB of `Δp` ≥ −5 percentage points |
| Absolute usability floor | A3 acceptance ≥ 60% absolute — relative non-inferiority alone permits both arms to be unusable |
| No category collapse | no task category where A3 acceptance < 70% of A2's |
| Maintenance holds | on *feature change in an existing service*: acceptance not lower **and** regression count not higher than A2. Cost reported but not gating — the maintenance sample is small |

### Redesign

Cost fails but every acceptance condition holds, **or** the gain sits in a single category.
Identify which mechanism underperformed, take **one** targeted iteration, re-gate under a
fresh pre-registration.

### Stop

Any acceptance condition fails and cannot be attributed to a fixable defect in the
harness, **or** the result is inconclusive.

**Inconclusive means stop.** The trial count is fixed in advance by power analysis at the
end of Phase 3. There is no "run more trials if it is trending our way" clause: that
requires a prespecified sequential-analysis boundary, and without one it is optional
stopping. Any further experiment is a separate, separately pre-registered evaluation.

### Tooling capture — reported, never a stop rule

Capture = `(C₁ − C₂) / (C₁ − C₃)`, the fraction of the total improvement attributable to
tooling alone.

An earlier draft made "capture ≥ 80% ⇒ stop" an automatic rule. **It contradicts the
continue rule.** At `C₁=$100, C₂=$20, C₃=$15`: Nazm is 25% cheaper than A2, so continue
fires; capture is `80/85 ≈ 94%`, so stop also fires. Both cannot bind.

A large tooling win does not make an additional language win worthless. Capture is
published as a descriptive figure. The investment decision rests on A3's incremental
benefit over A2, its reliability, and the cost of building and maintaining the language —
the last being a judgement, made explicitly and recorded, not a threshold.

### Chronology, not just consistency

A registration that merely *agrees* with the documentation proves nothing: both files can
be written in one commit after the results have been examined. The registration is
therefore bound by git ancestry — every attempt record names the registration commit, and
that commit must be a strict **ancestor** of the run commit. A registration written after
a run cannot be an ancestor of it, so the ordering is checkable rather than trusted.

A registration binds the protocol and analysis rules (its own content), the evaluated
implementation (the freeze tag), the task sets (a corpus digest), and its version. A later
protocol change creates a **new** registration; earlier runs keep pointing at the one they
ran under and stay interpretable under the rules that applied to them.

## 7. What the gate establishes

**It is the primary comparative evaluation of Nazm's agent-development benefit. It is not
proof of the project.** It establishes whether Nazm adds value *for these tasks, these
models, and these configurations*. Nothing more.

It does not establish that Nazm is a good language, that the design is sound, that the
results generalise to other domains, or that the advantage persists as either arm matures.

## 8. Harness validation

Validated **by construction**, not by agreement with published results.

Known-answer tests must confirm correct handling of:

- each outcome class — passing, wrong answer, compile failure, crash, timeout
- an agent that modifies the tests, or invokes the wrong executable
- missing token-usage records, retries, interrupted runs
- state leaking between attempts
- a solution that passes the visible tests but fails the independent acceptance tests

**Missing cost data invalidates or explicitly qualifies a result. It must never silently
become zero cost** — that single failure mode can manufacture an arbitrary winner.

Then report what the runs show, **including results that contradict the published
direction**. A different outcome can come from different models, tasks, or conditions; it
is not evidence the harness is broken. An earlier draft of the plan had this backwards and
said a non-reproducing result meant the harness was wrong. That assumes the conclusion.

## 9. Prior results this protocol is calibrated against

Cited as context and as a sanity check on the instrument — **not** as thresholds, and not
as findings this protocol expects to reproduce.

- [Dan Luu, *How does programming language affect token efficiency and correctness?*](https://danluu.com/pl-tokens/)
  — language *popularity* predicted cost and correctness better than density, on two real
  tasks. He explicitly warns against generalising from two tasks.
- [*The Best Programming Language for Tokenmaxxing*](https://arxiv.org/html/2607.22807v1)
  — 2,000 trajectories, 5 models, 4 languages, 100 contest problems. Terse OCaml consumed
  1.3–1.7× Python's output tokens. Problem identity explained 73–97% of variance.

Figures from that paper are cell-specific and are **not** general properties of coding
agents: the "92% of failures are compile errors" result is the Gemma–OCaml cell, and the
"3.6× post-success tokens" result is Gemma–Rust medians.
