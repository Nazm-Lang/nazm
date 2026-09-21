//! Following `use` from a root file to the whole program.
//!
//! # A map of files, not a buffer
//!
//! Every reachable file is read once and given a [`FileId`]; the result is a
//! [`SourceMap`]. Each file is then parsed **on its own text**, so a span is a position in
//! a file a reader can open.
//!
//! Until 2026-09-21 the files were concatenated into a single anonymous buffer and the
//! originating file was recovered by binary-searching the offset. That worked, and it is
//! why this module used to carry a running `base` offset to place an import diagnostic
//! correctly. The arithmetic is gone: a diagnostic raised while reading a file already
//! names that file.
//!
//! # What the loader decides, and what it does not
//!
//! It decides *which files* and *in what order*. It does not decide what a name means:
//! after loading, name resolution is the checker's, unchanged, over one program. That
//! keeps modules from becoming a second scoping system.
//!
//! # Determinism
//!
//! The root comes first, then its imports depth-first in source order, and each file is
//! read at most once keyed by its **canonical path** — so a file reached by two different
//! spellings is one file, and the map's contents depend on the import graph rather than on
//! the filesystem's iteration order. A cycle terminates because the second visit finds the
//! file already loaded; it is not an error, and `docs/spec.md` says why.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nazm_diag::{codes, Diagnostic};
use nazm_span::{FileId, SourceMap, Span};

/// Read `root` and everything it imports, as one buffer.
///
/// # Errors
///
/// A file that cannot be read, or an import that cannot be found. Parse errors are not
/// reported here: the merged buffer is parsed again by the caller, which reports them
/// all at once against the positions they will keep.
/// On failure the buffer built **so far** comes back with the diagnostics, so a
/// "cannot find" points at the `use` line that wrote it rather than at an empty file.
pub(crate) fn load(root: &Path) -> Result<SourceMap, (SourceMap, Vec<Diagnostic>)> {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut seen: HashMap<PathBuf, ()> = HashMap::new();
    let mut errors: Vec<Diagnostic> = Vec::new();

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
                // No file to point into: this one could not be read. Said plainly rather
                // than aimed at an arbitrary byte of whatever had been loaded so far,
                // which is what the merged-buffer version did.
                errors.push(
                    Diagnostic::error(
                        codes::MODULE_NOT_FOUND,
                        Span::synthetic(),
                        format!("cannot read `{}`", path.display()),
                    )
                    .help(format!("{e}")),
                );
                continue;
            }
        };

        // The id this file is about to get. Known before the map exists, because it is
        // the file's position in load order — which is what a `FileId` is.
        let id = FileId::from_index(parts.len());

        // Parsed for its imports only. Parse errors are ignored here and reported by the
        // caller, which parses every file again with the same ids, so a syntax error is
        // reported once at the position it will still have.
        let (program, _) = nazm_core::__syntax::parse_file(id, &text);
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
                        // Already names this file: no base to add.
                        import.path_span,
                        format!("cannot find `{}`", import.path),
                    )
                    .help(format!(
                        "looked for `{}`, relative to the importing file",
                        target.display()
                    )),
                );
            }
        }

        // Pushed in reverse so the stack pops them in source order: the map's order
        // follows the program's text, which is what makes it reproducible.
        for child in children.into_iter().rev() {
            stack.push(child);
        }

        parts.push((path.display().to_string(), text));
    }

    let map = SourceMap::from_parts(parts);
    if errors.is_empty() {
        Ok(map)
    } else {
        Err((map, errors))
    }
}
