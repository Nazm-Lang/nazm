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
//!
//! `nazm run` and `nazm check` are unchanged by any of this.

use std::path::PathBuf;
use std::process::ExitCode;

mod build;

use clap::{Parser, Subcommand};
use nazm_core::Outcome;
use nazm_span::SourceFile;

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
                  and arithmetic and comparison over Int, Bool and Str. No collections, \
                  user-defined types, effects or I/O yet; programs using them are refused \
                  by name rather than misparsed. See docs/user/spec.md.",
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
    /// Compile to a native executable.
    ///
    /// A narrow subset for now — see `docs/user/roadmap.md` M1a. Anything outside it is
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
    let (path, json, limits) = match cli.command {
        Command::Run {
            file,
            json,
            max_iterations,
            max_depth,
        } => (
            file,
            json,
            Some(nazm_core::eval::Limits {
                max_iterations,
                max_depth,
            }),
        ),
        Command::Check { file, json } => (file, json, None),
        // Handled above; `main` keeps its original shape for the two commands whose
        // behaviour this increment must not change.
        Command::Build { .. } => unreachable!("handled above"),
    };

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nazm: cannot read {}: {e}", path.display());
            return ExitCode::from(exit::NO_SUCH_FILE);
        }
    };
    let file = SourceFile::new(path.display().to_string(), text);

    // Checking always runs; evaluation only when it passed. Running a program known to be
    // ill-typed produces a second, less useful error about the same mistake.
    let outcome = if let Some(limits) = limits {
        nazm_core::run_with_limits(file.text(), limits)
    } else {
        // Through `check_source`, which puts parsing and checking on the same
        // interpreter-owned stack `run` uses. Both recurse with the shape of the tree, so
        // `nazm check` on a deeply nested file needs the stack as much as `nazm run` does.
        let diagnostics = nazm_core::check_source(file.text());
        if diagnostics.is_empty() {
            return report_ok("ok", None);
        }
        Outcome::CheckFailed(diagnostics)
    };

    match outcome {
        Outcome::Value(v) => report_ok(&v.to_string(), None),
        Outcome::CheckFailed(ds) => {
            emit(&ds, &file, json);
            eprintln!(
                "nazm: {} error{}",
                ds.len(),
                if ds.len() == 1 { "" } else { "s" }
            );
            ExitCode::from(exit::CHECK_FAILED)
        }
        Outcome::RuntimeFailed(d) => {
            emit(std::slice::from_ref(&d), &file, json);
            ExitCode::from(exit::RUNTIME_FAILED)
        }
    }
}

fn report_ok(text: &str, _extra: Option<&str>) -> ExitCode {
    println!("{text}");
    ExitCode::from(exit::OK)
}

fn emit(diagnostics: &[nazm_diag::Diagnostic], file: &SourceFile, json: bool) {
    for d in diagnostics {
        if json {
            match d.to_json() {
                Ok(line) => eprintln!("{line}"),
                // A diagnostic that cannot be serialised must still be visible; falling
                // back to the rendering beats losing it.
                Err(_) => eprint!("{}", d.render(file)),
            }
        } else {
            eprint!("{}", d.render(file));
        }
    }
}
