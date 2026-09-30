//! Every diagnostic code is either asserted by a test, or named as unprovokable.
//!
//! A code is part of the machine interface: `docs/diagnostics.md` promises that one
//! never changes meaning and is never reused, so each is a commitment. A commitment no
//! test exercises is a claim about behaviour nobody has seen.
//!
//! Some genuinely cannot be provoked from a test — they need the machine to run out of
//! memory or of threads. Those are listed below **with the reason**, because a code
//! nobody tested and a code nobody can test look identical from the outside and only one
//! of them is acceptable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// Codes no test asserts, and why each one cannot be reached from a test.
///
/// Adding to this list is how a code escapes the requirement, so it takes a sentence
/// that has to be true. Removing something from it is what happens when someone finds a
/// way to provoke it.
const UNPROVOKABLE: &[(&str, &str)] = &[
    (
        "N0404",
        "the interpreter or a task could not be given a thread. Reaching it means \
         exhausting the machine's threads, which a test cannot do without deciding \
         what else on the machine fails first",
    ),
    (
        "N0406",
        "an allocation failed. Reaching it means exhausting memory, and a test that \
         did would be measuring the machine rather than the compiler",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn read(path: &str) -> String {
    let p = repo_root().join(path);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every `.rs` file under the workspace's test directories, concatenated.
fn all_tests() -> String {
    let mut out = String::new();
    for dir in [
        "crates/nazm-core/tests",
        "crates/nazm-cli/tests",
        "crates/nazm-syntax/tests",
    ] {
        let Ok(entries) = std::fs::read_dir(repo_root().join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            // Not this file. It names every excused code in `UNPROVOKABLE`, so reading
            // itself would make every excused code look asserted — and the check that
            // a code is not both would fire on its own text.
            if p.file_name().is_some_and(|n| n == "codes.rs") {
                continue;
            }
            if p.extension().is_some_and(|e| e == "rs") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    out.push_str(&text);
                }
            }
        }
    }
    // Unit tests live beside the code they test and assert codes just as usefully — but
    // only the test module counts. The first version of this concatenated whole source
    // files, which contain the `Code::new("N0404")` declarations and every `codes::X`
    // emission site, so every code looked asserted and the check proved nothing. Take
    // the text from `mod tests {` onward and no more.
    for file in [
        "crates/nazm-syntax/src/lexer.rs",
        "crates/nazm-syntax/src/parser.rs",
        "crates/nazm-diag/src/lib.rs",
        "crates/nazm-core/src/check.rs",
        "crates/nazm-core/src/lower.rs",
    ] {
        let text = read(file);
        if let Some(i) = text.find("\nmod tests {") {
            out.push_str(text.get(i..).unwrap_or_default());
        }
    }
    out
}

#[test]
fn every_code_is_asserted_by_a_test_or_named_as_unprovokable() {
    let declared = read("crates/nazm-diag/src/lib.rs");
    let tests = all_tests();

    let mut codes = Vec::new();
    for line in declared.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(": Code = Code::new(\"") else {
            continue;
        };
        let Some(number) = rest.split('"').next() else {
            continue;
        };
        codes.push((name.to_owned(), number.to_owned()));
    }
    assert!(codes.len() > 25, "only found {} codes", codes.len());

    let mut missing = Vec::new();
    for (name, number) in &codes {
        // Either the constant is named in a test, or the number appears in one — a
        // JSON assertion quotes the number rather than the constant.
        let asserted = tests.contains(&format!("codes::{name}")) || tests.contains(number);
        let excused = UNPROVOKABLE.iter().any(|(n, _)| n == number);
        if !asserted && !excused {
            missing.push(format!("{number} ({name})"));
        }
        assert!(
            !(asserted && excused),
            "{number} is listed as unprovokable and is also asserted by a test — remove \
             it from the list"
        );
    }
    assert!(
        missing.is_empty(),
        "these codes are promised and never asserted: {missing:?}. Either assert one, \
         or add it to UNPROVOKABLE with the reason it cannot be reached."
    );
}

#[test]
fn every_unprovokable_code_is_a_real_code_with_a_reason() {
    let declared = read("crates/nazm-diag/src/lib.rs");
    for (number, why) in UNPROVOKABLE {
        assert!(
            declared.contains(&format!("Code::new(\"{number}\")")),
            "{number} is excused and is not a code"
        );
        assert!(
            why.len() > 40,
            "{number}'s reason is too short to be one: {why:?}"
        );
    }
}
