//! `docs/user/guide.md`'s syntax reference, generated from `docs/user/grammar.ebnf`.
//!
//! # Why generate rather than write
//!
//! A language guide and a grammar say the same thing twice. Written twice, they disagree
//! within a release — and the guide is the one people read, so the disagreement lands on
//! the reader rather than on the maintainer. Here the grammar is the source, the guide's
//! reference section is derived from it, and `cargo xtask check` fails when the file on
//! disk is not what the generator produces.
//!
//! Only the section between the markers is generated. Everything around it is prose
//! somebody wrote, and regenerating never touches it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub(crate) const BEGIN: &str = "<!-- generated from docs/user/grammar.ebnf: begin -->";
pub(crate) const END: &str = "<!-- generated from docs/user/grammar.ebnf: end -->";

pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// One rule, with the comment written above it and its examples.
struct Rule {
    name: String,
    body: String,
    prose: Vec<String>,
    examples: Vec<String>,
}

/// Read the grammar in source order — the order a reader should meet the rules in, which
/// is why this does not sort.
fn rules(text: &str) -> Vec<Rule> {
    let mut out: Vec<Rule> = Vec::new();
    let mut prose: Vec<String> = Vec::new();
    let mut examples: Vec<String> = Vec::new();
    let mut open: Option<usize> = None;
    let mut in_comment = false;

    for line in text.lines() {
        let trimmed = line.trim();
        // The closer comes off first. Taking the leading `*` first turns ` *)` into
        // `)`, which then survives as a line of prose that is just a bracket.
        let comment_line = |t: &str| {
            t.trim_end_matches("*)")
                .trim_start_matches("(*")
                .trim_start_matches('*')
                .trim()
                .to_owned()
        };

        if in_comment {
            if let Some(rest) = trimmed.strip_prefix("* @example ") {
                examples.push(rest.trim_end_matches("*)").trim().to_owned());
            } else {
                let p = comment_line(trimmed);
                if !p.is_empty() {
                    prose.push(p);
                }
            }
            if trimmed.ends_with("*)") {
                in_comment = false;
            }
            continue;
        }
        if trimmed.starts_with("(*") {
            if let Some(rest) = trimmed.strip_prefix("(* @example ") {
                examples.push(rest.trim_end_matches("*)").trim().to_owned());
            } else {
                let p = comment_line(trimmed);
                if !p.is_empty() {
                    prose.push(p);
                }
            }
            if !trimmed.ends_with("*)") {
                in_comment = true;
            }
            continue;
        }
        // A blank line ends the attachment. A comment separated from a rule by one is
        // not that rule's prose — which is what makes the file's own header a header
        // rather than the first rule's description.
        if trimmed.is_empty() {
            if open.is_none() {
                prose.clear();
                examples.clear();
            }
            continue;
        }

        match open {
            None => {
                if let Some((name, rest)) = trimmed.split_once('=') {
                    out.push(Rule {
                        name: name.trim().to_owned(),
                        body: rest.trim().to_owned(),
                        prose: std::mem::take(&mut prose),
                        examples: std::mem::take(&mut examples),
                    });
                    open = Some(out.len() - 1);
                }
            }
            Some(i) => {
                if let Some(rule) = out.get_mut(i) {
                    rule.body.push(' ');
                    rule.body.push_str(trimmed);
                }
            }
        }
        if trimmed.ends_with(';') {
            open = None;
        }
    }
    out
}

/// The generated section, without the markers.
fn section(text: &str) -> String {
    let rules = rules(text);
    let mut out = String::new();

    // The file's own header comment is the grammar's preamble, not a rule's prose, so it
    // is skipped here: the guide says what it needs to say in its own words above the
    // markers.
    let _ = writeln!(out, "Every rule below is checked against the parser by");
    let _ = writeln!(
        out,
        "`crates/nazm-syntax/tests/grammar.rs`, and every example is a program that parser"
    );
    let _ = writeln!(out, "accepts.\n");
    let _ = writeln!(out, "| Rule | Definition |");
    let _ = writeln!(out, "| --- | --- |");
    for r in &rules {
        let body = r.body.trim_end_matches(';').trim().replace('|', "\\|");
        let _ = writeln!(out, "| `{}` | `{body}` |", r.name);
    }
    out.push('\n');

    for r in &rules {
        let _ = writeln!(out, "### `{}`\n", r.name);
        let _ = writeln!(out, "```ebnf");
        let _ = writeln!(out, "{} = {}", r.name, r.body.trim());
        let _ = writeln!(out, "```\n");
        for p in &r.prose {
            let _ = writeln!(out, "{p}");
        }
        if !r.prose.is_empty() {
            out.push('\n');
        }
        for e in &r.examples {
            let _ = writeln!(out, "```nazm");
            let _ = writeln!(out, "{e}");
            let _ = writeln!(out, "```\n");
        }
    }
    out
}

/// The whole guide, with the generated section spliced into the existing file.
///
/// # Errors
///
/// If the guide is missing, or has lost one of its markers — which would mean writing
/// the generated section into a file with no place for it.
pub(crate) fn render() -> Result<String, String> {
    let grammar = root().join("docs/user/grammar.ebnf");
    let guide = root().join("docs/user/guide.md");
    let text =
        std::fs::read_to_string(&grammar).map_err(|e| format!("{}: {e}", grammar.display()))?;
    let existing =
        std::fs::read_to_string(&guide).map_err(|e| format!("{}: {e}", guide.display()))?;

    let (Some(b), Some(e)) = (existing.find(BEGIN), existing.find(END)) else {
        return Err(format!(
            "{}: the markers {BEGIN} and {END} must both be present",
            guide.display()
        ));
    };
    if e < b {
        return Err(format!(
            "{}: the markers are the wrong way round",
            guide.display()
        ));
    }
    let head = existing.get(..b + BEGIN.len()).unwrap_or_default();
    let tail = existing.get(e..).unwrap_or_default();
    Ok(format!("{head}\n\n{}\n{tail}", section(&text)))
}

/// Whether the guide on disk is what the grammar produces.
///
/// A `Vec` of problems, like every other gate, so `cargo xtask check` reports it the
/// same way — even though this one can only ever have a single problem to report.
pub(crate) fn check() -> Result<(), Vec<String>> {
    let want = render().map_err(|e| vec![e])?;
    let guide = root().join("docs/user/guide.md");
    let have =
        std::fs::read_to_string(&guide).map_err(|e| vec![format!("{}: {e}", guide.display())])?;
    if have == want {
        return Ok(());
    }
    Err(vec![format!(
        "{} is not what docs/user/grammar.ebnf produces. Run `cargo xtask guide` to rewrite it.",
        guide.display()
    )])
}

/// Rewrite the guide from the grammar.
///
/// # Errors
///
/// If either file cannot be read, or the guide cannot be written.
pub(crate) fn write() -> Result<(), String> {
    let text = render()?;
    let guide = root().join("docs/user/guide.md");
    std::fs::write(&guide, text).map_err(|e| format!("{}: {e}", guide.display()))
}
