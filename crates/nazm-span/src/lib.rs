//! Source text, byte spans, and the line/column resolution diagnostics need.
//!
//! Depends on nothing. Every later layer reports positions through this, so a span that
//! travels from the lexer to a type error means the same thing at both ends.
//!
//! Spans are **byte offsets**, not char indices. Byte offsets are what the lexer produces
//! and what slicing needs; converting to a human-facing line and column happens once, at
//! the point of rendering, where the cost is paid per diagnostic rather than per token.

use serde::{Deserialize, Serialize};

/// A half-open byte range `[start, end)` into one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// A span covering both, and everything between. Used to give a compound expression a
    /// position that covers its whole text rather than just its operator.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    #[must_use]
    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.end <= self.start
    }
}

/// A 1-based position, as a human reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

/// One source file, with its line starts precomputed.
#[derive(Debug, Clone)]
pub struct SourceFile {
    name: String,
    text: String,
    /// Byte offset of each line's first character. Always starts with 0.
    line_starts: Vec<u32>,
}

impl SourceFile {
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                // Saturating rather than panicking: a source file over 4GB is not a case
                // worth a crash, and the worst outcome is a misplaced caret.
                line_starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
            }
        }
        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text a span covers, or `None` if it does not lie on a character boundary.
    #[must_use]
    pub fn slice(&self, span: Span) -> Option<&str> {
        self.text.get(span.start as usize..span.end as usize)
    }

    /// Resolve a byte offset to a 1-based line and column.
    ///
    /// Column counts **characters**, not bytes, so a caret under a line containing
    /// non-ASCII text lands where a reader expects it rather than several columns right.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> LineCol {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line_idx).copied().unwrap_or(0);
        let prefix = self
            .text
            .get(line_start as usize..offset as usize)
            .unwrap_or("");
        LineCol {
            line: u32::try_from(line_idx + 1).unwrap_or(u32::MAX),
            column: u32::try_from(prefix.chars().count() + 1).unwrap_or(u32::MAX),
        }
    }

    /// The full text of the line containing `offset`, without its newline.
    #[must_use]
    pub fn line_text(&self, offset: u32) -> &str {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let start = self.line_starts.get(line_idx).copied().unwrap_or(0) as usize;
        let end = self
            .line_starts
            .get(line_idx + 1)
            .map_or(self.text.len(), |e| *e as usize);
        self.text
            .get(start..end)
            .unwrap_or("")
            .trim_end_matches('\n')
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

    #[test]
    fn the_first_character_is_line_one_column_one() {
        let f = SourceFile::new("x.nz", "fn main() -> Int { 1 }");
        assert_eq!(f.line_col(0), LineCol { line: 1, column: 1 });
    }

    #[test]
    fn positions_resolve_across_lines() {
        let f = SourceFile::new("x.nz", "one\ntwo\nthree");
        assert_eq!(f.line_col(4), LineCol { line: 2, column: 1 });
        assert_eq!(f.line_col(6), LineCol { line: 2, column: 3 });
        assert_eq!(f.line_col(8), LineCol { line: 3, column: 1 });
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // A caret placed by byte offset would land four columns right of the `x`.
        let f = SourceFile::new("x.nz", "let نظم = x;");
        let x_at = u32::try_from(f.text().find('x').unwrap()).unwrap();
        let col = f.line_col(x_at).column;
        let char_index = f.text().chars().position(|c| c == 'x').unwrap();
        let expected = u32::try_from(char_index + 1).unwrap();
        assert_eq!(col, expected);
    }

    #[test]
    fn a_span_joins_to_cover_both() {
        let a = Span::new(4, 6);
        let b = Span::new(10, 12);
        assert_eq!(a.join(b), Span::new(4, 12));
        assert_eq!(b.join(a), Span::new(4, 12));
    }

    #[test]
    fn line_text_returns_the_whole_line_without_its_newline() {
        let f = SourceFile::new("x.nz", "alpha\nbeta\ngamma");
        assert_eq!(f.line_text(6), "beta");
        assert_eq!(f.line_text(0), "alpha");
        assert_eq!(f.line_text(11), "gamma");
    }

    #[test]
    fn slicing_a_span_returns_its_text() {
        let f = SourceFile::new("x.nz", "fn main");
        assert_eq!(f.slice(Span::new(3, 7)), Some("main"));
    }
}
