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
    run_with_limits(source, eval::Limits::default())
}

/// As [`run`], with explicit budgets.
///
/// The budgets are the interpreter's, not the language's — see `docs/spec.md`. They exist
/// so that a program which does not terminate reports instead of consuming the machine,
/// whether it fails to terminate by looping or by recursing.
#[must_use]
pub fn run_with_limits(source: &str, limits: eval::Limits) -> Outcome {
    // The whole pipeline on a stack the interpreter controls, sized from the budget it is
    // about to enforce. Every pass here recurses: the parser with the shape of the tree,
    // the checker and the evaluator with it again, and the drop at the end once more.
    // Honouring a 512-call bound on a caller's 2 MiB thread would be a guarantee we
    // cannot keep, and so would parsing a deeply nested expression there.
    match eval::on_interpreter_stack(limits.max_depth, || pipeline(source, limits)) {
        Ok(outcome) => outcome,
        // The machine would not give us a thread. Nothing to do with the program, and
        // saying so beats blaming the code that was about to run. The span points at the
        // start of the file because no position would be more honest than that.
        Err(e) => Outcome::RuntimeFailed(
            nazm_diag::Diagnostic::error(
                nazm_diag::codes::INTERPRETER_UNAVAILABLE,
                nazm_span::Span::new(0, 0),
                format!("could not start the evaluator: {e}"),
            )
            .help(
                "the interpreter runs on a thread of its own so that its limits mean \
                 something; this machine would not give it one",
            ),
        ),
    }
}

/// Parse, check, and evaluate. Always called on an interpreter-owned stack.
fn pipeline(source: &str, limits: eval::Limits) -> Outcome {
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

    match Interpreter::new(&program).with_limits(limits).run_main() {
        Ok(v) => Outcome::Value(v),
        Err(e) => Outcome::RuntimeFailed(*e.diagnostic),
    }
}

/// Parse and check without evaluating, on the same owned stack.
///
/// `nazm check` needs the stack as much as `nazm run` does: parsing and checking recurse
/// with the shape of the tree, and neither has anything to do with call depth.
#[must_use]
pub fn check_source(source: &str) -> Vec<nazm_diag::Diagnostic> {
    eval::on_interpreter_stack(0, || {
        let (program, mut diagnostics) = nazm_syntax::parse_source(source);
        let parse_failed = !diagnostics.is_empty();
        let (_, check_errors) = check(&program);
        diagnostics.extend(
            check_errors
                .into_iter()
                .filter(|d| !(parse_failed && d.code == nazm_diag::codes::MISSING_MAIN)),
        );
        diagnostics
    })
    .unwrap_or_else(|e| {
        vec![nazm_diag::Diagnostic::error(
            nazm_diag::codes::INTERPRETER_UNAVAILABLE,
            nazm_span::Span::new(0, 0),
            format!("could not start the checker: {e}"),
        )]
    })
}
