//! `--summary-json` for `nazm check`, `nazm build` and `nazm test`, against the real binary.
//! N31.
//!
//! A summary is compared with what the same command says without it: the same exit status,
//! the same diagnostics by code and place, and — for tests — the same verdicts. Every
//! summary is one line on stdout and nothing on stderr.

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

fn nazm(dir: &Path, args: &[&str]) -> Run {
    let out = Command::new(NAZM)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run nazm");
    Run {
        code: out.status.code(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// The summary, checked to be exactly one line on stdout and nothing on stderr.
fn summary(dir: &Path, args: &[&str]) -> (Value, Option<i32>) {
    let r = nazm(dir, args);
    assert_eq!(r.stdout.lines().count(), 1, "one line: {:?}", r.stdout);
    assert_eq!(r.stderr, "", "nothing on stderr: {args:?}");
    (serde_json::from_str(&r.stdout).unwrap(), r.code)
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let p = dir.path().join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }
    dir
}

/// Every `nazm.diagnostic/1` the command prints with `--json`, by code.
fn codes_of(r: &Run) -> Vec<String> {
    r.stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| v["code"].as_str().unwrap().to_owned())
        .collect()
}

fn ref_codes(s: &Value) -> Vec<String> {
    s["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

const OK: &str = "fn main() -> Int { 1 }\n";
const TYPED: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x + true\n}\n";
const SYNTAX: &str = "fn main() -> Int { 1 +  }\n";
const NO_MAIN: &str = "fn helper() -> Int { 1 }\n";

#[test]
fn a_check_summary_is_the_checks_outcome_and_its_diagnostics_by_reference() {
    for (what, text, status) in [
        ("clean", OK, "success"),
        ("type errors", TYPED, "rejected"),
        ("a syntax error", SYNTAX, "rejected"),
        ("no main", NO_MAIN, "rejected"),
    ] {
        let dir = project(&[("main.nz", text)]);
        let plain = nazm(dir.path(), &["check", "--json", "main.nz"]);
        let (s, code) = summary(dir.path(), &["check", "--summary-json", "main.nz"]);
        assert_eq!(s["schema"], "nazm.command-summary/1", "{what}");
        assert_eq!(s["command"], "check");
        assert_eq!(s["status"], status, "{what}: {s}");
        // The exit status is the check's, with or without a summary.
        assert_eq!(code, plain.code, "{what}");
        assert_eq!(
            s["exit_code"].as_i64().and_then(|c| i32::try_from(c).ok()),
            code
        );
        // Every diagnostic the check reported, and no other.
        assert_eq!(sorted(ref_codes(&s)), sorted(codes_of(&plain)), "{what}");
        assert_eq!(s["counts"]["diagnostics"], ref_codes(&s).len());
        // No prose: none of the check's messages or help reaches the summary.
        for line in plain.stderr.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            for prose in [&v["message"], &v["help"]] {
                if let Some(p) = prose.as_str() {
                    assert!(!s.to_string().contains(p), "{what}: {p:?}");
                }
            }
        }
        assert_eq!(s["artifact"], Value::Null);
    }
    // Clean is no diagnostics, and the summary still says what the check concluded.
    let dir = project(&[("main.nz", OK)]);
    let (s, _) = summary(dir.path(), &["check", "--summary-json", "main.nz"]);
    assert_eq!(s["diagnostics"], serde_json::json!([]));
}

#[test]
fn a_diagnostic_n30_indexes_carries_its_id_and_one_only_the_command_reports_does_not() {
    let dir = project(&[("main.nz", "fn helper() -> Int { 1 + true }\n")]);
    let (s, _) = summary(dir.path(), &["check", "--summary-json", "main.nz"]);
    let refs = s["diagnostics"].as_array().unwrap();
    // In N30's canonical order: a diagnostic with no place first, then by place.
    assert_eq!(ref_codes(&s), ["N0204", "N0304"]);
    // `N0204`, the program's missing `main`, is the command's alone: no N30 id is given.
    let no_main = refs
        .iter()
        .find(|r| r["code"] == "N0204")
        .expect("N0204 kept");
    assert_eq!(no_main["scope"], "command");
    assert_eq!(no_main["id"], Value::Null);
    // The type error is N30's, with N30's id in N30's state: detail answers it.
    let typed = refs.iter().find(|r| r["code"] == "N0304").unwrap();
    assert_eq!(typed["scope"], "index");
    let id = typed["id"].as_str().unwrap();
    let state = s["state"].as_str().unwrap();
    let index: Value = serde_json::from_str(
        &nazm(dir.path(), &["diagnostics", "--json", "--root", "main.nz"]).stdout,
    )
    .unwrap();
    assert_eq!(index["state"]["source"], state);
    let entry = index["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id)
        .unwrap();
    assert_eq!(entry["primary"], typed["primary"]);
    let detail: Value = serde_json::from_str(
        &nazm(
            dir.path(),
            &[
                "diagnostics",
                "--json",
                "--root",
                "main.nz",
                "--detail",
                id,
                "--state",
                state,
            ],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(detail["status"], "available");
    assert_eq!(detail["diagnostic"]["code"], "N0304");
    // No reference in the index universe lacks an id, and none outside it has one.
    for r in refs {
        assert_eq!(r["id"].is_null(), r["scope"] == "command", "{r}");
    }
}

#[test]
fn an_unreadable_root_is_its_own_status_and_keeps_its_diagnostic() {
    let dir = project(&[]);
    let plain = nazm(dir.path(), &["check", "--json", "missing.nz"]);
    let (s, code) = summary(dir.path(), &["check", "--summary-json", "missing.nz"]);
    assert_eq!(code, plain.code);
    assert_eq!(s["status"], "unreadable");
    assert_eq!(sorted(ref_codes(&s)), sorted(codes_of(&plain)));
    assert_eq!(s["state"], Value::Null);
    assert_eq!(s["root"], Value::Null);
    let broken = project(&[("main.nz", "use \"gone.nz\";\nfn main() -> Int { 1 }\n")]);
    let (s, code) = summary(broken.path(), &["check", "--summary-json", "main.nz"]);
    assert_eq!(code, Some(3));
    assert_eq!(s["status"], "unreadable");
    assert!(!s["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn a_build_summary_is_the_builds_outcome_and_absence_of_diagnostics_is_not_success() {
    // Success: the executable written, and named as the build names it.
    let dir = project(&[("main.nz", OK)]);
    let plain = nazm(dir.path(), &["build", "main.nz", "-o", "plain"]);
    assert_eq!(plain.code, Some(0), "{}", plain.stderr);
    let (s, code) = summary(
        dir.path(),
        &["build", "--summary-json", "main.nz", "-o", "prog"],
    );
    assert_eq!(code, Some(0));
    assert_eq!(s["command"], "build");
    assert_eq!(s["status"], "success");
    assert_eq!(s["artifact"], serde_json::json!({"path": "prog"}));
    assert!(dir.path().join("prog").exists());
    assert_eq!(plain.stdout.trim(), "plain");

    // Rejected by the checker, and by the backend: diagnostics, and nothing written.
    for (text, code_wanted) in [
        (TYPED, "N0304"),
        (NO_MAIN, "N0204"),
        (
            "fn f(n: Int) -> Int { if n == 0 { 0 } else { f(n - 1) } }\nfn main() -> Int { f(3) }\n",
            "N0101",
        ),
    ] {
        let dir = project(&[("main.nz", text)]);
        let plain = nazm(dir.path(), &["build", "--json", "main.nz", "-o", "p"]);
        let (s, code) = summary(dir.path(), &["build", "--summary-json", "main.nz", "-o", "q"]);
        assert_eq!(code, plain.code);
        assert_eq!(s["status"], "rejected", "{s}");
        assert!(ref_codes(&s).contains(&code_wanted.to_owned()), "{s}");
        assert_eq!(sorted(ref_codes(&s)), sorted(codes_of(&plain)));
        assert_eq!(s["artifact"], Value::Null);
        assert!(!dir.path().join("q").exists());
    }
    // The backend's refusal of a construct is the command's: no N30 id.
    let rec = project(&[(
        "main.nz",
        "fn f(n: Int) -> Int { if n == 0 { 0 } else { f(n - 1) } }\nfn main() -> Int { f(3) }\n",
    )]);
    let (s, _) = summary(rec.path(), &["build", "--summary-json", "main.nz"]);
    let unsupported = s["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["code"] == "N0101")
        .unwrap();
    assert_eq!(unsupported["scope"], "command");

    // Refused with no diagnostic at all: not success.
    let dir = project(&[("main.nz", OK)]);
    let plain = nazm(dir.path(), &["build", "main.nz", "-o", "main.nz"]);
    let (s, code) = summary(
        dir.path(),
        &["build", "--summary-json", "main.nz", "-o", "main.nz"],
    );
    assert_eq!(code, plain.code);
    assert_eq!(code, Some(4));
    assert_eq!(s["status"], "refused");
    assert_eq!(s["diagnostics"], serde_json::json!([]));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("main.nz")).unwrap(),
        OK
    );
}

#[test]
fn ambiguous_flags_are_refused_and_the_existing_outputs_are_unchanged() {
    let dir = project(&[("main.nz", TYPED)]);
    for args in [
        &["check", "--summary-json", "--json", "main.nz"][..],
        &["check", "--summary-json", "--cache-report", "main.nz"],
        &["build", "--summary-json", "--json", "main.nz"],
        &["build", "--summary-json", "--build-report", "main.nz"],
        &["test", "--summary-json", "--json", "main.nz"],
    ] {
        let r = nazm(dir.path(), args);
        assert_eq!(r.code, Some(2), "{args:?}: {}", r.stderr);
        assert_eq!(r.stdout, "", "{args:?}");
    }
    // `--json` is what it was: nazm.diagnostic/1 on stderr, one per line, nothing on stdout.
    let r = nazm(dir.path(), &["check", "--json", "main.nz"]);
    assert_eq!(r.stdout, "");
    assert!(r
        .stderr
        .lines()
        .filter(|l| l.starts_with('{'))
        .all(|l| serde_json::from_str::<Value>(l).unwrap()["schema"] == "nazm.diagnostic/1"));
    // The human rendering is what it was.
    let human = nazm(dir.path(), &["check", "main.nz"]);
    assert!(human.stderr.contains("error[N0304]"), "{}", human.stderr);
    assert_eq!(human.code, Some(1));
}

#[test]
fn the_same_sources_give_the_same_summary_from_any_directory() {
    let a = project(&[("main.nz", TYPED)]);
    let b = project(&[("main.nz", TYPED)]);
    let (first, _) = summary(a.path(), &["check", "--summary-json", "main.nz"]);
    let (again, _) = summary(a.path(), &["check", "--summary-json", "main.nz"]);
    let (moved, _) = summary(b.path(), &["check", "--summary-json", "main.nz"]);
    assert_eq!(first, again);
    assert_eq!(first, moved);
    let text = first.to_string();
    for dir in [a.path(), b.path()] {
        assert!(!text.contains(dir.to_str().unwrap()), "{text}");
    }
}

/// A test corpus: each `(name, program, expected)`.
fn corpus(cases: &[(&str, &str, Option<&str>)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().join("t");
    std::fs::create_dir_all(&t).unwrap();
    for (name, program, expected) in cases {
        std::fs::write(t.join(format!("{name}.nz")), program).unwrap();
        if let Some(e) = expected {
            std::fs::write(t.join(format!("{name}.expected")), e).unwrap();
        }
    }
    dir
}

#[test]
fn a_test_summary_counts_every_verdict_and_names_only_the_cases_that_did_not_pass() {
    let dir = corpus(&[
        ("a_pass", "fn main() -> Int { 1 }\n", Some("1\n")),
        ("b_wrong", "fn main() -> Int { 2 }\n", Some("1\n")),
        ("c_runtime", "fn main() -> Int { 1 / 0 }\n", Some("1\n")),
        ("d_type", "fn main() -> Int { 1 + true }\n", Some("1\n")),
        (
            "e_native",
            "fn f(n: Int) -> Int { if n == 0 { 0 } else { f(n - 1) } }\nfn main() -> Int { f(3) }\n",
            Some("0\n"),
        ),
        ("f_helper", "fn helper() -> Int { 1 }\n", None),
    ]);
    let plain = nazm(dir.path(), &["test", "--json", "t"]);
    let (s, code) = summary(dir.path(), &["test", "--summary-json", "t"]);
    assert_eq!(code, plain.code);
    assert_eq!(s["schema"], "nazm.test-summary/1");
    assert_eq!(s["status"], "failed");
    assert_eq!(s["native"], true);
    // Counts from each case's verdict, as nazm.test/1 gives them.
    let cases: Vec<Value> = plain
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let passed = cases.iter().filter(|c| c["passed"] == true).count();
    assert_eq!(s["counts"]["cases"], cases.len());
    assert_eq!(s["counts"]["passed"], passed);
    assert_eq!(s["counts"]["failed"], cases.len() - passed);
    assert_eq!(s["counts"]["without_expectation"], 1);
    // Only the cases that did not pass, in the order they ran, each with how its legs ended.
    let failures = s["failures"].as_array().unwrap();
    let names: Vec<&str> = failures
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["b_wrong", "c_runtime", "d_type", "e_native"]);
    let legs = |n: &str| {
        let f = failures.iter().find(|f| f["name"] == n).unwrap();
        (
            f["interpreted"].as_str().unwrap().to_owned(),
            f["native"].as_str().unwrap().to_owned(),
        )
    };
    assert_eq!(legs("b_wrong"), ("mismatched".into(), "mismatched".into()));
    // A runtime error and a type error are outputs that did not match: no diagnostic is
    // made of them.
    assert_eq!(legs("c_runtime").0, "mismatched");
    assert_eq!(legs("d_type").0, "mismatched");
    assert_eq!(legs("d_type").1, "build_failed");
    // The interpreter agrees; the compiler refuses recursion.
    assert_eq!(legs("e_native"), ("matched".into(), "build_failed".into()));
    // No passed name and no output.
    let text = s.to_string();
    assert!(!text.contains("a_pass"), "{text}");
    assert!(!text.contains("division"), "{text}");
    assert!(!text.contains("expected"), "{text}");
    // Interpreted only: the native leg was not run, and says so.
    let (i, _) = summary(
        dir.path(),
        &["test", "--summary-json", "--interpret-only", "t"],
    );
    assert_eq!(i["native"], false);
    assert!(i["failures"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["native"] == "not_run"));
    assert_eq!(i["counts"]["failed"], 3);
}

#[test]
fn a_run_with_no_cases_is_refused_as_the_command_refuses_it() {
    let dir = corpus(&[("helper", "fn helper() -> Int { 1 }\n", None)]);
    let plain = nazm(dir.path(), &["test", "t"]);
    let (s, code) = summary(dir.path(), &["test", "--summary-json", "t"]);
    assert_eq!(code, plain.code);
    assert_eq!(code, Some(3));
    assert_eq!(s["status"], "no_cases");
    assert_eq!(s["counts"]["cases"], 0);
    assert_eq!(s["counts"]["without_expectation"], 1);
    let (u, code) = summary(dir.path(), &["test", "--summary-json", "nowhere"]);
    assert_eq!(code, Some(3));
    assert_eq!(u["status"], "unreadable");
}

#[test]
fn a_large_passing_run_is_counts_and_no_names() {
    let mut cases: Vec<(String, String, Option<String>)> = (0..200)
        .map(|i| {
            (
                format!("pass_{i:03}"),
                format!("fn main() -> Int {{ {i} }}\n"),
                Some(format!("{i}\n")),
            )
        })
        .collect();
    cases.push((
        "zz_fail".into(),
        "fn main() -> Int { 0 }\n".into(),
        Some("1\n".into()),
    ));
    let borrowed: Vec<(&str, &str, Option<&str>)> = cases
        .iter()
        .map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_deref()))
        .collect();
    let dir = corpus(&borrowed);
    let full = nazm(dir.path(), &["test", "--json", "--interpret-only", "t"]);
    let (s, _) = summary(
        dir.path(),
        &["test", "--summary-json", "--interpret-only", "t"],
    );
    assert_eq!(s["counts"]["cases"], 201);
    assert_eq!(s["counts"]["passed"], 200);
    assert_eq!(s["failures"].as_array().unwrap().len(), 1);
    assert!(!s.to_string().contains("pass_"));
    let summary_bytes = s.to_string().len();
    assert!(
        summary_bytes * 20 < full.stdout.len(),
        "{summary_bytes} vs {}",
        full.stdout.len()
    );
    eprintln!(
        "201 cases: nazm.test/1 stream {} B, summary {summary_bytes} B",
        full.stdout.len()
    );
}

#[test]
fn a_test_summary_is_the_same_every_run() {
    let dir = corpus(&[
        ("a", "fn main() -> Int { 1 }\n", Some("1\n")),
        ("b", "fn main() -> Int { 2 }\n", Some("1\n")),
    ]);
    let (first, _) = summary(
        dir.path(),
        &["test", "--summary-json", "--interpret-only", "t"],
    );
    let (again, _) = summary(
        dir.path(),
        &["test", "--summary-json", "--interpret-only", "t"],
    );
    assert_eq!(first, again);
}

#[test]
fn the_check_summary_the_mcp_server_gives_is_nazm_check_s_with_or_without_its_cache() {
    for text in [OK, TYPED, SYNTAX, NO_MAIN] {
        let dir = project(&[("main.nz", text)]);
        let root = dir.path().join("main.nz");
        let (cached, _) = summary(dir.path(), &["check", "--summary-json", "main.nz"]);
        let (uncached, _) = summary(
            dir.path(),
            &["check", "--summary-json", "--no-cache", "main.nz"],
        );
        let service = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
                .spawn_scoped(scope, || {
                    serde_json::to_value(nazm_service::summary::check(&root, None)).unwrap()
                })
                .unwrap()
                .join()
                .unwrap()
        });
        assert_eq!(cached, uncached, "{text}");
        assert_eq!(uncached, service, "{text}");
    }
}
