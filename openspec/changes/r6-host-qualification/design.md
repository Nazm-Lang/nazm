## Context

At `95793836` the toolchain written in Nazm has every piece a build needs: the front end
(`compiler/`, R2) writes the Core IR document `nazm core-ir` prints; the pipeline
(`toolchain/pipeline/`, R3) lowers it to MIR and LIR and generates the runtime's units; the code
generator (`toolchain/codegen/`, R9) writes an object per LIR unit. Until now each was run beside
the Rust toolchain and compared with it; none had built another, and none had built itself.

Containment is in force for all of R6 (`rewrite-in-nazm` design, decision 9; `docs/internal/runbook.md`,
*Mandatory containment*). The contained runner is a Linux container, the only place on this host
where a memory ceiling is enforced.

## Goals / Non-Goals

**Goals:** the toolchain builds itself, stage over stage, to a byte-identical fixpoint, each built
program under a heap limit, every step's peak memory recorded (task 7.1); a decision, argued from
that evidence, on what host execution the runbook could allow (task 7.2) — **proposed here, decided
by the copyright holder**.

**Non-goals:** the cut-over (R8: the released `nazm` built by Nazm); a `nazm build` written in Nazm
(the self-build drives the three programs from a script); Linux as a supported target; timings
(the quiet window measures time, not this change).

## Decisions

### 1. The self-build: what builds what

`toolchain/selfbuild/selfbuild.sh`, run only by `cargo xtask contained selfbuild`:

| stage | built by | builds |
| --- | --- | --- |
| 0 | the seed, the Rust `nazm build --heap-limit` | the builder: `fe` (`compiler/diagnose.nz`), `pl` (`toolchain/pipeline/main.nz`), `cg` (`toolchain/codegen/main.nz`) |
| 1, 2, 3 | the previous stage's builder | the builder, and `nazm` (`toolchain/commands/nazm.nz`), `mcp` (`toolchain/commands/mcp.nz`), `nazm-mutate` (`toolchain/harness/mutate/main.nz`), `jtool` (`toolchain/harness/tools/jtool.nz`) |

A program is built as `fe --coredoc-sources` → `pl ops` (LIR) and `pl --target T runtime CORE - HEAP`
(the runtime's units, with the counted heap) → `cg --target T [--one-thread]` (an object per unit);
the runtime's units, which R3 leaves as LLVM IR text, are assembled by the platform's `clang -c`, and
`cc` links. The script's glue is what `nazm build` does around those steps: the program's LIR calls
the counted heap where it calls C's allocator (`nazm_lir::op::count_heap`, imports only),
`--one-thread` where the runtime's stack limit is a plain global, `-lpthread -lm` (and `-lssl
-lcrypto` with a TLS unit), as `link.txt` names them on Linux.

The target is the container's own, `aarch64-unknown-linux-gnu`. The pipeline accepted only the
macOS and Windows triples; it now also accepts the two Linux ones, for this and nothing else: the
runtime generator already generated Linux units (`rtgen.nz`), and the code generator already wrote
ELF (R9 6.1). Linux stays outside `docs/user/support.md` and the copyright holder's 1.0 narrowing is
unchanged; what is new is only that the ELF objects are now linked and run, inside the container.

### 2. What the run checks

1. **The fixpoint.** Stages 1, 2 and 3 are compared file by file — every Core IR document, LIR,
   runtime unit and object, every executable: all identical. Stage 1 (built by a builder Rust
   built) equals stage 2 (built by one Nazm built), so the seed-built and the self-built builders
   agree as well as reproduce.
2. **Core IR against the reference.** For each program, the last stage's document is the seed's
   `nazm core-ir --with-sources`, byte for byte, whole (slice 3 of `r8-cut-over` wrote the two
   members the front end lacked: a call's `callee_at`, a payload binding's `at`).
3. **Behaviour against the reference.** The last builder builds 66 programs (the conformance
   corpus, `tests/compat/v1/run/`, the code generator's own test programs) and each runs as the
   seed's build of it does: standard output, standard error and exit status byte for byte —
   failures included, with their `file:line:column`.
4. **The bound.** The last builder builds the front end again with a 256 MiB heap, and it is run
   on the largest program: it must end with `N0406` and status 2.
5. **Memory.** Every step's peak resident memory, the kernel's `ru_maxrss` of the waited child
   (`wait4`, a few lines of C in the script: the image has no tool that reports it), is written
   to `memory.txt`. No time is taken.

`compiler/diagnose.nz` gained `--coredoc-sources`, the document with each module's `source`, `file`
and `path` (`print_with_sources`): without the sources the pipeline cannot turn a span into a line
and column, and every failure message said `:1:1` (the first contained run found 9 of 66 programs
disagreeing only there).

### 3. ACCEPTED 2026-10-20 — revising the containment rule (task 7.2)

**Accepted by the copyright holder on 2026-10-20, exactly as proposed below**, and applied:
`runbook-draft.md` is now `docs/internal/runbook.md`, *What may run on the host: one bounded-heap stage*; the
runner is `cargo xtask host-run` (`xtask/src/host_run.rs`; tests `xtask/tests/host_run.rs`). The
copyright holder also accepted keeping the pipeline's Linux triples **internal-only** (the container
self-build), with no user-facing support claim: recorded in `docs/user/support.md` and the capability
matrix. The text below is the proposal as decided; "would" reads as "does".

**What it would allow on the macOS host, outside the contained runner:** running one program
written in Nazm — a builder program (`fe`, `pl`, `cg`), a command of `toolchain/commands/`, or a
harness tool — **built with a heap limit of at most 2 GiB**, as one foreground process with a
wall-clock deadline, **one at a time**, never alongside another such process or a contained
workload's host side. The runner that would start it (to be written once this is accepted) refuses
a binary without the counted heap (its `nz.heap_malloc` defined) or with a larger limit, so the
condition is checked, not declared.

**What it would not allow**, unchanged: the selfhost suite; `compiler/bootstrap.sh` (its `nazmc`
stages are built without a heap limit); the self-build itself (many stages, built and run in a
row); fuzzing, the mutation catalogue, heavy benchmarks; any program built without a limit; two
host stages at once. `NAZM_CONTAINED=1` and `NAZM_ALLOW_UNCONTAINED=1` stay forbidden on a host.

**The argument.** The runbook's rule exists because macOS cannot cap a child's memory from outside
(`RLIMIT_AS` is advisory) and because on 2026-09-21 ten uncapped stages grew together until the
machine panicked. The heap limit is a ceiling from *inside*: every byte the program's code and its
runtime ask the allocator for is counted before it is allocated, and an allocation past the limit
does not happen — the program writes `N0406` and exits (`docs/user/spec.md`, *A bounded heap*). The
count is the bytes asked for, so it is the same on Linux and macOS; it does not depend on the
platform enforcing anything. The evidence (*The evidence*, below):

- under a 2 GiB limit, the largest step of the whole self-build — the front end on
  `toolchain/commands/nazm.nz` — peaks at about 1.3 GB resident, and the builder building itself
  at about 0.9 GB;
- with the limit lowered to 256 MiB, the same front end on the same input stops with `N0406` at
  225 MiB resident: the bound holds in a self-built stage, and resident memory stays below it
  (stacks, image and C's own allocations are small);
- three stages, 21 builds, 66 corpus programs and the probe ran with no out-of-memory kill and no
  survivor.

So one host stage under a 2 GiB limit holds at most about 2 GiB plus what the heap does not count;
on this 32 GiB machine that is a bounded fraction, where the incident was unbounded growth times ten.

**What the heap does not bound, and why the rule still holds:** a task's stack (each its own
mapping, sized by *Native recursion*: bounded by the number of tasks, and the builder programs
start none); the image and static data (tens of MB); what C allocates for itself (stdio buffers);
and child processes (the builder spawns none; a command that runs `clang` or `cc` runs the
platform's tools, as the Rust `nazm` does on the host today). Hence *one at a time*, and the
deadline, which bounds time but not memory.

**Conditions that would withdraw it:** any host run whose peak resident memory (read with
`/usr/bin/time -l`) exceeds its heap limit by more than 256 MiB; any `N0406` that did not stop the
program; any change to the counted heap's runtime (`crates/nazm-runtime/src/heap.rs`, `rtgen.nz`'s
`count_heap`) until the self-build's probe passes again. Each returns the rule to *contained only*
until investigated.

**Precedent and caveat.** R2's design D6 (revised 2026-10-15) already runs the R2 drivers on this
host, built by the reference with `--heap-limit 2G`, under a deadline: the same mechanism, for
programs the reference built. This proposal extends it to programs the Nazm toolchain built. The
numbers above were measured in the Linux container; macOS's allocator may keep more resident for
the same heap, which is why the withdrawal condition is stated in resident memory, and why the first
host runs record it.

## The evidence

`cargo xtask contained selfbuild` at this change, P1 (1 CPU, 4096 MiB, swap equal), Linux aarch64
in `nazm-contained:1.98.1`, heap limit 2 GiB per built program: **passed** — stages 1, 2 and 3 identical in all 385
files, 7 Core IR documents identical to the reference's, 66 of 66 programs agreeing with the seed's
builds, the probe stopped by `N0406`, no out-of-memory kill, no survivor. An earlier run with a 3 GiB
limit gave the same fixpoint and peaks within 2%.

Peak resident memory, MiB, the last stage (stages 1 and 2 within 2% of it):

| program | front end | pipeline (LIR) | pipeline (runtime) | code generator |
| --- | --- | --- | --- | --- |
| `fe` (the front end) | 900.5 | 374.7 | 347.9 | 206.1 |
| `pl` (the pipeline) | 557.0 | 219.2 | 203.8 | 118.9 |
| `cg` (the code generator) | 265.4 | 108.1 | 101.0 | 46.8 |
| `nazm` (R5 commands) | 1299.9 | 522.4 | 475.0 | 299.9 |
| `mcp` | 927.5 | 381.4 | 357.6 | 213.9 |
| `nazm-mutate` | 136.6 | 58.8 | 57.4 | 33.0 |
| `jtool` | 62.6 | 27.0 | 25.4 | 13.4 |

Largest step per stage: stage 1 1,299.4, stage 2 1,302.1, stage 3 1,299.9 — each the front end on `nazm.nz`. Stage 0, the seed building the builder: `fe` 300.5, `pl` 147.5, `cg` 91.4 (the Rust `nazm build`, its children included).
The probe (front end built with 256 MiB, on `nazm.nz`): stopped with `N0406`, status 2, at 224.7 MiB resident.

## Gaps

- **The interpreter and `nazm-check` are not built.** `toolchain/interp/` and
  `toolchain/harness/check/` are written for 0.10.0, where `List[T]` was the immutable sequence
  R1.5 D7 renamed `Chain[T]`: the reference refuses them as the Nazm front end does (`N0359` on
  `V.Arr(items: List[V])`, `N0637` for `list_*`, `N0300` for `check_packages(items)` given a
  `Chain[Toml]`). About fifteen lines (`types.nz:23`, `values.nz:57–60`, `vm.nz:519–821`,
  `supply.nz:68`). Not changed here: their tracks build them with the 0.10.0 release, which has no
  `Chain`; they move when those tracks move to the checkout's language, and then join the
  self-build's list.
- **The runtime's units are LLVM IR text**, assembled by `clang -c`: R3 generates them as text and
  R9 does not compile LLVM IR. Nothing written in Rust is involved, but the platform's `clang` is.
- **The self-build is Linux only.** A macOS self-build is a host run, which decision 3 (accepted
  2026-10-20) still keeps contained: the self-build is many stages in a row.
- **No `nazm build` in Nazm.** The script is the driver; R5 does not carry `build`.
