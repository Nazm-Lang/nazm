# Migrating across Gate 2

Gate 2 (2026-10-07 to 2026-10-09, `docs/general-purpose.md`) lands before the first 1.0.0
release. It added mechanisms; it did not change what any program accepted before it means. This
is what a program, a build or a tool written before it meets, and what to do.

## A program that was accepted still means the same

Every step of Gate 2 moved constructs from **refused** to **accepted** — numbers of every width and
floats, fixed arrays and constants, `Bytes` and `Text`, handles, files, sockets, time, processes,
entropy, attributes and `@cfg`, exports of every number — and none moved a construct the other way
or gave an accepted program another meaning. The compatibility corpus (`tests/compat/v1/`) is the
check: its programs run with the outputs they had at 1.0's baseline. The one case it lists as
superseded, `refuse/float-literal.nz`, was a refusal: `1.5` is a float now
(`tests/compat/v1/SUPERSEDED`).

## A name may now be the language's

The new types and built-ins took names a program could have used for its own declarations. A
program that declares one is now refused by name, and the remedy is to rename the declaration:

| what | refused with | the names |
| --- | --- | --- |
| a record, enum or trait named as a new type | `N0331` | `Int8` … `UInt64`, `Float32`, `Float64`, `Bytes`, `Text`, `OsHandle`, `NetCap`, `RandomCap`, `ProcessCap` (`N0305`'s help lists every built-in type) |
| a function named as a new built-in | `N0203` | the numbers', bytes', text's and `os_` built-ins, `time_now_ns`, `time_wall_ns`, `time_sleep_ms`, `os_random_bytes`, … — every name in `crates/nazm-sema/src/intrinsic.rs` |
| a foreign declaration of a C symbol the runtime now calls | `N0383` | `RESERVED_C_SYMBOLS` in `crates/nazm-core/src/check/foreign.rs` (`strtod`, `sin`, `getentropy`, …) |

`@` was an unknown character; a file that never wrote one is unaffected.

## A build

- **Cached checks** filed by an earlier toolchain are not reused: the semantic epoch moved from 30
  to 42, and the epoch is part of every check's key. Nothing to do; the first check rebuilds them.
- **A runtime artifact** (`nazm runtime build`, `--runtime DIR`) built for runtime ABI 14 is
  refused by name: the ABI is 20. Rebuild it with this toolchain.
- **A lockfile** stays `nazm.lock/1` for a build with no enabled feature, byte for byte. A package
  that declares features and enables one is locked as `nazm.lock/2`; run `nazm lock` once.

## A tool reading Nazm's output

- `nazm.test/1` gained `kind` (`golden`, `test` or `fuzz`), and a case can now be a test function,
  named `STEM::FUNCTION`, whose `expected` is `passes` or `fails`. Additive: a consumer that
  ignores unknown fields, as `docs/diagnostics.md` requires, reads it unchanged.
- `nazm bench` and `nazm.bench/1` are new, and EXPERIMENTAL.
- New diagnostic codes, each compatible by `docs/diagnostics.md`'s rule: among them `N0412` (a
  shift out of range), `N0617`, `N0618`, `N0621`–`N0624` (numbers, arrays and constants) and `N0619`
  (an attribute). No code changed meaning.

## Not carried

The compiler written in Nazm (`compiler/`) does not carry Gate 2: it refuses the new numbers,
arrays, constants, `Bytes`, `Text`, attributes and their built-ins, and the bootstrap holds the
subset it does carry (`limitations.md`).
