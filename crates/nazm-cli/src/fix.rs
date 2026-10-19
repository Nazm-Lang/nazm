//! `nazm fix` — apply the edits the compiler already knows about.
//!
//! # Why this is a command and not a document
//!
//! Every diagnostic that can suggest a repair already carries it as data: a span, a
//! replacement, and the condition under which the edit is right. Leaving that in the JSON
//! for someone else to implement means every tool implements it slightly differently, and
//! the differences are where the wrong edits live. One implementation, in the compiler,
//! is the one that can be tested.
//!
//! # What it will and will not do
//!
//! Only [`Applicability::Automatic`] fixes are applied. A `NeedsReview` fix is printed
//! with its precondition and left alone — the whole point of the distinction is that a
//! machine cannot decide it. `--apply` says so explicitly: without it nothing is written,
//! which makes the default safe to run against a tree you have not read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nazm_diag::{Applicability, Diagnostic};
use nazm_span::SourceMap;

use crate::{exit, load};

/// The version every `nazm fix` JSON object carries. See `docs/user/diagnostics.md`.
const SCHEMA: &str = "nazm.fix/1";

/// One suggested edit, resolved to a real file and offset.
#[derive(Debug, Clone, serde::Serialize)]
struct Edit {
    schema: &'static str,
    /// The diagnostic this edit came from.
    code: String,
    /// The file to write, as it was named when loaded.
    file: String,
    /// Byte range **within that file**, not within the merged buffer.
    start: u32,
    end: u32,
    replacement: String,
    description: String,
    applicability: Applicability,
    precondition: String,
    /// The bytes this edit expects to replace, as the checker saw them.
    ///
    /// Not part of the wire format — it is how the edit is checked against the file, not
    /// something a consumer needs. An edit is **stale** when the file on disk no longer
    /// has these bytes there: somebody saved over it between the check and the write,
    /// and applying a span computed against the old text would corrupt the new one.
    #[serde(skip)]
    expected: String,
    /// Whether this run wrote it. `false` for a dry run and for anything skipped.
    applied: bool,
    /// Why it was not applied, when it was not. `None` when it was.
    skipped: Option<&'static str>,
}

pub(crate) fn run(path: &Path, apply: bool, json: bool) -> ExitCode {
    let first = match loaded(path, json) {
        Ok(l) => l,
        Err(code) => return ExitCode::from(code),
    };

    let diagnostics = nazm_core::check_map(&first.map, &first.graph);
    let mut edits = collect(&diagnostics, &first.map);

    if apply {
        write_back(&mut edits);
    }

    report(&edits, json);

    // The state *after* the run, taken by re-reading rather than by predicting: an edit
    // that was applied may have revealed another error, and claiming success from the
    // number of edits made would be a guess about what the checker will say next.
    let remaining = match apply {
        true => match loaded(path, json) {
            Ok(again) => nazm_core::check_map(&again.map, &again.graph),
            Err(code) => return ExitCode::from(code),
        },
        false => diagnostics,
    };

    let applied = edits.iter().filter(|e| e.applied).count();
    if !json {
        summarise(applied, apply, remaining.len());
    }
    if remaining.is_empty() {
        ExitCode::from(exit::OK)
    } else {
        ExitCode::from(exit::CHECK_FAILED)
    }
}

/// The program, or the status its loading failed with (Q1-C1).
fn loaded(path: &Path, json: bool) -> Result<load::Loaded, u8> {
    // Discovered, never given: `nazm fix` edits source and persists nothing, so the only
    // reason it asks at all is that the loader computes a durable key for every module
    // whether or not this command has a use for one.
    crate::opened::opened(path, None, json)
}

/// Every fix in every diagnostic, resolved to the file it edits.
/// Turn each fix into an edit against a real file.
///
/// # What a `Span` carrying its file removed from here
///
/// This used to call `SourceFile::locate` twice — a binary search over a merged buffer to
/// recover which file an offset fell in and what the offset was inside it — and then
/// compare the two names, because a span whose ends landed in different files would have
/// been written to whichever one came first, at a position that meant nothing.
///
/// A [`nazm_span::Span`] now *has* one file. The search is a field access, the offsets are
/// already file-local, and the straddling case is not a check that passes but a state that
/// cannot be constructed.
fn collect(diagnostics: &[Diagnostic], map: &SourceMap) -> Vec<Edit> {
    let mut edits = Vec::new();
    for d in diagnostics {
        for f in &d.fixes {
            let name = map.name(f.span.file);
            let (start, end) = (f.span.start, f.span.end);
            let expected = map.slice(f.span).unwrap_or_default().to_owned();
            edits.push(Edit {
                schema: SCHEMA,
                code: d.code.to_string(),
                file: name.to_owned(),
                start,
                end,
                replacement: f.replacement.clone(),
                description: f.description.clone(),
                applicability: f.applicability,
                precondition: f.precondition.clone(),
                expected,
                applied: false,
                skipped: match f.applicability {
                    Applicability::NeedsReview => Some("it needs review"),
                    Applicability::Automatic => None,
                },
            });
        }
    }
    edits
}

/// Apply every automatic edit, grouped by file, and mark what was written.
///
/// # Right to left
///
/// Edits are applied in descending order of position, so each one's offsets are still
/// correct when it is made: an earlier edit that changed the text's length would move
/// every later span, and re-deriving them would mean re-running the checker between
/// every pair of edits.
///
/// # Overlaps are skipped, not merged
///
/// Two edits to the same bytes are two repairs for what may be one mistake, and applying
/// both produces text neither suggested. The first — the later one in the file — wins,
/// and the other is reported as skipped so the next run can offer it against the new
/// text.
fn write_back(edits: &mut [Edit]) {
    write_back_with(
        edits,
        |p| std::fs::read_to_string(p),
        |p, t| std::fs::write(p, t),
    );
}

/// [`write_back`], reading and writing through `read` and `write` — the file system in
/// every run, and a failure a test can arrange in the unit tests of each skip reason.
fn write_back_with(
    edits: &mut [Edit],
    read: impl Fn(&Path) -> std::io::Result<String>,
    mut write: impl FnMut(&Path, &str) -> std::io::Result<()>,
) {
    // Keyed by an owned name rather than a borrow of `edits`, which the loop below
    // writes back into.
    let mut by_file: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in edits.iter().enumerate() {
        if e.skipped.is_none() {
            by_file.entry(e.file.clone()).or_default().push(i);
        }
    }

    for (name, mut indices) in by_file {
        let path = PathBuf::from(&name);
        let Ok(original) = read(&path) else {
            for i in indices {
                if let Some(e) = edits.get_mut(i) {
                    e.skipped = Some("the file could not be read");
                }
            }
            continue;
        };

        indices.sort_by_key(|&i| std::cmp::Reverse(edits.get(i).map_or(0, |e| e.start)));

        let mut text = original.clone();
        let mut lowest = u32::MAX;
        let mut written = Vec::new();
        for i in indices {
            let Some(e) = edits.get(i) else { continue };
            if e.end > lowest {
                if let Some(e) = edits.get_mut(i) {
                    e.skipped = Some("it overlaps an edit already applied in this run");
                }
                continue;
            }
            let (from, to) = (e.start as usize, e.end as usize);
            if to > text.len() || !text.is_char_boundary(from) || !text.is_char_boundary(to) {
                if let Some(e) = edits.get_mut(i) {
                    e.skipped = Some("the span does not name a range of this file");
                }
                continue;
            }
            // The bytes must still be the ones the checker saw. Two things can break
            // that: somebody saving the file between the check and this write, and a
            // bug here that miscomputed an offset. Both produce the same corruption —
            // a replacement in the wrong place — and neither is worth risking to save
            // a comparison of a few bytes.
            if text.get(from..to) != Some(e.expected.as_str()) {
                if let Some(e) = edits.get_mut(i) {
                    e.skipped = Some("the file changed since it was checked");
                }
                continue;
            }
            text.replace_range(from..to, &e.replacement);
            lowest = e.start;
            written.push(i);
        }

        // Written once, and only if the result still parses. An automatic fix that
        // produced something the parser rejects is a defect in the fix, and shipping it
        // to disk would turn one reported error into a file nobody can compile.
        if !parses(&text) {
            for i in written {
                if let Some(e) = edits.get_mut(i) {
                    e.skipped = Some("applying it produced a file that does not parse");
                }
            }
            continue;
        }
        if text == original || write(&path, &text).is_ok() {
            for i in written {
                if let Some(e) = edits.get_mut(i) {
                    e.applied = true;
                }
            }
        } else {
            for i in written {
                if let Some(e) = edits.get_mut(i) {
                    e.skipped = Some("the file could not be written");
                }
            }
        }
    }
}

/// Whether the text still parses. Only *parsing*, because a fix may legitimately leave
/// a type error behind for the next round — what it may never leave behind is a file the
/// parser rejects.
fn parses(text: &str) -> bool {
    let (_, errors) = nazm_core::__syntax::parse_source(text);
    errors.is_empty()
}

fn report(edits: &[Edit], json: bool) {
    for e in edits {
        if json {
            match serde_json::to_string(e) {
                Ok(line) => println!("{line}"),
                Err(_) => println!("{{\"schema\":\"{SCHEMA}\",\"error\":\"unserialisable\"}}"),
            }
            continue;
        }
        let state = match (e.applied, e.skipped) {
            (true, _) => "applied".to_owned(),
            (false, Some(why)) => format!("not applied — {why}"),
            (false, None) => "would apply".to_owned(),
        };
        println!("{}: {} [{}]", e.code, e.description, state);
        println!("  in {} at bytes {}..{}", e.file, e.start, e.end);
        println!("  right when: {}", e.precondition);
    }
}

fn summarise(applied: usize, wrote: bool, remaining: usize) {
    if !wrote {
        println!("nazm: nothing written; pass --apply to make these edits");
    }
    println!(
        "nazm: {applied} edit{} applied, {remaining} diagnostic{} remaining",
        if applied == 1 { "" } else { "s" },
        if remaining == 1 { "" } else { "s" }
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    //! One test per reason an edit is not applied, each reaching its own branch: the reason
    //! is the exact one reported, and what was written — or that nothing was — is checked.

    use std::cell::RefCell;
    use std::io;

    use super::*;

    const FILE: &str = "prog.nz";
    const TEXT: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n";

    /// An automatic edit to [`FILE`] replacing `start..end`, expecting what [`TEXT`] has
    /// there.
    fn edit(start: u32, end: u32, replacement: &str) -> Edit {
        Edit {
            schema: SCHEMA,
            code: "N0310".to_owned(),
            file: FILE.to_owned(),
            start,
            end,
            replacement: replacement.to_owned(),
            description: "d".to_owned(),
            applicability: Applicability::Automatic,
            precondition: "p".to_owned(),
            expected: TEXT
                .get(start as usize..end as usize)
                .unwrap_or_default()
                .to_owned(),
            applied: false,
            skipped: None,
        }
    }

    /// `write_back_with` over [`TEXT`], returning what was written, if anything.
    fn run(edits: &mut [Edit], readable: bool, writable: bool) -> Option<String> {
        let written = RefCell::new(None);
        write_back_with(
            edits,
            |_| match readable {
                true => Ok(TEXT.to_owned()),
                false => Err(io::Error::other("unreadable")),
            },
            |_, t| {
                *written.borrow_mut() = Some(t.to_owned());
                match writable {
                    true => Ok(()),
                    false => Err(io::Error::other("unwritable")),
                }
            },
        );
        written.into_inner()
    }

    fn at(needle: &str) -> u32 {
        u32::try_from(TEXT.find(needle).unwrap()).unwrap()
    }

    #[test]
    fn a_fix_that_needs_review_is_collected_and_never_applied() {
        let map = SourceMap::single(FILE.to_owned(), TEXT.to_owned());
        let span = nazm_span::Span::new(nazm_span::FileId::ROOT, 0, 0);
        let d = Diagnostic::error(nazm_diag::codes::TYPE_MISMATCH, span, "m").fix_with_review(
            span,
            ";",
            "discard the value",
            "the value was meant to be discarded",
        );
        let mut edits = collect(&[d], &map);
        assert_eq!(edits[0].skipped, Some("it needs review"));
        assert_eq!(run(&mut edits, true, true), None, "nothing to write");
        assert!(!edits[0].applied);
    }

    #[test]
    fn an_unreadable_file_is_left_alone() {
        let mut edits = [edit(at("x ="), at("x =") + 1, "mut x")];
        assert_eq!(run(&mut edits, false, true), None);
        assert_eq!(edits[0].skipped, Some("the file could not be read"));
        assert!(!edits[0].applied);
    }

    #[test]
    fn of_two_overlapping_edits_the_later_is_applied_and_the_earlier_skipped() {
        // `let x` — one edit over `let x`, one over `x`: the later in the file wins.
        let x = at("x =");
        let mut edits = [edit(x - 4, x + 1, "let mut x"), edit(x, x + 1, "mut x")];
        let written = run(&mut edits, true, true).unwrap();
        assert_eq!(
            edits[0].skipped,
            Some("it overlaps an edit already applied in this run")
        );
        assert!(!edits[0].applied);
        assert!(edits[1].applied);
        assert_eq!(written, TEXT.replacen("let x", "let mut x", 1));
    }

    #[test]
    fn a_span_outside_the_file_is_not_a_place_to_edit() {
        let len = u32::try_from(TEXT.len()).unwrap();
        let mut edits = [edit(len, len + 3, "!")];
        assert_eq!(run(&mut edits, true, true), None);
        assert_eq!(
            edits[0].skipped,
            Some("the span does not name a range of this file")
        );
    }

    #[test]
    fn a_span_inside_a_character_is_not_a_place_to_edit() {
        // `é` is two bytes: offset 1 is inside it.
        let text = "fn é() -> Int { 1 }\n";
        let mut e = edit(0, 0, "!");
        e.start = 4;
        e.end = 5;
        e.expected = String::new();
        let written = RefCell::new(None);
        let mut edits = [e];
        write_back_with(
            &mut edits,
            |_| Ok(text.to_owned()),
            |_, t| {
                *written.borrow_mut() = Some(t.to_owned());
                Ok(())
            },
        );
        assert_eq!(written.into_inner(), None);
        assert_eq!(
            edits[0].skipped,
            Some("the span does not name a range of this file")
        );
    }

    #[test]
    fn an_edit_whose_bytes_changed_is_refused() {
        let x = at("x =");
        let mut e = edit(x, x + 1, "mut x");
        // The checker saw `y` here; the file says `x`.
        e.expected = "y".to_owned();
        let mut edits = [e];
        assert_eq!(run(&mut edits, true, true), None, "nothing is written");
        assert_eq!(
            edits[0].skipped,
            Some("the file changed since it was checked")
        );
        assert!(!edits[0].applied);
    }

    #[test]
    fn an_edit_that_breaks_the_parse_is_rolled_back() {
        let x = at("x =");
        let mut edits = [edit(x, x + 1, "{{{")];
        assert_eq!(run(&mut edits, true, true), None, "nothing is written");
        assert_eq!(
            edits[0].skipped,
            Some("applying it produced a file that does not parse")
        );
        assert!(!edits[0].applied);
    }

    #[test]
    fn a_file_that_cannot_be_written_is_not_claimed_applied() {
        let x = at("x =");
        let mut edits = [edit(x, x + 1, "mut x")];
        let attempted = run(&mut edits, true, false);
        assert_eq!(attempted, Some(TEXT.replacen("let x", "let mut x", 1)));
        assert_eq!(edits[0].skipped, Some("the file could not be written"));
        assert!(!edits[0].applied);
    }
}
