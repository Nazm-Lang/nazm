//! `nazm` — run a Nazm program.
//!
//! Exit status is part of the interface, not an afterthought: `0` ran, `1` did not
//! type-check, `2` failed at runtime, `3` could not read the file. A caller that has to
//! parse stderr to find out which happened is doing work the process could have told it.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nazm_core::Outcome;
use nazm_span::SourceFile;

/// Exit codes, named so the mapping is visible in one place.
mod exit {
    pub(crate) const OK: u8 = 0;
    pub(crate) const CHECK_FAILED: u8 = 1;
    pub(crate) const RUNTIME_FAILED: u8 = 2;
    pub(crate) const NO_SUCH_FILE: u8 = 3;
}

#[derive(Parser, Debug)]
#[command(
    name = "nazm",
    about = "The Nazm programming language.",
    long_about = "Nazm v0.3: an executable subset — functions, `let` and `let mut`, \
                  `if`/`else`, `while` with `break` and `continue`, `return`, recursion, \
                  and arithmetic and comparison over Int, Bool and Str. No collections, \
                  user-defined types, effects or I/O yet; programs using them are refused \
                  by name rather than misparsed. See docs/spec.md.",
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
