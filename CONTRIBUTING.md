# Contributing to Nazm

Thank you for looking. Nazm holds itself to a few rules that are unusual, and each exists because
breaking it once cost something. This page is the short version; `docs/master-architecture.md`
(the laws) and `docs/runbook.md` (operations) are the long one.

## Build and test

```sh
cargo build --workspace
cargo fmt --all -- --check
cargo lint                      # clippy with -D warnings, exactly as CI runs it
cargo xtask check               # architectural, documentation and catalogue gates
cargo xtask workspace-tests --host-safe --jobs 4 --workers 1 --test-threads 4 --shape ci
cargo test -p xtask --test lifecycle -- --ignored --test-threads=1
nazm fmt --check compiler/*.nz examples/*.nz    # Nazm sources have one layout too
```

The toolchain is pinned in `rust-toolchain.toml`; `clang` is required.

## Containment is not optional

The generated compiler stages, the selfhost suite, `compiler/bootstrap.sh`, fuzzing, the mutation
catalogue and heavy benchmarks run **only** through the bounded runner:

```sh
cargo xtask contained tests       # the whole suite, selfhost included
cargo xtask contained selfhost
cargo xtask contained bootstrap
cargo xtask contained mutate --only NAME
```

It needs Docker. One contained workload at a time. Never set `NAZM_CONTAINED=1` on a host and never
use `NAZM_ALLOW_UNCONTAINED=1`: an unbounded compiler stage once took the development machine down,
and the guards that refuse to run uncontained are not to be weakened. CI runs the host-safe subset
only; a change touching the compiler, runtime or self-hosted compiler needs the contained suite
too.

## The constitution comes before the code

- **Semantics first.** A change to what a program means is written into `docs/spec.md` — and, if
  architectural, `docs/architecture.md` — before it is implemented, in its own commit or at least
  its own section. A rule the checker gains bumps `SEMANTIC_EPOCH` in the same commit
  (`cargo xtask check` says so); a runtime contract change bumps the runtime ABI revision; a
  changed machine-readable shape bumps its schema's major version (`docs/stability.md`).
- **Diagnostics.** A new refusal gets a new code — codes are never reused or repurposed — a
  message that names the construct, and, where the compiler can tell, a fix. Schema changes follow
  `docs/diagnostics.md`, *What compatibility means*, and each schema in `schema/` is documented there.
- **Refuse by name.** An unsupported construct is rejected with a diagnostic that names it, never
  misparsed into something else or silently ignored.

## Compatibility within 1.x

Nazm 1.x promises that a program valid under stable Nazm 1.0, using only stable public surfaces,
does not silently change meaning ([`docs/stability.md`](docs/stability.md)). A change that would
alter what a stable program means is a 2.0 change, or the fix of a recorded defect against the
specification. [`tests/compat/v1/`](tests/compat/v1) holds every 1.x toolchain to that: it is
append-only, every file is pinned by its digest, and a new stable rule adds a case with its pin
(`tests/compat/v1/README.md`). Never edit a case to make a change pass.

## Licensing of contributions

Contributor terms (DCO, CLA, or inbound-equals-outbound) are not yet decided; that decision, and
whether the runtime and standard library carry an exception, are recorded as open in
[`docs/licensing.md`](docs/licensing.md).

## Evidence, in its own vocabulary

`docs/capability-matrix.md` is the status authority. Use its words — VERIFIED, VERIFIED as scoped,
PARTIAL, DESIGNED, RESEARCH, BLOCKED, MISSING — and cite the test or recorded run behind a claim.
Goals (`docs/NAZM_LANGUAGE_GOALS.md`) are never evidence. Report outcomes as **passed**,
**skipped**, **refused**, **failed** or **unexecuted**, and never one as another.

- **Mutation testing.** A change to compiler or runtime behaviour adds or updates entries in
  `xtask/mutations/mutations.toml`, each naming the test that kills it; a mutant no test catches is
  a gap to fix, not a number to report. Zero survivors describes a catalogue, never the absence of
  bugs.
- **No unchecked performance claims.** A number in `docs/performance.md` comes with the command
  that produced it and is registered in `bench/claims.toml`; "faster" without a measurement is not
  written.
- **Limitations honestly.** What a change does not do goes into `docs/limitations.md`, naming its
  capability-matrix area. Narrowing a limitation says what it was before. Never describe a goal as
  a feature, or a compile-only target as run-verified.

## Commits

One coherent change per commit, with a message that says what changed and why. Do not reformat
unrelated code in the same commit.

## Security

Report vulnerabilities privately — see [`SECURITY.md`](SECURITY.md).
