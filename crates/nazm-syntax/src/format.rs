//! The canonical layout of a Nazm program.
//!
//! # One layout, and no options
//!
//! There is no configuration and there will not be. A formatter with options has as many
//! canonical forms as it has settings, and then two files in one project disagree and a
//! diff is about whitespace again. The point of this one is that any two people — or any
//! two agents — who run it on the same program get the same bytes.
//!
//! # It works on tokens, not on the tree
//!
//! A printer driven by the syntax tree deletes comments, because the tree does not have
//! them. So this walks the token stream with the comments merged back in by position and
//! decides spacing from the tokens on either side. The consequence is the property that
//! matters: **the formatter cannot change the program**. It re-emits exactly the tokens
//! it was given, in order, and `crates/nazm-syntax/tests/format.rs` checks that by
//! re-lexing the output and comparing kinds.
//!
//! # What it guarantees
//!
//! - **Idempotence.** Formatting formatted text returns it unchanged.
//! - **The same tokens**, never one added, dropped, split or joined.
//! - **Every comment kept**, on the line it was written on.
//!
//! # What it deliberately does not do
//!
//! It does not reflow. A long line stays long: where to break an expression is a
//! judgement about what the expression means, and a wrong guess reads worse than the
//! long line did. It also does not reorder or insert anything — a formatter that adds a
//! `;` is a formatter that changes programs.

use nazm_diag::Diagnostic;
use nazm_span::Span;

use crate::lexer::{lex_with_comments, Token, TokenKind};

const INDENT: &str = "    ";

/// One thing to print, with where it came from.
#[derive(Debug, Clone, Copy)]
struct Piece<'a> {
    text: &'a str,
    kind: Option<&'a TokenKind>,
    start: u32,
    end: u32,
}

/// Lay out `text` canonically.
///
/// # Errors
///
/// The lexer's diagnostics, when the text does not tokenise. Formatting something that
/// does not lex would mean guessing what it was meant to be, and writing the guess to
/// the file.
pub fn format(text: &str) -> Result<String, Vec<Diagnostic>> {
    let (tokens, errors, comments) = lex_with_comments(text);
    if !errors.is_empty() {
        return Err(errors);
    }
    let pieces = pieces(text, &tokens, &comments);
    let layout = Layout::of(text, &pieces);
    Ok(print(text, &pieces, &layout))
}

/// What the formatter keeps of a file's existing layout.
///
/// One thing, and it is a decision it refuses to make for the author: whether a block is
/// written on one line. `if a > b { a } else { b }` says the branches do not matter
/// much, and `if b == 95 { return true; }` is a guard clause whose whole value is being
/// one line. A formatter that turns either into three has overruled the author about
/// something it cannot see.
///
/// **Nothing is inserted.** An earlier version forced a blank line between top-level
/// functions, which is what most formatters do — and it broke every table of one-line
/// definitions in `compiler/lex.nz`, where adjacency is the point. Blank lines are
/// preserved where the author put them, capped at one, and added nowhere.
struct Layout {
    /// For the piece index of each `{`: whether its block stays on one line.
    inline: Vec<bool>,
    /// For the piece index of each brace: whether it opens or closes an effect set, the
    /// `{ io }` after a `!` (N36). A set is a list, not a block: it is always on one line,
    /// and its `}` never ends one, because the function's body follows it.
    set: Vec<bool>,
}

impl Layout {
    fn of(text: &str, pieces: &[Piece<'_>]) -> Self {
        let mut inline = vec![false; pieces.len()];
        let mut set = vec![false; pieces.len()];
        let mut previous: Option<&TokenKind> = None;

        // Open-brace positions, each with the number of comments seen before it. A
        // block written on one line stays on one line; the only thing that can stop it
        // is a comment inside, because `//` runs to the end of the line and would
        // swallow whatever followed it.
        //
        // An earlier version also required the block to contain no `;`, on the theory
        // that a one-line block should be one expression. `compiler/lex.nz` is full of
        // `if b == 95 { return true; }`, and exploding every guard clause into three
        // lines made the file materially worse to read. The author's line is the
        // author's.
        let mut open: Vec<(usize, usize)> = Vec::new();
        let mut comments = 0usize;
        for (i, piece) in pieces.iter().enumerate() {
            let Some(kind) = piece.kind else {
                comments += 1;
                continue;
            };
            match kind {
                TokenKind::LBrace => {
                    if previous == Some(&TokenKind::Bang) {
                        if let Some(slot) = set.get_mut(i) {
                            *slot = true;
                        }
                    }
                    open.push((i, comments));
                }
                TokenKind::RBrace => {
                    if let Some((j, seen)) = open.pop() {
                        let is_set = set.get(j).copied().unwrap_or(false);
                        let same_line = pieces
                            .get(j)
                            .is_some_and(|b| breaks(text, b.end, piece.start) == 0);
                        if let Some(slot) = inline.get_mut(j) {
                            // A comment inside still forces lines: `//` would swallow the rest.
                            *slot = (same_line || is_set) && seen == comments;
                        }
                        if let Some(slot) = set.get_mut(i) {
                            *slot = is_set;
                        }
                    }
                }
                _ => {}
            }
            previous = Some(kind);
        }
        Self { inline, set }
    }
}

/// Tokens and comments in source order.
///
/// A merge rather than a sort: both inputs are already ordered, and a comparison
/// function would have to decide ties that cannot occur.
fn pieces<'a>(text: &'a str, tokens: &'a [Token], comments: &'a [Span]) -> Vec<Piece<'a>> {
    let mut out = Vec::with_capacity(tokens.len() + comments.len());
    let mut c = comments.iter().peekable();
    let comment = |s: &Span| Piece {
        text: slice(text, *s).trim_end(),
        kind: None,
        start: s.start,
        end: s.end,
    };
    for t in tokens {
        if t.kind == TokenKind::Eof {
            break;
        }
        while c.peek().is_some_and(|s| s.start < t.span.start) {
            if let Some(s) = c.next() {
                out.push(comment(s));
            }
        }
        out.push(Piece {
            text: slice(text, t.span),
            kind: Some(&t.kind),
            start: t.span.start,
            end: t.span.end,
        });
    }
    out.extend(c.map(comment));
    out
}

fn slice(text: &str, span: Span) -> &str {
    text.get(span.start as usize..span.end as usize)
        .unwrap_or_default()
}

/// How many line breaks separate two offsets — the only thing the original layout is
/// consulted for.
fn breaks(text: &str, from: u32, to: u32) -> usize {
    text.get(from as usize..to as usize)
        .unwrap_or_default()
        .bytes()
        .filter(|b| *b == b'\n')
        .count()
}

/// Whether a token can end a value, which is what decides whether a following `-` is
/// subtraction or negation.
///
/// The same question the parser answers, and the reason it is written here too is that
/// getting it wrong prints `a - -1` as `a--1`, which no longer lexes the same way. The
/// token test catches that, and this comment says what it is catching.
fn ends_a_value(k: &TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Int(_)
            | TokenKind::IntTooLarge(_)
            | TokenKind::Float(_)
            | TokenKind::Str(_)
            | TokenKind::Ident(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::RParen
            | TokenKind::RBrace
            | TokenKind::Question
    )
}

/// Whether this token continues whatever the `}` before it was part of.
///
/// The closed list, written as one rule rather than as an exception per construct: a
/// closer, a separator, a projection or an operator all mean the expression is not over,
/// and everything else means it is. A `}` is deliberately absent — two closing braces are
/// two blocks ending, and each ends its own line.
fn continues_the_line(next: Option<&TokenKind>) -> bool {
    matches!(
        next,
        Some(
            TokenKind::Else
                | TokenKind::Semi
                | TokenKind::Comma
                | TokenKind::RParen
                | TokenKind::Dot
                | TokenKind::Question
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::EqEq
                | TokenKind::NotEq
                | TokenKind::Lt
                | TokenKind::LtEq
                | TokenKind::Gt
                | TokenKind::GtEq
                | TokenKind::AmpAmp
                | TokenKind::PipePipe
                | TokenKind::Amp
                | TokenKind::Pipe
                | TokenKind::Caret
                | TokenKind::Shl
                | TokenKind::Shr
        )
    )
}

struct Printer<'a> {
    text: &'a str,
    layout: &'a Layout,
    out: String,
    indent: usize,
    /// Whether each open block is being kept on one line, innermost last.
    inline: Vec<bool>,
    /// Which bracket opened each nesting level, innermost last: `true` for `{`.
    ///
    /// What a comma means depends on it. In a brace — a record's field list — a comma
    /// ends a line, exactly as a `;` does in a body. In a parenthesis — an argument list,
    /// a parameter list, a record construction — it separates items on one line. Without
    /// the stack the formatter would have to guess from the token alone, and would get
    /// one of the two wrong.
    brackets: Vec<bool>,
    /// Which bracket opened each nesting level, innermost last: `true` for `[` (Gate 2), where
    /// a `;` separates an array's element from its length rather than ending a statement.
    squares: Vec<bool>,
    /// The last two tokens printed. Two, because whether a `-` is negation depends on
    /// what came before *it*, not on the `-`.
    previous: Option<&'a TokenKind>,
    before: Option<&'a TokenKind>,
    /// Where the last piece ended in the source, so a blank line can be recognised.
    previous_end: u32,
    /// Whether the current output line is still empty.
    fresh_line: bool,
    /// Whether the current line continues a statement rather than beginning one.
    ///
    /// A continuation is indented two levels past its block, not one. One would put it
    /// level with the block's body, so a wrapped signature would look exactly like the
    /// first line of the function it introduces.
    continuing: bool,
    /// Whether a function's contract clauses are being printed (N87): each on its own line one
    /// level in, and the body's `{` back at the function's own level, so a reader sees the
    /// clauses as the header's and the body as the function's.
    contract: bool,
    /// Inside a declaration's attribute (Gate 2): whether its name has been printed, and how
    /// deep in its arguments' parentheses the printer is. An attribute ends its own line.
    attribute: Option<(bool, usize)>,
}

fn print<'a>(text: &'a str, pieces: &'a [Piece<'a>], layout: &'a Layout) -> String {
    let mut p = Printer {
        text,
        layout,
        out: String::new(),
        indent: 0,
        inline: Vec::new(),
        brackets: Vec::new(),
        squares: Vec::new(),
        previous: None,
        before: None,
        previous_end: 0,
        fresh_line: true,
        continuing: false,
        contract: false,
        attribute: None,
    };
    for (i, piece) in pieces.iter().enumerate() {
        let next = pieces.get(i + 1).and_then(|n| n.kind);
        match piece.kind {
            Some(kind) => p.token(piece, kind, i, next),
            None => p.comment(piece),
        }
    }
    p.finish()
}

impl<'a> Printer<'a> {
    /// A comment stays where it was written: after the code it sits beside, or on a line
    /// of its own. Moving one is the fastest way to make a formatter distrusted, because
    /// the reader can no longer tell what it refers to.
    fn comment(&mut self, piece: &Piece<'a>) {
        let trailing =
            !self.out.is_empty() && breaks(self.text, self.previous_end, piece.start) == 0;
        if trailing {
            // The `;` or `}` before it has already ended the line. Take that back: the
            // comment belongs on the line it was written on.
            while self.out.ends_with('\n') {
                self.out.pop();
            }
            self.out.push(' ');
        } else {
            self.break_line();
            self.blank_if_there_was_one(piece.start);
            // Among a function's contract clauses, a comment is one of them (N87).
            if self.contract {
                self.out.push_str(INDENT);
            } else {
                self.pad();
            }
        }
        self.out.push_str(piece.text);
        self.out.push('\n');
        self.fresh_line = true;
        self.previous_end = piece.end;
    }

    fn token(
        &mut self,
        piece: &Piece<'a>,
        kind: &'a TokenKind,
        at: usize,
        next: Option<&TokenKind>,
    ) {
        // A closing brace is printed at the enclosing level, so the level drops before
        // it rather than after — unless its block is on one line, where there is no
        // level and no break.
        let closing_inline = *kind == TokenKind::RBrace && self.inline.pop().unwrap_or(false);
        if *kind == TokenKind::RBrace {
            self.brackets.pop();
            self.squares.pop();
        }
        if *kind == TokenKind::RBrace && !closing_inline {
            self.indent = self.indent.saturating_sub(1);
            self.continuing = false;
            self.break_line();
        }

        // A line break the author wrote mid-statement is kept. Joining those lines is
        // reflowing in the other direction, and it is the destructive direction: a
        // wrapped parameter list exists because somebody decided the line was too long,
        // and a formatter has no way to know whether it was right.
        if !self.fresh_line
            && !self.in_inline()
            // An effect set's own braces are its line, not a place to break it.
            && !self.layout.set.get(at).copied().unwrap_or(false)
            && breaks(self.text, self.previous_end, piece.start) >= 1
        {
            self.break_line();
            self.continuing = true;
        }

        if self.fresh_line {
            if *kind != TokenKind::RBrace && !self.continuing {
                self.blank_if_there_was_one(piece.start);
            }
            // A contract clause starts its line at the top level of a declaration (N87).
            if self.indent == 0
                && self.continuing
                && matches!(kind, TokenKind::Ident(w) if w == "requires" || w == "ensures")
            {
                self.contract = true;
            }
            if self.contract && *kind == TokenKind::LBrace {
                self.continuing = false;
            }
            if self.contract && *kind != TokenKind::LBrace {
                self.out.push_str(INDENT);
            } else {
                self.pad();
            }
        } else if self.space_before(kind)
            // The empty effect set is `{}`: nothing to pad.
            && !(*kind == TokenKind::RBrace
                && self.previous == Some(&TokenKind::LBrace)
                && self.layout.set.get(at).copied().unwrap_or(false))
        {
            self.out.push(' ');
        }

        self.out.push_str(piece.text);
        self.fresh_line = false;

        if *kind == TokenKind::LBrace {
            self.contract = false;
        }
        match kind {
            TokenKind::LBrace => {
                let inline = self.layout.inline.get(at).copied().unwrap_or(false);
                self.inline.push(inline);
                self.brackets.push(true);
                self.squares.push(false);
                if !inline {
                    self.indent += 1;
                    self.continuing = false;
                    self.break_line();
                }
            }
            // A type parameter or argument list is one line, like an argument list:
            // `Pair[Int, Str]` is a type, and breaking inside it would be breaking a name.
            TokenKind::LParen | TokenKind::LBracket => {
                self.brackets.push(false);
                self.squares.push(*kind == TokenKind::LBracket);
            }
            TokenKind::RParen | TokenKind::RBracket => {
                self.brackets.pop();
                self.squares.pop();
            }
            TokenKind::Semi
                if !self.in_inline() && !self.squares.last().copied().unwrap_or(false) =>
            {
                self.continuing = false;
                self.break_line();
            }
            // A field list: one field to a line, like every other brace-delimited
            // sequence. An argument list is parenthesised and stays as it is.
            TokenKind::Comma
                if self.brackets.last().copied().unwrap_or(false) && !self.in_inline() =>
            {
                self.continuing = false;
                self.break_line();
            }
            // A `}` ends the line unless what follows **continues the same expression or
            // statement**. `} else` was the only case while blocks appeared only as an
            // `if`'s branches; N10 made `match` an expression, so `let n = match … { … };`
            // and `f(match … { … })` are ordinary, and a break before the `;` or the `)`
            // put punctuation on a line of its own. It did that for `let x = if c { 1 }
            // else { 2 };` too, which was a defect nobody had met because an `if`
            // initialiser written on one line is rare and a `match` one is not.
            TokenKind::RBrace
                if !continues_the_line(next)
                    && !self.in_inline()
                    && !self.layout.set.get(at).copied().unwrap_or(false) =>
            {
                self.continuing = false;
                self.break_line();
            }
            _ => {}
        }
        // An attribute ends at its name, or at the `)` that closes its arguments, and the
        // declaration it belongs to starts the next line at the same level (Gate 2).
        match (self.attribute, kind) {
            (None, TokenKind::At) if self.indent == 0 => self.attribute = Some((false, 0)),
            (Some((false, _)), TokenKind::Ident(_)) => {
                if matches!(next, Some(TokenKind::LParen)) {
                    self.attribute = Some((true, 0));
                } else {
                    self.attribute = None;
                    self.continuing = false;
                    self.break_line();
                }
            }
            (Some((true, d)), TokenKind::LParen) => self.attribute = Some((true, d + 1)),
            (Some((true, 1)), TokenKind::RParen) => {
                self.attribute = None;
                self.continuing = false;
                self.break_line();
            }
            (Some((true, d)), TokenKind::RParen) => {
                self.attribute = Some((true, d.saturating_sub(1)));
            }
            _ => {}
        }
        self.before = self.previous;
        self.previous = Some(kind);
        self.previous_end = piece.end;
    }

    /// Whether the block currently being printed is being kept on one line.
    fn in_inline(&self) -> bool {
        self.inline.last().copied().unwrap_or(false)
    }

    /// Whether a space goes between the previous token and this one.
    ///
    /// Written as the closed list of places a space is *unwanted*: a space between two
    /// things is the default and needs no rule, and an open-ended list of exceptions is
    /// how a formatter acquires cases nobody can predict.
    fn space_before(&self, next: &TokenKind) -> bool {
        let Some(previous) = self.previous else {
            return false;
        };
        // Negation binds to its operand, arithmetic or logical. A `!` after a value is the
        // effect-set marker, `-> Int ! {io}`, and keeps its spaces.
        if matches!(
            previous,
            TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde
        ) && !self.before.is_some_and(ends_a_value)
        {
            return false;
        }
        // Nothing before a closer or a separator. `.` is here because it binds to what
        // it projects out of: `p.x`, never `p . x` — and `?` for the same reason:
        // `parse(s)?`, never `parse(s) ?`.
        if matches!(
            next,
            TokenKind::RParen
                | TokenKind::RBracket
                | TokenKind::Comma
                | TokenKind::Colon
                | TokenKind::Semi
                | TokenKind::Dot
                | TokenKind::Question
        ) {
            return false;
        }
        // `@name`: an attribute's `@` is glued to its name (Gate 2).
        if *previous == TokenKind::At {
            return false;
        }
        // Nothing after an opener, or after a `.` or a `::`; and nothing before a `::`, which
        // binds an alias to the item it names: `t::split`, never `t :: split` (N49).
        if matches!(
            previous,
            TokenKind::LParen | TokenKind::LBracket | TokenKind::Dot | TokenKind::ColonColon
        ) || *next == TokenKind::ColonColon
        {
            return false;
        }
        // `Pair[` is one thing: a type argument list is glued to what it applies to, and an
        // index to what it indexes (Gate 2) — a name, a closing bracket or a call. Anywhere
        // else a `[` begins an array or an array type, and is spaced like any operand.
        if *next == TokenKind::LBracket {
            return !matches!(
                previous,
                TokenKind::Ident(_) | TokenKind::RBracket | TokenKind::RParen
            );
        }
        // `f(` is one thing; `while (` is two. The bracket is glued to a name — or to the
        // type arguments after one, `f[Int](` — and separated from anything else.
        // `fn(` too, in a function type or a closure (N50): `fn` before `(` is never a name.
        if *next == TokenKind::LParen {
            return !matches!(
                previous,
                TokenKind::Ident(_) | TokenKind::RBracket | TokenKind::Fn
            );
        }
        true
    }

    /// One blank line, if the source had at least one there.
    ///
    /// At most one, whatever the input had: more is a preference, and the whole point of
    /// a canonical layout is not to carry preferences.
    fn blank_if_there_was_one(&mut self, start: u32) {
        if self.out.is_empty() || self.previous == Some(&TokenKind::LBrace) {
            return;
        }
        if breaks(self.text, self.previous_end, start) >= 2 {
            self.blank();
        }
    }

    fn blank(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn break_line(&mut self) {
        if !self.fresh_line {
            self.out.push('\n');
            self.fresh_line = true;
        }
    }

    fn pad(&mut self) {
        let levels = self.indent + if self.continuing { 2 } else { 0 };
        for _ in 0..levels {
            self.out.push_str(INDENT);
        }
    }

    fn finish(mut self) -> String {
        self.break_line();
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if self.out.is_empty() {
            self.out.push('\n');
        }
        self.out
    }
}
