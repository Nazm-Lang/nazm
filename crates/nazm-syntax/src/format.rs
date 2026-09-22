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
}

impl Layout {
    fn of(text: &str, pieces: &[Piece<'_>]) -> Self {
        let mut inline = vec![false; pieces.len()];

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
                TokenKind::LBrace => open.push((i, comments)),
                TokenKind::RBrace => {
                    if let Some((j, seen)) = open.pop() {
                        let same_line = pieces
                            .get(j)
                            .is_some_and(|b| breaks(text, b.end, piece.start) == 0);
                        if let Some(slot) = inline.get_mut(j) {
                            *slot = same_line && seen == comments;
                        }
                    }
                }
                _ => {}
            }
        }
        Self { inline }
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
            | TokenKind::Str(_)
            | TokenKind::Ident(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::RParen
            | TokenKind::RBrace
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
}

fn print<'a>(text: &'a str, pieces: &'a [Piece<'a>], layout: &'a Layout) -> String {
    let mut p = Printer {
        text,
        layout,
        out: String::new(),
        indent: 0,
        inline: Vec::new(),
        brackets: Vec::new(),
        previous: None,
        before: None,
        previous_end: 0,
        fresh_line: true,
        continuing: false,
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
            self.pad();
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
            && breaks(self.text, self.previous_end, piece.start) >= 1
        {
            self.break_line();
            self.continuing = true;
        }

        if self.fresh_line {
            if *kind != TokenKind::RBrace && !self.continuing {
                self.blank_if_there_was_one(piece.start);
            }
            self.pad();
        } else if self.space_before(kind) {
            self.out.push(' ');
        }

        self.out.push_str(piece.text);
        self.fresh_line = false;

        match kind {
            TokenKind::LBrace => {
                let inline = self.layout.inline.get(at).copied().unwrap_or(false);
                self.inline.push(inline);
                self.brackets.push(true);
                if !inline {
                    self.indent += 1;
                    self.continuing = false;
                    self.break_line();
                }
            }
            TokenKind::LParen => self.brackets.push(false),
            TokenKind::RParen => {
                self.brackets.pop();
            }
            TokenKind::Semi if !self.in_inline() => {
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
            // `} else` stays on one line, and so does everything inside a block written
            // on one line. Every other `}` ends the line.
            TokenKind::RBrace if next != Some(&TokenKind::Else) && !self.in_inline() => {
                self.continuing = false;
                self.break_line();
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
        // Negation binds to its operand.
        if *previous == TokenKind::Minus && !self.before.is_some_and(ends_a_value) {
            return false;
        }
        // Nothing before a closer or a separator. `.` is here because it binds to what
        // it projects out of: `p.x`, never `p . x`.
        if matches!(
            next,
            TokenKind::RParen
                | TokenKind::Comma
                | TokenKind::Colon
                | TokenKind::Semi
                | TokenKind::Dot
        ) {
            return false;
        }
        // Nothing after an opener, or after a `.`.
        if matches!(previous, TokenKind::LParen | TokenKind::Dot) {
            return false;
        }
        // `f(` is one thing; `while (` is two. The bracket is glued to a name and
        // separated from anything else.
        if *next == TokenKind::LParen {
            return !matches!(previous, TokenKind::Ident(_));
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
