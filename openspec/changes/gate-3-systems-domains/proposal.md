## Why

Gate 3 of the v1 programme (`04_GATE_3_SYSTEMS_AND_DOMAIN_COVERAGE`) makes every target area v1
intends to claim real enough to deserve its claim: hosts with prebuilt toolchains, a systems and
embedded foundation, a stronger realtime contract, domain profiles with reference workloads, and a
domain matrix every claim points to. Gate 2 is closed at `3a2e3ba`; this change starts there.

## What Changes

The design is `docs/systems-domains.md`, written before the code. Steps 3B–3L:

- Hosts: Linux x86_64 execution, prebuilt archives and clean installs; a Win32 runtime port run under
  Wine in Docker (labelled so; real Windows execution stays a final-v1 item).
- Systems: `Atomic`, `static`, `@section`, board manifests, `@interrupt`, a board arena, a failure
  hook; a kernel-style workload. Embedded: Cortex-M or its blocker, `@std/hal`, timers.
- Realtime rules and a fixed-priority board scheduler; robotics, industrial, high-assurance,
  aerospace, automotive, medical-style and cyber profiles as compositions, each with a workload.
- AI/HPC CPU provider and float kernels; a storage engine; a distributed foundation; WASI; Android
  and iOS libraries (iOS once Xcode is installed); a desktop host; web3 qualification;
  requirements traceability (`nazm trace`); `docs/v1-domain-matrix.md`.
- Versions move as each step requires, recorded per step.

## Capabilities

### New Capabilities
- `systems-domain-coverage`: the observable requirements of each part. Archived with
  `--skip-specs`: the normative text is `docs/spec.md` and the authorities.

### Modified Capabilities
None.

## Impact

`nazm-runtime` (platform layer, boards, Windows), `nazm-lir` (targets), `nazm-core`/`nazm-sema`
(atomics, statics, attributes), `nazm-service` (profile rules), `nazm-cli` (manifests, trace,
release), `library/std`, `examples/domains/`, docs and the domain matrix.
