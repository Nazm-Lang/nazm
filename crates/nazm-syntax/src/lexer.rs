//! Hand-written lexer.
//!
//! Handwritten rather than generated because of what comes later: incremental relexing of
//! a changed region, trivia attached to the right node, and errors that name the construct
//! rather than a token class. None of that is easier to retrofit than to start with.
//!
//! It does not stop at the first bad character. A file with three unknown characters
//! should report three, not one per compile — an agent that fixes one and recompiles to
//! find the next spends a turn per character.
//!
//! # One scan, and every byte in it
//!
//! Since N15 the scan is **lossless**: [`lex_lossless_in`] returns a [`Lexeme`] for every
//! byte of the input — a token, a run of whitespace, a comment, or a region that is not
//! valid Nazm — in source order, with no gap and no overlap. The token list the parser
//! reads and the comment list the formatter reads are both *derived* from that one stream
//! ([`lex_in`], [`lex_with_comments_in`]); nothing scans the text a second time, so the
//! two can never disagree about where a comment is.

use nazm_diag::{codes, Diagnostic};
use nazm_span::{FileId, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Literals and names
    Int(i64),
    /// A literal too large for `Int`, kept as digits.
    ///
    /// `i64::MIN` is written `-9223372036854775808`, and its magnitude is one greater
    /// than `i64::MAX` — so the lexer cannot produce it and the negation has to fold with
    /// it in the parser. Rejecting it here would make the smallest `Int` unwritable.
    IntTooLarge(String),
    Str(String),
    Ident(String),

    // Keywords
    Fn,
    Let,
    If,
    Else,
    True,
    False,
    Mut,
    /// `pub` — the one visibility keyword. See `docs/spec.md`.
    Pub,
    While,
    Break,
    Continue,
    Return,
    Use,
    Scope,
    Spawn,
    /// `struct` — the declaration keyword for a record. `docs/spec.md` explains why the
    /// keyword and the semantic category are spelled differently.
    Struct,
    /// `enum` — the declaration keyword for a closed sum type. Reserved by name since
    /// v0.1 and refused with *"there are no variant types yet"* until N10.
    Enum,
    /// `match` — the one elimination form for an enum. Reserved by name since v0.1 and
    /// refused with *"there is no pattern matching yet"* until N10.
    Match,

    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    /// `[` and `]` — a type parameter list, a type application, and explicit type
    /// arguments, and nothing else. N11. There is no indexing expression, which is what
    /// keeps `name[` unambiguous wherever it appears.
    LBracket,
    RBracket,
    Comma,
    Colon,
    /// `.` — field projection, and the separator between an enum and one of its
    /// variants. Never part of a number: `docs/spec.md` has no float.
    Dot,
    Semi,
    /// `?` — propagation, N12: postfix, after an operand whose type is the core prelude's
    /// `Result`. One character with one meaning; `??` is two of them, not a token of its
    /// own, so `r??` removes two `Result` layers exactly as `(r?)?` does.
    Question,
    /// `!` — before a function's effect set, N36: `-> Int ! {io}`. It has no other
    /// meaning, and `!=` is its own token, so neither can be read as the other.
    Bang,
    Arrow,
    /// `=>` — between a `match` pattern and the arm it selects.
    ///
    /// Its own token rather than `=` followed by `>`: written as two, `x = > 1` would be
    /// accepted, and a two-character operator is exactly how `->` and `==` already work.
    FatArrow,
    Eq,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,

    Eof,
}

impl TokenKind {
    /// Every token the language has, one value per variant, with a placeholder payload
    /// for the three that carry one.
    ///
    /// The single list. `docs/grammar.ebnf` is checked against it, so a keyword or
    /// operator added to the lexer and not to the grammar is a test failure rather than
    /// a grammar that quietly describes an older language. [`TokenKind::written`] is
    /// what keeps the list honest: it is an exhaustive `match`, so a new variant does
    /// not compile until someone has touched both.
    pub const ALL: &'static [Self] = &[
        Self::Int(0),
        Self::Str(String::new()),
        Self::Ident(String::new()),
        Self::Fn,
        Self::Let,
        Self::If,
        Self::Else,
        Self::True,
        Self::False,
        Self::Mut,
        Self::Pub,
        Self::While,
        Self::Break,
        Self::Continue,
        Self::Return,
        Self::Use,
        Self::Scope,
        Self::Spawn,
        Self::Struct,
        Self::Enum,
        Self::Match,
        Self::LParen,
        Self::RParen,
        Self::LBrace,
        Self::RBrace,
        Self::LBracket,
        Self::RBracket,
        Self::Comma,
        Self::Colon,
        Self::Dot,
        Self::Semi,
        Self::Question,
        Self::Bang,
        Self::Arrow,
        Self::FatArrow,
        Self::Eq,
        Self::Plus,
        Self::Minus,
        Self::Star,
        Self::Slash,
        Self::Percent,
        Self::EqEq,
        Self::NotEq,
        Self::Lt,
        Self::LtEq,
        Self::Gt,
        Self::GtEq,
    ];

    /// How this token is spelled in source, for the ones with a fixed spelling.
    ///
    /// `None` for the three that are a *class* rather than a word — an integer, a string
    /// literal, a name — and for the two that are never written: `IntTooLarge`, which is
    /// a diagnostic path, and `Eof`, which is a position.
    ///
    /// The `match` is exhaustive on purpose. Adding a token to the language stops this
    /// compiling, which is the only reliable way to make the grammar artefact keep up.
    #[must_use]
    pub const fn written(&self) -> Option<&'static str> {
        match self {
            Self::Int(_) | Self::IntTooLarge(_) | Self::Str(_) | Self::Ident(_) | Self::Eof => None,
            Self::Fn => Some("fn"),
            Self::Let => Some("let"),
            Self::If => Some("if"),
            Self::Else => Some("else"),
            Self::True => Some("true"),
            Self::False => Some("false"),
            Self::Mut => Some("mut"),
            Self::Pub => Some("pub"),
            Self::While => Some("while"),
            Self::Break => Some("break"),
            Self::Continue => Some("continue"),
            Self::Return => Some("return"),
            Self::Use => Some("use"),
            Self::Scope => Some("scope"),
            Self::Spawn => Some("spawn"),
            Self::Struct => Some("struct"),
            Self::Enum => Some("enum"),
            Self::Match => Some("match"),
            Self::LParen => Some("("),
            Self::RParen => Some(")"),
            Self::LBrace => Some("{"),
            Self::RBrace => Some("}"),
            Self::LBracket => Some("["),
            Self::RBracket => Some("]"),
            Self::Comma => Some(","),
            Self::Colon => Some(":"),
            Self::Dot => Some("."),
            Self::Semi => Some(";"),
            Self::Question => Some("?"),
            Self::Bang => Some("!"),
            Self::Arrow => Some("->"),
            Self::FatArrow => Some("=>"),
            Self::Eq => Some("="),
            Self::Plus => Some("+"),
            Self::Minus => Some("-"),
            Self::Star => Some("*"),
            Self::Slash => Some("/"),
            Self::Percent => Some("%"),
            Self::EqEq => Some("=="),
            Self::NotEq => Some("!="),
            Self::Lt => Some("<"),
            Self::LtEq => Some("<="),
            Self::Gt => Some(">"),
            Self::GtEq => Some(">="),
        }
    }

    /// How the token is written, for use in "expected X, found Y" messages.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Int(v) => format!("integer `{v}`"),
            Self::IntTooLarge(d) => format!("integer `{d}`"),
            Self::Str(_) => "string literal".to_owned(),
            Self::Ident(n) => format!("`{n}`"),
            Self::Fn => "`fn`".to_owned(),
            Self::Let => "`let`".to_owned(),
            Self::If => "`if`".to_owned(),
            Self::Else => "`else`".to_owned(),
            Self::True => "`true`".to_owned(),
            Self::False => "`false`".to_owned(),
            Self::Mut => "`mut`".to_owned(),
            Self::Pub => "`pub`".to_owned(),
            Self::While => "`while`".to_owned(),
            Self::Break => "`break`".to_owned(),
            Self::Continue => "`continue`".to_owned(),
            Self::Return => "`return`".to_owned(),
            Self::Use => "`use`".to_owned(),
            Self::Scope => "`scope`".to_owned(),
            Self::Spawn => "`spawn`".to_owned(),
            Self::Struct => "`struct`".to_owned(),
            Self::Enum => "`enum`".to_owned(),
            Self::Match => "`match`".to_owned(),
            Self::LParen => "`(`".to_owned(),
            Self::RParen => "`)`".to_owned(),
            Self::LBrace => "`{`".to_owned(),
            Self::RBrace => "`}`".to_owned(),
            Self::LBracket => "`[`".to_owned(),
            Self::RBracket => "`]`".to_owned(),
            Self::Comma => "`,`".to_owned(),
            Self::Colon => "`:`".to_owned(),
            Self::Dot => "`.`".to_owned(),
            Self::Semi => "`;`".to_owned(),
            Self::Question => "`?`".to_owned(),
            Self::Bang => "`!`".to_owned(),
            Self::Arrow => "`->`".to_owned(),
            Self::FatArrow => "`=>`".to_owned(),
            Self::Eq => "`=`".to_owned(),
            Self::Plus => "`+`".to_owned(),
            Self::Minus => "`-`".to_owned(),
            Self::Star => "`*`".to_owned(),
            Self::Slash => "`/`".to_owned(),
            Self::Percent => "`%`".to_owned(),
            Self::EqEq => "`==`".to_owned(),
            Self::NotEq => "`!=`".to_owned(),
            Self::Lt => "`<`".to_owned(),
            Self::LtEq => "`<=`".to_owned(),
            Self::Gt => "`>`".to_owned(),
            Self::GtEq => "`>=`".to_owned(),
            Self::Eof => "end of file".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Tokenise, collecting every error rather than stopping at the first.
///
/// For a single-file program. Multi-file compilation goes through
/// [`crate::parse_map`], which lexes each file against its own [`FileId`].
#[must_use]
pub fn lex(text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_in(FileId::ROOT, text)
}

/// Tokenise one file, so every span it produces names that file.
#[must_use]
pub fn lex_in(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let (lexemes, errors) = lex_lossless_in(file, text);
    (tokens(&lexemes), errors)
}

/// Tokenise, and also report where the comments were.
///
/// A comment is not a token and the parser must never see one, so [`lex`] drops them.
/// The formatter cannot: a formatter that deletes comments is not a formatter. Both are
/// views of the one lossless scan rather than the formatter having a second, nearly
/// identical scanner — the two would agree about everything except `"// not a comment"`
/// inside a string literal, which is exactly the case a second scanner gets wrong.
#[must_use]
pub fn lex_with_comments(text: &str) -> (Vec<Token>, Vec<Diagnostic>, Vec<Span>) {
    lex_with_comments_in(FileId::ROOT, text)
}

/// [`lex_with_comments`], for a named file.
#[must_use]
pub fn lex_with_comments_in(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>, Vec<Span>) {
    let (lexemes, errors) = lex_lossless_in(file, text);
    let comments = lexemes
        .iter()
        .filter_map(|l| match l {
            Lexeme::Comment(span) => Some(*span),
            _ => None,
        })
        .collect();
    (tokens(&lexemes), errors, comments)
}

/// The tokens of a lossless scan, in order, ending with [`TokenKind::Eof`].
#[must_use]
pub fn tokens(lexemes: &[Lexeme]) -> Vec<Token> {
    lexemes
        .iter()
        .filter_map(|l| match l {
            Lexeme::Token(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// One piece of a lossless scan: every byte of the input belongs to exactly one.
///
/// Only [`Lexeme::Token`] is seen by the parser's grammar. The other three are what the
/// abstract tree forgets and the concrete tree keeps. [`Lexeme::Invalid`] owns the bytes a
/// diagnostic was reported for — an unknown character, a string with no closing quote —
/// so the concrete tree still reproduces them however the diagnostic reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lexeme {
    Token(Token),
    /// A run of ASCII whitespace, newlines included.
    Whitespace(Span),
    /// `//` to the end of its line, the newline excluded.
    Comment(Span),
    /// Bytes that are not a token and not trivia, reported by the diagnostic beside them.
    Invalid(Span),
}

impl Lexeme {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Token(t) => t.span,
            Self::Whitespace(s) | Self::Comment(s) | Self::Invalid(s) => *s,
        }
    }
}

/// Scan one file into a [`Lexeme`] per region, in order, ending with a zero-width
/// [`TokenKind::Eof`] token. The spans cover `0..text.len()` exactly.
#[must_use]
pub fn lex_lossless_in(file: FileId, text: &str) -> (Vec<Lexeme>, Vec<Diagnostic>) {
    let bytes = text.as_bytes();
    // Checked access throughout. Every one of these reads is provably in bounds from the
    // surrounding loop condition, but "provably" is a property of today's code: the lint
    // is on for the whole workspace precisely so a later edit cannot quietly make one of
    // them a panic on malformed input.
    let byte = |i: usize| -> Option<u8> { bytes.get(i).copied() };
    let mut i = 0usize;
    let mut out: Vec<Lexeme> = Vec::new();
    let mut errors = Vec::new();

    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };

    while i < bytes.len() {
        let start = i;
        let Some(c) = byte(i) else { break };

        // Whitespace, one lexeme per run: the tree keeps the bytes, and nothing needs to
        // know where one space ends and the next begins.
        if c.is_ascii_whitespace() {
            while byte(i).is_some_and(|b| b.is_ascii_whitespace()) {
                i += 1;
            }
            out.push(Lexeme::Whitespace(Span::new(file, at(start), at(i))));
            continue;
        }

        // Line comments
        if c == b'/' && byte(i + 1) == Some(b'/') {
            while byte(i).is_some_and(|b| b != b'\n') {
                i += 1;
            }
            // Not including the newline: the comment is the text, and where the line
            // ends is the formatter's decision rather than the comment's.
            out.push(Lexeme::Comment(Span::new(file, at(start), at(i))));
            continue;
        }

        // Numbers
        if c.is_ascii_digit() {
            while byte(i).is_some_and(|b| b.is_ascii_digit() || b == b'_') {
                i += 1;
            }
            let raw: String = text
                .get(start..i)
                .unwrap_or_default()
                .chars()
                .filter(|c| *c != '_')
                .collect();
            match raw.parse::<i64>() {
                Ok(v) => out.push(Lexeme::Token(Token {
                    kind: TokenKind::Int(v),
                    span: Span::new(file, at(start), at(i)),
                })),
                // Refused at the boundary rather than silently wrapped. A literal that
                // does not fit the type it is written as is a mistake, not a value.
                // Not an error here: `-9223372036854775808` is a valid `Int` and its
                // magnitude does not fit. The parser folds the sign and reports if it
                // still does not.
                Err(_) => out.push(Lexeme::Token(Token {
                    kind: TokenKind::IntTooLarge(raw),
                    span: Span::new(file, at(start), at(i)),
                })),
            }
            continue;
        }

        // Identifiers and keywords
        if c.is_ascii_alphabetic() || c == b'_' {
            while byte(i).is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
                i += 1;
            }
            let word = text.get(start..i).unwrap_or_default();
            let kind = match word {
                "fn" => TokenKind::Fn,
                "let" => TokenKind::Let,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "mut" => TokenKind::Mut,
                "pub" => TokenKind::Pub,
                "while" => TokenKind::While,
                "break" => TokenKind::Break,
                "continue" => TokenKind::Continue,
                "return" => TokenKind::Return,
                "use" => TokenKind::Use,
                "struct" => TokenKind::Struct,
                "enum" => TokenKind::Enum,
                "match" => TokenKind::Match,
                "scope" => TokenKind::Scope,
                "spawn" => TokenKind::Spawn,
                _ => TokenKind::Ident(word.to_owned()),
            };
            out.push(Lexeme::Token(Token {
                kind,
                span: Span::new(file, at(start), at(i)),
            }));
            continue;
        }

        // Strings. No escapes in v0.1: an escape table is a decision about text encoding
        // that nothing here needs yet, and a half-done one is worse than none.
        if c == b'"' {
            i += 1;
            let content_start = i;
            while byte(i).is_some_and(|b| b != b'"' && b != b'\n') {
                i += 1;
            }
            if byte(i).is_none_or(|b| b == b'\n') {
                errors.push(
                    Diagnostic::error(
                        codes::UNTERMINATED_STRING,
                        Span::new(file, at(start), at(i)),
                        "this string has no closing quote",
                    )
                    .fix_with_review(
                        Span::new(file, at(i), at(i)),
                        "\"",
                        "close the string",
                        "the string was meant to end at the end of this line, rather \
                         than a quote being missing from somewhere earlier",
                    ),
                );
                // The quote and what followed it are still the author's bytes.
                out.push(Lexeme::Invalid(Span::new(file, at(start), at(i))));
                continue;
            }
            let value = text.get(content_start..i).unwrap_or("").to_owned();
            i += 1;
            out.push(Lexeme::Token(Token {
                kind: TokenKind::Str(value),
                span: Span::new(file, at(start), at(i)),
            }));
            continue;
        }

        // Two-character operators, checked before their one-character prefixes.
        let two = byte(i + 1).map(|n| (c, n));
        let (kind, width) = match two {
            Some((b'-', b'>')) => (Some(TokenKind::Arrow), 2),
            Some((b'=', b'=')) => (Some(TokenKind::EqEq), 2),
            Some((b'=', b'>')) => (Some(TokenKind::FatArrow), 2),
            Some((b'!', b'=')) => (Some(TokenKind::NotEq), 2),
            Some((b'<', b'=')) => (Some(TokenKind::LtEq), 2),
            Some((b'>', b'=')) => (Some(TokenKind::GtEq), 2),
            _ => (None, 0),
        };
        if let Some(kind) = kind {
            i += width;
            out.push(Lexeme::Token(Token {
                kind,
                span: Span::new(file, at(start), at(i)),
            }));
            continue;
        }

        let single = match c {
            b'(' => Some(TokenKind::LParen),
            b')' => Some(TokenKind::RParen),
            b'{' => Some(TokenKind::LBrace),
            b'}' => Some(TokenKind::RBrace),
            b'[' => Some(TokenKind::LBracket),
            b']' => Some(TokenKind::RBracket),
            b',' => Some(TokenKind::Comma),
            b':' => Some(TokenKind::Colon),
            b'.' => Some(TokenKind::Dot),
            b';' => Some(TokenKind::Semi),
            b'?' => Some(TokenKind::Question),
            b'!' => Some(TokenKind::Bang),
            b'=' => Some(TokenKind::Eq),
            b'+' => Some(TokenKind::Plus),
            b'-' => Some(TokenKind::Minus),
            b'*' => Some(TokenKind::Star),
            b'/' => Some(TokenKind::Slash),
            b'%' => Some(TokenKind::Percent),
            b'<' => Some(TokenKind::Lt),
            b'>' => Some(TokenKind::Gt),
            _ => None,
        };
        match single {
            Some(kind) => {
                i += 1;
                out.push(Lexeme::Token(Token {
                    kind,
                    span: Span::new(file, at(start), at(i)),
                }));
            }
            None => {
                // Advance by a whole character, not a byte: stepping into the middle of a
                // multi-byte sequence would produce one error per continuation byte.
                let ch = text
                    .get(start..)
                    .and_then(|t| t.chars().next())
                    .unwrap_or('\u{fffd}');
                i += ch.len_utf8();
                out.push(Lexeme::Invalid(Span::new(file, at(start), at(i))));
                errors.push(Diagnostic::error(
                    codes::UNKNOWN_CHARACTER,
                    Span::new(file, at(start), at(i)),
                    format!("`{ch}` is not valid here"),
                ));
            }
        }
    }

    out.push(Lexeme::Token(Token {
        kind: TokenKind::Eof,
        span: Span::new(file, at(bytes.len()), at(bytes.len())),
    }));
    (out, errors)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let (t, e) = lex(src);
        assert!(e.is_empty(), "unexpected errors: {e:?}");
        t.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn it_lexes_a_function() {
        let k = kinds("fn f(a: Int) -> Int { a }");
        assert_eq!(k.first(), Some(&TokenKind::Fn));
        assert!(k.contains(&TokenKind::Arrow));
        assert!(k.contains(&TokenKind::Ident("Int".to_owned())));
        assert_eq!(k.last(), Some(&TokenKind::Eof));
    }

    #[test]
    fn two_character_operators_win_over_their_prefixes() {
        // `<=` must not lex as `<` then `=`, and `->` must not lex as `-` then `>`.
        assert_eq!(
            kinds("<= >= == != ->"),
            vec![
                TokenKind::LtEq,
                TokenKind::GtEq,
                TokenKind::EqEq,
                TokenKind::NotEq,
                TokenKind::Arrow,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn a_bang_is_its_own_token_and_never_half_of_not_equal() {
        assert_eq!(
            kinds("! != !{"),
            vec![
                TokenKind::Bang,
                TokenKind::NotEq,
                TokenKind::Bang,
                TokenKind::LBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn comments_and_underscores_are_handled() {
        assert_eq!(
            kinds("// a comment\n1_000"),
            vec![TokenKind::Int(1000), TokenKind::Eof]
        );
    }

    #[test]
    fn a_literal_too_large_for_int_is_kept_for_the_parser_to_judge() {
        // Not an error here, and that is deliberate: `9223372036854775808` is out of
        // range on its own and exactly in range after a `-`. Only the parser can see the
        // sign, so the lexer hands over the digits rather than deciding without them.
        let (t, e) = lex("9223372036854775808");
        assert!(e.is_empty(), "the lexer should not judge this: {e:?}");
        assert_eq!(
            t[0].kind,
            TokenKind::IntTooLarge("9223372036854775808".to_string())
        );
    }

    #[test]
    fn underscores_are_stripped_before_the_parser_sees_the_digits() {
        // `-9_223_372_036_854_775_808` has to fold into `i64::MIN`, which it cannot do if
        // the separators are still in the string the parser parses.
        let (t, _) = lex("9_223_372_036_854_775_808");
        assert_eq!(
            t[0].kind,
            TokenKind::IntTooLarge("9223372036854775808".to_string())
        );
    }

    #[test]
    fn every_bad_character_is_reported_not_just_the_first() {
        // One error per recompile would cost an agent a turn per character.
        let (_, e) = lex("a @ b # c $");
        assert_eq!(e.len(), 3, "expected three, got {e:?}");
        assert!(e.iter().all(|d| d.code == codes::UNKNOWN_CHARACTER));
    }

    #[test]
    fn a_multibyte_character_produces_one_error_not_several() {
        // Advancing by byte would report once per continuation byte.
        let (_, e) = lex("let x = é;");
        assert_eq!(e.len(), 1, "got {e:?}");
    }

    #[test]
    fn an_unterminated_string_reports_where_it_started() {
        let (_, e) = lex("let s = \"open;");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].code, codes::UNTERMINATED_STRING);
        assert!(!e[0].fixes.is_empty(), "should suggest closing it");
    }
}
