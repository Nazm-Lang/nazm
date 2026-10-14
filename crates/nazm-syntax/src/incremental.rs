//! Incremental reparse: a file's next revision from its previous one and one edit (N76).
//!
//! # The contract
//!
//! For every text and every edit, [`Revision::edit`] returns a revision **equal** to
//! [`Revision::new`] of the edited text: the same lexemes, the same concrete tree (kind,
//! bytes and span of every node and leaf), the same abstract items (spans included) and the
//! same diagnostics in the same order. `architecture.md` §7.77 states why that holds; the
//! convergence tests in `tests/incremental.rs` are what would notice if it stopped holding.
//!
//! # How
//!
//! A revision keeps a *chunk* per top-level item: the lexemes it owns (its leading trivia
//! and its tokens), how far its parse read, the cascade mute on entering it, and what it
//! produced. An edit relexes from the first lexeme of the first chunk that could see it
//! until the scan lands on a boundary of the old scan, and reparses from that chunk until
//! the parse lands on an old chunk boundary with the same mute. Everything before is kept
//! as it was; everything after is kept and moved by the edit's length. Nothing here parses
//! or lexes on its own: [`crate::lexer::scan_one`] and [`crate::parser::parse_region`] are
//! the same functions a full parse runs.

use std::sync::Arc;

use nazm_diag::Diagnostic;
use nazm_span::{FileId, Span};

use crate::ast::{
    Block, EffectDecl, Enum, Expr, Field, FieldInit, FnTypeName, ForeignDecl, Function, Import,
    MatchArm, Param, PatternField, Place, Program, Qualifier, Record, Requirement, Stmt, TypeName,
    TypeParam, Variant,
};
use crate::cst::{Cst, GreenChild};
use crate::lexer::{self, FPiece, Lexeme, Token, TokenKind};
use crate::parser::{self, Item, Region};

/// Replace the bytes `start..end` of a revision's text with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// An edit that does not describe a change to the text it was applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// `start > end`, or `end` past the end of the text.
    OutOfRange,
    /// `start` or `end` falls inside a UTF-8 sequence.
    NotACharBoundary,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::OutOfRange => "the edit's range is not within the text",
            Self::NotACharBoundary => "the edit's range splits a character",
        })
    }
}

impl std::error::Error for EditError {}

/// What one revision's parse did: how much it read again and how much it kept.
///
/// A full parse relexes and reparses everything and reuses nothing. The counts are what the
/// tests and the benchmark read to tell the incremental path from a silent full one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseStats {
    /// Whether this revision was parsed from scratch.
    pub full: bool,
    pub lexemes_relexed: usize,
    pub lexemes_total: usize,
    pub tokens_reparsed: usize,
    pub tokens_total: usize,
    pub chunks_reparsed: usize,
    pub chunks_reused: usize,
}

/// One top-level item's share of a revision.
#[derive(Debug, Clone)]
struct Chunk {
    /// Its first lexeme: its leading trivia's, or its first token's.
    lex_start: usize,
    /// Its tokens, `tok_start..tok_end`, and the furthest token its parse read.
    tok_start: usize,
    tok_end: usize,
    seen: usize,
    /// The cascade mute on entering it and on leaving it.
    muted: bool,
    muted_after: bool,
    /// What it produced, as parsed: shared between revisions, and moved by `shift` bytes
    /// when read.
    item: Option<Arc<Item>>,
    errors: Arc<Vec<Diagnostic>>,
    shift: i64,
    /// How many of the root's green children are its own.
    children: usize,
}

/// One revision of one file: its text and everything parsing it produced.
#[derive(Debug, Clone)]
pub struct Revision {
    file: FileId,
    text: String,
    lexemes: Vec<Lexeme>,
    lex_errors: Vec<Diagnostic>,
    chunks: Vec<Chunk>,
    cst: Cst,
    stats: ParseStats,
}

impl Revision {
    /// Parse `text` from scratch.
    ///
    /// # Stack
    ///
    /// As [`crate::parse_source`]: the caller provides [`crate::PARSE_STACK_BYTES`].
    #[must_use]
    pub fn new(file: FileId, text: impl Into<String>) -> Self {
        let text = text.into();
        let (lexemes, lex_errors) = lexer::lex_lossless_in(file, &text);
        let tokens = tokens_of(&lexemes);
        let parsed = parser::parse_region(
            file,
            &text,
            &lexemes,
            &tokens,
            Region {
                start: 0,
                muted: false,
                landings: &[],
            },
            None,
            crate::cst::Strings::default(),
        );
        let tok_lex = token_lexemes(&lexemes);
        let mut chunks = chunks_of(parsed.chunks, &tok_lex);
        let children = children_of(&parsed.green);
        assign_children(&mut chunks, &children, 0, &lexemes, &tok_lex);
        let cst = Cst::from_green(file, parsed.green, parsed.strings);
        let stats = ParseStats {
            full: true,
            lexemes_relexed: lexemes.len(),
            lexemes_total: lexemes.len(),
            tokens_reparsed: tokens.len(),
            tokens_total: tokens.len(),
            chunks_reparsed: chunks.len(),
            chunks_reused: 0,
        };
        Self {
            file,
            text,
            lexemes,
            lex_errors,
            chunks,
            cst,
            stats,
        }
    }

    /// The next revision: this one's text with `edit` applied, parsed reusing every chunk
    /// the edit cannot have changed.
    ///
    /// # Errors
    ///
    /// When `edit` is not a range of this revision's text on character boundaries.
    ///
    /// # Stack
    ///
    /// As [`Revision::new`].
    pub fn edit(&self, edit: &TextEdit) -> Result<Self, EditError> {
        let (a, b) = (edit.start as usize, edit.end as usize);
        if a > b || b > self.text.len() {
            return Err(EditError::OutOfRange);
        }
        if !self.text.is_char_boundary(a) || !self.text.is_char_boundary(b) {
            return Err(EditError::NotACharBoundary);
        }
        let mut text = String::with_capacity(self.text.len() - (b - a) + edit.text.len());
        text.push_str(self.text.get(..a).unwrap_or_default());
        text.push_str(&edit.text);
        text.push_str(self.text.get(b..).unwrap_or_default());
        if u32::try_from(text.len()).is_err() {
            return Err(EditError::OutOfRange);
        }
        let delta = i64::try_from(edit.text.len()).unwrap_or(i64::MAX)
            - i64::try_from(b - a).unwrap_or(i64::MAX);
        let edit_end_new = a + edit.text.len();

        let old = &self.lexemes;
        let old_tok_lex = token_lexemes(old);

        // The first lexeme the edit can change: the first whose end reaches the edit's start,
        // because the lexer read one byte past every lexeme's end to know where it ended.
        let first_changed = old.partition_point(|l| (l.span().end as usize) < a);
        // The first token at or after it, by index into the old token list.
        let first_changed_tok = old_tok_lex.partition_point(|&li| li < first_changed);

        // The first chunk that owns the changed lexeme or read a token at or after it.
        let owner = self
            .chunks
            .partition_point(|c| c.lex_start <= first_changed)
            .saturating_sub(1);
        let owner = if self.chunks.is_empty() || first_changed >= self.tail_start() {
            self.chunks.len()
        } else {
            owner
        };
        let reader = self
            .chunks
            .iter()
            .position(|c| c.seen >= first_changed_tok)
            .unwrap_or(self.chunks.len());
        let f = owner.min(reader);
        let lex_from = self
            .chunks
            .get(f)
            .map_or_else(|| self.tail_start(), |c| c.lex_start);
        let tok_from = self
            .chunks
            .get(f)
            .map_or_else(|| self.tail_token_start(), |c| c.tok_start);
        let muted_from = match self.chunks.get(f) {
            Some(c) => c.muted,
            None => self.chunks.last().is_some_and(|c| c.muted_after),
        };

        // Relex from the first changed lexeme — every lexeme before it ended, and was read,
        // before the edit — until a boundary at or past the edit's end that the old scan
        // also had; from there the old scan, moved, is what a fresh scan would produce.
        let relex_from = first_changed;
        let mut fresh: Vec<Lexeme> = Vec::new();
        let mut fresh_errors = Vec::new();
        let mut i = old
            .get(relex_from)
            .map_or(self.text.len(), |l| l.span().start as usize);
        let mut resync: Option<usize> = None;
        loop {
            if i >= edit_end_new {
                let at_old = i64::try_from(i).unwrap_or(i64::MAX) - delta;
                if let Ok(at_old) = usize::try_from(at_old) {
                    let m = old.partition_point(|l| (l.span().start as usize) < at_old);
                    if old
                        .get(m)
                        .is_some_and(|l| l.span().start as usize == at_old)
                        && m >= first_changed
                    {
                        resync = Some(m);
                        break;
                    }
                }
            }
            if i >= text.len() {
                fresh.push(lexer::eof(self.file, &text));
                break;
            }
            let (lexeme, next) = lexer::scan_one(self.file, &text, i, &mut fresh_errors);
            fresh.push(lexeme);
            i = next;
        }
        let m = resync.unwrap_or(old.len());
        let relex_start_byte = old.get(relex_from).map_or(a, |l| l.span().start as usize);
        let old_m_byte = old.get(m).map(|l| l.span().start as usize);

        let mut lexemes: Vec<Lexeme> = Vec::with_capacity(relex_from + fresh.len() + old.len() - m);
        lexemes.extend(old.get(..relex_from).unwrap_or_default().iter().cloned());
        let relexed = fresh.len();
        lexemes.extend(fresh);
        lexemes.extend(
            old.get(m..)
                .unwrap_or_default()
                .iter()
                .map(|l| shift_lexeme(l, delta)),
        );
        let lex_delta = signed(relex_from + relexed) - signed(m);

        let mut lex_errors: Vec<Diagnostic> = self
            .lex_errors
            .iter()
            .filter(|d| (d.primary.span.start as usize) < relex_start_byte)
            .cloned()
            .collect();
        lex_errors.extend(fresh_errors);
        if let Some(mb) = old_m_byte {
            lex_errors.extend(
                self.lex_errors
                    .iter()
                    .filter(|d| (d.primary.span.start as usize) >= mb)
                    .map(|d| {
                        let mut d = d.clone();
                        shift_diagnostic(&mut d, self.file, delta);
                        d
                    }),
            );
        }

        let tokens = tokens_of(&lexemes);
        let tok_lex = token_lexemes(&lexemes);
        let old_tok_m = old_tok_lex.partition_point(|&li| li < m);
        let new_tok_m = tok_lex.partition_point(|&li| signed(li) < signed(m) + lex_delta);
        let tok_delta = signed(new_tok_m) - signed(old_tok_m);

        // Where the reparse may stop: an old chunk wholly inside the reused tail, whose
        // leading trivia starts right after a token in the new scan.
        let landings: Vec<(usize, bool)> = self
            .chunks
            .iter()
            .skip(f + 1)
            .filter(|c| c.lex_start >= m)
            .filter(|c| {
                let at = signed(c.lex_start) + lex_delta;
                at == 0
                    || usize::try_from(at - 1)
                        .ok()
                        .and_then(|k| lexemes.get(k))
                        .is_some_and(|l| matches!(l, Lexeme::Token(_)))
            })
            .filter_map(|c| {
                usize::try_from(signed(c.tok_start) + tok_delta)
                    .ok()
                    .map(|t| (t, c.muted))
            })
            .collect();

        let parsed = parser::parse_region(
            self.file,
            &text,
            lexemes.get(lex_from..).unwrap_or_default(),
            &tokens,
            Region {
                start: tok_from,
                muted: muted_from,
                landings: &landings,
            },
            None,
            self.cst.strings(),
        );
        let reparsed_tokens = parsed.stop - tok_from;
        let region_children = children_of(&parsed.green);
        let mut region_chunks = chunks_of(parsed.chunks, &tok_lex);

        // The old chunk the reparse landed on, if it landed.
        let landed = landings.iter().any(|(t, _)| *t == parsed.stop).then(|| {
            self.chunks
                .iter()
                .position(|c| signed(c.tok_start) + tok_delta == signed(parsed.stop))
        });
        let resume = landed.flatten();

        // The root's children: the kept prefix, the region, the kept suffix and tail.
        let old_children = self.cst.root_children();
        let prefix_children: usize = self.chunks.iter().take(f).map(|c| c.children).sum();
        let mut children: Vec<GreenChild> =
            old_children.iter().take(prefix_children).cloned().collect();
        let base = lexemes
            .get(lex_from)
            .map_or(text.len(), |l| l.span().start as usize);
        assign_children(
            &mut region_chunks,
            &region_children,
            base,
            &lexemes,
            &tok_lex,
        );
        children.extend(region_children);
        let mut chunks: Vec<Chunk> = self.chunks.iter().take(f).cloned().collect();
        let reparsed_chunks = region_chunks.len();
        chunks.extend(region_chunks);
        let mut reused_suffix = 0;
        if let Some(j) = resume {
            let suffix_children: usize = self.chunks.iter().take(j).map(|c| c.children).sum();
            children.extend(old_children.iter().skip(suffix_children).cloned());
            for c in self.chunks.iter().skip(j) {
                let mut c = c.clone();
                c.lex_start = shift_index(c.lex_start, lex_delta);
                c.tok_start = shift_index(c.tok_start, tok_delta);
                c.tok_end = shift_index(c.tok_end, tok_delta);
                c.seen = shift_index(c.seen, tok_delta);
                c.shift += delta;
                chunks.push(c);
                reused_suffix += 1;
            }
        }
        let cst = Cst::from_children(self.file, children, parsed.strings);

        let stats = ParseStats {
            full: false,
            lexemes_relexed: relexed,
            lexemes_total: lexemes.len(),
            tokens_reparsed: reparsed_tokens,
            tokens_total: tokens.len(),
            chunks_reparsed: reparsed_chunks,
            chunks_reused: f + reused_suffix,
        };
        Ok(Self {
            file: self.file,
            text,
            lexemes,
            lex_errors,
            chunks,
            cst,
            stats,
        })
    }

    /// The text this revision is of.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The file this revision's spans name.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// The lossless scan.
    #[must_use]
    pub fn lexemes(&self) -> &[Lexeme] {
        &self.lexemes
    }

    /// The concrete tree.
    #[must_use]
    pub const fn cst(&self) -> &Cst {
        &self.cst
    }

    /// What this revision's parse did.
    #[must_use]
    pub const fn stats(&self) -> ParseStats {
        self.stats
    }

    /// The abstract tree, built from the chunks' items in order.
    #[must_use]
    pub fn program(&self) -> Program {
        let mut program = Program {
            functions: Vec::new(),
            records: Vec::new(),
            enums: Vec::new(),
            imports: Vec::new(),
            traits: Vec::new(),
            impls: Vec::new(),
            consts: Vec::new(),
            hole: None,
        };
        for c in &self.chunks {
            if let Some(item) = &c.item {
                let mut item = (**item).clone();
                if c.shift != 0 {
                    item.shift(self.file, c.shift);
                }
                item.push_into(&mut program);
            }
        }
        program
    }

    /// Every lexical and syntactic diagnostic, lexical first, exactly as
    /// [`crate::parse_file`] reports them for the same text.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut out = self.lex_errors.clone();
        for c in &self.chunks {
            for d in c.errors.iter() {
                let mut d = d.clone();
                if c.shift != 0 {
                    shift_diagnostic(&mut d, self.file, c.shift);
                }
                out.push(d);
            }
        }
        out
    }

    /// The first lexeme after the last chunk: the trailing trivia and `Eof`.
    fn tail_start(&self) -> usize {
        self.chunks.last().map_or(0, |c| {
            token_lexemes(&self.lexemes)
                .get(c.tok_end.saturating_sub(1))
                .map_or(0, |&li| li + 1)
        })
    }

    /// The first token after the last chunk: `Eof`.
    fn tail_token_start(&self) -> usize {
        self.chunks.last().map_or(0, |c| c.tok_end)
    }
}

/// The scan's tokens, borrowed in order.
fn tokens_of(lexemes: &[Lexeme]) -> Vec<&Token> {
    lexemes
        .iter()
        .filter_map(|l| match l {
            Lexeme::Token(t) => Some(t),
            _ => None,
        })
        .collect()
}

/// For each token, the index of its lexeme.
fn token_lexemes(lexemes: &[Lexeme]) -> Vec<usize> {
    lexemes
        .iter()
        .enumerate()
        .filter_map(|(i, l)| matches!(l, Lexeme::Token(_)).then_some(i))
        .collect()
}

fn chunks_of(parsed: Vec<parser::ChunkParse>, tok_lex: &[usize]) -> Vec<Chunk> {
    let mut out: Vec<Chunk> = Vec::with_capacity(parsed.len());
    for c in parsed {
        // A chunk's lexemes begin just after the previous token: its leading trivia.
        let lex_start = c
            .start
            .checked_sub(1)
            .and_then(|t| tok_lex.get(t))
            .map_or(0, |&li| li + 1);
        out.push(Chunk {
            lex_start,
            tok_start: c.start,
            tok_end: c.end,
            seen: c.seen,
            muted: c.muted,
            muted_after: c.muted_after,
            item: c.item.map(Arc::new),
            errors: Arc::new(c.errors),
            shift: 0,
            children: 0,
        });
    }
    out
}

/// The root-level children of a `SourceFile` green node, owned.
fn children_of(green: &cstree::green::GreenNode) -> Vec<GreenChild> {
    use cstree::util::NodeOrToken;
    green
        .children()
        .map(|c| match c {
            NodeOrToken::Node(n) => NodeOrToken::Node(n.clone()),
            NodeOrToken::Token(t) => NodeOrToken::Token(t.clone()),
        })
        .collect()
}

fn text_len(c: &GreenChild) -> usize {
    use cstree::util::NodeOrToken;
    let len = match c {
        NodeOrToken::Node(n) => n.text_len(),
        NodeOrToken::Token(t) => t.text_len(),
    };
    u32::from(len) as usize
}

/// Count each chunk's share of `children`, which begin at byte `base` of the file.
///
/// A chunk owns the children from its first lexeme's byte up to the next chunk's; the last
/// owns up to the end of its last token. Children past that are the tail's — trailing
/// trivia — and are counted by nobody.
fn assign_children(
    chunks: &mut [Chunk],
    children: &[GreenChild],
    base: usize,
    lexemes: &[Lexeme],
    tok_lex: &[usize],
) {
    let start_of = |c: &Chunk| {
        lexemes
            .get(c.lex_start)
            .map_or(usize::MAX, |l| l.span().start as usize)
    };
    let end_of = |c: &Chunk| {
        c.tok_end
            .checked_sub(1)
            .and_then(|t| tok_lex.get(t))
            .and_then(|&li| lexemes.get(li))
            .map_or(0, |l| l.span().end as usize)
    };
    let starts: Vec<usize> = chunks.iter().map(start_of).collect();
    let last_end = chunks.last().map_or(0, end_of);
    for c in chunks.iter_mut() {
        c.children = 0;
    }
    let mut offset = base;
    for child in children {
        let owner = starts.partition_point(|&s| s <= offset);
        let is_last = owner == starts.len();
        if let Some(c) = owner.checked_sub(1).and_then(|k| chunks.get_mut(k)) {
            if !is_last || offset < last_end {
                c.children += 1;
            }
        }
        offset += text_len(child);
    }
}

const fn signed(n: usize) -> i64 {
    n as i64
}

fn shift_index(n: usize, by: i64) -> usize {
    usize::try_from(signed(n) + by).unwrap_or(0)
}

fn shift_lexeme(l: &Lexeme, delta: i64) -> Lexeme {
    let move_span = |s: Span| moved(s, delta);
    match l {
        Lexeme::Token(t) => Lexeme::Token(Token {
            kind: shift_kind(&t.kind, delta),
            span: move_span(t.span),
        }),
        Lexeme::Whitespace(s) => Lexeme::Whitespace(move_span(*s)),
        Lexeme::Comment(s) => Lexeme::Comment(move_span(*s)),
        Lexeme::Invalid(s) => Lexeme::Invalid(move_span(*s)),
    }
}

/// A token's kind, moved: an interpolated string's holes carry positions of their own (L4), which
/// move with the token.
fn shift_kind(kind: &TokenKind, delta: i64) -> TokenKind {
    match kind {
        TokenKind::FStr(pieces) => TokenKind::FStr(
            pieces
                .iter()
                .map(|p| match p {
                    FPiece::Hole {
                        text,
                        at,
                        spec,
                        span,
                    } => FPiece::Hole {
                        text: text.clone(),
                        at: u32::try_from(i64::from(*at) + delta).unwrap_or(0),
                        spec: *spec,
                        span: moved(*span, delta),
                    },
                    FPiece::Text(t) => FPiece::Text(t.clone()),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn moved(s: Span, delta: i64) -> Span {
    let at = |n: u32| u32::try_from(i64::from(n) + delta).unwrap_or(0);
    Span::new(s.file, at(s.start), at(s.end))
}

fn shift_diagnostic(d: &mut Diagnostic, file: FileId, delta: i64) {
    let one = |s: &mut Span| {
        if s.file == file {
            *s = moved(*s, delta);
        }
    };
    one(&mut d.primary.span);
    for l in &mut d.secondary {
        one(&mut l.span);
    }
    for f in &mut d.fixes {
        one(&mut f.span);
    }
}

/// Moving every span of an abstract item by the same number of bytes: what keeping an item
/// the edit did not reach means for its positions.
///
/// Exhaustive on purpose — every field of every type is named — so a field added to the
/// abstract tree without a line here fails to compile rather than keeping a stale position.
trait Shift {
    fn shift(&mut self, file: FileId, by: i64);
}

/// Move every span of `e` in `file` by `by` bytes (L4): an interpolation's hole is parsed on its
/// own and then placed where it is written.
pub(crate) fn shift_expr(e: &mut Expr, file: FileId, by: i64) {
    e.shift(file, by);
}

impl Shift for Span {
    fn shift(&mut self, file: FileId, by: i64) {
        if self.file == file {
            *self = moved(*self, by);
        }
    }
}

impl<T: Shift> Shift for Option<T> {
    fn shift(&mut self, file: FileId, by: i64) {
        if let Some(t) = self {
            t.shift(file, by);
        }
    }
}

impl<T: Shift> Shift for Vec<T> {
    fn shift(&mut self, file: FileId, by: i64) {
        for t in self {
            t.shift(file, by);
        }
    }
}

impl<T: Shift> Shift for Box<T> {
    fn shift(&mut self, file: FileId, by: i64) {
        (**self).shift(file, by);
    }
}

impl Shift for (String, Span) {
    fn shift(&mut self, file: FileId, by: i64) {
        self.1.shift(file, by);
    }
}

impl Item {
    pub(crate) fn shift(&mut self, file: FileId, by: i64) {
        match self {
            Self::Function(f) => f.shift(file, by),
            Self::Record(r) => r.shift(file, by),
            Self::Enum(e) => e.shift(file, by),
            Self::Import(i) => i.shift(file, by),
            Self::Trait(t) => t.shift(file, by),
            Self::Const(c) => c.shift(file, by),
            Self::Impl(decl, methods) => {
                decl.shift(file, by);
                methods.shift(file, by);
            }
            Self::Methods(methods) => methods.shift(file, by),
        }
    }
}

impl Shift for crate::ast::Attribute {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            name: _,
            name_span,
            args,
            span,
        } = self;
        name_span.shift(file, by);
        if let Some(args) = args {
            for a in args {
                a.shift(file, by);
            }
        }
        span.shift(file, by);
    }
}

impl Shift for crate::ast::AttrArg {
    fn shift(&mut self, file: FileId, by: i64) {
        match self {
            Self::Str { value: _, span } | Self::Word { name: _, span } => span.shift(file, by),
            Self::Pair {
                key: _,
                key_span,
                value: _,
                span,
            } => {
                key_span.shift(file, by);
                span.shift(file, by);
            }
            Self::Call {
                name: _,
                name_span,
                args,
                span,
            } => {
                name_span.shift(file, by);
                for a in args {
                    a.shift(file, by);
                }
                span.shift(file, by);
            }
        }
    }
}

impl Shift for crate::ast::Const {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            name: _,
            name_span,
            pub_span,
            ty,
            value,
            span,
            is_static: _,
        } = self;
        attrs.shift(file, by);
        name_span.shift(file, by);
        pub_span.shift(file, by);
        ty.shift(file, by);
        value.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for crate::ast::Trait {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            name: _,
            name_span,
            pub_span,
            methods,
            span,
        } = self;
        attrs.shift(file, by);
        name_span.shift(file, by);
        pub_span.shift(file, by);
        methods.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for crate::ast::TraitMethod {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            name: _,
            name_span,
            params,
            return_type,
            effects,
            body,
            span,
        } = self;
        name_span.shift(file, by);
        params.shift(file, by);
        return_type.shift(file, by);
        effects.shift(file, by);
        body.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for crate::ast::ImplDecl {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            type_params,
            trait_name,
            self_ty,
            impl_span,
            span,
        } = self;
        attrs.shift(file, by);
        type_params.shift(file, by);
        trait_name.shift(file, by);
        self_ty.shift(file, by);
        impl_span.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for crate::ast::MethodOf {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            trait_name,
            self_ty,
            impl_span,
        } = self;
        trait_name.shift(file, by);
        self_ty.shift(file, by);
        impl_span.shift(file, by);
    }
}

impl Shift for Import {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            path: _,
            path_span,
            alias,
            reexport,
            span,
        } = self;
        path_span.shift(file, by);
        alias.shift(file, by);
        if let Some(r) = reexport {
            r.pub_span.shift(file, by);
            for n in r.names.iter_mut().flatten() {
                n.span.shift(file, by);
                if let Some((_, at)) = &mut n.alias {
                    at.shift(file, by);
                }
            }
        }
        span.shift(file, by);
    }
}

impl Shift for Qualifier {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self { name: _, span } = self;
        span.shift(file, by);
    }
}

impl Shift for Record {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            name: _,
            name_span,
            pub_span,
            type_params,
            fields,
            foreign,
            span,
        } = self;
        attrs.shift(file, by);
        name_span.shift(file, by);
        pub_span.shift(file, by);
        type_params.shift(file, by);
        fields.shift(file, by);
        if let Some(f) = foreign {
            f.abi_span.shift(file, by);
        }
        span.shift(file, by);
    }
}

impl Shift for Field {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            name: _,
            name_span,
            ty,
            span,
        } = self;
        attrs.shift(file, by);
        name_span.shift(file, by);
        ty.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for Enum {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            name: _,
            name_span,
            pub_span,
            type_params,
            variants,
            span,
        } = self;
        attrs.shift(file, by);
        name_span.shift(file, by);
        pub_span.shift(file, by);
        type_params.shift(file, by);
        variants.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for Variant {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            name: _,
            name_span,
            fields,
            span,
        } = self;
        attrs.shift(file, by);
        name_span.shift(file, by);
        fields.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for Function {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            attrs,
            name: _,
            name_span,
            pub_span,
            type_params,
            effect_params,
            params,
            return_type,
            effects,
            contracts,
            body,
            foreign,
            export,
            method_of,
            span,
        } = self;
        attrs.shift(file, by);
        name_span.shift(file, by);
        pub_span.shift(file, by);
        type_params.shift(file, by);
        effect_params.shift(file, by);
        params.shift(file, by);
        return_type.shift(file, by);
        effects.shift(file, by);
        for c in contracts.iter_mut() {
            c.keyword_span.shift(file, by);
            c.expr.shift(file, by);
            c.span.shift(file, by);
        }
        body.shift(file, by);
        foreign.shift(file, by);
        export.shift(file, by);
        method_of.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for ForeignDecl {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            abi: _,
            abi_span,
            symbol: _,
            symbol_span,
        } = self;
        abi_span.shift(file, by);
        symbol_span.shift(file, by);
    }
}

impl Shift for EffectDecl {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self { span, names } = self;
        span.shift(file, by);
        names.shift(file, by);
    }
}

impl Shift for Param {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            name: _,
            name_span,
            ty,
            span,
        } = self;
        name_span.shift(file, by);
        ty.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for TypeName {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            qualifier,
            function,
            name: _,
            name_span,
            span,
            args,
            length,
        } = self;
        qualifier.shift(file, by);
        function.shift(file, by);
        name_span.shift(file, by);
        span.shift(file, by);
        args.shift(file, by);
        length.shift(file, by);
    }
}

impl Shift for FnTypeName {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            params,
            ret,
            effects,
        } = self;
        params.shift(file, by);
        ret.shift(file, by);
        effects.shift(file, by);
    }
}

impl Shift for TypeParam {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            name: _,
            span,
            effect: _,
            requires,
        } = self;
        span.shift(file, by);
        requires.shift(file, by);
    }
}

impl Shift for Requirement {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self { name: _, span } = self;
        span.shift(file, by);
    }
}

impl Shift for Block {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            statements,
            tail,
            span,
        } = self;
        statements.shift(file, by);
        tail.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for Stmt {
    fn shift(&mut self, file: FileId, by: i64) {
        match self {
            Self::Let(l) => {
                let crate::ast::Let {
                    name: _,
                    name_span,
                    mutable: _,
                    ty,
                    value,
                    span,
                } = l;
                name_span.shift(file, by);
                if let Some(t) = ty {
                    t.shift(file, by);
                }
                value.shift(file, by);
                span.shift(file, by);
            }
            Self::Assign(x) => {
                let crate::ast::Assign { place, value, span } = x;
                place.shift(file, by);
                value.shift(file, by);
                span.shift(file, by);
            }
            Self::While(w) => {
                let crate::ast::While { cond, body, span } = w;
                cond.shift(file, by);
                body.shift(file, by);
                span.shift(file, by);
            }
            Self::For(f) => {
                let crate::ast::For {
                    name: _,
                    name_span,
                    iter,
                    body,
                    span,
                } = f;
                name_span.shift(file, by);
                match iter {
                    crate::ast::ForIter::Each(e) => e.shift(file, by),
                    crate::ast::ForIter::Range { start, end } => {
                        start.shift(file, by);
                        end.shift(file, by);
                    }
                }
                body.shift(file, by);
                span.shift(file, by);
            }
            Self::Return(r) => {
                let crate::ast::Return { value, span } = r;
                value.shift(file, by);
                span.shift(file, by);
            }
            Self::Break { span } | Self::Continue { span } => span.shift(file, by),
            Self::Expr(e) => e.shift(file, by),
            Self::Scope(sc) => {
                let crate::ast::Scope { body, span } = sc;
                body.shift(file, by);
                span.shift(file, by);
            }
            Self::Defer(df) => {
                let crate::ast::Defer {
                    keyword_span,
                    body,
                    span,
                } = df;
                keyword_span.shift(file, by);
                body.shift(file, by);
                span.shift(file, by);
            }
            Self::Spawn(sp) => {
                let crate::ast::Spawn {
                    callee: _,
                    callee_span,
                    args,
                    span,
                } = sp;
                callee_span.shift(file, by);
                args.shift(file, by);
                span.shift(file, by);
            }
        }
    }
}

impl Shift for Place {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            name: _,
            name_span,
            path,
            span,
        } = self;
        name_span.shift(file, by);
        for p in path {
            match p {
                crate::ast::PlaceStep::Field(f) => f.name_span.shift(file, by),
                crate::ast::PlaceStep::Index { index, span } => {
                    index.shift(file, by);
                    span.shift(file, by);
                }
            }
        }
        span.shift(file, by);
    }
}

impl Shift for MatchArm {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            pattern,
            body,
            span,
        } = self;
        pattern.shift(file, by);
        body.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for crate::ast::Pattern {
    fn shift(&mut self, file: FileId, by: i64) {
        match self {
            Self::Wildcard { span } | Self::Bind { span, .. } => span.shift(file, by),
            Self::Literal(e) => e.shift(file, by),
            Self::Variant(v) => {
                let crate::ast::VariantPattern {
                    qualifier,
                    ty: _,
                    ty_span,
                    variant: _,
                    variant_span,
                    fields,
                    span,
                } = v;
                qualifier.shift(file, by);
                ty_span.shift(file, by);
                variant_span.shift(file, by);
                fields.shift(file, by);
                span.shift(file, by);
            }
            Self::Or { alts, span } | Self::Tuple { elems: alts, span } => {
                alts.shift(file, by);
                span.shift(file, by);
            }
        }
    }
}

impl Shift for PatternField {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            name: _,
            name_span,
            pattern,
        } = self;
        name_span.shift(file, by);
        pattern.shift(file, by);
    }
}

impl Shift for FieldInit {
    fn shift(&mut self, file: FileId, by: i64) {
        let Self {
            name: _,
            name_span,
            value,
            span,
        } = self;
        name_span.shift(file, by);
        value.shift(file, by);
        span.shift(file, by);
    }
}

impl Shift for Expr {
    fn shift(&mut self, file: FileId, by: i64) {
        match self {
            Self::Interp { parts, span } => {
                for p in parts {
                    if let crate::ast::InterpPart::Hole { expr, span, .. } = p {
                        expr.shift(file, by);
                        span.shift(file, by);
                    }
                }
                span.shift(file, by);
            }
            Self::Array { elems, span } | Self::Tuple { elems, span } => {
                elems.shift(file, by);
                span.shift(file, by);
            }
            Self::ArrayRepeat { value, count, span } => {
                value.shift(file, by);
                count.shift(file, by);
                span.shift(file, by);
            }
            Self::Index {
                base,
                index,
                bracket_span,
                span,
            } => {
                base.shift(file, by);
                index.shift(file, by);
                bracket_span.shift(file, by);
                span.shift(file, by);
            }
            Self::Int { value: _, span }
            | Self::WideInt { value: _, span }
            | Self::Float { text: _, span }
            | Self::Bool { value: _, span }
            | Self::Str { value: _, span }
            | Self::Var { name: _, span } => span.shift(file, by),
            Self::Neg {
                operand,
                op_span,
                span,
            }
            | Self::Not {
                operand,
                op_span,
                span,
            }
            | Self::BitNot {
                operand,
                op_span,
                span,
            }
            | Self::Propagate {
                operand,
                op_span,
                span,
            } => {
                operand.shift(file, by);
                op_span.shift(file, by);
                span.shift(file, by);
            }
            Self::Closure {
                params,
                return_type,
                effects,
                body,
                fn_span,
                span,
            } => {
                params.shift(file, by);
                return_type.shift(file, by);
                effects.shift(file, by);
                body.shift(file, by);
                fn_span.shift(file, by);
                span.shift(file, by);
            }
            Self::Binary {
                op: _,
                lhs,
                rhs,
                op_span,
                span,
            } => {
                lhs.shift(file, by);
                rhs.shift(file, by);
                op_span.shift(file, by);
                span.shift(file, by);
            }
            Self::Call {
                qualifier,
                callee: _,
                callee_span,
                type_args,
                args,
                span,
            } => {
                qualifier.shift(file, by);
                callee_span.shift(file, by);
                type_args.shift(file, by);
                args.shift(file, by);
                span.shift(file, by);
            }
            Self::Construct {
                qualifier,
                ty: _,
                ty_span,
                type_args,
                fields,
                span,
            } => {
                qualifier.shift(file, by);
                ty_span.shift(file, by);
                type_args.shift(file, by);
                fields.shift(file, by);
                span.shift(file, by);
            }
            Self::Variant {
                qualifier,
                ty: _,
                ty_span,
                type_args,
                variant: _,
                variant_span,
                fields,
                span,
            } => {
                qualifier.shift(file, by);
                ty_span.shift(file, by);
                type_args.shift(file, by);
                variant_span.shift(file, by);
                fields.shift(file, by);
                span.shift(file, by);
            }
            Self::Match {
                scrutinee,
                arms,
                source: _,
                span,
            } => {
                scrutinee.shift(file, by);
                arms.shift(file, by);
                span.shift(file, by);
            }
            Self::Field {
                base,
                name: _,
                name_span,
                span,
            } => {
                base.shift(file, by);
                name_span.shift(file, by);
                span.shift(file, by);
            }
            Self::If {
                cond,
                then_block,
                else_block,
                span,
            } => {
                cond.shift(file, by);
                then_block.shift(file, by);
                else_block.shift(file, by);
                span.shift(file, by);
            }
            Self::Block(b) => b.shift(file, by),
            Self::MethodCall {
                qualifier,
                receiver,
                method: _,
                method_span,
                args,
                span,
            } => {
                qualifier.shift(file, by);
                receiver.shift(file, by);
                method_span.shift(file, by);
                args.shift(file, by);
                span.shift(file, by);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_no_op_edit_reuses_every_chunk() {
        let r = Revision::new(FileId::ROOT, "fn a() -> Int { 1 }\nfn b() -> Int { 2 }\n");
        let e = r
            .edit(&TextEdit {
                start: 0,
                end: 0,
                text: String::new(),
            })
            .unwrap();
        assert_eq!(e.text(), r.text());
        assert_eq!(e.program(), r.program());
    }

    #[test]
    fn a_range_outside_the_text_or_inside_a_character_is_refused() {
        let r = Revision::new(FileId::ROOT, "é");
        let bad = |start, end| TextEdit {
            start,
            end,
            text: String::new(),
        };
        assert_eq!(r.edit(&bad(0, 3)).err(), Some(EditError::OutOfRange));
        assert_eq!(r.edit(&bad(2, 1)).err(), Some(EditError::OutOfRange));
        assert_eq!(r.edit(&bad(1, 1)).err(), Some(EditError::NotACharBoundary));
    }
}
