//! Recursive descent, with Pratt precedence for binary operators.
//!
//! Handwritten for the reasons in `lexer.rs`, plus one specific to parsing: a construct
//! the language does not support yet should be **named** in the error, not reported as an
//! unexpected token. `while` is not a stray identifier; saying so costs a match arm and
//! saves whoever hits it from guessing whether they mistyped something.
//!
//! # One parser, two trees
//!
//! Since N15 every decision here builds two things from the same tokens: the abstract tree
//! the checker reads, and the lossless concrete tree in [`crate::cst`]. There is no second
//! parser and no second grammar. Every consumed token goes through [`Parser::bump`], which
//! is also what puts its leaf — and the trivia before it — into the concrete tree; every
//! construct takes a checkpoint before its first token and wraps its node once it has been
//! recognised, at the same point it builds its abstract node. A construct that fails part
//! way wraps nothing, so no concrete node is ever left open by an early `?`.
//!
//! **This file is the boundary where concrete syntax becomes abstract syntax.** It is not a
//! pass over the concrete tree: nothing walks the concrete tree to produce the abstract
//! one, and nothing downstream of here reads the concrete tree at all. That is the
//! transitional shape recorded in `architecture.md`; what makes it one grammar rather than
//! two is that neither tree can be built without the other's decision, and
//! `crates/nazm-syntax/tests/cst.rs` checks that the two agree on every construct's span.
//!
//! # Recovery
//!
//! A statement that does not parse no longer loses the rest of its function. The block
//! wraps what was consumed in an error node, skips to the next place a statement can start
//! — a `;`, the block's own `}`, or a statement keyword — and carries on, so later
//! statements are still in the concrete tree and a later independent mistake is still
//! reported. The **abstract** tree keeps its old contract: a function whose body had a
//! syntax error has no body, so the checker never sees a statement list with a hole in it.

use nazm_diag::{codes, Diagnostic};
use nazm_span::{FileId, Span};

use crate::ast::{
    Assign, BinOp, Block, EffectDecl, Enum, Expr, Field, FieldInit, FnTypeName, ForeignDecl,
    ForeignStruct, Function, HoleSite, ImplDecl, Import, Let, MatchArm, MethodOf, Param,
    PatternField, Place, PlaceField, Program, Qualifier, Record, Requirement, Return, Scope, Spawn,
    Stmt, Trait, TraitMethod, TypeName, TypeParam, Variant, While,
};
use crate::cst::{Checkpoint, Cst, Sink, SyntaxKind};
use crate::lexer::{Lexeme, Token, TokenKind};

/// Constructs that are deliberately absent from v0.1, so they can be refused by name.
const RESERVED: &[(&str, &str)] = &[
    ("for", "there is no `for`; iterate with `while`"),
    (
        "label",
        "there are no loop labels; `break` leaves the innermost loop",
    ),
    ("loop", "there is no `loop`; write `while true`"),
    // `struct` left this list in N9, and `enum` and `match` in N10, each when it became
    // the keyword it was reserving. The entries existed to refuse words people would
    // reach for before they worked.
    // `use` left this list when imports arrived; `mod` stays, because there is no
    // module *declaration* — a file is the unit, and `use` names one by path.
    (
        "mod",
        "a file is the module; import one with `use \"other.nz\";`",
    ),
    // `impl` left this list in N78, when it became the keyword it was reserving.
    // `print` left this list when `print` became a built-in. The entry existed to refuse
    // a name people would reach for before it worked; now it works, and refusing it
    // would be refusing the thing it was reserving.
];

/// What the language does have, for the help text on a refused construct.
const SUPPORTED: &str = concat!(
    "the supported subset is functions, `let` and `let mut`, assignment, `while` with ",
    "`break` and `continue`, `return`, `if`/`else` as a statement or an expression, ",
    "arithmetic and comparison over `Int`, `Bool` and `Str`, the `Ints` and `Strs` ",
    "sequences, `struct` records with named fields, `enum` variants with `match`, and the ",
    "built-in operations on them — see docs/spec.md"
);

#[derive(Debug)]
pub struct Parser<'a> {
    /// The file being read, for spans invented where no token exists.
    file: FileId,
    /// The scan's tokens, borrowed from its lexemes rather than copied out of them, behind
    /// a view that remembers the furthest one read (N76).
    tokens: Toks<'a>,
    pos: usize,
    errors: Vec<Diagnostic>,
    /// Current parser recursion, against [`MAX_NESTING`].
    depth: usize,
    /// Depth of the subtree most recently built, against [`MAX_NESTING`].
    built: usize,
    /// Whether the limit has already been reported for this item.
    reported_too_deep: bool,
    /// The concrete tree, built by the same decisions. N15.
    sink: Sink<'a>,
    /// Whether a statement of the current item failed and was recovered from. A function
    /// whose body recovered gets no abstract body, exactly as before recovery existed.
    poisoned: bool,
    /// Where the current item began, for [`Parser::delimiters_balanced`].
    item_start: usize,
    /// Whether the current item's declaration — its header or member list — needed
    /// recovery (N76): its concrete node stands, and it is left out of the abstract tree.
    drop_item: bool,
    /// Whether syntax diagnostics are being withheld as cascades of one already reported.
    /// See [`Parser::report`].
    muted: bool,
    /// For a completion probe (N26): the byte offset of the editor's cursor, until a hole
    /// is found there. `None` for every ordinary parse, and after the hole is taken.
    hole: Option<u32>,
    /// The hole found, if any.
    hole_site: Option<HoleSite>,
    /// Whether the statement holding the hole is still being read. While it is, the `;`
    /// or `)` the unfinished construct leaves unwritten is taken as written — only then,
    /// and only those — so the statement ends where the author has not yet ended it.
    hole_open: bool,
}

/// The parser's view of the token list: every read goes through it, and it remembers the
/// furthest index read since it was last reset.
///
/// That high-water mark is how far an item's parse *looked*, which is not how far it
/// consumed: the delimiter-balance scan behind cascade muting reads forward to the next
/// item that must begin a new one. Incremental reparse (N76) may reuse an item only if
/// nothing it read has changed, so it needs exactly this number, and the convergence tests
/// are what would notice a read that bypassed it.
#[derive(Debug)]
struct Toks<'a> {
    all: &'a [&'a Token],
    seen: std::cell::Cell<usize>,
}

impl<'a> Toks<'a> {
    fn get(&self, index: usize) -> Option<&'a Token> {
        // Past the end is `Eof`'s business: reading there is reading the end of the file.
        let noted = index.min(self.all.len().saturating_sub(1));
        if noted > self.seen.get() {
            self.seen.set(noted);
        }
        self.all.get(index).copied()
    }

    /// The tokens `from..to`, every one of them counted as read.
    fn range(&self, from: usize, to: usize) -> &'a [&'a Token] {
        if to > from {
            let _ = self.get(to - 1);
        }
        self.all.get(from..to).unwrap_or_default()
    }

    /// Start counting again, from `index`.
    fn reset(&self, index: usize) {
        self.seen.set(index);
    }
}

/// One top-level item's parse: what [`crate::incremental`] keeps per chunk (N76).
#[derive(Debug, Clone)]
pub(crate) struct ChunkParse {
    /// Its first token's index, and one past its last consumed token's.
    pub start: usize,
    pub end: usize,
    /// The furthest token its parse read.
    pub seen: usize,
    /// Whether diagnostics were muted as cascades when it began.
    pub muted: bool,
    /// Whether they were muted when it ended — the next chunk's entry state.
    pub muted_after: bool,
    /// The abstract item it produced, if it produced one.
    pub item: Option<Item>,
    /// Every syntax diagnostic reported while parsing it, in order.
    pub errors: Vec<Diagnostic>,
}

/// One top-level item of the abstract tree.
///
/// Sized by its largest variant on purpose: an item is built once and moved straight into
/// the `Arc` its chunk keeps, so boxing a function would be one allocation more per item
/// and nothing saved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Item {
    Function(Function),
    Record(Record),
    Enum(Enum),
    Import(Import),
    /// A trait declaration (N78).
    Trait(Trait),
    /// An impl: its header and its methods, each a function marked as the impl's.
    Impl(ImplDecl, Vec<Function>),
}

/// Where a parse of top-level items starts and where it may stop (N76).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Region<'r> {
    /// The token the first item begins at.
    pub start: usize,
    /// The cascade mute in force there.
    pub muted: bool,
    /// Ascending `(token, mute)` pairs: the parse stops at the first item boundary that is
    /// one of these tokens with that mute in force. Empty for a full parse.
    pub landings: &'r [(usize, bool)],
}

/// The result of parsing a [`Region`].
#[derive(Debug)]
pub(crate) struct RegionParse {
    pub chunks: Vec<ChunkParse>,
    /// The token the parse stopped at: a landing, or the end of the file.
    pub stop: usize,
    /// The root-level green children of the region, in order, and the strings they use.
    pub green: cstree::green::GreenNode,
    pub strings: crate::cst::Strings,
    pub hole: Option<HoleSite>,
}

/// What one position in a block turned out to hold.
#[derive(Debug)]
enum Parsed {
    Stmt(Stmt),
    /// An expression with no `;`, followed by the block's `}`: the block's value.
    Tail(Expr),
}

/// Parse one file's lossless scan into both trees: the abstract [`Program`] and the
/// concrete [`Cst`], with every syntax error found.
///
/// `tokens` is the scan's [`Lexeme::Token`]s, borrowed in order; the caller passes both so
/// the scan is done once and no token's text is held twice. `text` is the source the scan was of, from which every leaf takes its bytes.
pub(crate) fn parse_lossless(
    file: FileId,
    text: &str,
    lexemes: &[Lexeme],
    tokens: &[&Token],
    hole: Option<u32>,
) -> (Program, Vec<Diagnostic>, Cst) {
    let region = Region {
        start: 0,
        muted: false,
        landings: &[],
    };
    let parsed = parse_region(
        file,
        text,
        lexemes,
        tokens,
        region,
        hole,
        crate::cst::Strings::default(),
    );
    let mut program = Program {
        functions: Vec::new(),
        records: Vec::new(),
        enums: Vec::new(),
        imports: Vec::new(),
        traits: Vec::new(),
        impls: Vec::new(),
        hole: parsed.hole,
    };
    let mut errors = Vec::new();
    for c in parsed.chunks {
        errors.extend(c.errors);
        if let Some(item) = c.item {
            item.push_into(&mut program);
        }
    }
    (
        program,
        errors,
        Cst::from_green(file, parsed.green, parsed.strings),
    )
}

impl Item {
    /// Put this item where a [`Program`] keeps its kind, after the ones already there.
    pub(crate) fn push_into(self, program: &mut Program) {
        match self {
            Self::Function(f) => program.functions.push(f),
            Self::Record(r) => program.records.push(r),
            Self::Enum(e) => program.enums.push(e),
            Self::Import(i) => program.imports.push(i),
            Self::Trait(t) => program.traits.push(t),
            Self::Impl(decl, methods) => {
                program.impls.push(decl);
                program.functions.extend(methods);
            }
        }
    }
}

/// Parse top-level items from `region.start` until a landing or the end of the file.
///
/// `lexemes` begins at the lexeme the region's first item owns — its leading trivia, or
/// its first token — and `tokens` is the **whole** file's token list, because an item's
/// parse may read past its own end. The green tree returned is a `SourceFile` node whose
/// children are the region's root-level children: what a full parse puts at the root for
/// these items, and, when the region reaches the end of the file, the trailing trivia.
pub(crate) fn parse_region(
    file: FileId,
    text: &str,
    lexemes: &[Lexeme],
    tokens: &[&Token],
    region: Region<'_>,
    hole: Option<u32>,
    strings: crate::cst::Strings,
) -> RegionParse {
    let mut p = Parser {
        file,
        tokens: Toks {
            all: tokens,
            seen: std::cell::Cell::new(region.start),
        },
        pos: region.start,
        errors: Vec::new(),
        depth: 0,
        built: 0,
        reported_too_deep: false,
        sink: Sink::new(text, lexemes),
        poisoned: false,
        item_start: region.start,
        drop_item: false,
        muted: region.muted,
        hole,
        hole_site: None,
        hole_open: false,
    };
    let mut chunks = Vec::new();
    let mut landings = region.landings.iter().peekable();

    while !p.at(&TokenKind::Eof) {
        // A landing behind the parse was crossed by an item that grew past it.
        while landings.next_if(|(at, _)| *at < p.pos).is_some() {}
        if landings
            .peek()
            .is_some_and(|(at, muted)| *at == p.pos && *muted == p.muted)
        {
            break;
        }
        let at = p.checkpoint();
        let start = p.pos;
        let entry_muted = p.muted;
        p.tokens.reset(start);
        // Per item: abandoning one function unwinds out of `leave()` calls that never
        // ran, and a stale count would shrink the budget for every function after it.
        p.depth = 0;
        p.built = 0;
        p.reported_too_deep = false;
        p.poisoned = false;
        p.drop_item = false;
        p.item_start = start;
        // A cascade ends where a new item visibly begins. Anything else here is what an
        // item whose braces did not balance left behind, and stays muted.
        if matches!(
            p.peek(),
            TokenKind::Fn
                | TokenKind::Pub
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Use
                | TokenKind::Trait
                | TokenKind::Impl
        ) {
            p.muted = false;
        }
        let item = if p.at(&TokenKind::Use) {
            p.import().map(Item::Import)
        } else if p.at(&TokenKind::Impl) {
            p.impl_decl().map(|(d, m)| Item::Impl(d, m))
        } else {
            // `struct`/`enum`, each optionally behind a `pub`. Looking one token past a
            // `pub` is what keeps the kinds of item apart without any grammar knowing
            // about another's.
            let mut next = p.pos + usize::from(p.at(&TokenKind::Pub));
            // `extern "C" struct` (N85) is a record; `extern "C" fn` stays a function.
            if matches!(p.tokens.get(next).map(|t| &t.kind), Some(TokenKind::Ident(w)) if w == "extern")
                && matches!(
                    p.tokens.get(next + 1).map(|t| &t.kind),
                    Some(TokenKind::Str(_))
                )
                && matches!(
                    p.tokens.get(next + 2).map(|t| &t.kind),
                    Some(TokenKind::Struct)
                )
            {
                next += 2;
            }
            let after_pub = p.tokens.get(next).map(|t| t.kind.clone());
            match after_pub {
                Some(TokenKind::Struct) => p.record().map(Item::Record),
                Some(TokenKind::Enum) => p.enumeration().map(Item::Enum),
                Some(TokenKind::Trait) => p.trait_decl().map(Item::Trait),
                _ => p.function().map(Item::Function),
            }
        };
        // A declaration that needed recovery is in the concrete tree and not the abstract
        // one: the checker never sees a declaration with a hole in it (N76).
        let item = item.filter(|_| !p.drop_item);
        if item.is_none() && !p.drop_item {
            // Skip to the next plausible item so one bad item does not cascade into an
            // error per token for the rest of the file. What was read and what was
            // skipped are one error node: they are the author's bytes either way.
            p.recover_to_item(start);
            p.wrap(at, SyntaxKind::Error);
        }
        chunks.push(ChunkParse {
            start,
            end: p.pos,
            seen: p.tokens.seen.get().max(p.pos),
            muted: entry_muted,
            muted_after: p.muted,
            item,
            errors: std::mem::take(&mut p.errors),
        });
    }

    let stop = p.pos;
    let at_end = p.at(&TokenKind::Eof);
    let Parser {
        sink, hole_site, ..
    } = p;
    let (green, strings) = sink.finish_green(strings, at_end);
    RegionParse {
        chunks,
        stop,
        green,
        strings,
        hole: hole_site,
    }
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

    /// Record a syntax diagnostic, unless it would be a cascade.
    ///
    /// Recovery inside a block reports a later mistake as its own diagnostic, and that is
    /// the point of it: two independent mistakes in one function are two diagnostics. But
    /// when the item's delimiters do not balance, where a later statement begins and which
    /// block it belongs to is exactly what the mistake made ambiguous, and every report
    /// after the first would be that one mistake seen from somewhere else — a missing `}`
    /// found again as a stray token at the top level, a missing `(` found again as a stray
    /// `)`. So once such an item has recovered, its further diagnostics are withheld until
    /// the next item begins. The first is never withheld: it is reported before recovery
    /// decides. The concrete tree is unaffected: it holds every byte either way.
    fn report(&mut self, d: Diagnostic) {
        if !self.muted {
            self.errors.push(d);
        }
    }

    /// Whether the delimiters of the current item balance — braces, parentheses and
    /// brackets, each on its own — from where it began to the next token that can only
    /// start an item (`fn`, `pub`) or the end of the file.
    fn delimiters_balanced(&self) -> bool {
        let mut open = [0usize; 3];
        // Every token read through the view, one at a time, so the furthest one this scan
        // reached is exactly what incremental reparse sees it reached (N76).
        let rest = |i: usize| self.tokens.get(self.item_start + i);
        // The item's own leading `pub` and `fn` are where it starts, not where the next
        // one does.
        let skip = (0..)
            .take_while(|&i| {
                rest(i).is_some_and(|t| matches!(t.kind, TokenKind::Pub | TokenKind::Fn))
            })
            .count();
        let mut i = skip;
        while let Some(t) = rest(i) {
            i += 1;
            // `fn NAME` begins the next item; `fn(` is a closure or a function type (N50).
            let item_fn = matches!(t.kind, TokenKind::Fn)
                && matches!(rest(i).map(|n| &n.kind), Some(TokenKind::Ident(_)));
            let (slot, opens) = match t.kind {
                TokenKind::Fn if !item_fn => continue,
                TokenKind::Fn | TokenKind::Pub | TokenKind::Eof => break,
                TokenKind::LBrace => (0, true),
                TokenKind::RBrace => (0, false),
                TokenKind::LParen => (1, true),
                TokenKind::RParen => (1, false),
                TokenKind::LBracket => (2, true),
                TokenKind::RBracket => (2, false),
                _ => continue,
            };
            let Some(n) = open.get_mut(slot) else {
                continue;
            };
            if opens {
                *n += 1;
            } else if let Some(m) = n.checked_sub(1) {
                *n = m;
            } else {
                return false;
            }
        }
        open == [0, 0, 0]
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
        self.report(
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

    /// Consume the current token. **The only place the parser moves forward**, and the
    /// only place a leaf enters the concrete tree, so the two cannot disagree about which
    /// tokens were read. `Eof` is never consumed; it owns no byte.
    fn bump(&mut self) {
        if !matches!(self.peek(), TokenKind::Eof) {
            self.sink.token();
            self.pos += 1;
        }
    }

    /// Where a construct starts, for [`Parser::wrap`] once it has been recognised.
    fn checkpoint(&self) -> Checkpoint {
        self.sink.checkpoint()
    }

    /// Give the construct that started at `at` its concrete node.
    fn wrap(&mut self, at: Checkpoint, kind: SyntaxKind) {
        self.sink.wrap(at, kind);
    }

    fn advance(&mut self) -> TokenKind {
        let k = self.peek().clone();
        self.bump();
        k
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, context: &str) -> Option<Span> {
        if self.at(kind) {
            let s = self.peek_span();
            self.bump();
            return Some(s);
        }
        // A probe's unfinished statement: the `;` or `)` its hole left unwritten (N26).
        if self.hole_open && matches!(kind, TokenKind::Semi | TokenKind::RParen) {
            let at = self.peek_span().start;
            return Some(Span::new(self.file, at, at));
        }
        let found = self.peek().describe();
        let span = self.peek_span();
        self.report(
            Diagnostic::error(
                codes::UNEXPECTED_TOKEN,
                span,
                format!("expected {} {context}, found {found}", kind.describe()),
            )
            .primary_label(format!("expected {}", kind.describe())),
        );
        None
    }

    /// Whether the token at `index` ends exactly at the probe's hole (N26).
    fn ends_at_hole(&self, index: usize) -> bool {
        self.hole
            .is_some_and(|h| self.tokens.get(index).is_some_and(|t| t.span.end == h))
    }

    /// Whether the token at `index` is an identifier.
    fn ident_at(&self, index: usize) -> bool {
        matches!(
            self.tokens.get(index).map(|t| &t.kind),
            Some(TokenKind::Ident(_))
        )
    }

    /// Whether a label — an identifier and its `:` — starts at `index`: the grammar's own
    /// test for a construction's or pattern's field, which a hole never is.
    fn label_at(&self, index: usize) -> bool {
        self.ident_at(index)
            && matches!(
                self.tokens.get(index + 1).map(|t| &t.kind),
                Some(TokenKind::Colon)
            )
    }

    /// Take the hole: it is found once, and the statement holding it is now being read.
    fn take_hole(&mut self, site: HoleSite) -> Span {
        let at = self.hole.take().unwrap_or_default();
        self.hole_site = Some(site);
        self.hole_open = true;
        Span::new(self.file, at, at)
    }

    /// `receiver.` at the hole: the projection of an empty member name (N26). Out of line,
    /// like every hole below: no ordinary parse reaches them.
    #[cold]
    #[inline(never)]
    fn member_hole(&mut self, at: Checkpoint, base: Expr, depth: usize) -> Option<Expr> {
        let name_span = self.hole_span();
        self.advance();
        self.take_hole(HoleSite::Member { name: name_span });
        self.built(depth)?;
        self.wrap(at, SyntaxKind::FieldExpr);
        Some(Expr::Field {
            span: base.span().join(name_span),
            base: Box::new(base),
            name: String::new(),
            name_span,
        })
    }

    /// `Name[T, …].` at the hole: an applied enum's construction of an empty variant name.
    #[cold]
    #[inline(never)]
    fn variant_hole(
        &mut self,
        at: Checkpoint,
        name: String,
        span: Span,
        args: Vec<TypeName>,
    ) -> Option<Expr> {
        let variant_span = self.hole_span();
        self.advance();
        self.take_hole(HoleSite::Member { name: variant_span });
        self.built(1)?;
        self.wrap(at, SyntaxKind::VariantExpr);
        Some(Expr::Variant {
            qualifier: None,
            ty: name,
            ty_span: span,
            type_args: Some(args),
            variant: String::new(),
            variant_span,
            fields: Vec::new(),
            span: span.join(variant_span),
        })
    }

    /// `Name(` at the hole: a call of `Name` with nothing written, as the grammar reads it.
    #[cold]
    #[inline(never)]
    fn call_hole(
        &mut self,
        at: Checkpoint,
        name: String,
        span: Span,
        type_args: Option<Vec<TypeName>>,
    ) -> Option<Expr> {
        let args_at = self.checkpoint();
        self.bump();
        self.take_hole(HoleSite::Call { callee: span });
        let close = self.expect(&TokenKind::RParen, "to close the argument list")?;
        self.wrap(args_at, SyntaxKind::ArgList);
        self.built(1)?;
        self.wrap(at, SyntaxKind::CallExpr);
        Some(Expr::Call {
            qualifier: None,
            callee: name,
            callee_span: span,
            type_args,
            args: Vec::new(),
            span: span.join(close),
        })
    }

    /// The zero-width span at the hole, for a node whose name the hole is.
    fn hole_span(&self) -> Span {
        let at = self.hole.unwrap_or_default();
        Span::new(self.file, at, at)
    }

    /// Skip to the next token that can start an item.
    ///
    /// `start` is where the failed item began. If nothing has been consumed since, one
    /// token is, whatever it is: a failure that read nothing, sitting on a token that
    /// Whether the next tokens begin a function declaration, `fn NAME`, rather than a
    /// closure or a function type, `fn(` (N50): what recovery stops at.
    fn at_item_fn(&self) -> bool {
        self.at(&TokenKind::Fn) && self.ident_at(self.pos + 1)
    }

    /// starts an item, would otherwise be retried at the same place forever.
    fn recover_to_item(&mut self, start: usize) {
        if self.pos == start {
            self.bump();
        }
        while !self.at(&TokenKind::Eof) && !self.at_item_start() {
            self.bump();
        }
    }

    /// Reject a deliberately-absent construct by name.
    fn reserved(&mut self, name: &str, span: Span) -> bool {
        if let Some((_, why)) = RESERVED.iter().find(|(k, _)| *k == name) {
            self.report(
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
        let first = self.pos;
        let parsed = self.import_inner();
        if parsed.is_none() {
            // A broken import ends at its own `;` when it has one before the next item
            // (N76): the item after it is not the import's to lose.
            while self.pos > first
                && !self.at(&TokenKind::Semi)
                && !self.at(&TokenKind::Eof)
                && !self.at_item_start()
            {
                self.bump();
            }
            self.eat(&TokenKind::Semi);
        }
        parsed
    }

    /// Whether the next token can only begin a top-level item: `fn NAME`, `pub`, `struct`,
    /// `enum` or `use`.
    fn at_item_start(&self) -> bool {
        self.at_item_fn()
            || self.at(&TokenKind::Pub)
            || self.at(&TokenKind::Struct)
            || self.at(&TokenKind::Enum)
            || self.at(&TokenKind::Use)
            || self.at(&TokenKind::Trait)
            || self.at(&TokenKind::Impl)
    }

    fn import_inner(&mut self) -> Option<Import> {
        let at = self.checkpoint();
        let start = self.expect(&TokenKind::Use, "to start an import")?;
        let path_span = self.peek_span();
        let path = match self.advance() {
            TokenKind::Str(p) => p,
            other => {
                self.report(
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
        // `as name` (N49): `as` is a word here and nowhere else, so no identifier is taken.
        let alias = if matches!(self.peek(), TokenKind::Ident(w) if w == "as") {
            self.advance();
            let alias_span = self.peek_span();
            match self.advance() {
                TokenKind::Ident(name) => Some(Qualifier {
                    name,
                    span: alias_span,
                }),
                other => {
                    self.report(
                        Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            alias_span,
                            format!("expected a name after `as`, found {}", other.describe()),
                        )
                        .help("an import under a name is written `use \"other.nz\" as other;`"),
                    );
                    return None;
                }
            }
        } else {
            None
        };
        let end = self.expect(&TokenKind::Semi, "to end an import")?;
        self.wrap(at, SyntaxKind::UseItem);
        Some(Import {
            path,
            path_span,
            alias,
            span: start.join(end),
        })
    }

    /// `scope { … }`
    fn scope(&mut self) -> Option<Scope> {
        let at = self.checkpoint();
        let start = self.expect(&TokenKind::Scope, "to start a scope")?;
        let body = self.block()?;
        self.wrap(at, SyntaxKind::ScopeStmt);
        Some(Scope {
            span: start.join(body.span),
            body,
        })
    }

    /// `spawn f(args);`
    fn spawn(&mut self) -> Option<Spawn> {
        let at = self.checkpoint();
        let start = self.expect(&TokenKind::Spawn, "to start a task")?;
        let callee_span = self.peek_span();
        let callee = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.report(
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
        let list = self.checkpoint();
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
        self.wrap(list, SyntaxKind::ArgList);
        let end = self.expect(&TokenKind::Semi, "to end a `spawn`")?;
        self.wrap(at, SyntaxKind::SpawnStmt);
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
        let at = self.checkpoint();
        let start = self.peek_span();
        let pub_span = self.at(&TokenKind::Pub).then(|| {
            let s = self.peek_span();
            self.advance();
            s
        });
        // `extern "C"` (N85): the dispatcher only sends a record here with it when a string and
        // `struct` follow, so the two tokens are there.
        let mut foreign: Option<ForeignStruct> = None;
        if matches!(self.peek(), TokenKind::Ident(w) if w == "extern") {
            self.advance();
            let abi_span = self.peek_span();
            if let TokenKind::Str(abi) = self.advance() {
                foreign = Some(ForeignStruct {
                    abi,
                    abi_span,
                    opaque: false,
                });
            }
        }
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
                self.report(
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
        let type_params = self.only_type_params()?;

        // `extern "C" struct Name;`: an opaque handle, with no field list at all (N85).
        if let Some(f) = foreign.as_mut() {
            if self.at(&TokenKind::Semi) {
                let close = self.peek_span();
                self.advance();
                f.opaque = true;
                self.wrap(at, SyntaxKind::RecordItem);
                self.drop_item = self.poisoned;
                return Some(Record {
                    name,
                    name_span,
                    pub_span,
                    type_params,
                    fields: Vec::new(),
                    foreign,
                    span: start.join(close),
                });
            }
        }

        let list = self.checkpoint();
        self.expect(&TokenKind::LBrace, "to start the fields")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let field = self.checkpoint();
            let start = self.pos;
            match self.field_decl("each field is written `name: Type,`", "after a field name") {
                Some(f) => fields.push(f),
                None => {
                    if !self.recover_member(field, start, &TokenKind::RBrace) {
                        return None;
                    }
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(&TokenKind::RBrace, "to close the fields")?;
        self.wrap(list, SyntaxKind::FieldList);
        self.wrap(at, SyntaxKind::RecordItem);
        self.drop_item = self.poisoned;
        Some(Record {
            name,
            name_span,
            pub_span,
            type_params,
            fields,
            foreign,
            span: start.join(close),
        })
    }

    /// `enum Name { Variant, Other(field: Type, …), … }`, with an optional `pub` in front.
    ///
    /// A zero-payload variant is written with no parentheses and a variant with a payload
    /// names every field, exactly as a record declaration does. There is no second
    /// spelling for either: `Eof()` in a declaration would be a field list with no fields,
    /// which is the thing a record is refused for.
    fn enumeration(&mut self) -> Option<Enum> {
        let at = self.checkpoint();
        let start = self.peek_span();
        let pub_span = self.at(&TokenKind::Pub).then(|| {
            let s = self.peek_span();
            self.advance();
            s
        });
        self.expect(&TokenKind::Enum, "to start an enum")?;

        let name_span = self.peek_span();
        let name = match self.advance() {
            TokenKind::Ident(n) => {
                if self.reserved(&n, name_span) {
                    return None;
                }
                n
            }
            other => {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        name_span,
                        format!("expected an enum name, found {}", other.describe()),
                    )
                    .help("an enum is declared `enum State { Ready, Failed(code: Int), }`"),
                );
                return None;
            }
        };
        let type_params = self.only_type_params()?;

        let list = self.checkpoint();
        self.expect(&TokenKind::LBrace, "to start the variants")?;
        let mut variants = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let variant = self.checkpoint();
            let start = self.pos;
            match self.variant_decl() {
                Some(v) => variants.push(v),
                None => {
                    if !self.recover_member(variant, start, &TokenKind::RBrace) {
                        return None;
                    }
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(&TokenKind::RBrace, "to close the variants")?;
        self.wrap(list, SyntaxKind::VariantList);
        self.wrap(at, SyntaxKind::EnumItem);
        self.drop_item = self.poisoned;
        Some(Enum {
            name,
            name_span,
            pub_span,
            type_params,
            variants,
            span: start.join(close),
        })
    }

    /// `[pub] trait Name { fn m(self: Self, …) -> R [! {e}]; … }` (N78).
    ///
    /// A method is a signature ended by `;`: a body here is the one thing a reader might
    /// write by habit, and it is refused with the expected `;` rather than read.
    fn trait_decl(&mut self) -> Option<Trait> {
        let at = self.checkpoint();
        let start = self.peek_span();
        let pub_span = self.at(&TokenKind::Pub).then(|| {
            let s = self.peek_span();
            self.advance();
            s
        });
        self.expect(&TokenKind::Trait, "to start a trait")?;
        let name_span = self.peek_span();
        let name = match self.advance() {
            TokenKind::Ident(n) => {
                if self.reserved(&n, name_span) {
                    return None;
                }
                n
            }
            other => {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        name_span,
                        format!("expected a trait name, found {}", other.describe()),
                    )
                    .help("a trait is declared `trait Show { fn show(self: Self) -> Str; }`"),
                );
                return None;
            }
        };
        self.expect(&TokenKind::LBrace, "to start the trait's methods")?;
        let mut methods = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            methods.push(self.trait_method()?);
        }
        let close = self.expect(&TokenKind::RBrace, "to close the trait")?;
        self.wrap(at, SyntaxKind::TraitItem);
        Some(Trait {
            name,
            name_span,
            pub_span,
            methods,
            span: start.join(close),
        })
    }

    /// One method of a trait: `fn m(params) -> R [! {e}];`.
    fn trait_method(&mut self) -> Option<TraitMethod> {
        let at = self.checkpoint();
        let start = self.expect(&TokenKind::Fn, "to start a trait method")?;
        let name_span = self.peek_span();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.report(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    name_span,
                    format!("expected a method name, found {}", other.describe()),
                ));
                return None;
            }
        };
        let params = self.param_list("after the method name")?;
        let ret = self.checkpoint();
        self.expect(&TokenKind::Arrow, "before the return type")?;
        let return_type = self.type_name()?;
        self.wrap(ret, SyntaxKind::RetType);
        let effects = self.effect_set()?;
        let end = self.peek_span();
        if !self.at(&TokenKind::Semi) {
            self.report(
                Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    end,
                    format!(
                        "expected `;` after a trait method's signature, found {}",
                        self.peek().describe()
                    ),
                )
                .help(
                    "a trait method has no body: each `impl` of the trait writes one. \
                     There are no default methods",
                ),
            );
            return None;
        }
        self.advance();
        self.wrap(at, SyntaxKind::TraitMethod);
        Some(TraitMethod {
            name,
            name_span,
            params,
            return_type,
            effects,
            span: start.join(end),
        })
    }

    /// `impl Trait for Type { fn … }` (N78): the header, and each method as a function
    /// marked as this impl's.
    fn impl_decl(&mut self) -> Option<(ImplDecl, Vec<Function>)> {
        let at = self.checkpoint();
        let impl_span = self.expect(&TokenKind::Impl, "to start an impl")?;
        let trait_name = self.type_name()?;
        // `for` is a word here and nowhere else: a reserved name everywhere a name may be
        // declared, so no program can mean anything else by it.
        if matches!(self.peek(), TokenKind::Ident(w) if w == "for") {
            self.advance();
        } else {
            let found = self.peek().describe();
            self.report(
                Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    self.peek_span(),
                    format!("expected `for` after the trait's name, found {found}"),
                )
                .help("an impl is written `impl Show for Point { … }`"),
            );
            return None;
        }
        let self_ty = self.type_name()?;
        self.expect(&TokenKind::LBrace, "to start the impl's methods")?;
        let mut methods = Vec::new();
        let mut recovered = false;
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            if !self.at_item_fn() {
                let found = self.peek().describe();
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        self.peek_span(),
                        format!("expected a method, `fn NAME(…)`, found {found}"),
                    )
                    .help("an impl holds the trait's methods and nothing else"),
                );
                return None;
            }
            // Each method is its own function: what one body needed recovery for is not
            // the next one's header.
            self.poisoned = false;
            self.drop_item = false;
            let mut f = self.function()?;
            recovered |= self.drop_item;
            f.method_of = Some(Box::new(MethodOf {
                trait_name: trait_name.clone(),
                self_ty: self_ty.clone(),
                impl_span,
            }));
            methods.push(f);
        }
        let close = self.expect(&TokenKind::RBrace, "to close the impl")?;
        self.wrap(at, SyntaxKind::ImplItem);
        self.drop_item = recovered;
        Some((
            ImplDecl {
                trait_name,
                self_ty,
                impl_span,
                span: impl_span.join(close),
            },
            methods,
        ))
    }

    /// One variant of an enum declaration: `Name` or `Name(field: Type, …)`.
    fn variant_decl(&mut self) -> Option<Variant> {
        let variant = self.checkpoint();
        let variant_span = self.peek_span();
        let variant_name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        variant_span,
                        format!("expected a variant name, found {}", other.describe()),
                    )
                    .help(
                        "a variant is written `Name,` or `Name(field: Type,),` — the \
                         parentheses are the payload, so a variant without one has none",
                    ),
                );
                return None;
            }
        };
        let mut fields = Vec::new();
        let mut last = variant_span;
        if self.at(&TokenKind::LParen) {
            let payload = self.checkpoint();
            self.bump();
            // `Eof()` is refused rather than read as a variant with no payload. One
            // concept gets one spelling, and the spelling for *no payload* is no
            // brackets — `docs/spec.md` says so, and a record with no fields is
            // refused for the same reason.
            if self.at(&TokenKind::RParen) {
                let close = self.peek_span();
                self.advance();
                self.report(
                    Diagnostic::error(
                        codes::UNSUPPORTED_CONSTRUCT,
                        variant_span.join(close),
                        format!("`{variant_name}` has an empty payload, which is written by writing nothing"),
                    )
                    .help(
                        "a variant with no payload is declared `Name,` — the brackets \
                         are the payload. Construction and patterns write them anyway, \
                         because there they are what separates a variant from a field \
                         read",
                    ),
                );
                return None;
            }
            while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
                let field = self.checkpoint();
                let start = self.pos;
                match self.field_decl(
                    "each payload field is written `name: Type,`",
                    "after a payload field name",
                ) {
                    Some(f) => fields.push(f),
                    None => {
                        if !self.recover_member(field, start, &TokenKind::RParen) {
                            return None;
                        }
                    }
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            last = self.expect(&TokenKind::RParen, "to close the payload")?;
            self.wrap(payload, SyntaxKind::FieldList);
        }
        self.wrap(variant, SyntaxKind::Variant);
        Some(Variant {
            name: variant_name,
            name_span: variant_span,
            fields,
            span: variant_span.join(last),
        })
    }

    /// One field of a record or a payload: `name: Type`. `help` and `after` name which.
    fn field_decl(&mut self, help: &str, after: &str) -> Option<Field> {
        let field = self.checkpoint();
        let field_span = self.peek_span();
        let field_name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        field_span,
                        format!("expected a field name, found {}", other.describe()),
                    )
                    .help(help.to_owned()),
                );
                return None;
            }
        };
        self.expect(&TokenKind::Colon, after)?;
        let ty = self.type_name()?;
        self.wrap(field, SyntaxKind::Field);
        Some(Field {
            name: field_name,
            name_span: field_span,
            span: field_span.join(ty.span),
            ty,
        })
    }

    /// A member of a delimited list did not parse (N76). Mark the item recovered, skip what
    /// is left of the member — to its `,` or the list's `close`, at the nesting it began at
    /// — and keep its bytes in an error node. Returns `false` when the skip met something
    /// no list can hold — the next item, a statement boundary, a `}` that is not the list's,
    /// the end of the file — so the list has no closer here and the caller gives it up, as
    /// it did before recovery reached inside lists.
    ///
    /// The first recovery of an item decides, as a statement's does, whether its later
    /// diagnostics are cascades: they are when its delimiters do not balance.
    fn recover_member(&mut self, at: Checkpoint, start: usize, close: &TokenKind) -> bool {
        if !self.poisoned && !self.delimiters_balanced() {
            self.muted = true;
        }
        self.poisoned = true;
        let resumable = self.skip_member(start, close);
        if self.pos > start {
            self.wrap(at, SyntaxKind::Error);
        }
        resumable
    }

    /// Skip to the end of a member that began at token `start`: see
    /// [`Parser::recover_member`]. Every delimiter opened since `start` must close first.
    fn skip_member(&mut self, start: usize, close: &TokenKind) -> bool {
        let mut open = self
            .tokens
            .range(start, self.pos)
            .iter()
            .fold(0usize, |n, t| match t.kind {
                TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => n + 1,
                TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket => n.saturating_sub(1),
                _ => n,
            });
        loop {
            let here = self.peek();
            let closes = std::mem::discriminant(here) == std::mem::discriminant(close);
            match here {
                TokenKind::Fn if !self.at_item_fn() => self.bump(),
                TokenKind::Eof
                | TokenKind::Fn
                | TokenKind::Pub
                | TokenKind::Struct
                | TokenKind::Enum
                | TokenKind::Use => return false,
                TokenKind::Comma if open == 0 => return true,
                _ if closes && open == 0 => return true,
                TokenKind::RBrace
                | TokenKind::Semi
                | TokenKind::Let
                | TokenKind::While
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Scope
                | TokenKind::Spawn
                    if open == 0 =>
                {
                    return false;
                }
                TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => {
                    open += 1;
                    self.bump();
                }
                TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket => {
                    open = open.saturating_sub(1);
                    self.bump();
                }
                _ => self.bump(),
            }
        }
    }

    fn function(&mut self) -> Option<Function> {
        let at = self.checkpoint();
        let first = self.pos;
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

        // `extern "C"` (N42). `extern` is contextual: an identifier everywhere else, and a
        // foreign declaration's first word only here, at the start of an item and before a
        // string — where no other item can begin — so no program naming something `extern`
        // changes meaning.
        let mut foreign_abi: Option<(String, nazm_span::Span)> = None;
        if matches!(self.peek(), TokenKind::Ident(w) if w == "extern")
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Str(_))
            )
        {
            self.advance();
            let abi_span = self.peek_span();
            if let TokenKind::Str(abi) = self.advance() {
                foreign_abi = Some((abi, abi_span));
            }
        }

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
                self.report(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    name_span,
                    format!("expected a function name, found {}", other.describe()),
                ));
                return None;
            }
        };

        let (effect_params, type_params): (Vec<TypeParam>, Vec<TypeParam>) =
            self.type_params()?.into_iter().partition(|p| p.effect);
        let params = self.param_list("after the function name")?;

        // Required, not optional. A missing return type would have to mean either "infer
        // it" or "returns nothing", and v0.1 has neither.
        let ret = self.checkpoint();
        self.expect(&TokenKind::Arrow, "before the return type")?;
        let return_type = self.type_name()?;
        self.wrap(ret, SyntaxKind::RetType);
        let effects = self.effect_set()?;

        // A foreign declaration ends where a body would begin: `= "symbol";`. The symbol is
        // written, never derived from the name. A body after the symbol makes it an export
        // instead (N55): a Nazm function C may call, parsed as any other function from here.
        let mut export: Option<ForeignDecl> = None;
        if let Some((abi, abi_span)) = foreign_abi {
            self.expect(&TokenKind::Eq, "before a foreign function's symbol")?;
            let symbol_span = self.peek_span();
            let symbol = match self.advance() {
                TokenKind::Str(sym) => sym,
                other => {
                    self.report(Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        symbol_span,
                        format!(
                            "expected the foreign function's symbol as a string, found {}",
                            other.describe()
                        ),
                    ));
                    return None;
                }
            };
            let decl = ForeignDecl {
                abi,
                abi_span,
                symbol,
                symbol_span,
            };
            if self.at(&TokenKind::LBrace) {
                export = Some(decl);
            } else {
                let end = self.peek_span();
                self.expect(&TokenKind::Semi, "after a foreign function's symbol")?;
                self.wrap(at, SyntaxKind::FnItem);
                self.drop_item = self.poisoned;
                return Some(Function {
                    name,
                    name_span,
                    pub_span,
                    type_params,
                    effect_params,
                    params,
                    return_type,
                    effects,
                    body: None,
                    foreign: Some(decl),
                    export: None,
                    method_of: None,
                    span: start.join(end),
                });
            }
        }

        // A header that needed recovery (a parameter, N76) leaves the function out of the
        // abstract tree; its body is still parsed, for its own mistakes and its tree.
        let header_recovered = self.poisoned;

        // Deliberately not `?`. A parse error inside the body would otherwise drop the
        // whole declaration, and every call to it would then report that the function
        // does not exist — several errors for one mistake, which is the failure mode this
        // language exists to avoid. Keep the signature; skip to the next item.
        let body_at = self.checkpoint();
        let body_start = self.pos;
        let body = match self.block() {
            // A body that recovered from a statement is complete in the concrete tree and
            // absent from the abstract one: the checker sees no body rather than a body
            // with a statement missing from the middle of it.
            Some(_) if self.poisoned => None,
            Some(b) => Some(b),
            None => {
                self.recover_to_item(first);
                if self.pos > body_start {
                    self.wrap(body_at, SyntaxKind::Error);
                }
                None
            }
        };
        let span = body.as_ref().map_or(return_type.span, |b| b.span);
        self.wrap(at, SyntaxKind::FnItem);
        self.drop_item = header_recovered;
        Some(Function {
            name,
            name_span,
            pub_span,
            type_params,
            effect_params,
            params,
            return_type,
            effects,
            body,
            foreign: None,
            export,
            method_of: None,
            span: start.join(span),
        })
    }

    /// `( name: Type, … )` — a declaration's or a closure's parameters (N50: one parser).
    fn param_list(&mut self, after: &str) -> Option<Vec<Param>> {
        let list = self.checkpoint();
        self.expect(&TokenKind::LParen, after)?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
            let param = self.checkpoint();
            let start = self.pos;
            match self.param() {
                Some(p) => params.push(p),
                None => {
                    if !self.recover_member(param, start, &TokenKind::RParen) {
                        return None;
                    }
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "to close the parameter list")?;
        self.wrap(list, SyntaxKind::ParamList);
        Some(params)
    }

    /// One parameter: `name: Type`.
    fn param(&mut self) -> Option<Param> {
        let param = self.checkpoint();
        let p_span = self.peek_span();
        // `fn f(mut x: Int)` is a real thing to try, and "expected a parameter name,
        // found `mut`" describes the symptom rather than the rule.
        if self.at(&TokenKind::Mut) {
            self.advance();
            self.report(
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
                self.report(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    p_span,
                    format!("expected a parameter name, found {}", other.describe()),
                ));
                return None;
            }
        };
        self.expect(&TokenKind::Colon, "after a parameter name")?;
        let ty = self.type_name()?;
        self.wrap(param, SyntaxKind::Param);
        Some(Param {
            name: p_name,
            name_span: p_span,
            span: p_span.join(ty.span),
            ty,
        })
    }

    /// `! { name, … }` after a return type, or nothing (N36).
    ///
    /// Braces, not brackets or a bare list, so that the empty set — the purity
    /// assertion — has a spelling of its own: `! {}`. Names are separated by commas with
    /// no trailing one, like a type parameter list. Whether a name is an effect, and
    /// whether it is written twice, are the checker's questions: this is punctuation.
    /// Flat, so it adds no recursion.
    fn effect_set(&mut self) -> Option<Option<EffectDecl>> {
        if !self.at(&TokenKind::Bang) {
            return Some(None);
        }
        let at = self.checkpoint();
        let start = self.peek_span();
        self.bump();
        self.expect(&TokenKind::LBrace, "after `!`, to open the effect set")?;
        let mut names = Vec::new();
        if !self.at(&TokenKind::RBrace) {
            loop {
                let span = self.peek_span();
                match self.advance() {
                    TokenKind::Ident(n) => names.push((n, span)),
                    // The effect of starting a task is named by the word that starts one.
                    TokenKind::Spawn => names.push(("spawn".to_owned(), span)),
                    other => {
                        self.report(Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            span,
                            format!("expected an effect name, found {}", other.describe()),
                        ));
                        return None;
                    }
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        let end = self.peek_span();
        self.expect(&TokenKind::RBrace, "to close the effect set")?;
        self.wrap(at, SyntaxKind::EffectSet);
        Some(Some(EffectDecl {
            span: start.join(end),
            names,
        }))
    }

    /// `[A, B]` after a declared name, or nothing.
    ///
    /// At least one name and no trailing comma: an empty list declares nothing, and a
    /// parameter list is written on one line. Uniqueness and what a name may be are the
    /// checker's — they are questions about a namespace, not about punctuation.
    /// A record's or an enum's type parameters: an effect parameter is a function's alone
    /// (N51), since only a function performs anything.
    fn only_type_params(&mut self) -> Option<Vec<TypeParam>> {
        let list = self.type_params()?;
        if let Some(e) = list.iter().find(|p| p.effect) {
            self.report(
                Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    e.span,
                    format!(
                        "`{}` is an effect parameter, and only a function has one",
                        e.name
                    ),
                )
                .help(
                    "a record or an enum performs nothing; a function value it holds carries \
                     its effects in its own type",
                ),
            );
            return None;
        }
        Some(list)
    }

    fn type_params(&mut self) -> Option<Vec<TypeParam>> {
        let mut out = Vec::new();
        if !self.at(&TokenKind::LBracket) {
            return Some(out);
        }
        let list = self.checkpoint();
        self.bump();
        loop {
            let param = self.checkpoint();
            let span = self.peek_span();
            match self.advance() {
                // `effects E` (N51): `effects` is a word only here, and only before a name.
                TokenKind::Ident(w)
                    if w == "effects" && matches!(self.peek(), TokenKind::Ident(_)) =>
                {
                    let at = self.peek_span();
                    let TokenKind::Ident(n) = self.advance() else {
                        return None;
                    };
                    self.wrap(param, SyntaxKind::TypeParam);
                    out.push(TypeParam {
                        name: n,
                        span: at,
                        effect: true,
                        requires: None,
                    });
                }
                TokenKind::Ident(n) => {
                    // `T: Equality` — at most one requirement, one name. There is no `+`:
                    // one capability exists, and a list of one is not a list.
                    let requires = if self.eat(&TokenKind::Colon) {
                        let at = self.peek_span();
                        match self.advance() {
                            TokenKind::Ident(r) => Some(Requirement { name: r, span: at }),
                            other => {
                                self.report(
                                    Diagnostic::error(
                                        codes::UNEXPECTED_TOKEN,
                                        at,
                                        format!(
                                            "expected a capability after `:`, found {}",
                                            other.describe()
                                        ),
                                    )
                                    .help(
                                        "a type parameter may require equality: \
                                         `fn same[T: Equality](a: T, b: T) -> Bool`",
                                    ),
                                );
                                return None;
                            }
                        }
                    } else {
                        None
                    };
                    self.wrap(param, SyntaxKind::TypeParam);
                    out.push(TypeParam {
                        name: n,
                        span,
                        effect: false,
                        requires,
                    });
                }
                other => {
                    self.report(
                        Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            span,
                            format!("expected a type parameter, found {}", other.describe()),
                        )
                        .help("type parameters are names: `fn identity[T](value: T) -> T`"),
                    );
                    return None;
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "to close the type parameters")?;
        self.wrap(list, SyntaxKind::TypeParamList);
        Some(out)
    }

    /// A type: a name, optionally applied to type arguments.
    ///
    /// Recursion here is counted against the same budget as every other: `Vec[Vec[…]]` is a
    /// tree, and each level is one more frame in every pass that walks it.
    /// `m::` before a name in a type or a pattern (N49), consumed; `None` and nothing consumed
    /// otherwise. Two tokens decide it: a name, then `::`.
    fn qualifier_before_name(&mut self) -> Option<Qualifier> {
        let TokenKind::Ident(name) = self.peek().clone() else {
            return None;
        };
        if !matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.kind),
            Some(TokenKind::ColonColon)
        ) {
            return None;
        }
        let span = self.peek_span();
        self.advance();
        self.advance();
        Some(Qualifier { name, span })
    }

    /// `fn(T, …) -> R ! {e}` in a type position (N50). The parameter types and the result
    /// are type names, so a function type nests through [`Self::type_name`], which counts.
    fn fn_type(&mut self, at: Checkpoint) -> Option<TypeName> {
        let fn_span = self.peek_span();
        self.bump();
        self.expect(&TokenKind::LParen, "after `fn` in a function type")?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
            params.push(self.type_name()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "to close a function type's parameters")?;
        self.expect(&TokenKind::Arrow, "before a function type's result")?;
        let ret = self.type_name()?;
        let effects = self.effect_set()?;
        let end = effects.as_ref().map_or(ret.span, |e| e.span);
        self.wrap(at, SyntaxKind::FnType);
        Some(TypeName {
            qualifier: None,
            function: Some(Box::new(FnTypeName {
                params,
                ret,
                effects,
            })),
            name: "fn".to_owned(),
            name_span: fn_span,
            span: fn_span.join(end),
            args: None,
        })
    }

    /// `fn(x: T, …) -> R ! {e} { body }` in an expression (N50). Its own frame, never
    /// inlined, for the reason [`Self::ident_primary`] has one.
    #[inline(never)]
    fn closure(&mut self, at: Checkpoint) -> Option<Expr> {
        let fn_span = self.peek_span();
        self.bump();
        let params = self.param_list("after `fn` in a closure")?;
        self.expect(&TokenKind::Arrow, "before a closure's result type")?;
        let return_type = self.type_name()?;
        let effects = self.effect_set()?;
        let body = self.block()?;
        self.built(self.child() + 1)?;
        self.wrap(at, SyntaxKind::ClosureExpr);
        let span = fn_span.join(body.span);
        Some(Expr::Closure {
            params,
            return_type,
            effects,
            body: Box::new(body),
            fn_span,
            span,
        })
    }

    fn type_name(&mut self) -> Option<TypeName> {
        if !self.enter() {
            return None;
        }
        let out = self.type_name_inner();
        self.leave();
        out
    }

    fn type_name_inner(&mut self) -> Option<TypeName> {
        let at = self.checkpoint();
        if self.at(&TokenKind::Fn) {
            return self.fn_type(at);
        }
        let name_span = self.peek_span();
        let qualifier = self.qualifier_before_name();
        let name_span = if qualifier.is_some() {
            self.peek_span()
        } else {
            name_span
        };
        match self.advance() {
            TokenKind::Ident(n) => {
                let (args, end) = if self.at(&TokenKind::LBracket) {
                    let (args, close) = self.type_args()?;
                    (Some(args), close)
                } else {
                    (None, name_span)
                };
                self.wrap(at, SyntaxKind::Type);
                Some(TypeName {
                    qualifier,
                    function: None,
                    name: n,
                    name_span,
                    span: name_span.join(end),
                    args,
                })
            }
            other => {
                self.report(Diagnostic::error(
                    codes::UNEXPECTED_TOKEN,
                    name_span,
                    format!("expected a type, found {}", other.describe()),
                ));
                None
            }
        }
    }

    /// `[T, U]` — type arguments, possibly none.
    ///
    /// `[]` parses: it is a type written with the wrong number of arguments, which the
    /// checker reports with the arity it wanted (`N0353`), rather than a syntax error that
    /// could only say a type was expected.
    fn type_args(&mut self) -> Option<(Vec<TypeName>, Span)> {
        let at = self.checkpoint();
        self.expect(&TokenKind::LBracket, "to start the type arguments")?;
        let mut args = Vec::new();
        if !self.at(&TokenKind::RBracket) {
            loop {
                args.push(self.type_name()?);
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        let close = self.expect(&TokenKind::RBracket, "to close the type arguments")?;
        self.wrap(at, SyntaxKind::TypeArgList);
        Some((args, close))
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
        let at = self.checkpoint();
        let open = self.expect(&TokenKind::LBrace, "to start a block")?;
        let mut statements = Vec::new();
        let mut tail = None;
        let mut deepest = 0;

        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let stmt = self.checkpoint();
            let start = self.pos;
            let parsed = self.statement(&mut deepest);
            // A probe's hole ends with the statement holding it (N26).
            self.hole_open = false;
            match parsed {
                Some(Parsed::Stmt(s)) => statements.push(s),
                Some(Parsed::Tail(e)) => {
                    tail = Some(Box::new(e));
                    break;
                }
                None => {
                    // The statement reported what was wrong with it. Keep its bytes in an
                    // error node, skip to where the next statement can start, and carry on:
                    // one mistake costs one statement, not the rest of the function.
                    if !self.poisoned && !self.delimiters_balanced() {
                        self.muted = true;
                    }
                    self.poisoned = true;
                    let resumable = self.skip_statement(start);
                    // Progress, whatever the skip decided: a statement that failed on its
                    // first token, with the skip stopped in front of that same token, would
                    // otherwise be tried again in place for ever. `}` is the one token it
                    // may stop in front of, because the loop then ends.
                    if resumable && self.pos == start && !self.at(&TokenKind::RBrace) {
                        self.bump();
                    }
                    if self.pos > start {
                        self.wrap(stmt, SyntaxKind::Error);
                    }
                    if !resumable {
                        // The skip ran into the next item or the end of the file, so this
                        // block's `}` is missing — which is the mistake already reported,
                        // seen from the other side. Abandon the block rather than report it
                        // twice; the function recovers to that item exactly as before.
                        return None;
                    }
                }
            }
        }

        let close = self.expect(&TokenKind::RBrace, "to close the block")?;
        self.built(deepest + 1)?;
        self.wrap(at, SyntaxKind::Block);
        Some(Block {
            statements,
            tail,
            span: open.join(close),
        })
    }

    /// One statement of a block, or the block's tail expression.
    ///
    /// `deepest` is the block's running depth: a statement is not itself a node, so its
    /// depth is its deepest sub-expression.
    fn statement(&mut self, deepest: &mut usize) -> Option<Parsed> {
        if self.at(&TokenKind::Let) {
            let s = Stmt::Let(self.let_stmt()?);
            *deepest = (*deepest).max(self.child());
            return Some(Parsed::Stmt(s));
        }
        if self.at(&TokenKind::While) {
            let s = Stmt::While(self.while_stmt()?);
            *deepest = (*deepest).max(self.child());
            return Some(Parsed::Stmt(s));
        }
        if self.at(&TokenKind::Return) {
            let s = Stmt::Return(self.return_stmt()?);
            *deepest = (*deepest).max(self.child());
            return Some(Parsed::Stmt(s));
        }
        if self.at(&TokenKind::Break) || self.at(&TokenKind::Continue) {
            return self.loop_control().map(Parsed::Stmt);
        }
        if self.at(&TokenKind::Scope) {
            let s = Stmt::Scope(self.scope()?);
            *deepest = (*deepest).max(self.child());
            return Some(Parsed::Stmt(s));
        }
        if self.at(&TokenKind::Spawn) {
            let s = Stmt::Spawn(self.spawn()?);
            *deepest = (*deepest).max(self.child());
            return Some(Parsed::Stmt(s));
        }
        if let Some(a) = self.try_assign()? {
            *deepest = (*deepest).max(self.child());
            return Some(Parsed::Stmt(Stmt::Assign(a)));
        }

        let at = self.checkpoint();
        let block_like = self.at(&TokenKind::If) || self.at(&TokenKind::LBrace);
        let e = self.expr()?;
        *deepest = (*deepest).max(self.child());

        if self.eat(&TokenKind::Semi) {
            // Explicitly discarded.
            self.wrap(at, SyntaxKind::ExprStmt);
            Some(Parsed::Stmt(Stmt::Expr(e)))
        } else if self.at(&TokenKind::RBrace) {
            Some(Parsed::Tail(e))
        } else if block_like || self.hole_open {
            // `if c { … }` followed by more statements needs no `;`, the same rule
            // every block-like expression follows — and neither does a probe's unfinished
            // statement (N26).
            self.wrap(at, SyntaxKind::ExprStmt);
            Some(Parsed::Stmt(Stmt::Expr(e)))
        } else {
            let span = self.peek_span();
            let found = self.peek().describe();
            self.report(
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
            None
        }
    }

    /// Skip what is left of a statement that did not parse, to the nearest place the
    /// block can resume: just past a `;`, just before the block's own `}`, or just before
    /// a keyword that can only start a statement — `let`, `while`, `return`, `break`,
    /// `continue`, `scope`, `spawn`. Not `if` or `match`: they begin expressions too, and
    /// the tail of a malformed statement is full of them (`=> if c { … }`), so stopping
    /// there re-parses a fragment of the statement that failed as if it were the next one.
    ///
    /// # Where "the block's own `}`" is
    ///
    /// The skip starts from the braces the failed statement **left open**. A statement
    /// that failed inside `f(match e { A.B() => …` has consumed a `{` that no token it will
    /// skip opened; counting from zero would take the `match`'s `}` for the block's and end
    /// the block early, and every statement after it would become a cascade of errors
    /// about the wrong thing.
    ///
    /// Braces bound the skip: nothing stops inside an unclosed `{`. Parentheses do not: a
    /// statement keyword stops the skip at brace level whatever parentheses are open,
    /// because an unclosed `(` is exactly what a malformed call leaves behind and waiting
    /// for its `)` would swallow the statement after it.
    ///
    /// Returns `false` when the skip reached the next item (`fn`, `pub`) or the end of the
    /// file instead: neither can occur inside a block, so the block has no `}` here.
    ///
    /// **It always makes progress or stops the block.** It stops without consuming only at
    /// `}` (the block's loop then ends), at `fn`, `pub` or the end of the file (the block is
    /// abandoned), or at a statement keyword — and only once at least one token of the failed
    /// statement has been consumed, so the same keyword cannot fail twice in place.
    fn skip_statement(&mut self, start: usize) -> bool {
        let mut braces = self.left_open(start);
        loop {
            match self.peek() {
                TokenKind::Fn if !self.at_item_fn() => self.bump(),
                TokenKind::Eof | TokenKind::Fn | TokenKind::Pub => return false,
                TokenKind::RBrace if braces == 0 => return true,
                TokenKind::RBrace => {
                    braces -= 1;
                    self.bump();
                }
                TokenKind::LBrace => {
                    braces += 1;
                    self.bump();
                }
                TokenKind::Semi => {
                    self.bump();
                    if braces == 0 {
                        return true;
                    }
                }
                TokenKind::Let
                | TokenKind::While
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Scope
                | TokenKind::Spawn
                    if braces == 0 && self.pos > start =>
                {
                    return true;
                }
                _ => self.bump(),
            }
        }
    }

    /// Braces opened and not closed by the tokens consumed since `start`.
    fn left_open(&self, start: usize) -> usize {
        let mut braces = 0usize;
        for t in self.tokens.range(start, self.pos) {
            match t.kind {
                TokenKind::LBrace => braces += 1,
                TokenKind::RBrace => braces = braces.saturating_sub(1),
                _ => {}
            }
        }
        braces
    }

    fn loop_control(&mut self) -> Option<Stmt> {
        let at = self.checkpoint();
        let is_break = self.at(&TokenKind::Break);
        let span = self.peek_span();
        self.advance();
        let word = if is_break { "break" } else { "continue" };
        let semi = self.expect(&TokenKind::Semi, &format!("after `{word}`"))?;
        let span = span.join(semi);
        self.wrap(
            at,
            if is_break {
                SyntaxKind::BreakStmt
            } else {
                SyntaxKind::ContinueStmt
            },
        );
        Some(if is_break {
            Stmt::Break { span }
        } else {
            Stmt::Continue { span }
        })
    }

    fn return_stmt(&mut self) -> Option<Return> {
        let at = self.checkpoint();
        let start = self.peek_span();
        self.advance();

        // `return;` would need a unit type to mean anything, and this language has not
        // made that decision. Refused by name rather than parsed into something else.
        if self.at(&TokenKind::Semi) {
            let span = self.peek_span();
            self.advance();
            self.report(
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
        self.wrap(at, SyntaxKind::ReturnStmt);
        Some(Return {
            value,
            span: start.join(semi),
        })
    }

    fn let_stmt(&mut self) -> Option<Let> {
        let at = self.checkpoint();
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
                self.report(Diagnostic::error(
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
        self.wrap(at, SyntaxKind::LetStmt);
        Some(Let {
            name,
            name_span,
            mutable,
            value,
            span: let_span.join(semi),
        })
    }

    fn while_stmt(&mut self) -> Option<While> {
        let at = self.checkpoint();
        let start = self.peek_span();
        self.advance();
        let cond = self.expr()?;
        let cond_depth = self.child();
        let body = self.block()?;
        // Both children count: a `while` whose condition is deeper than its body is
        // bounded by the condition.
        self.built = cond_depth.max(self.child());
        self.wrap(at, SyntaxKind::WhileStmt);
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

        let at = self.checkpoint();
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
        // The place is its own node: the name and its field path, without the `=`.
        // Wrapped after the fact, at the same checkpoint the statement will use.
        self.wrap(at, SyntaxKind::Place);
        self.advance(); // `=`
        let value = self.expr()?;
        let semi = self.expect(&TokenKind::Semi, "after an assignment")?;
        self.wrap(at, SyntaxKind::AssignStmt);
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

    /// `( field: value, … )` — the initialiser list a record construction and a variant
    /// construction share.
    ///
    /// Every field is named, there is no positional form, and a trailing comma is allowed.
    /// Duplicates, omissions and unknown names are the checker's to report: they are
    /// questions about a type, and the parser has not resolved one.
    ///
    /// Returns the fields, the closing parenthesis and the deepest subtree any initialiser
    /// built, because the caller is the node that has to account for it.
    fn field_inits(&mut self, variant: Option<Span>) -> Option<(Vec<FieldInit>, Span, usize)> {
        let list = self.checkpoint();
        let open = self.pos;
        self.expect(&TokenKind::LParen, "to start the fields")?;
        // `E.V(` with the cursor right after the `(` and no label: a probe's hole (N26).
        let mut empty = false;
        if let Some(variant) = variant {
            if self.ends_at_hole(open) && !self.label_at(self.pos) {
                self.take_hole(HoleSite::Payload {
                    variant,
                    pattern: false,
                });
                empty = true;
            }
        }
        let mut fields = Vec::new();
        let mut deepest = 0;
        while !empty && !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
            let field = self.checkpoint();
            let start = self.pos;
            match self.field_init() {
                Some(f) => {
                    fields.push(f);
                    deepest = deepest.max(self.child());
                }
                None => {
                    if !self.recover_member(field, start, &TokenKind::RParen) {
                        return None;
                    }
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(&TokenKind::RParen, "to close the fields")?;
        self.wrap(list, SyntaxKind::FieldInitList);
        Some((fields, close, deepest))
    }

    /// The argument list of a method call whose receiver and method name are read, and the
    /// call (N78). Arguments recover one by one, as a call's do.
    fn method_call(
        &mut self,
        at: Checkpoint,
        qualifier: Option<Qualifier>,
        receiver: Expr,
        method: String,
        method_span: Span,
    ) -> Option<Expr> {
        let mut deepest = self.child();
        let args_at = self.checkpoint();
        self.expect(&TokenKind::LParen, "to start the arguments")?;
        let mut args = Vec::new();
        while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
            let arg = self.checkpoint();
            let start = self.pos;
            match self.expr() {
                Some(e) => {
                    args.push(e);
                    deepest = deepest.max(self.child());
                }
                None => {
                    if !self.recover_member(arg, start, &TokenKind::RParen) {
                        return None;
                    }
                }
            }
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(&TokenKind::RParen, "to close the argument list")?;
        self.wrap(args_at, SyntaxKind::ArgList);
        self.built(deepest + 1)?;
        self.wrap(at, SyntaxKind::MethodCallExpr);
        Some(Expr::MethodCall {
            qualifier,
            span: receiver.span().join(close),
            receiver: Box::new(receiver),
            method,
            method_span,
            args,
        })
    }

    /// One field initialiser: `name: value`.
    fn field_init(&mut self) -> Option<FieldInit> {
        let field = self.checkpoint();
        let name_span = self.peek_span();
        let name = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        name_span,
                        format!("expected a field name, found {}", other.describe()),
                    )
                    .help(
                        "every field is named when a value is built: \
                         `Point(x: 1, y: 2)`, `State.Failed(code: 1)`",
                    ),
                );
                return None;
            }
        };
        self.expect(&TokenKind::Colon, "after a field name")?;
        let value = self.expr()?;
        self.wrap(field, SyntaxKind::FieldInit);
        Some(FieldInit {
            name,
            name_span,
            span: name_span.join(value.span()),
            value,
        })
    }

    /// `Name(field: value, …)`, the `(` already seen by the caller's lookahead.
    fn construction(
        &mut self,
        at: Checkpoint,
        ty: String,
        ty_span: Span,
        type_args: Option<Vec<TypeName>>,
    ) -> Option<Expr> {
        let (fields, close, deepest) = self.field_inits(None)?;
        self.built(deepest + 1)?;
        self.wrap(at, SyntaxKind::ConstructExpr);
        Some(Expr::Construct {
            qualifier: None,
            ty,
            ty_span,
            type_args,
            fields,
            span: ty_span.join(close),
        })
    }

    /// `Enum.Variant(field: value, …)`, the `.` already seen by the caller's lookahead.
    ///
    /// The parentheses are written even when the variant has no payload, because
    /// `Enum.Variant` alone is exactly the shape of a field projection. See `docs/spec.md`.
    fn variant_construction(
        &mut self,
        at: Checkpoint,
        ty: String,
        ty_span: Span,
        type_args: Option<Vec<TypeName>>,
    ) -> Option<Expr> {
        self.advance(); // `.`
        let variant_span = self.peek_span();
        let TokenKind::Ident(variant) = self.advance() else {
            // Unreachable: the caller looked at this token before calling.
            return None;
        };
        let (fields, close, deepest) = self.field_inits(Some(variant_span))?;
        self.built(deepest + 1)?;
        self.wrap(at, SyntaxKind::VariantExpr);
        Some(Expr::Variant {
            qualifier: None,
            ty,
            ty_span,
            type_args,
            variant,
            variant_span,
            fields,
            span: ty_span.join(close),
        })
    }

    /// One arm of a `match`: its pattern and its body, and whether it is a probe's
    /// unfinished arm (N26).
    fn match_arm(&mut self) -> Option<(MatchArm, bool)> {
        let arm = self.checkpoint();
        let qualifier = self.qualifier_before_name();
        let ty_span = self.peek_span();
        let ty = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        ty_span,
                        format!("expected an enum name, found {}", other.describe()),
                    )
                    .help(
                        "every arm names the variant it selects, qualified by its \
                         enum: `State.Ready() => 0,`",
                    ),
                );
                return None;
            }
        };
        self.expect(&TokenKind::Dot, "between an enum and one of its variants")?;
        let variant_span = self.peek_span();
        let variant = match self.advance() {
            TokenKind::Ident(n) => n,
            other => {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        variant_span,
                        format!("expected a variant name, found {}", other.describe()),
                    )
                    .help("an arm selects one variant: `State.Failed(code: c) => c,`"),
                );
                return None;
            }
        };
        let holed = self.hole.is_some();
        let fields = self.pattern_fields(variant_span)?;
        self.wrap(arm, SyntaxKind::Pattern);
        // A probe's hole in this pattern, with no `=>` written yet: the arm has no body
        // to read, and is given a zero-width `0` so the pattern can be checked (N26).
        let hole_arm = holed && self.hole.is_none() && !self.at(&TokenKind::FatArrow);
        let body = if hole_arm {
            Expr::Int {
                value: 0,
                span: Span::new(self.file, variant_span.end, variant_span.end),
            }
        } else {
            self.expect(&TokenKind::FatArrow, "between a pattern and its arm")?;
            self.expr()?
        };
        self.wrap(arm, SyntaxKind::MatchArm);
        Some((
            MatchArm {
                qualifier,
                ty,
                ty_span,
                variant,
                variant_span,
                fields,
                span: ty_span.join(body.span()),
                body,
            },
            hole_arm,
        ))
    }

    /// `match scrutinee { Enum.Variant(field: binding) => body, … }`
    ///
    /// The `match` keyword is already consumed. The scrutinee is an ordinary expression and
    /// the arms are a brace-delimited list, exactly as an `if`'s condition and block are —
    /// there is no second grammar here and no second set of completion rules.
    fn match_expr(&mut self, at: Checkpoint, start: Span) -> Option<Expr> {
        let scrutinee = self.expr()?;
        let mut deepest = self.child();
        let list = self.checkpoint();
        self.expect(&TokenKind::LBrace, "to start the arms")?;
        let mut arms = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let arm = self.checkpoint();
            let start = self.pos;
            let hole_arm = match self.match_arm() {
                Some((a, hole_arm)) => {
                    arms.push(a);
                    deepest = deepest.max(self.child());
                    hole_arm
                }
                None => {
                    if !self.recover_member(arm, start, &TokenKind::RBrace) {
                        return None;
                    }
                    false
                }
            };
            if !self.eat(&TokenKind::Comma) && !(hole_arm && !self.at(&TokenKind::RBrace)) {
                break;
            }
        }
        let close = self.expect(&TokenKind::RBrace, "to close the arms")?;
        self.wrap(list, SyntaxKind::MatchArmList);
        self.built(deepest + 1)?;
        self.wrap(at, SyntaxKind::MatchExpr);
        Some(Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
            span: start.join(close),
        })
    }

    /// `( field: binding, … )` — a pattern's payload list.
    ///
    /// Each field is named and bound to an identifier, or discarded with `_`. Whether every
    /// field of the variant is present, and whether two bindings share a name, are the
    /// checker's questions: both are about a variant the parser has not resolved.
    fn pattern_fields(&mut self, variant: Span) -> Option<Vec<PatternField>> {
        let list = self.checkpoint();
        let open = self.pos;
        self.expect(&TokenKind::LParen, "to start the payload")?;
        // `E.V(` in a pattern, with the cursor right after the `(` and no label (N26).
        let mut empty = false;
        if self.ends_at_hole(open) && !self.label_at(self.pos) {
            self.take_hole(HoleSite::Payload {
                variant,
                pattern: true,
            });
            empty = true;
        }
        let mut fields = Vec::new();
        while !empty && !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
            let field = self.checkpoint();
            let name_span = self.peek_span();
            let name = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    self.report(
                        Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            name_span,
                            format!("expected a payload field name, found {}", other.describe()),
                        )
                        .help("a pattern names every payload field: `V(left: a, right: _)`"),
                    );
                    return None;
                }
            };
            self.expect(&TokenKind::Colon, "after a payload field name")?;
            let binding_span = self.peek_span();
            let binding = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    self.report(
                        Diagnostic::error(
                            codes::UNEXPECTED_TOKEN,
                            binding_span,
                            format!("expected a binding or `_`, found {}", other.describe()),
                        )
                        .help(
                            "a payload field is bound to a name or discarded with `_`; \
                             `_` still leaves the value to the enum, it only declines to \
                             name it",
                        ),
                    );
                    return None;
                }
            };
            self.wrap(field, SyntaxKind::PatternField);
            fields.push(PatternField {
                name,
                name_span,
                // `_` is an ordinary identifier to the lexer and a discard here. One
                // spelling, decided by position: `docs/spec.md` gives `_` its meaning
                // inside a pattern and nowhere else.
                binding: (binding != "_").then_some(binding),
                binding_span,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "to close the payload")?;
        self.wrap(list, SyntaxKind::PatternFieldList);
        Some(fields)
    }

    fn expr(&mut self) -> Option<Expr> {
        self.binary(0)
    }

    /// Pratt loop. `||` binds loosest, then `&&` (N49), then comparison, then `+`/`-`, then
    /// `*`/`/`/`%`.
    ///
    /// The loop does not recurse, but the tree it builds does: `1 + 1 + 1` is
    /// `((1 + 1) + 1)`, one level of left spine per operator. So a chain costs nothing
    /// against the parser-recursion bound and everything against the tree bound, which
    /// is why the two are separate measurements and why `lhs_depth` carries the operand's
    /// own depth into the node built on top of it.
    fn binary(&mut self, min_bp: u8) -> Option<Expr> {
        let at = self.checkpoint();
        let mut lhs = self.primary()?;
        // The operand's own depth, which the node built on top of it adds to. Counting
        // only the operators would miss this entirely: `- - - 1 + 1 + 1` releases the
        // negations before the first `+` is seen, and the two compose into a tree as deep
        // as both together.
        let mut lhs_depth = self.child();

        loop {
            let (op, bp) = match self.peek() {
                TokenKind::PipePipe => (BinOp::Or, 1),
                TokenKind::AmpAmp => (BinOp::And, 2),
                TokenKind::EqEq => (BinOp::Eq, 3),
                TokenKind::NotEq => (BinOp::NotEq, 3),
                TokenKind::Lt => (BinOp::Lt, 3),
                TokenKind::LtEq => (BinOp::LtEq, 3),
                TokenKind::Gt => (BinOp::Gt, 3),
                TokenKind::GtEq => (BinOp::GtEq, 3),
                TokenKind::Plus => (BinOp::Add, 4),
                TokenKind::Minus => (BinOp::Sub, 4),
                TokenKind::Star => (BinOp::Mul, 5),
                TokenKind::Slash => (BinOp::Div, 5),
                TokenKind::Percent => (BinOp::Rem, 5),
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
            // This recursion needs no budget of its own: `bp` is at most 5, and a call at
            // min_bp 6 matches no operator and returns immediately, so the binary-to-
            // binary chain is at most three frames however long the expression. The
            // unbounded path into an operand goes through `primary`, which `enter`
            // counts.
            let rhs = self.binary(bp + 1)?;
            let rhs_depth = self.child();
            lhs_depth = lhs_depth.max(rhs_depth) + 1;
            self.built(lhs_depth)?;
            // Left-associative in the concrete tree too: each operator wraps everything
            // since the chain's first operand, so `a - b - c` nests as `(a - b) - c`.
            self.wrap(at, SyntaxKind::BinaryExpr);
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
        let at = self.checkpoint();
        let out = self.primary_inner(at).and_then(|e| self.projections(at, e));
        self.leave();
        out
    }

    /// `expr.name` and `expr?`, repeated in any order. Both bind tighter than any
    /// operator, including negation's operand, because they are part of the operand:
    /// `-r?` is `-(r?)`, and `make()?.field` projects out of what `?` produced.
    ///
    /// A loop rather than recursion: `a.b?.c` is a left spine, so it costs one tree level
    /// per postfix and no parser recursion at all.
    fn projections(&mut self, at: Checkpoint, base: Expr) -> Option<Expr> {
        let mut out = base;
        let mut depth = self.child();
        loop {
            if self.at(&TokenKind::Question) {
                let op_span = self.peek_span();
                self.advance();
                depth += 1;
                self.built(depth)?;
                self.wrap(at, SyntaxKind::PropagateExpr);
                out = Expr::Propagate {
                    span: out.span().join(op_span),
                    operand: Box::new(out),
                    op_span,
                };
                continue;
            }
            if !self.at(&TokenKind::Dot) {
                break;
            }
            // `receiver.` with the cursor right after the `.` and no name: a probe's hole,
            // an empty member name the checker looks for on the receiver's type (N26).
            if self.hole.is_some() && self.ends_at_hole(self.pos) && !self.ident_at(self.pos + 1) {
                depth += 1;
                out = self.member_hole(at, out, depth)?;
                continue;
            }
            self.advance();
            let name_span = self.peek_span();
            let name = match self.advance() {
                TokenKind::Ident(n) => n,
                other => {
                    self.report(
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
            if self.at(&TokenKind::LParen) {
                // `receiver.m(args)` (N78): a method call on any receiver.
                out = self.method_call(at, None, out, name, name_span)?;
                depth = self.built;
                continue;
            }
            depth += 1;
            self.built(depth)?;
            self.wrap(at, SyntaxKind::FieldExpr);
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

    /// A primary that begins with a name: a variable, a call, a construction, a variant, each
    /// possibly qualified (N49). Its own frame, never inlined: `primary_inner` recurses through
    /// every nesting level, and the parser's depth bound is measured against that frame, so
    /// what only a name needs is kept out of it.
    #[inline(never)]
    fn ident_primary(&mut self, at: Checkpoint, name: String, span: Span) -> Option<Expr> {
        if self.reserved(&name, span) {
            self.advance();
            return None;
        }
        self.advance();
        // `m::name` (N49): the alias is held here, in this frame, and attached to the
        // node the name turns out to begin once it is built — a name nested in its
        // arguments has a frame of its own. The item's name and span are what
        // everything below reads.
        let (qualifier, name, span) = if self.at(&TokenKind::ColonColon) {
            self.advance();
            let item_span = self.peek_span();
            match self.advance() {
                TokenKind::Ident(item) => (Some(Qualifier { name, span }), item, item_span),
                other => {
                    self.report(Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        item_span,
                        format!("expected a name after `::`, found {}", other.describe()),
                    ));
                    return None;
                }
            }
        } else {
            (None, name, span)
        };
        // `name[T, …]` — type arguments, before a call, a construction or a
        // variant. Nothing else can follow a name with `[`: there is no indexing.
        let type_args = if self.at(&TokenKind::LBracket) {
            let (args, _) = self.type_args()?;
            // `Maybe[Int].` with the cursor right after the `.`: a probe's hole,
            // the empty variant name of an applied enum's construction (N26).
            if self.hole.is_some()
                && self.at(&TokenKind::Dot)
                && self.ends_at_hole(self.pos)
                && !self.ident_at(self.pos + 1)
            {
                return self
                    .variant_hole(at, name, span, args)
                    .map(|e| qualified(e, qualifier));
            }
            let applies = self.at(&TokenKind::LParen)
                || (self.at(&TokenKind::Dot)
                    && matches!(
                        self.tokens.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokenKind::Ident(_))
                    )
                    && matches!(
                        self.tokens.get(self.pos + 2).map(|t| &t.kind),
                        Some(TokenKind::LParen)
                    ));
            if !applies {
                let at = self.peek_span();
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        at,
                        format!(
                            "expected `(` after the type arguments of `{name}`, found {}",
                            self.peek().describe()
                        ),
                    )
                    .help(
                        "type arguments are written where something is called or \
                         built: `identity[Int](7)`, `Box[Int](value: 7)`, \
                         `Maybe[Int].None()`",
                    ),
                );
                return None;
            }
            Some(args)
        } else {
            None
        };
        // `Enum.Variant(…)` is a variant. Three tokens decide it, and the
        // decision is total: the language has no methods and no function values,
        // so a `(` can never follow a field projection and nothing else has this
        // shape. `docs/spec.md` records why a variant is always parenthesised.
        let variant = self.at(&TokenKind::Dot)
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(TokenKind::Ident(_))
            )
            && matches!(
                self.tokens.get(self.pos + 2).map(|t| &t.kind),
                Some(TokenKind::LParen)
            );
        // `name.m(arg, …)` with an unlabelled argument is a method call (N78): a variant's
        // payload is always labelled, so the token after the `(` decides it. `name.m()` has
        // nothing to decide by and stays a variant; the checker reads it as a call when
        // `name` is a value rather than an enum.
        let unlabelled = variant
            && !matches!(
                self.tokens.get(self.pos + 3).map(|t| &t.kind),
                Some(TokenKind::RParen)
            )
            && !self.label_at(self.pos + 3)
            && !(self.hole.is_some() && self.ends_at_hole(self.pos + 2));
        if unlabelled {
            if type_args.is_some() {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        span,
                        format!(
                            "type arguments cannot be written on `{name}` before a method call"
                        ),
                    )
                    .help("a method's types come from its receiver: `x.show()`"),
                );
                return None;
            }
            let receiver = Expr::Var { name, span };
            self.built(1)?;
            self.wrap(at, SyntaxKind::NameRef);
            self.advance();
            let method_span = self.peek_span();
            let method = match self.advance() {
                TokenKind::Ident(m) => m,
                _ => return None,
            };
            return self.method_call(at, qualifier, receiver, method, method_span);
        }
        if variant {
            return self
                .variant_construction(at, name, span, type_args)
                .map(|e| qualified(e, qualifier));
        }
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
            return self
                .construction(at, name, span, type_args)
                .map(|e| qualified(e, qualifier));
        }
        // `Name(` with the cursor right after the `(` and no label: a probe's hole.
        // The grammar reads a `(` without labels as a call, and so does this; what
        // `Name` is, the checker decides (N26).
        if self.hole.is_some() && self.at(&TokenKind::LParen) && self.ends_at_hole(self.pos) {
            return self
                .call_hole(at, name, span, type_args)
                .map(|e| qualified(e, qualifier));
        }
        if self.at(&TokenKind::LParen) {
            // Taken only once the `(` is there: a checkpoint emits the trivia before
            // the next token, and for a plain name that trivia is not the name's.
            let args_at = self.checkpoint();
            self.bump();
            let mut args = Vec::new();
            let mut deepest = 0;
            while !self.at(&TokenKind::RParen) && !self.at(&TokenKind::Eof) {
                let arg = self.checkpoint();
                let start = self.pos;
                match self.expr() {
                    Some(e) => {
                        args.push(e);
                        deepest = deepest.max(self.child());
                    }
                    None => {
                        if !self.recover_member(arg, start, &TokenKind::RParen) {
                            return None;
                        }
                    }
                }
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            let close = self.expect(&TokenKind::RParen, "to close the argument list")?;
            self.wrap(args_at, SyntaxKind::ArgList);
            self.built(deepest + 1)?;
            self.wrap(at, SyntaxKind::CallExpr);
            Some(Expr::Call {
                qualifier,
                callee: name,
                callee_span: span,
                type_args,
                args,
                span: span.join(close),
            })
        } else {
            // A qualified name names a definition, and Nazm has no module-level
            // values: `m::x` alone is not an expression (N49).
            if let Some(q) = qualifier {
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        q.span.join(span),
                        format!("`{}::{name}` names a definition, not a value", q.name),
                    )
                    .help(
                        "a qualified name is called, constructed or names a variant: \
                         `m::f(…)`, `m::R(…)`, `m::E.V(…)`",
                    ),
                );
                return None;
            }
            self.built(1)?;
            self.wrap(at, SyntaxKind::NameRef);
            Some(Expr::Var { name, span })
        }
    }

    /// `at` is the checkpoint taken before the operand's first token, which every form
    /// below wraps its node at.
    fn primary_inner(&mut self, at: Checkpoint) -> Option<Expr> {
        let span = self.peek_span();

        // Prefix logical negation (N49). Binds as tightly as `-`, so `!a && b` is
        // `(!a) && b`; its operand is a primary, which `enter` counts.
        if self.at(&TokenKind::Bang) {
            let op_span = self.peek_span();
            self.advance();
            let operand = self.primary()?;
            self.built(self.child() + 1)?;
            self.wrap(at, SyntaxKind::NotExpr);
            let full = op_span.join(operand.span());
            return Some(Expr::Not {
                operand: Box::new(operand),
                op_span,
                span: full,
            });
        }

        // Prefix negation, before anything else can claim the `-`.
        if self.at(&TokenKind::Minus) {
            let op_span = self.peek_span();
            self.advance();

            // Fold the sign into the literal. `i64::MIN` has a magnitude one greater than
            // `i64::MAX`, so `-9223372036854775808` is only representable as a whole.
            if let TokenKind::IntTooLarge(digits) = self.peek().clone() {
                let lit = self.checkpoint();
                let lit_span = self.peek_span();
                self.advance();
                let full = op_span.join(lit_span);
                return match digits.parse::<u64>() {
                    Ok(v) if v == 1u64 << 63 => {
                        self.built(1)?;
                        // Concretely a `-` and a literal; abstractly one `Int`.
                        self.wrap(lit, SyntaxKind::Literal);
                        self.wrap(at, SyntaxKind::NegExpr);
                        Some(Expr::Int {
                            value: i64::MIN,
                            span: full,
                        })
                    }
                    _ => {
                        self.report(
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
            self.wrap(at, SyntaxKind::NegExpr);
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
                self.wrap(at, SyntaxKind::Literal);
                Some(Expr::Int { value, span })
            }
            TokenKind::IntTooLarge(digits) => {
                self.advance();
                self.report(
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
                self.wrap(at, SyntaxKind::Literal);
                Some(Expr::Bool { value: true, span })
            }
            TokenKind::False => {
                self.advance();
                self.built(1)?;
                self.wrap(at, SyntaxKind::Literal);
                Some(Expr::Bool { value: false, span })
            }
            TokenKind::Str(value) => {
                self.advance();
                self.built(1)?;
                self.wrap(at, SyntaxKind::Literal);
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
                self.wrap(at, SyntaxKind::IfExpr);
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
                // No abstract node: `(x)` *is* `x`. `self.built` already holds the inner
                // depth and must not be incremented, or the limit would count punctuation.
                // The concrete tree has one, because the parentheses are in the file.
                self.wrap(at, SyntaxKind::ParenExpr);
                Some(inner)
            }
            TokenKind::LBrace => {
                // The block is its own concrete node; a block expression adds no other.
                let b = self.block()?;
                self.built(self.child() + 1)?;
                Some(Expr::Block(Box::new(b)))
            }
            TokenKind::Ident(name) => self.ident_primary(at, name, span),
            TokenKind::Fn => self.closure(at),
            TokenKind::Match => {
                self.advance();
                self.match_expr(at, span)
            }
            kw @ (TokenKind::Struct | TokenKind::Enum) => {
                // A declaration inside a body is in the wrong place, not an expression.
                // Saying which is the difference between one useful error and a cascade of
                // "expected an expression".
                let (word, what) = if matches!(kw, TokenKind::Struct) {
                    ("struct", "A record")
                } else {
                    ("enum", "An enum")
                };
                self.advance();
                self.report(
                    Diagnostic::error(
                        codes::UNEXPECTED_TOKEN,
                        span,
                        format!(
                            "{} can only be declared at the top level",
                            what.to_lowercase()
                        ),
                    )
                    .help(format!(
                        "move the `{word}` out of the function. {what} is a module-level \
                         declaration, like a function",
                    )),
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
                self.report(
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
                self.report(Diagnostic::error(
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
                    Expr::Neg { operand, .. } | Expr::Not { operand, .. } => {
                        stack.push((Node::Expr(operand), d + 1));
                    }
                    Expr::Binary { lhs, rhs, .. } => {
                        stack.push((Node::Expr(lhs), d + 1));
                        stack.push((Node::Expr(rhs), d + 1));
                    }
                    Expr::Call { args, .. } => {
                        for a in args {
                            stack.push((Node::Expr(a), d + 1));
                        }
                    }
                    Expr::Construct { fields, .. } | Expr::Variant { fields, .. } => {
                        for f in fields {
                            stack.push((Node::Expr(&f.value), d + 1));
                        }
                    }
                    Expr::Match {
                        scrutinee, arms, ..
                    } => {
                        stack.push((Node::Expr(scrutinee), d + 1));
                        for a in arms {
                            stack.push((Node::Expr(&a.body), d + 1));
                        }
                    }
                    Expr::Field { base, .. } => stack.push((Node::Expr(base), d + 1)),
                    Expr::Propagate { operand, .. } => stack.push((Node::Expr(operand), d + 1)),
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
                    Expr::Closure { body, .. } => stack.push((Node::Block(body), d + 1)),
                    Expr::MethodCall { receiver, args, .. } => {
                        stack.push((Node::Expr(receiver), d + 1));
                        for a in args {
                            stack.push((Node::Expr(a), d + 1));
                        }
                    }
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

/// `e` with the alias written before the name it begins (N49): a call, a construction or a
/// variant, the only nodes a qualified name can begin.
fn qualified(mut e: Expr, q: Option<Qualifier>) -> Expr {
    if q.is_none() {
        return e;
    }
    match &mut e {
        Expr::Call { qualifier, .. }
        | Expr::Construct { qualifier, .. }
        | Expr::Variant { qualifier, .. } => *qualifier = q,
        _ => {}
    }
    e
}
