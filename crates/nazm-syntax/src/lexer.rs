//! Hand-written lexer.
//!
//! Handwritten rather than generated because of what comes later: incremental relexing of
//! a changed region, trivia attached to the right node, and errors that name the construct
//! rather than a token class. None of that is easier to retrofit than to start with.
//!
//! It does not stop at the first bad character. A file with three unknown characters
//! should report three, not one per compile — an agent that fixes one and recompiles to
//! find the next spends a turn per character.

use nazm_diag::{codes, Diagnostic};
use nazm_span::Span;

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
    While,
    Break,
    Continue,
    Return,

    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semi,
    Arrow,
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
            Self::While => "`while`".to_owned(),
            Self::Break => "`break`".to_owned(),
            Self::Continue => "`continue`".to_owned(),
            Self::Return => "`return`".to_owned(),
            Self::LParen => "`(`".to_owned(),
            Self::RParen => "`)`".to_owned(),
            Self::LBrace => "`{`".to_owned(),
            Self::RBrace => "`}`".to_owned(),
            Self::Comma => "`,`".to_owned(),
            Self::Colon => "`:`".to_owned(),
            Self::Semi => "`;`".to_owned(),
            Self::Arrow => "`->`".to_owned(),
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
#[must_use]
pub fn lex(text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let bytes = text.as_bytes();
    // Checked access throughout. Every one of these reads is provably in bounds from the
    // surrounding loop condition, but "provably" is a property of today's code: the lint
    // is on for the whole workspace precisely so a later edit cannot quietly make one of
    // them a panic on malformed input.
    let byte = |i: usize| -> Option<u8> { bytes.get(i).copied() };
    let mut i = 0usize;
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };

    while i < bytes.len() {
        let start = i;
        let Some(c) = byte(i) else { break };

        // Whitespace
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comments
        if c == b'/' && byte(i + 1) == Some(b'/') {
            while byte(i).is_some_and(|b| b != b'\n') {
                i += 1;
            }
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
                Ok(v) => tokens.push(Token {
                    kind: TokenKind::Int(v),
                    span: Span::new(at(start), at(i)),
                }),
                // Refused at the boundary rather than silently wrapped. A literal that
                // does not fit the type it is written as is a mistake, not a value.
                // Not an error here: `-9223372036854775808` is a valid `Int` and its
                // magnitude does not fit. The parser folds the sign and reports if it
                // still does not.
                Err(_) => tokens.push(Token {
                    kind: TokenKind::IntTooLarge(raw),
                    span: Span::new(at(start), at(i)),
                }),
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
                "while" => TokenKind::While,
                "break" => TokenKind::Break,
                "continue" => TokenKind::Continue,
                "return" => TokenKind::Return,
                _ => TokenKind::Ident(word.to_owned()),
            };
            tokens.push(Token {
                kind,
                span: Span::new(at(start), at(i)),
            });
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
                        Span::new(at(start), at(i)),
                        "this string has no closing quote",
                    )
                    .fix(Span::new(at(i), at(i)), "\"", "close the string"),
                );
                continue;
            }
            let value = text.get(content_start..i).unwrap_or("").to_owned();
            i += 1;
            tokens.push(Token {
                kind: TokenKind::Str(value),
                span: Span::new(at(start), at(i)),
            });
            continue;
        }

        // Two-character operators, checked before their one-character prefixes.
        let two = byte(i + 1).map(|n| (c, n));
        let (kind, width) = match two {
            Some((b'-', b'>')) => (Some(TokenKind::Arrow), 2),
            Some((b'=', b'=')) => (Some(TokenKind::EqEq), 2),
            Some((b'!', b'=')) => (Some(TokenKind::NotEq), 2),
            Some((b'<', b'=')) => (Some(TokenKind::LtEq), 2),
            Some((b'>', b'=')) => (Some(TokenKind::GtEq), 2),
            _ => (None, 0),
        };
        if let Some(kind) = kind {
            i += width;
            tokens.push(Token {
                kind,
                span: Span::new(at(start), at(i)),
            });
            continue;
        }

        let single = match c {
            b'(' => Some(TokenKind::LParen),
            b')' => Some(TokenKind::RParen),
            b'{' => Some(TokenKind::LBrace),
            b'}' => Some(TokenKind::RBrace),
            b',' => Some(TokenKind::Comma),
            b':' => Some(TokenKind::Colon),
            b';' => Some(TokenKind::Semi),
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
                tokens.push(Token {
                    kind,
                    span: Span::new(at(start), at(i)),
                });
            }
            None => {
                // Advance by a whole character, not a byte: stepping into the middle of a
                // multi-byte sequence would produce one error per continuation byte.
                let ch = text
                    .get(start..)
                    .and_then(|t| t.chars().next())
                    .unwrap_or('\u{fffd}');
                i += ch.len_utf8();
                errors.push(Diagnostic::error(
                    codes::UNKNOWN_CHARACTER,
                    Span::new(at(start), at(i)),
                    format!("`{ch}` is not valid here"),
                ));
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(at(bytes.len()), at(bytes.len())),
    });
    (tokens, errors)
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
