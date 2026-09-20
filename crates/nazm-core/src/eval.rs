//! Tree-walking evaluator.
//!
//! Runs only after checking has passed, so the shapes it handles are the ones the checker
//! guaranteed. What remains at runtime is the two failures a type system cannot rule out:
//! integer overflow and division by zero. Both stop the program with a diagnostic that
//! points at the operator, and both behave identically in every build — a bug that
//! reproduces only in one build configuration is the worst kind to be handed.

use std::collections::HashMap;

use nazm_diag::{codes, Diagnostic};
use nazm_syntax::{Assign, BinOp, Block, Expr, Function, Program, Stmt, While};

use crate::intrinsic::Intrinsic;

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug)]
pub struct Interpreter<'a> {
    functions: HashMap<String, &'a Function>,
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
}

impl<'a> Interpreter<'a> {
    #[must_use]
    pub fn new(program: &'a Program) -> Self {
        let functions = program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f))
            .collect();
        Self {
            functions,
            depth: 0,
            max_depth: DEFAULT_MAX_DEPTH,
            iterations_left: DEFAULT_MAX_ITERATIONS,
        }
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
        let main = self
            .functions
            .get("main")
            .copied()
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
        let out_of_bounds = |detail: String| RuntimeError {
            diagnostic: Box::new(
                Diagnostic::error(codes::INDEX_OUT_OF_BOUNDS, span, detail).help(
                    "`Str` is indexed in bytes, from 0; `str_len` gives the length in \
                     bytes",
                ),
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
                    return Err(out_of_bounds(format!(
                        "byte {at} is outside a `Str` of length {}",
                        s.len()
                    )));
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
                    return Err(out_of_bounds(format!(
                        "the slice {from}..{to} is not inside a `Str` of length {}",
                        s.len()
                    )));
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
            _ => Err(bug("arguments of the wrong shape")),
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

                let f = self
                    .functions
                    .get(callee)
                    .copied()
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
    fn body_of(f: &'a Function) -> Result<&'a Block, RuntimeError> {
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
    }
}
