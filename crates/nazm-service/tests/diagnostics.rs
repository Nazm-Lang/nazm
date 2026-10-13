//! Compact diagnostics and diagnostic detail through the language service. N30.
//!
//! Every index and detail is asked of a fresh service that opened the root from disk, as
//! `nazm diagnostics` and `nazm-mcp` ask, and compared as the JSON it serialises to.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use nazm_service::LanguageService;
use serde_json::{json, Value};

mod common;
use common::{on_the_service_stack, Project};

fn index_json(root: &Path) -> String {
    let root = root.to_path_buf();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        s.diagnostic_index(&root, 0).to_json()
    })
}

fn index(root: &Path) -> Value {
    serde_json::from_str(&index_json(root)).unwrap()
}

fn detail(root: &Path, id: &str, state: &str) -> Value {
    let (root, id, state) = (root.to_path_buf(), id.to_owned(), state.to_owned());
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        serde_json::from_str(&s.diagnostic_detail(&root, 0, &id, &state).to_json()).unwrap()
    })
}

/// Every diagnostic `nazm check --json` would print, in `nazm.diagnostic/1`, from a fresh
/// service — the full stream the index is compared with.
fn full(root: &Path) -> Vec<Value> {
    let root = root.to_path_buf();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        let i: Value = serde_json::from_str(&s.diagnostic_index(&root, 0).to_json()).unwrap();
        let state = i["state"]["source"].as_str().unwrap().to_owned();
        i["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                let v: Value = serde_json::from_str(
                    &s.diagnostic_detail(&root, 0, d["id"].as_str().unwrap(), &state)
                        .to_json(),
                )
                .unwrap();
                v["diagnostic"].clone()
            })
            .collect()
    })
}

fn at(text: &str, needle: &str) -> u32 {
    u32::try_from(text.find(needle).unwrap_or_else(|| panic!("{needle:?}"))).unwrap()
}

fn ids(i: &Value) -> Vec<String> {
    i["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_owned())
        .collect()
}

const TYPED: &str = "\
fn helper(n: Int) -> Int { n }
fn main() -> Int {
    let x = 1 + true;
    helper(1, 2) + missing()
}
";

#[test]
fn an_index_is_every_diagnostic_as_compiler_owned_facts_and_no_prose() {
    let p = Project::new(&[("main.nz", TYPED)]);
    let i = index(&p.root());
    assert_eq!(i["schema"], "nazm.diagnostic-index/1");
    assert_eq!(i["status"], "available", "{i}");
    assert_eq!(i["root"]["module"], "main.nz");
    let n = i["diagnostics"].as_array().unwrap().len();
    assert!(n >= 3, "{i}");
    assert_eq!(i["counts"]["total"], n);
    assert_eq!(i["counts"]["error"], n);
    // A compilation with a snapshot is bound to it, as well as to its sources.
    assert_eq!(i["state"]["source"].as_str().unwrap().len(), 64);
    assert_eq!(i["state"]["snapshot"].as_str().unwrap().len(), 64);
    for d in i["diagnostics"].as_array().unwrap() {
        let keys: Vec<&str> = d.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["code", "fixes", "id", "owner", "primary", "severity"]
        );
        assert_eq!(d["severity"], "error");
        assert_eq!(d["primary"]["file"], "main.nz");
        // Inside `main`, which is a durable definition.
        assert_eq!(
            d["owner"],
            json!({"identity": "main.nz::fn main", "kind": "function", "name": "main"})
        );
        let id = d["id"].as_str().unwrap();
        assert!(id.starts_with(d["code"].as_str().unwrap()), "{id}");
    }
    // No prose: not one sentence the compiler wrote reaches the index.
    let text = i.to_string();
    for d in full(&p.root()) {
        for prose in [&d["message"], &d["primary"]["message"], &d["help"]] {
            if let Some(s) = prose.as_str() {
                assert!(!text.contains(s), "{s:?} in the index");
            }
        }
    }
    assert!(
        !text.contains("message") && !text.contains("help"),
        "{text}"
    );
    // The primary places are the diagnostics' own, in the file.
    let places: Vec<u32> = i["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| u32::try_from(d["primary"]["start"].as_u64().unwrap()).unwrap())
        .collect();
    let mut sorted = places.clone();
    sorted.sort_unstable();
    assert_eq!(places, sorted, "ordered by place");
    assert!(places
        .iter()
        .any(|&p| p >= at(TYPED, "let x") && p < at(TYPED, "helper(1")));
    assert!(places.contains(&at(TYPED, "missing")));
}

#[test]
fn a_detail_is_the_published_diagnostic_unchanged_with_its_places_in_their_files() {
    let p = Project::new(&[("main.nz", TYPED)]);
    let i = index(&p.root());
    let state = i["state"]["source"].as_str().unwrap();
    let stream = full(&p.root());
    for (k, d) in i["diagnostics"].as_array().unwrap().iter().enumerate() {
        let id = d["id"].as_str().unwrap();
        let v = detail(&p.root(), id, state);
        assert_eq!(v["schema"], "nazm.diagnostic-detail/1");
        assert_eq!(v["status"], "available", "{v}");
        assert_eq!(v["id"], id);
        assert_eq!(v["state"], i["state"]);
        // The canonical diagnostic, as nazm.diagnostic/1 has it: its prose is the
        // compiler's, and the code and places agree with the index.
        assert_eq!(v["diagnostic"], stream[k]);
        assert_eq!(v["diagnostic"]["code"], d["code"]);
        assert!(v["diagnostic"]["message"]
            .as_str()
            .is_some_and(|m| !m.is_empty()));
        assert_eq!(v["locations"]["primary"], d["primary"]);
        assert_eq!(v["owner"], d["owner"]);
        assert_eq!(v["facts"], "unavailable");
        assert_eq!(
            v["compiler_fixes"],
            v["diagnostic"]["fixes"].as_array().unwrap().len()
        );
    }
}

#[test]
fn syntax_diagnostics_are_indexed_without_a_snapshot_and_their_fixes_are_no_patches() {
    let cases = [
        (
            "lexer: unterminated string",
            "fn main() -> Str {\n    \"open\n}\n",
        ),
        (
            "parser: missing `;`",
            "fn main() -> Int {\n    let a = 1 a\n}\n",
        ),
        (
            "parser: a mutable parameter",
            "fn f(mut n: Int) -> Int { n }\nfn main() -> Int { f(1) }\n",
        ),
        (
            "parser: a value before `}`",
            "fn main() -> Int {\n    1 2\n}\n",
        ),
    ];
    let mut saw_fix = false;
    for (what, text) in cases {
        let p = Project::new(&[("main.nz", text)]);
        let i = index(&p.root());
        assert_eq!(i["status"], "available", "{what}: {i}");
        assert_eq!(
            i["state"]["snapshot"],
            Value::Null,
            "{what}: no N28 snapshot"
        );
        assert!(i["counts"]["total"].as_u64().unwrap() >= 1, "{what}: {i}");
        for d in i["diagnostics"].as_array().unwrap() {
            // A compilation that needed recovery gives no owner: the outline is of a
            // recovery, not the source as written.
            assert_eq!(d["owner"], Value::Null, "{what}");
            let v = detail(
                &p.root(),
                d["id"].as_str().unwrap(),
                i["state"]["source"].as_str().unwrap(),
            );
            assert_eq!(v["status"], "available", "{what}: {v}");
            assert_eq!(v["semantic_patches"], 0, "{what}: {v}");
            assert!(v["fixes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["semantic_patch"].is_null()));
            if v["compiler_fixes"].as_u64().unwrap() > 0 {
                saw_fix = true;
            }
        }
    }
    // At least one syntax diagnostic carries a compiler fix that is no semantic patch.
    assert!(saw_fix);
}

const IMMUTABLE: &str = "fn main() -> Int {\n    let x = 1;\n    x = 2;\n    x\n}\n";
// A contract narrower than its body (N0366): widening it needs review. (Until R1 this was a loop
// body's value, which a line's end now discards.)
const LOOP: &str = "fn f(out: OutCap) -> Int ! {} {\n    print(\"x\");\n    0\n}\nfn main(out: OutCap) -> Int ! {io} { f(out) }\n";

#[test]
fn a_fix_n29_can_plan_carries_the_request_that_plans_it_and_needs_review_stays() {
    for (text, applicability) in [(IMMUTABLE, "automatic"), (LOOP, "needs_review")] {
        let p = Project::new(&[("main.nz", text)]);
        let i = index(&p.root());
        assert_eq!(i["counts"]["with_fixes"], 1, "{i}");
        let key = if applicability == "automatic" {
            "automatic_fixes"
        } else {
            "needs_review_fixes"
        };
        assert_eq!(i["counts"][key], 1, "{i}");
        let other = if applicability == "automatic" {
            "needs_review"
        } else {
            "automatic"
        };
        let other_key = if applicability == "automatic" {
            "needs_review_fixes"
        } else {
            "automatic_fixes"
        };
        assert_eq!(i["counts"][other_key], 0, "{i}");
        let d = &i["diagnostics"][0];
        assert_eq!(d["fixes"]["total"], 1);
        assert_eq!(d["fixes"][applicability], 1);
        assert_eq!(d["fixes"][other], 0);
        let state = i["state"]["source"].as_str().unwrap();
        let v = detail(&p.root(), d["id"].as_str().unwrap(), state);
        assert_eq!(v["compiler_fixes"], 1);
        assert_eq!(v["semantic_patches"], 1);
        let f = &v["fixes"][0];
        assert_eq!(f["applicability"], applicability);
        assert_eq!(v["diagnostic"]["fixes"][0]["applicability"], applicability);
        let sel = &f["semantic_patch"];
        assert_eq!(sel["file"], "main.nz");
        // The selector plans exactly that fix, by N29.
        let (root, offset, index) = (
            p.root(),
            u32::try_from(sel["byte_offset"].as_u64().unwrap()).unwrap(),
            usize::try_from(sel["fix"].as_u64().unwrap()).unwrap(),
        );
        let patch: Value = serde_json::from_str(&on_the_service_stack(move || {
            let mut s = LanguageService::new();
            s.open_from_disk(&root).unwrap();
            s.patch_fix(&root, 0, &root, offset, index).to_json()
        }))
        .unwrap();
        assert_eq!(patch["status"], "available", "{patch}");
        assert_eq!(patch["operation"]["applicability"], applicability);
        assert_eq!(patch["operation"]["diagnostic"]["code"], d["code"]);
    }
}

#[test]
fn one_code_twice_is_two_ids_in_one_file_and_across_files() {
    let lib = "pub fn far() -> Int { true }\nfn near() -> Int { false }\n";
    let main = "use \"lib.nz\";\nfn main() -> Int {\n    let a = 1 + true;\n    let b = 2 + false;\n    a + b + far()\n}\n";
    let p = Project::new(&[("main.nz", main), ("lib.nz", lib)]);
    let i = index(&p.root());
    let all = ids(&i);
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "{all:?}");
    let by_code = |file: &str| -> Vec<&Value> {
        i["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["primary"]["file"] == file)
            .collect()
    };
    let (in_lib, in_main) = (by_code("lib.nz"), by_code("main.nz"));
    assert_eq!(in_lib.len(), 2, "{i}");
    assert!(in_main.len() >= 2, "{i}");
    assert_eq!(in_lib[0]["code"], in_lib[1]["code"]);
    // lib.nz's places are lib.nz's own offsets; the published form's are the
    // concatenation's, and the two differ exactly by where lib.nz starts.
    assert_eq!(in_lib[0]["primary"]["start"], at(lib, "true"));
    assert_eq!(in_lib[1]["primary"]["start"], at(lib, "false"));
    assert_eq!(in_lib[0]["owner"]["identity"], "lib.nz::fn far");
    assert_eq!(in_lib[1]["owner"]["identity"], "lib.nz::fn near");
    let state = i["state"]["source"].as_str().unwrap();
    let v = detail(&p.root(), in_lib[0]["id"].as_str().unwrap(), state);
    let global = v["diagnostic"]["primary"]["span"]["start"]
        .as_u64()
        .unwrap();
    let local = v["locations"]["primary"]["start"].as_u64().unwrap();
    assert_ne!(global, local);
    assert_eq!(global - local, main.len() as u64, "lib.nz follows main.nz");
}

#[test]
fn byte_offsets_are_utf8_bytes_after_accents_and_emoji() {
    let text = "// é😀 before\nfn main() -> Int {\n    let s = 1 + \"é😀\";\n    s\n}\n";
    let p = Project::new(&[("main.nz", text)]);
    let i = index(&p.root());
    let d = &i["diagnostics"][0];
    // Independently: the `+` is at this byte, and at a smaller character or UTF-16 index.
    let plus = text.find("+ ").unwrap();
    assert_ne!(text[..plus].chars().count(), plus);
    assert_eq!(
        d["primary"],
        json!({"file": "main.nz", "start": plus, "end": plus + 1})
    );
    let v = detail(
        &p.root(),
        d["id"].as_str().unwrap(),
        i["state"]["source"].as_str().unwrap(),
    );
    let quoted = text.find('"').unwrap();
    let literal = "\"é😀\"".len();
    assert!(
        v["locations"]["secondary"]
            .as_array()
            .unwrap()
            .contains(&json!({"file": "main.nz", "start": quoted, "end": quoted + literal})),
        "{v}"
    );
}

#[test]
fn a_detail_of_another_state_is_stale_and_never_another_diagnostic() {
    let p = Project::new(&[("main.nz", IMMUTABLE)]);
    let a = index(&p.root());
    let id = a["diagnostics"][0]["id"].as_str().unwrap().to_owned();
    let state = a["state"]["source"].as_str().unwrap().to_owned();
    // A fake id in the current state: no such diagnostic, whatever a client believes.
    assert_eq!(
        detail(&p.root(), "N0300@main.nz:0-1#0", &state)["reason"],
        "no_such_diagnostic"
    );
    assert_eq!(
        detail(&p.root(), &id, &"0".repeat(64))["reason"],
        "stale_state"
    );
    // Moved: a line above. The old id and state are stale; a fresh index has a fresh id.
    p.write("main.nz", &format!("// moved\n{IMMUTABLE}"));
    assert_eq!(detail(&p.root(), &id, &state)["reason"], "stale_state");
    let b = index(&p.root());
    assert_ne!(b["state"], a["state"]);
    assert_ne!(b["diagnostics"][0]["id"], a["diagnostics"][0]["id"]);
    // Repaired: gone, and the old id is stale — not the next diagnostic's.
    p.write("main.nz", &IMMUTABLE.replace("let x", "let mut x"));
    assert_eq!(detail(&p.root(), &id, &state)["reason"], "stale_state");
    assert_eq!(index(&p.root())["counts"]["total"], 0);
    // The same text again is the same state, and the old id means what it meant.
    p.write("main.nz", IMMUTABLE);
    assert_eq!(detail(&p.root(), &id, &state)["status"], "available");
}

#[test]
fn the_same_sources_give_the_same_bytes_from_any_directory_and_only_the_roots_files() {
    let a = Project::new(&[("main.nz", TYPED)]);
    let b = Project::new(&[("main.nz", TYPED)]);
    let first = index_json(&a.root());
    assert_eq!(first, index_json(&a.root()));
    assert_eq!(first, index_json(&b.root()));
    for dir in [a.dir.path(), b.dir.path()] {
        assert!(!first.contains(dir.to_str().unwrap()), "{first}");
    }
    // An edit in an imported file is another state, though no diagnostic moved.
    let two = Project::new(&[
        (
            "main.nz",
            "use \"lib.nz\";\nfn main() -> Int { 1 + true }\n",
        ),
        ("lib.nz", "pub fn one() -> Int { 1 }\n"),
    ]);
    let before = index(&two.root());
    two.write("lib.nz", "// edited\npub fn one() -> Int { 1 }\n");
    let after = index(&two.root());
    assert_ne!(before["state"]["source"], after["state"]["source"]);
    assert_eq!(before["diagnostics"], after["diagnostics"]);
    // A broken file nothing imports is not in the compilation, and not in the index.
    a.write("stray.nz", "fn stray() -> Int { true }\n");
    assert_eq!(index_json(&a.root()), first);
    // Two roots over one shared file: each index is of its own compilation.
    let shared = Project::new(&[
        ("common.nz", "pub fn shared(n: Int) -> Int { n }\n"),
        (
            "a.nz",
            "use \"common.nz\";\nfn main() -> Int { shared(true) }\n",
        ),
        (
            "b.nz",
            "use \"common.nz\";\nfn main() -> Int { shared(1) }\n",
        ),
    ]);
    assert_eq!(index(&shared.path("a.nz"))["counts"]["total"], 1);
    assert_eq!(index(&shared.path("b.nz"))["counts"]["total"], 0);
}

#[test]
fn a_thousand_indexes_and_details_keep_nothing() {
    let p = Project::new(&[("main.nz", TYPED)]);
    let root = p.root();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        let first = s.diagnostic_index(&root, 0).to_json();
        let i: Value = serde_json::from_str(&first).unwrap();
        let id = i["diagnostics"][0]["id"].as_str().unwrap().to_owned();
        let state = i["state"]["source"].as_str().unwrap().to_owned();
        let d = s.diagnostic_detail(&root, 0, &id, &state).to_json();
        let retained = s.retained();
        for n in 0..500 {
            assert_eq!(s.diagnostic_index(&root, 0).to_json(), first, "{n}");
            assert_eq!(
                s.diagnostic_detail(&root, 0, &id, &state).to_json(),
                d,
                "{n}"
            );
        }
        assert_eq!(s.retained(), retained);
        assert_eq!(s.open_documents(), 1);
    });
}

#[test]
fn every_kind_of_semantic_diagnostic_is_indexed_with_its_owner_and_detail() {
    // Each an actual diagnostic of the compiler, with the definition it is inside — or none,
    // for a declaration the checker refused.
    let lib = "fn hidden() -> Int { 1 }\npub fn open() -> Int { 2 }\n";
    let cases: [(&str, &str, &str, Option<&str>); 8] = [
        ("unknown name", "fn main() -> Int { missing() }\n", "N0201", Some("main.nz::fn main")),
        ("type mismatch", "fn main() -> Int { 1 + true }\n", "N0304", Some("main.nz::fn main")),
        (
            "wrong arity",
            "fn f(n: Int) -> Int { n }\nfn main() -> Int { f(1, 2) }\n",
            "N0303",
            Some("main.nz::fn main"),
        ),
        (
            "visibility",
            "use \"lib.nz\";\nfn main() -> Int { hidden() }\n",
            "N0206",
            Some("main.nz::fn main"),
        ),
        (
            "duplicate declaration",
            "fn twice() -> Int { 1 }\nfn twice() -> Int { 2 }\nfn main() -> Int { twice() }\n",
            "N0203",
            None,
        ),
        (
            "generic mismatch",
            "fn id[T](x: T) -> T { x }\nfn main() -> Int { id(true) }\n",
            "N0300",
            Some("main.nz::fn main"),
        ),
        (
            "unsatisfied requirement",
            "fn same[T: Equality](a: T, b: T) -> Bool { a == b }\nfn wrap[T](a: T, b: T) -> Bool { same(a, b) }\nfn main() -> Int { if wrap(1, 1) { 1 } else { 0 } }\n",
            "N0364",
            Some("main.nz::fn wrap"),
        ),
        (
            "task capture",
            "fn use_seq(xs: Ints) -> Int { ints_push(xs, 1) }\nfn main(tasks: SpawnCap) -> Int {\n    let xs = ints_new();\n    scope { spawn use_seq(xs); }\n    ints_len(xs)\n}\n",
            "N0321",
            Some("main.nz::fn main"),
        ),
    ];
    for (what, text, code, owner) in cases {
        let p = Project::new(&[("main.nz", text), ("lib.nz", lib)]);
        let i = index(&p.root());
        let d = i["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["code"] == code)
            .unwrap_or_else(|| panic!("{what}: no {code} in {i}"));
        assert_eq!(d["owner"]["identity"].as_str(), owner, "{what}: {d}");
        let v = detail(
            &p.root(),
            d["id"].as_str().unwrap(),
            i["state"]["source"].as_str().unwrap(),
        );
        assert_eq!(v["diagnostic"]["code"], code, "{what}");
        assert_eq!(v["locations"]["primary"], d["primary"], "{what}");
    }
}
