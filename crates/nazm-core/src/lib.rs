//! Checking and evaluation for Nazm v0.1.
//!
//! The pipeline is deliberately one direction with no back edges: source → tokens → AST →
//! checked → evaluated. Each stage collects every diagnostic it can before the next runs,
//! so a program with three mistakes reports three.

pub mod check;
mod effects;
pub mod eval;
mod provenance;

pub use check::{
    check, check_reusing, check_unit, declare_types, declare_unit, resolve_type_fields,
    validate_type_layouts, CheckEverything, Reuse, Signature, Unit,
};
pub use eval::{Interpreter, RuntimeError, Value};
// `Type`, `Intrinsic` and `Resolution` moved to `nazm-syntax` on 2026-09-21, so the
// backend could see them without depending on the interpreter, and to `nazm-sema` on
// 2026-09-22, once N2 showed that the syntax crate's own code referred to them exactly
// never. Re-exported here because they are still what a caller of this crate means by a
// type, a built-in and a resolution.
pub use nazm_sema::{Intrinsic, Resolution, Type, UnitInterface};

/// What a program means, as its own vocabulary.
///
/// Re-exported rather than depended on directly, so that the front end has one entry
/// point: the driver builds a [`sema::ModuleGraph`] while loading, hands it to [`check`],
/// and never needs a second route into the compiler.
pub use nazm_sema as sema;

/// Re-exported so the CLI can parse without taking its own dependency on the syntax
/// crate. The double underscore marks it as plumbing rather than interface.
pub mod __syntax {
    /// A completion probe's hole (N26).
    pub use nazm_syntax::HoleSite;
    pub use nazm_syntax::{
        format, lex, lex_in, parse_file, parse_map, parse_source, Block, Enum, Expr, Field,
        FieldInit, Function, MatchArm, PatternField, Place, PlaceField, Program, Record, Stmt,
        TokenKind, TypeName, TypeParam, Variant, PARSE_STACK_BYTES,
    };
    /// The lossless tree (N15), for a tool that needs to know where a position is. What a
    /// position *means* is the resolution's, never the tree's.
    pub use nazm_syntax::{parse_cst, Cst, CstNode, Leaf, SyntaxKind};
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

/// A compilation of one file: that file as the root module, and the core prelude.
///
/// The prelude is part of every compilation, including this one — `docs/spec.md`, *The
/// core prelude* — so a program given as a string sees `Result` and `Option` exactly as a
/// program read from disk does. It comes **after** the file, so the file is still
/// `FileId::ROOT` and every span in it means what it meant before.
#[must_use]
pub fn one_file(source: &str) -> (nazm_span::SourceMap, sema::ModuleGraph) {
    let map = nazm_span::SourceMap::from_parts(vec![
        (ONE_FILE.to_owned(), source.to_owned()),
        (
            sema::prelude::FILE_NAME.to_owned(),
            sema::prelude::SOURCE.to_owned(),
        ),
    ]);
    let mut graph = sema::ModuleGraph::single();
    graph.attach_prelude(nazm_span::FileId::from_index(1));
    (map, graph)
}

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
    let (map, graph) = one_file(source);
    run_map(&map, &graph, limits, args)
}

/// As [`run_with`], over every file of a compilation.
#[must_use]
pub fn run_map(
    map: &nazm_span::SourceMap,
    graph: &sema::ModuleGraph,
    limits: eval::Limits,
    args: Vec<String>,
) -> Outcome {
    // The whole pipeline on a stack the interpreter controls, sized from the budget it is
    // about to enforce. Every pass here recurses: the parser with the shape of the tree,
    // the checker and the evaluator with it again, and the drop at the end once more.
    // Honouring a 512-call bound on a caller's 2 MiB thread would be a guarantee we
    // cannot keep, and so would parsing a deeply nested expression there.
    match eval::on_interpreter_stack(limits.max_depth, || pipeline(map, graph, limits, args)) {
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
fn pipeline(
    map: &nazm_span::SourceMap,
    graph: &sema::ModuleGraph,
    limits: eval::Limits,
    args: Vec<String>,
) -> Outcome {
    let (program, mut diagnostics) = nazm_syntax::parse_map(map);
    let parse_failed = !diagnostics.is_empty();
    let (resolution, check_errors) = check(&program, graph);

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

    match Interpreter::new(&program, &resolution)
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
    let (map, graph) = one_file(source);
    check_map(&map, &graph)
}

/// As [`check_source`], over every file of a compilation, reusing what `reuse` allows.
///
/// # Why this is not `check_map` with an argument
///
/// It is, and the separation is deliberate anyway: `check_map` is the whole-compilation
/// entry point every other consumer uses, and a caller that has no cache should not have
/// to pass something saying so. There is one checking path underneath — this function and
/// `check_map` both reach [`check_reusing`], and a compilation that reuses nothing takes
/// exactly the route it took before N4.
///
/// The modules whose source did not parse are computed here rather than asked for,
/// because this is where parsing happens and a caller cannot know before it does.
#[must_use]
pub fn check_map_reusing(
    map: &nazm_span::SourceMap,
    graph: &sema::ModuleGraph,
    reuse: &mut (dyn Reuse + Send),
) -> Vec<nazm_diag::Diagnostic> {
    let run = || {
        let (program, mut diagnostics) = nazm_syntax::parse_map(map);
        let parse_failed = !diagnostics.is_empty();

        // A module whose file produced a parse diagnostic. Its tree is partial, so its
        // declarations are partial, so nothing about it may be written down for a later
        // run to inherit.
        let unparsable: Vec<sema::ModuleId> = diagnostics
            .iter()
            .filter_map(|d| graph.module_of(d.primary.span.file))
            .collect();

        let (_, check_errors) = check_reusing(&program, graph, reuse, &unparsable);
        // A function that failed to parse was never registered, so "this program has no
        // `main`" would be reported for a file whose first line is `fn main`.
        diagnostics.extend(
            check_errors
                .into_iter()
                .filter(|d| !(parse_failed && d.code == nazm_diag::codes::MISSING_MAIN)),
        );
        diagnostics
    };

    eval::on_interpreter_stack(0, run).unwrap_or_else(|e| {
        vec![nazm_diag::Diagnostic::error(
            nazm_diag::codes::INTERPRETER_UNAVAILABLE,
            nazm_span::Span::synthetic(),
            format!("could not start the checker: {e}"),
        )]
    })
}

/// As [`check_source`], over every file of a compilation.
#[must_use]
pub fn check_map(
    map: &nazm_span::SourceMap,
    graph: &sema::ModuleGraph,
) -> Vec<nazm_diag::Diagnostic> {
    // `()` as the result, so the tree is built, walked and **dropped** inside the closure.
    // Returning it would move a recursive structure onto whatever stack the caller
    // happens to have, which is the thing this function exists to avoid.
    match on_checked_map(map, graph, |_, _| Ok::<(), Vec<nazm_diag::Diagnostic>>(())) {
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
    F: FnOnce(&__syntax::Program, &Resolution) -> Result<T, Vec<nazm_diag::Diagnostic>> + Send,
{
    let (map, graph) = one_file(source);
    on_checked_map(&map, &graph, f)
}

/// As [`on_checked`], over every file of a compilation.
///
/// # Errors
///
/// The parse and check diagnostics if there are any, otherwise whatever `f` returns.
pub fn on_checked_map<T, F>(
    map: &nazm_span::SourceMap,
    graph: &sema::ModuleGraph,
    f: F,
) -> Result<T, Vec<nazm_diag::Diagnostic>>
where
    T: Send,
    F: FnOnce(&__syntax::Program, &Resolution) -> Result<T, Vec<nazm_diag::Diagnostic>> + Send,
{
    on_checked_with(map, graph, Entry::Required, f)
}

/// Whether this compilation is a **program**.
///
/// The root module owns `main` and a program without one is `N0204` — a rule about
/// programs, settled in `docs/spec.md`. Asking a module what it *offers* is a different
/// question, and a library answers it perfectly well with no entry point. This is the one
/// distinction between the two, named rather than expressed as a filter over diagnostics
/// at a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// It is a program: the root module must have `main`.
    Required,
    /// It is a module: whether anything can run is not being asked.
    Absent,
}

/// As [`on_checked_map`], for a compilation that need not be a program.
///
/// # Errors
///
/// The parse and check diagnostics if there are any, otherwise whatever `f` returns.
pub fn on_module_map<T, F>(
    map: &nazm_span::SourceMap,
    graph: &sema::ModuleGraph,
    f: F,
) -> Result<T, Vec<nazm_diag::Diagnostic>>
where
    T: Send,
    F: FnOnce(&__syntax::Program, &Resolution) -> Result<T, Vec<nazm_diag::Diagnostic>> + Send,
{
    on_checked_with(map, graph, Entry::Absent, f)
}

/// Everything one parse and check of a compilation produced, whatever it found.
///
/// The tree, what resolution decided about it, and every diagnostic — together, because a
/// language service needs the current answer to *where does this name resolve* for a file
/// that also has a type error somewhere else. It is the same parse and the same check
/// every other entry point runs; [`on_checked_map`] is this with the diagnostics turned
/// into an early return.
#[derive(Debug)]
pub struct Analysis {
    pub program: __syntax::Program,
    pub resolution: Resolution,
    pub diagnostics: Vec<nazm_diag::Diagnostic>,
    /// The files whose text had a syntax error, each once, in file order (N19): where the
    /// tree is a recovery, what the checker saw of it may be missing a declaration.
    pub syntax_errors: Vec<nazm_span::FileId>,
}

/// Parse and check `map`, keeping the tree, the resolution and every diagnostic.
///
/// # Stack
///
/// **Not** on a stack of its own: the caller provides [`__syntax::PARSE_STACK_BYTES`], as
/// for `parse_source`, and the returned tree must be dropped on it. The language service
/// owns one thread with that stack for exactly this reason. Every other caller wants the
/// stack arranged for it and uses [`on_checked_map`] or [`on_module_map`].
#[must_use]
pub fn analyse(map: &nazm_span::SourceMap, graph: &sema::ModuleGraph, entry: Entry) -> Analysis {
    let (program, mut diagnostics) = nazm_syntax::parse_map(map);
    let parse_failed = !diagnostics.is_empty();
    let mut syntax_errors: Vec<nazm_span::FileId> =
        diagnostics.iter().map(|d| d.primary.span.file).collect();
    syntax_errors.sort_unstable();
    syntax_errors.dedup();
    // The resolution is **handed on** rather than discarded. Two call sites used to
    // throw it away — `let (_, check_errors) = check(&program)` — and the backend then
    // rebuilt an equivalent one, which is the duplication N1 exists to remove.
    let (resolution, check_errors) = check(&program, graph);
    // A function that failed to parse was never registered, so "this program has no
    // `main`" would be reported for a file whose first line is `fn main`.
    let drop_missing_main = parse_failed || entry == Entry::Absent;
    diagnostics.extend(
        check_errors
            .into_iter()
            .filter(|d| !(drop_missing_main && d.code == nazm_diag::codes::MISSING_MAIN)),
    );
    Analysis {
        program,
        resolution,
        diagnostics,
        syntax_errors,
    }
}

/// A completion probe (N26): what the checker made of the current source with a hole at
/// byte `offset` of the root file, where the grammar has one there.
///
/// The same loader's map and graph, the same parser told the cursor's offset, the same
/// checker. Nothing about it is kept or published: its diagnostics describe a program with
/// a hole in it, and its resolution is only for reading what the hole is a member of.
#[derive(Debug)]
pub struct Probe {
    pub analysis: Analysis,
    /// The hole the parser found, if it found one.
    pub hole: Option<__syntax::HoleSite>,
    /// The root file's concrete tree, as the probe parsed it.
    pub root: Option<__syntax::Cst>,
}

/// Parse and check `map` with a completion hole at byte `offset` of the root file. As
/// [`analyse`] in every other respect, stack included.
#[must_use]
pub fn probe(map: &nazm_span::SourceMap, graph: &sema::ModuleGraph, offset: u32) -> Probe {
    let (program, diagnostics, root) = nazm_syntax::parse_map_at(map, Some(offset));
    let hole = program.hole;
    let mut syntax_errors: Vec<nazm_span::FileId> =
        diagnostics.iter().map(|d| d.primary.span.file).collect();
    syntax_errors.sort_unstable();
    syntax_errors.dedup();
    let (resolution, check_errors) = check(&program, graph);
    let mut diagnostics = diagnostics;
    diagnostics.extend(check_errors);
    Probe {
        analysis: Analysis {
            program,
            resolution,
            diagnostics,
            syntax_errors,
        },
        hole,
        root,
    }
}

fn on_checked_with<T, F>(
    map: &nazm_span::SourceMap,
    graph: &sema::ModuleGraph,
    entry: Entry,
    f: F,
) -> Result<T, Vec<nazm_diag::Diagnostic>>
where
    T: Send,
    F: FnOnce(&__syntax::Program, &Resolution) -> Result<T, Vec<nazm_diag::Diagnostic>> + Send,
{
    let run = || {
        let a = analyse(map, graph, entry);
        if a.diagnostics.is_empty() {
            f(&a.program, &a.resolution)
        } else {
            Err(a.diagnostics)
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
