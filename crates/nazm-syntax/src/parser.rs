//! Recursive descent, with Pratt precedence for binary operators.
//!
//! Handwritten for the reasons in `lexer.rs`, plus one specific to parsing: a construct
//! the language does not support yet should be **named** in the error, not reported as an
//! unexpected token. `while` is not a stray identifier; saying so costs a match arm and
//! saves whoever hits it from guessing whether they mistyped something.

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;

use crate::ast::{BinOp, Block, Expr, Function, Let, Param, Program, TypeName};
use crate::lexer::{Token, TokenKind};

/// Constructs that are deliberately absent from v0.1, so they can be refused by name.
const RESERVED: &[(&str, &str)] = &[
    ("while", "loops"),
    ("for", "loops"),
    ("loop", "loops"),
    ("return", "early return"),
    ("mut", "mutation"),
    ("struct", "user-defined types"),
    ("enum", "user-defined types"),
    ("match", "pattern matching"),
    ("use", "modules"),
    ("mod", "modules"),
    ("impl", "methods"),
    ("print", "input and output"),
];

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
        if let Some((_, feature)) = RESERVED.iter().find(|(k, _)| *k == name) {
            self.errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    span,
                    format!("`{name}` is not supported in Nazm v0.1"),
                )
                .help(format!(
                    "{feature} are not implemented yet. The supported subset is functions, \
                     `let`, `if`/`else`, arithmetic and comparison."
                )),
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
        let mut lets = Vec::new();

        while self.at(&TokenKind::Let) {
            let let_span = self.peek_span();
            self.advance();
            let name_span = self.peek_span();
            let name = match self.advance() {
                TokenKind::Ident(n) => {
                    // `let mut x = 1` reads as a binding named `mut`, and reporting
                    // "expected `=`, found `x`" describes a mistake nobody made.
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
            lets.push(Let {
                name,
                name_span,
                value,
                span: let_span.join(semi),
            });
        }

        let tail = self.expr()?;
        let close = self.expect(&TokenKind::RBrace, "to close the block")?;
        Some(Block {
            lets,
            tail: Box::new(tail),
            span: open.join(close),
        })
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
