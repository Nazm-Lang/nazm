//! Semantic snapshots and deltas through the language service. N28.
//!
//! Every snapshot here is read from disk by a fresh service, exactly as `nazm snapshot` and
//! `nazm-mcp` read one, and compared as the JSON it serialises to. A delta is asked of a
//! fresh service too, with the earlier snapshot as data: nothing carries over but that.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_service::LanguageService;
use serde_json::{json, Value};

fn on_the_service_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, f)
            .expect("spawn")
            .join()
            .expect("join")
    })
}

struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, text) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, text).unwrap();
        }
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.path(name), text).unwrap();
    }
    fn root(&self) -> PathBuf {
        self.path("main.nz")
    }
}

fn snapshot_json(root: &Path) -> String {
    let root = root.to_path_buf();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        s.semantic_snapshot(&root, 0).to_json()
    })
}

fn snapshot(root: &Path) -> Value {
    serde_json::from_str(&snapshot_json(root)).unwrap()
}

fn delta(root: &Path, baseline: &Value) -> Value {
    let (root, baseline) = (root.to_path_buf(), baseline.clone());
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        serde_json::from_str(&s.semantic_delta(&root, 0, &baseline).to_json()).unwrap()
    })
}

fn identities(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|d| d["identity"].as_str().unwrap().to_owned())
        .collect()
}

const SECTIONS: [&str; 9] = [
    "source",
    "shape",
    "dependencies",
    "related_types",
    "references",
    "callers",
    "callees",
    "diagnostics",
    "effects",
];

/// The sections a changed definition reports as changed, by name, in the schema's order.
fn changed(delta: &Value, identity: &str) -> Vec<String> {
    let entry = delta["changed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["identity"] == identity)
        .unwrap_or_else(|| panic!("{identity} not changed: {delta}"));
    SECTIONS
        .iter()
        .filter(|k| entry["sections"][**k] == json!(true))
        .map(|k| (*k).to_owned())
        .collect()
}

fn definition<'a>(snapshot: &'a Value, identity: &str) -> &'a Value {
    snapshot["definitions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["identity"] == identity)
        .unwrap_or_else(|| panic!("{identity} not in {snapshot}"))
}

const LIB: &str = "pub fn far() -> Int { helper() }\nfn helper() -> Int { 7 }\n";
const MAIN: &str = "\
use \"lib.nz\";
struct Point {
    x: Int,
    y: Int,
}
enum Shape {
    Dot,
    Circle(radius: Int),
}
fn helper(p: Point) -> Int { p.x }
fn area(s: Shape) -> Int { match s { Shape.Dot() => 0, Shape.Circle(radius: r) => r * r, } }
fn main() -> Int { helper(Point(x: 1, y: 2)) + area(Shape.Dot()) + far() }
";

fn project() -> Project {
    Project::new(&[("main.nz", MAIN), ("lib.nz", LIB)])
}

// ---------------------------------------------------------------------------------------
// The snapshot.
// ---------------------------------------------------------------------------------------

#[test]
fn a_snapshot_is_every_durable_definition_of_the_compilation_by_identity() {
    let p = project();
    let s = snapshot(&p.root());
    assert_eq!(s["schema"], "nazm.snapshot/2");
    assert_eq!(s["status"], "available");
    assert_eq!(s["root"]["module"], "main.nz");
    assert!(s["root"]["compiler"].as_str().unwrap().starts_with("nazm "));
    // Ordered by identity; two modules' `helper` are two definitions.
    assert_eq!(
        identities(&s["definitions"]),
        [
            "lib.nz::fn far",
            "lib.nz::fn helper",
            "main.nz::enum Shape",
            "main.nz::fn area",
            "main.nz::fn helper",
            "main.nz::fn main",
            "main.nz::record Point",
        ]
    );
    assert_eq!(s["untracked"], 0);
    // What is not known is named as not known.
    assert_eq!(
        s["unsupported"],
        json!([
            "capabilities",
            "tests",
            "semantic_history",
            "behavioural_equivalence"
        ])
    );
    let point = definition(&s, "main.nz::record Point");
    assert_eq!(point["kind"], "record");
    let start = MAIN.find("struct Point").unwrap();
    let end = MAIN.find("enum Shape").unwrap() - 1;
    assert_eq!(
        point["declaration"],
        json!({"file": "main.nz", "start": start, "end": end})
    );
    // A record or an enum calls nothing: no callers or callees digest, which is not an
    // empty one.
    assert_eq!(point["digests"]["callers"], Value::Null);
    assert_eq!(point["digests"]["callees"], Value::Null);
    assert_eq!(point["digests"]["effects"], Value::Null);
    let main = definition(&s, "main.nz::fn main");
    for section in [
        "source",
        "shape",
        "dependencies",
        "related_types",
        "references",
        "callers",
        "callees",
        "diagnostics",
        "effects",
    ] {
        let d = main["digests"][section].as_str().unwrap();
        assert_eq!(d.len(), 64, "{section}");
        assert!(d
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }
    // No full packet and no source text: a compact manifest.
    let text = s.to_string();
    assert!(!text.contains("p.x"), "{text}");
    assert!(!text.contains("nazm.context/2"), "{text}");
}

#[test]
fn the_same_sources_give_the_same_bytes_from_any_directory() {
    let a = project();
    let b = project();
    let first = snapshot_json(&a.root());
    // Repeated, in fresh services.
    assert_eq!(first, snapshot_json(&a.root()));
    // Moved: the same project in another directory.
    assert_eq!(first, snapshot_json(&b.root()));
    for dir in [a.dir.path(), b.dir.path()] {
        let host = dir.to_str().unwrap();
        assert!(!first.contains(host), "a host path in {first}");
    }
    // Neither directory is a git repository, and nothing needed one.
    for dir in [a.dir.path(), b.dir.path()] {
        assert!(dir.ancestors().all(|d| !d.join(".git").exists()));
    }
}

#[test]
fn only_the_roots_compilation_is_in_a_snapshot() {
    let p = project();
    // Not imported: not in the compilation, and not in the snapshot.
    p.write("stray.nz", "pub fn stray() -> Int { 1 }\n");
    let s = snapshot(&p.root());
    assert!(!s.to_string().contains("stray"), "{s}");
    // The compilation rooted at lib.nz is another root, without main's definitions.
    let lib = snapshot(&p.path("lib.nz"));
    assert_eq!(lib["root"]["module"], "lib.nz");
    assert_eq!(
        identities(&lib["definitions"]),
        ["lib.nz::fn far", "lib.nz::fn helper"]
    );
}

#[test]
fn a_definition_without_a_durable_key_is_counted_and_never_matched() {
    // `nazm.root` declares `app/` the source root, so `../outside.nz` has no durable key.
    let p = Project::new(&[
        ("app/nazm.root", ""),
        (
            "app/main.nz",
            "use \"../outside.nz\";\nfn main() -> Int { out() }\n",
        ),
        ("outside.nz", "pub fn out() -> Int { 1 }\n"),
    ]);
    let s = snapshot(&p.path("app/main.nz"));
    assert_eq!(s["status"], "available", "{s}");
    assert_eq!(identities(&s["definitions"]), ["main.nz::fn main"]);
    assert_eq!(s["untracked"], 1);
    assert!(!s.to_string().contains("out()"));
}

#[test]
fn a_compilation_that_needed_recovery_has_no_snapshot() {
    let p = Project::new(&[("main.nz", "fn main() -> Int { 1 +  }\n")]);
    assert_eq!(
        snapshot(&p.root()),
        json!({"schema": "nazm.snapshot/2", "status": "unavailable", "reason": "incomplete_compilation"})
    );
    let q = Project::new(&[("main.nz", "use \"missing.nz\";\nfn main() -> Int { 1 }\n")]);
    assert_eq!(snapshot(&q.root())["reason"], "incomplete_compilation");
}

// ---------------------------------------------------------------------------------------
// The delta.
// ---------------------------------------------------------------------------------------

#[test]
fn no_edit_is_no_change_and_a_move_is_not_one_either() {
    let p = project();
    let before = snapshot(&p.root());
    let d = delta(&p.root(), &before);
    assert_eq!(d["schema"], "nazm.delta/2");
    assert_eq!(d["status"], "available");
    assert_eq!(d["added"], json!([]));
    assert_eq!(d["removed"], json!([]));
    assert_eq!(d["changed"], json!([]));
    assert_eq!(d["unchanged"], 7);
    assert_eq!(d["baseline"], json!({"definitions": 7, "untracked": 0}));
    assert_eq!(d["unsupported"], before["unsupported"]);
    // A line above every definition moves all of them and changes none: relationships are
    // digested by identity, never by offset.
    p.write("main.nz", &MAIN.replacen("struct", "// moved\n\nstruct", 1));
    let d = delta(&p.root(), &before);
    assert_eq!(d["changed"], json!([]), "{d}");
    assert_eq!(d["unchanged"], 7);
}

#[test]
fn a_comment_or_a_literal_changes_source_and_nothing_else() {
    let p = project();
    let before = snapshot(&p.root());
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper() }\nfn helper() -> Int {\n    // seven\n    7\n}\n",
    );
    let d = delta(&p.root(), &before);
    assert_eq!(changed(&d, "lib.nz::fn helper"), ["source"]);
    assert_eq!(d["changed"].as_array().unwrap().len(), 1, "{d}");
    // Every recorded section is reported, false where it did not change.
    let entry = &d["changed"][0];
    assert_eq!(
        entry["sections"],
        json!({"source": true, "shape": false, "dependencies": false, "related_types": false,
               "references": false, "callers": false, "callees": false, "diagnostics": false,
               "effects": false})
    );
    // `{ 7 }` to `{ 8 }`: a different program, and only `source` says so — the delta does
    // not claim the sections it records describe behaviour.
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper() }\nfn helper() -> Int { 8 }\n",
    );
    assert_eq!(
        changed(&delta(&p.root(), &before), "lib.nz::fn helper"),
        ["source"]
    );
    assert!(d.get("behaviour").is_none() && d["unsupported"].to_string().contains("behavioural"));
}

#[test]
fn added_removed_and_a_rename_is_both() {
    let p = project();
    let before = snapshot(&p.root());
    let edited = MAIN
        .replace("enum Shape {\n    Dot,\n    Circle(radius: Int),\n}\n", "")
        .replace(
            "fn area(s: Shape) -> Int { match s { Shape.Dot() => 0, Shape.Circle(radius: r) => r * r, } }\n",
            "",
        )
        .replace(" + area(Shape.Dot())", "")
        .replace("fn helper(p: Point)", "fn fresh() -> Int { 3 }\nfn helper(p: Point)");
    p.write("main.nz", &edited);
    let d = delta(&p.root(), &before);
    assert_eq!(identities(&d["added"]), ["main.nz::fn fresh"]);
    assert_eq!(d["added"][0]["kind"], "function");
    assert_eq!(
        identities(&d["removed"]),
        ["main.nz::enum Shape", "main.nz::fn area"]
    );

    // Renaming `lib.nz::fn helper` is its key removed and another added — not a change,
    // and not a rename: that is not inferred.
    let before = snapshot(&p.root());
    p.write(
        "lib.nz",
        "pub fn far() -> Int { assist() }\nfn assist() -> Int { 7 }\n",
    );
    let d = delta(&p.root(), &before);
    assert_eq!(identities(&d["added"]), ["lib.nz::fn assist"]);
    assert_eq!(identities(&d["removed"]), ["lib.nz::fn helper"]);
    assert_eq!(identities(&d["changed"]), ["lib.nz::fn far"]);
    assert!(!d.to_string().contains("rename"));
}

#[test]
fn a_signature_a_field_or_a_variant_changes_shape() {
    let p = project();
    let before = snapshot(&p.root());
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper(1) }\nfn helper(n: Int) -> Int { n }\n",
    );
    let d = delta(&p.root(), &before);
    let helper = changed(&d, "lib.nz::fn helper");
    assert!(helper.contains(&"shape".to_owned()), "{helper:?}");

    let before = snapshot(&p.root());
    p.write(
        "main.nz",
        &MAIN
            .replace("    y: Int,\n", "    y: Int,\n    z: Int,\n")
            .replace("Point(x: 1, y: 2)", "Point(x: 1, y: 2, z: 3)"),
    );
    let d = delta(&p.root(), &before);
    let point = changed(&d, "main.nz::record Point");
    assert!(point.contains(&"shape".to_owned()), "{point:?}");

    let before = snapshot(&p.root());
    let now = std::fs::read_to_string(p.root()).unwrap();
    p.write(
        "main.nz",
        &now.replace("    Dot,\n", "    Dot,\n    Square(side: Int),\n")
            .replace(
                "Shape.Circle(radius: r) => r * r,",
                "Shape.Circle(radius: r) => r * r, Shape.Square(side: s) => s,",
            ),
    );
    let d = delta(&p.root(), &before);
    let shape = changed(&d, "main.nz::enum Shape");
    assert!(shape.contains(&"shape".to_owned()), "{shape:?}");
    // Visibility is part of a declaration's shape.
    let before = snapshot(&p.root());
    let now = std::fs::read_to_string(p.path("lib.nz")).unwrap();
    p.write("lib.nz", &now.replace("fn helper", "pub fn helper"));
    assert_eq!(
        changed(&delta(&p.root(), &before), "lib.nz::fn helper"),
        ["source", "shape"]
    );
}

#[test]
fn a_new_call_changes_the_callee_without_touching_its_source() {
    // `g` begins calling `t`: `t`'s own text is the same, and its references and callers
    // are not.
    let p = Project::new(&[(
        "main.nz",
        "fn t() -> Int { 1 }\nfn u() -> Int { 2 }\nfn g() -> Int { u() }\nfn main() -> Int { g() }\n",
    )]);
    let before = snapshot(&p.root());
    p.write(
        "main.nz",
        "fn t() -> Int { 1 }\nfn u() -> Int { 2 }\nfn g() -> Int { t() }\nfn main() -> Int { g() }\n",
    );
    let d = delta(&p.root(), &before);
    assert_eq!(changed(&d, "main.nz::fn t"), ["references", "callers"]);
    assert_eq!(changed(&d, "main.nz::fn u"), ["references", "callers"]);
    assert_eq!(
        changed(&d, "main.nz::fn g"),
        ["source", "dependencies", "callees"]
    );
    assert_eq!(identities(&d["changed"]).len(), 3, "{d}");

    // A second call to the same function is one more call, and one more reference.
    let before = snapshot(&p.root());
    p.write(
        "main.nz",
        "fn t() -> Int { 1 }\nfn u() -> Int { 2 }\nfn g() -> Int { t() + t() }\nfn main() -> Int { g() }\n",
    );
    let d = delta(&p.root(), &before);
    assert_eq!(changed(&d, "main.nz::fn t"), ["references", "callers"]);
}

#[test]
fn a_use_elsewhere_changes_references_of_a_type() {
    let p = project();
    let before = snapshot(&p.root());
    // `far` begins to mention nothing new; main's `helper` takes a second Point.
    p.write(
        "main.nz",
        &MAIN
            .replace(
                "fn helper(p: Point) -> Int { p.x }",
                "fn helper(p: Point, q: Point) -> Int { p.x + q.y }",
            )
            .replace(
                "helper(Point(x: 1, y: 2))",
                "helper(Point(x: 1, y: 2), Point(x: 3, y: 4))",
            ),
    );
    let d = delta(&p.root(), &before);
    let point = changed(&d, "main.nz::record Point");
    assert_eq!(point, ["references"], "{d}");
    let helper = changed(&d, "main.nz::fn helper");
    for s in ["source", "shape", "dependencies", "related_types"] {
        assert!(helper.contains(&s.to_owned()), "{helper:?}");
    }
}

#[test]
fn a_new_diagnostic_is_a_diagnostics_change() {
    let p = project();
    let before = snapshot(&p.root());
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper() }\nfn helper() -> Int { true }\n",
    );
    let d = delta(&p.root(), &before);
    assert_eq!(changed(&d, "lib.nz::fn helper"), ["source", "diagnostics"]);
    // Only the definition the diagnostic is in.
    assert_eq!(identities(&d["changed"]), ["lib.nz::fn helper"]);
}

#[test]
fn same_names_in_two_modules_are_two_definitions() {
    let p = project();
    let before = snapshot(&p.root());
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper() }\nfn helper() -> Int { 70 }\n",
    );
    let d = delta(&p.root(), &before);
    assert_eq!(identities(&d["changed"]), ["lib.nz::fn helper"]);
    // main's `helper` is untouched, and its digests are exactly as before.
    assert_eq!(
        definition(&snapshot(&p.root()), "main.nz::fn helper"),
        definition(&before, "main.nz::fn helper")
    );
}

#[test]
fn a_broken_edit_has_no_delta_and_a_repair_does() {
    let p = project();
    let before = snapshot(&p.root());
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper( }\nfn helper() -> Int { 7 }\n",
    );
    assert_eq!(
        delta(&p.root(), &before),
        json!({"schema": "nazm.delta/2", "status": "unavailable", "reason": "incomplete_compilation"})
    );
    p.write("lib.nz", LIB);
    let d = delta(&p.root(), &before);
    assert_eq!(d["status"], "available");
    assert_eq!(d["changed"], json!([]));
}

#[test]
fn a_baseline_that_is_not_exactly_a_snapshot_of_this_root_is_refused() {
    let p = project();
    let good = snapshot(&p.root());
    let refused = |b: Value| delta(&p.root(), &b)["reason"].as_str().unwrap().to_owned();

    let mut b = good.clone();
    b["schema"] = json!("nazm.snapshot/3");
    assert_eq!(refused(b), "baseline_wrong_schema");
    assert_eq!(
        refused(json!({"schema": "nazm.context/2"})),
        "baseline_wrong_schema"
    );
    assert_eq!(refused(json!("not an object")), "baseline_wrong_schema");

    assert_eq!(
        refused(
            json!({"schema": "nazm.snapshot/2", "status": "unavailable", "reason": "incomplete_compilation"})
        ),
        "baseline_unavailable"
    );

    let mut b = good.clone();
    b["extra"] = json!(1);
    assert_eq!(refused(b), "baseline_malformed");
    let mut b = good.clone();
    b["definitions"][0]["digests"]["capabilities"] = json!("0".repeat(64));
    assert_eq!(refused(b), "baseline_malformed");
    let mut b = good.clone();
    b["definitions"][0]["digests"]
        .as_object_mut()
        .unwrap()
        .remove("callers");
    assert_eq!(refused(b), "baseline_malformed");
    let mut b = good.clone();
    b["unsupported"] = json!(["capabilities", "tests"]);
    assert_eq!(refused(b), "baseline_malformed");
    // A record with a callers digest is a file that disagrees with itself.
    let mut b = good.clone();
    let i = identities(&b["definitions"])
        .iter()
        .position(|d| d == "main.nz::record Point")
        .unwrap();
    b["definitions"][i]["digests"]["callers"] = json!("0".repeat(64));
    assert_eq!(refused(b), "baseline_malformed");

    for identity in [
        "main.nz::record main",
        "./main.nz::fn main",
        "main.nz::fn  main",
        "/tmp/main.nz::fn main",
        "@core/prelude::fn main",
        "main",
    ] {
        let mut b = good.clone();
        let i = identities(&b["definitions"])
            .iter()
            .position(|d| d == "main.nz::fn main")
            .unwrap();
        b["definitions"][i]["identity"] = json!(identity);
        assert_eq!(refused(b), "baseline_invalid_identity", "{identity}");
    }

    let mut b = good.clone();
    let first = b["definitions"][0].clone();
    b["definitions"].as_array_mut().unwrap().push(first);
    assert_eq!(refused(b), "baseline_duplicate_identity");

    for digest in [
        "0".repeat(63),
        "A".repeat(64),
        format!("{}g", "0".repeat(63)),
    ] {
        let mut b = good.clone();
        b["definitions"][0]["digests"]["source"] = json!(digest);
        assert_eq!(refused(b), "baseline_invalid_digest");
    }

    // Another root's snapshot is not compared with this one.
    let lib = snapshot(&p.path("lib.nz"));
    assert_eq!(refused(lib), "baseline_wrong_root");
    let mut b = good.clone();
    b["root"]["compiler"] = json!("nazm 0.0.0");
    assert_eq!(refused(b), "baseline_wrong_compiler");

    // And the good one is accepted.
    assert_eq!(delta(&p.root(), &good)["status"], "available");
}

#[test]
fn a_thousand_snapshots_and_deltas_keep_nothing() {
    let p = project();
    let root = p.root();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        let retained = s.retained();
        let first = s.semantic_snapshot(&root, 0).to_json();
        let baseline: Value = serde_json::from_str(&first).unwrap();
        let first_delta = s.semantic_delta(&root, 0, &baseline).to_json();
        for i in 0..500 {
            assert_eq!(s.semantic_snapshot(&root, 0).to_json(), first, "{i}");
            assert_eq!(
                s.semantic_delta(&root, 0, &baseline).to_json(),
                first_delta,
                "{i}"
            );
        }
        assert_eq!(s.retained(), retained);
        assert_eq!(s.open_documents(), 1);
    });
}

#[test]
fn a_removed_record_is_removed() {
    let p = Project::new(&[(
        "main.nz",
        "struct Unused {\n    a: Int,\n}\nfn main() -> Int { 1 }\n",
    )]);
    let before = snapshot(&p.root());
    p.write("main.nz", "fn main() -> Int { 1 }\n");
    let d = delta(&p.root(), &before);
    assert_eq!(identities(&d["removed"]), ["main.nz::record Unused"]);
    assert_eq!(d["removed"][0]["kind"], "record");
    assert_eq!(
        d["removed"][0]["declaration"],
        json!({"file": "main.nz", "start": 0, "end": 29})
    );
    assert_eq!(d["added"], json!([]));
    // `main`'s text moved to the start of the file and did not change.
    assert_eq!(d["changed"], json!([]), "{d}");
}

#[test]
fn a_second_declaration_of_a_key_is_not_a_second_definition() {
    // The checker refuses the second `twice` and declares one: the snapshot has it once,
    // under its one key, and never a guess at which text is which.
    let p = Project::new(&[(
        "main.nz",
        "fn twice() -> Int { 1 }\nfn twice() -> Int { 2 }\nfn main() -> Int { 3 }\n",
    )]);
    let s = snapshot(&p.root());
    assert_eq!(s["status"], "available", "{s}");
    assert_eq!(
        identities(&s["definitions"]),
        ["main.nz::fn main", "main.nz::fn twice"]
    );
    assert_eq!(s["untracked"], 0);
}

#[test]
fn a_snapshot_is_the_same_at_any_generation_of_the_service() {
    // The service's generation moves with every edit anywhere; a snapshot of unchanged
    // sources does not.
    let p = project();
    let other = Project::new(&[("main.nz", "fn main() -> Int { 0 }\n")]);
    let (root, elsewhere) = (p.root(), other.root());
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&root, 1, MAIN.to_owned());
        let first = s.semantic_snapshot(&root, 1).to_json();
        s.open(&elsewhere, 1, "fn main() -> Int { 0 }\n".to_owned());
        s.change(&elsewhere, 2, "fn main() -> Int { 1 }\n".to_owned());
        s.change(&root, 2, MAIN.to_owned());
        assert_eq!(s.semantic_snapshot(&root, 2).to_json(), first);
        // And a version that is not current has no snapshot, never the previous one.
        assert!(s
            .semantic_snapshot(&root, 1)
            .to_json()
            .contains("not_current"));
    });
}

// ---------------------------------------------------------------------------------------
// N37: the authority a function requires is its signature.
// ---------------------------------------------------------------------------------------

#[test]
fn a_capability_only_change_is_a_change_of_shape_under_the_same_identity() {
    let body =
        "-> Int ! { io } { print(int_to_str(n)) }\nfn main(io: IoCap) -> Int { save(io, 1) }\n";
    let p = Project::new(&[("main.nz", &format!("fn save(io: IoCap, n: Int) {body}"))]);
    let before = snapshot(&p.root());
    // The same body and the same declared set: only the authority `save` requires moved.
    p.write(
        "main.nz",
        &format!("fn save(io: IoCap, t: SpawnCap, n: Int) {body}"),
    );
    let d = delta(&p.root(), &before);
    let save = changed(&d, "main.nz::fn save");
    assert!(save.contains(&"shape".to_owned()), "{d}");
    assert!(!save.contains(&"effects".to_owned()), "{d}");
    // A contract change, not a new definition: the identity holds.
    assert_eq!(d["added"], json!([]), "{d}");
    assert_eq!(d["removed"], json!([]), "{d}");
    // And a different kind in the same position is a different shape again.
    let middle = snapshot(&p.root());
    p.write(
        "main.nz",
        &format!("fn save(io: IoCap, t: IoCap, n: Int) {body}"),
    );
    let d = delta(&p.root(), &middle);
    assert!(
        changed(&d, "main.nz::fn save").contains(&"shape".to_owned()),
        "{d}"
    );
}

// ---------------------------------------------------------------------------------------
// N36: effects are a recorded section.
// ---------------------------------------------------------------------------------------

#[test]
fn an_effect_change_is_its_own_section_whether_declared_or_required() {
    let text = "fn f() -> Int { 1 }\nfn main() -> Int { f() }\n";
    let p = Project::new(&[("main.nz", text)]);
    let before = snapshot(&p.root());
    // The body starts to print: nothing declared moved, but what `f` requires did, and so
    // did what its caller requires. Neither's shape changed.
    p.write(
        "main.nz",
        "fn f() -> Int { print(\"x\") }\nfn main() -> Int { f() }\n",
    );
    let d = delta(&p.root(), &before);
    // A built-in is not a dependency or a callee entity: only `effects` says it is new.
    assert_eq!(changed(&d, "main.nz::fn f"), ["source", "effects"]);
    assert_eq!(changed(&d, "main.nz::fn main"), ["effects"]);
    // Declared pure, same body: the contract is the signature, and it is an effect change.
    p.write(
        "main.nz",
        "fn f() -> Int ! {} { 1 }\nfn main() -> Int { f() }\n",
    );
    let d = delta(&p.root(), &before);
    assert_eq!(changed(&d, "main.nz::fn f"), ["source", "shape", "effects"]);
    assert!(
        d["changed"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["identity"] != "main.nz::fn main"),
        "{d}"
    );
    // One set in two orders is one contract: only the source differs.
    p.write(
        "main.nz",
        "fn f() -> Int ! { io, spawn } { 1 }\nfn main() -> Int { f() }\n",
    );
    let one = snapshot(&p.root());
    p.write(
        "main.nz",
        "fn f() -> Int ! { spawn, io } { 1 }\nfn main() -> Int { f() }\n",
    );
    let d = delta(&p.root(), &one);
    assert_eq!(changed(&d, "main.nz::fn f"), ["source"]);
}
