//! Checking and evaluation for Nazm v0.1.
//!
//! The pipeline is deliberately one direction with no back edges: source → tokens → AST →
//! checked → evaluated. Each stage collects every diagnostic it can before the next runs,
//! so a program with three mistakes reports three.

pub mod check;
pub mod eval;
pub mod types;

pub use check::{check, Signature};
pub use eval::{Interpreter, RuntimeError, Value};
pub use types::Type;

/// Re-exported so the CLI can parse without taking its own dependency on the syntax
/// crate. The double underscore marks it as plumbing rather than interface.
pub mod __syntax {
    pub use nazm_syntax::{parse_source, Program};
}

use nazm_diag::Diagnostic;

/// What running a program produced.
#[derive(Debug)]
pub enum Outcome {
    /// It type-checked and ran.
    Value(Value),
    /// It did not type-check. Nothing was evaluated.
    CheckFailed(Vec<Diagnostic>),
    /// It type-checked and then failed at runtime.
    RuntimeFailed(Diagnostic),
}

/// Check and, if that passes, evaluate.
///
/// Evaluation is not attempted when checking failed. Running a program known to be
/// ill-typed produces a second, less useful error about the same mistake.
#[must_use]
pub fn run(source: &str) -> Outcome {
    run_with_limit(source, eval::DEFAULT_MAX_ITERATIONS)
}

/// As [`run`], with an explicit iteration budget.
///
/// The budget is the interpreter's, not the language's — see `docs/spec.md`. It exists so
/// a non-terminating program reports instead of consuming the machine.
#[must_use]
pub fn run_with_limit(source: &str, max_iterations: u64) -> Outcome {
    let (program, mut diagnostics) = nazm_syntax::parse_source(source);
    let parse_failed = !diagnostics.is_empty();
    let (_signatures, check_errors) = check(&program);

    // A function that failed to parse was never registered, so "this program has no
    // `main`" would be reported for a file whose first line is `fn main`. True of the
    // tree that was built, misleading about the file, and it buries the real error.
    diagnostics.extend(
        check_errors
            .into_iter()
            .filter(|d| !(parse_failed && d.code == nazm_diag::codes::MISSING_MAIN)),
    );

    if !diagnostics.is_empty() {
        return Outcome::CheckFailed(diagnostics);
    }

    match Interpreter::new(&program)
        .with_max_iterations(max_iterations)
        .run_main()
    {
        Ok(v) => Outcome::Value(v),
        Err(e) => Outcome::RuntimeFailed(e.diagnostic),
    }
}
