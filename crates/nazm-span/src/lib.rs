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

/// One file merged into a [`SourceFile`] buffer.
#[derive(Debug, Clone)]
struct Segment {
    name: String,
    /// Where this file's text begins in the combined buffer.
    start: u32,
    /// Index into `line_starts` of this file's first line, so a line number can be
    /// reported relative to the file a reader is looking at rather than to the buffer.
    first_line: usize,
}

/// The source of one compilation, with its line starts precomputed.
///
/// # Why one buffer rather than a list of files
///
/// A [`Span`] is a pair of byte offsets and carries no file identity, and giving it one
/// would touch every diagnostic, every AST node and every test in the workspace. So a
/// multi-file program is **concatenated into one buffer** and the buffer remembers where
/// each file began: spans stay globally unique and comparable, and rendering asks which
/// segment an offset falls in. A single-file program is the same structure with one
/// segment, so there is one code path rather than two.
#[derive(Debug, Clone)]
pub struct SourceFile {
    name: String,
    text: String,
    /// Byte offset of each line's first character. Always starts with 0.
    line_starts: Vec<u32>,
    /// In load order, never empty. `start` is non-decreasing.
    segments: Vec<Segment>,
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
        let name = name.into();
        Self {
            segments: vec![Segment {
                name: name.clone(),
                start: 0,
                first_line: 0,
            }],
            name,
            text,
            line_starts,
        }
    }

    /// Several files as one buffer, in load order.
    ///
    /// Each part is given a trailing newline if it lacks one, so a file that does not end
    /// in one cannot join its last line to the next file's first.
    #[must_use]
    pub fn from_parts(parts: Vec<(String, String)>) -> Self {
        let root = parts.first().map_or_else(String::new, |(n, _)| n.clone());
        let mut text = String::new();
        let mut line_starts = vec![0u32];
        let mut segments = Vec::new();
        for (name, part) in parts {
            segments.push(Segment {
                name,
                start: u32::try_from(text.len()).unwrap_or(u32::MAX),
                first_line: line_starts.len() - 1,
            });
            let from = text.len();
            text.push_str(&part);
            if !part.ends_with('\n') {
                text.push('\n');
            }
            // Only the bytes just appended, so building an N-file buffer stays linear
            // rather than rescanning everything once per file.
            for (i, b) in text.bytes().enumerate().skip(from) {
                if b == b'\n' {
                    line_starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
                }
            }
        }
        if segments.is_empty() {
            segments.push(Segment {
                name: root.clone(),
                start: 0,
                first_line: 0,
            });
        }
        Self {
            name: root,
            text,
            line_starts,
            segments,
        }
    }

    /// The name of the file an offset falls in.
    ///
    /// For a single-file buffer this is always [`SourceFile::name`]; for a merged one it
    /// is the file a reader would open to find that byte.
    #[must_use]
    pub fn name_at(&self, offset: u32) -> &str {
        self.segment_at(offset)
            .map_or(self.name.as_str(), |s| s.name.as_str())
    }

    /// The file an offset falls in, and the offset **within that file**.
    ///
    /// What a tool applying an edit needs. A span in a merged buffer names a byte in one
    /// of several files; writing at the merged offset would edit whichever file happens
    /// to be first, at a position that means nothing.
    #[must_use]
    pub fn locate(&self, offset: u32) -> (&str, u32) {
        self.segment_at(offset)
            .map_or((self.name.as_str(), offset), |s| {
                (s.name.as_str(), offset.saturating_sub(s.start))
            })
    }

    /// Every file in this buffer, in load order.
    #[must_use]
    pub fn file_names(&self) -> Vec<&str> {
        self.segments.iter().map(|s| s.name.as_str()).collect()
    }

    fn segment_at(&self, offset: u32) -> Option<&Segment> {
        match self.segments.binary_search_by_key(&offset, |s| s.start) {
            Ok(i) => self.segments.get(i),
            Err(i) => self.segments.get(i.saturating_sub(1)),
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
        // Relative to the file the offset is in, not to the combined buffer: a reader
        // has the file open, not the concatenation.
        let first = self.segment_at(offset).map_or(0, |s| s.first_line);
        LineCol {
            line: u32::try_from(line_idx.saturating_sub(first) + 1).unwrap_or(u32::MAX),
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod multi_file {
    use super::*;

    #[test]
    fn a_merged_buffer_reports_the_file_and_the_line_within_it() {
        let f = SourceFile::from_parts(vec![
            ("a.nz".to_owned(), "one\ntwo\n".to_owned()),
            ("b.nz".to_owned(), "three\nfour".to_owned()),
        ]);
        // a.nz occupies 0..8, b.nz 8..
        assert_eq!(f.name_at(0), "a.nz");
        assert_eq!(f.name_at(7), "a.nz");
        assert_eq!(f.name_at(8), "b.nz");
        assert_eq!(f.name_at(20), "b.nz");

        // Lines are counted within the file a reader would open, not across the buffer.
        assert_eq!(f.line_col(0), LineCol { line: 1, column: 1 });
        assert_eq!(f.line_col(4), LineCol { line: 2, column: 1 });
        assert_eq!(f.line_col(8), LineCol { line: 1, column: 1 });
        assert_eq!(f.line_col(14), LineCol { line: 2, column: 1 });

        assert_eq!(f.line_text(8), "three");
        assert_eq!(f.line_text(14), "four");
        assert_eq!(f.file_names(), vec!["a.nz", "b.nz"]);
    }

    #[test]
    fn a_part_without_a_trailing_newline_does_not_join_the_next_file() {
        let f = SourceFile::from_parts(vec![
            ("a.nz".to_owned(), "let x = 1;".to_owned()),
            ("b.nz".to_owned(), "let y = 2;".to_owned()),
        ]);
        assert_eq!(f.line_text(0), "let x = 1;");
        assert_eq!(f.line_text(11), "let y = 2;");
        assert_eq!(f.name_at(11), "b.nz");
    }

    #[test]
    fn a_single_file_is_the_same_structure_with_one_segment() {
        let f = SourceFile::new("only.nz", "a\nb\n");
        assert_eq!(f.name_at(0), "only.nz");
        assert_eq!(f.name_at(3), "only.nz");
        assert_eq!(f.file_names(), vec!["only.nz"]);
        assert_eq!(f.line_col(2), LineCol { line: 2, column: 1 });
    }
}
