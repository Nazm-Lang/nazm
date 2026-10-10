//! Gate 3: requirements traceability, `nazm trace` (`docs/systems-domains.md` Part S;
//! `examples/requirements/`).
//!
//! A requirement resolves to the definitions it constrains — by durable identity and semantic
//! digest, with their obligations — and to the tests that verify it, run. A definition renamed
//! away, a test missing and a test failing are each reported by name, never dropped, and fail the
//! command. A definition's identity outlives an edit to its body; its digest does not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{run_bounded, Ran};

use std::path::Path;
use std::process::Command;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

fn example(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/requirements")
            .join(name),
    )
    .unwrap()
}

/// `src` traced against `reqs` in a fresh directory.
fn trace(src: &str, reqs: &str) -> (Ran, serde_json::Value) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("heater.nz"), src).unwrap();
    std::fs::write(d.path().join("requirements.toml"), reqs).unwrap();
    let r = run_bounded(
        Command::new(NAZM).current_dir(d.path()).args([
            "trace",
            "heater.nz",
            "--requirements",
            "requirements.toml",
        ]),
        "nazm trace",
    );
    let v = serde_json::from_str(r.stdout.lines().next().unwrap_or("null")).unwrap();
    (r, v)
}

#[test]
fn each_requirement_reaches_its_definition_obligations_and_test() {
    let (r, v) = trace(&example("heater.nz"), &example("requirements.toml"));
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    assert_eq!(v["schema"], "nazm.trace/1");
    assert_eq!(v["status"], "traced");
    assert_eq!(v["broken"], serde_json::json!([]));
    let first = &v["requirements"][0];
    assert_eq!(first["id"], "HTR-1");
    let def = &first["definitions"][0];
    assert_eq!(def["key"], "heater.nz::fn power");
    assert_eq!(def["digest"].as_str().unwrap().len(), 64);
    // Its contract is an obligation, keyed by the definition: the postcondition, checked.
    assert_eq!(def["obligations"]["checked"], 1);
    assert_eq!(def["obligations"]["each"][0]["kind"], "postcondition");
    assert_eq!(first["tests"][0]["outcome"], "passed");
    assert_eq!(
        v["requirements"][1]["definitions"][0]["key"],
        "heater.nz::fn over"
    );
}

#[test]
fn a_broken_link_is_reported_by_name_and_fails_the_trace() {
    let src = example("heater.nz");
    let reqs = example("requirements.toml");
    // The definition renamed away — its callers with it, the requirement left behind.
    let (r, v) = trace(
        &src.replace("fn over(", "fn tripped(")
            .replace("over(801)", "tripped(801)")
            .replace("!over(800)", "!tripped(800)"),
        &reqs,
    );
    assert_eq!(r.code, Some(1), "{r:?}");
    assert_eq!(v["status"], "broken");
    assert!(
        r.stderr
            .contains("HTR-2: the definition `over` is not in heater.nz"),
        "{}",
        r.stderr
    );
    // A test that fails.
    let (r, _) = trace(&src.replace("t > 800", "t > 801"), &reqs);
    assert_eq!(r.code, Some(1), "{r:?}");
    assert!(
        r.stderr
            .contains("HTR-2: the test `trips_above_eighty` fails"),
        "{}",
        r.stderr
    );
    // A test that is not there.
    let (r, v) = trace(&src, &reqs.replace("power_is_bounded", "power_is_fine"));
    assert_eq!(r.code, Some(1), "{r:?}");
    assert_eq!(v["requirements"][0]["tests"][0]["outcome"], "missing");
    // A requirement that names nothing to verify it.
    let (r, _) = trace(
        &src,
        "schema = \"nazm.requirements/1\"\n[[requirement]]\nid = \"HTR-9\"\ndefinitions = [\"power\"]\n",
    );
    assert_eq!(r.code, Some(1));
    assert!(
        r.stderr.contains("HTR-9: a requirement needs"),
        "{}",
        r.stderr
    );
    // A file that is not a requirements file.
    let (r, _) = trace(&src, "[[requirement]]\nid = \"X\"\n");
    assert_eq!(r.code, Some(1));
    assert!(r.stderr.contains("nazm.requirements/1"), "{}", r.stderr);
}

#[test]
fn a_definitions_identity_outlives_an_edit_and_its_digest_does_not() {
    let reqs = example("requirements.toml");
    let (_, before) = trace(&example("heater.nz"), &reqs);
    let edited = example("heater.nz").replace("demand > 100 { 100 }", "demand >= 100 { 100 }");
    let (r, after) = trace(&edited, &reqs);
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let def = |v: &serde_json::Value| v["requirements"][0]["definitions"][0].clone();
    assert_eq!(def(&before)["key"], def(&after)["key"]);
    assert_ne!(def(&before)["digest"], def(&after)["digest"]);
    // The other requirement's definition, untouched, keeps its digest.
    let other = |v: &serde_json::Value| v["requirements"][1]["definitions"][0]["digest"].clone();
    assert_eq!(other(&before), other(&after));
}
