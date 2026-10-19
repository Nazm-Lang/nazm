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
    /// A floating-point literal (Gate 2): digits, a `.` and digits, and/or an exponent, kept as
    /// written with any `_` removed. Its value — `Float64` or `Float32` — is decided where its
    /// type is, so the lexer does not round it. `docs/internal/general-purpose.md` §1.
    Float(String),
    Str(String),
    /// An interpolated string (R1a, L4): its text and holes, in order.
    FStr(Vec<FPiece>),
    Ident(String),

    // Keywords
    Fn,
    Let,
    If,
    Else,
    True,
    False,
    Mut,
    /// `pub` — the one visibility keyword. See `docs/user/spec.md`.
    Pub,
    While,
    Break,
    Continue,
    Return,
    Use,
    Scope,
    Spawn,
    /// `struct` — the declaration keyword for a record. `docs/user/spec.md` explains why the
    /// keyword and the semantic category are spelled differently.
    Struct,
    /// `enum` — the declaration keyword for a closed sum type. Reserved by name since
    /// v0.1 and refused with *"there are no variant types yet"* until N10.
    Enum,
    /// `match` — the one elimination form for an enum. Reserved by name since v0.1 and
    /// refused with *"there is no pattern matching yet"* until N10.
    Match,
    /// `trait` — a trait declaration, N78.
    Trait,
    /// `impl` — an implementation of a trait for a type, N78. Reserved by name since v0.1
    /// and refused with *"there are no methods yet"* until then.
    Impl,

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
    /// variants. Never part of a number: `docs/user/spec.md` has no float.
    Dot,
    /// `..` — only between a `for` loop's bounds, `for i in a..b` (R1a). Before R1a two dots
    /// were two `Dot`s and always an error, so the token changes no valid program.
    DotDot,
    Semi,
    /// `?` — propagation, N12: postfix, after an operand whose type is the core prelude's
    /// `Result`. One character with one meaning; `??` is two of them, not a token of its
    /// own, so `r??` removes two `Result` layers exactly as `(r?)?` does.
    Question,
    /// `!` — before a function's effect set, N36: `-> Int ! {io}`; and, since N49, prefix
    /// logical negation in an expression, `!done`. The two never meet: an effect set follows
    /// a result type, and an expression never does. `!=` is its own token.
    Bang,
    /// `&&` — short-circuit conjunction, N49.
    AmpAmp,
    /// `::` — between a module alias and the item it names, N49: `t::text_split`.
    ColonColon,
    /// `||` — short-circuit disjunction, N49.
    PipePipe,
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
    /// `&`, `|`, `^`, `~`, `<<` and `>>` — the bit operators (Gate 2), on integers only.
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    /// `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=` — compound assignment (R1,
    /// 5b.12): each one token, so `x + = 1` is not one and a formatter cannot split one.
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    /// `@` — the start of a declaration's attribute (Gate 2, `docs/internal/general-purpose.md` §16).
    At,

    Eof,
}

impl TokenKind {
    /// Every token the language has, one value per variant, with a placeholder payload
    /// for the three that carry one.
    ///
    /// The single list. `docs/user/grammar.ebnf` is checked against it, so a keyword or
    /// operator added to the lexer and not to the grammar is a test failure rather than
    /// a grammar that quietly describes an older language. [`TokenKind::written`] is
    /// what keeps the list honest: it is an exhaustive `match`, so a new variant does
    /// not compile until someone has touched both.
    pub const ALL: &'static [Self] = &[
        Self::Int(0),
        Self::Float(String::new()),
        Self::Str(String::new()),
        Self::FStr(Vec::new()),
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
        Self::Trait,
        Self::Impl,
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
        Self::DotDot,
        Self::Semi,
        Self::Question,
        Self::Bang,
        Self::AmpAmp,
        Self::PipePipe,
        Self::ColonColon,
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
        Self::Amp,
        Self::Pipe,
        Self::Caret,
        Self::Tilde,
        Self::Shl,
        Self::Shr,
        Self::PlusEq,
        Self::MinusEq,
        Self::StarEq,
        Self::SlashEq,
        Self::PercentEq,
        Self::AmpEq,
        Self::PipeEq,
        Self::CaretEq,
        Self::ShlEq,
        Self::ShrEq,
        Self::At,
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
            Self::Int(_)
            | Self::IntTooLarge(_)
            | Self::Float(_)
            | Self::Str(_)
            | Self::FStr(_)
            | Self::Ident(_)
            | Self::Eof => None,
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
            Self::Trait => Some("trait"),
            Self::Impl => Some("impl"),
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
            Self::DotDot => Some(".."),
            Self::Semi => Some(";"),
            Self::Question => Some("?"),
            Self::Bang => Some("!"),
            Self::AmpAmp => Some("&&"),
            Self::ColonColon => Some("::"),
            Self::PipePipe => Some("||"),
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
            Self::Amp => Some("&"),
            Self::Pipe => Some("|"),
            Self::Caret => Some("^"),
            Self::Tilde => Some("~"),
            Self::Shl => Some("<<"),
            Self::Shr => Some(">>"),
            Self::PlusEq => Some("+="),
            Self::MinusEq => Some("-="),
            Self::StarEq => Some("*="),
            Self::SlashEq => Some("/="),
            Self::PercentEq => Some("%="),
            Self::AmpEq => Some("&="),
            Self::PipeEq => Some("|="),
            Self::CaretEq => Some("^="),
            Self::ShlEq => Some("<<="),
            Self::ShrEq => Some(">>="),
            Self::At => Some("@"),
        }
    }

    /// How the token is written, for use in "expected X, found Y" messages.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Int(v) => format!("integer `{v}`"),
            Self::IntTooLarge(d) => format!("integer `{d}`"),
            Self::Float(d) => format!("number `{d}`"),
            Self::Str(_) => "string literal".to_owned(),
            Self::FStr(_) => "interpolated string".to_owned(),
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
            Self::Trait => "`trait`".to_owned(),
            Self::Impl => "`impl`".to_owned(),
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
            Self::DotDot => "`..`".to_owned(),
            Self::Semi => "`;`".to_owned(),
            Self::Question => "`?`".to_owned(),
            Self::Bang => "`!`".to_owned(),
            Self::AmpAmp => "`&&`".to_owned(),
            Self::ColonColon => "`::`".to_owned(),
            Self::PipePipe => "`||`".to_owned(),
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
            Self::Amp => "`&`".to_owned(),
            Self::Pipe => "`|`".to_owned(),
            Self::Caret => "`^`".to_owned(),
            Self::Tilde => "`~`".to_owned(),
            Self::Shl => "`<<`".to_owned(),
            Self::Shr => "`>>`".to_owned(),
            Self::PlusEq => "`+=`".to_owned(),
            Self::MinusEq => "`-=`".to_owned(),
            Self::StarEq => "`*=`".to_owned(),
            Self::SlashEq => "`/=`".to_owned(),
            Self::PercentEq => "`%=`".to_owned(),
            Self::AmpEq => "`&=`".to_owned(),
            Self::PipeEq => "`|=`".to_owned(),
            Self::CaretEq => "`^=`".to_owned(),
            Self::ShlEq => "`<<=`".to_owned(),
            Self::ShrEq => "`>>=`".to_owned(),
            Self::At => "`@`".to_owned(),
            Self::Eof => "end of file".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// A literal's bytes with its escapes decoded (`docs/user/spec.md`, *String escapes*, N49).
///
/// `span(a, b)` is the source span of `raw[a..b]`, so a refused escape is reported at the bytes
/// written. A refused escape contributes nothing to the value and the rest of the literal is
/// still decoded, so one bad escape is one diagnostic.
/// `\u{H…}` starting at `bytes[k]` (the backslash): the scalar value and the escape's width,
/// or the width of what is refused — through the `}` when there is one, else as far as the
/// hexadecimal digits go.
fn unicode_escape(bytes: &[u8], k: usize) -> Result<(char, usize), usize> {
    if bytes.get(k + 2) != Some(&b'{') {
        return Err(2);
    }
    let digits = bytes
        .get(k + 3..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| b.is_ascii_hexdigit())
        .count();
    let close = k + 3 + digits;
    if bytes.get(close) != Some(&b'}') {
        return Err(3 + digits);
    }
    let width = 4 + digits;
    if digits == 0 || digits > 6 {
        return Err(width);
    }
    let value = bytes
        .get(k + 3..close)
        .unwrap_or_default()
        .iter()
        .try_fold(0u32, |v, b| char::from(*b).to_digit(16).map(|d| v * 16 + d));
    // `char::from_u32` refuses exactly the surrogates and everything above `10FFFF`.
    value
        .and_then(char::from_u32)
        .map(|c| (c, width))
        .ok_or(width)
}

/// A raw string starting at `start` (R1a, L9): `r`, some `#`s, a quote, the text as written —
/// no escape, any line break — and a quote followed by as many `#`s. A line break written as
/// CR LF is one LF, as in every string. Returns the lexeme and the offset past it.
fn raw_string(
    file: FileId,
    text: &str,
    start: usize,
    errors: &mut Vec<Diagnostic>,
) -> (Lexeme, usize) {
    let bytes = text.as_bytes();
    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };
    let hashes = bytes
        .get(start + 1..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| **b == b'#')
        .count();
    let open = start + 1 + hashes;
    if bytes.get(open) != Some(&b'"') {
        errors.push(
            Diagnostic::error(
                codes::UNTERMINATED_STRING,
                Span::new(file, at(start), at(open)),
                "a raw string's `#`s are followed by its opening quote",
            )
            .help("a raw string is `r\"…\"` or `r#\"…\"#`, with as many `#`s at each end"),
        );
        return (Lexeme::Invalid(Span::new(file, at(start), at(open))), open);
    }
    let closing: Vec<u8> = std::iter::once(b'"')
        .chain(std::iter::repeat_n(b'#', hashes))
        .collect();
    let body = open + 1;
    let found = bytes
        .get(body..)
        .unwrap_or_default()
        .windows(closing.len())
        .position(|w| w == closing.as_slice());
    let Some(len) = found else {
        errors.push(Diagnostic::error(
            codes::UNTERMINATED_STRING,
            Span::new(file, at(start), at(text.len())),
            "this raw string has no closing quote",
        ));
        return (
            Lexeme::Invalid(Span::new(file, at(start), at(text.len()))),
            text.len(),
        );
    };
    let value = text
        .get(body..body + len)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    let end = body + len + closing.len();
    (
        Lexeme::Token(Token {
            kind: TokenKind::Str(value),
            span: Span::new(file, at(start), at(end)),
        }),
        end,
    )
}

/// A multi-line string starting at `start` (R1a, L9): `"""` and a line break; lines; and a
/// line holding only indentation and `"""`. That indentation is taken off every line, each of
/// which must begin with it (`N0005`) unless it is blank; the line breaks between the lines
/// are the value's, the first and the last not. Escapes are an ordinary string's, and a
/// backslash ending a line joins it to the next.
fn multi_line_string(
    file: FileId,
    text: &str,
    start: usize,
    errors: &mut Vec<Diagnostic>,
) -> (Lexeme, usize) {
    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };
    let (lines, end) = match multi_line_layout(file, text, start, 3, errors) {
        Ok(laid) => laid,
        Err(stop) => return stop,
    };
    let mut body = String::new();
    for (n, (_, kept)) in lines.iter().enumerate() {
        if n > 0 {
            body.push('\n');
        }
        body.push_str(kept);
    }
    // Escapes over the whole text; a span inside it is reported at the literal's start.
    let value = decode_escapes(
        &body,
        |_, _| Span::new(file, at(start), at(start + 3)),
        errors,
    );
    (
        Lexeme::Token(Token {
            kind: TokenKind::Str(value),
            span: Span::new(file, at(start), at(end)),
        }),
        end,
    )
}

/// A multi-line literal's content lines: each line's byte offset and its text.
type Lines<'t> = Vec<(usize, &'t str)>;

/// A multi-line literal's lines (L9), its opening delimiter `open` bytes long at `start`: each
/// line's kept text — its indentation taken off — with the offset that text starts at, and the
/// offset past the closing `"""`. `Err` holds the lexeme to return when there is no literal to
/// read: no line break after the delimiter, or no closing one.
fn multi_line_layout<'t>(
    file: FileId,
    text: &'t str,
    start: usize,
    open: usize,
    errors: &mut Vec<Diagnostic>,
) -> Result<(Lines<'t>, usize), (Lexeme, usize)> {
    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };
    let after = start + open;
    let rest = text.get(after..).unwrap_or_default();
    let line_end = rest.find('\n').map(|n| after + n);
    // Nothing but blanks after the opening delimiter.
    let opening_tail = text
        .get(after..line_end.unwrap_or(text.len()))
        .unwrap_or_default();
    if line_end.is_none() || !opening_tail.trim_end_matches('\r').trim().is_empty() {
        let stop = line_end.unwrap_or(text.len());
        errors.push(
            Diagnostic::error(
                codes::STRING_LAYOUT,
                Span::new(file, at(after), at(stop)),
                "a multi-line string's text begins on the line after its `\"\"\"`",
            )
            .help("write `\"\"\"`, a line break, the lines, and `\"\"\"` on a line of its own"),
        );
        // Read on as if the line broke after the delimiter, so its closing `"""` is not
        // taken for a second opening one: one mistake, one error.
        if line_end.is_none() {
            return Err((Lexeme::Invalid(Span::new(file, at(start), at(stop))), stop));
        }
    }
    let first = line_end.unwrap_or(text.len()) + 1;
    // The lines, each with its offset, up to the one that is only indentation and `"""`.
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut pos = first;
    let mut close: Option<(usize, &str)> = None;
    while pos <= text.len() {
        let end = text
            .get(pos..)
            .and_then(|r| r.find('\n'))
            .map_or(text.len(), |n| pos + n);
        let line = text.get(pos..end).unwrap_or_default();
        let line = line.strip_suffix('\r').unwrap_or(line);
        let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
        if line
            .get(indent_len..)
            .is_some_and(|r| r.starts_with("\"\"\""))
        {
            close = Some((pos, line.get(..indent_len).unwrap_or_default()));
            break;
        }
        lines.push((pos, line));
        if end >= text.len() {
            break;
        }
        pos = end + 1;
    }
    let Some((close_at, indent)) = close else {
        errors.push(Diagnostic::error(
            codes::UNTERMINATED_STRING,
            Span::new(file, at(start), at(text.len())),
            "this multi-line string has no closing `\"\"\"`",
        ));
        return Err((
            Lexeme::Invalid(Span::new(file, at(start), at(text.len()))),
            text.len(),
        ));
    };
    let end = close_at + indent.len() + 3;
    let mut kept_lines = Vec::with_capacity(lines.len());
    for (offset, line) in lines {
        if line.trim().is_empty() {
            kept_lines.push((offset, ""));
            continue;
        }
        match line.strip_prefix(indent) {
            Some(kept) => kept_lines.push((offset + indent.len(), kept)),
            None => {
                errors.push(
                    Diagnostic::error(
                        codes::STRING_LAYOUT,
                        Span::new(file, at(offset), at(offset + line.len())),
                        "this line of a multi-line string does not begin with its closing \
                         `\"\"\"`'s indentation",
                    )
                    .help(
                        "every line is indented at least as far as the closing `\"\"\"`, \
                         which is taken off each; indent this line, or the delimiter less",
                    ),
                );
                let kept = line.trim_start_matches([' ', '\t']);
                kept_lines.push((offset + line.len() - kept.len(), kept));
            }
        }
    }
    Ok((kept_lines, end))
}

/// Where an interpolation's text is laid out (R1a, L4): `"…"` on one line, or the lines of a
/// `"""…"""`.
enum FLayout<'t> {
    Line(usize, &'t str),
    Lines(Vec<(usize, &'t str)>),
}

/// An interpolated string starting at `start` (R1a, L4): `f"…"`, or `f"""` laid out as a
/// multi-line string is. Text between holes is an ordinary literal's; `{{` and `}}` are one
/// brace each; `{expr}` or `{expr:spec}` is a hole, which stays on one line.
fn interpolated(
    file: FileId,
    text: &str,
    start: usize,
    errors: &mut Vec<Diagnostic>,
) -> (Lexeme, usize) {
    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };
    let bytes = text.as_bytes();
    let multi = bytes.get(start + 1..start + 4) == Some(b"\"\"\"".as_slice());
    let (layout, end) = if multi {
        match multi_line_layout(file, text, start, 4, errors) {
            Ok((lines, end)) => (FLayout::Lines(lines), end),
            Err(stop) => return stop,
        }
    } else {
        // One line, to the closing quote: a quote inside a hole is the hole's.
        let content = start + 2;
        let mut i = content;
        loop {
            match bytes.get(i) {
                None | Some(b'\n') => {
                    errors.push(Diagnostic::error(
                        codes::UNTERMINATED_STRING,
                        Span::new(file, at(start), at(i)),
                        "this string has no closing quote",
                    ));
                    return (Lexeme::Invalid(Span::new(file, at(start), at(i))), i);
                }
                Some(b'\\') => i += 2,
                Some(b'{') if bytes.get(i + 1) == Some(&b'{') => i += 2,
                Some(b'{') => match hole_end(bytes, i + 1) {
                    Some(close) => i = close + 1,
                    None => i += 1,
                },
                Some(b'"') => break,
                Some(_) => i += 1,
            }
        }
        let line = text.get(content..i).unwrap_or_default();
        (FLayout::Line(content, line), i + 1)
    };
    let lines = match layout {
        FLayout::Line(offset, line) => vec![(offset, line)],
        FLayout::Lines(lines) => lines,
    };
    let mut pieces: Vec<FPiece> = Vec::new();
    let mut raw = String::new();
    let mut raw_at = lines.first().map_or(start, |(o, _)| *o);
    for (n, (offset, line)) in lines.iter().enumerate() {
        if n > 0 {
            raw.push('\n');
        }
        split_pieces(
            file,
            line,
            *offset,
            &mut raw,
            &mut raw_at,
            &mut pieces,
            errors,
        );
    }
    flush_text(file, &mut raw, raw_at, &mut pieces, errors);
    (
        Lexeme::Token(Token {
            kind: TokenKind::FStr(pieces),
            span: Span::new(file, at(start), at(end)),
        }),
        end,
    )
}

/// One line of an interpolation's text (L4), its kept text `line` starting at `offset`: text
/// gathered into `raw`, each hole a piece of its own after the text before it.
fn split_pieces(
    file: FileId,
    line: &str,
    offset: usize,
    raw: &mut String,
    raw_at: &mut usize,
    pieces: &mut Vec<FPiece>,
    errors: &mut Vec<Diagnostic>,
) {
    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };
    let bytes = line.as_bytes();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'\\' => {
                raw.push_str(line.get(i..(i + 2).min(line.len())).unwrap_or_default());
                i += 2;
            }
            b'{' if bytes.get(i + 1) == Some(&b'{') => {
                raw.push('{');
                i += 2;
            }
            b'}' if bytes.get(i + 1) == Some(&b'}') => {
                raw.push('}');
                i += 2;
            }
            b'}' => {
                errors.push(
                    Diagnostic::error(
                        codes::INTERPOLATION,
                        Span::new(file, at(offset + i), at(offset + i + 1)),
                        "a `}` in an interpolation's text closes nothing",
                    )
                    .help("write `}}` for a brace in the text"),
                );
                i += 1;
            }
            b'{' => {
                let Some(close) = hole_end(bytes, i + 1) else {
                    errors.push(
                        Diagnostic::error(
                            codes::INTERPOLATION,
                            Span::new(file, at(offset + i), at(offset + line.len())),
                            "this hole has no closing `}` on its line",
                        )
                        .help("a hole is `{expr}` on one line; write `{{` for a brace in the text"),
                    );
                    return;
                };
                flush_text(file, raw, *raw_at, pieces, errors);
                let inside = line.get(i + 1..close).unwrap_or_default();
                let span = Span::new(file, at(offset + i), at(offset + close + 1));
                match hole(inside) {
                    Some((expr, spec)) if !expr.trim().is_empty() => pieces.push(FPiece::Hole {
                        text: expr.to_owned(),
                        at: at(offset + i + 1),
                        spec,
                        span,
                    }),
                    Some(_) => errors.push(Diagnostic::error(
                        codes::INTERPOLATION,
                        span,
                        "this hole holds no expression",
                    )),
                    None => errors.push(
                        Diagnostic::error(
                            codes::INTERPOLATION,
                            span,
                            "this hole's format is not one interpolation has",
                        )
                        .help("a format is `[<|>][width][.precision]`: `{x:>8}`, `{ratio:.2}`"),
                    ),
                }
                i = close + 1;
                *raw_at = offset + i;
            }
            _ => {
                let len = utf8_len(b);
                raw.push_str(line.get(i..i + len).unwrap_or_default());
                i += len;
            }
        }
    }
}

/// The bytes a UTF-8 sequence starting with `b` takes, so text is copied whole characters at a
/// time.
fn utf8_len(b: u8) -> usize {
    match b {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

/// The text gathered so far, its escapes decoded, as one piece (L4).
fn flush_text(
    file: FileId,
    raw: &mut String,
    raw_at: usize,
    pieces: &mut Vec<FPiece>,
    errors: &mut Vec<Diagnostic>,
) {
    if raw.is_empty() {
        return;
    }
    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };
    let value = decode_escapes(
        raw,
        |a, b| Span::new(file, at(raw_at + a), at(raw_at + b)),
        errors,
    );
    pieces.push(FPiece::Text(value));
    raw.clear();
}

/// The offset of the `}` closing a hole whose text starts at `from` (L4): braces balanced, and a
/// string inside — ordinary or raw — skipped whole; `None` at a line break or the end.
fn hole_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = from;
    loop {
        match *bytes.get(i)? {
            b'\n' => return None,
            b'{' => depth += 1,
            b'}' if depth == 0 => return Some(i),
            b'}' => depth -= 1,
            b'"' => {
                i += 1;
                while *bytes.get(i)? != b'"' {
                    if *bytes.get(i)? == b'\n' {
                        return None;
                    }
                    if bytes.get(i) == Some(&b'\\') {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// A hole's text split into its expression and its format (L4): the last `:` outside brackets
/// and strings and not half of a `::` begins the format, when what follows it is one; `None`
/// when it is not.
fn hole(inside: &str) -> Option<(&str, FormatSpec)> {
    let bytes = inside.as_bytes();
    let mut depth = 0i32;
    let mut colon: Option<usize> = None;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'"' => {
                i += 1;
                while bytes.get(i).is_some_and(|c| *c != b'"') {
                    if bytes.get(i) == Some(&b'\\') {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b':' if depth == 0
                && bytes.get(i + 1) != Some(&b':')
                && (i == 0 || bytes.get(i - 1) != Some(&b':')) =>
            {
                colon = Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    let Some(c) = colon else {
        return Some((inside, FormatSpec::default()));
    };
    let spec = FormatSpec::parse(inside.get(c + 1..).unwrap_or_default())?;
    Some((inside.get(..c).unwrap_or_default(), spec))
}

/// A piece of an interpolated string (R1a, L4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FPiece {
    /// Text, its escapes decoded and each `{{` or `}}` one brace.
    Text(String),
    /// A hole: its expression's source, the offset that source starts at in the file, its
    /// format, and the span of the braces around it.
    Hole {
        text: String,
        at: u32,
        spec: FormatSpec,
        span: Span,
    },
}

/// A hole's format (L4): `[<|>][width][.precision]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormatSpec {
    /// `<` or `>`, where one is written.
    pub align: Option<Align>,
    /// The fewest bytes the hole writes.
    pub width: Option<u32>,
    /// A float's decimals.
    pub precision: Option<u32>,
}

/// Which side of a hole's text its padding goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// `<`: the text first, the padding after it.
    Left,
    /// `>`: the padding first.
    Right,
}

impl FormatSpec {
    /// A format as written after a hole's `:`, or `None` when it is not one: an optional `<` or
    /// `>`, optional width digits, and an optional `.` and precision digits, not all absent.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut rest = text;
        let align = match rest.as_bytes().first() {
            Some(b'<') => Some(Align::Left),
            Some(b'>') => Some(Align::Right),
            _ => None,
        };
        if align.is_some() {
            rest = rest.get(1..)?;
        }
        let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
        let w = digits(rest);
        let width = if w > 0 {
            Some(rest.get(..w)?.parse().ok()?)
        } else {
            None
        };
        rest = rest.get(w..)?;
        let precision = if let Some(p) = rest.strip_prefix('.') {
            let n = digits(p);
            if n == 0 || n != p.len() {
                return None;
            }
            rest = "";
            Some(p.parse().ok()?)
        } else {
            None
        };
        let spec = Self {
            align,
            width,
            precision,
        };
        (rest.is_empty() && spec != Self::default()).then_some(spec)
    }
}

fn decode_escapes(
    raw: &str,
    span: impl Fn(usize, usize) -> Span,
    errors: &mut Vec<Diagnostic>,
) -> String {
    if !raw.contains('\\') {
        return raw.to_owned();
    }
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut k = 0;
    while let Some(&b) = bytes.get(k) {
        if b != b'\\' {
            out.push(b);
            k += 1;
            continue;
        }
        let one = |v: u8| Some((v, 2usize));
        // A backslash ending a line of a multi-line string (L9) joins it to the next: the
        // line break is not in the value, and nor is the next line's indentation, already
        // taken off.
        if bytes.get(k + 1) == Some(&b'\n') {
            k += 2;
            continue;
        }
        let decoded = match bytes.get(k + 1) {
            Some(b'n') => one(b'\n'),
            Some(b't') => one(b'\t'),
            Some(b'r') => one(b'\r'),
            Some(b'0') => one(0),
            Some(b'\\') => one(b'\\'),
            Some(b'"') => one(b'"'),
            Some(b'x') => {
                let hex = |o: usize| bytes.get(k + o).and_then(|h| char::from(*h).to_digit(16));
                match (hex(2), hex(3)) {
                    (Some(hi), Some(lo)) if hi * 16 + lo <= 0x7f => {
                        u8::try_from(hi * 16 + lo).ok().map(|v| (v, 4usize))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some((v, width)) = decoded {
            out.push(v);
            k += width;
            continue;
        }
        // `\u{H…}` (N76): a Unicode scalar value, as the UTF-8 bytes the same character
        // written directly in the source would be. Several bytes, so not in `decoded`.
        if bytes.get(k + 1) == Some(&b'u') {
            match unicode_escape(bytes, k) {
                Ok((c, width)) => {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    k += width;
                }
                Err(width) => {
                    let written = raw.get(k..k + width).unwrap_or("\\u");
                    errors.push(
                        Diagnostic::error(
                            codes::INVALID_ESCAPE,
                            span(k, k + width),
                            format!("`{written}` is not a Unicode escape this language has"),
                        )
                        .help(
                            "a Unicode escape is `\\u{` and one to six hexadecimal digits \
                             naming a scalar value — not a surrogate, at most 10FFFF — and `}`",
                        ),
                    );
                    k += width;
                }
            }
            continue;
        }
        // The extent of what was refused: the backslash and the one character after it, or
        // the whole `\xHH` when there were two hex digits that named a byte above `7F`.
        let width = match bytes.get(k + 1) {
            Some(b'x')
                if bytes.get(k + 2).is_some_and(u8::is_ascii_hexdigit)
                    && bytes.get(k + 3).is_some_and(u8::is_ascii_hexdigit) =>
            {
                4
            }
            Some(n) if n.is_ascii() => 2,
            Some(_) => {
                let rest = raw.get(k + 1..).unwrap_or("");
                1 + rest.chars().next().map_or(0, char::len_utf8)
            }
            None => 1,
        };
        let written = raw.get(k..k + width).unwrap_or("\\");
        errors.push(
            Diagnostic::error(
                codes::INVALID_ESCAPE,
                span(k, k + width),
                format!("`{written}` is not an escape this language has"),
            )
            .help(
                "the escapes are \\n \\t \\r \\0 \\\\ \\\" \\xHH up to \\x7F and \\u{H…}; write \
                 `\\\\` for a backslash, and use `str_from_byte` for a byte above 127",
            ),
        );
        k += width;
    }
    // Every byte pushed is ASCII from an escape or a byte of `raw`, which was UTF-8, copied
    // in order between escapes — so the result is UTF-8 too.
    String::from_utf8(out).unwrap_or_default()
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
    let mut out: Vec<Lexeme> = Vec::new();
    let mut errors = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        let (lexeme, next) = scan_one(file, text, i, &mut errors);
        out.push(lexeme);
        i = next;
    }
    out.push(eof(file, text));
    (out, errors)
}

/// The zero-width end of `text`.
pub(crate) fn eof(file: FileId, text: &str) -> Lexeme {
    let end = u32::try_from(text.len()).unwrap_or(u32::MAX);
    Lexeme::Token(Token {
        kind: TokenKind::Eof,
        span: Span::new(file, end, end),
    })
}

/// Scan the one lexeme that begins at byte `i` of `text`, which must be a lexeme boundary,
/// and return it with the offset just past it.
///
/// **Context-free from a boundary, and one byte of lookahead** (N76): what this returns
/// depends only on `text[i..]`, and of that only on the lexeme's own bytes and at most the
/// one byte after it — the byte that ends a run, or the second byte of a two-character
/// operator. Incremental relexing (`crate::incremental`) rests on exactly that, and its
/// equality test against a full scan is what would notice if it stopped being true.
pub(crate) fn scan_one(
    file: FileId,
    text: &str,
    i: usize,
    errors: &mut Vec<Diagnostic>,
) -> (Lexeme, usize) {
    let bytes = text.as_bytes();
    // Checked access throughout. Every one of these reads is provably in bounds from the
    // surrounding loop condition, but "provably" is a property of today's code: the lint
    // is on for the whole workspace precisely so a later edit cannot quietly make one of
    // them a panic on malformed input.
    let byte = |i: usize| -> Option<u8> { bytes.get(i).copied() };
    let at = |i: usize| -> u32 { u32::try_from(i).unwrap_or(u32::MAX) };
    let mut i = i;
    let start = i;
    let Some(c) = byte(i) else {
        return (eof(file, text), text.len());
    };
    macro_rules! emit {
        ($l:expr) => {
            return ($l, i)
        };
    }

    // Whitespace, one lexeme per run: the tree keeps the bytes, and nothing needs to
    // know where one space ends and the next begins.
    if c.is_ascii_whitespace() {
        while byte(i).is_some_and(|b| b.is_ascii_whitespace()) {
            i += 1;
        }
        emit!(Lexeme::Whitespace(Span::new(file, at(start), at(i))));
    }

    // Block comments (R1, 5b.17): `/*` to its matching `*/`, nesting, newlines included. A file
    // that ends inside one is `N0007`, the comment running to the end of the file.
    if c == b'/' && byte(i + 1) == Some(b'*') {
        let mut depth = 0usize;
        while let Some(b) = byte(i) {
            if b == b'/' && byte(i + 1) == Some(b'*') {
                depth += 1;
                i += 2;
            } else if b == b'*' && byte(i + 1) == Some(b'/') {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    break;
                }
            } else {
                i += 1;
            }
        }
        if depth > 0 {
            errors.push(Diagnostic::error(
                codes::UNTERMINATED_COMMENT,
                Span::new(file, at(start), at(start + 2)),
                "this comment has no closing `*/`",
            ));
        }
        emit!(Lexeme::Comment(Span::new(file, at(start), at(i))));
    }

    // Line comments
    if c == b'/' && byte(i + 1) == Some(b'/') {
        while byte(i).is_some_and(|b| b != b'\n') {
            i += 1;
        }
        // Not including the newline: the comment is the text, and where the line
        // ends is the formatter's decision rather than the comment's.
        emit!(Lexeme::Comment(Span::new(file, at(start), at(i))));
    }

    // Numbers: decimal, `0x`, `0o` and `0b` integers, and decimal floating point (Gate 2).
    if c.is_ascii_digit() {
        let radix = match (c, byte(i + 1)) {
            (b'0', Some(b'x' | b'X')) => 16,
            (b'0', Some(b'o' | b'O')) => 8,
            (b'0', Some(b'b' | b'B')) => 2,
            _ => 10,
        };
        if radix != 10 {
            i += 2;
            let digits_start = i;
            while byte(i).is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_') {
                i += 1;
            }
            let digits: String = text
                .get(digits_start..i)
                .unwrap_or_default()
                .chars()
                .filter(|c| *c != '_')
                .collect();
            let span = Span::new(file, at(start), at(i));
            match u128::from_str_radix(&digits, radix) {
                Ok(v) => match i64::try_from(v) {
                    Ok(v) => emit!(Lexeme::Token(Token {
                        kind: TokenKind::Int(v),
                        span,
                    })),
                    // Too large for `Int`, and perhaps right for a `UInt64`: the checker
                    // judges it against the type it is written as, in decimal like any other.
                    Err(_) => emit!(Lexeme::Token(Token {
                        kind: TokenKind::IntTooLarge(v.to_string()),
                        span,
                    })),
                },
                Err(_) => {
                    let name = match radix {
                        16 => "hexadecimal",
                        8 => "octal",
                        _ => "binary",
                    };
                    errors.push(Diagnostic::error(
                        codes::UNKNOWN_CHARACTER,
                        span,
                        format!("this is not a {name} integer"),
                    ));
                    emit!(Lexeme::Invalid(span));
                }
            }
        }
        while byte(i).is_some_and(|b| b.is_ascii_digit() || b == b'_') {
            i += 1;
        }
        // A fraction needs a digit after the `.`, so `1.f` stays a projection and `x.0` is
        // never a number; an exponent needs a digit after its `e` and optional sign.
        let mut float = false;
        if byte(i) == Some(b'.') && byte(i + 1).is_some_and(|b| b.is_ascii_digit()) {
            float = true;
            i += 1;
            while byte(i).is_some_and(|b| b.is_ascii_digit() || b == b'_') {
                i += 1;
            }
        }
        if matches!(byte(i), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(byte(i + 1), Some(b'+' | b'-')));
            if byte(i + 1 + sign).is_some_and(|b| b.is_ascii_digit()) {
                float = true;
                i += 1 + sign;
                while byte(i).is_some_and(|b| b.is_ascii_digit() || b == b'_') {
                    i += 1;
                }
            }
        }
        let raw: String = text
            .get(start..i)
            .unwrap_or_default()
            .chars()
            .filter(|c| *c != '_')
            .collect();
        if float {
            emit!(Lexeme::Token(Token {
                kind: TokenKind::Float(raw),
                span: Span::new(file, at(start), at(i)),
            }));
        }
        match raw.parse::<i64>() {
            Ok(v) => emit!(Lexeme::Token(Token {
                kind: TokenKind::Int(v),
                span: Span::new(file, at(start), at(i)),
            })),
            // Refused at the boundary rather than silently wrapped. A literal that
            // does not fit the type it is written as is a mistake, not a value.
            // Not an error here: `-9223372036854775808` is a valid `Int` and its
            // magnitude does not fit. The parser folds the sign and reports if it
            // still does not.
            Err(_) => emit!(Lexeme::Token(Token {
                kind: TokenKind::IntTooLarge(raw),
                span: Span::new(file, at(start), at(i)),
            })),
        }
    }

    // Interpolated strings (R1a, L4): `f"…"` or `f"""`, decided by the quote after the `f`.
    if c == b'f' && byte(i + 1) == Some(b'"') {
        return interpolated(file, text, start, errors);
    }

    // Raw strings (R1a, L9): `r"…"` or `r#"…"#` with any number of `#`s, decided by the
    // byte after the `r` — a quote, or a `#`, which begins no other token.
    if c == b'r' && matches!(byte(i + 1), Some(b'"' | b'#')) {
        return raw_string(file, text, start, errors);
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
            "trait" => TokenKind::Trait,
            "impl" => TokenKind::Impl,
            _ => TokenKind::Ident(word.to_owned()),
        };
        emit!(Lexeme::Token(Token {
            kind,
            span: Span::new(file, at(start), at(i)),
        }));
    }

    // Multi-line strings (R1a, L9): `"""`, a line break, lines, and `"""` on a line of its
    // own, whose indentation is taken off every line.
    if c == b'"' && byte(i + 1) == Some(b'"') && byte(i + 2) == Some(b'"') {
        return multi_line_string(file, text, start, errors);
    }

    // Strings, with the escapes `docs/user/spec.md` lists (N49). The scan skips the byte
    // after a backslash, so `\"` does not end the literal; decoding is below, once the
    // literal's extent is known.
    if c == b'"' {
        i += 1;
        let content_start = i;
        while byte(i).is_some_and(|b| b != b'"' && b != b'\n') {
            if byte(i) == Some(b'\\') && byte(i + 1).is_some_and(|n| n != b'\n') {
                i += 1;
            }
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
            emit!(Lexeme::Invalid(Span::new(file, at(start), at(i))));
        }
        let raw = text.get(content_start..i).unwrap_or("");
        let value = decode_escapes(
            raw,
            |a, b| Span::new(file, at(content_start + a), at(content_start + b)),
            errors,
        );
        i += 1;
        emit!(Lexeme::Token(Token {
            kind: TokenKind::Str(value),
            span: Span::new(file, at(start), at(i)),
        }));
    }

    // Three-character operators (R1, 5b.12), checked before their two-character prefixes.
    let three = match (c, byte(i + 1), byte(i + 2)) {
        (b'<', Some(b'<'), Some(b'=')) => Some(TokenKind::ShlEq),
        (b'>', Some(b'>'), Some(b'=')) => Some(TokenKind::ShrEq),
        _ => None,
    };
    if let Some(kind) = three {
        i += 3;
        emit!(Lexeme::Token(Token {
            kind,
            span: Span::new(file, at(start), at(i)),
        }));
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
        Some((b'&', b'&')) => (Some(TokenKind::AmpAmp), 2),
        Some((b':', b':')) => (Some(TokenKind::ColonColon), 2),
        Some((b'.', b'.')) => (Some(TokenKind::DotDot), 2),
        Some((b'|', b'|')) => (Some(TokenKind::PipePipe), 2),
        Some((b'<', b'<')) => (Some(TokenKind::Shl), 2),
        Some((b'>', b'>')) => (Some(TokenKind::Shr), 2),
        Some((b'+', b'=')) => (Some(TokenKind::PlusEq), 2),
        Some((b'-', b'=')) => (Some(TokenKind::MinusEq), 2),
        Some((b'*', b'=')) => (Some(TokenKind::StarEq), 2),
        Some((b'/', b'=')) => (Some(TokenKind::SlashEq), 2),
        Some((b'%', b'=')) => (Some(TokenKind::PercentEq), 2),
        Some((b'&', b'=')) => (Some(TokenKind::AmpEq), 2),
        Some((b'|', b'=')) => (Some(TokenKind::PipeEq), 2),
        Some((b'^', b'=')) => (Some(TokenKind::CaretEq), 2),
        _ => (None, 0),
    };
    if let Some(kind) = kind {
        i += width;
        emit!(Lexeme::Token(Token {
            kind,
            span: Span::new(file, at(start), at(i)),
        }));
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
        b'&' => Some(TokenKind::Amp),
        b'|' => Some(TokenKind::Pipe),
        b'^' => Some(TokenKind::Caret),
        b'~' => Some(TokenKind::Tilde),
        b'@' => Some(TokenKind::At),
        _ => None,
    };
    match single {
        Some(kind) => {
            i += 1;
            emit!(Lexeme::Token(Token {
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
            errors.push(Diagnostic::error(
                codes::UNKNOWN_CHARACTER,
                Span::new(file, at(start), at(i)),
                format!("`{ch}` is not valid here"),
            ));
            emit!(Lexeme::Invalid(Span::new(file, at(start), at(i))));
        }
    }
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
        let (_, e) = lex("a ` b # c $");
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

    fn string(src: &str) -> String {
        match kinds(src).first() {
            Some(TokenKind::Str(v)) => v.clone(),
            other => panic!("not a string: {other:?}"),
        }
    }

    #[test]
    fn every_escape_is_the_one_byte_the_specification_names() {
        assert_eq!(string(r#""a\nb""#).as_bytes(), b"a\nb");
        assert_eq!(string(r#""\t\r\0""#).as_bytes(), b"\t\r\0");
        assert_eq!(string(r#""\\""#).as_bytes(), b"\\");
        assert_eq!(string(r#""say \"hi\"""#).as_bytes(), b"say \"hi\"");
        assert_eq!(string(r#""\x41\x7f\x0a\x7F""#).as_bytes(), b"A\x7f\n\x7f");
        // Everything that is not an escape is its own bytes, UTF-8 included.
        assert_eq!(string("\"é\\n\"").as_bytes(), "é\n".as_bytes());
        // An escaped quote does not end the literal.
        assert_eq!(kinds(r#""a\"b" x"#).len(), 3);
    }

    #[test]
    fn a_refused_escape_is_reported_at_its_own_bytes_and_decoding_goes_on() {
        for (src, written) in [
            (r#""a\qb""#, r"\q"),
            (r#""\x8""#, r"\x"),
            (r#""\x80""#, r"\x80"),
            (r#""\xFF""#, r"\xFF"),
            (r#""\u41""#, r"\u"),
            ("\"\\é\"", "\\é"),
        ] {
            let (t, e) = lex(src);
            assert_eq!(e.len(), 1, "{src}: {e:?}");
            assert_eq!(e[0].code, codes::INVALID_ESCAPE, "{src}");
            let at = &e[0].primary.span;
            assert_eq!(
                src.get(at.start as usize..at.end as usize),
                Some(written),
                "{src}"
            );
            // Still one string token, so what follows lexes as before.
            assert!(
                matches!(t.first().map(|t| &t.kind), Some(TokenKind::Str(_))),
                "{src}"
            );
        }
        let (t, e) = lex(r#""\q\n""#);
        assert_eq!(e.len(), 1);
        assert!(matches!(t.first().map(|t| &t.kind), Some(TokenKind::Str(v)) if v == "\n"));
    }

    #[test]
    fn a_unicode_escape_is_the_utf8_bytes_of_its_scalar_value() {
        // N76: the same bytes the character written directly would be.
        assert_eq!(string(r#""\u{41}""#).as_bytes(), b"A");
        assert_eq!(string(r#""\u{e9}""#), "é");
        assert_eq!(string(r#""\u{E9}""#).as_bytes(), "é".as_bytes());
        assert_eq!(string(r#""\u{80}""#).as_bytes(), [0xC2, 0x80]);
        assert_eq!(string(r#""\u{200B}x""#).as_bytes(), "\u{200B}x".as_bytes());
        assert_eq!(string(r#""\u{1F600}""#), "😀");
        assert_eq!(string(r#""\u{10FFFF}""#).len(), 4);
        assert_eq!(string(r#""\u{0}""#).as_bytes(), b"\0");
        assert_eq!(string(r#""\u{00000A}""#).as_bytes(), b"\n");
    }

    #[test]
    fn a_unicode_escape_outside_the_scalar_values_is_refused_at_its_bytes() {
        for (src, written) in [
            (r#""\u{}""#, r"\u{}"),
            (r#""\u{D800}""#, r"\u{D800}"),
            (r#""\u{dfff}""#, r"\u{dfff}"),
            (r#""\u{110000}""#, r"\u{110000}"),
            (r#""\u{0000041}""#, r"\u{0000041}"),
            (r#""\u{12g}""#, r"\u{12"),
            (r#""\u{41""#, r"\u{41"),
            (r#""\u""#, r"\u"),
        ] {
            let (t, e) = lex(src);
            assert_eq!(e.len(), 1, "{src}: {e:?}");
            assert_eq!(e[0].code, codes::INVALID_ESCAPE, "{src}");
            let at = &e[0].primary.span;
            assert_eq!(
                src.get(at.start as usize..at.end as usize),
                Some(written),
                "{src}"
            );
            assert!(
                matches!(t.first().map(|t| &t.kind), Some(TokenKind::Str(_))),
                "{src}"
            );
        }
    }
}
