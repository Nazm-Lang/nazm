//! Tree-walking evaluator.
//!
//! Runs only after checking has passed, so the shapes it handles are the ones the checker
//! guaranteed. What remains at runtime is the two failures a type system cannot rule out:
//! integer overflow and division by zero. Both stop the program with a diagnostic that
//! points at the operator, and both behave identically in every build — a bug that
//! reproduces only in one build configuration is the worst kind to be handed.

use std::collections::HashMap;

use nazm_diag::{codes, Diagnostic};
use nazm_syntax::{BinOp, Block, Expr, Function, Program};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Str(v) => write!(f, "{v}"),
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
    pub diagnostic: Diagnostic,
}

type Env = Vec<HashMap<String, Value>>;

#[derive(Debug)]
pub struct Interpreter<'a> {
    functions: HashMap<String, &'a Function>,
    /// Bounded so an accidental infinite recursion reports rather than aborting the
    /// process with a stack overflow, which produces no diagnostic at all.
    depth: usize,
    max_depth: usize,
}

#[allow(clippy::result_large_err)]
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
            max_depth: 512,
        }
    }

    /// Evaluate `main` and return its value.
    pub fn run_main(&mut self) -> Result<Value, RuntimeError> {
        let main = self
            .functions
            .get("main")
            .copied()
            .ok_or_else(|| RuntimeError {
                diagnostic: Diagnostic::error(
                    codes::MISSING_MAIN,
                    nazm_span::Span::new(0, 0),
                    "this program has no `main` function",
                ),
            })?;
        let mut env: Env = vec![HashMap::new()];
        self.block(&main.body, &mut env)
    }

    fn block(&mut self, b: &Block, env: &mut Env) -> Result<Value, RuntimeError> {
        env.push(HashMap::new());
        for l in &b.lets {
            let v = self.expr(&l.value, env)?;
            if let Some(frame) = env.last_mut() {
                frame.insert(l.name.clone(), v);
            }
        }
        let out = self.expr(&b.tail, env);
        env.pop();
        out
    }

    fn expr(&mut self, e: &Expr, env: &mut Env) -> Result<Value, RuntimeError> {
        match e {
            Expr::Int { value, .. } => Ok(Value::Int(*value)),
            Expr::Bool { value, .. } => Ok(Value::Bool(*value)),
            Expr::Str { value, .. } => Ok(Value::Str(value.clone())),

            Expr::Var { name, span } => env
                .iter()
                .rev()
                .find_map(|f| f.get(name).cloned())
                .ok_or_else(|| RuntimeError {
                    // Unreachable if checking ran. Kept as a diagnostic rather than a
                    // panic so a checker bug surfaces as a report, not a crash.
                    diagnostic: Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not defined"),
                    ),
                }),

            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => {
                // Left to right, as the spec fixes. v0.1 has no side effects for the
                // order to be observable through, which is exactly why fixing it now is
                // free and changing it later would not be.
                let l = self.expr(lhs, env)?;
                let r = self.expr(rhs, env)?;
                self.binary(*op, &l, &r, *op_span)
            }

            Expr::Call {
                callee, args, span, ..
            } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.expr(a, env)?);
                }
                let f = self
                    .functions
                    .get(callee)
                    .copied()
                    .ok_or_else(|| RuntimeError {
                        diagnostic: Diagnostic::error(
                            codes::UNDEFINED_FUNCTION,
                            *span,
                            format!("`{callee}` is not a function"),
                        ),
                    })?;

                self.depth += 1;
                if self.depth > self.max_depth {
                    self.depth -= 1;
                    return Err(RuntimeError {
                        diagnostic: Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            *span,
                            format!("`{callee}` recursed more than {} times", self.max_depth),
                        )
                        .help(
                            "v0.1 has no loops, so deep recursion is usually a missing base case",
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
                let out = self.block(&f.body, &mut call_env);
                self.depth -= 1;
                out
            }

            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => match self.expr(cond, env)? {
                Value::Bool(true) => self.block(then_block, env),
                Value::Bool(false) => self.block(else_block, env),
                other => Err(RuntimeError {
                    diagnostic: Diagnostic::error(
                        codes::CONDITION_NOT_BOOL,
                        cond.span(),
                        format!("an `if` condition must be `Bool`, but this is `{other}`"),
                    ),
                }),
            },

            Expr::Neg {
                operand, op_span, ..
            } => match self.expr(operand, env)? {
                // `-i64::MIN` has no representable result, so negation overflows exactly
                // once and it is the value people reach for when testing edges.
                Value::Int(v) => v.checked_neg().map(Value::Int).ok_or_else(|| RuntimeError {
                    diagnostic: Diagnostic::error(
                        codes::INTEGER_OVERFLOW,
                        *op_span,
                        "this negation overflows `Int`",
                    ),
                }),
                other => Err(RuntimeError {
                    diagnostic: Diagnostic::error(
                        codes::INVALID_OPERANDS,
                        *op_span,
                        format!("`-` applies to `Int`, but this is `{other}`"),
                    ),
                }),
            },

            Expr::Block(b) => self.block(b, env),
        }
    }

    fn binary(
        &self,
        op: BinOp,
        l: &Value,
        r: &Value,
        op_span: nazm_span::Span,
    ) -> Result<Value, RuntimeError> {
        use Value::{Bool, Int};

        let overflow = |what: &str| RuntimeError {
            diagnostic: Diagnostic::error(
                codes::INTEGER_OVERFLOW,
                op_span,
                format!("this {what} overflows `Int`"),
            )
            .help(
                "`Int` is a 64-bit signed integer; overflow stops the program rather than wrapping",
            ),
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
                diagnostic: Diagnostic::error(codes::DIVISION_BY_ZERO, op_span, "division by zero")
                    .help("guard the divisor, for example `if d != 0 { n / d } else { 0 }`"),
            }),
            // `checked_div` also catches i64::MIN / -1, which overflows rather than
            // dividing — the one division that is not a zero check.
            (BinOp::Div, Int(a), Int(b)) => a
                .checked_div(*b)
                .map(Int)
                .ok_or_else(|| overflow("division")),
            (BinOp::Rem, Int(a), Int(b)) => a
                .checked_rem(*b)
                .map(Int)
                .ok_or_else(|| overflow("remainder")),

            (BinOp::Lt, Int(a), Int(b)) => Ok(Bool(a < b)),
            (BinOp::LtEq, Int(a), Int(b)) => Ok(Bool(a <= b)),
            (BinOp::Gt, Int(a), Int(b)) => Ok(Bool(a > b)),
            (BinOp::GtEq, Int(a), Int(b)) => Ok(Bool(a >= b)),

            (BinOp::Eq, a, b) => Ok(Bool(a == b)),
            (BinOp::NotEq, a, b) => Ok(Bool(a != b)),

            // Unreachable after checking. A diagnostic rather than a panic, for the same
            // reason as the undefined-variable arm above.
            (op, a, b) => Err(RuntimeError {
                diagnostic: Diagnostic::error(
                    codes::INVALID_OPERANDS,
                    op_span,
                    format!(
                        "`{}` does not apply to `{}` and `{}`",
                        op.symbol(),
                        type_name(a),
                        type_name(b)
                    ),
                ),
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
