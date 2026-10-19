# Change governance

How a material change to Nazm is proposed, designed, carried out and closed. Adopted in M1,
2026-10-08, with OpenSpec (`@fission-ai/openspec`, CLI 1.6.0, schema `spec-driven`) as the
**change-governance layer** — and only that.

## The authority rule

> **OpenSpec records proposed and in-flight changes. It is not the final authority for current
> Nazm semantics or implementation status.**

The authorities stay where they were (`AGENTS.md`, *Authority*): `docs/internal/capability-matrix.md` for
status and evidence, `docs/user/spec.md` for semantics, `docs/internal/master-architecture.md` and
`docs/internal/architecture.md` for laws and designs, `docs/user/stability.md` for compatibility,
`docs/user/limitations.md` for boundaries. An accepted change updates **them**; an OpenSpec change that
was archived without updating them is not done.

Consequences, each enforced or stated:

- **No language specification in OpenSpec.** A delta spec states the change's observable
  requirements and cites the authority sections it changes; it never restates `docs/user/spec.md`.
- **Language and toolchain changes archive with `--skip-specs`.** Their normative text is in the
  authorities, so nothing accumulates in `openspec/specs/`.
- **`openspec/specs/` holds repository-process capabilities only** — today `repository-governance`.
  The `agent governance` gate in `cargo xtask check` fails on any other.
- **History is not migrated.** N1–N108 and Gates 1–2 stay recorded where they are
  (`docs/internal/architecture.md` §7, the capability matrix, `docs/internal/general-purpose.md`).

## When a change needs one

A **proposal threshold**: an OpenSpec change is required for a change that alters what a program
means, a stable surface (`docs/user/stability.md`), a schema, the runtime ABI, the standard library API,
a target or profile, containment or the release path, or that spans several crates. The same
conditions call for an ExecPlan (`.agent/PLANS.md`); the two share fields rather than duplicate them.

**The tiny-change exception:** a typo, a broken link, a comment, a focused test, or a one-file fix
that changes no behaviour and no stated rule needs neither — the commit message is the record.

## The lifecycle

1. **Read the authorities** that own the change.
2. **Propose:** `openspec new change <id>`, then `proposal.md` — why, what changes, the authority
   sections touched, and whether the epoch, runtime ABI, a schema or the std API moves.
3. **State requirements:** `specs/<capability>/spec.md` with `## ADDED|MODIFIED|REMOVED Requirements`,
   each requirement with at least one `#### Scenario:` — observable behaviour, citing the authority.
4. **Design:** `design.md` — the existing mechanism considered, why it is not enough, decisions,
   risks. (`docs/internal/general-purpose.md`'s *rule every mechanism follows* is the same discipline.)
5. **Tasks:** `tasks.md`, ending with the authority updates and the evidence.
6. **Implement**, following `CONTRIBUTING.md`: semantics before code, version bumps in the same
   commit, a new refusal a new code.
7. **Test and mutate:** focused tests, the mutation catalogue's responsibility-mapped entries,
   contained runs where the runbook requires them.
8. **Update the authorities:** spec, architecture, stability, limitations, capability matrix.
9. **Verify and archive:** `openspec validate <id> --strict`, then
   `openspec archive <id> --yes` (repository-process capability) or
   `openspec archive <id> --yes --skip-specs` (everything else).

Run every OpenSpec command with `DO_NOT_TRACK=1`: the CLI otherwise sends usage telemetry. No
correctness test needs the CLI, Node or the network; the `agent governance` gate checks OpenSpec's
structure from Rust.

## The agent workflow

An agent making a material change follows the same nine steps, recorded as it goes:

1. read `AGENTS.md`, the nested `AGENTS.md`, and the owning authority sections;
2. create or update the OpenSpec change;
3. state the observable requirements;
4. document the design;
5. list the tasks;
6. implement;
7. test and mutate;
8. update the authoritative Nazm documents;
9. validate and archive, and leave a handoff (`.agent/templates/handoff.md`).

The tiny-change exception applies to agents exactly as to people.

## The pilot

M1 ran the lifecycle once on a non-semantic change, `m1-agent-governance-gate`: the `agent
governance` gate in `cargo xtask check` (`xtask/src/governance.rs`). Its proposal, design, delta
spec and tasks were written, the gate implemented with unit tests that break each rule, `openspec
validate --strict` run, and the change archived with its `repository-governance` spec — now
`openspec/specs/repository-governance/spec.md` — under `openspec/changes/archive/`. The outcome of
each step is in the M1 report.
