//! Tree-walking evaluator.
//!
//! Runs only after checking has passed, so the shapes it handles are the ones the checker
//! guaranteed. What remains at runtime is the two failures a type system cannot rule out:
//! integer overflow and division by zero. Both stop the program with a diagnostic that
//! points at the operator, and both behave identically in every build — a bug that
//! reproduces only in one build configuration is the worst kind to be handed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use nazm_diag::{codes, Diagnostic};
use nazm_syntax::{Assign, BinOp, Block, Expr, Function, Program, Stmt, While};

use crate::intrinsic::Intrinsic;

/// A runtime value.
///
/// `PartialEq` is written out rather than derived, because a sequence has no derivable
/// answer: `Mutex` does not compare, and comparing contents would be a language decision
/// nobody has made. Sequences therefore compare by **identity** — the same storage or
/// not — and the checker refuses `==` on them anyway (`N0304`), so the answer is never
/// observable from a Nazm program. It exists so the rest of `Value` can still be
/// compared.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Bool(bool),
    /// **Bytes, not text.** `docs/spec.md` specifies `Str` as a byte sequence, and
    /// `str_slice` is allowed to split a multi-byte character — so the representation
    /// cannot be a Rust `String`, which refuses to hold the result.
    ///
    /// This was a real defect, not a hypothetical: with a `String` here,
    /// `str_byte(str_slice("é", 0, 1), 0)` produced 239 (the first byte of the
    /// replacement character `from_utf8_lossy` substituted) where the specification
    /// requires 195. The host's text type was acting as an unstated specification,
    /// which is exactly what the spec section warns against.
    Str(Vec<u8>),
    /// A growable sequence of `Int`, **by reference**.
    ///
    /// `Rc<RefCell<…>>` is the reference semantics, stated rather than incidental: two
    /// bindings of the same sequence are the same storage, and a push through one is
    /// visible through the other. `docs/spec.md` says so, and the native representation
    /// — a pointer to a header — behaves identically by construction.
    ///
    /// This is emphatically **not** a claim of ownership safety. Nothing prevents
    /// aliasing, nothing tracks lifetimes, and nothing is ever freed. The contract
    /// forbids claiming otherwise, so it is written down here rather than left for a
    /// reader to discover.
    /// `Arc<Mutex<…>>` rather than `Rc<RefCell<…>>` because the whole pipeline runs on
    /// a thread the interpreter owns (`on_interpreter_stack`), so a `Value` has to be
    /// `Send`. The lock is never contended — evaluation is single-threaded — and it is
    /// not a concurrency mechanism.
    Ints(Arc<Mutex<Vec<i64>>>),
    /// The same, for `Str`.
    Strs(Arc<Mutex<Vec<Vec<u8>>>>),
    /// A bounded channel of `Int`, by reference.
    ///
    /// The one value designed to be shared between tasks: its operations *are* the
    /// synchronisation, which is why `docs/spec.md` lets it cross a `spawn` while a
    /// sequence may not.
    Chan(Arc<Channel>),
}

/// A bounded queue with a close flag.
///
/// One mutex and one condition variable. Both senders and receivers wait on the same
/// variable and every change notifies all of them — which is not the fastest arrangement
/// and is the obviously correct one, and `docs/spec.md` promises no fairness anyway.
#[derive(Debug)]
pub struct Channel {
    state: Mutex<ChannelState>,
    changed: std::sync::Condvar,
    capacity: usize,
}

#[derive(Debug, Default)]
struct ChannelState {
    queue: std::collections::VecDeque<i64>,
    closed: bool,
}

impl Channel {
    fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(ChannelState::default()),
            changed: std::sync::Condvar::new(),
            capacity,
        }
    }

    /// Queue a value, or report that the channel is closed — in which case **the value
    /// was not sent**, which is what `docs/spec.md` promises.
    fn send(&self, value: i64) -> Option<bool> {
        let mut state = self.state.lock().ok()?;
        while !state.closed && state.queue.len() >= self.capacity {
            state = self.changed.wait(state).ok()?;
        }
        if state.closed {
            return Some(false);
        }
        state.queue.push_back(value);
        drop(state);
        self.changed.notify_all();
        Some(true)
    }

    /// Take a value, or report that the channel is closed **and drained**. Values queued
    /// before a close are still delivered: closing ends a channel, it does not discard
    /// what is in it.
    fn recv(&self) -> Option<Option<i64>> {
        let mut state = self.state.lock().ok()?;
        while state.queue.is_empty() && !state.closed {
            state = self.changed.wait(state).ok()?;
        }
        let taken = state.queue.pop_front();
        drop(state);
        self.changed.notify_all();
        Some(taken)
    }

    /// Idempotent, and wakes everyone: a blocked sender and a blocked receiver both have
    /// to find out, and neither can be left waiting on a channel nobody will touch again.
    fn close(&self) -> Option<()> {
        let mut state = self.state.lock().ok()?;
        state.closed = true;
        drop(state);
        self.changed.notify_all();
        Some(())
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Ints(a), Self::Ints(b)) => Arc::ptr_eq(a, b),
            (Self::Strs(a), Self::Strs(b)) => Arc::ptr_eq(a, b),
            (Self::Chan(a), Self::Chan(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Value {}

/// Take a sequence's contents, or report the poisoning that can only follow a panic.
///
/// The lock is never contended, so this only fails if a previous evaluation panicked
/// while holding it — in which case the interpreter is already unsound and saying so is
/// better than continuing.
fn seq<T>(lock: &Mutex<T>) -> Option<std::sync::MutexGuard<'_, T>> {
    lock.lock().ok()
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
            // Rendering, not semantics: a `Str` that is not valid UTF-8 has no text
            // form, and showing the replacement character is a display decision. No
            // operation on `Str` goes through here.
            Self::Str(v) => write!(f, "{}", String::from_utf8_lossy(v)),
            // A sequence is never a program's result — `main` returns `Int` — so this
            // is for diagnostics only, and says what it is rather than dumping it.
            Self::Ints(v) => match v.lock() {
                Ok(v) => write!(f, "Ints({} items)", v.len()),
                Err(_) => f.write_str("Ints(unreadable)"),
            },
            Self::Strs(v) => match v.lock() {
                Ok(v) => write!(f, "Strs({} items)", v.len()),
                Err(_) => f.write_str("Strs(unreadable)"),
            },
            Self::Chan(_) => f.write_str("Chan"),
        }
    }
}

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

type Env = Vec<HashMap<String, Value>>;

/// How a statement finished.
///
/// A statement normally falls through to the next one; `break` and `continue` do not, and
/// the loop that has to act on them may be several blocks up. Returning the signal rather
/// than unwinding keeps the control flow visible in the types — every caller has to say
/// what it does with a `Break` it did not expect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flow {
    /// Fell off the end. Carries the tail's value when the block had one.
    Fell(Option<Value>),
    Break,
    Continue,
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
    ($slf:ident, $e:expr, $env:expr) => {
        match $slf.eval($e, $env)? {
            Flow::Fell(Some(v)) => v,
            Flow::Fell(None) => return Err($slf.no_value($e)),
            left => return Ok(left),
        }
    };
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
/// Chiefly the passes over the tree — parsing, checking, evaluating and dropping it —
/// which recurse with its shape rather than with call depth. That requirement belongs to
/// the parser, which sets the nesting limit, so the number is taken from there rather
/// than restated here where the two could drift apart.
pub const STACK_BASE_BYTES: usize = nazm_syntax::PARSE_STACK_BYTES;

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
    program: Arc<Program>,
    /// Name to index in `program.functions`. Shared with tasks for the same reason.
    functions: Arc<HashMap<String, usize>>,
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
    /// The program's own arguments, **excluding** the program name — `nazm run f.nz --
    /// a b` gives `["a", "b"]`. Excluding it is what makes `arg(i)` mean the same thing
    /// interpreted and compiled: a native `argv[0]` is a path nobody should depend on.
    args: Vec<String>,
}

impl Interpreter {
    #[must_use]
    ///
    /// The tree is cloned once, here, so that tasks can share it. One copy per run of a
    /// program is not a cost worth designing around; one copy per task would be.
    pub fn new(program: &Program) -> Self {
        let functions = program
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.clone(), i))
            .collect();
        Self {
            program: Arc::new(program.clone()),
            functions: Arc::new(functions),
            pending: Vec::new(),
            depth: 0,
            max_depth: DEFAULT_MAX_DEPTH,
            iterations_left: DEFAULT_MAX_ITERATIONS,
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

    /// Evaluate `main` and return its value.
    pub fn run_main(&mut self) -> Result<Value, RuntimeError> {
        // Cloned before the lookup so that the body being run borrows the clone and not
        // `self`, which `call_body` needs mutably.
        let program = Arc::clone(&self.program);
        let main = self
            .functions
            .get("main")
            .copied()
            .map(|i| &program.functions[i])
            .ok_or_else(|| RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::MISSING_MAIN,
                    nazm_span::Span::new(0, 0),
                    "this program has no `main` function",
                )),
            })?;
        let mut env: Env = vec![HashMap::new()];
        self.call_body(Self::body_of(main)?, &mut env, "main")
    }

    /// Run a function body and produce its value.
    ///
    /// `return` and falling off the end with a tail are the two ways a function produces
    /// one; checking guarantees every path does one or the other, so the remaining cases
    /// are diagnostics rather than panics.
    fn call_body(
        &mut self,
        body: &Block,
        env: &mut Env,
        name: &str,
    ) -> Result<Value, RuntimeError> {
        match self.block_value(body, env)? {
            Flow::Return(v) | Flow::Fell(Some(v)) => Ok(v),
            Flow::Fell(None) => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    body.span,
                    format!("`{name}` finished without producing a value"),
                )),
            }),
            Flow::Break | Flow::Continue => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::NOT_IN_A_LOOP,
                    body.span,
                    format!("loop control escaped from `{name}`"),
                )),
            }),
        }
    }

    /// Run a block in its own scope.
    fn block(&mut self, b: &Block, env: &mut Env) -> Result<Flow, RuntimeError> {
        env.push(HashMap::new());
        let out = self.block_inner(b, env);
        env.pop();
        out
    }

    fn block_inner(&mut self, b: &Block, env: &mut Env) -> Result<Flow, RuntimeError> {
        for stmt in &b.statements {
            let flow = self.stmt(stmt, env)?;
            if !flow.is_fall_through() {
                // Leaves this block and travels outward to whatever handles it.
                return Ok(flow);
            }
        }
        match &b.tail {
            Some(tail) => self.eval(tail, env),
            None => Ok(Flow::Fell(None)),
        }
    }

    /// Run a block where a value is required.
    ///
    /// Used for `if` branches in expression position and for a function body. A block
    /// that diverges has no value and the caller decides what that means.
    fn block_value(&mut self, b: &Block, env: &mut Env) -> Result<Flow, RuntimeError> {
        self.block(b, env)
    }

    fn stmt(&mut self, stmt: &Stmt, env: &mut Env) -> Result<Flow, RuntimeError> {
        match stmt {
            Stmt::Let(l) => {
                let v = take!(self, &l.value, env);
                if let Some(frame) = env.last_mut() {
                    frame.insert(l.name.clone(), v);
                }
                Ok(Flow::Fell(None))
            }
            Stmt::Assign(a) => self.assign(a, env),
            // A loop consumes its own `break` and `continue`, but a `return` inside one
            // leaves the function and must travel through.
            Stmt::While(w) => self.while_loop(w, env),
            // Through `take!`: `return (if c { return 1; } else { 2 });` is legal, and
            // the inner `return` is the one that wins.
            Stmt::Return(r) => Ok(Flow::Return(take!(self, &r.value, env))),
            Stmt::Break { .. } => Ok(Flow::Break),
            Stmt::Continue { .. } => Ok(Flow::Continue),
            Stmt::Scope(sc) => {
                // Tasks run while the body does. The closing brace joins every one of
                // them, and joins them *before* propagating a failure of the body —
                // returning early would leave tasks running past the scope that promised
                // to contain them, which is the one guarantee this construct exists for.
                self.pending.push(Vec::new());
                let flow = self.block(&sc.body, env);
                let joined = Self::join_all(self.pending.pop().unwrap_or_default());
                // The body's failure wins: a task that failed because the body stopped
                // feeding it is a consequence, not the cause.
                let flow = flow?;
                joined?;
                Ok(match flow {
                    // A `scope` produces nothing; a `break` or `return` inside one still
                    // travels outward, after the join.
                    Flow::Fell(_) => Flow::Fell(None),
                    other => other,
                })
            }
            Stmt::Spawn(sp) => {
                let mut values = Vec::with_capacity(sp.args.len());
                for a in &sp.args {
                    values.push(take!(self, a, env));
                }
                // Checked before starting anything: a task with no scope to join it is
                // the one case where starting it would be unrecoverable. Unreachable —
                // the checker reports `N0320`.
                if self.pending.is_empty() {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::SPAWN_OUTSIDE_SCOPE,
                            sp.span,
                            "`spawn` reached the interpreter with no scope to join it",
                        )),
                    });
                }
                let task = self.start_task(&sp.callee, values, sp.span)?;
                if let Some(scope) = self.pending.last_mut() {
                    scope.push(task);
                }
                Ok(Flow::Fell(None))
            }
            Stmt::Expr(e) => {
                // The value is dropped; the flow is not. An `if` in statement position
                // may still `return` or `break`, and that has to travel outward.
                Ok(match self.eval(e, env)? {
                    Flow::Fell(_) => Flow::Fell(None),
                    other => other,
                })
            }
        }
    }

    /// A condition's value, once something has established it produced one.
    ///
    /// Not a `Bool` here means checking passed something it should not have, so this
    /// reports rather than panics: a checker bug should be readable.
    fn as_bool(&self, v: &Value, cond: &Expr) -> Result<bool, RuntimeError> {
        match v {
            Value::Bool(b) => Ok(*b),
            other => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::CONDITION_NOT_BOOL,
                    cond.span(),
                    format!("a condition must be `Bool`, but this is `{other}`"),
                )),
            }),
        }
    }

    /// An expression that was asked for a value and produced none.
    ///
    /// Checking has already established that one arrives, so this is a checker bug rather
    /// than a program's mistake — reported, not panicked on.
    fn no_value(&self, e: &Expr) -> RuntimeError {
        RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    e.span(),
                    "this produces no value, but one is needed here",
                )
                .help("a block's value is its final expression — one with no `;` after it"),
            ),
        }
    }

    /// Evaluate a built-in.
    ///
    /// Arity and argument types were established by checking, so a mismatch here is a
    /// compiler bug and is reported as one rather than assumed away. The failures that
    /// remain are the specified ones: an index outside a string (`N0405`) and an
    /// overflowing length, which cannot happen for a string that fits in memory but is
    /// handled rather than truncated.
    fn intrinsic(
        &mut self,
        i: Intrinsic,
        args: &[Value],
        span: nazm_span::Span,
    ) -> Result<Value, RuntimeError> {
        let bug = |what: &str| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    span,
                    format!("`{}` was called with {what}", i.name()),
                )
                .help("this is a bug in the interpreter; the checker should have caught it"),
            ),
        };
        // The *message* is the same sentence the compiled program writes, because the
        // two implementations reporting the same failure differently is a difference a
        // reader has to reconcile for no reason. What the interpreter knows and the
        // backend cannot — the actual index and length — goes in the help, where it
        // costs nothing and is not part of the contract.
        let out_of_bounds = |message: &str, detail: String| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(codes::INDEX_OUT_OF_BOUNDS, span, message).help(format!(
                    "{detail}. `Str` is indexed in bytes, from 0; `str_len` gives the \
                     length in bytes"
                )),
            ),
        };

        /// A length as an `Int`, or `None` on a host where it would not fit.
        fn index_fits(n: usize) -> Option<Value> {
            i64::try_from(n).ok().map(Value::Int)
        }

        // The path and the host's reason go in the help, not the message, so the two
        // implementations report the same sentence for the same failure — the native
        // side knows neither at compile time.
        let io_failed = |message: &str, path: &str, e: &std::io::Error| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(codes::IO_FAILED, span, message).help(format!("`{path}`: {e}")),
            ),
        };

        match (i, args) {
            (Intrinsic::StrLen, [Value::Str(s)]) => {
                // A length that does not fit in `Int` cannot arise from a string that
                // fits in memory on any 64-bit host, but saying so is cheaper than
                // discovering it.
                i64::try_from(s.len()).map_or_else(
                    |_| Err(bug("a string longer than `Int` can measure")),
                    |n| Ok(Value::Int(n)),
                )
            }
            (Intrinsic::StrByte, [Value::Str(s), Value::Int(at)]) => {
                let index = usize::try_from(*at).ok().filter(|n| *n < s.len());
                let Some(index) = index else {
                    return Err(out_of_bounds(
                        "this byte index is outside the `Str`",
                        format!("asked for byte {at} of a `Str` {} bytes long", s.len()),
                    ));
                };
                s.get(index).map_or_else(
                    || Err(bug("an index that passed its own bounds check")),
                    |b| Ok(Value::Int(i64::from(*b))),
                )
            }
            (Intrinsic::StrSlice, [Value::Str(s), Value::Int(from), Value::Int(to)]) => {
                let ok =
                    *from >= 0 && *to >= *from && usize::try_from(*to).is_ok_and(|t| t <= s.len());
                if !ok {
                    return Err(out_of_bounds(
                        "this slice is not inside the `Str`",
                        format!("asked for {from}..{to} of a `Str` {} bytes long", s.len()),
                    ));
                }
                let (a, b) = (
                    usize::try_from(*from).unwrap_or(0),
                    usize::try_from(*to).unwrap_or(0),
                );
                // Bytes, not characters: slicing is allowed to land inside a multi-byte
                // sequence, and `docs/spec.md` says what that produces. Rust's `String`
                // will not hold invalid UTF-8, so the bytes are carried as lossy text
                // here — the *native* representation has no such constraint, and the
                // specification is written to what both can do.
                Ok(Value::Str(s.get(a..b).unwrap_or(&[]).to_vec()))
            }
            (Intrinsic::StrConcat, [Value::Str(a), Value::Str(b)]) => {
                let mut out = Vec::with_capacity(a.len() + b.len());
                out.extend_from_slice(a);
                out.extend_from_slice(b);
                Ok(Value::Str(out))
            }
            (Intrinsic::IntToStr, [Value::Int(n)]) => Ok(Value::Str(n.to_string().into_bytes())),
            (Intrinsic::IntsNew, []) => Ok(Value::Ints(Arc::default())),
            (Intrinsic::StrsNew, []) => Ok(Value::Strs(Arc::default())),
            (Intrinsic::IntsLen, [Value::Ints(a)]) => seq(a)
                .and_then(|a| index_fits(a.len()))
                .map_or_else(|| Err(bug("a sequence that could not be read")), Ok),
            (Intrinsic::StrsLen, [Value::Strs(a)]) => seq(a)
                .and_then(|a| index_fits(a.len()))
                .map_or_else(|| Err(bug("a sequence that could not be read")), Ok),
            (Intrinsic::IntsGet, [Value::Ints(a), Value::Int(at)]) => {
                let Some(a) = seq(a) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let Some(v) = usize::try_from(*at).ok().and_then(|i| a.get(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked for item {at} of an `Ints` holding {}", a.len()),
                    ));
                };
                Ok(Value::Int(*v))
            }
            (Intrinsic::StrsGet, [Value::Strs(a), Value::Int(at)]) => {
                let Some(a) = seq(a) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let Some(v) = usize::try_from(*at).ok().and_then(|i| a.get(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked for item {at} of a `Strs` holding {}", a.len()),
                    ));
                };
                Ok(Value::Str(v.clone()))
            }
            (Intrinsic::IntsSet, [Value::Ints(a), Value::Int(at), Value::Int(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                let len = a.len();
                let Some(slot) = usize::try_from(*at).ok().and_then(|i| a.get_mut(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked to write item {at} of an `Ints` holding {len}"),
                    ));
                };
                *slot = *v;
                Ok(Value::Int(*at))
            }
            (Intrinsic::StrsSet, [Value::Strs(a), Value::Int(at), Value::Str(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                let len = a.len();
                let Some(slot) = usize::try_from(*at).ok().and_then(|i| a.get_mut(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the sequence",
                        format!("asked to write item {at} of a `Strs` holding {len}"),
                    ));
                };
                slot.clone_from(v);
                Ok(Value::Int(*at))
            }
            (Intrinsic::IntsPush, [Value::Ints(a), Value::Int(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.push(*v);
                index_fits(a.len())
                    .map_or_else(|| Err(bug("a sequence longer than `Int` can measure")), Ok)
            }
            (Intrinsic::StrsPush, [Value::Strs(a), Value::Str(v)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.push(v.clone());
                index_fits(a.len())
                    .map_or_else(|| Err(bug("a sequence longer than `Int` can measure")), Ok)
            }
            (Intrinsic::IntsPop, [Value::Ints(a)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.pop().map_or_else(
                    || {
                        Err(out_of_bounds(
                            "this sequence is empty",
                            "asked to remove the last item of an empty `Ints`".to_owned(),
                        ))
                    },
                    |v| Ok(Value::Int(v)),
                )
            }
            (Intrinsic::ChanNew, [Value::Int(capacity)]) => {
                let Ok(capacity) = usize::try_from(*capacity).map(|c| c.max(0)) else {
                    return Err(out_of_bounds(
                        "a channel needs a capacity of at least 1",
                        format!("asked for a capacity of {capacity}"),
                    ));
                };
                if capacity == 0 {
                    return Err(out_of_bounds(
                        "a channel needs a capacity of at least 1",
                        "asked for a capacity of 0".to_owned(),
                    ));
                }
                Ok(Value::Chan(Arc::new(Channel::new(capacity))))
            }
            (Intrinsic::ChanSend, [Value::Chan(c), Value::Int(value)]) => c
                .send(*value)
                .map_or_else(|| Err(bug("a channel that could not be locked")), |sent| {
                    Ok(Value::Bool(sent))
                }),
            (Intrinsic::ChanRecv, [Value::Chan(c), Value::Ints(out)]) => {
                let Some(taken) = c.recv() else {
                    return Err(bug("a channel that could not be locked"));
                };
                match taken {
                    Some(v) => {
                        let Some(mut out) = seq(out) else {
                            return Err(bug("a sequence that could not be written"));
                        };
                        out.push(v);
                        Ok(Value::Bool(true))
                    }
                    None => Ok(Value::Bool(false)),
                }
            }
            (Intrinsic::ChanClose, [Value::Chan(c)]) => c
                .close()
                .map_or_else(|| Err(bug("a channel that could not be locked")), |()| {
                    Ok(Value::Int(0))
                }),
            (Intrinsic::StrJoin, [Value::Strs(parts), Value::Str(sep)]) => {
                let Some(parts) = seq(parts) else {
                    return Err(bug("a sequence that could not be read"));
                };
                let total: usize = parts.iter().map(Vec::len).sum::<usize>()
                    + sep.len() * parts.len().saturating_sub(1);
                let mut out = Vec::with_capacity(total);
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.extend_from_slice(sep);
                    }
                    out.extend_from_slice(part);
                }
                Ok(Value::Str(out))
            }
            (Intrinsic::StrsPop, [Value::Strs(a)]) => {
                let Some(mut a) = seq(a) else {
                    return Err(bug("a sequence that could not be written"));
                };
                a.pop().map_or_else(
                    || {
                        Err(out_of_bounds(
                            "this sequence is empty",
                            "asked to remove the last item of an empty `Strs`".to_owned(),
                        ))
                    },
                    |v| Ok(Value::Str(v)),
                )
            }

            // --- the outside world ---------------------------------------------
            (Intrinsic::ArgsCount, []) => index_fits(self.args.len())
                .map_or_else(|| Err(bug("more arguments than `Int` can count")), Ok),
            (Intrinsic::Arg, [Value::Int(at)]) => {
                let Some(a) = usize::try_from(*at).ok().and_then(|i| self.args.get(i)) else {
                    return Err(out_of_bounds(
                        "this index is outside the arguments",
                        format!("asked for argument {at} of {} given", self.args.len()),
                    ));
                };
                Ok(Value::Str(a.clone().into_bytes()))
            }
            (Intrinsic::ReadFile, [Value::Str(path)]) => {
                let name = String::from_utf8_lossy(path).into_owned();
                match std::fs::read(&name) {
                    Ok(bytes) => Ok(Value::Str(bytes)),
                    Err(e) => Err(io_failed("this file could not be read", &name, &e)),
                }
            }
            (Intrinsic::WriteFile, [Value::Str(path), Value::Str(data)]) => {
                let name = String::from_utf8_lossy(path).into_owned();
                match std::fs::write(&name, data) {
                    Ok(()) => index_fits(data.len())
                        .map_or_else(|| Err(bug("a file longer than `Int` can measure")), Ok),
                    Err(e) => Err(io_failed("this file could not be written", &name, &e)),
                }
            }
            (Intrinsic::FileExists, [Value::Str(path)]) => {
                let name = String::from_utf8_lossy(path).into_owned();
                // Openable for reading, which is what the native side tests. "Exists"
                // and "can be opened" differ for a directory or an unreadable file, and
                // the built-in is named for the question a program actually asks.
                Ok(Value::Bool(std::fs::File::open(&name).is_ok()))
            }
            (Intrinsic::Print, [Value::Str(data)]) | (Intrinsic::EPrint, [Value::Str(data)]) => {
                use std::io::Write as _;
                let written = if i == Intrinsic::Print {
                    std::io::stdout().write_all(data).map(|()| data.len())
                } else {
                    std::io::stderr().write_all(data).map(|()| data.len())
                };
                match written {
                    Ok(n) => index_fits(n)
                        .map_or_else(|| Err(bug("more bytes than `Int` can count")), Ok),
                    Err(e) => Err(io_failed("this output could not be written", "-", &e)),
                }
            }
            (Intrinsic::ExitWith, [Value::Int(code)]) => {
                // Ends the process, here as natively. The interpreter runs on a thread
                // it owns, and `exit` from a thread ends the whole process — which is
                // the point: `exit_with` means the program is over, not that this
                // evaluation is.
                use std::io::Write as _;
                let _ = std::io::stdout().flush();
                std::process::exit(i32::try_from(*code).unwrap_or(1))
            }
            (Intrinsic::StrFromByte, [Value::Int(b)]) => {
                let Ok(byte) = u8::try_from(*b) else {
                    return Err(out_of_bounds(
                        "this is not a byte value",
                        format!("asked for byte {b}, which is outside 0..=255"),
                    ));
                };
                Ok(Value::Str(vec![byte]))
            }
            _ => Err(bug("arguments of the wrong shape")),
        }
    }

    /// Start one task and hand back the handle its scope will join.
    ///
    /// One OS thread per task, and `docs/spec.md` says so rather than implying anything
    /// else. Each task gets its **own** interpreter over the same program: the state
    /// shared between tasks is exactly the channels they were handed, which is what the
    /// capture rule is for.
    ///
    /// Each task gets its own iteration and depth budgets rather than a share of the
    /// parent's. Splitting them would make a program's own limit depend on how many
    /// tasks happened to be running, which is not something a person can reason about.
    fn start_task(
        &self,
        callee: &str,
        args: Vec<Value>,
        span: nazm_span::Span,
    ) -> Result<JoinHandle<Result<(), RuntimeError>>, RuntimeError> {
        let index = self
            .functions
            .get(callee)
            .copied()
            .ok_or_else(|| RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::UNDEFINED_FUNCTION,
                    span,
                    format!("`{callee}` is not a function"),
                )),
            })?;
        let program = Arc::clone(&self.program);
        let functions = Arc::clone(&self.functions);
        let max_depth = self.max_depth;
        let iterations_left = self.iterations_left;
        let name = callee.to_string();

        std::thread::Builder::new()
            .name(name.clone())
            // The same reservation the main evaluation thread gets, for the same reason:
            // the depth limit has to be reached before the stack is, or a deep recursion
            // inside a task crashes the process with no diagnostic at all.
            .stack_size(stack_for(max_depth))
            .spawn(move || {
                let mut task = Self {
                    program: Arc::clone(&program),
                    functions,
                    pending: Vec::new(),
                    depth: 0,
                    max_depth,
                    iterations_left,
                    // A task has no arguments of its own: `arg(i)` is the program's
                    // command line, and a task is not a program.
                    args: Vec::new(),
                };
                let f = &program.functions[index];
                let mut env: Env = vec![HashMap::new()];
                if let Some(frame) = env.last_mut() {
                    for (p, v) in f.params.iter().zip(args) {
                        frame.insert(p.name.clone(), v);
                    }
                }
                Self::body_of(f).and_then(|body| task.call_body(body, &mut env, &name).map(|_| ()))
            })
            .map_err(|e| RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(
                        codes::INTERPRETER_UNAVAILABLE,
                        span,
                        format!("could not start a task for `{callee}`: {e}"),
                    )
                    .help(
                        "each task is an operating-system thread; this is a limit of the \
                         machine, not of the program",
                    ),
                ),
            })
    }

    /// Wait for every task of one scope.
    ///
    /// Every handle is joined even after one has failed. Returning at the first failure
    /// would leave the rest running, and a scope that does not contain its tasks is not
    /// a scope.
    ///
    /// A task that fails ends the whole program, because that is what the specification
    /// promises — tasks do not have private failure modes. The first failure is reported
    /// and the rest are treated as consequences of it.
    fn join_all(tasks: Vec<JoinHandle<Result<(), RuntimeError>>>) -> Result<(), RuntimeError> {
        let mut first: Option<RuntimeError> = None;
        for handle in tasks {
            let outcome = match handle.join() {
                Ok(outcome) => outcome,
                // A panic is an interpreter bug, not a program error, but it still has to
                // come out as a diagnostic: resuming the unwind here would lose the other
                // tasks still waiting to be joined.
                Err(_) => Err(RuntimeError {
                    diagnostic: Box::new(
                        Diagnostic::error(
                            codes::INTERPRETER_UNAVAILABLE,
                            nazm_span::Span::new(0, 0),
                            "a task ended abnormally",
                        )
                        .help("this is a bug in the interpreter"),
                    ),
                }),
            };
            if let Err(e) = outcome {
                first.get_or_insert(e);
            }
        }
        match first {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Assign to the innermost binding of that name.
    ///
    /// Innermost, so a loop body assigning to a variable declared outside it updates that
    /// one rather than creating a shadow that vanishes each iteration — which would make
    /// every accumulator silently do nothing.
    fn assign(&mut self, a: &Assign, env: &mut Env) -> Result<Flow, RuntimeError> {
        let v = take!(self, &a.value, env);
        for frame in env.iter_mut().rev() {
            if let Some(slot) = frame.get_mut(&a.name) {
                *slot = v;
                return Ok(Flow::Fell(None));
            }
        }
        Err(RuntimeError {
            diagnostic: Box::new(Diagnostic::error(
                codes::UNDEFINED_VARIABLE,
                a.name_span,
                format!("`{}` is not defined", a.name),
            )),
        })
    }

    fn while_loop(&mut self, w: &While, env: &mut Env) -> Result<Flow, RuntimeError> {
        loop {
            // `take!`, because a condition is an expression like any other: `while (if c
            // { return 1; } else { true })` leaves the function from the condition.
            let c = take!(self, &w.cond, env);
            if !self.as_bool(&c, &w.cond)? {
                return Ok(Flow::Fell(None));
            }

            let Some(left) = self.iterations_left.checked_sub(1) else {
                return Err(RuntimeError {
                    diagnostic: Box::new(
                        Diagnostic::error(
                            codes::ITERATION_LIMIT,
                            w.span,
                            "this program exceeded the interpreter's iteration limit",
                        )
                        .help(
                            "Nazm does not bound iteration; `nazm run` does, so a \
                         non-terminating program reports instead of hanging. Raise it \
                         with `--max-iterations`, or check the loop's condition ever \
                         becomes false",
                        ),
                    ),
                });
            };
            self.iterations_left = left;

            match self.block(&w.body, env)? {
                Flow::Fell(_) | Flow::Continue => {}
                Flow::Break => return Ok(Flow::Fell(None)),
                // A `return` inside a loop leaves the function, not just the loop.
                r @ Flow::Return(_) => return Ok(r),
            }
        }
    }

    /// Evaluate an expression, keeping both its value and any control flow it produces.
    ///
    /// The two cannot be separated, which is why this returns a [`Flow`] rather than a
    /// [`Value`]. `if c { 1 } else { continue; }` in tail position both *is* the block's
    /// value on one path and leaves the loop on the other; `let x = if c { return 1; }
    /// else { 2 };` is the same shape one level in. Every sub-expression here takes its
    /// operands through `take!`, so a flow raised anywhere inside travels straight out
    /// instead of being mistaken for a value that never arrives.
    fn eval(&mut self, e: &Expr, env: &mut Env) -> Result<Flow, RuntimeError> {
        let value = match e {
            Expr::Int { value, .. } => Value::Int(*value),
            Expr::Bool { value, .. } => Value::Bool(*value),
            // A literal's bytes are the source's bytes, which are valid UTF-8 because
            // the source is. Nothing downstream may assume that of every `Str`.
            Expr::Str { value, .. } => Value::Str(value.clone().into_bytes()),

            Expr::Var { name, span } => env
                .iter()
                .rev()
                .find_map(|f| f.get(name).cloned())
                .ok_or_else(|| RuntimeError {
                    // Unreachable if checking ran. A diagnostic rather than a panic, so a
                    // checker bug surfaces as a report.
                    diagnostic: Box::new(Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not defined"),
                    )),
                })?,

            Expr::Neg {
                operand, op_span, ..
            } => match take!(self, operand, env) {
                // `-i64::MIN` has no representable result, so negation overflows exactly
                // once — and it is the value people reach for when testing edges.
                Value::Int(v) => v
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or_else(|| RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::INTEGER_OVERFLOW,
                            *op_span,
                            "this negation overflows `Int`",
                        )),
                    })?,
                other => {
                    return Err(RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::INVALID_OPERANDS,
                            *op_span,
                            format!("`-` applies to `Int`, but this is `{other}`"),
                        )),
                    })
                }
            },

            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => {
                // Left to right, as the spec fixes — and `take!` preserves that: a flow
                // raised by the left operand leaves before the right one is evaluated.
                let l = take!(self, lhs, env);
                let r = take!(self, rhs, env);
                self.binary(*op, &l, &r, *op_span)?
            }

            Expr::Call {
                callee, args, span, ..
            } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(take!(self, a, env));
                }
                // Built-ins are resolved before user functions, which costs nothing:
                // the checker rejects a program that defines one, so the two sets are
                // disjoint and the order is a statement of intent rather than a rule.
                if let Some(i) = Intrinsic::by_name(callee) {
                    return Ok(Flow::Fell(Some(self.intrinsic(i, &values, *span)?)));
                }

                let program = Arc::clone(&self.program);
                let f = self
                    .functions
                    .get(callee)
                    .copied()
                    .map(|i| &program.functions[i])
                    .ok_or_else(|| RuntimeError {
                        diagnostic: Box::new(Diagnostic::error(
                            codes::UNDEFINED_FUNCTION,
                            *span,
                            format!("`{callee}` is not a function"),
                        )),
                    })?;

                self.depth += 1;
                if self.depth > self.max_depth {
                    self.depth -= 1;
                    return Err(RuntimeError {
                        diagnostic: Box::new(
                            Diagnostic::error(
                                codes::CALL_DEPTH_LIMIT,
                                *span,
                                format!("`{callee}` recursed deeper than {}", self.max_depth),
                            )
                            .help(
                                "the interpreter bounds call depth so runaway recursion \
                             reports instead of exhausting the stack, which would crash \
                             without a diagnostic. Raise it with `--max-depth`, or check \
                             the recursion has a base case",
                            ),
                        ),
                    });
                }

                // A fresh environment: a function sees its parameters and nothing from
                // the caller. Lexical scope, not dynamic.
                let mut call_env: Env = vec![HashMap::new()];
                if let Some(frame) = call_env.last_mut() {
                    for (p, v) in f.params.iter().zip(values) {
                        frame.insert(p.name.clone(), v);
                    }
                }
                let out = self.call_body(Self::body_of(f)?, &mut call_env, callee);
                self.depth -= 1;
                // A call absorbs its callee's `return`: that is what makes it a call and
                // not a jump. Nothing propagates outward from here but a genuine error.
                out?
            }

            // Both return their flow directly rather than a value. An `if` with no `else`
            // produces nothing, and whether that is allowed is the caller's question —
            // `take!` reports it where a value was needed, and `stmt` drops it where one
            // was not.
            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let c = take!(self, cond, env);
                return if self.as_bool(&c, cond)? {
                    self.block(then_block, env)
                } else if let Some(b) = else_block {
                    self.block(b, env)
                } else {
                    Ok(Flow::Fell(None))
                };
            }

            Expr::Block(b) => return self.block(b, env),
        };
        Ok(Flow::Fell(Some(value)))
    }

    /// The body of a function that is about to run.
    ///
    /// A missing body means the parser reported an error, and a program with errors is
    /// never evaluated — so this is unreachable. It reports rather than panicking,
    /// because a bug in that reasoning should still produce something readable.
    fn body_of(f: &Function) -> Result<&Block, RuntimeError> {
        f.body.as_ref().ok_or_else(|| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    f.name_span,
                    format!("`{}` has no body to run", f.name),
                )
                .help("its body did not parse; fix the reported syntax error first"),
            ),
        })
    }

    fn binary(
        &self,
        op: BinOp,
        l: &Value,
        r: &Value,
        op_span: nazm_span::Span,
    ) -> Result<Value, RuntimeError> {
        use Value::{Bool, Int};

        let overflow = |what: &str| {
            RuntimeError {
            diagnostic: Box::new(Diagnostic::error(
                codes::INTEGER_OVERFLOW,
                op_span,
                format!("this {what} overflows `Int`"),
            )
            .help(
                "`Int` is a 64-bit signed integer; overflow stops the program rather than wrapping",
            )),
        }
        };

        match (op, l, r) {
            (BinOp::Add, Int(a), Int(b)) => a
                .checked_add(*b)
                .map(Int)
                .ok_or_else(|| overflow("addition")),
            (BinOp::Sub, Int(a), Int(b)) => a
                .checked_sub(*b)
                .map(Int)
                .ok_or_else(|| overflow("subtraction")),
            (BinOp::Mul, Int(a), Int(b)) => a
                .checked_mul(*b)
                .map(Int)
                .ok_or_else(|| overflow("multiplication")),

            (BinOp::Div | BinOp::Rem, Int(_), Int(0)) => Err(RuntimeError {
                diagnostic: Box::new(
                    Diagnostic::error(codes::DIVISION_BY_ZERO, op_span, "division by zero")
                        .help("guard the divisor, for example `if d != 0 { n / d } else { 0 }`"),
                ),
            }),
            // `checked_div` also catches i64::MIN / -1, which overflows rather than
            // dividing — the one division that is not a zero check.
            (BinOp::Div, Int(a), Int(b)) => a
                .checked_div(*b)
                .map(Int)
                .ok_or_else(|| overflow("division")),
            // `i64::MIN % -1` is mathematically 0 and 0 is representable, so nothing
            // overflows. `checked_rem` says `None` anyway, because the hardware
            // instruction that computes it faults on the *quotient*. Inheriting that
            // would make the rule "`%` fails on a zero divisor, and also on one exact
            // pair of operands" — a wart an agent would have to memorise. `wrapping_rem`
            // is the correct answer here, and the zero divisor is already handled above.
            (BinOp::Rem, Int(a), Int(b)) => Ok(Int(a.wrapping_rem(*b))),

            (BinOp::Lt, Int(a), Int(b)) => Ok(Bool(a < b)),
            (BinOp::LtEq, Int(a), Int(b)) => Ok(Bool(a <= b)),
            (BinOp::Gt, Int(a), Int(b)) => Ok(Bool(a > b)),
            (BinOp::GtEq, Int(a), Int(b)) => Ok(Bool(a >= b)),

            (BinOp::Eq, a, b) => Ok(Bool(a == b)),
            (BinOp::NotEq, a, b) => Ok(Bool(a != b)),

            // Unreachable after checking. A diagnostic rather than a panic, for the same
            // reason as the undefined-variable arm above.
            (op, a, b) => Err(RuntimeError {
                diagnostic: Box::new(Diagnostic::error(
                    codes::INVALID_OPERANDS,
                    op_span,
                    format!(
                        "`{}` does not apply to `{}` and `{}`",
                        op.symbol(),
                        type_name(a),
                        type_name(b)
                    ),
                )),
            }),
        }
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "Int",
        Value::Bool(_) => "Bool",
        Value::Str(_) => "Str",
        Value::Ints(_) => "Ints",
        Value::Strs(_) => "Strs",
        Value::Chan(_) => "Chan",
    }
}
