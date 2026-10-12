//! The command line's grammar: every command, its arguments and their help, as clap reads them.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// What `nazm runtime` does (N82).
#[derive(clap::Subcommand, Debug)]
pub(crate) enum RuntimeAction {
    /// Compile the whole runtime for one target and profile into `OUT/libnazmrt.a`, with its
    /// manifest `OUT/nazm-runtime.json`.
    Build {
        /// The target; the host's when absent.
        #[arg(long, value_name = "TRIPLE")]
        target: Option<String>,
        /// `hosted`, `board` or `wasm`: the target's own, which is the default.
        #[arg(long)]
        profile: Option<String>,
        /// Optimisation level handed to `clang`.
        #[arg(short = 'O', long = "opt-level", default_value_t = 2)]
        opt_level: u8,
        /// The directory to write the archive and manifest into.
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Check that the artifact in DIR is whole and fits this compiler and the target.
    Verify {
        dir: PathBuf,
        /// The target; the host's when absent.
        #[arg(long, value_name = "TRIPLE")]
        target: Option<String>,
    },
}

#[derive(Parser, Debug)]
#[command(
    name = "nazm",
    about = "The Nazm programming language.",
    long_about = "Nazm language 1.0 draft, toolchain 0.9.0 — a public preview for macOS on \
                  Apple Silicon, with no compatibility promise until 1.0. Records, \
                  enums and `match`, first-order generics, traits and methods, `Result`, \
                  `Option` and `?`, closures, modules and packages, declared effects, \
                  capabilities and provenance, structured concurrency (scope, spawn, \
                  channels), integers of every width, floats, fixed arrays, constants, \
                  `Bytes` and UTF-8 `Text`. `nazm run` interprets a program; `nazm build` \
                  compiles it natively through LLVM or Cranelift, and the two are held to \
                  the same results. Trait objects and effect handlers are not in the \
                  language; a program using an absent construct is refused by name \
                  rather than misparsed. Run `nazm capabilities` for this build's own \
                  answer. See docs/spec.md and docs/stability.md.",
    version
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// The operations of `nazm attest` (N94).
#[derive(Subcommand, Debug)]
pub(crate) enum AttestOperation {
    /// Wrap a provenance record in an in-toto Statement v1 over the ARTEFACTs — each checked to
    /// be an output the record names — and sign it with `ssh-keygen -Y sign`, writing `FILE.sig`.
    Sign {
        /// The private key, as `ssh-keygen -f` takes it.
        #[arg(long)]
        key: PathBuf,
        /// Where the statement goes: `PROVENANCE.intoto.json` by default.
        #[arg(long)]
        out: Option<PathBuf>,
        provenance: PathBuf,
        #[arg(required = true)]
        artefacts: Vec<PathBuf>,
    },
    /// Verify a signed statement for IDENTITY against an allowed-signers file, and that every
    /// ARTEFACT's digest is one of its subjects.
    Verify {
        #[arg(long)]
        allowed_signers: PathBuf,
        #[arg(long)]
        identity: String,
        statement: PathBuf,
        #[arg(required = true)]
        artefacts: Vec<PathBuf>,
    },
}

/// The operations `nazm patch` plans.
#[derive(Subcommand, Debug)]
pub(crate) enum PatchOperation {
    /// Rename the entity declared or used at `--byte` of `--file`, by the language
    /// service's validated rename: locals and private definitions, and — within a declared
    /// workspace (N89) — exported ones, across every module under the source root.
    Rename {
        /// The compilation's root module.
        #[arg(long)]
        root: PathBuf,
        /// The file the position is in. It must be one the root's compilation loads.
        #[arg(long)]
        file: PathBuf,
        /// The position, as a byte offset into `--file`.
        #[arg(long)]
        byte: u32,
        /// The new name: one identifier.
        #[arg(long)]
        new_name: String,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// Measure every module's durable identity from this directory, as
        /// `nazm check --source-root` does.
        #[arg(long)]
        source_root: Option<PathBuf>,
    },
    /// Apply, as a plan, fix `--fix` of those the current diagnostics at `--byte` of
    /// `--file` carry, in the order the compiler attached them.
    Fix {
        /// The compilation's root module.
        #[arg(long)]
        root: PathBuf,
        /// The file the position is in. It must be one the root's compilation loads.
        #[arg(long)]
        file: PathBuf,
        /// The position, as a byte offset into `--file`: a diagnostic's primary span there.
        #[arg(long)]
        byte: u32,
        /// Which of the fixes at that position.
        #[arg(long, default_value_t = 0)]
        fix: usize,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// Measure every module's durable identity from this directory, as
        /// `nazm check --source-root` does.
        #[arg(long)]
        source_root: Option<PathBuf>,
    },
    /// Apply a `nazm.patch/1` plan to disk, all or nothing (N89): every file must hash to the
    /// plan's digest and hold each edit's expected bytes, or nothing is written and the
    /// differing file is named. Each file is written through a temporary file and a rename.
    Apply {
        /// The plan, as `nazm patch rename|fix --json` printed it.
        plan: PathBuf,
        /// The directory the plan's paths are relative to: the source root it was planned
        /// in. The current directory by default.
        #[arg(long)]
        base: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
// Parsed once per process and matched at once: boxing a command's arguments would buy nothing.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    /// Type-check and evaluate a program, printing the value of `main`.
    Run {
        file: PathBuf,
        /// Arguments for the program itself, after `--`.
        ///
        /// What `arg(i)` sees, **excluding** the program's own name — so `arg(0)` means
        /// the same thing here and in a compiled executable, where `argv[0]` is a path
        /// nobody should depend on.
        #[arg(last = true)]
        args: Vec<String>,
        /// Emit diagnostics as one JSON object per line.
        ///
        /// The machine-readable form is the contract; the rendered text is a view of it.
        #[arg(long)]
        json: bool,
        /// Stop after this many loop iterations.
        ///
        /// An interpreter guard, not a language rule: with no `break` and no I/O a
        /// non-terminating program produces nothing, and a tool that hangs is worse than
        /// one that reports.
        #[arg(long, default_value_t = nazm_core::eval::DEFAULT_MAX_ITERATIONS)]
        max_iterations: u64,
        /// Stop after this many nested calls.
        ///
        /// The iteration budget does not bound recursion: `fn f(n: Int) -> Int { f(n) }`
        /// never loops. Without this the host stack runs out and the process dies with no
        /// diagnostic at all.
        ///
        /// Raising it raises the stack the interpreter reserves, so the diagnostic is
        /// still reached before the stack is. What it costs is stated below rather than
        /// left for someone to discover.
        #[arg(
            long,
            default_value_t = nazm_core::eval::DEFAULT_MAX_DEPTH,
            value_parser = max_depth,
        )]
        max_depth: usize,
        /// Refuse to build unless the package's `nazm.lock` records exactly the packages this
        /// resolves to, with their bytes unchanged (N46).
        #[arg(long)]
        locked: bool,
        /// A restriction profile to hold the program to (N47): `general`, `embedded`,
        /// `critical`, `cyber`, `realtime`, `web3`, `accounts`, `authority`, `kernel`, `robotics`,
        /// `automotive`, `aerospace`, `assurance`, `industrial` or `medical`. Added to any the
        /// packages' manifests require; a profile only refuses, and never changes what an
        /// accepted program does.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// The `@test`, `@fuzz` or `@bench` function to run through the generated entry, as `nazm
        /// test` and `nazm bench` ask (Gate 2, §18–§19): `test:NAME`, `fuzz:NAME:CASES` or
        /// `bench:NAME:ITERATIONS`. Not an interface of its own.
        #[arg(long, hide = true, value_name = "SPEC")]
        harness: Option<String>,
        /// Features of the root package to enable beyond its `default`, comma-separated (Gate 2,
        /// §22); each package's `@cfg(feature = …)` sees its own.
        #[arg(long, value_name = "NAMES", value_delimiter = ',')]
        features: Vec<String>,
    },
    /// Type-check without evaluating.
    Check {
        file: PathBuf,
        #[arg(long)]
        json: bool,
        /// Where this project begins, for durable module identity.
        ///
        /// Normally discovered: the nearest `nazm.root` marker at or above the file, and
        /// otherwise the file's own directory. Naming it here is for a tree that cannot
        /// be written to, and for a caller that would rather state it than rely on a
        /// search.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
        /// Check every module, reusing nothing.
        ///
        /// The cache is derived data and deleting it changes no program's meaning, so
        /// this exists to **establish** that rather than to work around it: the
        /// equivalence tests compare a run with this flag against one without, and
        /// require the same answer.
        #[arg(long)]
        no_cache: bool,
        /// One `nazm.check-report/1` object on stdout: what was checked, and what was
        /// reused. Off by default, because a compiler that reports on itself every time
        /// is a compiler whose output has to be filtered.
        #[arg(long)]
        cache_report: bool,
        /// One `nazm.command-summary/1` object on stdout, and nothing else on either stream:
        /// the outcome, and a reference to each diagnostic — no prose. The exit status is
        /// unchanged. `--json` still gives every diagnostic in full.
        #[arg(long, conflicts_with_all = ["json", "cache_report"])]
        summary_json: bool,
        /// Refuse to build unless the package's `nazm.lock` records exactly the packages this
        /// resolves to, with their bytes unchanged (N46).
        #[arg(long)]
        locked: bool,
        /// A restriction profile to hold the program to (N47): `general`, `embedded`,
        /// `critical`, `cyber`, `realtime`, `web3`, `accounts`, `authority`, `kernel`, `robotics`,
        /// `automotive`, `aerospace`, `assurance`, `industrial` or `medical`. Added to any the
        /// packages' manifests require; a profile only refuses, and never changes what an
        /// accepted program does.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Features of the root package to enable beyond its `default`, comma-separated (Gate 2,
        /// §22); each package's `@cfg(feature = …)` sees its own.
        #[arg(long, value_name = "NAMES", value_delimiter = ',')]
        features: Vec<String>,
        /// One `nazm.profile-report/1` object on stdout: every rule of the profiles in force,
        /// its verdict and its witnesses.
        #[arg(long)]
        profile_report: bool,
        /// Refuse a program the checker warns of (R1b, L15), as though each warning were an
        /// error: for a build that must stay free of them.
        #[arg(long)]
        deny_warnings: bool,
    },
    /// Apply the repairs the compiler already knows about.
    ///
    /// Only fixes marked `automatic` are applied; one marked `needs_review` is printed
    /// with the condition under which it is right and left for a person. Nothing is
    /// written without `--apply`.
    Fix {
        file: PathBuf,
        /// Write the edits. Without this the command reports and changes nothing.
        #[arg(long)]
        apply: bool,
        /// One JSON object per edit, on stdout. See `docs/diagnostics.md`.
        #[arg(long)]
        json: bool,
    },
    /// Print one definition's semantic context packet, `nazm.context/3`.
    ///
    /// The function, record or enum declared or used at byte `--byte` of `--file`, a file
    /// the compilation rooted at `--root` loads: its exact source, and compact links to what
    /// it uses, what uses it, what it calls and what calls it, from the disk as it is now.
    /// `docs/architecture.md` §7.29.
    Context {
        /// The compilation's root module: every fact in the packet is of this program.
        #[arg(long)]
        root: PathBuf,
        /// The file the position is in. It must be one the root's compilation loads.
        #[arg(long)]
        file: PathBuf,
        /// The position, as a byte offset into `--file`.
        #[arg(long)]
        byte: u32,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// Measure every module's durable identity from this directory, as
        /// `nazm check --source-root` does.
        #[arg(long)]
        source_root: Option<PathBuf>,
    },
    /// Print the semantic snapshot of a root's compilation, `nazm.snapshot/3`.
    ///
    /// Every durable function, record and enum the compilation rooted at `--root` declares,
    /// each with one digest per semantic section the compiler records, from the disk as it
    /// is now. Keep it as the baseline of a later `nazm delta`. A digest is evidence the
    /// canonical data it was taken over is equal, not that behaviour is.
    /// `docs/architecture.md` §7.30.
    Snapshot {
        /// The compilation's root module.
        #[arg(long)]
        root: PathBuf,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// Measure every module's durable identity from this directory, as
        /// `nazm check --source-root` does.
        #[arg(long)]
        source_root: Option<PathBuf>,
    },
    /// Print what changed since a baseline snapshot, `nazm.delta/3`.
    ///
    /// Compares `--baseline`, a `nazm.snapshot/3` file of the same root, with the
    /// compilation rooted at `--root` as it is on disk now: durable definitions added,
    /// removed, and changed — naming exactly which recorded sections changed. Matching is
    /// by durable key only, so a rename is a removal and an addition. A baseline that is not
    /// exactly a snapshot of this root, by this compiler, is refused.
    Delta {
        /// The compilation's root module.
        #[arg(long)]
        root: PathBuf,
        /// A `nazm.snapshot/3` file, as `nazm snapshot --json` wrote it.
        #[arg(long)]
        baseline: PathBuf,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// Measure every module's durable identity from this directory, as
        /// `nazm check --source-root` does.
        #[arg(long)]
        source_root: Option<PathBuf>,
    },
    /// Print the root's current diagnostics compactly, `nazm.diagnostic-index/1`, or one
    /// of them in full, `nazm.diagnostic-detail/1`.
    ///
    /// Without `--detail`: every diagnostic's id, code, severity, place, owning definition
    /// and fix counts — no message, help or label text. With `--detail ID --state DIGEST`:
    /// that diagnostic exactly as `nazm.diagnostic/1` publishes it, with its places in their
    /// files and, for each fix, the `nazm patch fix` selector where one exists — refused if
    /// the sources are no longer the index's state. From the disk as it is now; `nazm check
    /// --json` is unchanged. `docs/architecture.md` §7.32.
    Diagnostics {
        /// The compilation's root module.
        #[arg(long)]
        root: PathBuf,
        /// A diagnostic's id, from an index.
        #[arg(long, requires = "state")]
        detail: Option<String>,
        /// The index's `state.source`: the sources the id is of.
        #[arg(long, requires = "detail")]
        state: Option<String>,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// Measure every module's durable identity from this directory, as
        /// `nazm check --source-root` does.
        #[arg(long)]
        source_root: Option<PathBuf>,
    },
    /// Retrieve Nazm's own documentation by section: a compact index, `nazm.docs-index/1`,
    /// or one section's exact canonical text, `nazm.docs-section/1`.
    ///
    /// The index lists every document of the corpus with its authority — normative, goal,
    /// planning, evidence and the rest — and every section's stable id, title, parent and
    /// digest, with no body. `--section ID` returns that section's own text byte for byte,
    /// never its subsections', and never a paraphrase; `--state` binds the request to the
    /// corpus an index described. Read from the repository's documentation as it is now;
    /// ids are exact, with no search. `docs/architecture.md` §7.34.
    #[command(group(clap::ArgGroup::new("ask").required(true).args(["index", "section"])))]
    Docs {
        /// Print the index.
        #[arg(long)]
        index: bool,
        /// Only this document's sections, by its id.
        #[arg(long, conflicts_with = "section")]
        document: Option<String>,
        /// Print one section, by its exact id.
        #[arg(long)]
        section: Option<String>,
        /// The index's `state`: refuse the section if the corpus has changed since.
        #[arg(long, conflicts_with = "index")]
        state: Option<String>,
        /// The one canonical compact serialisation. Without it, a section's text or the
        /// index's ids and titles.
        #[arg(long)]
        json: bool,
        /// The repository whose `docs/` to read. Without it, the one this binary was built
        /// from.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Print the repository context map, `nazm.repository-map/1`, or the minimum context
    /// for a task, `nazm.task-context/1`.
    ///
    /// `--map`: every package, target, source root, compilation root, module, durable
    /// definition, document, schema and mutation profile, with its authority, its structural
    /// relationships and where to retrieve more — never a body. `--task`: for the seeds (map
    /// ids) and the intent, the smallest context the repository can justify — each item with
    /// its reason, authority and retrieval, the seed's exact source and the selected
    /// sections' exact text — within a structural budget; a budget the seeds do not fit is
    /// refused, and anything else it cuts is listed and makes the answer `partial`. `--state`
    /// refuses a repository that has changed since. Ids only; no path is read.
    /// `docs/architecture.md` §7.35.
    #[command(group(clap::ArgGroup::new("repo_ask").required(true).args(["map", "task"])))]
    Repo {
        /// Print the map.
        #[arg(long)]
        map: bool,
        /// Print the context for a task.
        #[arg(long)]
        task: bool,
        /// `understand`, `edit`, `diagnose` or `test`. Without it, `edit`, the widest.
        #[arg(long, conflicts_with = "map", value_parser = ["understand", "edit", "diagnose", "test"])]
        intent: Option<String>,
        /// A repository entity id, from a map or an earlier context. Repeatable.
        #[arg(long, conflicts_with = "map")]
        seed: Vec<String>,
        /// A map's `state`: refuse the task if the repository has changed since.
        #[arg(long, conflicts_with = "map")]
        state: Option<String>,
        /// At most this many items.
        #[arg(long, conflicts_with = "map")]
        max_entities: Option<usize>,
        /// At most this many bytes of definition source.
        #[arg(long, conflicts_with = "map")]
        max_source_bytes: Option<usize>,
        /// At most this many bytes of section text.
        #[arg(long, conflicts_with = "map")]
        max_doc_bytes: Option<usize>,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// The repository. Without it, the one this binary was built from.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Print a semantic patch plan, `nazm.patch/1`. Plans; never applies.
    ///
    /// A validated rename (`rename`) or a compiler-owned fix (`fix`), as minimal exact-byte
    /// edits bound to the root's semantic snapshot, each edited file's digest and each
    /// edit's expected bytes, from the disk as it is now. Nothing is written: a patch is a
    /// proposal bound to exact source state, not permission to change a file.
    /// `docs/architecture.md` §7.31.
    Patch {
        #[command(subcommand)]
        operation: PatchOperation,
    },
    /// Serve the Language Server Protocol to an editor, on stdin and stdout.
    ///
    /// Started and owned by the editor; ends when it says `exit`. Diagnostics, definition
    /// and hover, from the same loader and checker every other command uses, over the
    /// editor's unsaved buffers. `docs/architecture.md` §7.18 says what it does and does
    /// not do.
    Lsp {
        /// Measure every module's durable identity from this directory, as
        /// `nazm check --source-root` does. Without it, each open file's root is decided
        /// exactly as `nazm check` decides it for that file.
        #[arg(long)]
        source_root: Option<PathBuf>,
    },
    /// Lay out source files canonically.
    ///
    /// One layout and no options: a formatter with settings has as many canonical forms
    /// as it has settings. Per file, because layout is a property of a file and not of
    /// the program a `use` assembles.
    Fmt {
        /// The files to format. A directory is not walked — name the files, or let a
        /// shell glob do it, so nothing is rewritten that was not asked for.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Write nothing; list the files that would change and fail if there are any.
        #[arg(long)]
        check: bool,
    },
    /// Run a corpus of programs against their recorded outputs.
    ///
    /// A case is `NAME.nz` beside `NAME.expected`, the convention
    /// `compiler/bootstrap.sh` has used since the bootstrap existed. Each case runs
    /// interpreted **and** compiled, and both must produce the recorded output.
    Test {
        /// Files or directories. A directory is scanned for `.nz` files, and any
        /// without a `.expected` beside them are reported rather than ignored.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// One JSON object per case, on stdout. See `docs/diagnostics.md`.
        #[arg(long)]
        json: bool,
        /// Skip the native leg. For a machine with no `clang`.
        #[arg(long)]
        interpret_only: bool,
        /// One `nazm.test-summary/1` object on stdout, and nothing else on either stream:
        /// counts, and every case that did not pass — no passed names, no output. The exit
        /// status is unchanged. `--json` still gives every case in full.
        #[arg(long, conflicts_with = "json")]
        summary_json: bool,
        /// Run only the cases whose name contains this: `STEM` for a recorded output,
        /// `STEM::FUNCTION` for a `@test` or `@fuzz` function (Gate 2, §18).
        #[arg(long, value_name = "TEXT")]
        filter: Option<String>,
        /// How many generated inputs each `@fuzz` function is given, the same ones every run.
        #[arg(long, value_name = "N", default_value_t = crate::test::DEFAULT_FUZZ_CASES)]
        fuzz: u32,
    },
    /// Measure every `@bench` function of the files named (Gate 2, §19): each built natively at
    /// `-O2` and called `--iterations` times between two readings of the monotonic clock.
    Bench {
        /// Files or directories; a directory is scanned for `.nz` files.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// How many calls each measurement times together.
        #[arg(long, value_name = "N", default_value_t = 1000)]
        iterations: u32,
        /// One `nazm.bench/1` object per function, on stdout.
        #[arg(long)]
        json: bool,
    },
    /// An interactive session (N65): definitions and expressions a line at a time on standard
    /// input, checked as one program on every edit and evaluated by the interpreter. A
    /// redefinition that breaks another definition is refused by that one's name. `:defs`,
    /// `:reset`, `:quit`.
    Repl,
    /// Evaluate a function at compile time (N65): one of no parameters, declared or inferred pure,
    /// under the interpreter's budgets — so it holds no capability and reaches nothing of the host.
    Comptime {
        file: PathBuf,
        /// The function to evaluate, in the file's own module.
        function: String,
        /// As `nazm run --max-iterations`: a runaway evaluation stops with `N0402`.
        #[arg(long, default_value_t = nazm_core::eval::DEFAULT_MAX_ITERATIONS)]
        max_iterations: u64,
        /// As `nazm run --max-depth`.
        #[arg(long, default_value_t = nazm_core::eval::DEFAULT_MAX_DEPTH, value_parser = max_depth)]
        max_depth: usize,
    },
    /// A contract (N69): its facts as `nazm.contract/1` (`--interface`, the default), or a
    /// deterministic run of the transactions in `--txs` — `CALLER ENTRY ARG…` per line — as
    /// `nazm.contract-run/1`. The module is held to the `web3` profile first.
    Contract {
        file: PathBuf,
        /// Print the contract's facts (the default when `--txs` is not given).
        #[arg(long)]
        interface: bool,
        /// Run these transactions, one per line, against a fresh `init()` state.
        #[arg(long, value_name = "FILE")]
        txs: Option<PathBuf>,
        /// Compile to EVM bytecode (N70): write `deploy.hex`, `runtime.hex`, `abi.json`,
        /// `storage.json` and the py-evm harness `evm_run.py` into DIR.
        #[arg(long, value_name = "DIR")]
        evm: Option<PathBuf>,
        /// Whether this contract's storage can take over OLD's (N70): `nazm.evm-upgrade/1`.
        #[arg(long, value_name = "OLD")]
        upgrade_check: Option<PathBuf>,
        /// Build for WebAssembly with the contract adapter (N71): the objects, `link.txt`,
        /// `contract.json` and the reference host `contract_host.mjs` into DIR.
        #[arg(long, value_name = "DIR")]
        wasm: Option<PathBuf>,
        /// The account-oriented metadata (N72): each instruction's discriminator, arguments and
        /// accounts with their signer and writable constraints, as `DIR/accounts.json`. The module
        /// is held to the `accounts` profile.
        #[arg(long, value_name = "DIR")]
        accounts: Option<PathBuf>,
    },
    /// Map a kernel over a sequence on an accelerator (N67): an eligible pure `(T) -> T` function
    /// of the file — `T` an `Int`, or (Gate 3) a `Float32` or `Float64` — over the
    /// whitespace-separated numbers of `--input`, on macOS's OpenCL GPU or the CPU provider. One
    /// `nazm.accel/3` line: the provider and why, the device, every transfer and the
    /// synchronisation, timings, the kernel's identity, and whether the results agree with the
    /// interpreter's.
    Accel {
        file: PathBuf,
        /// The function to run as a kernel, in the file's own module.
        kernel: String,
        /// The input: whitespace-separated numbers of the kernel's type, at most 8,000,000. Twice
        /// for a kernel of two, a zip (N88): the two inputs are of one length, element `i` of each
        /// its arguments.
        #[arg(long, value_name = "FILE", required = true)]
        input: Vec<PathBuf>,
        /// Fold the kernel's results into one `Int`, in index order (N88): `add`, checked as
        /// `ints_sum` is; `min`; `max`. On the host, after the transfer, timed in the report.
        #[arg(long, value_name = "OP", value_parser = ["add", "min", "max"])]
        reduce: Option<String>,
        /// Do not hold the results to the interpreter's.
        #[arg(long)]
        no_check: bool,
        /// Print the generated OpenCL C kernel and stop: no device is used, on any host.
        #[arg(long)]
        print_kernel: bool,
        /// Where it runs (Gate 3): `opencl`, the GPU, refused where there is none; `cpu`, the
        /// same kernel compiled as C and run on a pool of threads; `auto`, the GPU where one can
        /// run it and the CPU otherwise, the report saying which and why.
        #[arg(long, value_name = "PROVIDER", value_parser = ["auto", "opencl", "cpu"], default_value = "auto")]
        provider: String,
        /// Compile the CPU provider's loop without LLVM's vectoriser (Gate 3): the scalar side of
        /// a SIMD measurement.
        #[arg(long)]
        scalar: bool,
    },
    /// Whether a running image could take a new version of a program (N65): one
    /// `nazm.reload-check/1` line naming each change as `reloadable` or `restart`, and why. Exits
    /// non-zero when any change needs a restart.
    ReloadCheck { old: PathBuf, new: PathBuf },
    /// Print the compilation's resolved units (N77): one `nazm.resolved/1` line per module
    /// with a durable identity — its imports, definitions and every resolved occurrence, by
    /// durable identity. Runs the cached check; a reused module's unit is its entry's.
    Resolve {
        file: PathBuf,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Every occurrence of one entity in the compilation (N77), from its modules' resolved
    /// units: `nazm.references/1`. `DEF` is a durable key, `module::kind name`, with
    /// `.member` for a variant, field or payload field (`Variant.field`). A module whose
    /// check is reused is not checked again; the output says how many were.
    References {
        def: String,
        file: PathBuf,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Write a module's published interface: what it offers, durably.
    ///
    /// The file named is the root, so the source root is its directory. Output is one
    /// `nazm.interface/1` JSON object on stdout — the same bytes every time, for the same
    /// interface, in this process and the next one. This is not a cache and reuses
    /// nothing; see `docs/architecture.md` §7.3.
    Interface {
        file: PathBuf,
        /// Print the fingerprint instead of the interface: `blake3-256 <hex>`.
        #[arg(long)]
        hash: bool,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Print a checked program's Core IR, `nazm.core-ir/2`: what each function became,
    /// typed, with every name resolved and every transfer explicit.
    ///
    /// A debugging view of the one representation `nazm run` and `nazm build` both start
    /// from, built only for a program that checks. Deterministic, and naming every
    /// definition and type by its durable key. Not user-facing syntax, and its version
    /// promises nothing to a reader outside this build; `docs/architecture.md` §7.41.
    CoreIr {
        file: PathBuf,
        /// The one canonical compact serialisation. Without it, the same object indented.
        #[arg(long)]
        json: bool,
        /// Leave out every node's source span, so two programs that differ only in where
        /// their text sits print the same.
        #[arg(long)]
        no_spans: bool,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Trace requirements to what verifies them, `nazm.trace/1` (Gate 3): each requirement of a
    /// `requirements.toml` to the definitions it constrains — by durable identity and digest, with
    /// their obligations — and to the `@test` functions it names, run. A definition renamed away, a
    /// test missing or failing is reported by name and fails the command. Not certification.
    Trace {
        file: PathBuf,
        /// The requirements: `schema = "nazm.requirements/1"`, and each `[[requirement]]` with its
        /// `id`, `text`, `definitions` and `tests`.
        #[arg(long, value_name = "FILE")]
        requirements: PathBuf,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Print every obligation of a checked program, `nazm.obligations/1` (N87): each contract
    /// clause, call of a function with a precondition, operation that can overflow or divide
    /// by zero, index, allocation, call through a function value and call into C, each proved,
    /// checked as the program runs, or unknown. Compiler evidence, not certification.
    Obligations {
        file: PathBuf,
        /// `nazm.obligations/1` on one line, rather than a line per obligation.
        #[arg(long)]
        json: bool,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Print each concrete function of the native program as MIR, `nazm.mir/1` (N40): basic
    /// blocks with every copy, move, drop, scope join and failure edge written out, after the
    /// validator accepted it. Debug output, internal and unstable; `docs/architecture.md` §7.42.
    Mir {
        file: PathBuf,
        /// One line rather than indented.
        #[arg(long)]
        json: bool,
        /// Name each statement's source position.
        #[arg(long)]
        spans: bool,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Build or check a runtime artifact (N82, `docs/architecture.md` §7.83): the whole runtime
    /// for one target and profile, compiled once into `libnazmrt.a` beside a `nazm.runtime/1`
    /// manifest, which `nazm build --runtime DIR` links.
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
    /// Print what LIR decided for a checked program, `nazm.lir/2` (N81, N105): the target's data
    /// layout, every unit's symbols and linkage, every record's and enum's byte layout on that
    /// target, which types own anything, and every unit's instructions — what both backends
    /// read, validated. `docs/architecture.md` §7.82 and §7.106.
    Lir {
        file: PathBuf,
        /// The target whose layout to print; the host's when absent.
        #[arg(long, value_name = "TRIPLE")]
        target: Option<String>,
        /// Print each unit's instructions as text (N105), validated, instead of the tables.
        #[arg(long)]
        ops: bool,
        /// With `--ops`: lay out every eligible `Vec[R]` field by field (N68).
        #[arg(long)]
        soa: bool,
        /// Run the program on LIR's interpreter, the oracle the native backends are held to
        /// (N105): its output, and the status a native build would end with.
        #[arg(long)]
        run: bool,
        /// With `--run`: the program's arguments.
        #[arg(last = true)]
        args: Vec<String>,
        /// Where this project begins. As `nazm check --source-root`.
        #[arg(long, value_name = "DIR")]
        source_root: Option<PathBuf>,
    },
    /// Document a program's public API (N74), from its semantic definitions: every exported
    /// function, record and enum of its own modules, with effects, capabilities, foreign symbols and
    /// a link to the source. One `nazm.api-doc/2` line on stdout, or `api.json` and `api.md` in
    /// `-o DIR`. Not `nazm docs`, which reads Nazm's own documentation.
    Doc {
        /// A source file, or a package's directory.
        path: PathBuf,
        #[arg(short, long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Declare the supported subset of a C header as `extern "C"` functions (N74): prototypes of
    /// `int64_t` and `bool` (and `const char *` arguments). Everything else is refused with the
    /// reason, never widened. With `-o FILE`, the module is written there and one `nazm.bindgen/1`
    /// line is printed; exits 1 when anything was refused.
    Bindgen {
        header: PathBuf,
        #[arg(short, long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Create a project from a template (N74): `cli`, `library`, `server`, `embedded`, `wasm` or
    /// `contract`. The directory is the package's name; it must not exist or must be empty. Its
    /// README.md names the commands that check, test, build and document it.
    Init {
        dir: PathBuf,
        #[arg(long, default_value = "cli", value_name = "NAME")]
        template: String,
    },
    /// Report what this build of the compiler actually supports.
    ///
    /// Read from the compiler's own tables rather than from a written list, so it cannot
    /// describe a version of itself that does not exist.
    Capabilities {
        /// One JSON object, on stdout. See `docs/diagnostics.md`.
        #[arg(long)]
        json: bool,
    },
    /// Compile to a native executable.
    ///
    /// A narrower subset than `nazm run` — ask `nazm capabilities` for this build's own
    /// answer rather than a document. Anything outside it is
    /// refused by name after ordinary checking; nothing is partially compiled.
    Build {
        file: PathBuf,
        /// Where to write the executable. Defaults to the source's stem.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Emit diagnostics as one JSON object per line. Build-time only: see the module
        /// documentation for why a generated executable does not do this.
        #[arg(long)]
        json: bool,
        /// Also write the generated LLVM IR next to the executable, as `<out>.ll`.
        #[arg(long)]
        emit_ir: bool,
        /// Optimisation level handed to `clang`.
        ///
        /// Defaults to 0. The specified arithmetic semantics hold at every level by
        /// construction — overflow is an intrinsic and a branch, never `nsw` — and the
        /// test suite compiles the same programs at 0 and 2 to keep that true.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
        opt_level: u8,
        /// Compile every unit, reusing nothing.
        ///
        /// As `nazm check --no-cache`, and for the same reason: the equivalence tests
        /// compare a build with this flag against one without and require the same
        /// program. It disables the native object store; nothing in `nazm build` reads
        /// the semantic one.
        #[arg(long)]
        no_cache: bool,
        /// One `nazm.build-report/1` object on stdout: what was compiled, and what was
        /// reused.
        #[arg(long)]
        build_report: bool,
        /// One `nazm.command-summary/1` object on stdout, and nothing else on either stream:
        /// the outcome, and a reference to each diagnostic — no prose. The exit status is
        /// unchanged. `--json` still gives every diagnostic in full.
        #[arg(long, conflicts_with_all = ["json", "build_report"])]
        summary_json: bool,
        /// The code generator: `llvm` (the default: LLVM IR text compiled by clang) or
        /// `cranelift` (in process). Neither falls back to the other; a program one cannot
        /// compile is refused by name.
        #[arg(long, default_value = "llvm", value_parser = ["llvm", "cranelift"])]
        backend: String,
        /// The target triple: one of the nine `nazm capabilities` lists (N57). A target this
        /// host links is built to an executable; any other needs `--objects`. An unknown
        /// triple is refused rather than silently built for this machine.
        #[arg(long, value_name = "TRIPLE")]
        target: Option<String>,
        /// An object file or static library that defines foreign (`extern "C"`) functions
        /// the program calls, handed to the linker after the program's own objects. It may
        /// be given more than once; it must exist. Nazm does not compile C.
        #[arg(long = "link", value_name = "FILE")]
        link: Vec<PathBuf>,
        /// Build a static library instead of an executable (N55): no `main` is needed, `-o`
        /// names the archive, and a C header declaring every `extern "C"` export is written
        /// beside it with the extension `.h`.
        #[arg(long)]
        lib: bool,
        /// With `--lib`, a shared library (`.dylib`, `.so`) instead of a static archive (N85):
        /// its only visible symbols are the exports, and the header is the same.
        #[arg(long, requires = "lib")]
        shared: bool,
        /// Write every object and `DIR/link.txt` instead of linking (N57): for a `--target` this
        /// host has no linker for, linked by that target's own toolchain.
        #[arg(long, value_name = "DIR")]
        objects: Option<PathBuf>,
        /// Paint the board's stack in `_start` and write `nazm-stack: used=N` to its UART after
        /// `main` (N63): the deepest byte the run touched, beside `bounds.json`'s static bound.
        /// A board's target only: `aarch64-unknown-none`, `riscv64gc-unknown-none-elf` or
        /// `thumbv7m-none-eabi`.
        #[arg(long)]
        stack_watermark: bool,
        /// The stack `main` and every task run on (R1b, L14), in bytes or with a `K`, `M` or `G`
        /// suffix (KiB, MiB, GiB): `--stack-size 64M` for a program that recurses deeply. Hosted
        /// targets only. Without it, `main` has the process's stack, a task 8 MiB on its own
        /// thread or 256 KiB in the pool, and `NAZM_TASK_STACK` still overrides the pool's.
        #[arg(long, value_name = "BYTES", value_parser = stack_size)]
        stack_size: Option<u64>,
        /// Refuse a program the checker warns of (R1b, L15), as `nazm check --deny-warnings`.
        #[arg(long)]
        deny_warnings: bool,
        /// A board's manifest (Gate 3, `docs/spec.md`, *Boards*): load address, stack, UART, how
        /// the machine stops, a heap. A board's target only; without it, the target's built-in
        /// board, which `nazm inspect` prints as a manifest.
        #[arg(long, value_name = "FILE")]
        board: Option<PathBuf>,
        /// How a `Vec` of records is laid out (N68): `aos`, each element whole, or `soa`, one
        /// array per field for every record of `Int`s and `Bool`s. Unobservable to the program;
        /// part of every object's identity. LLVM backend only.
        #[arg(long, value_name = "LAYOUT", default_value = "aos")]
        layout: String,
        /// One `nazm.timings/1` line on stderr: each phase's wall time — load, front end, MIR,
        /// LIR, code generation, native compilation, link — and the total (N48).
        #[arg(long)]
        timings: bool,
        /// Carry DWARF debug information (N48): a subprogram for each function and a source
        /// position on each instruction, so a debugger stops in Nazm functions and shows their
        /// lines. LLVM backend only; the same program, with more metadata.
        #[arg(long)]
        debug: bool,
        /// Link the runtime artifact in DIR, built by `nazm runtime build`, instead of generating
        /// the runtime (N82): refused, by name and before linking, when it does not fit this
        /// compiler, the target or what the program reaches.
        #[arg(long, value_name = "DIR")]
        runtime: Option<PathBuf>,
        /// What the build is of (Gate 2): a marked function, and the package's features.
        #[command(flatten)]
        selection: Box<Selection>,
        /// Refuse to build unless the package's `nazm.lock` records exactly the packages this
        /// resolves to, with their bytes unchanged (N46).
        #[arg(long)]
        locked: bool,
        /// A restriction profile to hold the program to (N47): `general`, `embedded`,
        /// `critical`, `cyber`, `realtime`, `web3`, `accounts`, `authority`, `kernel`, `robotics`,
        /// `automotive`, `aerospace`, `assurance`, `industrial` or `medical`. Added to any the
        /// packages' manifests require; a profile only refuses, and never changes what an
        /// accepted program does.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// After a successful build, write `nazm.provenance/1` to FILE (N73): every source module
        /// and package with its BLAKE3 digest, the target, the compiler and C compiler, the
        /// runtime's ABI revision and digest, the backend and its options, the profiles and the
        /// output's digest. Deterministic: no path, time or host name.
        #[arg(long, value_name = "FILE")]
        provenance: Option<PathBuf>,
        /// Run CMD (its words, no shell) with the provenance file's path appended, and keep its
        /// standard output as `FILE.sig` — a hook for any local signer. Needs `--provenance`.
        #[arg(long, value_name = "CMD", requires = "provenance")]
        attest_with: Option<String>,
        /// After a successful build, write a CycloneDX 1.5 SBOM to FILE (N94): the output, every
        /// package, source module and the runtime with their BLAKE3 hashes, and the tools.
        #[arg(long, value_name = "FILE")]
        sbom: Option<PathBuf>,
    },
    /// Print one program's facts as `nazm.inspect/2` (N48; `/2` since N104): every definition's identity,
    /// signature, effects, capabilities, provenance, Core IR and MIR digests, and the toolchain,
    /// package and lock identities — one deterministic line of JSON.
    Inspect {
        file: PathBuf,
        /// Only the function of this name.
        #[arg(long, value_name = "NAME")]
        def: Option<String>,
    },
    /// Build the program and run it once with its memory and scheduler counters on (N58), and
    /// print `nazm.profile/2`: its exit status, wall time and the counts; with `--sample`, also
    /// where its samples fell (N91).
    Profile {
        file: PathBuf,
        /// The code generator, as `nazm build --backend`.
        #[arg(long, default_value = "llvm", value_parser = ["llvm", "cranelift"])]
        backend: String,
        /// Sample the run with the host's sampler, attributing each sample to a Nazm function
        /// and line, the runtime, foreign code or the system (N91).
        #[arg(long)]
        sample: bool,
        /// With `--sample`, the time between samples.
        #[arg(long, value_name = "MS", default_value_t = 1, requires = "sample")]
        interval_ms: u64,
    },
    /// Print where each function allocates, retains, blocks, talks to a channel, calls C or
    /// starts a task (N52), read off MIR, each with its count per call — `at-most-once` or
    /// `unknown`, never a guess. `--json` prints `nazm.cost/2`, which since N106 also says which
    /// scheduler a native build runs the program's tasks on, and why.
    ExplainCost {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Print every sink — a path written to, standard output, a file's data — and every
    /// declassification, with the origins that reach it (N52). `--json` prints `nazm.flow/2`.
    ExplainFlow {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Sign and verify in-toto statements of a build's provenance (N94).
    Attest {
        #[command(subcommand)]
        operation: AttestOperation,
    },
    /// Resolve a package and write its `nazm.lock` (N46): the exact package versions, sources
    /// and digests a `--locked` build is held to. Written only when its bytes would change.
    Lock {
        /// The package's directory, holding `nazm.toml`.
        dir: PathBuf,
    },
    /// Re-resolve a package with its lockfile's registry versions no longer preferred — every
    /// one's, or only the named packages' — write `nazm.lock`, and print each change (N90).
    Update {
        /// The package's directory, holding `nazm.toml`.
        dir: PathBuf,
        /// The registry packages to update; none means all.
        names: Vec<String>,
    },
    /// Publish a package into a local registry (N56): copy its manifest and sources and record
    /// the version and its digest. A published version never changes.
    Publish {
        /// The package's directory, holding `nazm.toml`.
        dir: PathBuf,
        /// The registry's directory.
        #[arg(long, value_name = "DIR")]
        registry: PathBuf,
        /// Make every check a publish makes and print what it would record — one
        /// `nazm.publish-plan/1` line: the name, version, digest and files — writing nothing (N74).
        #[arg(long)]
        dry_run: bool,
    },
    /// Withdraw a published version (N56): never chosen anew, still used where a lockfile names it.
    Yank {
        /// `NAME@VERSION`.
        package: String,
        /// The registry's directory.
        #[arg(long, value_name = "DIR")]
        registry: PathBuf,
        /// Restore a yanked version instead.
        #[arg(long)]
        undo: bool,
    },
}

/// Parse `--stack-size` (R1b, L14): bytes, or a number with a `K`, `M` or `G` suffix, from
/// 64 KiB to 4 GiB.
fn stack_size(raw: &str) -> Result<u64, String> {
    let (digits, unit) = match raw.char_indices().last() {
        Some((i, 'K' | 'k')) => (&raw[..i], 1u64 << 10),
        Some((i, 'M' | 'm')) => (&raw[..i], 1u64 << 20),
        Some((i, 'G' | 'g')) => (&raw[..i], 1u64 << 30),
        _ => (raw, 1),
    };
    let n: u64 = digits
        .parse()
        .map_err(|_| format!("`{raw}` is not a size: bytes, or a number with `K`, `M` or `G`"))?;
    let bytes = n
        .checked_mul(unit)
        .ok_or_else(|| format!("`{raw}` is too large"))?;
    if !((64 << 10)..=(4 << 30)).contains(&bytes) {
        return Err(format!(
            "`{raw}` is {bytes} bytes; a stack is from 64K to 4G"
        ));
    }
    Ok(bytes)
}

/// Parse `--max-depth`, refusing a budget no machine would grant the stack for.
///
/// The number is not a safety boundary — the reservation grows with it — so this refuses
/// only where the reservation itself stops being meaningful, and says what the chosen
/// value costs so that it is a decision rather than a surprise.
fn max_depth(raw: &str) -> Result<usize, String> {
    let n: usize = raw
        .parse()
        .map_err(|_| format!("`{raw}` is not a number"))?;
    if n == 0 {
        return Err("a depth budget of 0 would refuse every call".to_owned());
    }
    if n > nazm_core::eval::MAX_SUPPORTED_DEPTH {
        return Err(format!(
            "at most {}; {n} calls would need {} GiB of stack reserved, which no machine \
             will grant — and a budget the interpreter cannot back with stack is not a \
             limit, it is a crash waiting for the right program",
            nazm_core::eval::MAX_SUPPORTED_DEPTH,
            nazm_core::eval::stack_for(n) >> 30,
        ));
    }
    Ok(n)
}

/// `nazm build`'s Gate 2 options, boxed so the largest command stays near the others in size.
#[derive(Debug, clap::Args)]
pub(crate) struct Selection {
    /// The `@test`, `@fuzz` or `@bench` function to run through the generated entry, as `nazm
    /// test` and `nazm bench` ask (Gate 2, §18–§19): `test:NAME`, `fuzz:NAME:CASES` or
    /// `bench:NAME:ITERATIONS`. Not an interface of its own.
    #[arg(long, hide = true, value_name = "SPEC")]
    pub(crate) harness: Option<String>,
    /// Features of the root package to enable beyond its `default`, comma-separated (Gate 2,
    /// §22); each package's `@cfg(feature = …)` sees its own.
    #[arg(long, value_name = "NAMES", value_delimiter = ',')]
    pub(crate) features: Vec<String>,
}
