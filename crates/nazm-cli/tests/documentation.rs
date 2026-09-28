//! `nazm docs` against the real binary: the index and a section of Nazm's own
//! documentation, their channels and exit statuses, and the same bytes from any copy of the
//! repository. N32.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn nazm(args: &[&str]) -> Run {
    let out = Command::new(NAZM).args(args).output().expect("run nazm");
    Run {
        code: out.status.code(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// The one object on stdout, with nothing on stderr.
fn json(args: &[&str]) -> (Value, Option<i32>) {
    let r = nazm(args);
    assert_eq!(r.stdout.lines().count(), 1, "one line: {args:?}");
    assert_eq!(r.stderr, "", "nothing on stderr: {args:?}");
    (serde_json::from_str(&r.stdout).unwrap(), r.code)
}

/// A copy of the corpus's documents, and nothing else, under a fresh directory.
fn copy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = nazm_docs::default_root();
    for d in nazm_docs::CORPUS {
        let to = dir.path().join(d.path);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(root.join(d.path), to).unwrap();
    }
    dir
}

fn at(root: &Path) -> String {
    root.to_str().unwrap().to_owned()
}

#[test]
fn the_index_and_a_section_are_one_object_each_on_stdout() {
    let (index, code) = json(&["docs", "--index", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(index["schema"], "nazm.docs-index/1");
    assert_eq!(index["status"], "available");
    let state = index["state"].as_str().unwrap().to_owned();
    // No body in an index.
    assert!(!index.to_string().contains("\"body\""));
    let (s, code) = json(&[
        "docs",
        "--section",
        "goals:G78",
        "--state",
        &state,
        "--json",
    ]);
    assert_eq!(code, Some(0));
    assert_eq!(s["schema"], "nazm.docs-section/1");
    assert_eq!(s["authority"], "goal");
    assert!(s["body"]
        .as_str()
        .unwrap()
        .starts_with("## G78 — Documentation Token Efficiency\n"));
    // Refusals are objects too, on stdout, with exit 1.
    for (args, status) in [
        (
            vec!["docs", "--section", "goals:G999", "--json"],
            "not_found",
        ),
        (
            vec!["docs", "--section", "../README.md", "--json"],
            "not_found",
        ),
        (
            vec!["docs", "--section", "/etc/passwd", "--json"],
            "not_found",
        ),
        (
            vec![
                "docs",
                "--section",
                "goals:G78",
                "--state",
                &"0".repeat(64),
                "--json",
            ],
            "stale_state",
        ),
        (
            vec!["docs", "--index", "--document", "nowhere", "--json"],
            "not_found",
        ),
    ] {
        let (v, code) = json(&args);
        assert_eq!(v["status"], status, "{args:?}");
        assert_eq!(code, Some(1), "{args:?}");
    }
}

#[test]
fn a_section_for_a_person_is_its_text_exactly() {
    let (s, _) = json(&["docs", "--section", "spec:generics", "--json"]);
    let plain = nazm(&["docs", "--section", "spec:generics"]);
    assert_eq!(plain.code, Some(0));
    assert_eq!(plain.stdout, s["body"].as_str().unwrap());
    let file = std::fs::read_to_string(nazm_docs::default_root().join("docs/spec.md")).unwrap();
    assert!(file.contains(&plain.stdout));
    // The index for a person: one id and title per section.
    let (index, _) = json(&["docs", "--index", "--json"]);
    let listed = nazm(&["docs", "--index"]);
    assert_eq!(
        listed.stdout.lines().count(),
        usize::try_from(index["corpus"]["sections"].as_u64().unwrap()).unwrap()
    );
    assert!(listed
        .stdout
        .lines()
        .any(|l| l == "goals:G78\tG78 — Documentation Token Efficiency"));
}

#[test]
fn one_documents_index_is_that_documents_sections() {
    let (all, _) = json(&["docs", "--index", "--json"]);
    let (spec, code) = json(&["docs", "--index", "--document", "spec", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(spec["documents"].as_array().unwrap().len(), 1);
    assert_eq!(spec["documents"][0]["authority"], "normative");
    assert_eq!(spec["state"], all["state"]);
    let whole = all["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == "spec")
        .unwrap()
        .clone();
    assert_eq!(spec["documents"][0], whole);
}

#[test]
fn any_copy_of_the_same_documents_gives_the_same_bytes() {
    let (one, two) = (copy(), copy());
    for args in [
        vec!["docs", "--index", "--json"],
        vec!["docs", "--section", "architecture:7.33", "--json"],
        vec!["docs", "--section", "grammar:expression", "--json"],
    ] {
        let here = nazm(&args).stdout;
        let a = nazm(&[&args[..], &["--root", &at(one.path())]].concat()).stdout;
        let b = nazm(&[&args[..], &["--root", &at(two.path())]].concat()).stdout;
        assert_eq!(a, b, "{args:?}");
        assert_eq!(a, here, "{args:?}");
        assert!(!a.contains(&at(one.path())), "{args:?}");
    }
}

#[test]
fn a_changed_document_is_a_new_state_and_a_broken_corpus_is_unavailable() {
    let dir = copy();
    let root = at(dir.path());
    let (before, _) = json(&["docs", "--index", "--json", "--root", &root]);
    let state = before["state"].as_str().unwrap().to_owned();
    let spec = dir.path().join("docs/spec.md");
    let text = std::fs::read_to_string(&spec).unwrap();
    let reworded = "### `while` is a statement, and there is still no loop value\n";
    std::fs::write(
        &spec,
        text.replace(
            "### `while` is a statement, and there is no loop value\n",
            reworded,
        ),
    )
    .unwrap();
    let (s, code) = json(&[
        "docs",
        "--section",
        "spec:while",
        "--state",
        &state,
        "--json",
        "--root",
        &root,
    ]);
    assert_eq!((s["status"].as_str(), code), (Some("stale_state"), Some(1)));
    // Without a state, the current corpus answers: the anchored id survived the rewording.
    let (s, _) = json(&["docs", "--section", "spec:while", "--json", "--root", &root]);
    assert_eq!(s["title"], reworded.trim_start_matches("### ").trim_end());
    // Rewording the heading a link names breaks the link, and the corpus says so.
    std::fs::write(
        &spec,
        text.replace("### `return`\n", "### `return` again\n"),
    )
    .unwrap();
    let (i, code) = json(&["docs", "--index", "--json", "--root", &root]);
    assert_eq!((i["status"].as_str(), code), (Some("unavailable"), Some(1)));
    assert_eq!(i["problems"][0]["kind"], "broken_link");
    // A directory with no documentation.
    let empty = tempfile::tempdir().unwrap();
    let (i, _) = json(&["docs", "--index", "--json", "--root", &at(empty.path())]);
    assert_eq!(i["status"], "unavailable");
    assert_eq!(
        i["problems"].as_array().unwrap().len(),
        nazm_docs::CORPUS.len()
    );
    assert!(i["problems"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["kind"] == "missing_document"));
}

#[test]
fn an_ambiguous_request_is_refused_by_the_parser() {
    for args in [
        vec!["docs"],
        vec!["docs", "--json"],
        vec!["docs", "--index", "--section", "spec"],
        vec!["docs", "--index", "--state", "x"],
        vec!["docs", "--section", "spec", "--document", "spec"],
        vec!["docs", "--section", "spec", "--path", "docs/spec.md"],
    ] {
        assert_eq!(nazm(&args).code, Some(2), "{args:?}");
    }
}
