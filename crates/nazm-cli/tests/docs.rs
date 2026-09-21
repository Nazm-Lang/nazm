//! Claims the documentation makes that the build can check.
//!
//! `cargo xtask check`'s "doc claims" gate exists for this, and today it checks two
//! things about the benchmark harness's pre-registration status — both of which are
//! no-ops, because the file they are about does not exist. So the gate passes and
//! establishes nothing about the language, which is where this session's claims are.
//!
//! These are the ones a machine can settle. A README that lists a command the binary
//! does not have, or omits one it does, is the drift this project exists to refuse; so
//! is a schema file nothing documents, or a documented target the emitter disagrees
//! with.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

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

/// The text with every run of whitespace collapsed to one space.
///
/// Prose is wrapped, so a sentence a document genuinely contains is split across two
/// lines and an exact substring search misses it. The first version of the target check
/// failed for exactly that reason, on a README that said the right thing.
fn flowed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_readme_lists_exactly_the_commands_the_binary_has() {
    let help = Command::new(NAZM).arg("--help").output().expect("run");
    let help = String::from_utf8_lossy(&help.stdout).into_owned();

    // clap's `Commands:` block, one per line, name first.
    let mut real: BTreeSet<String> = BTreeSet::new();
    let mut inside = false;
    for line in help.lines() {
        if line.starts_with("Commands:") {
            inside = true;
            continue;
        }
        if inside {
            if line.trim().is_empty() {
                break;
            }
            if let Some(name) = line.split_whitespace().next() {
                // `help` is clap's, not this compiler's, and documenting it would be
                // documenting the argument parser.
                if name != "help" {
                    real.insert(name.to_owned());
                }
            }
        }
    }
    assert!(real.len() >= 5, "only found {real:?} in --help");

    // The README's command table: rows beginning `| \`nazm NAME`.
    let readme = read("README.md");
    let mut documented: BTreeSet<String> = BTreeSet::new();
    for line in readme.lines() {
        let Some(rest) = line.trim().strip_prefix("| `nazm ") else {
            continue;
        };
        if let Some(name) = rest.split_whitespace().next() {
            documented.insert(name.trim_end_matches('`').to_owned());
        }
    }

    assert_eq!(
        documented, real,
        "README.md's command table and `nazm --help` disagree.\n  documented: \
         {documented:?}\n  real:       {real:?}"
    );
}

#[test]
fn every_schema_file_is_documented_and_every_documented_schema_exists() {
    let dir = repo_root().join("schema");
    let mut files: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("schema/").flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "json") {
            files.insert(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    assert!(files.len() >= 4, "only {files:?}");

    let doc = read("docs/diagnostics.md");
    for f in &files {
        // `nazm.fix-1.json` is documented under its identifier, `nazm.fix/1`.
        let identifier = f
            .trim_end_matches(".json")
            .rsplit_once('-')
            .map(|(name, major)| format!("{name}/{major}"))
            .unwrap_or_default();
        assert!(
            doc.contains(&identifier),
            "schema/{f} exists and `{identifier}` is not mentioned in \
             docs/diagnostics.md"
        );
    }

    // And each schema agrees with itself: the title is the identifier it stamps on
    // every object.
    for f in &files {
        let text = std::fs::read_to_string(dir.join(f)).expect("read");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let title = value.get("title").and_then(serde_json::Value::as_str);
        let stamped = value
            .pointer("/properties/schema/const")
            .and_then(serde_json::Value::as_str);
        assert_eq!(
            title, stamped,
            "schema/{f}: the title and the `schema` constant differ"
        );
    }
}

#[test]
fn every_example_is_checked_by_a_test() {
    // `examples/` is what a reader is pointed at first, so an example that nothing runs
    // is the worst place for a program that does not work. The list in `tests/build.rs`
    // pairs each with its expected value and runs it; this requires the list to be
    // complete.
    let root = repo_root();
    let mut present: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(root.join("examples"))
        .expect("examples/")
        .flatten()
    {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "nz") {
            present.insert(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    assert!(present.len() >= 4, "only {present:?}");

    let build = read("crates/nazm-cli/tests/build.rs");
    let missing: Vec<&String> = present
        .iter()
        .filter(|f| !build.contains(&format!("examples/{f}")))
        .collect();
    assert!(
        missing.is_empty(),
        "these examples are run by nothing: {missing:?}. Add each to the list in          crates/nazm-cli/tests/build.rs with the value it should produce."
    );
}

#[test]
fn the_documented_target_is_the_one_the_emitter_names() {
    // One sentence, in one place, quoted rather than paraphrased wherever it appears.
    let note = nazm_lir::TARGET_NOTE;
    let target = "aarch64-apple-darwin";
    assert!(note.contains(target), "the emitter's note changed: {note}");
    for file in ["README.md", "docs/delivery.md"] {
        let text = flowed(&read(file));
        assert!(
            text.contains(target),
            "{file} does not name the only verified target"
        );
        assert!(
            text.contains("unverified rather than unsupported"),
            "{file} names the target without the qualification the emitter carries"
        );
    }
}
