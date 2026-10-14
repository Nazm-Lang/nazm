# Nazm

> نظم — *systematic order, well-formed arrangement.*

**Nazm is a natively compiled programming language with explicit, specified semantics,
deterministic memory reclamation without a garbage collector or lifetime annotations,
structured concurrency, compiler-checked effects, authority and data provenance, and
machine-readable tooling for people and coding agents alike. A second compiler, written in Nazm
for a declared subset of the language, compiles itself.**

## 0.9.0 — a public preview of the 1.0 draft

Nazm **0.9.0** is a preview: the toolchain, written in Rust, for **macOS on Apple Silicon**
(`aarch64-apple-darwin`) only, and the language as a **1.0 draft**. There is **no compatibility
promise until 1.0**: [`docs/stability.md`](docs/stability.md) states the contract 1.0 will make —
what will be stable, what experimental or internal, and what a patch, minor or major release may
change — and [`tests/compat/v1/`](tests/compat/v1) is the corpus the toolchain is held to on the way;
a preview may still change what they say. The toolchain is being rewritten in Nazm
([`openspec/changes/rewrite-in-nazm`](openspec/changes/rewrite-in-nazm)); 1.0 comes after it.

The preview is backed by contained bootstrap, mutation, fuzzing and artifact-verification evidence,
and by the gates and release notes in [`docs/releases/`](docs/releases) — what was not run for it
is named there ([`docs/releases/0.9.0.md`](docs/releases/0.9.0.md)). Optional subsystems — GPU
kernels, smart contracts, the freestanding and WebAssembly targets, the agent and editor tooling —
remain experimental or partial where documented, and nothing in Nazm is certified, proved correct
or claimed free of defects. External contributions are not accepted yet: the contributor licence
agreement awaits legal review ([`docs/licensing.md`](docs/licensing.md)).

| | |
| --- | --- |
| Language | **1.0 draft** — [`docs/spec.md`](docs/spec.md) |
| Toolchain | **0.9.0** — `nazm --version` |
| Target | macOS on Apple Silicon (`aarch64-apple-darwin`); others are unverified rather than unsupported, and not released |
| Standard library | **1.0 draft** |
| Licence | Apache-2.0 WITH LLVM-exception — what Nazm compiles into a program carries no obligation; the toolchain binary's third-party notices are in `THIRD-PARTY-NOTICES.md` ([`docs/licensing.md`](docs/licensing.md)) |

## A taste

<!-- file: taste.nz -->
```nazm
trait Area {
    fn area(self: Self) -> Int;
}

struct Rect {
    w: Int,
    h: Int,
}

impl Area for Rect {
    fn area(self: Rect) -> Int { self.w * self.h }
}

fn doubled[T: Area](shape: T) -> Int {
    shape.area() * 2
}

fn main() -> Int {
    let r = Rect(w: 3, h: 4);
    r.area() + doubled(r)
}
```

<!-- run -->
```console
$ nazm run taste.nz
36
$ nazm build taste.nz -o taste && ./taste
taste
36
```

`nazm run` checks and interprets; `nazm build` compiles the same program to a native executable,
which gives the same answer.

## Install

On macOS on Apple Silicon, the 0.9.0 preview installs prebuilt:

```sh
brew install nazm-lang/tap/nazm
```

or from the archive on the [release page](https://github.com/Nazm-Lang/nazm/releases/tag/v0.9.0)
([`docs/releases/0.9.0.md`](docs/releases/0.9.0.md), *Installing*). `nazm build` also needs the
Xcode command-line tools.

To build from source, you need the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it on first use) and
`clang`, which `nazm build` uses for the runtime and the link — Apple's on macOS (Xcode
command-line tools), your distribution's `clang` package on Linux.

```sh
git clone https://github.com/Nazm-Lang/nazm
cd nazm
cargo build --release -p nazm-cli            # target/release/nazm
# or: cargo install --path crates/nazm-cli --locked
nazm --version                                # nazm 0.9.0
```

The binary carries the standard library inside it and needs no installation directory. Nothing
else is required for the language itself. Optional tools unlock optional features only:

| Optional tool | For |
| --- | --- |
| Docker | `cargo xtask contained …`, the bounded runner the heavy gates (selfhost, bootstrap, fuzzing, mutation) must use, and the Linux, QEMU, WebAssembly and EVM test images |
| `lldb` / `gdb` | debugging a `nazm build --debug` executable |
| Node.js, `wasm-ld` | running a `wasm32-unknown-unknown` build (the reference host) |
| py-evm (in the `nazm-evm` image) | running `nazm contract --evm` output |
| QEMU, `ld.lld` (in the `nazm-qemu` image) | booting the freestanding board targets |
| OpenCL (built into macOS) | `nazm accel` |

## Try it

[`docs/getting-started.md`](docs/getting-started.md) goes from nothing to a checked, tested,
formatted, natively built program and a package in about ten minutes, and the test suite runs
every command on it. In short:

```sh
nazm run FILE.nz        # check, then interpret; main's result is the exit status
nazm check FILE.nz      # type-check only; --json for nazm.diagnostic/1
nazm build FILE.nz -o OUT [--backend llvm|cranelift] [--debug]
nazm test DIR           # every NAME.nz with a NAME.expected, interpreted and compiled
nazm fmt FILE…          # the one layout; --check to verify
nazm inspect FILE.nz    # everything the compiler knows about a program, as JSON
nazm init DIR --template cli    # a new package: cli, library, server, embedded, wasm, contract
```

[`examples/tour/`](examples/tour/) has small programs for records and enums, `Result` and `?`,
generics, closures and named functions as values, traits, and tasks under a `scope` with the
authority to start them — each checked, run and built under both backends by the suite — and
[`examples/worker-pool.nz`](examples/worker-pool.nz) shows a larger concurrent program.

## What is in the language

- **Values and types:** `Int` (64-bit, every operation checked — overflow and division by zero
  stop the program, never wrap), `Bool`, `Str` (bytes); records, enums with exhaustive `match`,
  first-order generics with `Equality` and trait bounds, traits and methods (nominal, static dispatch),
  closures and function values.
- **Errors as values:** `Result`, `Option` and `?`. No exceptions, no null.
- **Memory:** values are reclaimed deterministically without a garbage collector or lifetime
  annotations; sequences are shared handles, by specification
  ([`docs/spec.md`](docs/spec.md), *The memory constitution*).
- **Effects, authority, provenance:** a function may declare its effects (`! { io }`, `! {}`);
  authority to touch the outside world is a value passed down from `main`, never ambient; data
  from files and arguments is tracked to the sinks it may not reach. All checked statically —
  none of it is a sandbox.
- **Concurrency:** `scope` / `spawn` with typed channels, `select` and deadlines; no task outlives
  its scope. Compiled programs run tasks on a bounded M:N worker pool by default.
- **Modules and packages:** a file is a module, `pub` exports; `nazm.toml`, a lockfile with
  BLAKE3 digests, a local registry and a backtracking resolver.
- **Native code:** one shared LIR, translated by LLVM or Cranelift; C interop (a stated ABI
  subset); WebAssembly and two freestanding boards; restriction profiles (`embedded`,
  `realtime`, `critical`, `cyber`, `web3`, …) that refuse whole classes of construct.
- **Tooling:** formatter, machine-applicable fixes, LSP server, a read-only MCP server, JSON
  output under versioned schemas for every command, build provenance, SBOMs and signed
  attestations.

What each of these does **not** do is in [`docs/limitations.md`](docs/limitations.md).

## Platforms

| Target | LLVM | Cranelift |
| --- | --- | --- |
| `aarch64-apple-darwin` | run-verified | run-verified |
| `x86_64-apple-darwin` | run-verified (Rosetta 2) | run-verified (Rosetta 2) |
| `aarch64-unknown-linux-gnu` | run-verified (Linux container) | run-verified (Linux container) |
| `x86_64-unknown-linux-gnu` | compile-only | compile-only |
| `aarch64-unknown-none`, `riscv64gc-unknown-none-elf` | emulator-verified (QEMU) | unsupported |
| `wasm32-unknown-unknown` | run-verified (Node.js) | unsupported |

Windows, 32-bit hosted and big-endian targets are not supported. [`docs/support.md`](docs/support.md)
has the full matrix — debuggers, runtime restrictions, the optional features (GPU, WebAssembly,
EVM, sBPF, JIT, formal proof) and the evidence for each cell — and is checked against the
compiler's own table. Every emitted LLVM module still carries the note it was first given, *"verified on
aarch64-apple-darwin with Apple clang; other targets are unverified rather than unsupported"*;
the matrix is the current answer.

## What is experimental, and what is not done

The core above is what 1.0 makes stable. Around it, eight capability areas remain **partial** —
GPU and HPC, LSP and MCP, package tooling, the debugger and profiler, performance measurement,
reproducibility and provenance, the platform matrix, and smart contracts — and four things are **blocked by
something outside this repository**: an in-process JIT (it needs `unsafe`, which the workspace
forbids), theorem proving (no proof assistant; formal evidence is bounded model checking),
sBPF/Solana execution (no toolchain or validator), and a second GPU provider (no Metal or SPIR-V
toolchain). Among the language's own boundaries: no trait objects, no effect handlers, no networking in the standard library, and **a compiled program has no iteration
budget** — `nazm run` stops a runaway loop after `--max-iterations`, a native executable does not.

[`docs/limitations.md`](docs/limitations.md) is the full list;
[`docs/capability-matrix.md`](docs/capability-matrix.md) is the evidence, area by area.

## Evidence

Each claim above is backed by a test or a recorded run named in
[`docs/capability-matrix.md`](docs/capability-matrix.md), and the evidence is described by what it
does **not** establish as carefully as by what it does:

- **Differential testing:** the interpreter, both native backends and LIR's own interpreter are
  held to the same results on a shared corpus; passing says nothing about bugs all of them share.
- **Self-hosting:** `compiler/*.nz` is a compiler for a declared subset of Nazm, written in Nazm.
  The bootstrap (Rust → C1 → C2 → C3) reaches a fixpoint — C2 and C3 byte-identical — which shows
  it reproduces itself, and it agrees with the reference compiler on its conformance corpus (34
  cases and 30 refusals) and 21 parity probes. It does not cover the whole language — string
  escapes, `::` and `as`, packages, `Chan[T]`, `select`, the clock, device registers and most of
  the C boundary are outside its subset — and the Rust compiler remains the reference. A fixpoint
  is not an independent proof of semantics ([`docs/bootstrap.md`](docs/bootstrap.md)).
- **Mutation testing:** a catalogue of deliberately injected defects, 1,285 entries, each required
  to be caught by a named test; all 1,268 it held at the core release gate were caught there, and
  the seventeen added since in their own responsibility-mapped campaigns
  ([`docs/releases/920a2a5.md`](docs/releases/920a2a5.md),
  [`docs/releases/996984f.md`](docs/releases/996984f.md)). Zero survivors describes the
  catalogue, not the absence of bugs.
- **Fuzzing:** coverage-guided fuzzing of five targets with a replayed corpus.
- **Contained gates:** the heavy work runs only inside a bounded container (memory, swap, pids,
  disk and time all capped), because a runaway compiler stage once took the development machine
  down ([`docs/runbook.md`](docs/runbook.md)).
- **Artifacts:** a release is assembled by `scripts/release-candidate.sh` with a SHA-256 manifest
  of every file, a provenance record, and a BLAKE3-indexed evidence bundle with an SBOM. The v0.3.0
  candidate was assembled twice from one commit, in one image on one host, and the two archives
  were byte-identical ([`docs/releases/996984f.md`](docs/releases/996984f.md)).

| Mechanism | Does **not** establish |
| --- | --- |
| Ownership and reclamation | Business-logic correctness |
| Effects and purity | That an edit preserves behaviour, or that a call may be dropped or reordered |
| Capabilities (static) | Isolation at run time: nothing sandboxes the process |
| Provenance (static) | Non-interference or confidentiality: only explicit data flow is tracked |
| Structured concurrency | Deadlock freedom: a deadlock is possible and not detected |
| Foreign functions | Anything about the C side, which is trusted entirely once called |
| Restriction profiles | Certification, sandboxing or a security property beyond each rule |
| Bounded model checking | A proof: nothing beyond the stated bounds is covered |
| Reproducible builds | Identity across hosts, operating systems or toolchain versions |
| The bootstrap fixpoint | Compiler trustworthiness |
| Passing the test suite | Correctness |

"Verified-correct" does not appear anywhere in this project.

## Documentation

| | |
| --- | --- |
| [`docs/getting-started.md`](docs/getting-started.md) | The first ten minutes |
| [`docs/guide.md`](docs/guide.md) | The language, for someone writing a program in it |
| [`docs/spec.md`](docs/spec.md) | What each construct means, and the decision behind it |
| [`docs/stability.md`](docs/stability.md) | Version identities, and what v0.x promises |
| [`docs/support.md`](docs/support.md) | Targets and optional features, cell by cell |
| [`docs/limitations.md`](docs/limitations.md) | What Nazm does not do |
| [`docs/capability-matrix.md`](docs/capability-matrix.md) | **"Does X exist?"** — every area, with evidence; the status authority |
| [`docs/diagnostics.md`](docs/diagnostics.md) | The machine interface: schemas, fixes, exit statuses |
| [`docs/architecture.md`](docs/architecture.md) | The compiler design record |
| [`docs/master-architecture.md`](docs/master-architecture.md) | The durable laws and the evidence vocabulary |
| [`docs/bootstrap.md`](docs/bootstrap.md) | Self-hosting: what it does and does not establish |
| [`docs/performance.md`](docs/performance.md) | What has been measured, and the noise floor under it |
| [`docs/roadmap.md`](docs/roadmap.md) | What came in which order, and what is next |
| [`docs/runbook.md`](docs/runbook.md) | **Containment and resource safety — read before running anything heavy** |
| [`docs/releases/`](docs/releases/) | Frozen evidence, one file per release candidate |
| [`docs/background.md`](docs/background.md) | Why the project exists: the argument, the agent-cost experiment, and the design it was built toward |
| [`docs/NAZM_LANGUAGE_GOALS.md`](docs/NAZM_LANGUAGE_GOALS.md) | Long-term ambitions — **goals, not evidence** |
| [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md) | How to contribute, and how to report a vulnerability |

## Developing Nazm

```sh
cargo build --workspace
cargo lint                      # clippy, as CI runs it
cargo xtask check               # architectural and documentation gates
cargo xtask workspace-tests --host-safe --jobs 4 --workers 1 --test-threads 4 --shape ci
cargo xtask contained tests     # the whole suite, selfhost included, in the bounded runner
cargo xtask contained bootstrap # C1 → C2 → C3
```

The generated compiler stages, the selfhost suite, the bootstrap, fuzzing and the mutation
catalogue run **only** through `cargo xtask contained`; they refuse to run on a bare host.
CI runs the host-safe subset ([`docs/runbook.md`](docs/runbook.md)).
[`CONTRIBUTING.md`](CONTRIBUTING.md) has the rest.

Alongside the compiler: `nazm-mcp`, a read-only Model Context Protocol server over the same
compilation ([`docs/mcp.md`](docs/mcp.md)); `nazm-tokens`, which counts text under four pinned
tokenizers; and `nazm-bench` and `nazm-agent-bench`, the agent-cost evaluation harness
([`docs/background.md`](docs/background.md)). Nothing that ships depends on them.

## Command reference

| | |
| --- | --- |
| `nazm init DIR [--template T]` | A new project from a template — `cli`, `library`, `server`, `embedded`, `wasm` or `contract` — whose README names the commands that check, test, build and document it; an existing non-empty directory is refused (N74) |
| `nazm run FILE` | Check and evaluate, printing the value of `main` |
| `nazm check FILE` | Check without evaluating. Reuses the result of checking a module whose source and dependencies' interfaces are unchanged; `--no-cache` checks everything, `--cache-report` says what it did, `--summary-json` prints one compact `nazm.command-summary/1` — status, exit status and a reference to each diagnostic, no prose |
| `nazm build FILE` | Compile to a native executable. Each module becomes its own object, and an object whose exact LLVM input, backend and codegen configuration were compiled before is reused instead; `--no-cache` compiles everything, `--build-report` says what it did, `--summary-json` prints one `nazm.command-summary/1` with the artifact written. The link runs every time. `--backend llvm\|cranelift` picks the code generator (N41), `--target TRIPLE` names one of the seven targets — an executable where this host links it, `--objects` elsewhere (N57) — `--link FILE` adds C objects (N42), `--locked` and a package directory build a package against its lockfile (N46), `--profile NAME` holds it to a restriction profile (N47), `--debug` writes DWARF — under either backend since N91 — and `--timings` reports each phase (N48) |
| `nazm test PATH…` | Run every `NAME.nz` with a `NAME.expected` beside it, and every `@test` and `@fuzz` function, interpreted **and** compiled. `--filter TEXT` narrows by name; `--fuzz N` sets a fuzz target's inputs. `--summary-json` prints one `nazm.test-summary/1`: counts, and only the cases that did not pass, with how each leg ended |
| `nazm bench PATH…` | Measure every `@bench` function, built natively at `-O2` and called `--iterations` times; `--json` prints one `nazm.bench/1` per function |
| `nazm fmt FILE…` | Lay files out canonically. `--check` writes nothing and fails if any would change |
| `nazm fix FILE` | Apply the repairs the compiler already knows about. Nothing is written without `--apply`, and only fixes marked `automatic` are ever applied |
| `nazm capabilities` | What this build supports, read from the compiler's own tables |
| `nazm obligations FILE` | Every obligation of a checked program, `nazm.obligations/1` (N87): each contract clause, call of a function with a precondition, overflow, division, index, allocation, call through a function value and call into C — proved (with its witness), checked as the program runs, or unknown. Compiler evidence, not certification; `--json` |
| `nazm trace FILE --requirements FILE` | Requirements traced to what verifies them, `nazm.trace/1` (Gate 3): each requirement of a `requirements.toml` to its definitions — durable identity, digest and obligations — and to its `@test` functions, run; a broken link reported by name and failing the command. Not certification |
| `nazm core-ir FILE` | What a checked program became — its Core IR, `nazm.core-ir/2`: typed, every name an identity, every transfer explicit. A debugging view of what `nazm run` and `nazm build` both start from; `--no-spans`, `--json` |
| `nazm explain-cost FILE` | Each function's allocation, retain, block, channel, foreign and task sites, read off MIR, with a count per call — `at-most-once` or `unknown` (N52); `--json` for `nazm.cost/2` |
| `nazm explain-flow FILE` | Every sink — a path written to, standard output, a file's data — and every declassification, with the origins that reach it (N52); `--json` for `nazm.flow/2` |
| `nazm inspect FILE` | One program's facts as `nazm.inspect/2`, one deterministic line: the toolchain's identities, the package and its lock, and per definition its identity, types, effects, capabilities, provenance, Core IR and MIR digests (N48); `--def NAME` for one |
| `nazm attest sign --key KEY PROVENANCE ARTEFACT…` / `nazm attest verify --allowed-signers FILE --identity ID STATEMENT ARTEFACT…` | Wrap a build's provenance in an in-toto Statement v1 over the artefacts it names and sign it with `ssh-keygen -Y sign`; verify a statement's signature for an identity and every artefact's digest. `nazm build --sbom FILE` writes a CycloneDX 1.5 SBOM (N94) |
| `nazm lock DIR` | Resolve the package in `DIR` (its `nazm.toml`) and write `nazm.lock`, `nazm.lock/1`: every package's exact version, source and BLAKE3 digest, in name order, rewritten only when its bytes change. `nazm check`, `build` and `run` take a package's directory, and `--locked` holds them to its lockfile (N46) |
| `nazm update DIR [NAME…]` | Re-resolve the package in `DIR` with its lockfile's registry versions no longer preferred — only the named packages', or every one's — print each change as `name old → new`, and write `nazm.lock`. Resolution backtracks and explains a refusal (N90) |
| `nazm profile FILE` | Build the program and run it once with its memory and scheduler counters on; print `nazm.profile/2`: exit status, wall time, output size and the counts (N58). `--sample [--interval-ms N]` also samples the run with macOS's `sample` and attributes each sample to a Nazm function and `.nz` line, the runtime, foreign code or the system, waits counted as blocked (N91) |
| `nazm publish DIR --registry REG` | Copy the package in `DIR` into the local registry `REG` and record its version and digest in `REG/index/NAME.toml`, `nazm.registry-index/1`; a published version never changes (N56) |
| `nazm publish … --dry-run` | Every check a publish makes, and what it would record — `nazm.publish-plan/1`: name, version, digest and files — writing nothing (N74) |
| `nazm doc PATH [-o DIR]` | A program's or package's public API from its semantic definitions, `nazm.api-doc/2`: each exported function, record and enum with its signature, effects, capabilities, foreign symbol or export and a link to its source line; `-o` also writes `api.md`, anchored by identity (N74) |
| `nazm bindgen HEADER [-o FILE]` | The supported subset of a C header — `int64_t`, `bool` and `const char *` arguments — as `extern "C"` declarations the checker has held; everything else refused with the reason, never widened; `nazm.bindgen/1` with `-o` (N74) |
| `nazm build … --provenance FILE` | After a successful build, `nazm.provenance/1`: sources, packages, toolchain, runtime, backend options, profiles and output digests; `--attest-with CMD` keeps a local signer's output beside it (N73) |
| `nazm yank NAME@VERSION --registry REG` | Withdraw a published version: never chosen anew, still used where a lockfile names it; `--undo` restores it (N56) |
| `nazm runtime build -o DIR` | Compile the whole runtime for a target and profile once, into `DIR/libnazmrt.a` with a `nazm.runtime/1` manifest; `nazm build --runtime DIR` links it instead of generating one, and refuses one that does not fit; `nazm runtime verify DIR` checks it (N82) |
| `nazm lir FILE` | What LIR decided for a native build — `nazm.lir/2`: the target's data layout, every unit's symbols and linkage, every record's and enum's byte layout on that target, which types own anything, and every unit's instructions with their digest — the one LIR both backends translate (N105), validated. `--target TRIPLE` (N81) |
| `nazm mir FILE` | What `nazm build` lowers a checked program to — its MIR, `nazm.mir/1`: each concrete function (generic instances included) as basic blocks with every copy, move, drop, scope join and failure edge written out, after the validator accepted it. A debugging view; `--spans`, `--json` |
| `nazm contract FILE [--txs FILE]` | A contract under the `web3` profile: its state, entrypoints and read/write sets as `nazm.contract/1`, or a deterministic run of transactions on the interpreter as `nazm.contract-run/1` — reverts, a step meter, and conservation of a declared quantity (`N0396`) (N69); `--evm DIR` compiles it to EVM bytecode with its ABI, storage layout and a py-evm harness, `--upgrade-check OLD` checks a storage upgrade (N70) |
| `nazm accel FILE KERNEL --input FILE` | Map a pure `(Int) -> Int` function over `Int`s on the GPU through OpenCL (macOS); `nazm.accel/2` names every transfer, the synchronisation, timings and the kernel's identity, and holds the results to the interpreter's; `--print-kernel` shows the OpenCL C (N67) |
| `nazm repl` | Definitions and expressions a line at a time, checked as one program on every edit; a redefinition that breaks another definition is refused by its name (N65) |
| `nazm comptime FILE FUNCTION` | Evaluate a parameterless pure function under the interpreter's budgets; anything that could reach the host is refused (`N0393`) (N65) |
| `nazm reload-check OLD NEW` | `nazm.reload-check/1`: per definition, whether a running image could take the change or needs a restart, and why (N65) |
| `nazm interface FILE` | Write what that module offers, durably — `nazm.interface/13`, or its fingerprint with `--hash`. Not a cache: it reuses nothing |
| `nazm resolve FILE` | Each module's resolved unit, `nazm.resolved/1`, one per line: its imports, definitions and every resolved occurrence by durable identity — what its check entry stores; a reused module's is read, not recomputed (N77) |
| `nazm references DEF FILE` | Every occurrence of one entity — `DEF` a durable key, `module::kind name`, with `.member` for a variant, field or payload field — read from the modules' resolved units, `nazm.references/1`; a module whose check is reused is not checked again, and the output says how many were (N77) |
| `nazm context --root FILE --file FILE --byte N` | One definition's semantic context packet, `nazm.context/3`: the exact source of the function, record or enum declared or used at that byte, and compact links to what it uses, what uses it, what it calls and what calls it — in the root's compilation, read from disk. `--json` prints the canonical compact form |
| `nazm snapshot --root FILE` | The root compilation's semantic snapshot, `nazm.snapshot/3`: every durable function, record and enum by identity, with one digest per section the compiler records — source, shape, dependencies, related types, references, callers, callees, diagnostics. Equal digests are not equal behaviour. `--json` prints the canonical compact form |
| `nazm delta --root FILE --baseline FILE` | What changed since a kept snapshot, `nazm.delta/3`: definitions added, removed, and changed, naming which recorded sections changed — a comment or a literal is `source` alone, a rename is a removal and an addition. A baseline of another root or compiler, or not exactly `nazm.snapshot/3`, is refused. `--json` prints the canonical compact form |
| `nazm diagnostics --root FILE` | The root's current diagnostics compactly, `nazm.diagnostic-index/1`: each one's id, code, severity, file and byte range, owning definition and fix counts — no message or help text. `--detail ID --state DIGEST` prints one of them in full, `nazm.diagnostic-detail/1`: the diagnostic exactly as `nazm.diagnostic/1` has it, its places in their files, and each fix's `nazm patch fix` selector where one exists. `nazm check --json` is unchanged |
| `nazm docs --index` | Nazm's own documentation as stable sections, `nazm.docs-index/1`: every document with its authority — normative spec, grammar, architecture, evidence, planning, goal, and the rest — and every section's id, title, parent and digest, no body. `--section ID` prints that section's own canonical text byte for byte (`nazm.docs-section/1` with `--json`); `--state` refuses it if the documentation changed since the index. Ids are exact; there is no search. Reads the repository's `docs/`, not a project's |
| `nazm repo --map` | The repository's context map, `nazm.repository-map/1`: every package and target, source root, compilation root, module and durable definition, document, schema and mutation profile, with its authority, its structural relationships and where to retrieve more — no body. `--task --seed ID [--intent understand\|edit\|diagnose\|test]` prints the minimum context for a task, `nazm.task-context/1`: each item with why it is there, the seed's exact source and the selected sections' exact text, within `--max-entities`/`--max-source-bytes`/`--max-doc-bytes`; a cut makes it `partial`, and `--state` refuses a repository that changed since the map. Seeds are ids, never paths |
| `nazm patch rename --root FILE --file FILE --byte N --new-name NAME` | A planned rename, `nazm.patch/1`: the compiler's validated rename of the local or private definition at that byte — and, within a declared workspace (a `nazm.root` marker, `--source-root` or an application package), of an exported one across every module under the root (N89); never a library package's API — as minimal exact-byte edits bound to the root's snapshot, each file's digest and each edit's expected bytes. Plans; never applies. `--json` prints the canonical compact form |
| `nazm patch apply PLAN [--base DIR]` | Applies a `nazm.patch/1` plan to disk, all or nothing (N89): every file must hash to the plan's digest and hold each edit's expected bytes, or nothing is written and the differing file is named; each file written through a temporary file and a rename |
| `nazm patch fix --root FILE --file FILE --byte N --fix I` | A planned fix, `nazm.patch/1`: fix `I` of those the current diagnostics at that byte carry, with its applicability and precondition, bound the same way. Plans; never applies |
| `nazm lsp` | Serve the Language Server Protocol to an editor over stdin and stdout: diagnostics, definition, hover, references, safe rename of locals and private definitions — and of exported ones within a declared workspace (N89) — completion of values, call heads and types (invoked on an identifier), completion of a field after `.`, a variant after `E.` and a construction's or pattern's label — also with nothing written yet, right after the `.` or `(`, where the compiler can tell what it belongs to, and triggered by `.`, signature help for calls of functions and built-ins and for record and variant constructions, document symbols (one file's declarations, nested, in source order) workspace symbols (the declarations of every file the open documents' programs load — not a directory scan) full-document semantic tokens (every identifier the checker resolved, by kind and whether it declares; not keywords, comments or literals) quick fixes (the fixes the compiler attaches to current diagnostics, as versioned edits the editor applies) and call hierarchy (a function's direct callers and callees, from the calls the checker recorded in the program the request came from — not every caller in a project), over unsaved buffers synchronised by ranged edits — only the item an edit reaches is reparsed (N76) — from the same loader and checker as every other command, across a package's dependencies. Nothing else is offered. `editors/vscode` is a client for it (N74) |

An empty `nazm.root` file marks the directory it sits in as the project's source root, so a
module keeps one identity whichever file a compilation started from. `--source-root DIR`
says the same thing without writing a file. With neither, the root is the directory of the
file you named. The marker must be empty: it says where a project begins and owns nothing
else.

`--json` on any of them emits machine-readable output under a **named, versioned
schema**; the schemas are in [`schema/`](schema) and the compatibility rules are in
[`docs/diagnostics.md`](docs/diagnostics.md). `scripts/repair.sh` is the whole repair
loop as one command: apply every automatic fix until a pass changes nothing, lay the
result out, and report what is left as JSON.

## Licence

The Apache License 2.0 with LLVM Exceptions (`Apache-2.0 WITH LLVM-exception`), copyright Ferdous
Alam — see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). The exceptions, the ones LLVM and Cranelift
carry, mean that what Nazm compiles into your program — the runtime, the core prelude and the
standard library — imposes no attribution or notice requirement on it. What a compiled program
contains, and what is still open, is [`docs/licensing.md`](docs/licensing.md). The specification
is also under CC-BY-4.0, the `nazm init` templates are MIT-0, and contributions are made under a
contributor licence agreement ([`docs/CLA.md`](docs/CLA.md)).
