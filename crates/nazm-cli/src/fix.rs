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

/// The version every `nazm fix` JSON object carries. See `docs/diagnostics.md`.
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
    let Some(first) = loaded(path, json) else {
        return ExitCode::from(exit::NO_SUCH_FILE);
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
            Some(again) => nazm_core::check_map(&again.map, &again.graph),
            None => return ExitCode::from(exit::NO_SUCH_FILE),
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

fn loaded(path: &Path, json: bool) -> Option<load::Loaded> {
    match load::load(path) {
        Ok(f) => Some(f),
        Err((partial, diagnostics)) => {
            crate::emit(&diagnostics, &partial.map, json);
            None
        }
    }
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
        let Ok(original) = std::fs::read_to_string(&path) else {
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
        if text == original || std::fs::write(&path, &text).is_ok() {
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
