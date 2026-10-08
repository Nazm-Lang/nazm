# Maintainability — the M1 architecture-health audit

M1 (2026-10-08) asked whether the codebase can absorb the v1 breadth — Gates 2 and 3 — without
semantic duplication, platform sprawl, test fragility, ownership ambiguity or architecture
erosion. This is the answer, with its evidence. It is an audit of the tree at `f0023cd` and the
M1 commits after it; counts are signals, never proofs (`docs/master-architecture.md`, law 4).

**Method.** Five read-only inventories over `crates/` and `xtask/` (platform and OS; unsafe, FFI,
global state, environment and features; test architecture; canonical inventories; complexity),
each returning `file:line` evidence, kept under `target/gates/m1/audit-*.md`; plus the measured
footprint of the Gate 2 slices that had already landed — real changes, not hypotheticals — and the
one defect the audit found.

**Ordering note.** The v4 programme puts M1 before Gate 2. Three Gate 2 slices (numbers; arrays and
constants; bytes and text) landed before M1 under the earlier programme and are kept; their
footprints are this document's best evidence for §9 and §10.

---

## 1. Health

| Area | Status | Why, in one line |
| --- | --- | --- |
| Syntax / parser | WATCH | one coherent responsibility (4,549 lines); stack-frame growth on the recursive path nearly broke the 255-level bound in Gate 2E (§14) |
| Checker | WATCH | `check.rs` holds ~15 concerns in 8,776 lines; Gate 2 slices landed beside it (`constants.rs`, `nazm_sema::numeric`) without duplication, but it is the file Gate 2 will most want to grow (§14) |
| Memory / ownership | HEALTHY | area 4 VERIFIED; arrays are plain data, `Bytes` a counted handle on the existing rules |
| Effects / capabilities / provenance | WATCH | an M1 defect (imported impl methods' effects) found and fixed narrowly, `f0023cd`; the classifications are exhaustive per built-in |
| IR / lowering | HEALTHY | one semantic owner per rule (`nazm_sema::numeric` for every number); exhaustive CIR/MIR matches force each new form to be decided |
| Backends (interpreter, LLVM, Cranelift) | WATCH | lowering is shared through LIR; the interpreter's built-in dispatch ends in a wildcard — now guarded by a coverage test (§6); a Linux-only link defect from Gate 2C was found by M1's contained run and fixed (§19) |
| Runtime | WATCH | one file of per-feature LLVM-text constants with POSIX numbers inline; the parity-checked texts are protected (§8) |
| Platform layer | WATCH | the target model and the runtime's OS list are isolated; inline POSIX constants, the CLI's naming, `xtask`'s process control and the checker's reserved C symbols are not — designed below, not a Gate 2 blocker (§7) |
| Package / build | WATCH | `build.rs::run` is ~746 lines over ten phases (§14) |
| LSP / MCP | HEALTHY | coherent; `lsp.rs`'s path ↔ URI conversion is POSIX-only (§7) |
| Schemas | WATCH | the 29 published schemas are validated and classified; 25 emitted ids have no schema file or class (§11) |
| Tests | WATCH | improved in M1 (§12); remaining duplication of `everywhere`/`agrees`, and the heavy differential suites all sit on the host-safe path |
| Mutation / fuzz | HEALTHY | catalogue owned per file and killer; `mutations apply` gate; Gate 2 entries attached cleanly (§13) |
| Release harness | HEALTHY | contained runner, ledgers, lifecycle suite; `xtask/src/main.rs` mixes 21 gates with two runners (§14) |
| Documentation / change governance | WATCH | OpenSpec adopted as governance only (`docs/change-governance.md`); several prose inventories unguarded (§5) |
| Agentic governance | WATCH | complete and gated (`docs/agent-governance.md`, the `agent governance` gate); the ten evals are scenarios with no recorded run yet |

No area is REFACTOR-BEFORE-GATE-2 or BLOCKER. Every WATCH has a rule below that keeps Gate 2 from
making it worse.

## 2. Dependency direction

Workspace crates, by `[dependencies]`:

```
span ─┬─ diag ─┬─ syntax ─┐
      │        │          ├─ core ──┬─ service ─┬─ repo ─┬─ agent-bench
      ├─ sema ─┼─ cir ────┘         │           │        └─ mcp
      │        ├─ iface ─ cache     ├─ formal, tokens (dev)
      │        └─ mir ─ lir ─┬─ codegen-clif
      │                runtime┘
      └─ package
cli ← span, diag, core, iface, cache, lir, mir, codegen-clif, service, package, docs, repo
docs, bench, runtime: leaves.   xtask → docs only.
```

The intended direction is the one the `layering` gate already enforces (`xtask/src/rules.rs`):
semantics (`syntax`, `sema`, `core`, `cir`) know nothing of tooling, backends or platforms;
lowering (`mir`, `lir`) knows the runtime's inventory; the CLI composes. Findings:

- **No semantic crate depends on tooling, UI or a backend** — enforced (`layering`, `sema purity`,
  `core ir boundary`, `codegen reach`, `emission reach`).
- **One platform fact lives in the checker:** `RESERVED_C_SYMBOLS` (`crates/nazm-core/src/check/foreign.rs`)
  lists the POSIX runtime's C symbols, because a foreign declaration may not redeclare them. It is
  held to the runtime's inventory by `tests/ffi.rs` (which caught the Gate 2C omission in M1). A
  Windows runtime would add symbols; the list becomes the union over targets (§7).
- **`xtask` depends on `nazm-docs` only**, so vocabulary drift guards live in cargo tests rather than
  `xtask check` (§5). That keeps the gates fast and the harness independent of the compiler.
- Fan-in that matters: `nazm-sema` (6 crates) owns `Type` and `Intrinsic`, so a vocabulary change
  rebuilds `cir`, `mir`, `lir`, `iface`, `cache`, `core`; `nazm-core` (7) bundles check, eval, lower,
  effects and provenance. A crate split is not needed; module boundaries inside `core` are (§14).

## 3. Responsibility ownership

| Responsibility | Owner |
| --- | --- |
| What a construct means | `docs/spec.md`, enforced by `nazm-core` (`check.rs`, `effects.rs`, `provenance.rs`, `constants.rs`) |
| Number semantics (every implementation) | `nazm_sema::numeric` — the interpreter calls it; backends are tested against it. *Corrected in Q1:* this row said the LIR oracle calls it too; it does not — `nazm-lir/src/op/interp.rs` re-implements the rules on raw `u64`, which is why Q1-B-03 had to be fixed in both. Whether the oracle should share one set of rules or keep an independent one is an M2 decision |
| Built-in inventory, signatures, effects, authority, provenance | `nazm_sema::Intrinsic` and its per-concern tables (`intrinsic.rs`, `effect.rs`, `capability.rs`, `provenance.rs`) — exhaustive |
| Type vocabulary | `nazm_sema::Type::ALL` / `from_name` |
| Core IR, its verification and digest | `nazm-cir` |
| Ownership and drops, explicitly | `nazm-mir` |
| Machine-level lowering, shared by both backends | `nazm-lir` (`op/*`); LLVM text (`op/llvm.rs`), Cranelift (`nazm-codegen-clif`) |
| Runtime contract and inventory | `nazm-runtime` (`INVENTORY`, `IMPORTS`, `RUNTIME_ABI_REVISION`) |
| Targets and their support | `nazm_lir::backend` (`Target`, `SUPPORT`, `NON_GOALS`) |
| Toolchain driver (clang, linking) | `nazm-cli/src/backend.rs` |
| Containment, mutation, gates | `xtask` |
| External-service sandboxing | `nazm-bench/src/sandbox` (`docs/integration-testing.md`) |

## 4. The rules Gate 2 works under

Collected from the sections that justify them; each keeps a WATCH area from getting worse.

1. A new vocabulary item goes into its canonical source; any copy it needs is derived or guarded in
   the same commit (§5).
2. A new Gate 2 concern in the checker is a sibling module of `nazm-core`, not more of `check.rs`;
   a new build phase is a function, a new `xtask` gate a module, a new built-in family a sibling
   in `eval.rs` and in `nazm-lir/src/op/` (§14).
3. A new runtime service is a `Part` with its own text constant, outside the parity-checked texts
   unless the self-hosted compiler needs it, calling the OS only through `os.rs` (§8).
4. A new machine output ships with its schema file and its stability class (§11).
5. New test suites use `tests/common`'s harness; a test needing an external service is
   `integration-external` (§12, `docs/integration-testing.md`).
6. Platform facts are named by `Target`, never by `std::env::consts::OS` (§7).

## 5. Canonical inventories

Guarded means a test or gate fails on drift.

| Vocabulary | Canonical | Guarded copies | Unguarded copies |
| --- | --- | --- | --- |
| Built-in types | `Type::ALL` (`nazm-sema/src/types.rs`) | `nazm capabilities`, completion | `compiler/analyse.nz` `type_of_name` (a subset, checked only by behaviour); prose in spec / stability |
| Built-ins | `Intrinsic::ALL` | `analyse.nz` `add_builtin` (selfhost test, with `NOT_CARRIED`); the LIR's `Builtin` is a type alias; **interpreter coverage (M1)** | `emit.nz` lowering by name; `analyse.nz` `fx_of_builtin`, `auth_kind_of_builtin`; prose |
| Capabilities, effects | `CapabilityKind::ALL`, `Effect::ALL` | `Type::ALL` | spec's capability table (stale on `mmio` widths until M1 fixed it); stability list; `grammar.ebnf`; `analyse.nz` bits |
| Diagnostic codes | `nazm_diag::codes` | `CHECKER_CODES` (`cache soundness`); every code asserted or excused (`tests/codes.rs`) | no catalogue in docs; 14 codes appear in neither `diagnostics.md` nor `spec.md` |
| Targets | `nazm_lir::backend::SUPPORT` | `inspect::TARGETS`, `support.md`, the matrix's table, README's note | `Target::parse`'s error sentence; board triples |
| Restriction profiles | `Profile::ALL` (`nazm-service`) | **`--profile` help (M1 guard; it named 4 of 8)** | `support.md`, `master-architecture.md` prose |
| Schemas | `schema/*.json` | validated against output; documented; classified | 25 emitted ids with no file and no class (§11) |
| CLI commands | clap `Command` | README table; stability classes | mentions in guide, getting-started, runbook, mcp |
| Version identities | `SEMANTIC_EPOCH`, `RUNTIME_ABI_REVISION`, `wire::SCHEMA`, `CARGO_PKG_VERSION` | `--version`; the epoch pin; **`stability.md`'s table (M1 guard)** | `general-purpose.md`'s baseline header (deliberately historical), README |
| Runtime functions | `INVENTORY` / `IMPORTS` | runtime's own tests; `runtime parity` for eight texts; reserved C symbols (`tests/ffi.rs`) | `emit.nz`'s declares; ~30 `nz.*` names in `architecture.md` |
| Agent instructions | `AGENTS.md` + `docs/agent-governance.md` | **`agent governance` gate (M1)** | — |

Rule for Gate 2: **a new vocabulary item is added to its canonical source, and any copy it needs is
either derived or guarded in the same commit.** No new hand-written list is added unguarded.

## 6. Semantic duplication across execution engines

The flow is semantic decision → Core IR → MIR → LIR → backend. Findings:

- **Numbers have one semantics** (`nazm_sema::numeric`), called by the interpreter and the LIR
  oracle; the backends are differential-tested against it (`tests/numbers.rs`). Acceptable.
- **The interpreter evaluates Core IR directly** — a second implementation by design (it is the
  reference), not duplication.
- **The interpreter's built-in dispatch ends in a wildcard** (`eval.rs`, `intrinsic`), so a missing
  built-in fails at run time, not at compile time. Rewriting it as an exhaustive match over
  `(Intrinsic, args)` is not practical; **M1 added a guard instead**:
  `every_front_end_builtin_is_evaluated_by_the_interpreter` (`tests/build.rs`) runs every built-in
  and fails on the wildcard. Classified: historical debt, now guarded.
- **Selfhost** (`compiler/*.nz`) re-implements the checker and emitter for the language subset it
  compiles; its built-in table is guarded, its type and effect tables are not (§5). Acceptable for
  a bootstrap compiler with a declared `NOT_CARRIED` boundary.
- No Gate-2 risk was found where a semantic rule is decided in a backend.

## 7. Platform

**Where platform behaviour lives today:**

| Responsibility | Where | Isolated |
| --- | --- | --- |
| Target triples, host detection, support matrix | `nazm-lir/src/backend.rs` | yes |
| The OS API the runtime calls | `nazm-runtime/src/os.rs` (one POSIX list) | yes |
| POSIX constants and sizes in runtime text | `nazm-runtime/src/core.rs` (`getrlimit(3)`, `clock_gettime(0/4)`, pthread sizes, 128-byte mutex buffers, `mprotect`) | **no** |
| Context switch, errno location | `nazm-runtime/src/switch.rs` (SysV and AArch64; Mach-O and ELF) | yes |
| Target → runtime pieces | `nazm-cli/src/build.rs`, `runtime_artifact.rs` | partly |
| Compile and link driver | `nazm-cli/src/backend.rs` (clang only, hermetic environment) | yes |
| Executable naming, archives, `.dSYM`, link libraries | `nazm-cli/src/build.rs` | partly |
| Object format and DWARF for Cranelift | `nazm-codegen-clif/src/dwarf.rs` (Mach-O, ELF) | yes |
| Process control in the harness | `xtask/src/procs.rs`, `contained.rs`, `bench.rs` — Unix, unguarded | **no** |
| Paths and URIs | `lsp.rs` (POSIX-only), `nazm-repo` (string joins); `nazm-sema/src/key.rs` normalises both separators | **no** |
| Reserved C symbols | `nazm-core/src/check/foreign.rs` | no (§2) |

The semantic crates (`syntax`, `sema`, `cir`, `mir`) contain no platform code; `core` holds only the
reserved-symbol list and a diagnostic naming board triples. There is no `unsafe`, no Rust FFI and no
JIT anywhere (the workspace forbids `unsafe_code`).

**The boundary Windows and later platforms are to be implemented behind** (designed in M1, not
implemented — Windows is Gate 3):

1. **`nazm_lir::backend::Target` stays the only place a platform is named.** `Os` gains `Windows`;
   every other crate asks the target, never `std::env::consts::OS`.
2. **A per-target platform-facts table in `nazm-runtime`** beside `os.rs` — the constants now inline
   in `core.rs` (`RLIMIT_STACK`, clock ids, synchronisation-object sizes, page-size source), the
   declaration list, the errno source — selected by target when the runtime text is emitted. The
   runtime text then names facts, not numbers.
3. **`switch.rs` gains Win64 and COFF** as one more `SwitchTarget`.
4. **The CLI's artefact naming** (`.exe`, archive tool, import libraries, debug-info packaging) moves
   behind one `Artefacts` function of the target in `nazm-cli/src/build.rs`.
5. **The reserved C symbols** become the union of every target's runtime symbols, still tested
   against the runtime inventory.
6. **Harness process control** (`xtask/src/procs.rs`) gets a `cfg(unix)` boundary and an explicit
   refusal elsewhere; the contained runner is Linux-container based in any case.
7. **Paths:** `lsp.rs` and `nazm-repo` use `std::path` and a URL library rather than string joins.

This keeps platform behaviour out of the semantic layers.

## 8. Runtime extension

The runtime is LLVM IR text, per feature part (`Part`), emitted when `Needs` says the program uses
it (`emit_runtime`), with every function in `INVENTORY` and every import in `IMPORTS`. Gate 2C
(numbers) and 2G (bytes and text) each added a part or functions **without edits elsewhere in the
runtime**: a new constant (`NUMBER_RUNTIME`, `TEXT_RUNTIME`), inventory rows, LIR oracle models,
the reserved symbols. Files, networking, process and time can be added the same way:

- **one `Part` and one text constant per service**, outside the parity-checked texts unless the
  self-hosted compiler needs it (`crates/nazm-runtime/AGENTS.md`);
- **OS calls only through `os.rs`'s declarations** (and, after §7, the platform-facts table);
- **blocking I/O through the scheduler's existing park and wake** (`nz.pool_park`, `nz.cwake`) — a
  reactor is a new part, not an edit to the pool;
- the LIR oracle models each new function, so `lir_oracle` keeps checking lowering against it.

No service or provider abstraction is needed before Gate 2; the part system is that boundary.

## 9. Checker extensibility — what one change touches today

Measured from Gate 2's commits, not estimated:

| Change | Files | Where |
| --- | --- | --- |
| A family of scalar types, operators and conversions (2C, `d7feb20`) | 83 | lir 13, tests 10, cli 10, syntax 9, sema 9, core 6, mir 5, cir 5, service 4, runtime 3, docs 3, clif 2, diag, cache, schema, xtask |
| A type form, index expressions, constants (2E, `0693c1d`) | 67 | tests 10, lir 9, core 8, syntax 7, sema 6, mir 6, cli 6, cir 5, service 4, xtask 2, iface, diag, cache, docs |
| Two built-in types and 25 built-ins (2G, `8f691e2`) | 38 | tests 8, sema 6, lir 5, core 4, cli 4, runtime 3, mir 2, syntax, diag, cir, cache, xtask, docs |
| A checker rule fix (M1-1, `f0023cd`) | 6 | core, cache, cli tests, xtask, docs |

- **One built-in** touches, at minimum: `intrinsic.rs` (name, params, ret, and `requires`),
  `effect.rs`, `capability.rs`, `provenance.rs` (twice), `nazm-lir/src/op/prims.rs` or a sibling,
  the interpreter, the mutation catalogue and a test — eight exhaustive sites. That is per-concern
  by design (each table answers one question and a test holds them consistent); a declarative
  per-built-in row was considered and **not** adopted: it would merge four authorities into one
  table and lose the exhaustiveness that forced every Gate 2 built-in to be classified for effects,
  authority and provenance.
- **One scalar type** touches `Type`, `NumKind`, the CIR/MIR/LIR type maps (exhaustive), the classes
  in LIR, both backends' instruction selection, the interpreter value, the interface spelling.
- **One operator** has one semantic owner (`numeric::arith`/`bit`), one CIR form, and one lowering
  per backend.
- **One capability or effect** touches `CapabilityKind`/`Effect` (exhaustive), the tables above, and
  the prose inventories of §5.
- **One target or profile** touches `Target`/`SUPPORT` or `Profile::ALL`, with the guarded copies.

The missing abstraction is not a registry; it is **module boundaries inside `check.rs`** (§14).

## 10. IR and backend extensibility

The three traces the M1 brief asks for were real changes, not hypotheticals:

- **`Float64`** (2C): source literal → checker typing by expected type → `Const::Num`, `Convert`,
  `Fits` in CIR → MIR → LIR classes F32/F64 and `Conv` → `fptosi.sat` in LLVM, `fcvt_*_sat` in
  Cranelift; shortest-digit printing in a runtime part. No semantic shortcut: every
  implementation calls or is tested against `numeric`.
- **Fixed array** (2E): `Type::Array` → CIR `Array`/`ArrayRepeat`/`Index` → MIR places →
  `op/arrays.rs` with one bounds check (`N0405`) shared by both backends.
- **Scheduler-aware I/O** (not yet built): a runtime part for the reactor (kqueue/epoll), new
  built-ins classified through the four tables, lowering in an `op/io.rs` sibling, parking through
  the existing pool. The risk Gate 2 would otherwise introduce — blocking calls inside the pool's
  worker — is a runtime-part design rule, stated in §8.

## 11. Diagnostics and schemas

- **Codes** are constants in `nazm_diag::codes`; a code is never reused; every code is asserted by a
  test or excused (`tests/codes.rs`); the checker's set is part of its identity
  (`CHECKER_CODES`, `cache soundness`). Gate 2 added `N0413`, `N0618`, `N0621`–`N0624` through
  this path without drift.
- **Human text, JSON, spans and fixes** share one `Diagnostic` type; the JSON shape is
  `schema/nazm.diagnostic-1.json`, validated against real output.
- **Gap:** 25 emitted schema ids have no `schema/*.json` and no classification in
  `docs/stability.md` (list: `target/gates/m1/audit-inventories.md`). Rule for Gate 2: a new
  machine output ships with its schema file and its stability class in the same commit; the existing
  25 are a recorded debt, not a blocker.

## 12. Test architecture

About 1,500 tests in `nazm-cli`'s ~100 files, plus per-crate suites; categories and file lists are in
`target/gates/m1/audit-tests.md`. Findings and what M1 did:

- **Fixed in M1 — a hang disguised as a timeout.** Ten suites carried a byte-identical
  `run_bounded` that read a child's pipes only after it exited, so a child writing more than a pipe
  holds blocked and was reported as not terminating. One draining `run_bounded` now lives in
  `tests/common/mod.rs`. *Corrected in Q1:* "the ten copies are gone" was not so — two more,
  in `tests/build.rs` and `tests/concurrent.rs`, still read after exit; they now call
  `common::run_within`, the same draining run with a caller's limit.
- **Fixed in M1 — silent green.** The Docker-backed tests in `nazm-bench` and `resource_regression`
  passed by skipping; they are now `#[ignore = "integration-external: …"]`
  (`docs/integration-testing.md`).
- **Remaining duplication:** `everywhere` (13 copies), `agrees` (15), `refused` (13), `nazm(…)` (54,
  three signatures), deadlines from 20 s to 600 s. Rule for Gate 2: new suites use `common`'s
  harness; old ones migrate when touched.
- **Remaining environment sensitivity:** a few suites write `.nazm/` caches into the tree
  (`schemas.rs`, `workflow.rs`, `release_examples.rs`); most children inherit `NAZM_*` variables;
  some tests skip silently without `clang` or `lldb` (`platform_matrix.rs`, `debugging.rs`). WATCH.
- **Cost:** each differential assertion is one interpreter run and three native builds and runs; all
  of it is on the host-safe path. Gate 2 should batch assertions into fewer programs per suite.
- Two wall-clock assertions (`selfhost.rs`, `cost_and_flow.rs`) can flake under load. WATCH.

## 13. Mutation architecture

The catalogue (`xtask/mutations/mutations.toml`, 1,324 entries after M1) is owned per file and per
killer; `mutations apply` holds every pattern to exactly one match; campaigns are journaled, resumable
under one profile, and serial. Gate 2 attached 38 new entries to clear owners (numbers, arrays,
constants, bytes, text) and repointed the ones whose lines moved, with a note each. M1 added
`m1-an-imported-impl-method-has-no-contract` and `m1-a-linux-link-omits-libm`, each caught at tier
1 (§19). No change to the mutation law is needed.

## 14. Complexity hotspots

| Hotspot | One responsibility? | Gate 2 worsens it? | Split first? |
| --- | --- | --- | --- |
| `nazm-core/src/check.rs` (8,776 lines, ~15 concerns) | no | yes — attributes, FFI, serialization, test/bench would land here | **no, with a rule:** each new Gate 2 concern goes in a sibling module of `core` (as `constants.rs` did); existing concerns move out when next substantially edited. A wholesale split in M1 would be the cleanup spree M1 forbids |
| `nazm-cli/src/build.rs::run` (~746 lines, ten phases) | no | yes | **no, with a rule:** a new build phase is a function called from `run`, not inline |
| `xtask/src/main.rs` (21 gates, two runners) | no | yes | **no, with a rule:** a new gate is its own module (`governance.rs` is the pattern) |
| `nazm-runtime/src/core.rs` (25 text constants) | yes per constant | yes | no — new parts as their own constants; a file per part when one is next added |
| `nazm-core/src/eval/intrinsic.rs::intrinsic` (~513 lines, wildcard) | yes | yes | no — guarded (§6); new families in sibling functions (`bytes_text_intrinsic`, `numeric_intrinsic`) |
| `nazm-lir/src/op/prims.rs::primitive` (~562 lines) | yes | yes | no — new families in sibling modules (`op/numeric.rs`, `op/bytes.rs`, `op/arrays.rs`) |
| Parser recursive path | yes | yes | no — new constructs in `#[inline(never)]` helpers; `nazm` runs every command on `PARSE_STACK_BYTES` since Gate 2E |

## 15. Unsafe and FFI

None. `unsafe_code = "forbid"` for the whole workspace (root `Cargo.toml`), enforced by the
`unsafe policy` gate against an empty allowlist (`xtask/src/rules.rs`); no Rust `extern "C"`, no
`#[no_mangle]`, no dynamic loading, no JIT (Cranelift writes objects). Native code runs out of
process: clang, linkers, generated executables, `accel`'s OpenCL host. The gate matches the lint by
substring, so a commented copy of the line would pass — WATCH.

## 16. Configuration

- **No Cargo features** anywhere, and no `cfg(feature)`.
- **Environment variables the binaries read:** `NAZM_CACHE_DIR`, `NAZM_TEST_DRIVER` (a test seam in
  the shipped binary), `NAZM_MEMORY_REPORT`, `NAZM_SCHED_REPORT`, and in generated programs
  `NAZM_SCHEDULER`, `NAZM_WORKERS`, `NAZM_TASK_STACK`; clang receives only an allowlist
  (`nazm-cli/src/backend.rs`). The harness reads `NAZM_BENCH_DIR` (undocumented outside the code),
  `CARGO_TARGET_DIR`, `CARGO`, `NAZM_REQUIRE_DOCKER`, `NAZM_FUZZ_SEED`/`_CASES`, `NAZM_CLANG_IMAGE`.
- **Process-wide interpreter state:** the memory and scheduler counters, the `select` wake-up
  condition and the clock origin in `eval.rs` are global and never reset, so two runs in one process
  — `nazm live`, a future in-process host — share them. Correct today because every report-reading
  path runs one program per process. WATCH.
- **Ownership model recommended:** command flags are the interface; an environment variable is
  either a documented report switch, a documented test seam, or a harness input, and each is listed
  here and in the runbook. Targets, backends and profiles are named only by their canonical
  inventories (§5).

## 17. Documentation drift

Sampled against code-owned facts: versions (drifted in prose — `stability.md` now guarded),
builtins and capabilities (the spec's capability table was stale — fixed), profiles (the CLI help
was stale — fixed and guarded), targets (guarded), schemas (25 unclassified), selfhost scope
(declared by `NOT_CARRIED`). The structural cause is that most prose inventories restate a canonical
list; the rule in §5 stops new ones.

## 18. Gate 2 readiness

| # | Question | Answer | Evidence |
| --- | --- | --- | --- |
| 1 | A new scalar type without semantic duplication? | **YES** | 2C: one `numeric` semantics, every implementation held to it |
| 2 | A new operator with one semantic owner? | **YES** | `numeric::arith`/`bit`, one CIR form |
| 3 | A new runtime service without platform code leaking upward? | **YES** | the part system (§8); the one platform fact in the checker is guarded |
| 4 | Windows behind a clear platform boundary? | **YES, designed** | §7's boundary; implementation is Gate 3 |
| 5 | External-service tests without weakening containment? | **YES** | `docs/integration-testing.md`; the owned-resource engine |
| 6 | Authoritative vocabularies avoid drift? | **YES, with a rule** | §5's guards and its add-guarded-or-derived rule; the remaining unguarded prose is listed |
| 7 | Changes proposed and reviewed without duplicating the spec? | **YES** | `docs/change-governance.md`; the gate refuses language capabilities in `openspec/specs` |
| 8 | Reference apps testable independently and reproducibly? | **YES** | the shared harness; the compat corpus's pinned cases |
| 9 | New mutations attach to clear owners? | **YES** | 38 Gate 2 entries, each with a file and a killer |
| 10 | Known blockers that would compound debt? | **NO** | none; every WATCH has a rule (§14, §5, §11, §12) |
| 11 | A new agent finds the authority chain without conversational memory? | **YES** | `AGENTS.md` |
| 12 | Vendor files avoid policy drift? | **YES** | `CLAUDE.md` is a shim; the gate checks it defers |
| 13 | Trusted instructions distinguished from untrusted data? | **YES** | `docs/agent-governance.md` §2 |
| 14 | Tool and MCP output cannot escalate permissions? | **YES** | §3 |
| 15 | Work resumes safely after session loss? | **YES** | local state verified against live evidence; committed handoffs (§6) |
| 16 | Multiple agents avoid conflicting writes and heavy workloads? | **YES** | §7 |
| 17 | Human-only decisions separated from agent autonomy? | **YES** | §4, §11 |
| 18 | Agent behaviour regression-testable? | **YES, as scenarios** | `docs/agent-evals.md`; no automated harness is claimed |
| 19 | Vague tasks bounded before implementation? | **YES** | the task contract (§5) |
| 20 | Every substantial agent change leaves an auditable record? | **YES** | handoffs, provenance (§12), OpenSpec changes |

## 19. What M1 found and fixed, and its validation

Defects, each fixed narrowly with a regression test and, where it is behaviour, a mutant:

| Found by | Defect | Since | Fix |
| --- | --- | --- | --- |
| Gate 2 §7's design, reproduced at `3ba27ee` | a direct or UFCS call of an impl method in another module was checked as an undeclared function's — every effect assumed, a pure caller refused (`N0366`) | N78 | `f0023cd`: the trait method's contract; epoch 34 |
| the inventory audit | `--profile` help named 4 of 8 profiles | the four profiles added after N47 | `5b6a624`, guarded |
| the inventory audit | the spec's capability table and wasm row named only the 32-bit `mmio` built-ins | N86 | `5b6a624` |
| the test audit | ten suites could hang on a full pipe and report a timeout | the first copy | `5b6a624`, one draining harness |
| the test audit | Docker-backed tests passed by skipping | the suites' creation | `5b6a624`, `integration-external` |
| `xtask`'s release test | the compatibility corpus was outside the release source digest | Gate 1B, `4830170` | `5b6a624` |
| M1's contained run | a Linux link named no libm, so float built-ins failed to link | Gate 2C | `7a74cd6`, mutant |
| M1's contained run | Gate 2's 66 built-ins missing from the self-hosted checker's not-carried list | Gate 2C, 2G | `531769a` |
| M1's own gate | two mutants lost their lines to M1's edits (`mutations apply` failed at `5b6a624` and `7a74cd6`) | M1 | `9a50b59` |

The last three are why M1 requires the contained run: the host suites run on macOS and exclude
selfhost, so a Linux link and the self-hosted checker's table are invisible to them.

Validation, at `531769a`:

| | Outcome |
| --- | --- |
| `cargo fmt --check`, `cargo lint` (clippy) | passed |
| `cargo xtask check` — 21 gates, `agent governance` new | passed |
| `cargo xtask contained probe` | passed — memory, deadline, output, scratch, descendant; no survivors |
| `cargo xtask contained tests` — the whole suite, selfhost included | **passed: 2,538 passed, 0 failed, 58 ignored, 203 binaries**, 1,367.7 s, peak 1.6 GiB, no survivors (`target/gates/m1/contained-tests-3.log`); two earlier runs failed on the Linux-only defects above |
| targeted mutations | `m1-an-imported-impl-method-has-no-contract` caught (`341a6c89cbacc724`); `m1-a-linux-link-omits-libm` caught (`9c22557773af4909`); both tier 1 |
| OpenSpec pilot | `openspec validate --strict` valid; archived; `validate --all --strict` 1 passed (`target/gates/m1/openspec-*.txt`) |
| Full mutation catalogue | not run — M1's rule: not for audit and documentation work; Gate 2's 38 entries await Gate 2's contained gate |
| Lifecycle suite | not run — harness and container ownership code unchanged |
