//! Recursive descent, with Pratt precedence for binary operators.
//!
//! Handwritten for the reasons in `lexer.rs`, plus one specific to parsing: a construct
//! the language does not support yet should be **named** in the error, not reported as an
//! unexpected token. `while` is not a stray identifier; saying so costs a match arm and
//! saves whoever hits it from guessing whether they mistyped something.

use nazm_diag::{codes, Diagnostic};
use nazm_span::{FileId, Span};

use crate::ast::{
    Assign, BinOp, Block, Expr, Field, FieldInit, Function, Import, Let, Param, Place, PlaceField,
    Program, Record, Return, Scope, Spawn, Stmt, TypeName, While,
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
    // `struct` left this list in N9, when it became the keyword it was reserving. The
    // entry existed to refuse a word people would reach for before it worked.
    ("enum", "there are no variant types yet"),
    ("match", "there is no pattern matching yet"),
    // `use` left this list when imports arrived; `mod` stays, because there is no
    // module *declaration* — a file is the unit, and `use` names one by path.
    (
        "mod",
        "a file is the module; import one with `use \"other.nz\";`",
    ),
    ("impl", "there are no methods yet"),
    // `print` left this list when `print` became a built-in. The entry existed to refuse
    // a name people would reach for before it worked; now it works, and refusing it
    // would be refusing the thing it was reserving.
];

/// What the language does have, for the help text on a refused construct.
const SUPPORTED: &str = concat!(
    "the supported subset is functions, `let` and `let mut`, assignment, `while` with ",
    "`break` and `continue`, `return`, `if`/`else` as a statement or an expression, ",
    "arithmetic and comparison over `Int`, `Bool` and `Str`, the `Ints` and `Strs` ",
    "sequences, `struct` records with named fields, and the built-in operations on them ",
    "— see docs/spec.md"
);

#[derive(Debug)]
pub struct Parser<'a> {
    /// The file being read, for spans invented where no token exists.
    file: FileId,
    tokens: &'a [Token],
    pos: usize,
    errors: Vec<Diagnostic>,
    /// Current parser recursion, against [`MAX_NESTING`].
    depth: usize,
    /// Depth of the subtree most recently built, against [`MAX_NESTING`].
    built: usize,
    /// Whether the limit has already been reported for this item.
    reported_too_deep: bool,
}

/// Parse a whole program. Returns whatever was understood plus every error found.
#[must_use]
pub fn parse(tokens: &[Token]) -> (Program, Vec<Diagnostic>) {
    parse_in(FileId::ROOT, tokens)
}

/// [`parse`], told which file it is reading.
///
/// The file is needed only for the spans the parser invents where there is no token to
/// point at — an unexpected end of input, or an insertion point after the last
/// expression. Every other span comes from a token and already names its file.
pub fn parse_in(file: FileId, tokens: &[Token]) -> (Program, Vec<Diagnostic>) {
    let mut p = Parser {
        file,
        tokens,
        pos: 0,
        errors: Vec::new(),
        depth: 0,
        built: 0,
        reported_too_deep: false,
    };
    let mut functions = Vec::new();
    let mut records = Vec::new();
    let mut imports = Vec::new();

    while !p.at(&TokenKind::Eof) {
        if p.at(&TokenKind::Use) {
            if let Some(i) = p.import() {
                imports.push(i);
            } else {
                p.recover_to_item();
            }
            continue;
        }
        // Per item: abandoning one function unwinds out of `leave()` calls that never
        // ran, and a stale count would shrink the budget for every function after it.
        p.depth = 0;
        p.built = 0;
        p.reported_too_deep = false;
        // `struct` or `pub struct`. Looking one token past a `pub` is what keeps the two
        // kinds of item apart without either grammar knowing about the other.
        let is_record = p.at(&TokenKind::Struct)
            || (p.at(&TokenKind::Pub)
                && matches!(
                    p.tokens.get(p.pos + 1).map(|t| &t.kind),
                    Some(TokenKind::Struct)
                ));
        if is_record {
            match p.record() {
                Some(r) => records.push(r),
                None => p.recover_to_item(),
            }
            continue;
        }
        match p.function() {
            Some(f) => functions.push(f),
            None => {
                // Skip to the next plausible item so one bad function does not cascade
                // into an error per token for the rest of the file.
                p.recover_to_item();
            }
        }
    }

    (
        Program {
            functions,
            records,
            imports,
        },
        p.errors,
    )
}

/// How deep the syntax tree may be.
///
/// Call depth is not the only way to run out of stack: `((((1))))`, `- - - - 1` and
/// `{ { { 1 } } }` recurse through the parser, the checker, the evaluator and finally the
/// destructor of the tree itself, without a single call frame in the program. Bounding
/// the *tree* at parse time bounds all four at once, and is the only one of the four that
/// can report rather than abort.
///
/// This is a bound on the tree, measured as the tree is built — not on how deeply the
/// parser recursed to build it. The two differ, and the difference is not small. An
/// earlier version counted parser recursion and was wrong in both directions: parentheses
/// build no node at all, so `((((1))))` is a tree of depth 2 however many there are; and
/// `- - - 1 + 1 + 1` parses its operand *before* the chain of `+` starts counting, so the
/// operand's levels were released and the two composed into a tree twice as deep as the
/// limit. A test that measured the tree instead of the counter found 1022 against a limit
/// of 512, and nesting the composition would have compounded it further.
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
    /// Record the depth of the subtree just built, and report if it is too deep.
    ///
    /// Returns `None` when it is, so callers propagate with `?` exactly as they do for a
    /// syntax error. Every construction site calls this, and the depth it passes is
    /// `1 + max(children)` — which is what makes the bound exact rather than an estimate
    /// of it.
    fn built(&mut self, depth: usize) -> Option<usize> {
        self.built = depth;
        if depth > MAX_NESTING {
            self.too_deep();
            return None;
        }
        Some(depth)
    }

    /// The depth of the subtree most recently built. Read immediately after parsing a
    /// child, before anything else can overwrite it.
    fn child(&self) -> usize {
        self.built
    }

    /// Enter one level of parser recursion, or report that it has gone too far.
    ///
    /// Separate from the tree bound, and both are needed: the parser is itself a stack
    /// consumer, and parentheses make it recurse without building anything. Without this,
    /// `((((…))))` would parse until the stack ran out and then have nothing to measure.
    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            self.too_deep();
            return false;
        }
        true
    }

    /// Report that the limit was passed — once per item, not once per level.
    ///
    /// Every enclosing level would otherwise report the same thing on the way out, which
    /// is one error per paren.
    fn too_deep(&mut self) {
        if self.reported_too_deep {
            return;
        }
        self.reported_too_deep = true;
        self.errors.push(
            Diagnostic::error(
                codes::NESTING_TOO_DEEP,
                self.peek_span(),
                format!("expressions and blocks may nest {MAX_NESTING} deep"),
            )
            .help(
                "this is the parser's limit, not the language's: a deeper tree would \
                 exhaust the stack in a later pass, where it could only crash rather \
                 than report",
            ),
        );
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map_or(&TokenKind::Eof, |t| &t.kind)
    }

    fn peek_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map_or_else(|| Span::new(self.file, 0, 0), |t| t.span)
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
        while !self.at(&TokenKind::Eof)
            && !self.at(&TokenKind::Fn)
            && !self.at(&TokenKind::Pub)
            && !self.at(&TokenKind::Struct)
        {
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

    /// `use "path";`
    ///
    /// Only at the top level, and only with a string literal — a path is data, not an
    /// identifier chain, so there is no module-path grammar to design and nothing that
    /// could be mistaken for one.
    fn import(&mut self) -> Option<Import> {
        let start = self.expect(&TokenKind::Use, "to start an import")?;
        let path_span = self.peek_span();
        let path = match self.advance() {
            TokenKind::Str(p) => p,
            other => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        path_span,
                        format!("expected a quoted path, found {}", other.describe()),
                    )
                    .help("an import is written `use \"other.nz\";`"),
                );
                return None;
            }
        };
        let end = self.expect(&TokenKind::Semi, "to end an import")?;
        Some(Import {
            path,
            path_span,
            span: start.join(end),
        })
    }

    /// `scope { … }`
    fn scope(&mut self) -> Option<Scope> {
        let start = self.expect(&TokenKind::Scope, "to start a scope")?;
        let body = self.block()?;
        Some(Scope {
            span: start.join(body.span),
            body,
        })
    }

    /// `spawn f(args);`
    fn spawn(&mut self) -> Option<Spawn> {
        let start = self.expect(&TokenKind::Spawn, "to start a task")?;
        let callee_span = self.peek_span();
        let callee = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        callee_span,
                        format!(
                            "expected a function name after `spawn`, found {}",
                            other.describe()
                        ),
                    )
                    .help(
                        "a task runs a named function: `spawn worker(jobs);`. There are \
                         no function values to pass instead",
                    ),
                );
                return None;
            }
        };
        self.expect(&TokenKind::LParen, "to start the arguments")?;
        let mut args = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                args.push(self.expr()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RParen, "to end the arguments")?;
        let end = self.expect(&TokenKind::Semi, "to end a `spawn`")?;
        Some(Spawn {
            callee,
            callee_span,
            args,
            span: start.join(end),
        })
    }

    /// `struct Name { field: Type, … }`, with an optional `pub` in front.
    ///
    /// A trailing comma is allowed and canonical, as it is in every other comma list the
    /// formatter emits one item to a line.
    fn record(&mut self) -> Option<Record> {
        let start = self.peek_span();
        let pub_span = self.at(&TokenKind::Pub).then(|| {
            let s = self.peek_span();
            self.advance();
            s
        });
        self.expect(&TokenKind::Struct, "to start a record")?;

        let name_span = self.peek_span();
        let name = match self.advance() {
            TokenKind::Ident(n) => {
                if self.reserved(&n, name_span) {
                    return None;
                }
                n
            }
            other => {
                self.errors.push(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        name_span,
                        format!("expected a record name, found {}", other.describe()),
                    )
                    .help("a record is declared `struct Point { x: Int, y: Int, }`"),
                );
                return None;
            }
        };

        self.expect(&TokenKind::LBrace, "to start the fields")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let field_span = self.peek_span();
            let field_name = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            field_span,
                            format!("expected a field name, found {}", other.describe()),
                        )
                        .help("each field is written `name: Type,`"),
                    );
                    return None;
                }
            };
            self.expect(&TokenKind::Colon, "after a field name")?;
            let ty = self.type_name()?;
            fields.push(Field {
                name: field_name,
                name_span: field_span,
                span: field_span.join(ty.span),
                ty,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(&TokenKind::RBrace, "to close the fields")?;
        Some(Record {
            name,
            name_span,
            pub_span,
            fields,
            span: start.join(close),
        })
    }

    fn function(&mut self) -> Option<Function> {
        let start = self.peek_span();

        // `pub`, if it is there. The parser records **that the word was written and
        // where**, and stops: what it means — that this module exports the definition —
        // is a semantic question, and a parser that answered it would be a second place
        // visibility is decided. See `nazm_sema::Visibility`.
        let pub_span = self.at(&TokenKind::Pub).then(|| {
            let s = self.peek_span();
            self.advance();
            s
        });

        if let TokenKind::Ident(name) = self.peek().clone() {
            let at = self.peek_span();
            if self.reserved(&name, at) {
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
                    .fix_automatically(
                        p_span,
                        "",
                        "remove `mut`",
                        "a parameter is immutable either way, so removing the word \
                         changes nothing but whether the program is legal",
                    ),
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
            pub_span,
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
        let mut deepest = 0;

        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            // A statement is not itself a node: its depth is its deepest sub-expression.
            if self.at(&TokenKind::Let) {
                statements.push(Stmt::Let(self.let_stmt()?));
                deepest = deepest.max(self.child());
                continue;
            }
            if self.at(&TokenKind::While) {
                statements.push(Stmt::While(self.while_stmt()?));
                deepest = deepest.max(self.child());
                continue;
            }
            if self.at(&TokenKind::Return) {
                statements.push(Stmt::Return(self.return_stmt()?));
                deepest = deepest.max(self.child());
                continue;
            }
            if self.at(&TokenKind::Break) || self.at(&TokenKind::Continue) {
                statements.push(self.loop_control()?);
                continue;
            }
            if self.at(&TokenKind::Scope) {
                statements.push(Stmt::Scope(self.scope()?));
                deepest = deepest.max(self.child());
                continue;
            }
            if self.at(&TokenKind::Spawn) {
                statements.push(Stmt::Spawn(self.spawn()?));
                deepest = deepest.max(self.child());
                continue;
            }
            if let Some(a) = self.try_assign()? {
                statements.push(Stmt::Assign(a));
                deepest = deepest.max(self.child());
                continue;
            }

            let block_like = self.at(&TokenKind::If) || self.at(&TokenKind::LBrace);
            let e = self.expr()?;
            deepest = deepest.max(self.child());

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
                    .fix_with_review(
                        Span::new(e.span().file, e.span().end, e.span().end),
                        ";",
                        "discard the value",
                        "the value was meant to be discarded rather than returned — if \
                         it was meant to be the block's value, the statements after it \
                         are what should move",
                    ),
                );
                return None;
            }
        }

        let close = self.expect(&TokenKind::RBrace, "to close the block")?;
        self.built(deepest + 1)?;
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
        let cond_depth = self.child();
        let body = self.block()?;
        // Both children count: a `while` whose condition is deeper than its body is
        // bounded by the condition.
        self.built = cond_depth.max(self.child());
        Some(While {
            cond,
            span: start.join(body.span),
            body,
        })
    }

    /// An assignment, if what follows is `ident ( . ident )* =`.
    ///
    /// Lookahead rather than parsing an expression and checking what it turned out to be:
    /// `x` alone is a perfectly good tail expression, and `a.b` is a perfectly good one
    /// too, so committing to a statement before seeing the `=` would make `{ a.b }` a
    /// parse error. The scan is bounded by the tokens it walks and reads nothing it does
    /// not then consume.
    ///
    /// `Ok(None)` means "not an assignment", which is different from "an assignment that
    /// failed to parse" — the second has already reported and must not be retried as an
    /// expression.
    fn try_assign(&mut self) -> Option<Option<Assign>> {
        if !matches!(self.peek(), TokenKind::Ident(_)) {
            return Some(None);
        }
        // `ident` then any number of `. ident`, then `=`. A `==` is a different token, so
        // a comparison in statement position is never mistaken for one of these.
        let mut at = self.pos + 1;
        loop {
            match self.tokens.get(at).map(|t| &t.kind) {
                Some(TokenKind::Eq) => break,
                Some(TokenKind::Dot) => {
                    if matches!(
                        self.tokens.get(at + 1).map(|t| &t.kind),
                        Some(TokenKind::Ident(_))
                    ) {
                        at += 2;
                    } else {
                        return Some(None);
                    }
                }
                _ => return Some(None),
            }
        }

        let name_span = self.peek_span();
        let TokenKind::Ident(name) = self.advance() else {
            return Some(None);
        };
        let mut path = Vec::new();
        let mut last = name_span;
        while self.eat(&TokenKind::Dot) {
            let field_span = self.peek_span();
            let TokenKind::Ident(field) = self.advance() else {
                return Some(None);
            };
            last = field_span;
            path.push(PlaceField {
                name: field,
                name_span: field_span,
            });
        }
        self.advance(); // `=`
        let value = self.expr()?;
        let semi = self.expect(&TokenKind::Semi, "after an assignment")?;
        Some(Some(Assign {
            place: Place {
                name,
                name_span,
                path,
                span: name_span.join(last),
            },
            value,
            span: name_span.join(semi),
        }))
    }

    /// `Name(field: value, …)`, the `(` already seen by the caller's lookahead.
    ///
    /// Every field is named, there is no positional form, and a trailing comma is allowed.
    /// Duplicates, omissions and unknown names are the checker's to report: they are
    /// questions about a type, and the parser has not resolved one.
    fn construction(&mut self, ty: String, ty_span: Span) -> Option<Expr> {
        self.expect(&TokenKind::LParen, "to start the fields")?;
        let mut fields = Vec::new();
        let mut deepest = 0;
        while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
            let name_span = self.peek_span();
            let name = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            name_span,
                            format!("expected a field name, found {}", other.describe()),
                        )
                        .help(
                            "every field of a record is named when it is built: \
                             `Point(x: 1, y: 2)`",
                        ),
                    );
                    return None;
                }
            };
            self.expect(&TokenKind::Colon, "after a field name")?;
            let value = self.expr()?;
            deepest = deepest.max(self.child());
            fields.push(FieldInit {
                name,
                name_span,
                span: name_span.join(value.span()),
                value,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(&TokenKind::RParen, "to close the fields")?;
        self.built(deepest + 1)?;
        Some(Expr::Construct {
            ty,
            ty_span,
            fields,
            span: ty_span.join(close),
        })
    }

    fn expr(&mut self) -> Option<Expr> {
        self.binary(0)
    }

    /// Pratt loop. Comparison binds loosest, then `+`/`-`, then `*`/`/`/`%`.
    ///
    /// The loop does not recurse, but the tree it builds does: `1 + 1 + 1` is
    /// `((1 + 1) + 1)`, one level of left spine per operator. So a chain costs nothing
    /// against the parser-recursion bound and everything against the tree bound, which
    /// is why the two are separate measurements and why `lhs_depth` carries the operand's
    /// own depth into the node built on top of it.
    fn binary(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.primary()?;
        // The operand's own depth, which the node built on top of it adds to. Counting
        // only the operators would miss this entirely: `- - - 1 + 1 + 1` releases the
        // negations before the first `+` is seen, and the two compose into a tree as deep
        // as both together.
        let mut lhs_depth = self.child();

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
            //
            // This recursion needs no budget of its own: `bp` is at most 3, and a call at
            // min_bp 4 matches no operator and returns immediately, so the binary-to-
            // binary chain is at most three frames however long the expression. The
            // unbounded path into an operand goes through `primary`, which `enter`
            // counts.
            let rhs = self.binary(bp + 1)?;
            let rhs_depth = self.child();
            lhs_depth = lhs_depth.max(rhs_depth) + 1;
            self.built(lhs_depth)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                op_span,
                span,
            };
        }
        // The loop may not have run, in which case this restores the operand's own depth
        // over whatever the last `rhs` left behind.
        self.built = lhs_depth;
        Some(lhs)
    }

    fn primary(&mut self) -> Option<Expr> {
        if !self.enter() {
            return None;
        }
        let out = self.primary_inner().and_then(|e| self.projections(e));
        self.leave();
        out
    }

    /// `expr.name`, repeated. Binds tighter than any operator, including negation's
    /// operand, because it is part of the operand.
    ///
    /// A loop rather than recursion: `a.b.c.d` is a left spine, so it costs one tree level
    /// per field and no parser recursion at all.
    fn projections(&mut self, base: Expr) -> Option<Expr> {
        let mut out = base;
        let mut depth = self.child();
        while self.at(&TokenKind::Dot) {
            self.advance();
            let name_span = self.peek_span();
            let name = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    self.errors.push(
                        Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            name_span,
                            format!(
                                "expected a field name after `.`, found {}",
                                other.describe()
                            ),
                        )
                        .help("`.` reads a field out of a record: `p.x`"),
                    );
                    return None;
                }
            };
            depth += 1;
            self.built(depth)?;
            out = Expr::Field {
                span: out.span().join(name_span),
                base: Box::new(out),
                name,
                name_span,
            };
        }
        self.built = depth;
        Some(out)
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
                    Ok(v) if v == 1u64 << 63 => {
                        self.built(1)?;
                        Some(Expr::Int {
                            value: i64::MIN,
                            span: full,
                        })
                    }
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
            self.built(self.child() + 1)?;
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
                self.built(1)?;
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
                self.built(1)?;
                Some(Expr::Bool { value: true, span })
            }
            TokenKind::False => {
                self.advance();
                self.built(1)?;
                Some(Expr::Bool { value: false, span })
            }
            TokenKind::Str(value) => {
                self.advance();
                self.built(1)?;
                Some(Expr::Str { value, span })
            }
            TokenKind::If => {
                self.advance();
                let cond = self.expr()?;
                let mut deepest = self.child();
                let then_block = self.block()?;
                deepest = deepest.max(self.child());
                // Optional. Whether a missing `else` is an error depends on whether a
                // value is wanted from the `if`, and that is a checking question.
                let else_block = if self.eat(&TokenKind::Else) {
                    let b = self.block()?;
                    deepest = deepest.max(self.child());
                    Some(b)
                } else {
                    None
                };
                self.built(deepest + 1)?;
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
                // No node: `(x)` *is* `x`. `self.built` already holds the inner depth and
                // must not be incremented, or the limit would count punctuation.
                Some(inner)
            }
            TokenKind::LBrace => {
                let b = self.block()?;
                self.built(self.child() + 1)?;
                Some(Expr::Block(Box::new(b)))
            }
            TokenKind::Ident(name) => {
                if self.reserved(&name, span) {
                    self.advance();
                    return None;
                }
                self.advance();
                // `Name(field: …)` is a construction; `f(a, b)` is a call. Two tokens
                // past the `(` decide it, and the decision is total: no expression can
                // begin with a name followed by a colon, so neither form can be mistaken
                // for the other and neither needs to know about the other.
                let constructing = self.at(&TokenKind::LParen)
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokenKind::Ident(_))
                    )
                    && matches!(
                        self.tokens.get(self.pos + 2).map(|t| &t.kind),
                        Some(TokenKind::Colon)
                    );
                if constructing {
                    return self.construction(name, span);
                }
                if self.eat(&TokenKind::LParen) {
                    let mut args = Vec::new();
                    let mut deepest = 0;
                    while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
                        args.push(self.expr()?);
                        deepest = deepest.max(self.child());
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    let close = self.expect(&TokenKind::RParen, "to close the argument list")?;
                    self.built(deepest + 1)?;
                    Some(Expr::Call {
                        callee: name,
                        callee_span: span,
                        args,
                        span: span.join(close),
                    })
                } else {
                    self.built(1)?;
                    Some(Expr::Var { name, span })
                }
            }
            TokenKind::Struct => {
                // A `struct` inside a body is a declaration in the wrong place, not an
                // expression. Saying which is the difference between one useful error and
                // a cascade of "expected an expression".
                self.advance();
                self.errors.push(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        span,
                        "a record can only be declared at the top level",
                    )
                    .help(
                        "move the `struct` out of the function. Records are module-level \
                         declarations, like functions",
                    ),
                );
                None
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::integer_division)]
mod ast_depth_tests {
    //! Does the nesting counter actually bound the depth of the tree it produces?
    //!
    //! The counter is a proxy. What matters is the tree, because that is what the
    //! checker, the evaluator and the destructor recurse over — and the two can disagree:
    //! a flat operator count could reject a shallow expression, and a counter released at
    //! the wrong moment could admit a deep one. So these tests measure the tree itself and
    //! compare it against the limit, over shapes built by composing every construct that
    //! recurses.

    use super::{MAX_NESTING, PARSE_STACK_BYTES};
    use crate::{parse_source, Block, Expr, Program, Stmt};
    use nazm_diag::codes;

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

    /// Longest root-to-leaf path through the tree, counted in nodes.
    ///
    /// Computed from child relationships alone. It never reads `Parser::built` or any
    /// other number the parser recorded — if it did, the test and the implementation
    /// could agree while both being wrong, which is the failure mode this is here to
    /// rule out. The only thing shared with the production accounting is the convention
    /// that a statement is not a node and a parenthesis is not a node, and those are
    /// visible in the tree itself: there is no `Stmt` or `Paren` node to count.
    ///
    /// Iterative on purpose: a recursive walker would overflow on exactly the trees this
    /// exists to catch, and a measuring instrument that dies on the interesting case is
    /// no instrument at all.
    fn depth(program: &Program) -> usize {
        enum Node<'a> {
            Expr(&'a Expr),
            Block(&'a Block),
        }

        let mut deepest = 0;
        let mut stack: Vec<(Node<'_>, usize)> = program
            .functions
            .iter()
            .filter_map(|f| f.body.as_ref().map(|b| (Node::Block(b), 1)))
            .collect();

        while let Some((node, d)) = stack.pop() {
            deepest = deepest.max(d);
            match node {
                Node::Block(b) => {
                    for s in &b.statements {
                        match s {
                            Stmt::Let(l) => stack.push((Node::Expr(&l.value), d + 1)),
                            Stmt::Assign(a) => stack.push((Node::Expr(&a.value), d + 1)),
                            Stmt::Return(r) => stack.push((Node::Expr(&r.value), d + 1)),
                            Stmt::While(w) => {
                                stack.push((Node::Expr(&w.cond), d + 1));
                                stack.push((Node::Block(&w.body), d + 1));
                            }
                            Stmt::Expr(e) => stack.push((Node::Expr(e), d + 1)),
                            Stmt::Scope(sc) => stack.push((Node::Block(&sc.body), d + 1)),
                            Stmt::Spawn(sp) => {
                                for a in &sp.args {
                                    stack.push((Node::Expr(a), d + 1));
                                }
                            }
                            Stmt::Break { .. } | Stmt::Continue { .. } => {}
                        }
                    }
                    if let Some(t) = &b.tail {
                        stack.push((Node::Expr(t), d + 1));
                    }
                }
                Node::Expr(e) => match e {
                    Expr::Int { .. } | Expr::Bool { .. } | Expr::Str { .. } | Expr::Var { .. } => {}
                    Expr::Neg { operand, .. } => stack.push((Node::Expr(operand), d + 1)),
                    Expr::Binary { lhs, rhs, .. } => {
                        stack.push((Node::Expr(lhs), d + 1));
                        stack.push((Node::Expr(rhs), d + 1));
                    }
                    Expr::Call { args, .. } => {
                        for a in args {
                            stack.push((Node::Expr(a), d + 1));
                        }
                    }
                    Expr::Construct { fields, .. } => {
                        for f in fields {
                            stack.push((Node::Expr(&f.value), d + 1));
                        }
                    }
                    Expr::Field { base, .. } => stack.push((Node::Expr(base), d + 1)),
                    Expr::If {
                        cond,
                        then_block,
                        else_block,
                        ..
                    } => {
                        stack.push((Node::Expr(cond), d + 1));
                        stack.push((Node::Block(then_block), d + 1));
                        if let Some(b) = else_block {
                            stack.push((Node::Block(b), d + 1));
                        }
                    }
                    Expr::Block(b) => stack.push((Node::Block(b), d + 1)),
                },
            }
        }
        deepest
    }

    /// Wrap `inner` in one layer of each construct that recurses, `times` deep.
    fn layer(kind: &str, times: usize, inner: &str) -> String {
        match kind {
            "paren" => format!("{}{inner}{}", "(".repeat(times), ")".repeat(times)),
            "neg" => format!("{}{inner}", "- ".repeat(times)),
            "block" => format!("{}{inner}{}", "{ ".repeat(times), " }".repeat(times)),
            "call" => format!("{}{inner}{}", "f(".repeat(times), ")".repeat(times)),
            "if" => format!(
                "{}{inner}{}",
                "if true { ".repeat(times),
                " } else { 0 }".repeat(times)
            ),
            // Left-associative, so this nests on the left spine without recursing.
            "chain" => format!("{inner}{}", " + 1".repeat(times)),
            // Mixed precedence, which nests on the right as well as the left.
            "mixed" => format!("{inner}{}", " + 1 * 2 - 3".repeat(times)),
            other => panic!("unknown shape {other}"),
        }
    }

    /// Every combination of two shapes, plus each shape alone.
    fn compositions() -> Vec<(String, String)> {
        const KINDS: [&str; 7] = ["paren", "neg", "block", "call", "if", "chain", "mixed"];
        let mut out = Vec::new();
        for outer in KINDS {
            out.push((outer.to_string(), layer(outer, 120, "1")));
            for inner in KINDS {
                // 60 of each, so a correct bound accepts the pair and a counter that
                // released the outer layers early would let the total run past it.
                let body = layer(outer, 60, &layer(inner, 60, "1"));
                out.push((format!("{outer}+{inner}"), body));
            }
        }
        out
    }

    #[test]
    fn no_accepted_program_is_deeper_than_the_limit() {
        // The property the bound exists for, stated over the tree rather than over the
        // counter that is supposed to track it.
        on_the_documented_stack(|| {
            for (name, body) in compositions() {
                let src = format!("fn f(n: Int) -> Int {{ n }}\nfn main() -> Int {{ {body} }}");
                let (program, errors) = parse_source(&src);
                if errors.iter().any(|d| d.code == codes::NESTING_TOO_DEEP) {
                    continue;
                }
                let d = depth(&program);
                assert!(
                    d <= MAX_NESTING,
                    "{name}: accepted a tree {d} deep, limit is {MAX_NESTING}"
                );
            }
        });
    }

    #[test]
    fn composition_is_not_rejected_when_it_fits() {
        // The other half: a bound that refused everything would satisfy the test above.
        // Two shapes at 60 layers each are nowhere near 512, so all of them must parse.
        on_the_documented_stack(|| {
            for (name, body) in compositions() {
                let src = format!("fn f(n: Int) -> Int {{ n }}\nfn main() -> Int {{ {body} }}");
                let (_, errors) = parse_source(&src);
                assert!(
                    !errors.iter().any(|d| d.code == codes::NESTING_TOO_DEEP),
                    "{name}: refused a tree that fits"
                );
            }
        });
    }

    #[test]
    fn a_deep_operand_followed_by_a_long_chain() {
        // The exact counterexample that broke the previous bound, kept by name so the
        // 49-pair search never becomes the only thing standing between this and a
        // regression. 510 negations then 510 additions: the negations are parsed and
        // their levels released before the first `+` is counted, so a parser-recursion
        // counter sees 511 while the tree is 1020 deep. Measured, it was accepted at
        // 1022 against a limit of 512.
        on_the_documented_stack(|| {
            let body = format!("{}1{}", "- ".repeat(510), " + 1".repeat(510));
            let src = format!("fn main() -> Int {{ {body} }}");
            let (program, errors) = parse_source(&src);
            assert!(
                errors.iter().any(|d| d.code == codes::NESTING_TOO_DEEP),
                "the composition that was accepted at 1022 deep: {errors:#?}"
            );
            assert!(
                depth(&program) <= MAX_NESTING,
                "even rejected, nothing over the limit may be retained"
            );
        });
    }

    #[test]
    fn parentheses_do_not_consume_the_budget() {
        // `(x)` is `x`, so grouping must cost nothing. Counting the punctuation would
        // refuse ordinary parenthesised arithmetic for depth it does not have — the
        // over-conservative direction, which no "never too deep" assertion can see.
        // Parentheses around the leftmost operand of a long chain: the parser recurses
        // once per paren and the chain adds nothing to that, while the chain drives the
        // tree depth and the parens add nothing to that. Each bound sees only its own
        // half, which is the point — counting the punctuation as a node would make the
        // tree measure 800 for a tree of 401.
        on_the_documented_stack(|| {
            let src = format!(
                "fn main() -> Int {{ {}1{}{} }}",
                "(".repeat(400),
                ")".repeat(400),
                " + 1".repeat(400)
            );
            let (program, errors) = parse_source(&src);
            assert!(
                errors.is_empty(),
                "400 parens around the operand of a 400-long chain: {errors:#?}"
            );
            let d = depth(&program);
            assert!(
                d < 410,
                "tree is {d} deep for a 400-long chain — the parentheses are nodes"
            );
        });
    }

    #[test]
    fn a_deep_condition_counts_toward_the_tree() {
        // Every `if` in `compositions` has the condition `true`, so a condition is the
        // one child that search never makes deep. It is a child like any other: the
        // checker and the evaluator walk into it exactly as far.
        on_the_documented_stack(|| {
            let cond = format!("1{} == 0", " + 1".repeat(400));
            let src = format!(
                "fn main() -> Int {{ if {cond} {{ 1 }} else {{ 0 }}{} }}",
                " + 1".repeat(200)
            );
            let (program, errors) = parse_source(&src);
            assert!(
                errors.iter().any(|d| d.code == codes::NESTING_TOO_DEEP),
                "a 400-deep condition beneath a 200-long chain is over the limit: \
                 {errors:#?}"
            );
            assert!(
                depth(&program) <= MAX_NESTING,
                "retained a tree {} deep",
                depth(&program)
            );
        });
    }

    #[test]
    fn a_deep_condition_within_the_limit_is_accepted() {
        // The other direction, so the test above cannot be satisfied by refusing every
        // condition: the same shape, small enough to fit.
        on_the_documented_stack(|| {
            let cond = format!("1{} == 0", " + 1".repeat(100));
            let src = format!(
                "fn main() -> Int {{ if {cond} {{ 1 }} else {{ 0 }}{} }}",
                " + 1".repeat(100)
            );
            let (_, errors) = parse_source(&src);
            assert!(errors.is_empty(), "{errors:#?}");
        });
    }

    #[test]
    fn a_chain_does_not_spend_the_recursion_budget() {
        // A Pratt loop holds no frame per operator, so a chain must not be charged for
        // parser recursion. An earlier version charged it, and refused this for a budget
        // it never needed: 200 parentheses (201 frames) around a 400-operator chain
        // (a tree 401 deep), both comfortably inside their own limits, rejected for the
        // 600 of their sum. Pinned so the widening cannot drift back.
        on_the_documented_stack(|| {
            let src = format!(
                "fn main() -> Int {{ {}1{}{} }}",
                "(".repeat(200),
                " + 1".repeat(400),
                ")".repeat(200)
            );
            let (program, errors) = parse_source(&src);
            assert!(errors.is_empty(), "{errors:#?}");
            let d = depth(&program);
            assert!(d <= MAX_NESTING, "tree is {d} deep");
        });
    }

    #[test]
    fn nothing_over_the_limit_is_ever_retained() {
        // Rejection has to happen before an oversized parent is kept, not after. What is
        // handed back is what the checker, the evaluator and the destructor will walk,
        // so an over-deep tree returned alongside an error is still an over-deep tree.
        on_the_documented_stack(|| {
            // Shapes refused outright, where the partial tree built before the limit
            // fired is what gets handed back and then dropped.
            //
            // One layer past the configured limit, rather than a round number chosen by
            // eye: the smallest input that probes the bound, so the probe moves with
            // `MAX_NESTING` instead of with whatever stack this machine happened to
            // have. It is not a guarantee of a clean failure — with a bound mutated
            // away, whether 513 levels abort still depends on stack size and frame size,
            // and a small enough stack would crash here rather than fail an assertion.
            // Containment for that case is the CLI suite, where the program runs as a
            // child process and a crash is an exit code the parent can assert on.
            //
            // Parentheses are absent from this list on purpose. They build no node, so
            // the tree bound cannot see them and the recursion bound is the only thing
            // refusing them — which leaves an over-limit paren probe unbounded the
            // moment that bound is removed, with the stack rather than the limit
            // deciding what happens. `examples/bad/paren_nesting.nz` covers them from
            // the CLI suite.
            let over_limit = MAX_NESTING + 1;
            for over in [
                format!("fn main() -> Int {{ {}1 }}", "- ".repeat(over_limit)),
                format!("fn main() -> Int {{ 1{} }}", " + 1".repeat(over_limit)),
                format!(
                    "fn main() -> Int {{ {}1{} }}",
                    "{ ".repeat(over_limit),
                    " }".repeat(over_limit)
                ),
                format!("{}1{}", "- ".repeat(510), " + 1".repeat(510)),
            ] {
                let (program, errors) = parse_source(&over);
                assert!(!errors.is_empty(), "should have been refused");
                assert!(
                    depth(&program) <= MAX_NESTING,
                    "retained an over-deep tree after refusing it"
                );
            }
        });
    }

    #[test]
    fn a_composition_at_its_own_edge_is_still_within_the_limit() {
        // Compositions well inside the limit prove nothing about the limit. This finds
        // where each pair is actually refused and checks the deepest tree still accepted
        // there — which is where a counter released too early would show up, by letting
        // the outer layers stop being counted once the inner ones finished.
        const KINDS: [&str; 7] = ["paren", "neg", "block", "call", "if", "chain", "mixed"];
        on_the_documented_stack(|| {
            for outer in KINDS {
                for inner in KINDS {
                    let build = |n: usize| {
                        format!(
                            "fn f(n: Int) -> Int {{ n }}\nfn main() -> Int {{ {} }}",
                            layer(outer, n, &layer(inner, n, "1"))
                        )
                    };
                    let refused = |n: usize| {
                        parse_source(&build(n))
                            .1
                            .iter()
                            .any(|d| d.code == codes::NESTING_TOO_DEEP)
                    };
                    let (mut lo, mut hi) = (1usize, MAX_NESTING);
                    while hi - lo > 1 {
                        let mid = (lo + hi) / 2;
                        if refused(mid) {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                    let (program, errors) = parse_source(&build(lo));
                    assert!(errors.is_empty(), "{outer}+{inner} at {lo}: {errors:#?}");
                    let d = depth(&program);
                    assert!(
                        d <= MAX_NESTING,
                        "{outer}+{inner}: accepted a tree {d} deep at {lo} layers each, \
                         limit is {MAX_NESTING}"
                    );
                    assert!(
                        refused(lo + 1),
                        "{outer}+{inner}: {} should be refused",
                        lo + 1
                    );
                }
            }
        });
    }

    #[test]
    fn the_counter_tracks_the_tree_at_the_edge_of_each_shape() {
        // At the edge the two must agree closely, or the counter is bounding something
        // other than what the later passes walk.
        on_the_documented_stack(|| {
            for kind in ["paren", "neg", "block", "call", "if", "chain", "mixed"] {
                // Bounded, and the bound matters more than the step. An open-ended
                // search for "the depth at which this is refused" stops being a search
                // the moment nothing is refused — it becomes a climb until the stack
                // gives out, which is a crash rather than a report and takes the rest of
                // the binary with it. Three times the limit is past any real edge, and
                // reaching the end without a refusal is itself the failure.
                let mut edge = 0;
                let ceiling = MAX_NESTING * 3;
                let mut refused_somewhere = false;
                for n in (1..=ceiling).step_by(8) {
                    let src = format!(
                        "fn f(n: Int) -> Int {{ n }}\nfn main() -> Int {{ {} }}",
                        layer(kind, n, "1")
                    );
                    if parse_source(&src)
                        .1
                        .iter()
                        .any(|d| d.code == codes::NESTING_TOO_DEEP)
                    {
                        refused_somewhere = true;
                        break;
                    }
                    edge = n;
                }
                assert!(
                    refused_somewhere,
                    "{kind}: never refused up to {ceiling} layers — nothing is bounding \
                     this shape, so there is no edge to measure"
                );
                let src = format!(
                    "fn f(n: Int) -> Int {{ n }}\nfn main() -> Int {{ {} }}",
                    layer(kind, edge, "1")
                );
                let (program, errors) = parse_source(&src);
                assert!(errors.is_empty(), "{kind}: {errors:#?}");
                let d = depth(&program);
                assert!(
                    d <= MAX_NESTING,
                    "{kind}: deepest accepted tree is {d}, limit is {MAX_NESTING}"
                );

                // Parentheses are the one shape that builds no node at all: `((((1))))`
                // parses to `Int 1`, depth 2 however many there were. What the counter
                // bounds for them is the parser's own recursion, which is real — the
                // parser is a stack consumer like the passes after it — so the limit is
                // doing its job while the tree stays trivial.
                if kind == "paren" {
                    assert_eq!(d, 2, "parentheses should build no node");
                    continue;
                }

                // For every shape that does build nodes, the counter has to track the
                // tree closely, or it is bounding something the later passes do not walk
                // and the real depth could run past it. Measured: 498 to 506 against 512.
                assert!(
                    d * 2 >= MAX_NESTING,
                    "{kind}: deepest accepted tree is only {d} against a limit of \
                     {MAX_NESTING} — the counter is tracking something else"
                );
            }
        });
    }
}
