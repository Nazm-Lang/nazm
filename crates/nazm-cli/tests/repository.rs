//! `nazm repo` against the real binary: the repository map and task contexts of this
//! repository, their channels and exit statuses, and the library's bytes. N33.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

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
fn json(args: &[&str]) -> (Value, Option<i32>, String) {
    let r = nazm(args);
    assert_eq!(r.stdout.lines().count(), 1, "one line: {args:?}");
    assert_eq!(r.stderr, "", "nothing on stderr: {args:?}");
    (serde_json::from_str(&r.stdout).unwrap(), r.code, r.stdout)
}

#[test]
fn the_map_and_every_answer_are_one_object_on_stdout_with_their_exit_status() {
    let (map, code, text) = json(&["repo", "--map", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(map["status"], "available");
    // The library's bytes, and the same bytes again.
    let root = nazm_docs::default_root();
    assert_eq!(text.trim_end(), nazm_repo::map(&root).to_json());
    assert_eq!(json(&["repo", "--map", "--json"]).2, text);
    let state = map["state"].as_str().unwrap().to_owned();

    let seed = "def:compiler:module.nz::fn core_module";
    let (v, code, _) = json(&[
        "repo", "--task", "--seed", seed, "--state", &state, "--json",
    ]);
    assert_eq!(
        (v["status"].as_str(), code),
        (Some("complete"), Some(0)),
        "{v}"
    );
    assert_eq!(v["intent"], "edit");
    assert_eq!(v["items"][0]["id"], seed);

    let (v, code, _) = json(&[
        "repo",
        "--task",
        "--intent",
        "understand",
        "--seed",
        seed,
        "--max-entities",
        "1",
        "--json",
    ]);
    assert_eq!(
        (v["status"].as_str(), code),
        (Some("partial"), Some(0)),
        "{v}"
    );
    assert!(!v["omitted"].as_array().unwrap().is_empty());

    for (args, status) in [
        (
            vec!["--seed", seed, "--state", &"0".repeat(64)],
            "stale_state",
        ),
        (
            vec!["--seed", "def:compiler:module.nz::fn no_such"],
            "not_found",
        ),
        (vec!["--seed", "/etc/passwd"], "invalid_request"),
        (vec!["--seed", "../../etc/passwd"], "invalid_request"),
        (
            vec!["--seed", seed, "--max-entities", "0"],
            "insufficient_budget",
        ),
    ] {
        let mut all = vec!["repo", "--task", "--json"];
        all.extend(args);
        let (v, code, _) = json(&all);
        assert_eq!(
            (v["status"].as_str(), code),
            (Some(status), Some(1)),
            "{all:?}: {v}"
        );
    }
    // An empty directory is not a repository.
    let empty = tempfile::tempdir().unwrap();
    let (v, code, _) = json(&[
        "repo",
        "--map",
        "--json",
        "--root",
        empty.path().to_str().unwrap(),
    ]);
    assert_eq!((v["status"].as_str(), code), (Some("unavailable"), Some(1)));
}

#[test]
fn without_json_the_same_object_is_indented() {
    let seed = "doc:spec:returns";
    let (compact, _, _) = json(&[
        "repo",
        "--task",
        "--seed",
        seed,
        "--intent",
        "understand",
        "--json",
    ]);
    let r = nazm(&["repo", "--task", "--seed", seed, "--intent", "understand"]);
    assert_eq!(r.code, Some(0));
    assert!(r.stdout.lines().count() > 1);
    assert_eq!(serde_json::from_str::<Value>(&r.stdout).unwrap(), compact);
}

#[test]
fn an_ambiguous_request_is_refused_by_the_parser() {
    for args in [
        vec!["repo"],
        vec!["repo", "--map", "--task"],
        vec!["repo", "--map", "--seed", "package:nazm-docs"],
        vec!["repo", "--map", "--intent", "edit"],
        vec!["repo", "--map", "--state", "x"],
        vec!["repo", "--map", "--max-entities", "3"],
        vec![
            "repo",
            "--task",
            "--intent",
            "guess",
            "--seed",
            "package:nazm-docs",
        ],
        vec![
            "repo",
            "--task",
            "--seed",
            "package:nazm-docs",
            "--max-entities",
            "-1",
        ],
    ] {
        let r = nazm(&args);
        assert_eq!(r.code, Some(5), "{args:?}");
        assert_eq!(r.stdout, "", "{args:?}");
    }
}
