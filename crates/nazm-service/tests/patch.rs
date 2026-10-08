//! Semantic patch plans through the language service. N29.
//!
//! Every patch here is planned by a fresh service that opened the root from disk, exactly as
//! `nazm patch` and `nazm-mcp` plan one, and compared as the JSON it serialises to. A patch
//! is applied only in these tests, in memory or in a copy of the fixture, to prove what it
//! would do; nothing in the service applies one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use std::path::Path;

use nazm_service::patch::digest;
use nazm_service::LanguageService;
use serde_json::{json, Value};

mod common;
use common::{on_the_service_stack, Project};

enum Ask<'a> {
    Rename(&'a str),
    Fix(usize),
}

fn patch_json(root: &Path, file: &Path, offset: u32, ask: &Ask<'_>) -> String {
    let (root, file) = (root.to_path_buf(), file.to_path_buf());
    let ask = match ask {
        Ask::Rename(n) => Ok((*n).to_owned()),
        Ask::Fix(i) => Err(*i),
    };
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        match ask {
            Ok(name) => s.patch_rename(&root, 0, &file, offset, &name),
            Err(i) => s.patch_fix(&root, 0, &file, offset, i),
        }
        .to_json()
    })
}

fn rename(p: &Project, file: &str, offset: u32, new_name: &str) -> Value {
    serde_json::from_str(&patch_json(
        &p.root(),
        &p.path(file),
        offset,
        &Ask::Rename(new_name),
    ))
    .unwrap()
}

fn fix(p: &Project, file: &str, offset: u32, index: usize) -> Value {
    serde_json::from_str(&patch_json(
        &p.root(),
        &p.path(file),
        offset,
        &Ask::Fix(index),
    ))
    .unwrap()
}

fn snapshot_json(root: &Path) -> String {
    let root = root.to_path_buf();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        s.semantic_snapshot(&root, 0).to_json()
    })
}

fn delta(root: &Path, baseline: &Value) -> Value {
    let (root, baseline) = (root.to_path_buf(), baseline.clone());
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        serde_json::from_str(&s.semantic_delta(&root, 0, &baseline).to_json()).unwrap()
    })
}

/// The diagnostics of the compilation rooted at `root`, by code, from a fresh service.
fn codes(root: &Path) -> Vec<String> {
    let root = root.to_path_buf();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open_from_disk(&root).unwrap();
        published
            .iter()
            .flat_map(|p| p.diagnostics.iter().map(|d| d.code.to_string()))
            .collect()
    })
}

/// `text` with one patch file's edits applied — right to left, as an applier would.
fn apply(text: &str, file: &Value) -> String {
    let mut out = text.to_owned();
    let mut edits: Vec<&Value> = file["edits"].as_array().unwrap().iter().collect();
    edits.reverse();
    for e in edits {
        let (s, t) = (
            e["start"].as_u64().unwrap() as usize,
            e["end"].as_u64().unwrap() as usize,
        );
        assert_eq!(&out[s..t], e["expected"].as_str().unwrap());
        out.replace_range(s..t, e["replacement"].as_str().unwrap());
    }
    out
}

/// Apply every file of `patch` to the project on disk — a fixture copy, never the
/// repository — after checking all three freshness layers, as a future applier would.
fn apply_to(p: &Project, patch: &Value) {
    assert_eq!(
        patch["baseline"]["snapshot"],
        digest(snapshot_json(&p.root()).as_bytes()),
        "the root's snapshot moved"
    );
    for f in patch["files"].as_array().unwrap() {
        let name = f["path"].as_str().unwrap();
        let text = p.read(name);
        assert_eq!(f["digest"], digest(text.as_bytes()), "{name} moved");
        p.write(name, &apply(&text, f));
    }
}

fn at(text: &str, needle: &str, n: usize) -> u32 {
    let mut from = 0;
    let mut found = 0;
    for _ in 0..=n {
        found = text[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} #{n}"))
            + from;
        from = found + needle.len();
    }
    u32::try_from(found).unwrap()
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
fn main() -> Int {
    let total = helper(Point(x: 1, y: 2)) + area(Shape.Dot());
    total + far()
}
";

fn project() -> Project {
    Project::new(&[("main.nz", MAIN), ("lib.nz", LIB)])
}

/// Every edit's `(start, end)`, in the one file of a patch.
fn ranges(patch: &Value) -> Vec<(u32, u32)> {
    assert_eq!(patch["files"].as_array().unwrap().len(), 1, "{patch}");
    patch["files"][0]["edits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                u32::try_from(e["start"].as_u64().unwrap()).unwrap(),
                u32::try_from(e["end"].as_u64().unwrap()).unwrap(),
            )
        })
        .collect()
}

fn occurrences(text: &str, name: &str, which: &[usize]) -> Vec<(u32, u32)> {
    let n = u32::try_from(name.len()).unwrap();
    which
        .iter()
        .map(|&i| {
            let s = at(text, name, i);
            (s, s + n)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// Rename.
// ---------------------------------------------------------------------------------------

#[test]
fn a_private_function_rename_is_its_declaration_and_calls_and_nothing_else() {
    let p = project();
    let patch = rename(&p, "main.nz", at(MAIN, "helper", 0), "assist");
    assert_eq!(patch["schema"], "nazm.patch/1");
    assert_eq!(patch["status"], "available", "{patch}");
    assert_eq!(
        patch["operation"],
        json!({"kind": "rename", "target": {"kind": "function", "durable": true,
               "identity": {"definition": "main.nz::fn helper", "member": null},
               "declaration": {"file": "main.nz", "start": at(MAIN, "helper", 0), "end": at(MAIN, "helper", 0) + 6},
               "within": null},
               "old_name": "helper", "new_name": "assist"})
    );
    assert_eq!(patch["root"]["module"], "main.nz");
    assert_eq!(patch["validation"], "reanalysed");
    // main's own `helper`, declared and called once: two edits. lib's `helper` is another
    // definition and is not touched.
    assert_eq!(ranges(&patch), occurrences(MAIN, "helper", &[0, 1]));
    let file = &patch["files"][0];
    assert_eq!(file["path"], "main.nz");
    assert_eq!(file["digest"], digest(MAIN.as_bytes()));
    for e in file["edits"].as_array().unwrap() {
        assert_eq!(e["expected"], "helper");
        assert_eq!(e["replacement"], "assist");
    }
    assert_eq!(
        patch["baseline"]["snapshot"],
        digest(snapshot_json(&p.root()).as_bytes())
    );
    // Minimal: no file text beyond the names.
    let text = patch.to_string();
    assert!(!text.contains("p.x"), "{text}");
    assert!(!text.contains("struct"), "{text}");
}

/// A category, the offset renamed, the new and old names, and every occurrence's range.
type Case = (
    &'static str,
    u32,
    &'static str,
    &'static str,
    Vec<(u32, u32)>,
);

#[test]
fn every_category_n18_renames_is_a_patch_of_exactly_its_occurrences() {
    let p = project();
    let cases: [Case; 6] = [
        (
            "local",
            at(MAIN, "total", 0),
            "sum",
            "total",
            occurrences(MAIN, "total", &[0, 1]),
        ),
        (
            "record",
            at(MAIN, "Point", 0),
            "Pt",
            "Point",
            occurrences(MAIN, "Point", &[0, 1, 2]),
        ),
        (
            "field",
            at(MAIN, "x: Int", 0),
            "ex",
            "x",
            vec![
                (at(MAIN, "x: Int", 0), at(MAIN, "x: Int", 0) + 1),
                (at(MAIN, "p.x", 0) + 2, at(MAIN, "p.x", 0) + 3),
                (at(MAIN, "x: 1", 0), at(MAIN, "x: 1", 0) + 1),
            ],
        ),
        (
            "enum",
            at(MAIN, "Shape", 0),
            "Form",
            "Shape",
            occurrences(MAIN, "Shape", &[0, 1, 2, 3, 4]),
        ),
        (
            "variant",
            at(MAIN, "Dot", 0),
            "Spot",
            "Dot",
            occurrences(MAIN, "Dot", &[0, 1, 2]),
        ),
        (
            "payload_field",
            at(MAIN, "radius", 0),
            "rad",
            "radius",
            occurrences(MAIN, "radius", &[0, 1]),
        ),
    ];
    for (kind, offset, new_name, old_name, expected) in cases {
        let patch = rename(&p, "main.nz", offset, new_name);
        assert_eq!(patch["status"], "available", "{kind}: {patch}");
        let target = &patch["operation"]["target"];
        assert_eq!(target["kind"], kind);
        assert_eq!(patch["operation"]["old_name"], old_name);
        assert_eq!(ranges(&patch), expected, "{kind}");
        // Applied to a copy, the program still checks.
        let copy = project();
        apply_to(&copy, &patch);
        assert_eq!(codes(&copy.root()), Vec::<String>::new(), "{kind}");
    }
    // A local has no durable key and is not given one: it is named by its place and the
    // function it is in.
    let local = rename(&p, "main.nz", at(MAIN, "total", 1), "sum");
    assert_eq!(
        local["operation"]["target"],
        json!({"kind": "local", "durable": false, "identity": null,
               "declaration": {"file": "main.nz", "start": at(MAIN, "total", 0), "end": at(MAIN, "total", 0) + 5},
               "within": "main.nz::fn main"})
    );
    // Members are durable as their definition's key and their name.
    let field = rename(&p, "main.nz", at(MAIN, "x: Int", 0), "ex");
    assert_eq!(
        field["operation"]["target"]["identity"],
        json!({"definition": "main.nz::record Point", "member": "x"})
    );
    let payload = rename(&p, "main.nz", at(MAIN, "radius", 0), "rad");
    assert_eq!(
        payload["operation"]["target"]["identity"],
        json!({"definition": "main.nz::enum Shape", "member": "Circle.radius"})
    );
}

#[test]
fn a_private_definition_in_an_imported_file_is_planned_from_disk() {
    // lib.nz is not the root and not open anywhere: the plan describes it, protected by its
    // digest and each edit's bytes. Two modules' `helper` stay two definitions.
    let p = project();
    let patch = rename(&p, "lib.nz", at(LIB, "helper", 1), "assist");
    assert_eq!(patch["status"], "available", "{patch}");
    assert_eq!(
        patch["operation"]["target"]["identity"]["definition"],
        "lib.nz::fn helper"
    );
    assert_eq!(patch["files"][0]["path"], "lib.nz");
    assert_eq!(patch["files"][0]["digest"], digest(LIB.as_bytes()));
    assert_eq!(ranges(&patch), occurrences(LIB, "helper", &[0, 1]));
    assert!(!patch.to_string().contains("main.nz\",\"digest"));
    // The language server's rename is unchanged: a file that is not an open document has
    // nothing to rename.
    let (main, lib) = (p.root(), p.path("lib.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&main).unwrap();
        assert_eq!(
            s.rename(&lib, 0, at(LIB, "helper", 1), "assist"),
            Err(nazm_service::Refusal::NotCurrent)
        );
    });
}

#[test]
fn a_rename_that_n18_refuses_is_no_patch() {
    let p = project();
    let reason = |file: &str, offset: u32, name: &str| {
        let r = rename(&p, file, offset, name);
        r["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("{file} {offset} {name:?}: {r}"))
            .to_owned()
    };
    // Exported: one root compilation is not every module that may import lib.nz.
    assert_eq!(reason("lib.nz", at(LIB, "far", 0), "near"), "exported");
    assert_eq!(reason("main.nz", at(MAIN, "far", 0), "near"), "exported");
    for bad in ["", "two words", "fn", "9lives", "_"] {
        assert_eq!(
            reason("main.nz", at(MAIN, "helper", 0), bad),
            "invalid_name"
        );
    }
    assert_eq!(
        reason("main.nz", at(MAIN, "helper", 0), "helper"),
        "unchanged"
    );
    // A name another definition has: the renamed program would not check, or would mean
    // something else.
    let collision = reason("main.nz", at(MAIN, "helper", 0), "area");
    assert!(
        collision == "rejected" || collision == "meaning_changed",
        "{collision}"
    );
    // A parameter renamed to its neighbour's name: two bindings of one name.
    let pair = Project::new(&[(
        "main.nz",
        "fn add(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { add(1, 2) }\n",
    )]);
    let clash = rename(&pair, "main.nz", 7, "b");
    let clash = clash["reason"].as_str().unwrap();
    assert!(clash == "rejected" || clash == "meaning_changed", "{clash}");
    assert_eq!(reason("main.nz", at(MAIN, "struct", 0), "x"), "no_entity");
    assert_eq!(reason("main.nz", 100_000, "x"), "out_of_range");
    // A file the compilation did not load.
    p.write("stray.nz", "fn stray() -> Int { 1 }\n");
    assert_eq!(reason("stray.nz", 3, "x"), "not_in_compilation");
    assert_eq!(reason("/etc/passwd", 0, "x"), "not_in_compilation");
    // A compilation with errors: not every use is known.
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper() }\nfn helper() -> Int { true }\n",
    );
    assert_eq!(reason("main.nz", at(MAIN, "total", 0), "sum"), "not_clean");
    // Broken syntax: nothing to bind to.
    p.write("lib.nz", "pub fn far() -> Int { helper( }\n");
    assert_eq!(
        reason("main.nz", at(MAIN, "total", 0), "sum"),
        "incomplete_compilation"
    );
}

// ---------------------------------------------------------------------------------------
// Diagnostic fixes.
// ---------------------------------------------------------------------------------------

const IMMUTABLE: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n";
const LOOP: &str =
    "fn main() -> Int {\n    let mut i = 0;\n    while i < 3 {\n        i = i + 1;\n        i\n    }\n    i\n}\n";

#[test]
fn an_automatic_fix_is_the_compilers_edit_with_its_applicability_and_precondition() {
    let p = Project::new(&[("main.nz", IMMUTABLE)]);
    let before = codes(&p.root());
    assert_eq!(before.len(), 1, "{before:?}");
    let offset = at(IMMUTABLE, "x = 2", 0);
    let patch = fix(&p, "main.nz", offset, 0);
    assert_eq!(patch["status"], "available", "{patch}");
    let op = &patch["operation"];
    assert_eq!(op["kind"], "diagnostic_fix");
    assert_eq!(op["applicability"], "automatic");
    assert_eq!(op["fix"], 0);
    assert_eq!(op["alternatives"], 1);
    assert_eq!(op["diagnostic"]["code"], before[0]);
    assert_eq!(op["diagnostic"]["file"], "main.nz");
    assert_eq!(op["diagnostic"]["start"], offset);
    assert!(!op["precondition"].as_str().unwrap().is_empty());
    assert_eq!(patch["validation"], "compiler_fix");
    // One edit, the compiler's span: `x` in the `let` becomes `mut x`.
    let decl = at(IMMUTABLE, "x = 1", 0);
    assert_eq!(ranges(&patch), [(decl, decl + 1)]);
    assert_eq!(patch["files"][0]["edits"][0]["expected"], "x");
    assert_eq!(patch["files"][0]["edits"][0]["replacement"], "mut x");
    assert_eq!(patch["files"][0]["digest"], digest(IMMUTABLE.as_bytes()));
    // Applied: the program checks, and the diagnostic is gone — so is the fix.
    apply_to(&p, &patch);
    assert_eq!(codes(&p.root()), Vec::<String>::new());
    assert_eq!(fix(&p, "main.nz", offset, 0)["reason"], "no_such_fix");
}

#[test]
fn a_needs_review_fix_stays_needs_review_with_its_precondition() {
    let p = Project::new(&[("main.nz", LOOP)]);
    let before = codes(&p.root());
    assert_eq!(before.len(), 1, "{before:?}");
    let offset = at(LOOP, "i\n    }", 0);
    let patch = fix(&p, "main.nz", offset, 0);
    assert_eq!(patch["status"], "available", "{patch}");
    let op = &patch["operation"];
    assert_eq!(op["applicability"], "needs_review");
    let precondition = op["precondition"].as_str().unwrap();
    assert!(precondition.contains("discarded"), "{precondition}");
    assert_ne!(op["applicability"], "automatic");
    apply_to(&p, &patch);
    assert_eq!(codes(&p.root()), Vec::<String>::new());
}

#[test]
fn only_a_current_diagnostics_own_fix_is_a_patch() {
    let p = Project::new(&[("main.nz", IMMUTABLE)]);
    // No diagnostic at the position, whatever a client believes is there.
    assert_eq!(fix(&p, "main.nz", 0, 0)["reason"], "no_such_fix");
    assert_eq!(
        fix(&p, "main.nz", at(IMMUTABLE, "Int", 0), 0)["reason"],
        "no_such_fix"
    );
    // A real diagnostic has one fix; there is no second.
    assert_eq!(
        fix(&p, "main.nz", at(IMMUTABLE, "x = 2", 0), 1)["reason"],
        "no_such_fix"
    );
    // A clean program has no fix anywhere.
    let clean = project();
    for offset in [0, at(MAIN, "helper", 1), at(MAIN, "total", 1)] {
        assert_eq!(fix(&clean, "main.nz", offset, 0)["reason"], "no_such_fix");
    }
    // A fix of a syntax diagnostic has no snapshot to bind to.
    let broken = Project::new(&[(
        "main.nz",
        "fn f(mut n: Int) -> Int { n }\nfn main() -> Int { f(1) }\n",
    )]);
    assert_eq!(
        fix(&broken, "main.nz", 5, 0)["reason"],
        "incomplete_compilation"
    );
}

// ---------------------------------------------------------------------------------------
// Freshness, determinism, and what N28 sees.
// ---------------------------------------------------------------------------------------

#[test]
fn every_layer_of_a_plan_goes_stale_when_what_it_protects_moves() {
    let p = project();
    let offset = at(MAIN, "helper", 0);
    let patch = rename(&p, "main.nz", offset, "assist");
    let snapshot = |p: &Project| digest(snapshot_json(&p.root()).as_bytes());
    assert_eq!(patch["baseline"]["snapshot"], snapshot(&p));
    // A comment elsewhere in the file: the file digest moves, and so does the snapshot (one
    // definition's source); the edit's bytes are where they were.
    p.write("main.nz", &MAIN.replace("fn main", "// n29\nfn main"));
    assert_ne!(
        patch["files"][0]["digest"],
        digest(p.read("main.nz").as_bytes())
    );
    assert_ne!(patch["baseline"]["snapshot"], snapshot(&p));
    // A line above the target: the expected bytes are no longer at the edit's range.
    p.write("main.nz", &format!("// n29\n{MAIN}"));
    let now = p.read("main.nz");
    let e = &patch["files"][0]["edits"][0];
    let (s, t) = (
        e["start"].as_u64().unwrap() as usize,
        e["end"].as_u64().unwrap() as usize,
    );
    assert_ne!(&now[s..t], e["expected"].as_str().unwrap());
    // An edit in another file of the compilation moves only the snapshot.
    p.write("main.nz", MAIN);
    p.write(
        "lib.nz",
        "pub fn far() -> Int { helper() }\nfn helper() -> Int { 8 }\n",
    );
    assert_eq!(
        patch["files"][0]["digest"],
        digest(p.read("main.nz").as_bytes())
    );
    assert_ne!(patch["baseline"]["snapshot"], snapshot(&p));
    // And the same request now is a fresh plan, never the old one.
    let fresh = rename(&p, "main.nz", offset, "assist");
    assert_eq!(fresh["files"], patch["files"]);
    assert_ne!(fresh["baseline"], patch["baseline"]);
}

#[test]
fn the_same_request_gives_the_same_bytes_from_any_directory() {
    let a = project();
    let b = project();
    let ask = |p: &Project| {
        patch_json(
            &p.root(),
            &p.path("main.nz"),
            at(MAIN, "Point", 0),
            &Ask::Rename("Pt"),
        )
    };
    let first = ask(&a);
    assert_eq!(first, ask(&a));
    assert_eq!(first, ask(&b));
    for dir in [a.dir.path(), b.dir.path()] {
        assert!(!first.contains(dir.to_str().unwrap()), "{first}");
    }
    let fixed = Project::new(&[("main.nz", IMMUTABLE)]);
    let again = Project::new(&[("main.nz", IMMUTABLE)]);
    let ask_fix = |p: &Project| {
        patch_json(
            &p.root(),
            &p.root(),
            at(IMMUTABLE, "x = 2", 0),
            &Ask::Fix(0),
        )
    };
    assert_eq!(ask_fix(&fixed), ask_fix(&again));
}

#[test]
fn a_renamed_function_is_its_old_key_removed_and_new_key_added_in_the_delta() {
    let p = project();
    let baseline: Value = serde_json::from_str(&snapshot_json(&p.root())).unwrap();
    let patch = rename(&p, "main.nz", at(MAIN, "helper", 0), "assist");
    apply_to(&p, &patch);
    let d = delta(&p.root(), &baseline);
    let ids = |k: &str| -> Vec<String> {
        d[k].as_array()
            .unwrap()
            .iter()
            .map(|x| x["identity"].as_str().unwrap().to_owned())
            .collect()
    };
    // N28 does not infer the rename: removed plus added. The caller's own text changed,
    // and it now uses and calls another definition.
    assert_eq!(ids("removed"), ["main.nz::fn helper"]);
    assert_eq!(ids("added"), ["main.nz::fn assist"]);
    // The record `helper` took is referenced from `assist` now: references are attributed
    // to the definition they are in, by identity.
    assert_eq!(
        ids("changed"),
        ["main.nz::fn main", "main.nz::record Point"]
    );
    assert_eq!(
        d["changed"][1]["sections"],
        json!({"source": false, "shape": false, "dependencies": false, "related_types": false,
               "references": true, "callers": null, "callees": null, "diagnostics": false,
               "effects": null, "provenance": null})
    );
    let sections = &d["changed"][0]["sections"];
    for s in ["source", "dependencies", "callees"] {
        assert_eq!(sections[s], true, "{s}: {d}");
    }
    assert_eq!(sections["shape"], false);
}

#[test]
fn a_fixed_diagnostic_changes_its_definitions_source_and_diagnostics_in_the_delta() {
    let p = Project::new(&[("main.nz", IMMUTABLE)]);
    let baseline: Value = serde_json::from_str(&snapshot_json(&p.root())).unwrap();
    let patch = fix(&p, "main.nz", at(IMMUTABLE, "x = 2", 0), 0);
    apply_to(&p, &patch);
    let d = delta(&p.root(), &baseline);
    assert_eq!(d["changed"].as_array().unwrap().len(), 1, "{d}");
    let c = &d["changed"][0];
    assert_eq!(c["identity"], "main.nz::fn main");
    assert_eq!(c["sections"]["source"], true);
    assert_eq!(c["sections"]["diagnostics"], true);
    assert_eq!(d["added"], json!([]));
    assert_eq!(d["removed"], json!([]));
}

#[test]
fn a_thousand_plans_are_each_of_the_disk_and_keep_nothing() {
    let p = project();
    let q = Project::new(&[("main.nz", IMMUTABLE)]);
    let (root, lib, broken) = (p.root(), p.path("lib.nz"), q.root());
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        let offset = at(LIB, "helper", 1);
        let first = s.patch_rename(&root, 0, &lib, offset, "assist").to_json();
        let docs = s.open_documents();
        let retained = s.retained();
        let mut f = LanguageService::new();
        f.open_from_disk(&broken).unwrap();
        let fix_first = f
            .patch_fix(&broken, 0, &broken, at(IMMUTABLE, "x = 2", 0), 0)
            .to_json();
        let fix_retained = f.retained();
        for i in 0..500 {
            assert_eq!(
                s.patch_rename(&root, 0, &lib, offset, "assist").to_json(),
                first,
                "{i}"
            );
            assert_eq!(
                f.patch_fix(&broken, 0, &broken, at(IMMUTABLE, "x = 2", 0), 0)
                    .to_json(),
                fix_first,
                "{i}"
            );
        }
        // One document for the root and one for the file the plan is in; nothing else.
        assert_eq!(s.open_documents(), docs);
        assert_eq!(docs, 2);
        assert_eq!(s.retained(), retained);
        assert_eq!(f.retained(), fix_retained);
    });
}
