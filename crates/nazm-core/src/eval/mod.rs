//! The Core IR evaluator — `nazm run`'s interpreter.
//!
//! Runs only Core IR, and Core IR exists only for a program that checked, so the shapes it
//! handles are the ones the checker guaranteed and the lowering stated. What remains at runtime
//! is what a type system cannot rule out: integer overflow and division by zero, an index
//! outside a string or sequence, a failing file operation. Each stops the program with a
//! diagnostic at the operator or call, and each behaves identically in every build — a bug
//! that reproduces only in one build configuration is the worst kind to be handed.
//!
//! # What it reads
//!
//! Core IR, and nothing else (`docs/architecture.md` §7.41). Not the syntax tree, and not the
//! checker's resolution: a local is a slot, a call names its callee, a field is an index, a
//! `break` names its loop, and `?` has already become the `match` it means. Until N39 this
//! walked the tree, found locals by searching frames for a name, and asked the resolution at
//! every call and field what the source had meant — a second lowering, done again on every
//! step, beside the native backend's.

use std::sync::{Arc, Mutex};

use nazm_cir as cir;
use nazm_diag::{codes, Diagnostic};

use cir::{Block, ExprKind, Stmt};
use nazm_sema::numeric::{self, NumError, NumVal};

use std::thread::JoinHandle;

use nazm_sema::enums::EnumId;
use nazm_sema::intrinsic::Intrinsic;
use nazm_sema::record::RecordId;
use nazm_sema::NumKind;
use nazm_span::Span;

/// A runtime failure. Carries a diagnostic so it renders exactly like a compile error.
///
/// `clippy::result_large_err` is allowed on the evaluator below rather than boxing this.
/// Boxing would add an allocation to a path taken once per program, at most — an
/// evaluation error ends the run — and in exchange every construction site would gain a
/// wrapper that says nothing about what went wrong.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    /// Boxed deliberately. Evaluating one Nazm call nests a dozen host frames, each
    /// carrying a `Result<_, RuntimeError>`, so the error's size is multiplied by the
    /// recursion depth. Keeping it behind a pointer is what lets the depth bound trip
    /// before the stack runs out.
    pub diagnostic: Box<Diagnostic>,
}

impl From<Diagnostic> for RuntimeError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostic: Box::new(diagnostic),
        }
    }
}

/// One call's locals, by [`cir::LocalId`]: `None` for a slot not yet bound, or bound in a
/// region that has since been left.
///
/// # Why a slot vector and not names
///
/// Until N39 a frame was an association list of `(name, value)` pairs, one per lexical
/// scope, searched innermost-first at every read — itself the fix for a `HashMap` that spent
/// 29% of `bench/programs/arith.nz` hashing names (`docs/performance.md`). Resolution had
/// already decided which binding each name meant; the search repeated that decision at run
/// time. Core IR carries the answer, so a read is an index.
///
/// A region's exit clears the slots it owns ([`Block::owns`]), which is exactly when a
/// scope's frame used to be dropped — so a string backing or sequence a binding held is
/// released at the same moment it always was, and the counters in `tests/memory.rs` see no
/// difference.
type Frame = Vec<Option<Value>>;

/// How a statement finished.
///
/// A statement normally falls through to the next one; `break` and `continue` do not, and
/// the loop that has to act on them may be several blocks up. Returning the signal rather
/// than unwinding keeps the control flow visible in the types — every caller has to say
/// what it does with a `Break` it did not expect. Since N39 a `break` and a `continue` name
/// the loop they target, and only that loop consumes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flow {
    /// Fell off the end. Carries the tail's value when the block had one.
    Fell(Option<Value>),
    Break(cir::LoopId),
    Continue(cir::LoopId),
    Return(Value),
}

impl Flow {
    fn is_fall_through(&self) -> bool {
        matches!(self, Self::Fell(_))
    }
}

/// Take the value of a sub-expression, or propagate the control flow that happened instead.
///
/// `return`, `break` and `continue` are statements, but a *block* is an expression — so
/// they can sit inside anything used as a value: `let x = if c { return 1; } else { 2 };`,
/// a call argument, an operand, a condition. When that happens the enclosing expression
/// has no value and never will, and the only correct thing to do is stop evaluating it
/// and carry the flow outward to whatever handles it.
///
/// Treating that case as an error was this evaluator's bug: programs that type-check —
/// the checker lets a diverging branch take the other branch's type — failed at run time
/// with "control flow escaped", a message about the interpreter rather than the program.
macro_rules! take {
    ($slf:ident, $f:expr, $e:expr, $env:expr) => {
        match $slf.eval($f, $e, $env)? {
            Flow::Fell(Some(v)) => v,
            Flow::Fell(None) => return Err(no_value($e)),
            left => return Ok(left),
        }
    };
}

/// A runtime error with one diagnostic.
fn fail(code: nazm_diag::Code, at: Span, message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        diagnostic: Box::new(Diagnostic::error(code, at, message)),
    }
}

/// An expression that was asked for a value and produced none.
///
/// Checking has already established that one arrives, so this is a checker bug rather
/// than a program's mistake — reported, not panicked on.
fn no_value(e: &cir::Expr) -> RuntimeError {
    RuntimeError {
        diagnostic: Box::new(
            Diagnostic::error(
                codes::TYPE_MISMATCH,
                e.at,
                "this produces no value, but one is needed here",
            )
            .help("a block's value is its final expression — one with no `;` after it"),
        ),
    }
}

/// Ten million iterations: high enough that no reasonable program reaches it, low enough
/// that a runaway one reports in seconds rather than consuming the machine.
pub const DEFAULT_MAX_ITERATIONS: u64 = 10_000_000;

/// Host stack reserved for each level of Nazm call depth.
///
/// Measured rather than guessed. On a debug build the deepest observed cost was about
/// 17 KB per nested call, for frames made deliberately heavy — nested calls and
/// arithmetic in the recursive position. This is that, doubled.
pub const STACK_BYTES_PER_CALL: usize = 32 * 1024;

/// Reserved on top of the per-call budget, for everything not proportional to call depth.
///
/// Chiefly the passes that run on this thread before the program does — parsing,
/// checking, lowering to Core IR and dropping the tree — which recurse with the program's
/// shape rather than with call depth. That requirement belongs to the parser, which sets
/// the nesting limit, so the pipeline takes the number from there ([`crate::TREE_STACK_BYTES`])
/// rather than restating it here where the two could drift apart.
pub const STACK_BASE_BYTES: usize = crate::TREE_STACK_BYTES;

/// The largest depth budget this interpreter will accept.
///
/// Not a safety boundary — the stack grows with the budget, so the guarantee holds above
/// the default too. It is the point past which the reservation is larger than any machine
/// will grant, and refusing with a number beats failing to spawn with an errno.
pub const MAX_SUPPORTED_DEPTH: usize = 1_000_000;

/// The stack a given depth budget needs.
///
/// This is what makes the bound a guarantee rather than a default that happens to fit: a
/// caller who raises `--max-depth` raises the reservation with it, so the diagnostic is
/// still reached before the stack is. A machine that cannot provide the reservation says
/// so ([`codes::INTERPRETER_UNAVAILABLE`]) instead of crashing part-way in.
#[must_use]
pub fn stack_for(max_depth: usize) -> usize {
    STACK_BASE_BYTES.saturating_add(max_depth.saturating_mul(STACK_BYTES_PER_CALL))
}

/// Run `f` on a thread with the stack `max_depth` calls require.
///
/// Scoped, so `f` may borrow the program. Panics inside `f` are resumed on the caller's
/// thread rather than swallowed.
///
/// Returns the spawn error rather than falling back to the caller's stack: running inline
/// would be quieter and would silently drop the guarantee [`DEFAULT_MAX_DEPTH`] rests on.
///
/// # Errors
///
/// If the thread cannot be spawned — resource exhaustion, not anything about the program.
pub fn on_interpreter_stack<T: Send>(
    max_depth: usize,
    f: impl FnOnce() -> T + Send,
) -> std::io::Result<T> {
    std::thread::scope(|scope| {
        Ok(std::thread::Builder::new()
            .name("nazm-eval".to_string())
            .stack_size(stack_for(max_depth))
            .spawn_scoped(scope, f)?
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
    })
}

/// The default call-depth bound.
///
/// Deep enough for any recursion a person writes deliberately, shallow enough that the
/// stack is nowhere near exhausted when it trips — which is the whole point: the limit has
/// to be reached *before* the process dies, or there is no diagnostic to read.
///
/// The margin is measured, not assumed. `the_default_depth_is_reached_before_the_stack_is`
/// in `tests/language.rs` recurses to this bound and requires a diagnostic; a per-level
/// cost that grew past the headroom would fail it rather than turning into an unexplained
/// abort in someone's terminal.
pub const DEFAULT_MAX_DEPTH: usize = 512;

/// What the interpreter will spend on a program.
///
/// Neither of these is a language rule, and `docs/spec.md` says so: Nazm bounds neither
/// iteration nor recursion. They are this interpreter's guards, and they exist for the
/// same reason — a program that consumes the machine and says nothing is worse than one
/// that stops and reports where it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_iterations: u64,
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

/// Join anything still running when an interpreter is dropped off the normal path.
///
/// `Stmt::Scope` joins its own tasks; this covers the case it cannot — a panic unwinding
/// through a scope, where the handles would otherwise be dropped and the threads detached.
/// `std::thread::scope` gets this for free; starting tasks eagerly means writing it down.
impl Drop for Interpreter {
    fn drop(&mut self) {
        for level in std::mem::take(&mut self.pending) {
            for handle in level {
                drop(handle.join());
            }
        }
    }
}

#[derive(Debug)]
pub struct Interpreter {
    /// The program, shared with every task.
    ///
    /// Owned through an `Arc` rather than borrowed, because a task's closure has to be
    /// `'static`: it outlives the stack frame that started it by construction, and
    /// nothing borrowed from the caller can be handed to it. The alternative — running
    /// the whole scope body inside `std::thread::scope` — means threading the scope
    /// handle through every method of the interpreter, including the ones that have
    /// nothing to do with concurrency.
    ///
    /// Core IR only. The syntax tree was cloned in here until N39, and the resolution with
    /// it; neither is needed, so neither is kept, and a run holds one representation of the
    /// program rather than three.
    core: Arc<cir::Program>,
    /// Every static `Atomic`'s one cell (Gate 3), made when the run starts and shared with every
    /// task, as the compiled program's cells are its data.
    cells: Arc<Cells>,
    /// Tasks started by the innermost `scope`, innermost last.
    ///
    /// A stack, because a scope may contain another and the inner one joins first.
    ///
    /// A task starts **when its `spawn` runs**, not at the closing brace. Collecting
    /// them and starting them all at the end reads as an optimisation and is a
    /// deadlock: a body that fills a bounded channel before its readers exist blocks
    /// forever, and that is the shape of every worker pool. The first concurrent
    /// program written against this interpreter hung on exactly that.
    pending: Vec<Vec<JoinHandle<Result<(), RuntimeError>>>>,
    /// Bounded so an accidental infinite recursion reports rather than aborting the
    /// process with a stack overflow, which produces no diagnostic at all.
    depth: usize,
    max_depth: usize,
    /// Loop iterations remaining.
    ///
    /// An interpreter guard, not a language rule — `docs/spec.md` is explicit that Nazm
    /// does not bound iteration. With no `break` and no I/O, a non-terminating program
    /// produces nothing whatsoever, and a tool that hangs is worse than one that reports.
    iterations_left: u64,
    /// Calls made, for a meter (N69): every function body entered, `main` excepted.
    calls: u64,
    /// The program's own arguments, **excluding** the program name — `nazm run f.nz --
    /// a b` gives `["a", "b"]`. Excluding it is what makes `arg(i)` mean the same thing
    /// interpreted and compiled: a native `argv[0]` is a path nobody should depend on.
    args: Vec<String>,
}

/// The static atomics' cells (Gate 3), by module and name.
type Cells = std::collections::HashMap<
    (nazm_sema::module::ModuleId, String),
    Arc<std::sync::atomic::AtomicI64>,
>;

/// A shape the lowering never produces, reported rather than assumed away.
fn unreachable_flow<T>(at: Span) -> Result<T, RuntimeError> {
    Err(fail(
        codes::COMPILER_DEFECT,
        at,
        "the interpreter met an expression it cannot evaluate",
    ))
}

/// A fresh frame for `f`, with its parameters bound to `args`.
fn frame(f: &cir::Function, args: Vec<Value>) -> Frame {
    let mut frame: Frame = vec![None; f.locals.len()];
    for (slot, v) in frame.iter_mut().zip(args) {
        *slot = Some(v);
    }
    frame
}

impl Interpreter {
    /// An interpreter for a checked program's Core IR, which it takes.
    #[must_use]
    pub fn new(core: cir::Program) -> Self {
        let cells = core
            .statics
            .iter()
            .filter_map(|s| match s.value {
                nazm_sema::StaticValue::Cell(init) => Some((
                    (s.module, s.name.clone()),
                    Arc::new(std::sync::atomic::AtomicI64::new(init)),
                )),
                nazm_sema::StaticValue::Value(_) => None,
            })
            .collect();
        Self {
            core: Arc::new(core),
            cells: Arc::new(cells),
            pending: Vec::new(),
            depth: 0,
            max_depth: DEFAULT_MAX_DEPTH,
            iterations_left: DEFAULT_MAX_ITERATIONS,
            calls: 0,
            args: Vec::new(),
        }
    }

    /// The arguments `arg(i)` will see.
    #[must_use]
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Change both budgets. `nazm run --max-iterations` and `--max-depth` set these.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.iterations_left = limits.max_iterations;
        self.max_depth = limits.max_depth;
        self
    }

    /// What remains of the iteration budget (N69: the simulator's meter is what one transaction
    /// spent of a fresh one).
    #[must_use]
    pub fn iterations_left(&self) -> u64 {
        self.iterations_left
    }

    /// How many calls have been made (N69).
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.calls
    }

    /// Give both budgets afresh (N67): a reference evaluated element by element holds each
    /// element to the budget a single run would have, not the whole sequence to one.
    pub fn reset_limits(&mut self, limits: Limits) {
        self.iterations_left = limits.max_iterations;
        self.max_depth = limits.max_depth;
    }

    /// Evaluate `main` and return its value.
    pub fn run_main(&mut self) -> Result<Value, RuntimeError> {
        let core = Arc::clone(&self.core);
        // The **root** module's `main`, which the lowering recorded as the entry. An
        // imported module may have one of its own and it is not an entry point;
        // `docs/spec.md` says so and this is where that is true at runtime.
        let Some(main) = core.entry.and_then(|e| core.function(e)) else {
            return Err(fail(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "this program has no `main` function",
            ));
        };
        // The root of authority (N37): each capability parameter `main` declares is handed
        // one, and nothing else in a program can make one. Checking has refused a `main`
        // with a parameter of any other type.
        let roots: Vec<Value> = main
            .param_locals()
            .iter()
            .map(|l| match l.ty {
                nazm_sema::Type::Cap(k) => Value::Cap(k),
                _ => Value::Int(0),
            })
            .collect();
        self.call_body(main, frame(main, roots))
    }

    /// Evaluate the parameterless function `f` and return its value (N65, comptime): a function
    /// of no parameters holds no capability, so whatever it reaches, it reaches by computing.
    /// The caller has held it to that; the budgets of [`Self::with_limits`] still bound it.
    pub fn run_constant(&mut self, f: nazm_sema::FnId) -> Result<Value, RuntimeError> {
        let core = Arc::clone(&self.core);
        let Some(func) = core.function(f).filter(|g| g.params == 0) else {
            return Err(fail(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "no parameterless function to evaluate",
            ));
        };
        self.call_body(func, frame(func, Vec::new()))
    }

    /// Call `f` with `args` (N67: the reference an accelerator's results are held to). The caller
    /// passes values of the parameters' types; a function holding a capability is not called.
    pub fn call(&mut self, f: nazm_sema::FnId, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let core = Arc::clone(&self.core);
        let Some(func) = core
            .function(f)
            .filter(|g| g.params as usize == args.len() && g.authority().is_empty())
        else {
            return Err(fail(
                codes::MISSING_MAIN,
                Span::synthetic(),
                "no function of that arity to call",
            ));
        };
        self.call_body(func, frame(func, args))
    }

    /// Run a function body and produce its value.
    ///
    /// `return` and falling off the end with a tail are the two ways a function produces
    /// one; checking guarantees every path does one or the other, so the remaining cases
    /// are diagnostics rather than panics.
    fn call_body(&mut self, f: &cir::Function, mut env: Frame) -> Result<Value, RuntimeError> {
        match self.block(f, &f.body, &mut env)? {
            Flow::Return(v) | Flow::Fell(Some(v)) => Ok(v),
            Flow::Fell(None) => Err(fail(
                codes::TYPE_MISMATCH,
                f.body.at,
                format!("`{}` finished without producing a value", f.name),
            )),
            Flow::Break(_) | Flow::Continue(_) => Err(fail(
                codes::NOT_IN_A_LOOP,
                f.body.at,
                format!("loop control escaped from `{}`", f.name),
            )),
        }
    }

    /// Run a region, then release what it owns.
    ///
    /// The release is the language's: a binding's value is dropped when the region it was
    /// bound in is left, by falling through, by any transfer, or by a failure — the moment a
    /// scope's frame used to be popped.
    fn block(
        &mut self,
        f: &cir::Function,
        b: &Block,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        let out = self.block_inner(f, b, env);
        for l in &b.owns {
            if let Some(slot) = env.get_mut(l.index()) {
                *slot = None;
            }
        }
        out
    }

    fn block_inner(
        &mut self,
        f: &cir::Function,
        b: &Block,
        env: &mut Frame,
    ) -> Result<Flow, RuntimeError> {
        for stmt in &b.stmts {
            let flow = self.stmt(f, stmt, env)?;
            if !flow.is_fall_through() {
                // Leaves this block and travels outward to whatever handles it.
                return Ok(flow);
            }
        }
        match &b.tail {
            Some(tail) => self.eval(f, tail, env),
            None => Ok(Flow::Fell(None)),
        }
    }

    /// Evaluate initialisers in the order written and place each at its field's declaration
    /// index. `Err` carries a flow that left part way, which drops what was built.
    fn initialise(
        &mut self,
        f: &cir::Function,
        arity: u32,
        fields: &[cir::Init],
        at: Span,
        env: &mut Frame,
    ) -> Result<Result<Vec<Value>, Flow>, RuntimeError> {
        let mut built: Vec<Option<Value>> = vec![None; arity as usize];
        for i in fields {
            let v = match self.eval(f, &i.value, env)? {
                Flow::Fell(Some(v)) => v,
                Flow::Fell(None) => return Err(no_value(&i.value)),
                left => return Ok(Err(left)),
            };
            if let Some(slot) = built.get_mut(i.field as usize) {
                *slot = Some(v);
            }
        }
        // Checking guarantees every field was given exactly one value, so an absent one is
        // a checker bug rather than a program's mistake.
        let mut values = Vec::with_capacity(built.len());
        for v in built {
            match v {
                Some(v) => values.push(v),
                None => return Err(fail(codes::MISSING_FIELD, at, "a field was given no value")),
            }
        }
        Ok(Ok(values))
    }
}

// After `take!`, which the statement and expression evaluators use.
mod arith;
mod channel;
mod counters;
mod exec;
mod intrinsic;
mod net;
mod os;
mod process;
mod task;
mod value;

pub use channel::Channel;
pub use counters::{memory_counts, sched_counts, Counts, SchedCounts};
pub use value::{Backing, Buffer, BytesView, EnumVal, FuncVal, Rec, Seq, Str, Value};
