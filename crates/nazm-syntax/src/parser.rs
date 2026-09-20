//! Recursive descent, with Pratt precedence for binary operators.
//!
//! Handwritten for the reasons in `lexer.rs`, plus one specific to parsing: a construct
//! the language does not support yet should be **named** in the error, not reported as an
//! unexpected token. `while` is not a stray identifier; saying so costs a match arm and
//! saves whoever hits it from guessing whether they mistyped something.

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;

use crate::ast::{
    Assign, BinOp, Block, Expr, Function, Let, Param, Program, Return, Stmt, TypeName, While,
};
use crate::lexer::{Token, TokenKind};

/// Constructs that are deliberately absent from v0.1, so they can be refused by name.
const RESERVED: &[(&str, &str)] = &[
    ("for", "there is no `for`; iterate with `while`"),
    (
        "label",
        "there are no loop labels; `break` leaves the innermost loop",
    ),
    ("loop", "there is no `loop`; write `while true`"),
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
    "the supported subset is functions, `let` and `let mut`, assignment, `while` with ",
    "`break` and `continue`, `if`/`else` as a statement or an expression, and arithmetic ",
    "and comparison over `Int`, `Bool` and `Str` — see docs/spec.md"
);

#[derive(Debug)]
pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    errors: Vec<Diagnostic>,
    /// Current expression/block nesting, against [`MAX_NESTING`].
    depth: usize,
    /// Whether the nesting limit has already been reported for this item.
    too_deep: bool,
}

/// Parse a whole program. Returns whatever was understood plus every error found.
#[must_use]
pub fn parse(tokens: &[Token]) -> (Program, Vec<Diagnostic>) {
    let mut p = Parser {
        tokens,
        pos: 0,
        errors: Vec::new(),
        depth: 0,
        too_deep: false,
    };
    let mut functions = Vec::new();

    while !p.at(&TokenKind::Eof) {
        // Per item: abandoning one function unwinds out of `leave()` calls that never
        // ran, and a stale count would shrink the budget for every function after it.
        p.depth = 0;
        p.too_deep = false;
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

/// How deeply expressions and blocks may nest.
///
/// Call depth is not the only way to run out of stack: `((((1))))`, `- - - - 1` and
/// `{ { { 1 } } }` recurse through the parser, the checker, the evaluator and finally the
/// destructor of the tree itself, without a single call frame in the program. Bounding
/// the *tree* at parse time bounds all four at once, and is the only one of the four that
/// can report rather than abort.
///
/// 512 is far past anything written by hand. It is not far past what a generator emits:
/// Nazm has no `else if`, so a dispatch chain written as `else { if … }` costs two levels
/// an arm, and a limit tight enough for a 2 MiB stack would have been about 64 — which is
/// a real constraint on real code. Raising the stack is the cheaper side of that trade,
/// so [`PARSE_STACK_BYTES`] does, and `parse_source` says so.
pub const MAX_NESTING: usize = 512;

/// The stack parsing needs, and with it checking, evaluating and dropping the tree.
///
/// Measured: the worst shape is nested blocks, at about 14 KB per level on a debug build,
/// so [`MAX_NESTING`] levels need roughly 7 MiB. This is that with better than 3x spare.
/// `nesting_at_the_limit_is_safe_on_the_documented_stack` parses the deepest accepted tree
/// on exactly this much and would fail if the per-level cost grew into the margin.
pub const PARSE_STACK_BYTES: usize = 24 * 1024 * 1024;

impl<'a> Parser<'a> {
    /// Enter one level of nesting, or report that the tree is too deep.
    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            // Once only: at this depth every enclosing level would report the same thing
            // on the way out, which is one error per paren.
            if !self.too_deep {
                self.too_deep = true;
                self.errors.push(
                    Diagnostic::error(
                        codes::NESTING_TOO_DEEP,
                        self.peek_span(),
                        format!("expressions and blocks may nest {MAX_NESTING} deep"),
                    )
                    .help(
                        "this is the parser's limit, not the language's: a deeper tree \
                         would exhaust the stack in a later pass, where it could only \
                         crash rather than report",
                    ),
                );
            }
            return false;
        }
        true
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Leave `n` levels at once.
    ///
    /// For a chain counted in a loop rather than by recursion, where there is no nesting
    /// of calls to unwind through one `leave` at a time.
    fn unwind(&mut self, levels: usize) {
        self.depth -= levels;
    }

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

        // Deliberately not `?`. A parse error inside the body would otherwise drop the
        // whole declaration, and every call to it would then report that the function
        // does not exist — several errors for one mistake, which is the failure mode this
        // language exists to avoid. Keep the signature; skip to the next item.
        let body = self.block();
        if body.is_none() {
            self.recover_to_item();
        }
        let span = body.as_ref().map_or(return_type.span, |b| b.span);
        Some(Function {
            name,
            name_span,
            params,
            return_type,
            body,
            span: start.join(span),
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

    /// Statements, then an optional tail expression.
    ///
    /// The tail is an expression with no `;` after it. That one rule replaces the forward
    /// brace-scanner the previous design needed to guess whether an `if` was a statement
    /// or a value — and unlike the scanner it has no case it gets wrong.
    fn block(&mut self) -> Option<Block> {
        if !self.enter() {
            return None;
        }
        let out = self.block_inner();
        self.leave();
        out
    }

    fn block_inner(&mut self) -> Option<Block> {
        let open = self.expect(&TokenKind::LBrace, "to start a block")?;
        let mut statements = Vec::new();
        let mut tail = None;

        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Let) {
                statements.push(Stmt::Let(self.let_stmt()?));
                continue;
            }
            if self.at(&TokenKind::While) {
                statements.push(Stmt::While(self.while_stmt()?));
                continue;
            }
            if self.at(&TokenKind::Return) {
                statements.push(Stmt::Return(self.return_stmt()?));
                continue;
            }
            if self.at(&TokenKind::Break) || self.at(&TokenKind::Continue) {
                statements.push(self.loop_control()?);
                continue;
            }
            if let Some(a) = self.try_assign()? {
                statements.push(Stmt::Assign(a));
                continue;
            }

            let block_like = self.at(&TokenKind::If) || self.at(&TokenKind::LBrace);
            let e = self.expr()?;

            if self.eat(&TokenKind::Semi) {
                // Explicitly discarded.
                statements.push(Stmt::Expr(e));
            } else if self.at(&TokenKind::RBrace) {
                tail = Some(Box::new(e));
                break;
            } else if block_like {
                // `if c { … }` followed by more statements needs no `;`, the same rule
                // every block-like expression follows.
                statements.push(Stmt::Expr(e));
            } else {
                let span = self.peek_span();
                let found = self.peek().describe();
                self.errors.push(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        span,
                        format!("expected `;` or `}}` after this expression, found {found}"),
                    )
                    .help("an expression with no `;` after it is the block's value, so it must be last")
                    .fix(Span::new(e.span().end, e.span().end), ";", "discard the value"),
                );
                return None;
            }
        }

        let close = self.expect(&TokenKind::RBrace, "to close the block")?;
        Some(Block {
            statements,
            tail,
            span: open.join(close),
        })
    }

    fn loop_control(&mut self) -> Option<Stmt> {
        let is_break = self.at(&TokenKind::Break);
        let span = self.peek_span();
        self.advance();
        let word = if is_break { "break" } else { "continue" };
        let semi = self.expect(&TokenKind::Semi, &format!("after `{word}`"))?;
        let span = span.join(semi);
        Some(if is_break {
            Stmt::Break { span }
        } else {
            Stmt::Continue { span }
        })
    }

    fn return_stmt(&mut self) -> Option<Return> {
        let start = self.peek_span();
        self.advance();

        // `return;` would need a unit type to mean anything, and this language has not
        // made that decision. Refused by name rather than parsed into something else.
        if self.at(&TokenKind::Semi) {
            let span = self.peek_span();
            self.advance();
            self.errors.push(
                Diagnostic::error(
                    codes::UNSUPPORTED_CONSTRUCT,
                    start.join(span),
                    "`return;` with no value is not supported",
                )
                .help(
                    "every function returns a value, so `return` needs one. There is no \
                     unit type to mean `return nothing` yet",
                ),
            );
            return None;
        }

        let value = self.expr()?;
        let semi = self.expect(&TokenKind::Semi, "after `return`")?;
        Some(Return {
            value,
            span: start.join(semi),
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
        let body = self.block()?;
        Some(While {
            cond,
            span: start.join(body.span),
            body,
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
    ///
    /// The loop does not recurse, but the tree it builds does: `1 + 1 + 1` is
    /// `((1 + 1) + 1)`, one level of left spine per operator. The checker, the evaluator
    /// and the destructor of the tree all walk that spine, so each operator is counted
    /// against the same nesting budget as everything else.
    ///
    /// Counting parse recursion alone would have left this unbounded — and a long
    /// arithmetic chain is by far the likeliest deep tree in a real program, which makes
    /// it the worst possible shape to have missed.
    fn binary(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.primary()?;
        let mut chain = 0;

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
            if !self.enter() {
                self.unwind(chain);
                return None;
            }
            chain += 1;
            let op_span = self.peek_span();
            self.advance();
            // `bp + 1`: all these operators are left-associative, so `a - b - c` is
            // `(a - b) - c`. Getting this wrong is silent for `+` and wrong for `-`.
            //
            // Not `?`: an early return here would leave the chain's levels counted, and
            // the item's remaining budget short by however far it had got.
            let Some(rhs) = self.binary(bp + 1) else {
                self.unwind(chain);
                return None;
            };
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                op_span,
                span,
            };
        }
        self.unwind(chain);
        Some(lhs)
    }

    fn primary(&mut self) -> Option<Expr> {
        if !self.enter() {
            return None;
        }
        let out = self.primary_inner();
        self.leave();
        out
    }

    fn primary_inner(&mut self) -> Option<Expr> {
        let span = self.peek_span();

        // Prefix negation, before anything else can claim the `-`.
        if self.at(&TokenKind::Minus) {
            let op_span = self.peek_span();
            self.advance();

            // Fold the sign into the literal. `i64::MIN` has a magnitude one greater than
            // `i64::MAX`, so `-9223372036854775808` is only representable as a whole.
            if let TokenKind::IntTooLarge(digits) = self.peek().clone() {
                let lit_span = self.peek_span();
                self.advance();
                let full = op_span.join(lit_span);
                return match digits.parse::<u64>() {
                    Ok(v) if v == 1u64 << 63 => Some(Expr::Int {
                        value: i64::MIN,
                        span: full,
                    }),
                    _ => {
                        self.errors.push(
                            Diagnostic::error(
                                codes::INTEGER_TOO_LARGE,
                                full,
                                format!("`-{digits}` does not fit in `Int`"),
                            )
                            .help("`Int` is a 64-bit signed integer: -9223372036854775808 to 9223372036854775807"),
                        );
                        None
                    }
                };
            }
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
            TokenKind::IntTooLarge(digits) => {
                self.advance();
                self.errors.push(
                    Diagnostic::error(
                        codes::INTEGER_TOO_LARGE,
                        span,
                        format!("`{digits}` does not fit in `Int`"),
                    )
                    .help(
                        "`Int` is a 64-bit signed integer: -9223372036854775808 to \
                         9223372036854775807",
                    ),
                );
                None
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
                // Optional. Whether a missing `else` is an error depends on whether a
                // value is wanted from the `if`, and that is a checking question.
                let else_block = if self.eat(&TokenKind::Else) {
                    Some(self.block()?)
                } else {
                    None
                };
                let end = else_block.as_ref().map_or(then_block.span, |b| b.span);
                Some(Expr::If {
                    cond: Box::new(cond),
                    then_block,
                    else_block,
                    span: span.join(end),
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
            kw @ (TokenKind::Break | TokenKind::Continue) => {
                // "expected an expression, found `break`" is true and unhelpful. The
                // reason is that `break` has no value, and saying so points at the fix.
                let word = if matches!(kw, TokenKind::Break) {
                    "break"
                } else {
                    "continue"
                };
                self.advance();
                self.errors.push(
                    Diagnostic::error(
                        codes::CONTROL_FLOW_AS_VALUE,
                        span,
                        format!("`{word}` has no value, so it cannot be used as one"),
                    )
                    .help(
                        "`break` and `continue` are statements. Write `if c { break; }` \
                         rather than using it where a value is expected",
                    ),
                );
                None
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    clippy::indexing_slicing
)]
mod nesting_tests {
    use super::{MAX_NESTING, PARSE_STACK_BYTES};
    use crate::parse_source;
    use nazm_diag::codes;

    /// Run `f` on exactly the stack `parse_source` documents as its requirement.
    ///
    /// A test thread gets 2 MiB, which is less than the contract asks for — so testing on
    /// it would measure the harness rather than the claim.
    fn on_the_documented_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(PARSE_STACK_BYTES)
                .spawn_scoped(scope, f)
                .expect("spawn")
                .join()
                .expect("join")
        })
    }

    /// `fn main() -> Int { <depth> nested parens around 1 }`. The body block is itself a
    /// level, so `depth` parens sit at `depth + 1`.
    fn nested(depth: usize) -> String {
        format!(
            "fn main() -> Int {{ {}1{} }}",
            "(".repeat(depth),
            ")".repeat(depth)
        )
    }

    /// The deepest tree of this shape the parser accepts, found by asking it rather than
    /// by counting levels by hand.
    ///
    /// Constructs do not cost one level each — a nested block recurses through `primary`
    /// *and* `block`, so it costs two — and a test that assumed otherwise would be
    /// testing at some depth short of the edge while claiming to test the edge.
    fn deepest_accepted(shape: &impl Fn(usize) -> String) -> usize {
        let refused = |n: usize| {
            parse_source(&shape(n))
                .1
                .iter()
                .any(|d| d.code == codes::NESTING_TOO_DEEP)
        };
        let (mut lo, mut hi) = (1usize, MAX_NESTING + 2);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if refused(mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        lo
    }

    #[test]
    fn every_tree_the_parser_accepts_fits_the_documented_stack() {
        // The claim `PARSE_STACK_BYTES` makes, tested at the edge for each shape: parse
        // the deepest tree the parser will accept, then drop it, which recurses just as
        // far. An earlier version of this test proved the opposite of what it claimed —
        // 256 was chosen by eye and overflowed a 2 MiB thread on the first run.
        /// A program of some nesting depth, named so the failure says which shape.
        type Shape = (&'static str, Box<dyn Fn(usize) -> String + Send + Sync>);

        let shapes: [Shape; 4] = [
            ("parens", Box::new(nested)),
            (
                "blocks",
                Box::new(|n: usize| {
                    format!(
                        "fn main() -> Int {{ {}1{} }}",
                        "{ ".repeat(n),
                        " }".repeat(n)
                    )
                }),
            ),
            (
                "negation",
                Box::new(|n: usize| format!("fn main() -> Int {{ {}1 }}", "- ".repeat(n))),
            ),
            (
                "calls",
                Box::new(|n: usize| {
                    format!(
                        "fn f(n: Int) -> Int {{ n }}\nfn main() -> Int {{ {}1{} }}",
                        "f(".repeat(n),
                        ")".repeat(n)
                    )
                }),
            ),
        ];
        on_the_documented_stack(move || {
            for (name, shape) in shapes {
                let edge = deepest_accepted(&shape);
                let (_, errors) = parse_source(&shape(edge));
                assert!(errors.is_empty(), "{name} at depth {edge}: {errors:#?}");
                assert!(
                    parse_source(&shape(edge + 1))
                        .1
                        .iter()
                        .any(|d| d.code == codes::NESTING_TOO_DEEP),
                    "{name}: depth {} should be past the limit",
                    edge + 1
                );
            }
        });
    }

    #[test]
    fn nesting_past_the_limit_is_refused_not_crashed() {
        let (_, errors) = on_the_documented_stack(|| parse_source(&nested(MAX_NESTING + 1)));
        assert_eq!(errors.len(), 1, "once, not once per paren: {errors:#?}");
        assert_eq!(errors[0].code, codes::NESTING_TOO_DEEP);
    }

    #[test]
    fn a_tree_far_past_the_limit_is_still_one_error() {
        // Four times the limit: comfortably refused, and still shallow enough to parse
        // within `PARSE_STACK_BYTES` if the limit were not there. A depth that could only
        // abort belongs in the CLI suite, where the abort is a child exit rather than the
        // end of this process.
        let (_, errors) = on_the_documented_stack(|| parse_source(&nested(MAX_NESTING * 4)));
        assert_eq!(errors.len(), 1, "{} errors", errors.len());
        assert_eq!(errors[0].code, codes::NESTING_TOO_DEEP);
    }

    #[test]
    fn nesting_is_counted_per_item_not_per_file() {
        // A function that blew the budget must not shrink it for the next one. Sharing
        // the counter would make the second function fail for the first one's mistake.
        let src = format!("{}\nfn ok() -> Int {{ 1 }}", nested(MAX_NESTING + 1));
        let (program, errors) = on_the_documented_stack(move || parse_source(&src));
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert!(
            program.functions.iter().any(|f| f.name == "ok"),
            "the second function should still parse"
        );
    }

    #[test]
    fn unary_and_block_and_call_nesting_are_bounded_too() {
        // Four shapes reach the same recursion. `primary` and `block` are the two doors,
        // and counting only one of them would leave the other wide open.
        for src in [
            format!("fn main() -> Int {{ {}1 }}", "- ".repeat(MAX_NESTING + 1)),
            format!(
                "fn main() -> Int {{ {}1{} }}",
                "{ ".repeat(MAX_NESTING + 1),
                " }".repeat(MAX_NESTING + 1)
            ),
            format!(
                "fn f(n: Int) -> Int {{ n }}\nfn main() -> Int {{ {}1{} }}",
                "f(".repeat(MAX_NESTING + 1),
                ")".repeat(MAX_NESTING + 1)
            ),
            format!(
                "fn main() -> Int {{ {}1{} }}",
                "if true { ".repeat(MAX_NESTING + 1),
                " } else { 0 }".repeat(MAX_NESTING + 1)
            ),
        ] {
            let (_, errors) = on_the_documented_stack(move || parse_source(&src));
            assert!(
                errors.iter().any(|d| d.code == codes::NESTING_TOO_DEEP),
                "not bounded: {errors:#?}"
            );
        }
    }
}
