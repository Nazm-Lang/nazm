//! The machine-readable output, checked against the schemas that describe it.
//!
//! # Why a validator and not a golden file
//!
//! A golden file pins one example. What a tool author needs pinned is the *shape*: that
//! `code` is always there, that `applicability` is one of two strings, that no field
//! appeared without being written down. So these tests run the real commands and
//! validate the real output against `schema/*.json` — which means the schema documents
//! are themselves tested, rather than being prose that drifts.
//!
//! # Why the validator is written here
//!
//! It is about sixty lines and it covers exactly the keywords these schemas use. A
//! validator crate would cover the rest of JSON Schema, which nothing here needs, in
//! exchange for a dependency the offline build has to carry. The subset is listed in
//! [`validate`] so a schema that reaches for a keyword outside it fails loudly rather
//! than being silently unchecked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn schema(name: &str) -> Value {
    let path = repo_root().join("schema").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Validate `value` against `node`, resolving `$ref` against `root`.
///
/// Supported keywords, and deliberately no others: `$ref` (to `#/$defs/*` only), `oneOf`, `type`,
/// `const`, `enum`, `required`, `properties`, `additionalProperties: false`, `items`,
/// `minimum`, `pattern` (as a literal prefix/suffix-free substring check on the two
/// patterns these schemas use). An unrecognised keyword is an error, so a schema cannot
/// quietly assert something this does not check.
fn validate(value: &Value, node: &Value, root: &Value, at: &str, problems: &mut Vec<String>) {
    let Some(map) = node.as_object() else {
        problems.push(format!("{at}: schema node is not an object"));
        return;
    };

    // A node whose `type` admits null accepts null and describes nothing further about it:
    // `nazm.context/3`'s sections that do not apply are null, not empty.
    let admits_null = match map.get("type") {
        Some(Value::Array(a)) => a.iter().any(|t| t == "null"),
        Some(Value::String(t)) => t == "null",
        _ => false,
    };
    if value.is_null() && admits_null {
        return;
    }

    if let Some(Value::String(r)) = map.get("$ref") {
        let Some(name) = r.strip_prefix("#/$defs/") else {
            problems.push(format!("{at}: unsupported $ref {r}"));
            return;
        };
        match root.get("$defs").and_then(|d| d.get(name)) {
            Some(target) => validate(value, target, root, at, problems),
            None => problems.push(format!("{at}: $ref {r} does not resolve")),
        }
        return;
    }

    for key in map.keys() {
        let known = matches!(
            key.as_str(),
            "$schema"
                | "$id"
                | "$defs"
                | "title"
                | "description"
                | "type"
                | "const"
                | "enum"
                | "required"
                | "properties"
                | "additionalProperties"
                | "items"
                | "minimum"
                | "pattern"
                | "oneOf"
        );
        if !known {
            problems.push(format!("{at}: the test does not check the keyword {key:?}"));
        }
    }

    // `oneOf` (N29's tagged operation): exactly one alternative accepts the value.
    if let Some(Value::Array(options)) = map.get("oneOf") {
        let accepting = options
            .iter()
            .filter(|o| {
                let mut inner = Vec::new();
                validate(value, o, root, at, &mut inner);
                inner.is_empty()
            })
            .count();
        if accepting != 1 {
            problems.push(format!(
                "{at}: {accepting} of {} alternatives accept {value}",
                options.len()
            ));
        }
    }
    if let Some(expected) = map.get("const") {
        if value != expected {
            problems.push(format!("{at}: expected {expected}, found {value}"));
        }
    }
    if let Some(Value::Array(options)) = map.get("enum") {
        if !options.contains(value) {
            problems.push(format!("{at}: {value} is not one of {options:?}"));
        }
    }
    if let Some(t) = map.get("type") {
        let names: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let actual = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        if !names.contains(&actual) {
            problems.push(format!("{at}: expected {names:?}, found {actual}"));
        }
    }
    if let (Some(Value::Number(min)), Some(n)) = (map.get("minimum"), value.as_i64()) {
        if min.as_i64().is_some_and(|m| n < m) {
            problems.push(format!("{at}: {n} is below the minimum"));
        }
    }
    // The only patterns these schemas use are the diagnostic code's and a snapshot
    // digest's (N28). Checked for what they mean rather than by a regex engine, and
    // asserted to be those patterns so a new one cannot slip through unchecked.
    if let Some(Value::String(p)) = map.get("pattern") {
        let ok = match p.as_str() {
            "^N[0-9]{4}$" => value.as_str().is_some_and(|s| {
                s.len() == 5 && s.starts_with('N') && s[1..].bytes().all(|b| b.is_ascii_digit())
            }),
            "^[0-9a-f]{64}$" => value.as_str().is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }),
            // A BLAKE3 digest as the N73 records write it.
            "^blake3:[0-9a-f]{64}$" => value
                .as_str()
                .and_then(|s| s.strip_prefix("blake3:"))
                .is_some_and(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                }),
            other => panic!("{at}: the test does not know the pattern {other:?}"),
        };
        if !ok {
            problems.push(format!("{at}: {value} does not match {p}"));
        }
    }

    if let Some(Value::Object(properties)) = map.get("properties") {
        let Some(object) = value.as_object() else {
            problems.push(format!("{at}: expected an object"));
            return;
        };
        if let Some(Value::Array(required)) = map.get("required") {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    problems.push(format!("{at}: required field {key:?} is missing"));
                }
            }
        }
        if map.get("additionalProperties") == Some(&Value::Bool(false)) {
            for key in object.keys() {
                if !properties.contains_key(key) {
                    problems.push(format!("{at}: {key:?} is not in the schema"));
                }
            }
        }
        for (key, sub) in properties {
            if let Some(v) = object.get(key) {
                validate(v, sub, root, &format!("{at}.{key}"), problems);
            }
        }
    }

    if let Some(items) = map.get("items") {
        let Some(array) = value.as_array() else {
            problems.push(format!("{at}: expected an array"));
            return;
        };
        for (i, v) in array.iter().enumerate() {
            validate(v, items, root, &format!("{at}[{i}]"), problems);
        }
    }
}

fn check_against(name: &str, objects: &[Value]) {
    let root = schema(name);
    let mut problems = Vec::new();
    for (i, v) in objects.iter().enumerate() {
        validate(v, &root, &root, &format!("{name}#{i}"), &mut problems);
    }
    assert!(
        problems.is_empty(),
        "output does not match {name}:\n  {}",
        problems.join("\n  ")
    );
    assert!(!objects.is_empty(), "{name}: nothing was validated");
}

fn write_program(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("prog.nz");
    std::fs::write(&src, body).expect("write");
    (dir, src)
}

fn lines(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|l| l.starts_with('{'))
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect()
}

/// A program provoking one of every diagnostic shape that carries a fix, plus one with
/// no fix and no help, so the optional fields are exercised in both states.
const BROKEN: &str = "\
fn f(mut n: Int) -> Int { n }
fn main() -> Int {
    let x = 1;
    x = 2;
    let mut i = 0;
    while i < 3 { i = i + 1; i }
    y
}
";

#[test]
fn every_diagnostic_matches_the_published_schema() {
    let (dir, path) = write_program(BROKEN);
    let out = Command::new(NAZM)
        .args(["check", "--json", path.to_str().unwrap()])
        .output()
        .expect("run");
    let objects = lines(&String::from_utf8_lossy(&out.stderr));
    assert!(
        objects.len() >= 3,
        "expected several diagnostics, got {}",
        objects.len()
    );
    check_against("nazm.diagnostic-1.json", &objects);

    // Both applicabilities and both states of `help` must actually occur, or the schema
    // is being validated against a sample that never exercises them.
    let fixes: Vec<&Value> = objects
        .iter()
        .filter_map(|o| o.get("fixes")?.as_array())
        .flatten()
        .collect();
    let applicabilities: Vec<&str> = fixes
        .iter()
        .filter_map(|f| f.get("applicability")?.as_str())
        .collect();
    assert!(
        applicabilities.contains(&"automatic") && applicabilities.contains(&"needs_review"),
        "the sample exercises only {applicabilities:?}"
    );
    assert!(
        fixes.iter().all(|f| f
            .get("precondition")
            .and_then(Value::as_str)
            .is_some_and(|p| !p.is_empty())),
        "a fix reached the output with no precondition"
    );
    drop(dir);
}

#[test]
fn every_edit_matches_the_published_schema() {
    let (dir, path) = write_program(BROKEN);
    let out = Command::new(NAZM)
        .args(["fix", "--json", path.to_str().unwrap()])
        .output()
        .expect("run");
    let objects = lines(&String::from_utf8_lossy(&out.stdout));
    check_against("nazm.fix-1.json", &objects);
    assert!(
        objects
            .iter()
            .all(|o| o.get("applied") == Some(&Value::Bool(false))),
        "`nazm fix` without --apply must write nothing"
    );
    drop(dir);
}

#[test]
fn the_capability_inventory_matches_the_published_schema() {
    let out = Command::new(NAZM)
        .args(["capabilities", "--json"])
        .output()
        .expect("run");
    let objects = lines(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(objects.len(), 1, "one object, not a stream");
    check_against("nazm.capabilities-1.json", &objects);
}

/// `nazm test --json` over `corpus`, under a deadline.
///
/// `nazm test` runs every case natively as well as interpreted, and a native program that
/// never finishes is a wrong answer: a miscompiled loop is exactly that. So it fails this
/// test rather than holding the suite until an outer limit ends it with no verdict. The
/// run gets its own process group, and the whole group is killed on the way out, so a
/// looping case is not left running beneath a `nazm` that has already been stopped.
fn test_corpus_with_deadline(corpus: &std::path::Path) -> std::process::Output {
    use std::os::unix::process::CommandExt;
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(600);
    let dir = tempfile::tempdir().expect("tempdir");
    let out_path = dir.path().join("test.out");
    let err_path = dir.path().join("test.err");
    let mut child = Command::new(NAZM)
        .args(["test", "--json"])
        .arg(corpus)
        .stdout(std::fs::File::create(&out_path).expect("stdout file"))
        .stderr(std::fs::File::create(&err_path).expect("stderr file"))
        .process_group(0)
        .spawn()
        .expect("run");
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if started.elapsed() > LIMIT {
            let _ = Command::new("kill")
                .arg("-KILL")
                // `--`: procps `kill` (the container's) parses `-<pgid>` after a signal as an
                // option and signals nothing without it. BSD `kill` accepts both.
                .arg("--")
                .arg(format!("-{}", child.id()))
                .status();
            let _ = child.wait();
            panic!("`nazm test` on the conformance corpus did not finish within {LIMIT:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    std::process::Output {
        status,
        stdout: std::fs::read(&out_path).unwrap_or_default(),
        stderr: std::fs::read(&err_path).unwrap_or_default(),
    }
}

#[test]
fn every_test_case_matches_the_published_schema() {
    let corpus = repo_root().join("compiler/conformance");
    let out = test_corpus_with_deadline(&corpus);
    let objects = lines(&String::from_utf8_lossy(&out.stdout));
    assert!(objects.len() >= 5, "expected the conformance corpus");
    check_against("nazm.test-1.json", &objects);
    assert!(
        objects
            .iter()
            .all(|o| o.get("passed") == Some(&Value::Bool(true))),
        "the conformance corpus should pass: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn the_cache_report_matches_the_published_schema() {
    // Twice, because the interesting fields only move on the second run: a report whose
    // `reused` was always zero would validate against the schema and describe nothing.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("nazm.root"), "").expect("marker");
    std::fs::write(
        dir.path().join("b.nz"),
        "pub fn twice(n: Int) -> Int { n * 2 }\n",
    )
    .expect("b");
    std::fs::write(
        dir.path().join("a.nz"),
        "use \"b.nz\";\nfn main() -> Int { twice(21) }\n",
    )
    .expect("a");

    let run = || {
        let out = Command::new(NAZM)
            .args(["check", "a.nz", "--cache-report"])
            .current_dir(dir.path())
            .output()
            .expect("run");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        lines(&String::from_utf8_lossy(&out.stdout))
    };

    let field = |objects: &[Value], name: &str| {
        objects
            .first()
            .and_then(|o| o.get(name))
            .cloned()
            .unwrap_or(Value::Null)
    };

    let cold = run();
    assert_eq!(cold.len(), 1, "one object, not a stream");
    check_against("nazm.check-report-1.json", &cold);
    assert_eq!(field(&cold, "reused"), Value::from(0));

    let warm = run();
    check_against("nazm.check-report-1.json", &warm);
    // Two project modules and the core prelude.
    assert_eq!(field(&warm, "reused"), Value::from(3));
    assert_eq!(field(&warm, "checked"), Value::from(0));
}

#[test]
fn the_schema_identifier_is_the_one_the_compiler_emits() {
    // The constant in `nazm-diag` and the `const` in the schema document are two places
    // one string is written. This is what stops them being two strings.
    let root = schema("nazm.diagnostic-1.json");
    let declared = root
        .pointer("/properties/schema/const")
        .and_then(Value::as_str)
        .expect("the schema declares its own identifier");
    assert_eq!(declared, nazm_diag::SCHEMA);
    assert_eq!(
        root.get("title").and_then(Value::as_str),
        Some(nazm_diag::SCHEMA),
        "the title and the identifier should be the same string"
    );
}

#[test]
fn the_build_report_matches_the_published_schema() {
    // Cold and warm, because the counters are the whole content and a schema that only
    // ever saw zeros would not be describing the interesting case.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("nazm.root"), "").expect("marker");
    std::fs::write(
        dir.path().join("b.nz"),
        "pub fn twice(n: Int) -> Int { n * 2 }\n",
    )
    .expect("b");
    std::fs::write(
        dir.path().join("a.nz"),
        "use \"b.nz\";\nfn main() -> Int { twice(21) }\n",
    )
    .expect("a");

    let run = |extra: &[&str]| {
        let out = Command::new(NAZM)
            .args(["build", "a.nz", "-o", "prog", "--build-report"])
            .args(extra)
            .current_dir(dir.path())
            .output()
            .expect("run");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text
            .lines()
            .find(|l| l.contains("nazm.build-report/1"))
            .expect("a report")
            .to_owned();
        vec![serde_json::from_str::<Value>(&line).expect("a report")]
    };

    let native = |objects: &[Value], name: &str| {
        objects
            .first()
            .and_then(|o| o.get("native"))
            .and_then(|n| n.get(name))
            .cloned()
            .unwrap_or(Value::Null)
    };

    let cold = run(&[]);
    check_against("nazm.build-report-1.json", &cold);
    assert_eq!(native(&cold, "reused"), Value::from(0));
    assert_eq!(native(&cold, "compiled"), native(&cold, "units"));

    let warm = run(&[]);
    check_against("nazm.build-report-1.json", &warm);
    assert_eq!(native(&warm, "compiled"), Value::from(0));
    assert_eq!(native(&warm, "reused"), native(&warm, "units"));

    // And with the cache off, which must read as a cold build that stored nothing.
    let off = run(&["--no-cache"]);
    check_against("nazm.build-report-1.json", &off);
    assert_eq!(native(&off, "reused"), Value::from(0));
    assert_eq!(native(&off, "stored"), Value::from(0));
}

#[test]
fn a_context_packet_is_nazm_context_1_available_or_not() {
    // A function with callers, callees, dependencies, a diagnostic and a prelude type; a
    // record and an enum, whose calls are not applicable; and each reason there is no packet.
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.nz");
    let text = "use \"lib.nz\";\nstruct P {\n    x: Int,\n}\nenum S {\n    A,\n    B(v: Int),\n}\n\
fn f(p: P, s: S) -> Option[Int] { let y = 1 + \"s\"; Option.Some(value: g(p.x)) }\n\
fn h() -> Int { match f(P(x: 1), S.A()) { Option.Some(value: v) => v, Option.None() => 0, } }\n";
    std::fs::write(&main, text).unwrap();
    std::fs::write(dir.path().join("lib.nz"), "pub fn g(n: Int) -> Int { n }\n").unwrap();
    let run = |file: &Path, byte: usize| -> Value {
        let out = Command::new(NAZM)
            .args(["context", "--json", "--root"])
            .arg(&main)
            .arg("--file")
            .arg(file)
            .args(["--byte", &byte.to_string()])
            .output()
            .expect("run nazm context");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one compact line: {stdout}");
        serde_json::from_str(&stdout).unwrap()
    };
    let at = |needle: &str| text.find(needle).unwrap();
    let packets = vec![
        run(&main, at("f(p")),
        run(&main, at("P {")),
        run(&main, at("S {")),
        run(&main, at("h()")),
        run(&main, at("y =")),
        run(&main, 0),
        run(&main, text.len() + 5),
        run(&dir.path().join("elsewhere.nz"), 0),
    ];
    assert_eq!(packets[0]["status"], "available");
    assert!(!packets[0]["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(packets[1]["callers"]["status"], "not_applicable");
    assert_eq!(packets[4]["reason"], "not_a_target");
    assert_eq!(packets[5]["reason"], "not_a_name");
    assert_eq!(packets[6]["reason"], "out_of_range");
    assert_eq!(packets[7]["reason"], "not_in_compilation");
    check_against("nazm.context-3.json", &packets);
}

#[test]
fn a_snapshot_and_a_delta_are_nazm_snapshot_1_and_nazm_delta_1_available_or_not() {
    // A function with callers, callees, dependencies and a diagnostic; a record and an enum,
    // whose calls are not applicable; each reason there is no snapshot, and several there
    // is no delta.
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.nz");
    let lib = dir.path().join("lib.nz");
    let text = "use \"lib.nz\";\nstruct P {\n    x: Int,\n}\nenum S {\n    A,\n    B(v: Int),\n}\n\
fn f(p: P, s: S) -> Int { let y = 1 + \"s\"; g(p.x) }\n\
fn h() -> Int { f(P(x: 1), S.A()) }\n";
    std::fs::write(&main, text).unwrap();
    std::fs::write(&lib, "pub fn g(n: Int) -> Int { n }\n").unwrap();
    let run = |args: &[&std::ffi::OsStr]| -> (Value, Option<i32>) {
        let out = Command::new(NAZM).args(args).output().expect("run nazm");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one compact line: {stdout}");
        (serde_json::from_str(&stdout).unwrap(), out.status.code())
    };
    let snapshot = |root: &Path| {
        run(&[
            "snapshot".as_ref(),
            "--json".as_ref(),
            "--root".as_ref(),
            root.as_os_str(),
        ])
    };
    let (good, code) = snapshot(&main);
    assert_eq!(code, Some(0));
    assert_eq!(good["status"], "available");
    let baseline = dir.path().join("baseline.json");
    std::fs::write(&baseline, good.to_string()).unwrap();
    let delta = |root: &Path, baseline: &Path| {
        run(&[
            "delta".as_ref(),
            "--json".as_ref(),
            "--root".as_ref(),
            root.as_os_str(),
            "--baseline".as_ref(),
            baseline.as_os_str(),
        ])
    };
    std::fs::write(
        &lib,
        "pub fn g(n: Int) -> Int { n + 1 }\npub fn k() -> Int { 2 }\n",
    )
    .unwrap();
    let (changed, code) = delta(&main, &baseline);
    assert_eq!(code, Some(0));
    assert_eq!(changed["changed"][0]["identity"], "lib.nz::fn g");
    assert_eq!(changed["added"][0]["identity"], "lib.nz::fn k");

    let other = dir.path().join("other.json");
    std::fs::write(&other, "{not json").unwrap();
    let (malformed, code) = delta(&main, &other);
    assert_eq!(code, Some(1));
    assert_eq!(malformed["reason"], "baseline_malformed");
    let (wrong_root, _) = delta(&lib, &baseline);
    assert_eq!(wrong_root["reason"], "baseline_wrong_root");
    std::fs::write(&lib, "pub fn g(n: Int) -> Int { n + }\n").unwrap();
    let (broken, code) = snapshot(&main);
    assert_eq!(code, Some(1));
    assert_eq!(broken["reason"], "incomplete_compilation");
    let (broken_delta, _) = delta(&main, &baseline);
    assert_eq!(broken_delta["reason"], "incomplete_compilation");

    check_against("nazm.snapshot-3.json", &[good, broken]);
    check_against(
        "nazm.delta-3.json",
        &[changed, malformed, wrong_root, broken_delta],
    );
}

#[test]
fn a_patch_is_nazm_patch_1_available_or_not() {
    // A rename of a private function, of a local and of a field; an automatic and a
    // needs-review fix; and refusals: exported, invalid name, no fix, a file outside.
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.nz");
    let lib = dir.path().join("lib.nz");
    let text = "use \"lib.nz\";\nstruct P {\n    x: Int,\n}\nfn g(p: P) -> Int { let y = p.x; y + far() }\nfn main() -> Int { g(P(x: 1)) }\n";
    std::fs::write(&main, text).unwrap();
    std::fs::write(&lib, "pub fn far() -> Int { 1 }\n").unwrap();
    let broken = dir.path().join("broken.nz");
    let broken_text = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    let mut i = 0;\n    while i < 3 {\n        i = i + 1;\n        i\n    }\n    x\n}\n";
    std::fs::write(&broken, broken_text).unwrap();
    let run = |args: &[&std::ffi::OsStr]| -> (Value, Option<i32>) {
        let out = Command::new(NAZM).args(args).output().expect("run nazm");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one compact line: {stdout}");
        (serde_json::from_str(&stdout).unwrap(), out.status.code())
    };
    let rename = |root: &Path, file: &Path, byte: usize, name: &str| {
        run(&[
            "patch".as_ref(),
            "rename".as_ref(),
            "--json".as_ref(),
            "--root".as_ref(),
            root.as_os_str(),
            "--file".as_ref(),
            file.as_os_str(),
            "--byte".as_ref(),
            byte.to_string().as_ref(),
            "--new-name".as_ref(),
            name.as_ref(),
        ])
    };
    let fix = |byte: usize, index: usize| {
        run(&[
            "patch".as_ref(),
            "fix".as_ref(),
            "--json".as_ref(),
            "--root".as_ref(),
            broken.as_os_str(),
            "--file".as_ref(),
            broken.as_os_str(),
            "--byte".as_ref(),
            byte.to_string().as_ref(),
            "--fix".as_ref(),
            index.to_string().as_ref(),
        ])
    };
    let at = |t: &str, needle: &str| t.find(needle).unwrap();
    let (function, code) = rename(&main, &main, at(text, "g(p"), "go");
    assert_eq!(code, Some(0));
    assert_eq!(function["status"], "available", "{function}");
    let (local, _) = rename(&main, &main, at(text, "y ="), "why");
    let (field, _) = rename(&main, &main, at(text, "x: Int"), "ex");
    let (automatic, code) = fix(at(broken_text, "x = 2"), 0);
    assert_eq!(code, Some(0));
    assert_eq!(
        automatic["operation"]["applicability"], "automatic",
        "{automatic}"
    );
    let (review, _) = fix(at(broken_text, "i\n    }"), 0);
    assert_eq!(
        review["operation"]["applicability"], "needs_review",
        "{review}"
    );
    let (exported, code) = rename(&main, &lib, 7, "near");
    assert_eq!(code, Some(1));
    assert_eq!(exported["reason"], "exported");
    let (invalid, _) = rename(&main, &main, at(text, "g(p"), "fn");
    assert_eq!(invalid["reason"], "invalid_name");
    let (none, _) = fix(0, 0);
    assert_eq!(none["reason"], "no_such_fix");
    let (outside, _) = rename(&main, Path::new("/etc/passwd"), 0, "x");
    assert_eq!(outside["reason"], "not_in_compilation");
    check_against(
        "nazm.patch-1.json",
        &[
            function, local, field, automatic, review, exported, invalid, none, outside,
        ],
    );
}

#[test]
fn a_diagnostic_index_and_detail_are_their_schemas_and_detail_is_nazm_check_s_diagnostic() {
    // Type errors, one with an automatic fix; a syntax error with a compiler fix; and the
    // refusals.
    let dir = tempfile::tempdir().expect("tempdir");
    let typed = dir.path().join("typed.nz");
    std::fs::write(
        &typed,
        "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x + true\n}\n",
    )
    .unwrap();
    let syntax = dir.path().join("syntax.nz");
    std::fs::write(&syntax, "fn main() -> Str {\n    \"open\n}\n").unwrap();
    let run = |args: &[&std::ffi::OsStr]| -> (Value, Option<i32>) {
        let out = Command::new(NAZM).args(args).output().expect("run nazm");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one compact line: {stdout}");
        (serde_json::from_str(&stdout).unwrap(), out.status.code())
    };
    let index = |root: &Path| {
        run(&[
            "diagnostics".as_ref(),
            "--json".as_ref(),
            "--root".as_ref(),
            root.as_os_str(),
        ])
    };
    let detail = |root: &Path, id: &str, state: &str| {
        run(&[
            "diagnostics".as_ref(),
            "--json".as_ref(),
            "--root".as_ref(),
            root.as_os_str(),
            "--detail".as_ref(),
            id.as_ref(),
            "--state".as_ref(),
            state.as_ref(),
        ])
    };
    let mut indexes = Vec::new();
    let mut details = Vec::new();
    let mut published = Vec::new();
    for root in [&typed, &syntax] {
        let (i, code) = index(root);
        assert_eq!(code, Some(0));
        let state = i["state"]["source"].as_str().unwrap().to_owned();
        // `nazm check --json`, as it always was: one nazm.diagnostic/1 per line on stderr.
        let check = Command::new(NAZM)
            .args(["check", "--json"])
            .arg(root)
            .output()
            .unwrap();
        let stream: Vec<Value> = String::from_utf8(check.stderr)
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .filter(|v: &Value| v["schema"] == "nazm.diagnostic/1")
            .collect();
        for d in i["diagnostics"].as_array().unwrap() {
            let (v, code) = detail(root, d["id"].as_str().unwrap(), &state);
            assert_eq!(code, Some(0), "{v}");
            let mut diagnostic = v["diagnostic"].clone();
            diagnostic
                .as_object_mut()
                .unwrap()
                .insert("schema".into(), Value::from("nazm.diagnostic/1"));
            assert!(
                stream.contains(&diagnostic),
                "{diagnostic} not in {stream:?}"
            );
            published.push(diagnostic);
            details.push(v);
        }
        let (stale, code) = detail(
            root,
            i["diagnostics"][0]["id"].as_str().unwrap(),
            &"0".repeat(64),
        );
        assert_eq!(code, Some(1));
        assert_eq!(stale["reason"], "stale_state");
        details.push(stale);
        indexes.push(i);
    }
    assert!(
        details.iter().any(|d| d["semantic_patches"] == 1),
        "an automatic fix planned"
    );
    assert!(details
        .iter()
        .any(|d| d["compiler_fixes"] == 1 && d["semantic_patches"] == 0));
    check_against("nazm.diagnostic-index-1.json", &indexes);
    check_against("nazm.diagnostic-detail-1.json", &details);
    check_against("nazm.diagnostic-1.json", &published);
}

#[test]
fn command_and_test_summaries_are_their_schemas() {
    let dir = tempfile::tempdir().expect("tempdir");
    let d = dir.path();
    std::fs::write(d.join("ok.nz"), "fn main() -> Int { 1 }\n").unwrap();
    std::fs::write(d.join("typed.nz"), "fn helper() -> Int { 1 + true }\n").unwrap();
    std::fs::write(d.join("syntax.nz"), "fn main() -> Int { 1 +  }\n").unwrap();
    std::fs::create_dir_all(d.join("t")).unwrap();
    std::fs::write(d.join("t/a.nz"), "fn main() -> Int { 1 }\n").unwrap();
    std::fs::write(d.join("t/a.expected"), "1\n").unwrap();
    std::fs::write(d.join("t/b.nz"), "fn main() -> Int { 2 }\n").unwrap();
    std::fs::write(d.join("t/b.expected"), "1\n").unwrap();
    std::fs::create_dir_all(d.join("none")).unwrap();
    let run = |args: &[&str]| -> Value {
        let out = Command::new(NAZM)
            .args(args)
            .current_dir(d)
            .output()
            .expect("run nazm");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one line: {stdout}");
        serde_json::from_str(&stdout).unwrap()
    };
    let commands = vec![
        run(&["check", "--summary-json", "ok.nz"]),
        run(&["check", "--summary-json", "typed.nz"]),
        run(&["check", "--summary-json", "syntax.nz"]),
        run(&["check", "--summary-json", "missing.nz"]),
        run(&["build", "--summary-json", "ok.nz", "-o", "ok"]),
        run(&["build", "--summary-json", "typed.nz", "-o", "typed"]),
        run(&["build", "--summary-json", "ok.nz", "-o", "ok.nz"]),
    ];
    let statuses: Vec<&str> = commands
        .iter()
        .map(|c| c["status"].as_str().unwrap())
        .collect();
    for s in ["success", "rejected", "unreadable", "refused"] {
        assert!(statuses.contains(&s), "{s} in {statuses:?}");
    }
    check_against("nazm.command-summary-1.json", &commands);
    let tests = vec![
        run(&["test", "--summary-json", "t"]),
        run(&["test", "--summary-json", "--interpret-only", "t"]),
        run(&["test", "--summary-json", "none"]),
        run(&["test", "--summary-json", "nowhere"]),
    ];
    check_against("nazm.test-summary-1.json", &tests);
}

#[test]
fn a_docs_index_and_section_are_their_schemas_in_every_status() {
    let run = |args: &[&str]| -> Value {
        let out = Command::new(NAZM).args(args).output().expect("run nazm");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one line: {args:?}");
        serde_json::from_str(&stdout).unwrap()
    };
    let empty = tempfile::tempdir().expect("tempdir");
    let nowhere = empty.path().to_str().unwrap();
    let index = run(&["docs", "--index", "--json"]);
    let state = index["state"].as_str().unwrap().to_owned();
    let indexes = vec![
        index.clone(),
        run(&["docs", "--index", "--document", "grammar", "--json"]),
        run(&["docs", "--index", "--document", "nowhere", "--json"]),
        run(&["docs", "--index", "--json", "--root", nowhere]),
    ];
    let statuses: Vec<&str> = indexes
        .iter()
        .map(|i| i["status"].as_str().unwrap())
        .collect();
    assert_eq!(
        statuses,
        ["available", "available", "not_found", "unavailable"]
    );
    check_against("nazm.docs-index-1.json", &indexes);

    // Every section of the corpus, and every refusal.
    let mut sections: Vec<Value> = index["documents"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| d["sections"].as_array().unwrap().iter().take(3))
        .map(|s| run(&["docs", "--section", s["id"].as_str().unwrap(), "--json"]))
        .collect();
    sections.push(run(&[
        "docs",
        "--section",
        "grammar:expression",
        "--state",
        &state,
        "--json",
    ]));
    sections.push(run(&["docs", "--section", "spec:nowhere", "--json"]));
    sections.push(run(&[
        "docs",
        "--section",
        "spec",
        "--state",
        &"0".repeat(64),
        "--json",
    ]));
    sections.push(run(&[
        "docs",
        "--section",
        "spec",
        "--json",
        "--root",
        nowhere,
    ]));
    for status in ["available", "not_found", "stale_state", "unavailable"] {
        assert!(sections.iter().any(|s| s["status"] == status), "{status}");
    }
    check_against("nazm.docs-section-1.json", &sections);
}

#[test]
fn a_repository_map_and_a_task_context_are_their_schemas_in_every_status() {
    let run = |args: &[&str]| -> Value {
        let out = Command::new(NAZM).args(args).output().expect("run nazm");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().count(), 1, "one line: {args:?}");
        serde_json::from_str(&stdout).unwrap()
    };
    let empty = tempfile::tempdir().expect("tempdir");
    let nowhere = empty.path().to_str().unwrap();
    let map = run(&["repo", "--map", "--json"]);
    let maps = vec![
        map.clone(),
        run(&["repo", "--map", "--json", "--root", nowhere]),
    ];
    let statuses: Vec<&str> = maps.iter().map(|m| m["status"].as_str().unwrap()).collect();
    assert_eq!(statuses, ["available", "unavailable"]);
    check_against("nazm.repository-map-1.json", &maps);

    let state = map["state"].as_str().unwrap().to_owned();
    let task = |seed: &str, intent: &str, extra: &[&str]| -> Value {
        let mut args = vec![
            "repo", "--task", "--json", "--seed", seed, "--intent", intent,
        ];
        args.extend(extra);
        run(&args)
    };
    let tasks = vec![
        task(
            "def:compiler:module.nz::fn attach_core",
            "edit",
            &["--state", &state],
        ),
        task(
            "def:compiler:analyse.nz::fn check_program",
            "understand",
            &["--max-doc-bytes", "400"],
        ),
        task(
            "def:examples-bad:return_type_mismatch.nz::fn describe",
            "diagnose",
            &[],
        ),
        task(
            "diagnostic:examples-bad:return_type_mismatch.nz:N0300@return_type_mismatch.nz:59-69#0",
            "diagnose",
            &[],
        ),
        task("doc:spec:returns", "understand", &[]),
        task("doc:guide:the-syntax/record", "edit", &[]),
        task("package:nazm-docs", "test", &[]),
        task("package:nazm-repo", "edit", &[]),
        task("target:nazm-docs:test:corpus", "test", &[]),
        task("mutation:selfhost-the-core-is-the-last-module", "edit", &[]),
        task("profile:docs", "test", &[]),
        task("module:compiler:lex.nz", "understand", &[]),
        task("root:examples-bad:bare_return.nz", "understand", &[]),
        task("sources:examples", "edit", &[]),
        task("schema:nazm.context/3", "understand", &[]),
        task(
            "def:compiler:module.nz::fn attach_core",
            "edit",
            &["--state", &"0".repeat(64)],
        ),
        task("def:compiler:module.nz::fn nowhere", "edit", &[]),
        task("/etc/passwd", "edit", &[]),
        task(
            "def:compiler:module.nz::fn attach_core",
            "edit",
            &["--max-entities", "0"],
        ),
        task("package:nazm-docs", "edit", &["--root", nowhere]),
    ];
    for status in [
        "complete",
        "partial",
        "stale_state",
        "not_found",
        "invalid_request",
        "insufficient_budget",
        "unavailable",
    ] {
        assert!(tasks.iter().any(|t| t["status"] == status), "{status}");
    }
    check_against("nazm.task-context-1.json", &tasks);
}

// --- N73 and N74: the build record, the API document, the binding report, the publish plan ---

fn nazm_in(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    let o = Command::new(env!("CARGO_BIN_EXE_nazm"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success() || args.first() == Some(&"bindgen"),
        "nazm {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    o
}

#[test]
fn a_provenance_record_matches_the_published_schema() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("p.nz"), "fn main() -> Int { 1 }\n").unwrap();
    nazm_in(
        d.path(),
        &["build", "p.nz", "-o", "p", "--provenance", "p.json"],
    );
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(d.path().join("p.json")).unwrap()).unwrap();
    check_against("nazm.provenance-1.json", &[v]);
}

#[test]
fn an_api_document_matches_the_published_schema() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("m.nz"),
        "// A point.\npub struct P { x: Int }\npub enum E { A, B(p: P) }\n\
         pub extern \"C\" fn c_add(a: Int, b: Int) -> Int = \"c_add\";\n\
         pub extern \"C\" fn tri(x: Int) -> Int ! {} = \"tri\" { x * 3 }\n\
         pub fn say(io: IoCap) -> Int ! { io } { print(\"x\"); 0 }\nfn main() -> Int { 0 }\n",
    )
    .unwrap();
    let o = nazm_in(d.path(), &["doc", "m.nz"]);
    check_against(
        "nazm.api-doc-2.json",
        &lines(&String::from_utf8_lossy(&o.stdout)),
    );
}

#[test]
fn a_binding_report_matches_the_published_schema() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("m.h"),
        "#include <stdint.h>\nint64_t add(int64_t a, int64_t b);\nint small(int x);\n",
    )
    .unwrap();
    let o = nazm_in(d.path(), &["bindgen", "m.h", "-o", "m.nz"]);
    check_against(
        "nazm.bindgen-1.json",
        &lines(&String::from_utf8_lossy(&o.stdout)),
    );
}

#[test]
fn a_publish_plan_matches_the_published_schema() {
    let d = tempfile::tempdir().unwrap();
    nazm_in(d.path(), &["init", "lib", "--template", "library"]);
    std::fs::create_dir(d.path().join("reg")).unwrap();
    let o = nazm_in(
        d.path(),
        &["publish", "--registry", "reg", "lib", "--dry-run"],
    );
    check_against(
        "nazm.publish-plan-1.json",
        &lines(&String::from_utf8_lossy(&o.stdout)),
    );
}

#[test]
fn resolved_units_and_references_match_the_published_schemas() {
    // N77: the units a check entry stores, and an entity's occurrences read from them.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("nazm.root"), "").expect("write");
    std::fs::write(
        dir.path().join("lib.nz"),
        "pub fn helper(x: Int) -> Int { x + 1 }\npub struct P { a: Int, }\npub enum E { A, B(v: Int), }\n",
    )
    .expect("write");
    std::fs::write(
        dir.path().join("main.nz"),
        "use \"lib.nz\" as l;\nfn main() -> Int { let p = l::P(a: l::helper(1)); match l::E.B(v: p.a) { l::E.A() => 0, l::E.B(v: x) => x } }\n",
    )
    .expect("write");
    let run = |args: &[&str]| {
        let out = Command::new(NAZM)
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("run");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        lines(&String::from_utf8_lossy(&out.stdout))
    };
    let units = run(&["resolve", "main.nz"]);
    assert!(units.len() >= 2);
    check_against("nazm.resolved-1.json", &units);
    let mut refs = Vec::new();
    for def in [
        "lib.nz::fn helper",
        "lib.nz::enum E.B",
        "lib.nz::enum E.B.v",
        "lib.nz::record P.a",
    ] {
        refs.extend(run(&["references", def, "main.nz"]));
    }
    check_against("nazm.references-1.json", &refs);
}

#[test]
fn the_lir_tables_match_the_published_schema_on_every_target_class() {
    // N81: one program with a record of three widths, an enum with a payload and a `Vec`,
    // printed for a 64-bit target and for wasm32.
    let (dir, src) = write_program(
        "struct Q { flag: Bool, name: Str, n: Int, xs: Ints }\n\
         enum S { Num(n: Int), Pair(q: Q, k: Int), Empty }\n\
         fn val(s: S) -> Int { match s { S.Num(n: n) => n, S.Pair(q: q, k: k) => q.n + k, S.Empty() => 0, } }\n\
         fn main() -> Int { let v = vec_new[Str](); vec_push(v, \"a\"); val(S.Pair(q: Q(flag: true, name: \"x\", n: 1, xs: ints_new()), k: 2)) + vec_len(v) }\n",
    );
    let mut objects = Vec::new();
    for target in ["aarch64-apple-darwin", "wasm32-unknown-unknown"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_nazm"))
            .args(["lir", "--target", target])
            .arg(&src)
            .current_dir(dir.path())
            .output()
            .expect("run nazm lir");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        objects.extend(lines(&String::from_utf8_lossy(&out.stdout)));
    }
    assert_eq!(objects.len(), 2);
    check_against("nazm.lir-2.json", &objects);
}

#[test]
fn an_obligation_census_matches_the_published_schema() {
    // N87: contracts, a proved and a checked call, arithmetic, an index and a function value.
    let (dir, src) = write_program(
        "fn half(n: Int) -> Int ! {} requires n >= 0 ensures result * 2 <= n { n / 2 }\n\
         fn ap(f: fn(Int) -> Int, x: Int) -> Int ! {} { f(x) }\n\
         fn main() -> Int { half(8) + half(str_len(\"ab\")) + str_byte(\"a\", 0) + ap(half, 4) }\n",
    );
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nazm"))
        .args(["obligations", "--json"])
        .arg(&src)
        .current_dir(dir.path())
        .output()
        .expect("run nazm obligations");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let objects = lines(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(objects.len(), 1);
    check_against("nazm.obligations-1.json", &objects);
}

#[test]
fn a_runtime_manifest_matches_the_published_schema_for_every_profile() {
    // N82: a hosted, a board and a WebAssembly artifact.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut objects = Vec::new();
    for (name, target) in [
        ("hosted", None),
        ("board", Some("aarch64-unknown-none")),
        ("wasm", Some("wasm32-unknown-unknown")),
    ] {
        let mut c = Command::new(NAZM);
        c.args(["runtime", "build", "-o", name])
            .current_dir(dir.path());
        if let Some(t) = target {
            c.args(["--target", t]);
        }
        let out = c.output().expect("run nazm runtime build");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = std::fs::read_to_string(dir.path().join(name).join("nazm-runtime.json"))
            .expect("manifest");
        objects.extend(lines(&text));
    }
    assert_eq!(objects.len(), 3);
    check_against("nazm.runtime-1.json", &objects);
}
