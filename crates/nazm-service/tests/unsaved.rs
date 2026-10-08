//! Unsaved buffers edited by ranges (N76): an open document's text moves by the edits an
//! editor sends, its revision is reparsed only where an edit reaches, and every answer is
//! the answer a whole-document change would have given.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use nazm_core::__syntax::{EditError, TextEdit};
use nazm_service::{LanguageService, Published};

mod common;
use common::on_the_service_stack;

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

fn edit(start: usize, end: usize, text: &str) -> TextEdit {
    TextEdit {
        start: u32::try_from(start).unwrap(),
        end: u32::try_from(end).unwrap(),
        text: text.to_owned(),
    }
}

/// What a client sees: each document's path, version and diagnostics, by code and span.
type Seen = Vec<(PathBuf, Option<i32>, Vec<(String, u32, u32)>)>;

fn seen(p: &[Published]) -> Seen {
    let mut v: Vec<_> = p
        .iter()
        .map(|p| {
            (
                p.path.clone(),
                p.version,
                p.diagnostics
                    .iter()
                    .map(|d| (d.code.to_string(), d.primary.span.start, d.primary.span.end))
                    .collect(),
            )
        })
        .collect();
    v.sort();
    v
}

const MAIN: &str = "use \"lib.nz\";\nfn helper(x: Int) -> Int { x + 1 }\nfn main() -> Int { helper(lib_value()) }\n";
const LIB: &str = "pub fn lib_value() -> Int { 41 }\n";

#[test]
fn ranged_edits_answer_exactly_as_whole_document_changes_do() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let main = dir.path().join("main.nz");
    on_the_service_stack(|| {
        let mut ranged = LanguageService::new();
        let mut whole = LanguageService::new();
        ranged.open(&main, 1, MAIN.to_owned());
        whole.open(&main, 1, MAIN.to_owned());
        let mut text = MAIN.to_owned();
        let steps: Vec<Vec<TextEdit>> = vec![
            // A type error, then a syntax error, then both fixed, then two edits at once.
            vec![edit(
                text.find("x + 1").unwrap(),
                text.find("x + 1").unwrap() + 1,
                "\"s\"",
            )],
            vec![edit(0, 0, "fn broken( -> Int { 0 }\n")],
            vec![edit(0, "fn broken( -> Int { 0 }\n".len(), "")],
            vec![edit(0, 0, "// top\n"), edit(0, 0, "// first\n")],
        ];
        for (k, step) in steps.into_iter().enumerate() {
            let version = i32::try_from(k).unwrap() + 2;
            for e in &step {
                text.replace_range(e.start as usize..e.end as usize, &e.text);
            }
            let a = ranged.edit(&main, version, &step).unwrap();
            let b = whole.change(&main, version, text.clone());
            assert_eq!(seen(&a), seen(&b), "step {k}");
            assert_eq!(&*ranged.text(&main).unwrap(), text, "step {k}");
            let stats = ranged.parse_stats(&main).unwrap();
            assert!(!stats.full, "step {k}: {stats:?}");
            assert_eq!(ranged.reused_root_parse(&main), Some(true), "step {k}");
            // Every query answers from the same analysis either way.
            let at = u32::try_from(text.find("helper(lib").unwrap()).unwrap();
            assert_eq!(
                ranged.definition(&main, version, at),
                whole.definition(&main, version, at),
                "step {k}"
            );
            assert_eq!(
                ranged.hover(&main, version, at),
                whole.hover(&main, version, at),
                "step {k}"
            );
        }
    });
}

#[test]
fn an_edit_that_does_not_fit_the_text_changes_nothing() {
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let main = dir.path().join("main.nz");
    on_the_service_stack(|| {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.to_owned());
        let past_the_end = MAIN.len() + "// ok\n".len() + 1;
        let refused = s.edit(
            &main,
            2,
            &[edit(0, 0, "// ok\n"), edit(past_the_end, past_the_end, "x")],
        );
        assert_eq!(refused.err(), Some(Some(EditError::OutOfRange)));
        // Neither edit applied, and version 1 is still the current one.
        assert_eq!(&*s.text(&main).unwrap(), MAIN);
        assert!(s.hover(&main, 2, 40).is_none());
        // Not open: nothing to edit.
        assert_eq!(s.edit(&dir.path().join("lib.nz"), 1, &[]).err(), Some(None));
    });
}

#[test]
fn an_imported_document_is_parsed_in_full_in_its_importers_compilation() {
    // `lib.nz` is open and edited by range; `main.nz`'s compilation reads the new text,
    // parses it in full (it is not that compilation's root), and sees the change.
    let dir = project(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let (main, lib) = (dir.path().join("main.nz"), dir.path().join("lib.nz"));
    on_the_service_stack(|| {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.to_owned());
        s.open(&lib, 1, LIB.to_owned());
        let at = LIB.find("41").unwrap();
        let out = s.edit(&lib, 2, &[edit(at, at + 2, "true")]).unwrap();
        let main_diagnostics = out
            .iter()
            .find(|p| p.path == main)
            .map(|p| p.diagnostics.len())
            .unwrap();
        let lib_diagnostics = out
            .iter()
            .find(|p| p.path == lib)
            .map(|p| p.diagnostics.len())
            .unwrap();
        // `lib_value` now returns a `Bool` where it declares an `Int`: an error in `lib`.
        assert!(lib_diagnostics > 0, "{out:?}");
        assert_eq!(main_diagnostics, 0, "{out:?}");
        assert_eq!(s.reused_root_parse(&lib), Some(true));
    });
}
