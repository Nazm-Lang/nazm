## Context

Surveyed 2026-10-11 at `91ccda5`:

| | Size | What it needs from a language |
| --- | --- | --- |
| Product (`bin/nazm`, `nazm-mcp`) | ~133k lines, 19 crates | trees (syntax, types, JSON, TOML, LSP messages), hashing (BLAKE3), process spawning with a controlled environment (clang, ar, dsymutil, ssh-keygen, python3), recursive file walks, threads, JSON-RPC over stdio |
| Harness (`xtask`, bench, agent-bench, tokens, formal, fuzz) | ~25k lines | Docker, git, curl and the `claude` CLI as child processes; file locks; temporary directories |
| Tests | ~100k lines | 109 of 130 CLI test files drive the `nazm` binary; ~45 also call Rust APIs (mostly constants and parse/check entry points); ~50 crate-level files are Rust-API tests |
| Already in Nazm | `compiler/*.nz` ~16.8k lines; `library/` ~4.3k | the compiler is written around today's limits: parallel `Ints`/`Strs` arrays, no recursion, integer codes for node kinds (`bootstrap.md`) |

Third-party code linked today: serde/serde_json, toml, blake3, cstree, clap, lsp-server/lsp-types,
rmcp with tokio, cranelift with gimli, object and target-lexicon.

## Goals / Non-Goals

**Goals:** every program of the shipped toolchain and its harness is Nazm source; the shipped
binary links no third-party code (clang and the system C library are called or linked as the
platform's, not bundled); behaviour is the Rust toolchain's, held by the same tests; the toolchain
builds itself to a fixpoint; Nazm can still be built from source without a downloaded binary.

**Non-goals:** changing what any 1.0 program means; a new optimiser (clang's stays); new targets;
performance beyond "not a regression a user notices" — budgets are set in R2 and R3.

## Decisions

Made by the copyright holder, 2026-10-11:

1. **Scope: everything before publication** — product, harness and tests.
2. **The interpreter is rewritten in Nazm** (R4). It stays an independent semantic reference and
   keeps `nazm run` free of clang.
3. **Cranelift is not ported.** Nazm gets its own code generator written in Nazm (R9), **before
   publication**: LIR to machine code and objects, Mach-O and ELF on aarch64 and x86_64 first, its
   line tables, and a differential suite against LLVM. Until R9 the Cranelift cases of the 55 test
   files that use the backend are recorded as a coverage reduction, never silently dropped.
4. **The Rust implementation is frozen at cut-over** as the bootstrap seed, changed afterwards only
   to keep it building the seed. A distribution builds Nazm from it.

Design decisions of this change:

5. **The tests are the contract.** The black-box CLI tests run against an `NAZM_BIN` pointing at
   the Nazm-built toolchain, phase by phase; a phase closes when its share of them passes against
   both binaries. Tests that call Rust APIs are first moved to go through the CLI, without
   weakening what they assert.
6. **The Rust toolchain stays the reference until R8.** Language changes (R1) land in it first, with
   spec text first, as `CONTRIBUTING.md` requires; the Nazm implementation follows and is held to it.
7. **Bootstrap chain at cut-over:** frozen Rust `nazm` (seed) → builds the Nazm toolchain (stage 1)
   → stage 1 builds stage 2 → stage 2 builds stage 3; stage 2 and stage 3 byte-identical. The
   fixpoint proves reproduction, not correctness; the tests prove behaviour.
8. **Phases are OpenSpec changes of their own,** proposed, designed and approved before code, each
   closing with the gates the v1 programme used: host suite, contained tests, selfhost and
   bootstrap, mutants for what it adds.
9. **Containment stays as the runbook says until R6.** Running Nazm-built compiler stages is heavy
   work and runs contained; R1 adds the bounded heap that R6 needs to argue host execution safe.

## Risks / Trade-offs

- **Scale.** On the order of 150k lines of Nazm against 17k today: months of sessions. Mitigation:
  phases that each leave a working, tested tree; nothing half-ported is shipped because nothing is
  shipped until R8.
- **Memory and time of Nazm-built tools.** The self-hosted stages have panicked the machine before
  (2026-09-21) and `str_concat` in a loop is quadratic. Mitigation: R1's linear string building
  and bounded heap; budgets measured from R2.
- **Expressiveness.** Without recursive types every tree is an index arena. Mitigation: R1.
- **Coverage loss during the switch** (Cranelift differential tests until R9). Recorded in the
  capability matrix, restored by R9.
- **Two implementations to keep in step until R8.** Every R1 language change lands in both.

## Open Questions

- The performance budgets (compile time and peak memory against the Rust toolchain), set in R2.
- Whether a parallel (task-based) compiler is in scope before R8, which bears on R1's crossing rule
  for immutable trees.
