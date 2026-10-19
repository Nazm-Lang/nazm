# ExecPlans

An ExecPlan is the written plan for a material change, kept beside the work so a later session —
or another agent — can resume it without the conversation that produced it. Governed by
`docs/internal/agent-governance.md` §5; the template is `.agent/templates/exec-plan.md`.

## When one is required

A task needs an ExecPlan when it:

- changes stable semantics (anything `docs/user/spec.md` or `docs/user/stability.md` owns);
- spans several crates or subsystems;
- introduces a new runtime or platform responsibility;
- requires a migration;
- changes a public API or a machine schema;
- affects containment, the mutation runner or the release path;
- is expected to take several implementation stages or sessions.

A typo, a one-file documentation fix or a focused test needs no plan; a plan for those is ceremony.

## Where it lives

`.agent/plans/<change-id>.md` while the work is active. When the change has an OpenSpec change, the
plan's design lives in that change's `design.md` and the ExecPlan holds the execution parts
(stages, evidence, handoff state) and links to it — no field is written twice. A finished plan is
kept with the change's archive or deleted once its handoff is committed; it is never an authority.

## What it contains

Every field of `.agent/templates/exec-plan.md`. A plan missing *non-goals*, *invariants*,
*acceptance* or *stop conditions* is not a plan: those are what bound the work.
