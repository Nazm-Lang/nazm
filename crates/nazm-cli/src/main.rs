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

mod build;
mod capabilities;
mod fix;
mod fmt;
mod interface;
mod load;
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
                  subset than `nazm run` — notably it refuses recursion. Run `nazm \
                  capabilities` for this build's own answer. See docs/spec.md.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
    },
    /// Type-check without evaluating.
    Check {
        file: PathBuf,
        #[arg(long)]
        json: bool,
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
    let cli = Cli::parse();
    if let Command::Build {
        file,
        out,
        json,
        emit_ir,
        opt_level,
    } = cli.command
    {
        return build::run(&file, out.as_deref(), json, emit_ir, opt_level);
    }
    match cli.command {
        Command::Fix { file, apply, json } => return fix::run(&file, apply, json),
        Command::Interface { file, hash } => return interface::run(&file, hash),
        Command::Capabilities { json } => return capabilities::run(json),
        Command::Fmt { files, check } => return fmt::run(&files, check),
        Command::Test {
            paths,
            json,
            interpret_only,
        } => return test::run(&paths, json, interpret_only),
        _ => {}
    }
    let (path, json, limits) = match cli.command {
        Command::Run {
            file,
            args,
            json,
            max_iterations,
            max_depth,
        } => (
            file,
            json,
            Some((
                nazm_core::eval::Limits {
                    max_iterations,
                    max_depth,
                },
                args,
            )),
        ),
        Command::Check { file, json } => (file, json, None),
        // Handled above; `main` keeps its original shape for the commands whose
        // behaviour these increments must not change.
        Command::Build { .. }
        | Command::Fix { .. }
        | Command::Fmt { .. }
        | Command::Test { .. }
        | Command::Interface { .. }
        | Command::Capabilities { .. } => {
            unreachable!("handled above")
        }
    };

    // The root file and everything it imports, and how they relate. A program with no
    // `use` takes the same path and produces a graph of one module.
    let loaded = match load::load(&path) {
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
    let outcome = if let Some((limits, args)) = limits {
        nazm_core::run_map(map, &loaded.graph, limits, args)
    } else {
        // Through `check_source`, which puts parsing and checking on the same
        // interpreter-owned stack `run` uses. Both recurse with the shape of the tree, so
        // `nazm check` on a deeply nested file needs the stack as much as `nazm run` does.
        let diagnostics = nazm_core::check_map(map, &loaded.graph);
        if diagnostics.is_empty() {
            return report_ok("ok", None);
        }
        Outcome::CheckFailed(diagnostics)
    };

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
