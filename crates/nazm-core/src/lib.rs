//! Checking and evaluation for Nazm v0.1.
//!
//! The pipeline is deliberately one direction with no back edges: source → tokens → AST →
//! checked → evaluated. Each stage collects every diagnostic it can before the next runs,
//! so a program with three mistakes reports three.

pub mod check;
pub mod eval;
pub mod intrinsic;
pub mod types;

pub use check::{check, Signature};
pub use eval::{Interpreter, RuntimeError, Value};
pub use intrinsic::Intrinsic;
pub use types::Type;

/// Re-exported so the CLI can parse without taking its own dependency on the syntax
/// crate. The double underscore marks it as plumbing rather than interface.
pub mod __syntax {
    pub use nazm_syntax::{
        format, lex, lex_in, parse_file, parse_map, parse_source, Block, Expr, Function, Program,
        Stmt, TokenKind,
    };
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

/// The name a single-file convenience gives its one file.
///
/// The `&str` entry points below exist for callers that genuinely have one file — tests,
/// mostly. Everything with a real program behind it goes through a [`SourceMap`] built by
/// the loader, because that is the only form that knows what a span belongs to.
const ONE_FILE: &str = "<input>";

/// As [`run`], with explicit budgets.
///
/// The budgets are the interpreter's, not the language's — see `docs/spec.md`. They exist
/// so that a program which does not terminate reports instead of consuming the machine,
/// whether it fails to terminate by looping or by recursing.
#[must_use]
pub fn run_with_limits(source: &str, limits: eval::Limits) -> Outcome {
    run_with(source, limits, Vec::new())
}

/// As [`run_with_limits`], with the arguments `arg(i)` will see.
///
/// Excluding the program's own name, so that `arg(0)` means the same thing interpreted
/// and compiled — a native `argv[0]` is a path nobody should depend on.
#[must_use]
pub fn run_with(source: &str, limits: eval::Limits, args: Vec<String>) -> Outcome {
    run_map(
        &nazm_span::SourceMap::single(ONE_FILE, source),
        limits,
        args,
    )
}

/// As [`run_with`], over every file of a compilation.
#[must_use]
pub fn run_map(map: &nazm_span::SourceMap, limits: eval::Limits, args: Vec<String>) -> Outcome {
    // The whole pipeline on a stack the interpreter controls, sized from the budget it is
    // about to enforce. Every pass here recurses: the parser with the shape of the tree,
    // the checker and the evaluator with it again, and the drop at the end once more.
    // Honouring a 512-call bound on a caller's 2 MiB thread would be a guarantee we
    // cannot keep, and so would parsing a deeply nested expression there.
    match eval::on_interpreter_stack(limits.max_depth, || pipeline(map, limits, args)) {
        Ok(outcome) => outcome,
        // The machine would not give us a thread. Nothing to do with the program, and
        // saying so beats blaming the code that was about to run. The span points at the
        // start of the file because no position would be more honest than that.
        Err(e) => Outcome::RuntimeFailed(
            nazm_diag::Diagnostic::error(
                nazm_diag::codes::INTERPRETER_UNAVAILABLE,
                nazm_span::Span::synthetic(),
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
fn pipeline(map: &nazm_span::SourceMap, limits: eval::Limits, args: Vec<String>) -> Outcome {
    let (program, mut diagnostics) = nazm_syntax::parse_map(map);
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
        .with_limits(limits)
        .with_args(args)
        .run_main()
    {
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
    check_map(&nazm_span::SourceMap::single(ONE_FILE, source))
}

/// As [`check_source`], over every file of a compilation.
#[must_use]
pub fn check_map(map: &nazm_span::SourceMap) -> Vec<nazm_diag::Diagnostic> {
    // `()` as the result, so the tree is built, walked and **dropped** inside the closure.
    // Returning it would move a recursive structure onto whatever stack the caller
    // happens to have, which is the thing this function exists to avoid.
    match on_checked_map(map, |_| Ok::<(), Vec<nazm_diag::Diagnostic>>(())) {
        Ok(()) => Vec::new(),
        Err(d) => d,
    }
}

/// Parse, check, and run `f` over the tree — all on the interpreter's own stack.
///
/// # Why the tree never leaves
///
/// `docs/spec.md` promises that the whole pipeline runs on a thread whose stack the
/// interpreter sets itself, because **a bound only means something against a known
/// stack**: macOS gives the main thread 8 MiB, a spawned thread 2 MiB, and the test
/// harness 2 MiB per test. Parsing, checking, lowering, emitting and *dropping* the tree
/// all recurse with its shape.
///
/// An earlier version of this file returned the `Program` to the caller. Nothing observed
/// an abort at `MAX_NESTING` — drop frames are small — but it made the guarantee depend on
/// the caller's stack and on frames staying small, neither of which the contract mentions.
/// So `f` runs *here*, and only its result crosses back. `T: Send` is what stops a
/// borrowed tree escaping with it.
///
/// # Errors
///
/// The parse and check diagnostics if there are any — in which case `f` is never called,
/// because running a later pass over a tree known to be ill-typed produces a second, less
/// useful error about the same mistake. Otherwise whatever `f` returns.
pub fn on_checked<T, F>(source: &str, f: F) -> Result<T, Vec<nazm_diag::Diagnostic>>
where
    T: Send,
    F: FnOnce(&__syntax::Program) -> Result<T, Vec<nazm_diag::Diagnostic>> + Send,
{
    on_checked_map(&nazm_span::SourceMap::single(ONE_FILE, source), f)
}

/// As [`on_checked`], over every file of a compilation.
///
/// # Errors
///
/// The parse and check diagnostics if there are any, otherwise whatever `f` returns.
pub fn on_checked_map<T, F>(
    map: &nazm_span::SourceMap,
    f: F,
) -> Result<T, Vec<nazm_diag::Diagnostic>>
where
    T: Send,
    F: FnOnce(&__syntax::Program) -> Result<T, Vec<nazm_diag::Diagnostic>> + Send,
{
    let run = || {
        let (program, mut diagnostics) = nazm_syntax::parse_map(map);
        let parse_failed = !diagnostics.is_empty();
        let (_, check_errors) = check(&program);
        // A function that failed to parse was never registered, so "this program has no
        // `main`" would be reported for a file whose first line is `fn main`.
        diagnostics.extend(
            check_errors
                .into_iter()
                .filter(|d| !(parse_failed && d.code == nazm_diag::codes::MISSING_MAIN)),
        );
        if diagnostics.is_empty() {
            f(&program)
        } else {
            Err(diagnostics)
        }
    };

    eval::on_interpreter_stack(0, run).unwrap_or_else(|e| {
        Err(vec![nazm_diag::Diagnostic::error(
            nazm_diag::codes::INTERPRETER_UNAVAILABLE,
            nazm_span::Span::synthetic(),
            format!("could not start the checker: {e}"),
        )])
    })
}
