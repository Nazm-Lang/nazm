## Why

M1 makes agent behaviour a repository-owned interface: a root `AGENTS.md`, nested ones for
`compiler/`, `crates/nazm-runtime/` and `xtask/`, a `CLAUDE.md` shim, `docs/agent-governance.md`,
`.agent/` templates, and an OpenSpec layer. Instruction files drift the way any duplicated
vocabulary does: a shim that stops pointing to its source, a link to a renamed authority, a
template that loses a required field, a nested file that quietly relaxes a root prohibition. None
of that is caught today, because nothing reads these files.

## What Changes

- A `cargo xtask check` gate, **agent governance**, that reads the tree (builds nothing, no
  network) and fails when:
  - `CLAUDE.md` does not defer to `AGENTS.md`;
  - a document `AGENTS.md` links to does not exist;
  - a nested `AGENTS.md` does not name itself as specialising the root, or grants an action the
    root forbids;
  - an `.agent/templates/` file is missing or lacks a required field;
  - an OpenSpec change directory lacks its `proposal.md`, or `openspec/specs/` holds a capability
    that is not a repository-process capability.
- No change to Nazm semantics, the toolchain, schemas, the runtime or the standard library.

## Capabilities

### New Capabilities
- `repository-governance`: the checks that keep the repository's agent-instruction and
  change-governance files consistent with their authorities.

### Modified Capabilities

## Impact

`xtask/src/` only (a new gate module and its registration in `run_checks`). Authorities touched:
`docs/agent-governance.md` §14 and `docs/change-governance.md` describe the gate. Semantic epoch,
runtime ABI, schemas and std API: unchanged.
