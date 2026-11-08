# How Nazm was built

Nazm's history is written to be read in order. Each commit title says what changed in plain words, and a `Stage:` line says which step of building a programming language the commit belongs to. This page walks the steps, why each exists, and how to find its commits.

Find the commits of a step: `git log --reverse --grep='^Stage: <step name>'`. `Was:` gives the title a commit had before the history was rewritten (2026-11-08); that rewrite changed titles only, and every commit's files are identical.

Steps overlap in time: after the first compiler worked, several parts were rewritten in Nazm in parallel, so a step is a topic, not a calendar slot.

## The steps, in the order a language is built

### 1. Justify the language (benchmark harness and first measurements)

Before writing a language, measure whether it is worth writing. Nazm began with a benchmark harness and honest accounting of what it must beat.

*The lesson:* Decide the goal in numbers first; build the measuring tool before the thing measured.

20 commits, 2026-09-19 to 2026-10-19. To start with:

- `b2d660d2` bench: add the benchmark harness to justify the language
- `b9d58ef6` bench: correct three accounting claims and validate the measurement path
- `0b5ac47a` bench: separate the two denominators and stop overclaiming chronology
- `ac5ae857` bench: add the execution boundary and its failure demonstrations
- `27bdabc6` bench: fix two accounting cases that could not express their measure
- `51465298` bench: add the Docker adapter and four confinement tests
- `5711e40b` bench: add access probes, credential separation and acceptance reset
- `d34fd6c9` bench: add a measurement suite before any number is claimed

### 2. Core language and first compiler (lexer, parser, type checker, IR, backends, runtime, standard library)

The first working compiler, written in Rust: tokens, syntax tree, types, an intermediate representation, two code generators, a runtime and a standard library.

*The lesson:* The classic pipeline: lex, parse, check, lower to IR, generate code, link a runtime.

188 commits, 2026-09-20 to 2026-10-16. To start with:

- `45aa466a` interpreter: execute a .nz program
- `975fe320` interpreter: add loops and mutation
- `5e91dc7a` parser: add break and continue and the second form of if
- `6a389ed5` codegen: Nazm-written compiler numbers its labels separately too
- `028d1149` build: Nazm compiler uses a record
- `8972c1f1` checker: add general, embedded, critical and cyber restriction profiles
- `30c82ae5` cli: add templates, library check, API docs, C headers and editor client
- `5bf107a0` std: add nanosecond clocks and a sleep that parks

### 3. Quality gates (governance, security, mutation testing, specification, release readiness)

Rules that stop the language from rotting: written design rules, a security model, mutation testing (break the code on purpose; a test must notice), a specification, and a gate every change passes.

*The lesson:* Add the safety net before the code it protects grows.

511 commits, 2026-09-21 to 2026-11-08. To start with:

- `10df4ed9` docs: record milestone B complete in the goal ledger
- `34521962` docs: record verified state and next action in the ledger
- `c4f3a93f` docs: update the ledger after the parser, with the checker design
- `15256890` mutation-tests: eighteen mutations against enum composition
- `61208cf9` mutation-tests: 19 mutations on the planner's laws; add repo profile
- `6f479eff` docs: design rules for escapes, logical operators and native recursion
- `4aa829e9` docs: record reproducibility as partial under the stated model
- `3cb763fe` diagnostics: report an unreadable file at itself; name reused parameters

### 4. Prepare the language for its own rewrite

Make the language good enough to write its own compiler: sum types, generics, collections, byte buffers and the checks that let a compiler be written in it.

*The lesson:* A self-hosting language first needs the features a compiler uses.

85 commits, 2026-10-04 to 2026-10-15. To start with:

- `fcc7a00c` tests: record parity of 21 probes through both compilers
- `5c4a42ba` docs: record parity as not accepted, 13 of 21 probes equal
- `1d9eb984` docs: write the design rules for the compiler written in Nazm at parity
- `08e77297` tests: share one memory report reader across the CLI tests
- `52acc9f6` xtask: add the module-boundaries gate
- `e1057e7e` lexer: string interpolation and fixed-point floats
- `9919982b` std: @std/toml, a TOML 1.0 parser and encoder
- `f9649461` tests: Nazm-written compiler and its corpora follow the new language

### 5. Performance and developer experience

Speed and ease of use: faster compiles and programs, simpler syntax, better errors, tools such as the formatter, language server and debugger.

*The lesson:* Make it fast and pleasant once it is correct.

235 commits, 2026-09-21 to 2026-11-05. To start with:

- `15f0f044` bench: measure, profile and act on what the profile showed
- `c2f2e434` bench: format the benchmark runner
- `f8b8fdda` bench: make the benchmark runner pass its lints
- `5b455f5b` cli: nazm fix reads and writes through injected functions; skips tested
- `aa2c93f2` llvm-backend: emit scalar temporaries as SSA values
- `233d6adf` checker: short closures with types taken from what is expected
- `1a5c5ad5` docs: document the failure convention and its measurements
- `086ea6f8` runtime: split the waiter table lock across bucket groups

### 6. Rewrite the front end in Nazm (lexer, parser, checker)

The lexer, parser and type checker rewritten in Nazm itself and compared, input by input, with the Rust originals.

*The lesson:* Port stage by stage; every port must agree with the reference on every input.

101 commits, 2026-10-15 to 2026-10-20. To start with:

- `0adcaa4f` docs: propose the front end in Nazm and add the comparison harness
- `52848811` lexer: Nazm-written lexer scans every token the reference does
- `31306b05` parser: whole-language parser written in Nazm
- `875b7b17` checker: imports and module graphs in Nazm
- `038e16d3` checker: provenance flows and N0372 in Nazm
- `4d0c8d15` tests: tree test reads x += e's place as the concrete tree does
- `64de512b` ir: Core IR of indexing, tuples and wide literals in Nazm
- `f9a68841` checker: add TLS built-ins and reserved symbols to the Nazm front end

### 7. Rewrite the compilation pipeline in Nazm

Name resolution, lowering and the intermediate representations (MIR, LIR) in Nazm.

*The lesson:* The middle of the compiler.

37 commits, 2026-10-15 to 2026-11-05. To start with:

- `1428fd3f` docs: propose the compilation pipeline in Nazm, with order of work
- `444e8d72` ir: Core IR to MIR written in Nazm, 102 of 105 inputs identical
- `2daabb77` ir: MIR validator written in Nazm, held to its mutants
- `3bdebcc1` cli: nazm lir --debug prints positions for line tables
- `73b7952b` ir: LIR type helpers that retain and release in Nazm
- `ed03ee82` ir: LIR statics, sections, exports and foreign calls in Nazm
- `a2700d34` ir: debug information (line positions) in the Nazm pipeline
- `88db89d0` ir: plain reference counts for single-threaded programs in rtgen

### 8. Rewrite the interpreter in Nazm

The interpreter that runs programs without compiling them, in Nazm.

*The lesson:* An interpreter is the oracle that code generation is tested against.

16 commits, 2026-10-15 to 2026-10-16. To start with:

- `020eec68` docs: propose the interpreter written in Nazm
- `256795ed` interpreter: Nazm interpreter runs integers, control flow and calls
- `12319ce8` interpreter: strings, sequences, records, closures, traits and numbers
- `d3fe4947` interpreter: --json diagnostics and failures inside library modules
- `dec9908f` interpreter: Core IR /3 tables answer dispatch, orders and statics
- `9e1c8b60` interpreter: a timed-out select gives way to other tasks
- `7fb020d4` interpreter: make connections and writes yield so networked counts agree
- `4796718c` interpreter: refuse TLS built-ins as values and follow the HTTP library

### 9. Rewrite the command-line tools in Nazm

The `nazm` commands (build, run, check, format, doc, test and more) in Nazm.

*The lesson:* The user-facing surface of the toolchain.

21 commits, 2026-10-15 to 2026-10-19. To start with:

- `40125d5b` cli: nazm fmt written in Nazm, agreeing with the Rust command
- `512dbb59` cli: command-line option parser in Nazm so fmt carries every option
- `e1bf7a5d` cli: nazm init written in Nazm, templates as the Rust command writes
- `de818915` docs: list what nazm doc needs from the Nazm checker
- `1f47e511` cli: nazm fix written in Nazm over the checker's facts
- `4742869e` cli: lift the pattern-let refusal, add source root and editor features
- `236ffe49` cli: write lock, update, publish, yank and attest in Nazm
- `52a55a85` cli: plan patch fix and patch rename over the reference index in Nazm

### 10. Qualify the host (safe running of Nazm-built programs)

Run a freshly self-built compiler safely on a developer's machine: memory ceilings, one stage at a time, containers for everything heavy.

*The lesson:* A compiler that compiles itself can also run away with the machine; bound it first.

27 commits, 2026-10-16 to 2026-11-05. To start with:

- `9d03ebac` windows: build on a Windows host with llvm-mingw and add nazm doctor
- `e9727174` windows: use the toolchain end to end on a Windows host, document setup
- `27f5f468` build: build the Nazm toolchain with itself under a 2 GiB heap limit
- `afd70d7e` docs: accept the containment revision, mark Linux triples internal-only
- `6e575987` repo: merge the host-run mutation catalogue fix
- `6455137d` docs: record the first host self-build run
- `4bbc9060` docs: record the second x86_64 self-build run under Rosetta
- `c35a8659` repo: merge the macOS self-build work for aarch64 and x86_64

### 11. Rewrite the test harness in Nazm

The test and verification harness (fuzzing, mutation, comparison, bench runner) in Nazm.

*The lesson:* Tests are part of the toolchain too.

64 commits, 2026-10-15 to 2026-11-06. To start with:

- `67dcf9b5` docs: propose the test harness and tests in Nazm
- `b602cbf5` tests: harness library and the first three CLI test files in Nazm
- `e30438a4` mutation-tests: mutation runner core in Nazm; CLI ports under mutation
- `0f33f377` tests: twelve more CLI test files in Nazm
- `138ce18b` tests: nineteen more CLI test files in Nazm
- `62667453` tests: debugging, profiling, provenance and FFI v2 tests in Nazm
- `f65dfff8` docs: record the nzx host runner as built and its decisions
- `6e3df6f6` docs: record the ported CLI suite results and classification

### 12. Cut over to the Nazm-written toolchain

Switch from the Rust implementation to the one written in Nazm, once the two agree: self-build, bootstrap fixpoint, release archives.

*The lesson:* The self-hosting moment: the compiler builds itself, and the result is identical.

113 commits, 2026-10-20 to 2026-11-05. To start with:

- `0ce905b0` docs: plan the cut-over to the Nazm-written toolchain in twenty slices
- `ab180923` interpreter: move the interpreter and checker onto the chain list type
- `37cc8de9` docs: record the cut-over questions settled by the copyright holder
- `7ce990f5` cli: write runtime build and verify and the static archive in Nazm
- `03631368` interpreter: compare native stack overflow by code, output and status
- `a23d785b` lsp: add signature help in Nazm
- `2c85fa8c` lsp: keep recovered parser spans for completion
- `6b983fe7` repo: merge the regenerated documentation data

### 13. Write the native code generator in Nazm

A code generator in Nazm that produces machine code directly, replacing the Rust-written one.

*The lesson:* Own the last stage, and the dependency on an outside code generator can end.

32 commits, 2026-10-15 to 2026-11-05. To start with:

- `837f0483` docs: propose Nazm's own code generator from LIR to objects
- `44ab4564` codegen: LIR to aarch64 Mach-O objects, fib(35) agrees with LLVM
- `42976b3e` codegen: failure bit, runtime, every integer width, floats and Int128
- `8f409a15` tests: check the encoders against the system assembler
- `c10c7243` codegen: use allocated registers in place; add a caller-saved pool
- `4fedbeef` cli: build --backend nazm runs Nazm's own code generator
- `0e4ab7f6` codegen: write Windows x86_64 PE/COFF objects from Nazm
- `ff5aefa6` repo: merge main into the ELF branch

### 14. Release engineering (versions, archives, provenance, site)

Versioning, release archives, checksums, provenance, the website and the package-manager formula.

*The lesson:* Shipping is a step of its own, with its own checks.

31 commits, 2026-10-03 to 2026-11-08. To start with:

- `748d17d5` release: copy the conformance corpus directories in the release script
- `ecd16cd0` release: record the core release candidate
- `aae6f025` release: settle the public version and the specification's status
- `d060cb7f` release: add prebuilt host archives and clean installs; Windows rules
- `fa162331` release: mark the 0.9.0 preview tasks done
- `bfeab8c4` release: 0.10.0 release note counts as the runs gave them
- `e09ece0b` build: write static archives deterministically for byte-identical builds
- `e36c4da5` build: harden release builds for either macOS architecture on macOS

The plan for shipping 0.11.0, by phase, is in [`release-0-11-0-plan.md`](release-0-11-0-plan.md).

## Old labels

Older notes, tasks and `Was:` lines use working labels. They map to steps like this:

| Old label | Step |
|---|---|
| N1 to N100 (numbered milestones) | 2 and 3 |
| Ga, M, Q, H, C, D (gate and domain notes) | 3 |
| R1 | 4 |
| R1.5 | 5 |
| R2 | 6 |
| R3 | 7 |
| R4 | 8 |
| R5 | 9 |
| R6 | 10 |
| R7 | 11 |
| R8, slice N | 12 |
| R9 | 13 |
| 0.9.0, 0.10.0, 0.11.0 release work | 14 |
