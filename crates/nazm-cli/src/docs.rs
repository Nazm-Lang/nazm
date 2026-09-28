//! `nazm docs` — Nazm's own documentation by section: the compact index,
//! `nazm.docs-index/1`, or one section's exact text, `nazm.docs-section/1`. N32.
//!
//! Every answer is read from the documentation as it is on disk at this call and dropped.
//! With `--json`, standard output is exactly one object, whatever the answer, and standard
//! error is empty; the exit status is 0 with an answer and 1 without one. Without it, a
//! section is printed as its own text and the index as one id and title per line — and a
//! refusal as its object, indented. `docs/architecture.md` §7.34.

use std::path::Path;
use std::process::ExitCode;

use nazm_docs::{IndexResult, SectionResult};

pub(crate) fn index(root: &Path, document: Option<&str>, json: bool) -> ExitCode {
    let r = nazm_docs::index(root, document);
    let text = match (&r, json) {
        (_, true) => r.to_json(),
        (IndexResult::Available(i), false) => {
            let mut out = String::new();
            for d in &i.documents {
                for s in &d.sections {
                    out.push_str(&s.id);
                    out.push('\t');
                    out.push_str(s.title.as_deref().unwrap_or_default());
                    out.push('\n');
                }
            }
            out.pop();
            out
        }
        (_, false) => pretty(&r).unwrap_or_else(|| r.to_json()),
    };
    println!("{text}");
    exit(matches!(r, IndexResult::Available(_)))
}

pub(crate) fn section(root: &Path, id: &str, state: Option<&str>, json: bool) -> ExitCode {
    let r = nazm_docs::section(root, id, state);
    match (&r, json) {
        (_, true) => println!("{}", r.to_json()),
        // The text exactly as the document has it.
        (SectionResult::Available(s), false) => print!("{}", s.body),
        (_, false) => println!("{}", pretty(&r).unwrap_or_else(|| r.to_json())),
    }
    exit(matches!(r, SectionResult::Available(_)))
}

fn pretty(value: &impl serde::Serialize) -> Option<String> {
    serde_json::to_string_pretty(value).ok()
}

fn exit(answered: bool) -> ExitCode {
    if answered {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
