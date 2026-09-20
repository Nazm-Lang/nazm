//! Recursive descent, with Pratt precedence for binary operators.
//!
//! Handwritten for the reasons in `lexer.rs`, plus one specific to parsing: a construct
//! the language does not support yet should be **named** in the error, not reported as an
//! unexpected token. `while` is not a stray identifier; saying so costs a match arm and
//! saves whoever hits it from guessing whether they mistyped something.

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;

use crate::ast::{
    Assign, BinOp, Block, Expr, Function, Let, Param, Program, Stmt, StmtBlock, TypeName, While,
};
use crate::lexer::{Token, TokenKind};

/// Constructs that are deliberately absent from v0.1, so they can be refused by name.
const RESERVED: &[(&str, &str)] = &[
    ("for", "there is no `for`; iterate with `while`"),
    ("loop", "there is no `loop`; write `while true`"),
    ("break", "a loop runs until its condition is false"),
    ("continue", "a loop runs until its condition is false"),
    ("return", "a function's value is its final expression"),
    ("struct", "there are no user-defined types yet"),
    ("enum", "there are no user-defined types yet"),
    ("match", "there is no pattern matching yet"),
    ("use", "there are no modules yet"),
    ("mod", "there are no modules yet"),
    ("impl", "there are no methods yet"),
    ("print", "there is no input or output yet"),
];

/// What the language does have, for the help text on a refused construct.
const SUPPORTED: &str = concat!(
    "the supported subset is functions, `let` and `let mut`, assignment, `while`, ",
    "`if`/`else`, arithmetic and comparison over `Int`, `Bool` and `Str` — see ",
    "docs/spec.md"
);

#[derive(Debug)]
pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    errors: Vec<Diagnostic>,
}

/// Parse a whole program. Returns whatever was understood plus every error found.
#[must_use]
pub fn parse(tokens: &[Token]) -> (Program, Vec<Diagnostic>) {
    let mut p = Parser {
        tokens,
        pos: 0,
        errors: Vec::new(),
    };
    let mut functions = Vec::new();

    while !p.at(&TokenKind::Eof) {
        match p.function() {
            Some(f) => functions.push(f),
            None => {
                // Skip to the next plausible item so one bad function does not cascade
                // into an error per token for the rest of the file.
                p.recover_to_item();
            }
        }
    }

    (Program { functions }, p.errors)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map_or(&TokenKind::Eof, |t| &t.kind)
    }

    fn peek_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map_or_else(|| Span::new(0, 0), |t| t.span)
    }

    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    fn advance(&mut self) -> TokenKind {
        let k = self.peek().clone();
        if !matches!(k, TokenKind::Eof) {
            self.pos += 1;
        }
        k
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, context: &str) -> Option<Span> {
        if self.at(kind) {
            let s = self.peek_span();
            self.pos += 1;
            return Some(s);
        }
        let found = self.peek().describe();
        let span = self.peek_span();
        self.errors.push(
            Diagnostic::error(
                codes::UNEXPECTED_TOKEN,
                span,
                format!("expected {} {context}, found {found}", kind.describe()),
            )
            .primary_label(format!("expected {}", kind.describe())),
        );
        None
    }

    fn recover_to_item(&mut self) {
        while !self.at(&TokenKind::Eof) && !self.at(&TokenKind::Fn) {
            self.pos += 1;
        }
    }

    /// Reject a deliberately-absent construct by name.
    fn reserved(&mut self, name: &str, span: Span) -> bool {
        if let Some((_, why)) = RESERVED.iter().find(|(k, _)| *k == name) {
            self.errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    span,
                    format!("`{name}` is not supported yet"),
                )
                .primary_label(format!("`{name}` is reserved"))
                .help(format!("{why}. In general, {SUPPORTED}")),
            );
            return true;
        }
        false
    }

    fn function(&mut self) -> Option<Function> {
        let start = self.peek_span();

        if let TokenKind::Ident(name) = self.peek().clone() {
            if self.reserved(&name, start) {
                self.advance();
                return None;
            }
        }

        self.expect(&TokenKind::Fn, "to start a function")?;

        let name_span = self.peek_span();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.errors.push(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    name_span,
                    format!("expected a function name, found {}", other.describe()),
                ));
                return None;
            }
        };

        self.expect(&TokenKind::LParen, "after the function name")?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
            let p_span = self.peek_span();
            // `fn f(mut x: Int)` is a real thing to try, and "expected a parameter name,
            // found `mut`" describes the symptom rather than the rule.
            if self.at(&TokenKind::Mut) {
                self.advance();
                self.errors.push(
                    Diagnostic::error(
                        codes::MUTABLE_PARAMETER,
                        p_span,
                        "parameters cannot be `mut`",
                    )
                    .help(
                        "a mutable parameter is a local copy, which reads as an \
                         out-parameter. Bind one in the body instead: `let mut n = n;`",
                    )
                    .fix(p_span, "", "remove `mut`"),
                );
                return None;
            }
            let p_name = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    self.errors.push(Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        p_span,
                        format!("expected a parameter name, found {}", other.describe()),
                    ));
                    return None;
                }
            };
            self.expect(&TokenKind::Colon, "after a parameter name")?;
            let ty = self.type_name()?;
            params.push(Param {
                name: p_name,
                span: p_span.join(ty.span),
                ty,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "to close the parameter list")?;

        // Required, not optional. A missing return type would have to mean either "infer
        // it" or "returns nothing", and v0.1 has neither.
        self.expect(&TokenKind::Arrow, "before the return type")?;
        let return_type = self.type_name()?;

        let body = self.block()?;
        let span = start.join(body.span);
        Some(Function {
            name,
            name_span,
            params,
            return_type,
            body,
            span,
        })
    }

    fn type_name(&mut self) -> Option<TypeName> {
        let span = self.peek_span();
        match self.advance() {
            TokenKind::Ident(n) => Some(TypeName { name: n, span }),
            other => {
                self.errors.push(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    span,
                    format!("expected a type, found {}", other.describe()),
                ));
                None
            }
        }
    }

    fn block(&mut self) -> Option<Block> {
        let open = self.expect(&TokenKind::LBrace, "to start a block")?;
        let mut statements = Vec::new();

        loop {
            if self.at(&TokenKind::Let) {
                statements.push(Stmt::Let(self.let_stmt()?));
            } else if self.at(&TokenKind::While) {
                statements.push(Stmt::While(self.while_stmt()?));
            } else if let Some(a) = self.try_assign()? {
                statements.push(Stmt::Assign(a));
            } else {
                break;
            }
        }

        let tail = self.expr()?;
        let close = self.expect(&TokenKind::RBrace, "to close the block")?;
        Some(Block {
            statements,
            tail: Box::new(tail),
            span: open.join(close),
        })
    }

    fn let_stmt(&mut self) -> Option<Let> {
        let let_span = self.peek_span();
        self.advance();
        let mutable = self.eat(&TokenKind::Mut);

        let name_span = self.peek_span();
        let name = match self.advance() {
            TokenKind::Ident(n) => {
                if self.reserved(&n, name_span) {
                    return None;
                }
                n
            }
            other => {
                self.errors.push(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    name_span,
                    format!("expected a variable name, found {}", other.describe()),
                ));
                return None;
            }
        };
        self.expect(&TokenKind::Eq, "in a `let` binding")?;
        let value = self.expr()?;
        let semi = self.expect(&TokenKind::Semi, "after a `let` binding")?;
        Some(Let {
            name,
            name_span,
            mutable,
            value,
            span: let_span.join(semi),
        })
    }

    fn while_stmt(&mut self) -> Option<While> {
        let start = self.peek_span();
        self.advance();
        let cond = self.expr()?;
        let body = self.stmt_block()?;
        Some(While {
            cond,
            span: start.join(body.span),
            body,
        })
    }

    /// A loop body: statements, no tail expression.
    fn stmt_block(&mut self) -> Option<StmtBlock> {
        let open = self.expect(&TokenKind::LBrace, "to start a loop body")?;
        let mut statements = Vec::new();
        loop {
            if self.at(&TokenKind::Let) {
                statements.push(Stmt::Let(self.let_stmt()?));
            } else if self.at(&TokenKind::While) {
                statements.push(Stmt::While(self.while_stmt()?));
            } else if let Some(a) = self.try_assign()? {
                statements.push(Stmt::Assign(a));
            } else {
                break;
            }
        }
        // A loop body parses no expression, so nothing else would reach the reserved-word
        // check — `break` would be reported as "expected `}`", which describes the
        // symptom and not the rule.
        if let TokenKind::Ident(name) = self.peek().clone() {
            let span = self.peek_span();
            if self.reserved(&name, span) {
                self.advance();
                return None;
            }
        }

        let close = self.expect(
            &TokenKind::RBrace,
            "to close the loop body — a loop body holds statements and has no value",
        )?;
        Some(StmtBlock {
            statements,
            span: open.join(close),
        })
    }

    /// An assignment, if the next two tokens are `ident =`.
    ///
    /// Two tokens of lookahead rather than parsing an expression and checking what it
    /// turned out to be: `x` alone is a perfectly good tail expression, and committing to
    /// a statement before seeing the `=` would make `{ x }` a parse error.
    ///
    /// `Ok(None)` means "not an assignment", which is different from "an assignment that
    /// failed to parse" — the second has already reported and must not be retried as an
    /// expression.
    fn try_assign(&mut self) -> Option<Option<Assign>> {
        let is_assign = matches!(self.peek(), TokenKind::Ident(_))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Eq)
            );
        if !is_assign {
            return Some(None);
        }

        let name_span = self.peek_span();
        let TokenKind::Ident(name) = self.advance() else {
            return Some(None);
        };
        self.advance(); // `=`
        let value = self.expr()?;
        let semi = self.expect(&TokenKind::Semi, "after an assignment")?;
        Some(Some(Assign {
            name,
            name_span,
            value,
            span: name_span.join(semi),
        }))
    }

    fn expr(&mut self) -> Option<Expr> {
        self.binary(0)
    }

    /// Pratt loop. Comparison binds loosest, then `+`/`-`, then `*`/`/`/`%`.
    fn binary(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.primary()?;

        loop {
            let (op, bp) = match self.peek() {
                TokenKind::EqEq => (BinOp::Eq, 1),
                TokenKind::NotEq => (BinOp::NotEq, 1),
                TokenKind::Lt => (BinOp::Lt, 1),
                TokenKind::LtEq => (BinOp::LtEq, 1),
                TokenKind::Gt => (BinOp::Gt, 1),
                TokenKind::GtEq => (BinOp::GtEq, 1),
                TokenKind::Plus => (BinOp::Add, 2),
                TokenKind::Minus => (BinOp::Sub, 2),
                TokenKind::Star => (BinOp::Mul, 3),
                TokenKind::Slash => (BinOp::Div, 3),
                TokenKind::Percent => (BinOp::Rem, 3),
                _ => break,
            };
            if bp < min_bp {
                break;
            }
            let op_span = self.peek_span();
            self.advance();
            // `bp + 1`: all these operators are left-associative, so `a - b - c` is
            // `(a - b) - c`. Getting this wrong is silent for `+` and wrong for `-`.
            let rhs = self.binary(bp + 1)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                op_span,
                span,
            };
        }
        Some(lhs)
    }

    fn primary(&mut self) -> Option<Expr> {
        let span = self.peek_span();

        // Prefix negation, before anything else can claim the `-`.
        if self.at(&TokenKind::Minus) {
            let op_span = self.peek_span();
            self.advance();
            // Binds tighter than any binary operator, so `-a * b` is `(-a) * b`.
            let operand = self.primary()?;
            let full = op_span.join(operand.span());
            return Some(Expr::Neg {
                operand: Box::new(operand),
                op_span,
                span: full,
            });
        }

        match self.peek().clone() {
            TokenKind::Int(value) => {
                self.advance();
                Some(Expr::Int { value, span })
            }
            TokenKind::True => {
                self.advance();
                Some(Expr::Bool { value: true, span })
            }
            TokenKind::False => {
                self.advance();
                Some(Expr::Bool { value: false, span })
            }
            TokenKind::Str(value) => {
                self.advance();
                Some(Expr::Str { value, span })
            }
            TokenKind::If => {
                self.advance();
                let cond = self.expr()?;
                let then_block = self.block()?;
                // `else` is required: without it an `if` has no value when the condition
                // is false, and every expression in v0.1 has a value.
                self.expect(
                    &TokenKind::Else,
                    "- an `if` used as a value needs both branches",
                )?;
                let else_block = self.block()?;
                let full = span.join(else_block.span);
                Some(Expr::If {
                    cond: Box::new(cond),
                    then_block,
                    else_block,
                    span: full,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(&TokenKind::RParen, "to close the group")?;
                Some(inner)
            }
            TokenKind::LBrace => {
                let b = self.block()?;
                Some(Expr::Block(Box::new(b)))
            }
            TokenKind::Ident(name) => {
                if self.reserved(&name, span) {
                    self.advance();
                    return None;
                }
                self.advance();
                if self.eat(&TokenKind::LParen) {
                    let mut args = Vec::new();
                    while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
                        args.push(self.expr()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let close = self.expect(&TokenKind::RParen, "to close the argument list")?;
                    Some(Expr::Call {
                        callee: name,
                        callee_span: span,
                        args,
                        span: span.join(close),
                    })
                } else {
                    Some(Expr::Var { name, span })
                }
            }
            other => {
                self.errors.push(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    span,
                    format!("expected an expression, found {}", other.describe()),
                ));
                None
            }
        }
    }
}
