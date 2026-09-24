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

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
/// Supported keywords, and deliberately no others: `$ref` (to `#/$defs/*` only), `type`,
/// `const`, `enum`, `required`, `properties`, `additionalProperties: false`, `items`,
/// `minimum`, `pattern` (as a literal prefix/suffix-free substring check on the two
/// patterns these schemas use). An unrecognised keyword is an error, so a schema cannot
/// quietly assert something this does not check.
fn validate(value: &Value, node: &Value, root: &Value, at: &str, problems: &mut Vec<String>) {
    let Some(map) = node.as_object() else {
        problems.push(format!("{at}: schema node is not an object"));
        return;
    };

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
        );
        if !known {
            problems.push(format!("{at}: the test does not check the keyword {key:?}"));
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
    // The only pattern these schemas use is the diagnostic code's. Checked for what it
    // means rather than by a regex engine, and asserted to be that pattern so a new one
    // cannot slip through unchecked.
    if let Some(Value::String(p)) = map.get("pattern") {
        assert_eq!(p, "^N[0-9]{4}$", "{at}: the test only knows one pattern");
        let ok = value.as_str().is_some_and(|s| {
            s.len() == 5 && s.starts_with('N') && s[1..].bytes().all(|b| b.is_ascii_digit())
        });
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
