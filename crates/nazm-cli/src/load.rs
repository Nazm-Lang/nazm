//! Following `use` from a root file to the whole program.
//!
//! # One buffer, not a list of files
//!
//! Every reachable file is read once and **concatenated into a single
//! [`SourceFile`]**, which is then parsed, checked and compiled exactly as a one-file
//! program is. A [`Span`](nazm_span::Span) carries no file identity, and giving it one
//! would touch every diagnostic, every syntax node and every test in the workspace; the
//! buffer remembers where each file began instead, so a diagnostic still names the file
//! a reader would open. There is one code path, and a single-file program takes it too.
//!
//! # What the loader decides, and what it does not
//!
//! It decides *which files* and *in what order*. It does not decide what a name means:
//! after merging, name resolution is the checker's, unchanged, over one program. That
//! keeps modules from becoming a second scoping system.
//!
//! # Determinism
//!
//! The root comes first, then its imports depth-first in source order, and each file is
//! read at most once keyed by its **canonical path** — so a file reached by two different
//! spellings is one file, and the buffer's contents depend on the import graph rather
//! than on the filesystem's iteration order. A cycle terminates because the second visit
//! finds the file already loaded; it is not an error, and `docs/spec.md` says why.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nazm_diag::{codes, Diagnostic};
use nazm_span::{SourceFile, Span};

/// Read `root` and everything it imports, as one buffer.
///
/// # Errors
///
/// A file that cannot be read, or an import that cannot be found. Parse errors are not
/// reported here: the merged buffer is parsed again by the caller, which reports them
/// all at once against the positions they will keep.
/// On failure the buffer built **so far** comes back with the diagnostics, so a
/// "cannot find" points at the `use` line that wrote it rather than at an empty file.
pub(crate) fn load(root: &Path) -> Result<SourceFile, (SourceFile, Vec<Diagnostic>)> {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut seen: HashMap<PathBuf, ()> = HashMap::new();
    let mut errors: Vec<Diagnostic> = Vec::new();
    // Where each loaded file begins in the buffer being built, so an import diagnostic
    // found while reading file F points at F's own text after F has been placed.
    let mut base: u32 = 0;

    // An explicit stack rather than recursion: an import chain has no bound, and the
    // compiler's own rule is that a recursive walk needs a bound or a controlled stack.
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.insert(key, ()).is_some() {
            continue;
        }

        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                errors.push(
                    Diagnostic::error(
                        codes::MODULE_NOT_FOUND,
                        Span::new(base, base),
                        format!("cannot read `{}`", path.display()),
                    )
                    .help(format!("{e}")),
                );
                continue;
            }
        };

        // Parsed for its imports only. Parse errors are ignored here and reported by the
        // caller against the merged buffer, so a syntax error is reported once, at the
        // position it will still have.
        let (program, _) = nazm_core::__syntax::parse_source(&text);
        let dir = path.parent().unwrap_or_else(|| Path::new("."));

        let mut children = Vec::new();
        for import in &program.imports {
            let target = dir.join(&import.path);
            if target.exists() {
                children.push(target);
            } else {
                errors.push(
                    Diagnostic::error(
                        codes::MODULE_NOT_FOUND,
                        Span::new(base + import.path_span.start, base + import.path_span.end),
                        format!("cannot find `{}`", import.path),
                    )
                    .help(format!(
                        "looked for `{}`, relative to the importing file",
                        target.display()
                    )),
                );
            }
        }

        // Pushed in reverse so the stack pops them in source order: the buffer's layout
        // follows the program's text, which is what makes it reproducible.
        for child in children.into_iter().rev() {
            stack.push(child);
        }

        base = base
            .saturating_add(u32::try_from(text.len()).unwrap_or(u32::MAX))
            .saturating_add(u32::from(!text.ends_with('\n')));
        parts.push((path.display().to_string(), text));
    }

    let buffer = SourceFile::from_parts(parts);
    if errors.is_empty() {
        Ok(buffer)
    } else {
        Err((buffer, errors))
    }
}
