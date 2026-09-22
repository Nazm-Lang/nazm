//! Source identity, byte spans, and the line/column resolution diagnostics need.
//!
//! Depends on nothing. Every later layer reports positions through this, so a span that
//! travels from the lexer to a type error means the same thing at both ends.
//!
//! # Spans name their file
//!
//! A [`Span`] is a [`FileId`] and a **file-local** byte range. That is the whole point of
//! this crate: a semantic consumer never has to ask which file an offset fell in, because
//! the span already says. Before 2026-09-21 a multi-file program was concatenated into one
//! anonymous buffer and the file was recovered by binary-searching the offset — which
//! worked, and made separate compilation impossible, because nothing could be parsed
//! without first knowing where it sat in a merged string.
//!
//! Offsets are **bytes**, not char indices: bytes are what the lexer produces and what
//! slicing needs. Converting to a human-facing line and column happens once, at rendering.
//!
//! # Nothing here is serialisable, on purpose
//!
//! This crate **does not depend on serde**, and that is a design constraint rather than an
//! omission. A [`Span`] is three fields, two of which are meaningless without the third:
//! offset 106 in *which* file. Between 2026-09-21 and 2026-09-22 `Span` carried a
//! hand-written `Serialize` that dropped [`FileId`] and a `Deserialize` that invented
//! [`FileId::ROOT`] in its place. It existed to keep the published diagnostic shape
//! unchanged, which was the right requirement solved in the wrong place: **any** future
//! serializer — a cache, an editor protocol, a module graph dumped for an agent — would
//! have silently destroyed source identity and round-tripped into a plausible lie.
//!
//! Publishing a span is now an explicit act performed by the consumer that owns a wire
//! format. `nazm-diag` owns `nazm.diagnostic/1` and converts through [`SourceMap::global`]
//! at the point of serialisation; `nazm fix` owns `nazm.fix/1` and publishes a file name
//! beside file-local offsets. Adding `serde` back to this crate's manifest is the act a
//! reviewer would have to see, and `cargo xtask check` fails if it appears.

/// Which source a span belongs to.
///
/// # Session-local, and deliberately so
///
/// A `FileId` is an index into one [`SourceMap`], valid for **one compilation in one
/// process**. It is not stable across runs, not derived from content, and not a name a
/// build system or a cache may key on. Two runs over the same tree may number files
/// differently and both are correct.
///
/// Anything needing identity that survives a process — incremental compilation, caching,
/// telling an agent that an edit produced the same definition — needs a different thing
/// that does not exist yet. See `docs/research-register.md` R2 and R3. Conflating the two
/// is the mistake this comment exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(u32);

impl FileId {
    /// The file a compilation started from. Imports are numbered after it, in load order.
    pub const ROOT: Self = Self(0);

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The id a file takes from its position in load order.
    ///
    /// For the loader, which knows a file's position before the [`SourceMap`] exists — it
    /// has to tag the spans it produces while reading a file's imports with the id that
    /// file is about to receive. Minting one for a position the map does not have yields
    /// an id whose lookups return empty, never a panic.
    #[must_use]
    pub fn from_index(i: usize) -> Self {
        Self(u32::try_from(i).unwrap_or(u32::MAX))
    }
}

/// A half-open byte range `[start, end)` **within one file**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub const fn new(file: FileId, start: u32, end: u32) -> Self {
        Self { file, start, end }
    }

    /// A position for a diagnostic that is **about no particular byte** — a built-in's
    /// signature, or "this program has no `main`".
    ///
    /// It names the root file at offset zero, which is what the old two-argument
    /// `Span::new(0, 0)` meant at these call sites. Given a name so that a reader can tell
    /// it apart from a span that genuinely points at the first byte of the root file, and
    /// so that the sites needing it are greppable when something better exists.
    #[must_use]
    pub const fn synthetic() -> Self {
        Self {
            file: FileId::ROOT,
            start: 0,
            end: 0,
        }
    }

    /// A span covering both, and everything between. Used to give a compound expression a
    /// position that covers its whole text rather than just its operator.
    ///
    /// Joining spans from **different files** keeps `self`'s file and range. That case is
    /// a bug in the caller — an expression does not straddle two files — and silently
    /// producing a range that covers most of an unrelated file would hide it behind a
    /// plausible-looking caret. Keeping the left operand fails locally instead.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        if self.file != other.file {
            return self;
        }
        Self {
            file: self.file,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

/// One source file: its name, its text, and where its lines begin.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    text: String,
    /// Byte offset of each line's first character, **file-local**. Always starts with 0.
    line_starts: Vec<u32>,
    /// Where this file would begin in the concatenation of every file in load order.
    ///
    /// Carried for one reason: the machine-readable diagnostic schema has always emitted
    /// offsets into that concatenation, and `docs/diagnostics.md` promises not to change
    /// the shape inside a version. It is a **wire compatibility** detail, used by
    /// [`SourceMap::global`] and nowhere else, and no semantic pass may read it.
    global_start: u32,
}

/// Every source of one compilation, addressed by [`FileId`].
#[derive(Debug, Clone)]
pub struct SourceMap {
    files: Vec<Entry>,
}

impl SourceMap {
    /// A one-file compilation.
    #[must_use]
    pub fn single(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self::from_parts(vec![(name.into(), text.into())])
    }

    /// Several files, in load order. The first is the root.
    ///
    /// Each part is given a trailing newline if it lacks one — preserved from the merged
    /// buffer this replaced, where it stopped a file that did not end in a newline from
    /// joining its last line to the next file's first. It still matters, because
    /// [`SourceMap::global`] has to reproduce those offsets exactly.
    #[must_use]
    pub fn from_parts(parts: Vec<(String, String)>) -> Self {
        let mut files = Vec::with_capacity(parts.len().max(1));
        let mut global = 0u32;
        for (name, part) in parts {
            let mut text = part;
            if !text.ends_with('\n') {
                text.push('\n');
            }
            let mut line_starts = vec![0u32];
            for (i, b) in text.bytes().enumerate() {
                if b == b'\n' {
                    // Saturating rather than panicking: a source file over 4GB is not a
                    // case worth a crash, and the worst outcome is a misplaced caret.
                    line_starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
                }
            }
            let len = u32::try_from(text.len()).unwrap_or(u32::MAX);
            files.push(Entry {
                name,
                text,
                line_starts,
                global_start: global,
            });
            global = global.saturating_add(len);
        }
        if files.is_empty() {
            files.push(Entry {
                name: String::new(),
                text: String::new(),
                line_starts: vec![0],
                global_start: 0,
            });
        }
        Self { files }
    }

    /// The file a compilation started from.
    #[must_use]
    pub const fn root(&self) -> FileId {
        FileId::ROOT
    }

    /// Every file, in load order.
    pub fn ids(&self) -> impl Iterator<Item = FileId> + use<> {
        (0..u32::try_from(self.files.len()).unwrap_or(u32::MAX)).map(FileId)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn entry(&self, file: FileId) -> Option<&Entry> {
        self.files.get(file.index())
    }

    /// The name a reader would open.
    #[must_use]
    pub fn name(&self, file: FileId) -> &str {
        self.entry(file).map_or("", |e| e.name.as_str())
    }

    /// The text of one file.
    #[must_use]
    pub fn text(&self, file: FileId) -> &str {
        self.entry(file).map_or("", |e| e.text.as_str())
    }

    /// Every file's name, in load order.
    #[must_use]
    pub fn file_names(&self) -> Vec<&str> {
        self.files.iter().map(|e| e.name.as_str()).collect()
    }

    /// The text a span covers, or `None` if it does not lie on a character boundary.
    #[must_use]
    pub fn slice(&self, span: Span) -> Option<&str> {
        self.entry(span.file)?
            .text
            .get(span.start as usize..span.end as usize)
    }

    /// The offsets this span would have had in a concatenation of every file.
    ///
    /// **Wire compatibility only.** `nazm.diagnostic/1` has always published offsets into
    /// that concatenation, so they are reproduced here rather than changed under existing
    /// consumers. Nothing semantic may use this; `docs/diagnostics.md` records the promise
    /// and what a future schema version would do instead.
    #[must_use]
    pub fn global(&self, span: Span) -> (u32, u32) {
        let base = self.entry(span.file).map_or(0, |e| e.global_start);
        (
            base.saturating_add(span.start),
            base.saturating_add(span.end),
        )
    }

    /// Resolve a span's start to a 1-based line and column **within its file**.
    ///
    /// Column counts **characters**, not bytes, so a caret under a line containing
    /// non-ASCII text lands where a reader expects it rather than several columns right.
    #[must_use]
    pub fn line_col(&self, span: Span) -> LineCol {
        let Some(e) = self.entry(span.file) else {
            return LineCol { line: 1, column: 1 };
        };
        let line_idx = match e.line_starts.binary_search(&span.start) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = e.line_starts.get(line_idx).copied().unwrap_or(0);
        let prefix = e
            .text
            .get(line_start as usize..span.start as usize)
            .unwrap_or("");
        LineCol {
            line: u32::try_from(line_idx + 1).unwrap_or(u32::MAX),
            column: u32::try_from(prefix.chars().count() + 1).unwrap_or(u32::MAX),
        }
    }

    /// The full text of the line containing the span's start, without its newline.
    #[must_use]
    pub fn line_text(&self, span: Span) -> &str {
        let Some(e) = self.entry(span.file) else {
            return "";
        };
        let line_idx = match e.line_starts.binary_search(&span.start) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let start = e.line_starts.get(line_idx).copied().unwrap_or(0) as usize;
        let end = e
            .line_starts
            .get(line_idx + 1)
            .map_or(e.text.len(), |x| *x as usize);
        e.text.get(start..end).unwrap_or("").trim_end_matches('\n')
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

    fn one(text: &str) -> (SourceMap, FileId) {
        let m = SourceMap::single("x.nz", text);
        let root = m.root();
        (m, root)
    }

    fn at(f: FileId, start: u32, end: u32) -> Span {
        Span::new(f, start, end)
    }

    #[test]
    fn the_first_character_is_line_one_column_one() {
        let (m, f) = one("fn main() -> Int { 1 }");
        assert_eq!(m.line_col(at(f, 0, 0)), LineCol { line: 1, column: 1 });
    }

    #[test]
    fn positions_resolve_across_lines() {
        let (m, f) = one("one\ntwo\nthree");
        assert_eq!(m.line_col(at(f, 4, 4)), LineCol { line: 2, column: 1 });
        assert_eq!(m.line_col(at(f, 6, 6)), LineCol { line: 2, column: 3 });
        assert_eq!(m.line_col(at(f, 8, 8)), LineCol { line: 3, column: 1 });
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // A caret placed by byte offset would land four columns right of the `x`.
        let (m, f) = one("let نظم = x;");
        let x_at = u32::try_from(m.text(f).find('x').unwrap()).unwrap();
        let col = m.line_col(at(f, x_at, x_at)).column;
        let char_index = m.text(f).chars().position(|c| c == 'x').unwrap();
        assert_eq!(col, u32::try_from(char_index + 1).unwrap());
    }

    #[test]
    fn a_span_joins_to_cover_both() {
        let f = FileId::ROOT;
        let a = at(f, 4, 6);
        let b = at(f, 10, 12);
        assert_eq!(a.join(b), at(f, 4, 12));
        assert_eq!(b.join(a), at(f, 4, 12));
    }

    #[test]
    fn joining_across_files_keeps_the_left_span_rather_than_inventing_a_range() {
        // An expression does not straddle two files, so this is a caller bug. Covering
        // "from here in a.nz to there in b.nz" would render a caret over an unrelated
        // file and look plausible; keeping the left operand does not.
        let m = SourceMap::from_parts(vec![
            ("a.nz".to_owned(), "aaaa".to_owned()),
            ("b.nz".to_owned(), "bbbb".to_owned()),
        ]);
        let mut it = m.ids();
        let a = it.next().unwrap();
        let b = it.next().unwrap();
        assert_eq!(at(a, 1, 2).join(at(b, 0, 4)), at(a, 1, 2));
    }

    #[test]
    fn line_text_returns_the_whole_line_without_its_newline() {
        let (m, f) = one("alpha\nbeta\ngamma");
        assert_eq!(m.line_text(at(f, 6, 6)), "beta");
        assert_eq!(m.line_text(at(f, 0, 0)), "alpha");
        assert_eq!(m.line_text(at(f, 11, 11)), "gamma");
    }

    #[test]
    fn slicing_a_span_returns_its_text() {
        let (m, f) = one("fn main");
        assert_eq!(m.slice(at(f, 3, 7)), Some("main"));
    }

    #[test]
    fn every_file_has_its_own_identity_and_its_own_offsets() {
        // The case the merged buffer could not express: two files whose text is
        // identical. Under concatenation these were told apart only by where they
        // happened to land; here they are different files and say so.
        let m = SourceMap::from_parts(vec![
            ("a.nz".to_owned(), "fn f() -> Int { 1 }\n".to_owned()),
            ("b.nz".to_owned(), "fn f() -> Int { 1 }\n".to_owned()),
        ]);
        let mut it = m.ids();
        let a = it.next().unwrap();
        let b = it.next().unwrap();
        assert_ne!(a, b);
        assert_eq!(m.name(a), "a.nz");
        assert_eq!(m.name(b), "b.nz");
        // Same local offsets, same text, different files.
        assert_eq!(m.slice(at(a, 3, 4)), Some("f"));
        assert_eq!(m.slice(at(b, 3, 4)), Some("f"));
        assert_eq!(m.line_col(at(a, 3, 4)), m.line_col(at(b, 3, 4)));
    }

    #[test]
    fn line_numbers_are_local_to_the_file() {
        // Under the merged buffer this needed a per-segment `first_line` correction.
        // Now it is what counting lines in the file gives, with nothing to subtract.
        let m = SourceMap::from_parts(vec![
            ("a.nz".to_owned(), "1\n2\n3\n".to_owned()),
            ("b.nz".to_owned(), "x\ny\n".to_owned()),
        ]);
        let b = m.ids().nth(1).unwrap();
        assert_eq!(m.line_col(at(b, 2, 3)), LineCol { line: 2, column: 1 });
    }

    #[test]
    fn global_offsets_reproduce_the_old_merged_buffer() {
        // The wire contract: `nazm.diagnostic/1` publishes offsets into the
        // concatenation of every file in load order, including the newline appended to a
        // file that lacks one. Changing those numbers would break consumers silently, so
        // they are reproduced rather than reinterpreted.
        let m = SourceMap::from_parts(vec![
            ("a.nz".to_owned(), "abc".to_owned()), // no trailing newline: one is added
            ("b.nz".to_owned(), "de\n".to_owned()),
        ]);
        let mut it = m.ids();
        let a = it.next().unwrap();
        let b = it.next().unwrap();
        assert_eq!(m.global(at(a, 0, 3)), (0, 3));
        // "abc\n" is four bytes, so b starts at 4.
        assert_eq!(m.global(at(b, 0, 2)), (4, 6));
    }

    #[test]
    fn a_file_without_a_trailing_newline_still_ends_its_own_last_line() {
        // Carried over from the merged-buffer era, where a file not ending in a newline
        // would have joined its last line to the next file's first. Files are separate
        // now so that cannot happen — but `from_parts` still appends the newline, because
        // `global` has to reproduce the offsets the schema publishes, and this pins both
        // halves: the line is whole, and the next file starts one byte later than its
        // text alone would suggest.
        let m = SourceMap::from_parts(vec![
            ("a.nz".to_owned(), "let x = 1;".to_owned()),
            ("b.nz".to_owned(), "let y = 2;".to_owned()),
        ]);
        let mut it = m.ids();
        let a = it.next().unwrap();
        let b = it.next().unwrap();
        assert_eq!(m.line_text(at(a, 0, 0)), "let x = 1;");
        assert_eq!(m.line_text(at(b, 0, 0)), "let y = 2;");
        assert_eq!(m.name(b), "b.nz");
        assert_eq!(m.line_col(at(b, 0, 0)), LineCol { line: 1, column: 1 });
        assert_eq!(m.global(at(b, 0, 0)), (11, 11));
    }

    #[test]
    fn a_file_id_is_an_index_into_its_own_map_and_nothing_else() {
        // Stated as a test because the comment on `FileId` is a promise about scope:
        // session-local, positional, not content-derived. Load order decides it.
        let m = SourceMap::from_parts(vec![
            ("first.nz".to_owned(), "a".to_owned()),
            ("second.nz".to_owned(), "b".to_owned()),
        ]);
        assert_eq!(m.root(), FileId::ROOT);
        assert_eq!(m.name(FileId::ROOT), "first.nz");
        assert_eq!(m.ids().count(), 2);
    }
}
