//! A diagnostic names the file it is about, and an edit lands in that file.
//!
//! # What this is testing, and why it needed testing
//!
//! Until 2026-09-21 a multi-file program was concatenated into one buffer and a span was a
//! pair of offsets into it. The originating file was recovered afterwards by binary-searching
//! the offset — which worked, and meant that *being right about the file* was a property of a
//! lookup rather than of the span. `nazm_span::Span` now carries a [`FileId`], so these
//! properties hold by construction.
//!
//! They are asserted through the binary rather than against the types, because the question
//! is what a caller sees. A test that a struct has a field would pass on a compiler that
//! reports every error against the wrong file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

fn nazm(args: &[&str], dir: &Path) -> (String, String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_nazm"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run nazm");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Write a set of files into a fresh directory and return it.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).expect("write");
    }
    dir
}

#[test]
fn a_diagnostic_in_the_root_names_the_root() {
    let dir = tree(&[("main.nz", "fn main() -> Int { nope }\n")]);
    let (_, stderr, code) = nazm(&["check", "main.nz"], dir.path());
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("N0200"), "{stderr}");
    assert!(
        stderr.contains("main.nz:1:20"),
        "the root's own name and position: {stderr}"
    );
}

#[test]
fn a_diagnostic_in_an_imported_file_names_that_file() {
    // The case the merged buffer had to search for. `helper.nz` is loaded second, so
    // under concatenation its offsets were large and its identity was whichever segment
    // the binary search landed in.
    let dir = tree(&[
        (
            "main.nz",
            "use \"helper.nz\";\nfn main() -> Int { help() }\n",
        ),
        ("helper.nz", "fn help() -> Int { missing }\n"),
    ]);
    let (_, stderr, code) = nazm(&["check", "main.nz"], dir.path());
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("N0200"), "{stderr}");
    assert!(
        stderr.contains("helper.nz:1:20"),
        "the imported file, at its own line 1: {stderr}"
    );
    assert!(
        !stderr.contains("main.nz:1:20"),
        "the error is not in the root: {stderr}"
    );
}

#[test]
fn two_files_with_the_same_text_are_not_confused() {
    // Identical contents, so an offset alone cannot tell them apart and nothing but real
    // identity distinguishes the error's file. `b.nz` is the one with the mistake.
    let dir = tree(&[
        (
            "main.nz",
            "use \"a.nz\";\nuse \"b.nz\";\nfn main() -> Int { one() + two() }\n",
        ),
        ("a.nz", "fn one() -> Int { 1 }\n"),
        ("b.nz", "fn two() -> Int { oops }\n"),
    ]);
    let (_, stderr, code) = nazm(&["check", "main.nz"], dir.path());
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("b.nz:1:19"), "{stderr}");
    assert!(!stderr.contains("a.nz:1:19"), "{stderr}");
}

#[test]
fn a_diamond_import_yields_one_file_not_two() {
    // `left` and `right` both import `shared`. It must be read once: twice would define
    // `shared_fn` twice and produce N0203 for a program that is correct.
    let dir = tree(&[
        (
            "main.nz",
            "use \"left.nz\";\nuse \"right.nz\";\nfn main() -> Int { l() + r() }\n",
        ),
        ("left.nz", "use \"shared.nz\";\npub fn l() -> Int { s() }\n"),
        (
            "right.nz",
            "use \"shared.nz\";\npub fn r() -> Int { s() }\n",
        ),
        ("shared.nz", "pub fn s() -> Int { 21 }\n"),
    ]);
    let (stdout, stderr, code) = nazm(&["run", "main.nz"], dir.path());
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim(), "42");
}

#[test]
fn a_fix_addresses_a_byte_range_in_the_file_it_belongs_to() {
    // The edit path is where getting the file wrong writes to the wrong place. The
    // mutation is in the *imported* file, so a fix computed against a merged offset and
    // applied to the root would corrupt `main.nz`.
    let dir = tree(&[
        (
            "main.nz",
            "use \"helper.nz\";\nfn main() -> Int { bump() }\n",
        ),
        (
            "helper.nz",
            "pub fn bump() -> Int {\n    let n = 1;\n    n = 2;\n    n\n}\n",
        ),
    ]);
    let (stdout, stderr, code) = nazm(&["fix", "--json", "main.nz"], dir.path());
    assert_eq!(code, 1, "assigning to an immutable binding: {stderr}");
    assert!(
        stdout.contains("\"file\":\"helper.nz\"") || stdout.contains("helper.nz"),
        "the edit names the file it belongs to: {stdout}"
    );

    // And the range is local to that file: `helper.nz`'s `let n` is at byte 23 of
    // `helper.nz`, not at 23 plus the length of `main.nz`.
    let helper = std::fs::read_to_string(dir.path().join("helper.nz")).expect("read");
    let expect_at = u32::try_from(helper.find("n = 1").expect("the binding")).expect("fits");
    let needle = format!("\"start\":{expect_at}");
    assert!(
        stdout.contains(&needle),
        "expected a local offset {expect_at} in {stdout}"
    );
}

#[test]
fn applying_that_fix_edits_the_imported_file_and_leaves_the_root_alone() {
    let dir = tree(&[
        (
            "main.nz",
            "use \"helper.nz\";\nfn main() -> Int { bump() }\n",
        ),
        (
            "helper.nz",
            "pub fn bump() -> Int {\n    let n = 1;\n    n = 2;\n    n\n}\n",
        ),
    ]);
    let root_before = std::fs::read_to_string(dir.path().join("main.nz")).expect("read");
    let (_, stderr, _) = nazm(&["fix", "--apply", "main.nz"], dir.path());

    let root_after = std::fs::read_to_string(dir.path().join("main.nz")).expect("read");
    assert_eq!(root_before, root_after, "the root was edited: {stderr}");

    let helper = std::fs::read_to_string(dir.path().join("helper.nz")).expect("read");
    assert!(
        helper.contains("let mut n = 1;"),
        "the imported file got the edit: {helper}"
    );

    // And the program now checks.
    let (_, stderr, code) = nazm(&["check", "main.nz"], dir.path());
    assert_eq!(code, 0, "{stderr}");
}
