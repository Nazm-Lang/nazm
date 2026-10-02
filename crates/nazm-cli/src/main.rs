//! `nazm` — run, check, or natively compile a Nazm program.
//!
//! Exit status is part of the interface, not an afterthought: `0` ran, `1` did not
//! type-check, `2` failed at runtime, `3` could not read the file, `4` the build
//! toolchain was missing or failed. A caller that has to parse stderr to find out which
//! happened is doing work the process could have told it.
//!
//! # The contract a compiled program keeps
//!
//! Decided before the wrapper was written, because these are the parts a user depends on
//! and they are awkward to change afterwards:
//!
//! | | |
//! | --- | --- |
//! | **Result** | The full signed 64-bit value, in decimal, on standard output, followed by a newline. **Not** the exit status, which is 8 bits and would silently truncate almost every `Int` |
//! | **Success** | Exit `0` |
//! | **Runtime failure** | Exit `2` — the same status `nazm run` uses — with one line on standard error: `nazm: error[CODE]: message at file:line:column` |
//! | **Diagnostic category** | The same stable code the interpreter reports: `N0400` for overflow, `N0401` for division by zero |
//! | **Source location** | Line and column of the operator that failed, resolved at compile time and baked into the binary as a constant |
//! | **`--json`** | Applies to **build-time** diagnostics only. A generated executable has no diagnostic framework linked into it and emits the plain line above; promising JSON there would mean shipping a serialiser in every binary |
//! | **Resource limits** | **None.** A compiled program has no iteration budget and no call-depth budget. `--max-iterations` and `--max-depth` belong to `nazm run` and are not read here |
//!
//! That last row is a real difference between the two implementations and is stated
//! rather than left to be discovered. `nazm run` stops a runaway loop with `N0402`; a
//! compiled one runs until something kills it. `docs/spec.md` already places both limits
//! in the interpreter rather than in the language, so the backend is not obliged to
//! reproduce them — it is obliged not to imply it has. Adopting a native budget is a
//! decision with its own diagnostic and its own increment, not a flag added in passing.
//!
//! `nazm run` and `nazm check` are unchanged by any of this.

use std::path::PathBuf;
use std::process::ExitCode;

mod backend;
mod build;
mod capabilities;
mod checking;
mod context;
mod core_ir;
mod derived;
mod diagnostics;
mod docs;
mod explain;
mod fix;
mod fmt;
mod inspect;
mod interface;
mod lsp;
mod mir;
mod package;
mod patch;
mod profile;
mod repo;
mod snapshot;
mod summary;
// Moved to `nazm-service` in N16, so `nazm lsp` loads exactly as every command does.
use nazm_service::{load, root};
mod test;

use clap::{Parser, Subcommand};
use nazm_core::Outcome;
use nazm_span::SourceMap;

/// Exit codes, named so the mapping is visible in one place.
mod exit {
    pub(crate) const OK: u8 = 0;
    pub(crate) const CHECK_FAILED: u8 = 1;
    pub(crate) const RUNTIME_FAILED: u8 = 2;
    pub(crate) const NO_SUCH_FILE: u8 = 3;
    /// The build toolchain was missing, or failed on IR this compiler produced. Separate
    /// from `CHECK_FAILED` on purpose: one means the program is wrong, the other means
    /// the machine could not build a program that is fine.
    pub(crate) const TOOLCHAIN_FAILED: u8 = 4;
}

#[derive(Parser, Debug)]
#[command(
    name = "nazm",
    about = "The Nazm programming language.",
    long_about = "Nazm v0.3: an executable subset — functions, `let` and `let mut`, \
                  `if`/`else`, `while` with `break` and `continue`, `return`, recursion, \
                  arithmetic and comparison over Int, Bool and Str, the Ints and Strs \
                  sequences, modules, file and output built-ins, and structured \
                  concurrency (scope, spawn, Chan). No user-defined types, pattern \
                  matching, generics, effects or floats yet; programs using them are \
                  refused by name rather than misparsed. `nazm build` compiles a narrower \
                  subset than `nazm run` — notably it refuses recursion. `nazm check` \
                  reuses the result of checking an unchanged module; nothing else does, \
                  and native compilation is not incremental. Run `nazm capabilities` for \
                  this build's own answer. See docs/spec.md.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The operations `nazm patch` plans.
#[derive(Subcommand, Debug)]
enum PatchOperation {
    /// Rename the entity declared or used at `--byte` of `--file`, by the language
    /// service's validated rename: locals and private definitions only.
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
}

#[derive(Subcommand, Debug)]
enum Command {
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
        /// `critical` or `cyber`. Added to any the packages' manifests require; a profile only
        /// refuses, and never changes what an accepted program does.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
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
        /// `critical` or `cyber`. Added to any the packages' manifests require; a profile only
        /// refuses, and never changes what an accepted program does.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// One `nazm.profile-report/1` object on stdout: every rule of the profiles in force,
        /// its verdict and its witnesses.
        #[arg(long)]
        profile_report: bool,
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
    /// Print a checked program's Core IR, `nazm.core-ir/1`: what each function became,
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
        /// The target triple. v1 builds for the host only, so any other triple is refused
        /// rather than silently built for this machine.
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
        /// One `nazm.timings/1` line on stderr: each phase's wall time — load, front end, MIR,
        /// LIR, code generation, native compilation, link — and the total (N48).
        #[arg(long)]
        timings: bool,
        /// Carry DWARF debug information (N48): a subprogram for each function and a source
        /// position on each instruction, so a debugger stops in Nazm functions and shows their
        /// lines. LLVM backend only; the same program, with more metadata.
        #[arg(long)]
        debug: bool,
        /// Refuse to build unless the package's `nazm.lock` records exactly the packages this
        /// resolves to, with their bytes unchanged (N46).
        #[arg(long)]
        locked: bool,
        /// A restriction profile to hold the program to (N47): `general`, `embedded`,
        /// `critical` or `cyber`. Added to any the packages' manifests require; a profile only
        /// refuses, and never changes what an accepted program does.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
    /// Print one program's facts as `nazm.inspect/1` (N48): every definition's identity,
    /// signature, effects, capabilities, provenance, Core IR and MIR digests, and the toolchain,
    /// package and lock identities — one deterministic line of JSON.
    Inspect {
        file: PathBuf,
        /// Only the function of this name.
        #[arg(long, value_name = "NAME")]
        def: Option<String>,
    },
    /// Print where each function allocates, retains, blocks, talks to a channel, calls C or
    /// starts a task (N52), read off MIR, each with its count per call — `at-most-once` or
    /// `unknown`, never a guess. `--json` prints `nazm.cost/1`.
    ExplainCost {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Print every sink — a path written to, standard output, a file's data — and every
    /// declassification, with the origins that reach it (N52). `--json` prints `nazm.flow/1`.
    ExplainFlow {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Resolve a package and write its `nazm.lock` (N46): the exact package versions, sources
    /// and digests a `--locked` build is held to. Written only when its bytes would change.
    Lock {
        /// The package's directory, holding `nazm.toml`.
        dir: PathBuf,
    },
    /// Publish a package into a local registry (N56): copy its manifest and sources and record
    /// the version and its digest. A published version never changes.
    Publish {
        /// The package's directory, holding `nazm.toml`.
        dir: PathBuf,
        /// The registry's directory.
        #[arg(long, value_name = "DIR")]
        registry: PathBuf,
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

fn main() -> ExitCode {
    let mut cli = Cli::parse();
    // A package (N46): its directory stands for its `main`, and `--locked` holds the build to
    // its lockfile — both settled before any command sees the file.
    if let Command::Lock { dir } = &cli.command {
        return package::lock(dir);
    }
    if let Command::Publish { dir, registry } = &cli.command {
        return package::publish(dir, registry);
    }
    if let Command::Yank {
        package: which,
        registry,
        undo,
    } = &cli.command
    {
        return package::yank(which, registry, *undo);
    }

    match &mut cli.command {
        Command::Run { file, locked, .. }
        | Command::Check { file, locked, .. }
        | Command::Build { file, locked, .. } => match package::entry(file, *locked) {
            Ok(Some(main)) => *file = main,
            Ok(None) => {}
            Err(code) => return code,
        },
        Command::Inspect { file, .. }
        | Command::ExplainCost { file, .. }
        | Command::ExplainFlow { file, .. } => match package::entry(file, false) {
            Ok(Some(main)) => *file = main,
            Ok(None) => {}
            Err(code) => return code,
        },
        _ => {}
    }
    if let Command::Inspect { file, def } = &cli.command {
        return inspect::run(file, def.as_deref());
    }
    if let Command::ExplainCost { file, json } = &cli.command {
        return explain::cost(file, *json);
    }
    if let Command::ExplainFlow { file, json } = &cli.command {
        return explain::flow(file, *json);
    }
    // Restriction profiles (N47): held before the command does its own work, from the facts a
    // full check settles.
    let wants_report = matches!(
        &cli.command,
        Command::Check {
            profile_report: true,
            ..
        }
    );
    match &cli.command {
        Command::Run {
            file,
            locked,
            profile,
            ..
        }
        | Command::Build {
            file,
            locked,
            profile,
            ..
        } => {
            if let Err(code) = profile::enforce(file, profile.as_deref(), *locked, false) {
                return code;
            }
        }
        Command::Check {
            file,
            locked,
            profile,
            ..
        } => {
            if let Err(code) = profile::enforce(file, profile.as_deref(), *locked, wants_report) {
                return code;
            }
        }
        _ => {}
    }
    if let Command::Build {
        file,
        out,
        json,
        emit_ir,
        opt_level,
        no_cache,
        build_report,
        summary_json,
        backend,
        target,
        link,
        lib,
        timings,
        debug,
        locked: _,
        profile: _,
    } = cli.command
    {
        return build::run(
            &file,
            out.as_deref(),
            json,
            emit_ir,
            opt_level,
            !no_cache,
            build_report,
            summary_json,
            build::Choice {
                backend: nazm_lir::backend::BackendId::by_name(&backend)
                    .unwrap_or(nazm_lir::backend::BackendId::Llvm),
                target: target.as_deref(),
                link: &link,
                library: lib,
                timings,
                debug,
            },
        );
    }
    match cli.command {
        Command::Check {
            file,
            json,
            source_root,
            no_cache,
            cache_report,
            summary_json,
            locked: _,
            profile: _,
            profile_report: _,
        } => {
            return checking::run(
                &file,
                json,
                source_root.as_deref(),
                !no_cache,
                cache_report,
                summary_json,
            )
        }
        Command::Fix { file, apply, json } => return fix::run(&file, apply, json),
        Command::Interface {
            file,
            hash,
            source_root,
        } => return interface::run(&file, hash, source_root.as_deref()),
        Command::Capabilities { json } => return capabilities::run(json),
        Command::CoreIr {
            file,
            json,
            no_spans,
            source_root,
        } => return core_ir::run(&file, json, !no_spans, source_root.as_deref()),
        Command::Mir {
            file,
            json,
            spans,
            source_root,
        } => return mir::run(&file, json, spans, source_root.as_deref()),
        Command::Lsp { source_root } => return lsp::run(source_root.as_deref()),
        Command::Context {
            root,
            file,
            byte,
            json,
            source_root,
        } => return context::run(&root, &file, byte, json, source_root.as_deref()),
        Command::Snapshot {
            root,
            json,
            source_root,
        } => return snapshot::snapshot(&root, json, source_root.as_deref()),
        Command::Delta {
            root,
            baseline,
            json,
            source_root,
        } => return snapshot::delta(&root, &baseline, json, source_root.as_deref()),
        Command::Diagnostics {
            root,
            detail,
            state,
            json,
            source_root,
        } => {
            return diagnostics::run(
                &root,
                detail.zip(state).as_ref(),
                json,
                source_root.as_deref(),
            )
        }
        Command::Patch { operation } => {
            return match operation {
                PatchOperation::Rename {
                    root,
                    file,
                    byte,
                    new_name,
                    json,
                    source_root,
                } => patch::run(
                    &root,
                    &file,
                    byte,
                    &patch::Ask::Rename(new_name),
                    json,
                    source_root.as_deref(),
                ),
                PatchOperation::Fix {
                    root,
                    file,
                    byte,
                    fix,
                    json,
                    source_root,
                } => patch::run(
                    &root,
                    &file,
                    byte,
                    &patch::Ask::Fix(fix),
                    json,
                    source_root.as_deref(),
                ),
            }
        }
        Command::Fmt { files, check } => return fmt::run(&files, check),
        Command::Docs {
            index,
            document,
            section,
            state,
            json,
            root,
        } => {
            let root = root.unwrap_or_else(nazm_docs::default_root);
            return match (index, section) {
                (true, _) => docs::index(&root, document.as_deref(), json),
                (false, Some(id)) => docs::section(&root, &id, state.as_deref(), json),
                (false, None) => unreachable!("clap requires one"),
            };
        }
        Command::Repo {
            map,
            task: _,
            intent,
            seed,
            state,
            max_entities,
            max_source_bytes,
            max_doc_bytes,
            json,
            root,
        } => {
            let root = root.unwrap_or_else(nazm_docs::default_root);
            if map {
                return repo::map(&root, json);
            }
            // Clap has already refused any other word.
            let intent = intent.as_deref().and_then(nazm_repo::Intent::parse);
            let request = nazm_repo::TaskRequest {
                intent,
                seeds: seed,
                state,
                budget: nazm_repo::Budget {
                    max_entities,
                    max_source_bytes,
                    max_doc_bytes,
                },
            };
            return repo::task(&root, &request, json);
        }
        Command::Test {
            paths,
            json,
            interpret_only,
            summary_json,
        } => return test::run(&paths, json, interpret_only, summary_json),
        _ => {}
    }
    let (path, json, limits) = match cli.command {
        Command::Run {
            file,
            args,
            json,
            max_iterations,
            max_depth,
            locked: _,
            profile: _,
        } => (
            file,
            json,
            (
                nazm_core::eval::Limits {
                    max_iterations,
                    max_depth,
                },
                args,
            ),
        ),
        // Handled above; `main` keeps its original shape for the commands whose
        // behaviour these increments must not change.
        Command::Build { .. }
        | Command::Check { .. }
        | Command::Fix { .. }
        | Command::Context { .. }
        | Command::Snapshot { .. }
        | Command::Delta { .. }
        | Command::Patch { .. }
        | Command::Diagnostics { .. }
        | Command::Docs { .. }
        | Command::Repo { .. }
        | Command::Fmt { .. }
        | Command::Test { .. }
        | Command::Interface { .. }
        | Command::Lsp { .. }
        | Command::CoreIr { .. }
        | Command::Mir { .. }
        | Command::Capabilities { .. }
        | Command::Lock { .. }
        | Command::Publish { .. }
        | Command::Yank { .. }
        | Command::ExplainCost { .. }
        | Command::ExplainFlow { .. }
        | Command::Inspect { .. } => {
            unreachable!("handled above")
        }
    };

    // `nazm run` evaluates; it persists nothing and reuses nothing, so where the project
    // begins is not its question. The root is discovered rather than given because the
    // loader computes a durable key for every module whether or not this command has a
    // use for one.
    let source_root = match root::decide(&path, None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nazm: {e}");
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };

    // The root file and everything it imports, and how they relate. A program with no
    // `use` takes the same path and produces a graph of one module.
    let loaded = match load::load(&path, source_root.path()) {
        Ok(f) => f,
        // What was loaded comes back with the errors, so the caret lands on the `use`
        // that could not be resolved rather than on nothing.
        Err((partial, diagnostics)) => {
            emit(&diagnostics, &partial.map, json);
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let map = &loaded.map;

    // Checking always runs; evaluation only when it passed. Running a program known to be
    // ill-typed produces a second, less useful error about the same mistake.
    let (limits, args) = limits;
    let outcome = nazm_core::run_map(map, &loaded.graph, limits, args);

    // After evaluation, so every environment frame and every value it held has been
    // dropped. The same line a compiled program writes, from the same environment
    // variable, so the two implementations can be compared on reclamation and not only on
    // the value they produce. `docs/spec.md` says when storage is reclaimed; this is how a
    // test checks that both of them did it.
    if std::env::var_os(nazm_lir::MEMORY_VARIABLE).is_some() {
        let c = nazm_core::eval::memory_counts();
        let mut line = String::from("nazm-memory:");
        for (name, (allocated, reclaimed)) in [
            ("sequences", c.sequences),
            ("strings", c.strings),
            ("channels", c.channels),
            ("closures", c.closures),
        ] {
            use std::fmt::Write as _;
            let _ = write!(
                line,
                " {name} allocated={allocated} reclaimed={reclaimed} live={}",
                allocated - reclaimed
            );
        }
        eprintln!("{line}");
    }
    // The scheduler's counters (N54), on their own variable, as a compiled program has them.
    if std::env::var_os(nazm_lir::runtime::SCHED_VARIABLE).is_some() {
        let c = nazm_core::eval::sched_counts();
        eprintln!(
            "nazm-sched: spawned={} joined={} selects={} timeouts={}",
            c.spawned, c.joined, c.selects, c.timeouts
        );
    }

    match outcome {
        Outcome::Value(v) => report_ok(&v.to_string(), None),
        Outcome::CheckFailed(ds) => {
            emit(&ds, map, json);
            eprintln!(
                "nazm: {} error{}",
                ds.len(),
                if ds.len() == 1 { "" } else { "s" }
            );
            ExitCode::from(exit::CHECK_FAILED)
        }
        Outcome::RuntimeFailed(d) => {
            emit(std::slice::from_ref(&d), map, json);
            ExitCode::from(exit::RUNTIME_FAILED)
        }
    }
}

fn report_ok(text: &str, _extra: Option<&str>) -> ExitCode {
    println!("{text}");
    ExitCode::from(exit::OK)
}

pub(crate) fn emit(diagnostics: &[nazm_diag::Diagnostic], map: &SourceMap, json: bool) {
    for d in diagnostics {
        if json {
            match d.to_json(map) {
                Ok(line) => eprintln!("{line}"),
                // A diagnostic that cannot be serialised must still be visible; falling
                // back to the rendering beats losing it.
                Err(_) => eprint!("{}", d.render(map)),
            }
        } else {
            eprint!("{}", d.render(map));
        }
    }
}
