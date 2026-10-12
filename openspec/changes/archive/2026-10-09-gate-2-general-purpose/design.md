## Context

The design record is `docs/general-purpose.md` (Gate 2A, `f29a23c`). It is not repeated here.

## Decisions

- **Order.** §7 first (it was in progress at the stop), then the outside world (§8 handles and
  files, §11 time, §12 process, §13 errors), then §9–§10 networking and the reactor, then §14,
  §16–§23, then the reference applications and the competitor map, then the contained gate.
- **One part, one constitution commit then one implementation commit** where the part changes
  semantics, as 2B–2G did; a library-only part may be one commit.
- **Narrower is allowed, wider is not.** Where the code does less than the record, the record
  gains *As built* and the matrix says what exists; no part does more than its section.
- **Stop discipline.** A real compiler semantic defect found mid-part stops the part and is
  reported (`docs/agent-governance.md`, *Stop conditions*).

## Risks / Trade-offs

- The reactor (§10) is a new runtime responsibility in two schedulers and three implementations;
  it is the part most likely to need narrowing, and is reported rather than invented if a sound
  design cannot be reached.
