## Why

Gate 2 of the v1 programme (`03_GATE_2_GENERAL_PURPOSE_COMPLETENESS`) closes the foundational gaps
that keep Nazm out of ordinary application, server, numeric, storage and platform work. Its first
part landed before M1 (`f29a23c`–`b2e2985`: numbers, fixed arrays, constants, `Bytes`, `Text`;
epochs 31–33) and stopped on a checker defect fixed in M1-1 (`28e54f8`). M1, Q1 and M2-Core are
closed; this change carries the rest from the accepted baseline `fa94535`.

## What Changes

The design is `docs/internal/general-purpose.md`, written before the code; each section there states the
problem, the existing mechanism, why it is not enough, the semantic cost, the placement and the
evidence. Where the code is narrower, that section gains an *As built* paragraph and `docs/user/spec.md`
is the rule.

- §7 collections: `@std/hash`, `@std/hashmap`, `@std/hashset`, `@std/deque`; impls for numeric types.
- §8–§13: files and handles, networking, scheduler-aware I/O, time, process and environment, errors.
- §14 serialization, §16–§19 attributes, conditional compilation, user tests and benchmarks.
- §20 logging, §21 randomness, §22 package features, §23 the C foundation.
- §24 five reference applications; §25 `docs/internal/v1-competitor-map.md`.
- Semantic epoch, runtime ABI, schema and standard library API move as each part requires
  (`docs/user/stability.md`); the compatibility corpus stays append-only.

## Capabilities

### New Capabilities
- `general-purpose-foundation`: the observable requirements of each part, citing `docs/user/spec.md`.
  Archived with `--skip-specs`: the language's normative text is in the authorities.

### Modified Capabilities

## Impact

`crates/nazm-core`, `nazm-sema`, `nazm-runtime`, `nazm-cli`, `nazm-mir`/`lir` and both backends,
`library/std`, `docs/user/spec.md`, `docs/internal/capability-matrix.md`, `docs/user/stability.md`,
`docs/user/limitations.md`, the mutation catalogue, `examples/apps/`.
