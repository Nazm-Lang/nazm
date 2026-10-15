## Context

Surveyed 2026-10-15 at `c73dab27`. `cargo xtask check` runs twenty-four gates (`xtask/src/checks/mod.rs`);
some read only files (the docs corpus, the guide from the grammar, the capability matrix, the
doc claims, module boundaries, dependency sources, the mutations-apply gate, performance claims,
agent governance), others start `cargo metadata` (layering, the reach gates, third-party notices).
`crates/nazm-cli/tests/` has 188 entries merged into five test binaries (`cli_a_b` … `cli_s_z`) and
`selfhost`; most black-box files share `common/` (`run_bounded`, `bounded_exe`) and `differential/`
(`agrees`, `codes`). The mutation catalogue has 1881 entries, each naming the tests that kill it.

The 0.10.0 language already has what a harness needs: `@std/process` (`process_spawn_with`: a
directory, an environment stated in full, a process group, a null standard input;
`process_wait_until`; `process_kill_group`), `@std/file` (`file_walk`, `dir_list`,
`dir_create_temp`, `file_symlink_metadata`), `@std/env`, `@std/json`, `@std/toml`, `@std/blake3`,
and `nazm test`, which runs every `@test` function of a file under the interpreter and as an
executable, each in its own process, holding the authority its parameters ask for.

## Goals / Non-Goals

**Goals:** every ported check gives the Rust gate's verdict, with the same report, on the same tree;
every ported test passes where the Rust test passes and fails where a catalogued mutation makes the
Rust test fail; one way to run each (`tests/run.sh`), with a one-line summary.

**Non-goals:** changing what any check or test asserts; the contained runner and Docker (Rust until
the coordinator says otherwise); porting a test that calls a Rust API before that API is reachable
through the CLI (`rewrite-in-nazm` design, decision 5).

## Decisions

1. **A Nazm test is an `@test` function, run by `nazm test`.** `crates/nazm-cli/tests/NAME.rs` becomes
   `toolchain/harness/cli/NAME.nz`, each `#[test] fn x` an `@test fn x(…) -> Result[Int, Str]` of the
   same name, so `NAME::x` names the same test in both and a catalogue killer
   (`test = "cli_e_l", name = "line_breaks::x"`) maps to `nazm test --filter line_breaks::x`. An
   assertion is a `Result`: `Err` carries what the Rust `assert!`'s message says. `nazm test` runs
   each test interpreted **and** native, so the harness itself is held to both implementations;
   the cost is in the children it starts, not in the harness.
2. **Authority is a parameter, bundled once.** A test takes `IoCap`, `ProcessCap`, `SpawnCap`,
   `VouchCap` and `TimeCap` and makes one `Caps` record (`lib/run.nz`) that every library function
   takes. `VouchCap` is there because of provenance: a directory listing is a file's contents to
   the checker, and every `Strs` holds whatever any `Strs` holds, so once a test lists a directory
   every path it builds from a `Strs` is refused as a path to write or a program to start
   (`N0372`). The library declassifies with `str_vouch` at exactly its sinks — the program and
   directory `run` starts, the paths `scratch_write` and `scratch_remove` touch — and nowhere else.
   *Finding for the coordinator:* this is the language's documented rule (containers are precise
   by type, `docs/spec.md`, *Provenance*), but it makes every tree-walking tool declassify.
3. **The binary under test is `$NAZM`; the tree is `$NAZM_HARNESS_ROOT`.** The Rust tests have them
   at compile time (`CARGO_BIN_EXE_nazm`, `CARGO_MANIFEST_DIR`); a Nazm harness is given them at
   run time, so one built harness runs against the 0.10.0 release, a tree-built binary, or a
   scratch copy of the tree. `rewrite-in-nazm` names `NAZM_BIN`; the coordinator chooses one
   spelling at integration.
4. **The environment is stated, not inherited.** `.cargo/config.toml` gives every process Cargo
   starts `NAZM_RESULT_REPORT=1`, and the Rust tests rely on it (a program's last line is `main`'s
   result). `suite_env` is this process's environment with that variable set; a test that runs as a
   reader runs (`release_examples`) removes it, as the Rust test's `env_remove` does.
5. **Every child has a deadline and drains both streams.** `run` is `common::run_bounded`: standard
   input empty, both streams read in tasks while the parent waits, 60 seconds (`DEADLINE_MS`). Past
   it the child's **process group** is ended — the Rust helper ends only the child, which leaves a
   grandchild holding the pipes; the group is the stronger guarantee, not a weaker one.
   `bounded_exe` is the same `sh` script under the same 512 MiB `ulimit -v`.
6. **Checks are one native program with the Rust report.** `check/main.nz` is built with
   `nazm build` into `nazm-check`, takes the root as an argument (the Rust gate bakes it in at
   compile time), and prints `ok    NAME` or `FAIL  NAME` and each problem indented eight spaces,
   exiting 1 on a failure — byte for byte what `xtask/src/checks/mod.rs`'s `report` prints, so the
   two reports are compared with `diff`. Each gate is a module of its own (`check/<gate>.nz`).
7. **What counts as agreement.**
   - *A check:* the same report lines, on the tree and on scratch copies of it with a defect seeded
     for every rule the gate enforces (each seeded copy is run under both; the Rust gate is built
     from that copy, since its root is compiled in).
   - *A test:* the same per-test verdict as the Rust test on the tree, and on the tree with each
     catalogued mutation whose killer is in the ported file applied (in a scratch copy of the
     sources, `nazm` rebuilt, restored after), run against both the 0.10.0 release and the
     tree-built binary. A mutation the Rust killer catches and the port does not is a defect of the
     port; one the port catches and the Rust test does not is reported, never hidden.
8. **Porting order.** The library; the three named files (`release_examples`, `line_breaks`, and
   `short_variants` — the brief's `shorter_forms.rs` does not exist; `short_variants.rs` is the R1
   shorter-forms file); then the remaining black-box files, those using only `common` and
   `differential` first; the file-reading gates (docs corpus, guide, notices, module boundaries,
   then doc claims, capability matrix, dependency sources, mutations apply, performance claims,
   governance); the mutation core; then the gates that start `cargo metadata`, `bench` and `fuzz`.
9. **The mutation core is pure file work.** Apply: the entry's `find` must occur in its file
   exactly once (else `NOT INJECTED`, as the Rust runner says), the original kept beside a journal
   naming the mutation and the file's BLAKE3; restore: the original written back and its digest
   checked. Verdict: `CAUGHT` when a killer fails with the mutation applied and passes restored,
   `SURVIVED` when every killer passes, `NOT INJECTED` when the pattern does not apply. What runs a
   killer is given to it (a command), so the core never decides how a workspace is built — that,
   and containment, stay with the Rust runner until the coordinator moves them.

10. **What a Rust test only some hosts run, the port runs on the same hosts.** A `#[ignore]`d test
    that starts Docker (a QEMU boot, `node:wasi`, the EVM, a measured memory bound, gdb) is not
    ported: `tests/run.sh` reads libtest's `ok`/`FAILED` lines, so an ignored test has no verdict to
    agree with, and the container stays Rust (decision 9). A test `cfg_attr`-ignored off macOS
    (`ssh-keygen`, Apple clang's CPU table, `--backend nazm`) asks `uname` and returns `Ok` off
    macOS, and on macOS skips as `common::skip_without` does (`NAZM_REQUIRE_HOST_TOOLS=1` fails).
11. **Nazm's own code generator is built once per run.** Where a Rust test asks for
    `--backend nazm` it sets `NAZM_NZCG` to `differential::nzcg()`, built from
    `toolchain/codegen/main.nz` once per test process. `tests/run.sh` builds it once (on macOS,
    where it builds before 1.0) and hands it down as `NAZM_HARNESS_NZCG`; without that variable
    `lib/differential.nz`'s `nzcg` builds it into the test's own scratch directory. The harness
    variable is not `NAZM_NZCG` itself, so a ported test that asserts what happens without
    `NAZM_NZCG` still sees it unset.
12. **The suite's environment follows `.cargo/config.toml`.** Since R1.5 (D12) it also sets
    `NAZM_USER_CACHE_DIR=` (the user cache off); `suite_env` sets it and `tests/run.sh` exports it.

13. **What several Rust files share, the harness shares once.** `platform_files.rs`' `runs_in` and
    `alike` (which `process_control.rs` imports) are `lib/platform.nz`; the "every implementation"
    that a dozen files each write for themselves — the oracle or not, which runs ask for the memory
    report, where an executable runs — is one function given a `Tiers` record (`lib/tiers.nz`);
    `traits.rs`' and `usability.rs`' `agrees_on` is `lib/agree.nz`. Where a Rust helper prepares a
    directory by calling a Rust function, the port gives the files to write and a shell script to
    run there (`chmod`, `ln -s`, `ln`): the same tree, said in the shell's words.
14. **A value a Rust test computes each run may be worked out once.** `scheduler.rs` folds
    `(x + i) % 97` over three million steps to know what a task prints; the port writes the result
    (`1039`) with the fold beside it, because the harness's interpreted leg would spend its
    iteration budget on the computation, not on the test (risk *`nazm test` runs every test twice*).
15. **A reference implementation the Rust test links is a library's in the port.** `hash_vectors.rs`
    checks its table against the `blake3` crate and `compat_v1.rs` pins the corpus with it; the port
    uses `@std/blake3` of the release that builds the harness — a second implementation, not the
    tree's, so a table entry wrong in a way the tree's and the crate's agree on is still caught.

## Risks / Trade-offs

- [`nazm test` runs every test twice] → the interpreter leg is a second reference for the harness
  itself; the time is in the children. A test whose own work trips the interpreter's iteration
  limit is reported, and runs native-only only with the coordinator's agreement.
- [The 0.10.0 binary and the tree differ] → comparisons run against the tree-built binary (what the
  Rust tests use) and are repeated against the release; a difference between the two is the
  tree's, not the port's.
- [Strings built in loops] → output gathered in a `Strs` and joined once; no `str_concat` in a loop.

## Findings for the coordinator

Things the comparisons turned up that belong outside `toolchain/harness/` — reported, not changed:

- **A stale catalogued killer.** `r1-ints-is-its-own-type-again` (`crates/nazm-core/src/check/written.rs`)
  names `lists::one_list_type_runs_alike_everywhere` as its killer, and on this tree that Rust test
  passes with the mutation applied (`tests/mutants.sh`; the Nazm port passes too, so the two agree).
  The catalogue's killer should be re-verified (`cargo xtask contained mutate --verify-killer`).
- **The governance gate's order is the file system's.** `xtask/src/governance.rs` lists
  `openspec/changes` with `read_dir`, unsorted — an order no file system promises to be by name — so
  two problems there would be reported in an order a reader cannot predict. The port sorts.
- **Provenance and tree-walking tools** (decision 2): every path built from a `Strs` that ever held a
  file's contents is refused as a write's path, so a harness declassifies at its sinks.
- **`label` is a reserved word** (`N0101`) that a natural parameter or binding name runs into.
- **A tuple pattern may rebind in one scope what a name may not.** `let (a, b) = (1, 2); let (a, b) =
  (3, 4);` checks and runs (both 0.10.0 and the tree), where `let x = 1; let x = 2;` is `N0202`
  ("Nazm allows shadowing in a nested scope but not in the same one"). One of the two is a defect.
- **`numbers.rs` and `resolution.rs` are black-box.** Listed among the files that call a Rust API,
  they name `nazm_sema` and `nazm_core` only in their comments; both are ported.

## Open Questions

- **The files that call a Rust API** (tasks 4.5). Most do so in one to three of their tests; the
  rest of each file is black-box. Either a file is ported with those tests left in Rust (`run.sh`
  learning a list of tests that stay Rust, so they are not reported missing), or each API becomes
  reachable through the CLI first (`rewrite-in-nazm` decision 5). What each needs:
  - a table the CLI does not print: `boards`, `manifests` (`board::BOARDS`), `cross`
    (`backend::Target`), `builtins`, `build` (two of 140: `Intrinsic::ALL` with each one's shape),
    `domain_profiles`, `profiles_v2` (`profile::Profile::ALL` and its rules), `documentation`
    (`nazm_docs::CORPUS`), `docs` (`SEMANTIC_EPOCH`, the interface `SCHEMA`,
    `RUNTIME_ABI_REVISION`, `TARGET_NOTE`, the support table's non-goals), `native_cache`
    (`runtime::Word`), `ffi` (`runtime::IMPORTS`, `RESERVED_C_SYMBOLS`);
  - LLVM text the CLI does not write alone: `freestanding`, `realtime` (`board::emit_entry`,
    `emit_runtime`), `runtime` (`emit_runtime` for a given `Needs`, the platform and switch
    emitters);
  - a library entry point: `repository` (`nazm_repo::map`, which `nazm repo --map --json` is
    compared with), `summary` (`nazm_service::summary::check`), `resolved_units` (a
    `LanguageService` on a thread of `PARSE_STACK_BYTES`), `incremental` (the cache store's
    types, which some five of its 46 tests read through `check_entries`);
  - the compiler's own stages, which the rewrite replaces (R2, R3): `core_ir`, `cranelift`, `mir`,
    `durable_identity`, `prelude`, `reexports`, `fuzz_replay`.
- **The TCP files.** `distributed` binds a port to find a free one and starts nodes side by side;
  `secure_service` starts a Nazm server and talks to it with a Rust `TcpStream`. Both port with
  `@std/process`' `process_spawn_with` (a child left running, ended by its group) and `@std/net`'s
  client: the harness gains a `NetCap` and a run that does not wait.

- `NAZM` or `NAZM_BIN` (decision 3).
- Whether a ported test file replaces its Rust file at integration or runs beside it until R8.
- Whether the interpreter leg of `nazm test` should be skipped for the harness (decision 1).
