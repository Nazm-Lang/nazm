# Security qualification

What Q1 (2026-10-08) examined in the implementation as it stood before Gate 2 widens it, what it
found, what it fixed, and what it carries. Findings are classified by `docs/quality-model.md`:
technical severity and programme disposition, never one score. Reporting is `SECURITY.md`; the
process after a report is `docs/vulnerability-process.md`.

The evidence is under `target/gates/q1/`: six audit reports (`audit-*.md`), their probes
(`probes/`), the fuzz campaign, the mutation campaign, the dependency audit and the secret scan.
Starting HEAD `4189318`; the fixes are commits `777ac98` onwards.

## Scope

Q1 qualified the surface as it exists — the language through Gate 2's numbers, arrays, constants,
bytes and text, both backends, the runtime, the toolchain commands, packages, caches, the
contained runner and the bench sandbox — not a future one. It claims nothing about arbitrary
foreign code a program calls, and it does not claim implicit-flow non-interference.

## Summary

| | |
| --- | --- |
| Findings | 95: 1 CRITICAL, 10 HIGH, 21 MEDIUM, 45 LOW, 18 INFO (two pairs are one defect seen twice: Q1-A1-08 and Q1-A2-03, Q1-D-21 and Q1-FI-02) |
| Release-blockers found | 8 — all fixed (one with a stated residual, below) |
| Gate-blockers found | 7 — all fixed or decided |
| Valid-source miscompiles found | 1 (the summation rewrite, Q1-B-01) — fixed |
| Cache hit changing a verdict | 1 (Q1-B-C1) — fixed |
| Profile bypasses within claimed scope | 4 (Q1-C-01…04) — fixed |
| Cleanup able to delete unowned resources | 1 (Q1-A2-01) — fixed |
| Compiler crashes on malformed input | 1 class (UInt64 products, Q1-B-03) — fixed |

## Attack surface

Every process the shipped CLI starts was read (`audit-a1-surface-unsafe-ffi.md`, §5): `clang` and
the linker with a cleared environment and fixed arguments, `dsymutil`, `ar`, `ssh-keygen` for
attestation, the system sampler, and the programs `nazm test`, `profile` and `accel` build and then
run. No command line is built by a shell from program or user data; `curl` in the agent bench takes
credentials on standard input; the only archive reader is xtask's, bounded and allow-listed.

Fixed: scratch directories that executables were built in and run from were predictable and
adopted an existing directory or symlink — now exclusive and 0700 (Q1-A1-02, -03); a comma in a
shared library's name reached `-Wl,` as another linker argument (Q1-A1-06); `-o` could overwrite a
non-entry module's source (Q1-A1-07). Carried: the object cache directory and `NAZM_TEST_DRIVER`
are trusted from the environment, like `PATH` (Q1-A1-13, INFO).

## Unsafe code and the C boundary

The workspace's Rust has no `unsafe` (the lint forbids it in every member); the fuzzer's coverage
callbacks are the one exception. The low-level layer is the runtime's hand-written LLVM text and
assembly — allocation, atomics, the context switch, guarded task stacks, device registers, a board's
`_start` — with no signal handlers (§6 of the A1 report inventories each).

Fixed: generated code asked for no stack probes, so a frame larger than a task stack's one guard
page could step over it into another task's stack (Q1-A1-01, the release-blocker); both backends
now probe, and the overrun faults on the guard. The guard page's `mprotect` results were ignored
(Q1-A1-10). Fixed before v1: such a frame now meets a check before its prologue lays it out and
fails with `N0408`; an export called on a small C thread is budgeted from that thread's stack
(Q1-A1-04); and on the 32-bit targets a C pointer crossing — which Nazm passes as 64 bits — is
refused by name (`N0381`) rather than miscompiled (Q1-A1-05). C's own behaviour once called is outside every claim (Q1-A1-16).

## Robustness

The fuzz campaign (contained, `target/gates/q1/fuzz-contained.log`): five targets — `front`,
`lower`, `run`, `lir`, `manifest` — 300 s each, 3.40 million executions, **no crash**; 36 inputs to
`run` blocked past two seconds and were counted, not kept (Q1-F-01). Malformed input by hand
(`audit-b-compiler.md`): truncated source, extreme nesting, 10 MB identifiers, 100,000-operator
chains, malformed generics, eleven malformed JSON baselines, corrupt and swapped cache entries and
LSP frames — refused cleanly, except: a UInt64 product past `i128` panicked the checker, the
interpreter and the LIR oracle (Q1-B-03, fixed); doubly nested records made type depth exponential
(Q1-A1-09, fixed); two symlinks back to a package's root hung resolution (Q1-A2-03, fixed). Carried:
huge diagnostics, quadratic checking of very wide records (Q1-B-09, -10), a huge LSP frame aborting
in the `lsp-server` dependency.

## Compiler correctness

Differential execution over the compatibility corpus, the examples, 37 numeric edge programs, 14
semantic probes, generated integer and float programs and 1,010 float values, through the
interpreter, the LIR oracle, LLVM at -O0 and -O2 and Cranelift. Emitted IR carries no `nsw`, `nuw`,
`inbounds` or fast-math flags; overflow is an intrinsic and a branch.

Found and fixed: **a miscompile** — the summation rewrite read the counter's start from its last
top-level `= 0` and missed a later write in a nested `if` or loop, so native code summed from 0
(Q1-B-01, CRITICAL); float text broke an exact tie between two equally short digit strings away
from even in the interpreter only (Q1-B-02); the specification contradicted itself on function
values crossing into tasks and on capturing capabilities (Q1-B-04, corrected to the checker).
Carried: `exit_with` outside 0..255 (Q1-B-05), exponential generic symbol names (Q1-B-06), the LIR
oracle's missing float formatting (Q1-B-07).

## Memory and concurrency

Memory holds (`audit-b2-memory-concurrency-cache.md`): MIR's ownership check rejects any function
owning a value at a return or failure exit; 24 probes — traps holding `Bytes` and `Text`, views
outliving owners, overlapping copies, closures in vectors, early returns from loops, out-of-memory,
task failure holding values, attempts past `N0616` — ended with no live value on every engine and
nothing under macOS's malloc checks. Concurrency holds as documented after about 2,000 probe runs;
one defect fixed — a pool task's select deadline past `i64` nanoseconds wrapped into the past and
spun a core (Q1-M-11). Fixed before v1: the wall-clock deadline (Q1-M-12). Carried: a blocking built-in holding a
worker (Q1-M-13), a failed task with a blocked sibling hanging silently (Q1-M-14, documented).

## Cache and incremental checking

The native object cache is sound: its key covers what decides the object, and every damaged entry
was recomputed. The check cache had two defects and a blind test: an entry's provenance facts were
used unchecked, so an edited sink list turned a refusal into `ok` (Q1-M-07); a module making a
closure filed an entry that cannot be read back, so the **next run refused what a cold check
accepts** (Q1-B-C1); and the cold/warm comparison read diagnostic fields that do not exist, so it
compared codes only (Q1-M-09). All three fixed — entries are `nazm.check/5`, sealed by a digest; a
closure-making module files no entry; whole diagnostics are compared. Fixed before v1: a warm check
named a reused module's parameter by position (Q1-M-08); it now names it from the signature.

## Profiles, capabilities, effects and provenance

253 bypass probes (`audit-c-bypass.md`). Effects, capabilities, task crossing and eleven profile
rules held against aliases, re-exports, generics, closures, function values, trait methods in other
modules, packages, cached interfaces and profile composition. Four bypasses were found and fixed:
`bounded-allocation` missed an allocation behind any call (Q1-C-01) and every allocating built-in
Gate 2 added (Q1-C-02); `signed-writes` missed a state built by a helper, field, array or trait
method (Q1-C-03); a `nazm.root` beside a package's entry dropped the manifest's profile (Q1-C-04).
Provenance: 85 explicit-flow probes through every mechanism, all refused; implicit flow is not
claimed. Carried: `signed-writes` is syntactic.

## Packages

No package construct names a program to run: manifests refuse unknown keys (now tested for
`build`, `links`, `[scripts]`, `[build-dependencies]`); builds call fixed host tools. The registry
refuses links and tampered digests; names cannot become paths; imports stay in the canonical source
root. Fixed: the symlink loop (Q1-A2-03); publish merging into a stale version directory, sealing
its extra files into the version (Q1-A2-04); a lockfile naming a package twice (Q1-A2-05). Carried:
path dependencies may name any directory (Q1-A2-12); lock digests only under `--locked`
(Q1-A2-13); unlocked index writes (Q1-A2-14).

## Containment

Fixed, release-blocker: the contained runner's reaper chose containers by name and judged liveness
with `kill -0`, which fails for a live process the user does not own — so Compose's
`nazm-postgres-1` (pid 1) or another user's run could be force-removed (Q1-A2-01). Containers now
carry an owner label with their supervisor's pid and host, and only such containers of this host
whose pid `ps` reports gone are removed. Fixed: `NAZM_CONTAINED=1` was a bare declaration — it is
now honoured only under a numeric cgroup `memory.max`, and `xtask mutate`, `xtask bench`,
`fuzz/run.sh` and `bootstrap.sh` refuse a host (Q1-A2-02); subnet overlap now compares prefix bits
(Q1-A2-08). Testcontainers was not adopted (`docs/integration-testing.md`); its ownership law is the
bench sandbox's, whose overstated wording Q1 corrected (Q1-A2-09). Carried: the contained workload
runs as root with default capabilities (Q1-A2-10, post-v1).

## Supply chain

`cargo deny check` — advisories, bans, licences, sources — passes (`target/gates/q1/deny-after.txt`)
against the advisory database fetched on 2026-10-08. It failed before Q1 and nothing ran it: every
workspace path dependency read as a wildcard, and `paste` (RUSTSEC-2024-0436) is unmaintained. The
first is allowed for `publish = false` members; the second is recorded with its path — a proc-macro
of `tokenizers`, reached only through the agent-cost harness, absent from the shipped `nazm`.
`cargo audit` reports that advisory alone. Seven crates appear at two versions (warnings, not
risks). All 238 locked crates come from crates.io with checksums, and `cargo xtask check`'s
**dependency sources** gate now holds that without a network. Not available here: `cargo-vet`,
`cargo-geiger`, an OSV scan.

## Secrets and sensitive data

`gitleaks` over all 671 commits: no leak. Tracked files hold no absolute home paths, no temporary
paths, no chat or prompt artefacts; `docs/prompts-from-chatgpt/` is untracked. `review/` holds the
early milestones' review bundles, tracked deliberately (`scripts/source-digest.sh`), with no secret.

## Failure injection

`target/gates/q1/failure-injection.md`: a missing or failing or killed `clang`, a read-only or
directory output, output naming the source, SIGINT and SIGTERM mid-build — each an explicit failure
with a defined status (4, or 128 + the signal) and no artefact at the output path. Interrupted
contained runs, campaigns, killed harnesses and timeouts are held by the lifecycle and sandbox tests
cited there. Not exercised: disk-full, an unavailable GPU or debugger.

## Release and installer requirements

No installer exists; a release is a local candidate (`scripts/release-candidate.sh`) with a
SHA-256 manifest verified on assembly, a provenance record (commit, clean tree, build-input digest
over every tracked file, toolchain, target, image, bootstrap record), a CycloneDX SBOM and signed
in-toto statements (`nazm attest`, SSH signatures with the caller's key — Nazm holds none). The
final release gate must, before anything is published:

1. re-run the `x86_64-apple-darwin` corpus under Rosetta 2 that `docs/stability.md` cites, and the
   full mutation catalogue;
2. if an installer or archive is added: refuse entries with `..`, absolute paths or links leaving
   the destination; never follow a symlink at the destination; write with explicit modes and set
   the executable bit only on the binaries; modify no `PATH` without consent; uninstall only what it
   installed; verify every download against the manifest's digest before use and refuse the wrong
   architecture; treat a truncated download as a failure;
3. make the repository public, enable private vulnerability reporting and confirm the fallback
   address (`SECURITY.md`, `docs/vulnerability-process.md`);
4. settle licensing (`docs/licensing.md`) — a publication blocker until the human decision is made;
5. ~~decide the exit statuses of usage errors and user errors (Q1-D-06, -07)~~ — decided and
   frozen in Q1-C1: 0 success, 1 invalid source/program/request, 2 run-time failure, 3 input not
   found or unreadable, 4 compiler/toolchain/environment failure, 5 command-line usage
   (`docs/stability.md`).

## Accepted limitations

`docs/limitations.md`, *Carried from Q1*, states each, so that no claim contradicts it.

## Finding register

Every finding, by area. Status: **fixed** (with its commit's test), **carried** (with its
disposition), **corrected** (documentation).

| Id | Finding | Severity | Disposition | Status |
| --- | --- | --- | --- | --- |
| Q1-B-01 | summation rewrite misses a counter write after `= 0` (miscompile) | CRITICAL | RELEASE-BLOCKER | fixed |
| Q1-A2-01 | reaper removes foreign containers by name and `kill -0` | HIGH | RELEASE-BLOCKER | fixed |
| Q1-A1-01 | no stack probes; a frame steps over the guard page | HIGH | RELEASE-BLOCKER | fixed; residual fixed before v1 (`N0408` before the frame exists) |
| Q1-B-02 | interpreter float text breaks ties away from even | HIGH | RELEASE-BLOCKER | fixed |
| Q1-B-03 | UInt64 product past `i128` panics | HIGH | RELEASE-BLOCKER | fixed |
| Q1-C-04 | `nazm.root` drops a manifest's profile | HIGH | RELEASE-BLOCKER | fixed |
| Q1-D-01 | `x86_64-apple-darwin` stable on one program's evidence | HIGH | RELEASE-BLOCKER | fixed: corpus run under Rosetta, claim states its evidence |
| Q1-B-C1 | a cache hit refuses what a cold check accepts | HIGH | RELEASE-BLOCKER | fixed |
| Q1-C-01 | `bounded-allocation` blind behind a call | HIGH | GATE-BLOCKER | fixed |
| Q1-C-02 | Gate 2's allocating built-ins not counted | HIGH | GATE-BLOCKER | fixed |
| Q1-C-03 | `signed-writes` bypassed by a state built out of sight | HIGH | GATE-BLOCKER | fixed |
| Q1-M-09 | cold/warm test compares codes only | MEDIUM | GATE-BLOCKER | fixed |
| Q1-B-04 | spec contradicts itself (spawn arguments, captured capabilities) | MEDIUM | GATE-BLOCKER | corrected |
| Q1-D-02 | six tests pass vacuously off their platform | MEDIUM | GATE-BLOCKER | fixed: ignored there; `NAZM_REQUIRE_HOST_TOOLS=1` fails a missing tool |
| Q1-D-03 | Gate 2's mutants never campaigned | MEDIUM | GATE-BLOCKER | see *Mutation* below |
| Q1-A2-02 | host refusal of heavy work only declared | MEDIUM | FIX-BEFORE-V1 | fixed |
| Q1-A1-02 | predictable scratch directories, then executed | MEDIUM | FIX-BEFORE-V1 | fixed |
| Q1-M-07 | check-cache facts not integrity-checked | MEDIUM | FIX-BEFORE-V1 | fixed |
| Q1-M-11 | pool select deadline wraps and spins | MEDIUM | FIX-BEFORE-V1 | fixed |
| Q1-D-05 | help and README deny floating point | MEDIUM | FIX-BEFORE-V1 | fixed |
| Q1-A1-04 | C-thread export faults instead of `N0408` | MEDIUM | FIX-BEFORE-V1 | fixed (the thread's own stack) |
| Q1-A1-05 | wasm32 handle width in C structs | MEDIUM | FIX-BEFORE-V1 | fixed by refusal (`N0381` on 32-bit targets) |
| Q1-B-05 | `exit_with` outside 0..255 differs by engine | MEDIUM | FIX-BEFORE-V1 | fixed (`N0416` everywhere; `exit_status.rs`) |
| Q1-D-04 | VERIFIED sub-claims resting on ignored tests | MEDIUM | FIX-BEFORE-V1 | carried (experimental surfaces) |
| Q1-D-06 | usage errors exit 2 | MEDIUM | FIX-BEFORE-V1 | fixed in Q1-C1: usage is 5 |
| Q1-D-07 | user errors exit 4 | MEDIUM | FIX-BEFORE-V1 | fixed in Q1-C1: 1 or 3 by kind |
| Q1-D-08 | `nazm doc` drops the loader's diagnostic | MEDIUM | FIX-BEFORE-V1 | fixed (message, and status by kind in Q1-C1) |
| Q1-D-10 | published schemas closed against additive fields | MEDIUM | FIX-BEFORE-V1 | corrected (`stability.md`, *Closed schema files*) |
| Q1-D-18 | stability omits `Bytes` and `Text` | MEDIUM | FIX-BEFORE-V1 | corrected in v1 (not done at Gate 2's close): listed as stable and in the memory contract |
| Q1-D-19 | Gate 2 untested in the formatter, provenance, validators | MEDIUM | FIX-BEFORE-V1 | fixed in v1 (not done at Gate 2's close): `format.rs` over the compat corpus and domain workloads, `provenance.rs` through `Bytes`/`Text`, the MIR and Core IR validators' array, index and conversion rules |
| Q1-M-14 | failed task with a blocked sibling hangs silently | MEDIUM | ACCEPTED-LIMITATION | carried, documented |
| Q1-A2-03 | symlink loop hangs resolution | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A2-04 | publish merges into a stale version directory | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A2-05 | lockfile names a package twice | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A2-06 | "no scripts" guarded indirectly | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A2-07 | contained runner's isolation flags untested | LOW | FIX-BEFORE-V1 | fixed (`run_argv`, a pure function; `a_contained_run_is_isolated_by_its_arguments`; three mutants) |
| Q1-A2-08 | subnet overlap by octets | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A2-09 | integration-testing overclaims | LOW | FIX-BEFORE-V1 | corrected |
| Q1-A1-03 | build work directory follows a planted symlink | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A1-06 | linker argument injection through a shared name | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A1-07 | `-o` over a non-entry module | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A1-08 | package source walk follows directory symlinks | LOW | FIX-BEFORE-V1 | fixed (with Q1-A2-03) |
| Q1-A1-09 | type depth exponential | LOW | FIX-BEFORE-V1 | fixed |
| Q1-A1-10 | guard-page `mprotect` unchecked | LOW | FIX-BEFORE-V1 | fixed |
| Q1-M-01, -02 | `Bytes`/`Text` absent from the memory tables; one reclamation test | LOW | FIX-BEFORE-V1 | fixed in v1 (not done at Gate 2's close): the four kinds list them; `a_buffer_and_a_text_are_reclaimed_in_every_shape_on_every_path` |
| Q1-M-08 | warm diagnostic names a parameter by position | LOW | FIX-BEFORE-V1 | fixed (names from the signature; `incremental.rs` compares whole) |
| Q1-M-10 | Cranelift key relied on a hand-bumped revision | LOW | FIX-BEFORE-V1 | fixed |
| Q1-M-12 | native select deadline on the wall clock | LOW | FIX-BEFORE-V1 | fixed (monotonic deadlines; `select.rs` steps the wall clock back) |
| Q1-B-06 | generic symbol names grow exponentially | LOW | FIX-BEFORE-V1 | fixed (a digest past 512 bytes; `generics.rs`) |
| Q1-B-07 | LIR oracle lacks float formatting | LOW | FIX-BEFORE-V1 | fixed (the interpreter's formatter; `lir_oracle.rs`) |
| Q1-D-09 | unreadable root reported at a prelude span | LOW | FIX-BEFORE-V1 | fixed (reported at itself; `exit_contract.rs`) |
| Q1-D-11, -13, -14 | no public code registry; `N0101` a catch-all; reserved codes | LOW | FIX-BEFORE-V1 | fixed (`diagnostics.md` registry of all 153 codes held by `codes.rs`; `N0101` named the umbrella; `N0620` reserved) |
| Q1-D-12 | phantom codes `N0703`/`N0704` | LOW | FIX-BEFORE-V1 | corrected (`N0357`, `N0358`) |
| Q1-D-15, -16 | no warning severity for deprecation; strict input readers undocumented | LOW | FIX-BEFORE-V1 | corrected (`stability.md`: deprecation announced until a warning severity exists; inputs read strictly) |
| Q1-D-17 | no compat case for epoch 34 or `N0359` | LOW | FIX-BEFORE-V1 | fixed (`run/imported-impl/`, `refuse/vec-ownership-cycle.nz`; and `trap/exit-status-outside.nz` for v1's `N0416`) |
| Q1-D-20 | self-hosted compiler does not carry Gate 2, unsaid | LOW | FIX-BEFORE-V1 | corrected (`limitations.md`; areas 29 and 31 state the scope) |
| Q1-D-22 | `fmt` lays out a file that does not parse | LOW | ACCEPTED-LIMITATION | carried, documented |
| Q1-D-24 | stale text in VERIFIED matrix rows | LOW | FIX-BEFORE-V1 | corrected (each line of the audit's list re-found by its text and dated or corrected; the `codes.rs` check is Q1-D-11's) |
| Q1-D-25 | unscoped "sandbox", "safe", "memory safety" | LOW | FIX-BEFORE-V1 | corrected |
| Q1-F-01 | fuzz hangs counted, not kept | LOW | FIX-BEFORE-V1 | fixed (kept under `OUT/hangs/`; `fuzz` crate's `a_hang_is_kept_once_and_unchanged`) |
| Q1-FI-01 | interrupted build leaves its scratch directory | LOW | POST-V1 | carried |
| Q1-A2-10, -11, -14 | root in the container; export staging name; unlocked index | LOW/INFO | POST-V1 | carried |
| Q1-A1-11, -14, -15 | reserved symbols; textual unsafe gate; developer tooling | LOW/INFO | POST-V1 | carried |
| Q1-B-08, -09, -10 | literal underscores; quadratic wide records; huge diagnostics | LOW | POST-V1 | carried |
| Q1-M-15, -16, -18 | runtime SIGINT, `NAZM_TASK_STACK` wrap, cache doc drift | INFO | POST-V1 | carried |
| Q1-M-17 | select's wake-while-parked path not forced by tests | INFO | FIX-BEFORE-V1 | fixed (a handshake parks each select; `v1-a-parked-select-is-never-woken` caught) |
| Q1-A1-12, -13, -16; Q1-A2-12, -13, -15; Q1-M-03, -05, -06, -13 | stated hazards and boundaries | LOW/INFO | ACCEPTED-LIMITATION | carried, documented |
| Q1-D-21, Q1-FI-02 | a failed build leaves the previous output | INFO | ACCEPTED-LIMITATION | by design: rename on success |
| Q1-E-01 | supply-chain policy failing and unenforced | MEDIUM | FIX-BEFORE-V1 | fixed |
| Q1-E-02 | `SECURITY.md` status stale; private repository has no private reporting | LOW | RELEASE-BLOCKER for publication | corrected; publication blocker carried |
| Q1-T-01 | the sampler test needs CPU it can be starved of | LOW | POST-V1 | carried |
| Q1-G-01 | governance incident: `NAZM_CONTAINED=1` set on a host for one refusal probe (it refused; nothing ran) | — | — | recorded (`docs/runbook.md`, *A second incident, recorded*); refusals now tested through child processes (`xtask/tests/host_refusal.rs`) |

## Mutation

Each fix has a focused mutant (`xtask/mutations/mutations.toml`, `q1-*`, 21) and four older
mutants were repointed; the reaper's `ps` probe has none because the contained runner is root, for
whom the `kill -0` it replaced also answers "alive" (the host test holds it).

The contained campaign over the 21 `q1-*` entries, Gate 2's 38 never campaigned (Q1-D-03) and the
15 Gate 2 and Q1 repointed — 74, every killer verified pristine-passes, mutant-fails,
restored-passes — caught **73 at tier 1**; none survived. On the way it found three faults in its
own evidence, each fixed: `q1-an-unsigned-product-may-pass-i128` was an equivalent mutant (a wrapped
unsigned product in `i128` is always negative, so still an overflow) — it now restores the
panicking `x * y`; the far-deadline test was clock- and race-dependent, and the defect's only
symptom on Linux is a spinning worker with a right answer — the test now measures CPU; Gate 2's
view-reclamation test read freed memory that still held its byte — it now scribbles freed memory.
`g2-a-view-does-not-keep-its-buffer` stays uncaught: a contained probe found no difference in output
or memory counts with the retain removed, so either another path holds the buffer (an equivalent
mutant) or the counters cannot see it. It is Gate 2's, and Gate 2's contained gate owes its verdict.
