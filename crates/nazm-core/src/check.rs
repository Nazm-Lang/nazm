//! Type and scope checking.
//!
//! Runs before evaluation and reports **every** error it can, not just the first: an
//! agent that fixes one error and recompiles to find the next spends a turn per mistake,
//! and the whole argument for this language rests on not doing that.
//!
//! Where an error makes a subexpression's type unknowable, checking continues with the
//! *expected* type rather than stopping. That produces one honest error instead of a
//! cascade of invented ones.

use std::collections::HashMap;

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;
use nazm_syntax::{BinOp, Block, Expr, Function, Program};

use crate::types::Type;

#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<Type>,
    pub ret: Type,
    pub name_span: Span,
}

/// Lexical scopes. Innermost last.
#[derive(Debug, Default)]
struct Scopes {
    frames: Vec<HashMap<String, Type>>,
}

impl Scopes {
    fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.frames.pop();
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        self.frames.iter().rev().find_map(|f| f.get(name).copied())
    }

    /// Whether the name is already bound **in the innermost scope**.
    ///
    /// Only the innermost: an inner scope may shadow an outer one, because there the
    /// reader can see the boundary. Two `let`s of the same name side by side is what
    /// makes a later mention ambiguous. See `docs/spec.md`.
    fn defined_here(&self, name: &str) -> bool {
        self.frames.last().is_some_and(|f| f.contains_key(name))
    }

    fn define(&mut self, name: &str, ty: Type) {
        if let Some(f) = self.frames.last_mut() {
            f.insert(name.to_owned(), ty);
        }
    }
}

/// The facts `check_binary` needs about one operator application.
///
/// A struct rather than six positional arguments: at the call site `(*op, lt, rt,
/// *op_span, lhs.span(), rhs.span())` is four spans and two types in an order nothing
/// enforces, and swapping two of them compiles.
#[derive(Debug, Clone, Copy)]
struct BinaryCheck {
    op: BinOp,
    lt: Type,
    rt: Type,
    op_span: Span,
    lhs_span: Span,
    rhs_span: Span,
}

#[derive(Debug)]
pub struct Checker {
    functions: HashMap<String, Signature>,
    scopes: Scopes,
    errors: Vec<Diagnostic>,
}

/// Check a program. Returns the signatures and every diagnostic found.
#[must_use]
pub fn check(program: &Program) -> (HashMap<String, Signature>, Vec<Diagnostic>) {
    let mut c = Checker {
        functions: HashMap::new(),
        scopes: Scopes::default(),
        errors: Vec::new(),
    };

    // Signatures first, so a call to a function defined later in the file resolves.
    for f in &program.functions {
        c.collect_signature(f);
    }

    if !c.functions.contains_key("main") {
        c.errors.push(
            Diagnostic::error(
                codes::MISSING_MAIN,
                Span::new(0, 0),
                "this program has no `main` function",
            )
            .help("`nazm run` evaluates `fn main() -> Int` and prints its value"),
        );
    }

    for f in &program.functions {
        c.check_function(f);
    }

    (c.functions, c.errors)
}

impl Checker {
    fn resolve_type(&mut self, name: &str, span: Span) -> Type {
        match Type::from_name(name) {
            Some(t) => t,
            None => {
                self.errors.push(
                    Diagnostic::error(codes::UNKNOWN_TYPE, span, format!("`{name}` is not a type"))
                        .help(format!("v0.1 has {}", Type::all_names())),
                );
                // Continue as `Int` rather than abandoning the function: one unknown type
                // should not suppress every other error in the body.
                Type::Int
            }
        }
    }

    fn collect_signature(&mut self, f: &Function) {
        let params = f
            .params
            .iter()
            .map(|p| self.resolve_type(&p.ty.name, p.ty.span))
            .collect();
        let ret = self.resolve_type(&f.return_type.name, f.return_type.span);

        if let Some(existing) = self.functions.get(&f.name) {
            self.errors.push(
                Diagnostic::error(
                    codes::DUPLICATE_FUNCTION,
                    f.name_span,
                    format!("`{}` is defined more than once", f.name),
                )
                .secondary(existing.name_span, "first defined here"),
            );
            return;
        }
        self.functions.insert(
            f.name.clone(),
            Signature {
                params,
                ret,
                name_span: f.name_span,
            },
        );
    }

    fn check_function(&mut self, f: &Function) {
        self.scopes.push();
        for p in &f.params {
            let ty = self.resolve_type(&p.ty.name, p.ty.span);
            if self.scopes.defined_here(&p.name) {
                self.errors.push(Diagnostic::error(
                    codes::ALREADY_DEFINED_IN_SCOPE,
                    p.span,
                    format!("`{}` is listed twice in this parameter list", p.name),
                ));
            }
            self.scopes.define(&p.name, ty);
        }

        let declared = self.resolve_type(&f.return_type.name, f.return_type.span);
        let actual = self.check_block(&f.body, Some(declared));

        if actual != declared {
            self.errors.push(
                Diagnostic::error(
                    codes::TYPE_MISMATCH,
                    f.body.tail.span(),
                    format!(
                        "`{}` returns `{actual}`, but its signature says `{declared}`",
                        f.name
                    ),
                )
                .secondary(f.return_type.span, format!("declared `{declared}` here"))
                .help("a function's value is its final expression"),
            );
        }
        self.scopes.pop();
    }

    fn check_block(&mut self, b: &Block, expected: Option<Type>) -> Type {
        self.scopes.push();
        for l in &b.lets {
            let ty = self.check_expr(&l.value, None);
            if self.scopes.defined_here(&l.name) {
                self.errors.push(
                    Diagnostic::error(
                        codes::ALREADY_DEFINED_IN_SCOPE,
                        l.name_span,
                        format!("`{}` is already defined in this scope", l.name),
                    )
                    .help(
                        "Nazm allows shadowing in a nested scope but not in the same one, \
                         so a later mention of a name is never ambiguous",
                    ),
                );
            }
            self.scopes.define(&l.name, ty);
        }
        let ty = self.check_expr(&b.tail, expected);
        self.scopes.pop();
        ty
    }

    /// Check an expression, returning its type.
    ///
    /// `expected` is used only for recovery: when an error makes the real type
    /// unknowable, it is returned so the caller does not then report a second, invented
    /// mismatch.
    fn check_expr(&mut self, e: &Expr, expected: Option<Type>) -> Type {
        match e {
            Expr::Int { .. } => Type::Int,
            Expr::Bool { .. } => Type::Bool,
            Expr::Str { .. } => Type::Str,

            Expr::Var { name, span } => match self.scopes.lookup(name) {
                Some(t) => t,
                None => {
                    let mut d = Diagnostic::error(
                        codes::UNDEFINED_VARIABLE,
                        *span,
                        format!("`{name}` is not defined"),
                    );
                    if self.functions.contains_key(name) {
                        d = d.help(format!("`{name}` is a function — call it as `{name}(...)`"));
                    }
                    self.errors.push(d);
                    expected.unwrap_or(Type::Int)
                }
            },

            Expr::Binary {
                op,
                lhs,
                rhs,
                op_span,
                ..
            } => {
                let lt = self.check_expr(lhs, None);
                let rt = self.check_expr(rhs, None);
                self.check_binary(
                    &BinaryCheck {
                        op: *op,
                        lt,
                        rt,
                        op_span: *op_span,
                        lhs_span: lhs.span(),
                        rhs_span: rhs.span(),
                    },
                    expected,
                )
            }

            Expr::Neg {
                operand, op_span, ..
            } => {
                let t = self.check_expr(operand, Some(Type::Int));
                if t != Type::Int {
                    self.errors.push(Diagnostic::error(
                        codes::INVALID_OPERANDS,
                        *op_span,
                        format!("`-` applies to `Int`, but this is `{t}`"),
                    ));
                }
                Type::Int
            }

            Expr::Call {
                callee,
                callee_span,
                args,
                span,
            } => {
                let Some(sig) = self.functions.get(callee).cloned() else {
                    self.errors.push(Diagnostic::error(
                        codes::UNDEFINED_FUNCTION,
                        *callee_span,
                        format!("`{callee}` is not a function"),
                    ));
                    for a in args {
                        self.check_expr(a, None);
                    }
                    return expected.unwrap_or(Type::Int);
                };

                if args.len() != sig.params.len() {
                    self.errors.push(
                        Diagnostic::error(
                            codes::WRONG_ARGUMENT_COUNT,
                            *span,
                            format!(
                                "`{callee}` takes {} argument{}, but {} {} given",
                                sig.params.len(),
                                if sig.params.len() == 1 { "" } else { "s" },
                                args.len(),
                                if args.len() == 1 { "was" } else { "were" }
                            ),
                        )
                        .secondary(sig.name_span, "defined here"),
                    );
                }

                // Arguments are checked left to right, matching the evaluation order the
                // spec fixes. Reporting them out of order would be confusing for no gain.
                for (i, a) in args.iter().enumerate() {
                    let want = sig.params.get(i).copied();
                    let got = self.check_expr(a, want);
                    if let Some(want) = want {
                        if got != want {
                            self.errors.push(Diagnostic::error(
                                codes::TYPE_MISMATCH,
                                a.span(),
                                format!(
                                    "argument {} of `{callee}` is `{want}`, but this is `{got}`",
                                    i + 1
                                ),
                            ));
                        }
                    }
                }
                sig.ret
            }

            Expr::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let ct = self.check_expr(cond, Some(Type::Bool));
                if ct != Type::Bool {
                    self.errors.push(
                        Diagnostic::error(
                            codes::CONDITION_NOT_BOOL,
                            cond.span(),
                            format!("an `if` condition must be `Bool`, but this is `{ct}`"),
                        )
                        .help("Nazm has no truthiness: use a comparison such as `x != 0`"),
                    );
                }
                let tt = self.check_block(then_block, expected);
                let et = self.check_block(else_block, expected.or(Some(tt)));
                if tt != et {
                    self.errors.push(
                        Diagnostic::error(
                            codes::BRANCH_TYPES_DIFFER,
                            else_block.tail.span(),
                            format!("this branch is `{et}`, but the other is `{tt}`"),
                        )
                        .secondary(then_block.tail.span(), format!("this one is `{tt}`"))
                        .help("both branches of an `if` must have the same type"),
                    );
                }
                tt
            }

            Expr::Block(b) => self.check_block(b, expected),
        }
    }

    fn check_binary(&mut self, b: &BinaryCheck, expected: Option<Type>) -> Type {
        let BinaryCheck {
            op,
            lt,
            rt,
            op_span,
            lhs_span,
            rhs_span,
        } = *b;
        let ok = match op {
            // Arithmetic is `Int` only. No string concatenation in v0.1, and `+` on two
            // `Bool`s is a mistake rather than something to define.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                lt == Type::Int && rt == Type::Int
            }
            // Ordering is `Int` only: comparing strings needs a collation decision, and
            // ordering booleans is almost always a mistake.
            BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => lt == Type::Int && rt == Type::Int,
            // Equality works on any single type, but not across two.
            BinOp::Eq | BinOp::NotEq => lt == rt,
        };

        if !ok {
            // "compare" is only right for a comparison. Saying it of `+` describes the
            // wrong operation and reads as a compiler that does not know what it is
            // looking at.
            let verb = if op.is_comparison() {
                "compare"
            } else {
                "combine"
            };
            let detail = if lt == rt {
                format!("`{}` does not apply to `{lt}`", op.symbol())
            } else {
                format!("`{}` cannot {verb} `{lt}` with `{rt}`", op.symbol())
            };
            self.errors.push(
                Diagnostic::error(codes::INVALID_OPERANDS, op_span, detail)
                    .secondary(lhs_span, format!("this is `{lt}`"))
                    .secondary(rhs_span, format!("this is `{rt}`")),
            );

            // The operands were wrong, so this expression has no honest type. Reporting
            // the operator's usual one produces a second error about a mismatch the
            // author never wrote — two diagnostics for one mistake, and the second is
            // fiction. Adopt what the context wanted instead.
            return expected.unwrap_or(if op.is_comparison() {
                Type::Bool
            } else {
                Type::Int
            });
        }

        if op.is_comparison() {
            Type::Bool
        } else {
            Type::Int
        }
    }
}
