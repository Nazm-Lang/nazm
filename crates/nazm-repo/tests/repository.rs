//! The repository map and the task planner over small repositories written for each law,
//! and over this repository where a law is about its real inputs. N33.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use std::path::Path;

use nazm_docs::{Authority, Document, Format};
use nazm_repo::{Budget, Intent, Layout, MapResult, SourceRoot, TaskRequest, FAN_OUT};
use serde_json::Value;

const DOCS: &[Document] = &[
    Document {
        id: "spec",
        path: "docs/spec.md",
        authority: Authority::Normative,
        format: Format::Markdown,
    },
    Document {
        id: "goals",
        path: "docs/goals.md",
        authority: Authority::Goal,
        format: Format::Markdown,
    },
    Document {
        id: "caps",
        path: "docs/caps.md",
        authority: Authority::Evidence,
        format: Format::Markdown,
    },
    Document {
        id: "plan",
        path: "docs/plan.md",
        authority: Authority::Planning,
        format: Format::Markdown,
    },
];

const SOURCES: &[SourceRoot] = &[SourceRoot {
    id: "app",
    dir: "src",
    roots: &["main.nz", "fan.nz", "typed.nz", "broken.nz"],
    docs: &["spec:rules"],
}];

const LAYOUT: Layout = Layout {
    sources: SOURCES,
    documents: DOCS,
    catalogue: Some("mut/catalogue.toml"),
    schemas: Some("schema"),
};

const MAIN: &str = "use \"lib.nz\";\n\nstruct P {\n    x: Int,\n}\n\nfn ping(n: Int) -> Int { pong(n) }\n\nfn pong(n: Int) -> Int { ping(n) }\n\nfn helper(p: P) -> Int { far() + p.x }\n\nfn user() -> Int { helper(P(x: 1)) }\n\nfn main() -> Int { user() + ping(0) }\n";

fn fan() -> String {
    let mut s =
        String::from("use \"lib.nz\";\n\nfn hub() -> Int { 1 }\n\nfn via() -> Int { far() }\n\n");
    for i in 0..20 {
        s.push_str(&format!("fn c{i:02}() -> Int {{ hub() }}\n\n"));
    }
    s.push_str("fn main() -> Int { c00() + via() }\n");
    s
}

const CATALOGUE: &str = r#"
[[mutation]]
name = "helper-drops-the-field"
why = "x"
file = "src/main.nz"
find = "far() + p.x"
replace = "far()"
killers = [{ package = "alpha", test = "flow", name = "helper_is_checked" }]

[[mutation]]
name = "user-builds-nothing"
why = "x"
file = "src/main.nz"
find = "helper(P(x: 1))"
replace = "0"
killers = [{ package = "alpha", test = "flow", name = "user_is_checked" }]

[[mutation]]
name = "alpha-lies"
why = "x"
file = "crates/alpha/src/lib.rs"
find = "true"
replace = "false"
killers = [{ package = "alpha", lib = true, name = "tests::truth" }]

[[profile]]
name = "app"
why = "x"
owns = ["src/"]
targets = [{ package = "alpha", test = "flow" }]

[[profile]]
name = "alpha"
why = "x"
owns = ["crates/alpha/"]
targets = [{ package = "alpha", lib = true }]
"#;

/// A repository with every kind of input: two packages, four documents of four
/// authorities, a catalogue, a schema, and four Nazm compilations — one that needs syntax
/// recovery.
fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_all(dir.path());
    dir
}

fn write_all(root: &Path) {
    let files: Vec<(&str, String)> = vec![
        ("Cargo.toml", "[workspace]\nmembers = [\"crates/alpha\", \"crates/beta\"]\n".into()),
        (
            "crates/alpha/Cargo.toml",
            "[package]\nname = \"alpha\"\n\n[package.metadata.nazm]\ndocs = [\"goals:g1\", \"plan:next\", \"caps:now\", \"spec:rules\"]\n\n[dependencies]\nbeta = { path = \"../beta\" }\n".into(),
        ),
        ("crates/alpha/src/lib.rs", "pub fn truth() -> bool { true }\n".into()),
        ("crates/alpha/tests/flow.rs", "#[test]\nfn helper_is_checked() {}\n".into()),
        ("crates/alpha/tests/helper/data.txt", "not a target\n".into()),
        (
            "crates/beta/Cargo.toml",
            "[package]\nname = \"beta\"\n\n[[bin]]\nname = \"b\"\npath = \"src/main.rs\"\n".into(),
        ),
        ("crates/beta/src/main.rs", "fn main() {}\n".into()),
        ("crates/beta/benches/speed.rs", "fn main() {}\n".into()),
        (
            "docs/spec.md",
            "# Spec\n\nThe language.\n\n## Rules\n<!-- id: rules -->\n\nA rule. See [the evidence](caps.md).\n\n## Other\n<!-- id: other -->\n\nUnrelated text that is not linked from anywhere at all.\n".into(),
        ),
        ("docs/goals.md", "# Goals\n\n## Far away\n<!-- id: g1 -->\n\nAspiration, never evidence.\n".into()),
        ("docs/caps.md", "# Caps\n\n## Now\n<!-- id: now -->\n\nWhat exists today.\n".into()),
        ("docs/plan.md", "# Plan\n\n## Next\n<!-- id: next -->\n\nLater. See [the rules](spec.md#rules).\n".into()),
        ("mut/catalogue.toml", CATALOGUE.into()),
        ("schema/one.json", "{\"properties\": {\"schema\": {\"const\": \"x.one/1\"}}}\n".into()),
        ("src/main.nz", MAIN.into()),
        ("src/lib.nz", "pub fn far() -> Int { 1 }\n".into()),
        ("src/fan.nz", fan()),
        ("src/typed.nz", "fn wrong() -> Int { true }\n\nfn main() -> Int { wrong() }\n".into()),
        ("src/broken.nz", "use \"orphan.nz\";\n\nfn main() -> Int { let = ; }\n".into()),
        ("src/orphan.nz", "pub fn lone() -> Int { 2 }\n".into()),
    ];
    for (path, text) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

fn map(root: &Path) -> Value {
    serde_json::from_str(&nazm_repo::map_of(root, &LAYOUT).to_json()).unwrap()
}

fn ask(root: &Path, intent: Option<Intent>, seeds: &[&str], budget: Budget) -> Value {
    let r = nazm_repo::task_of(
        root,
        &LAYOUT,
        &TaskRequest {
            intent,
            seeds: seeds.iter().map(|s| (*s).to_owned()).collect(),
            state: None,
            budget,
        },
    );
    serde_json::from_str(&r.to_json()).unwrap()
}

fn ids(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_owned())
        .collect()
}

fn item<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("{id} not in {:?}", ids(v)))
}

const NONE: Budget = Budget {
    max_entities: None,
    max_source_bytes: None,
    max_doc_bytes: None,
};

#[test]
fn every_entity_has_its_authoritys_identity_and_nothing_else_is_an_entity() {
    let r = repository();
    let m = map(r.path());
    assert_eq!(m["status"], "available", "{m}");
    let all: Vec<&str> = m["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    for id in [
        "package:alpha",
        "package:beta",
        "target:alpha:lib:alpha",
        "target:alpha:test:flow",
        "target:beta:bin:b",
        "target:beta:bench:speed",
        "sources:app",
        "root:app:main.nz",
        "root:app:broken.nz",
        "module:app:main.nz",
        "module:app:lib.nz",
        "module:app:orphan.nz",
        "def:app:main.nz::fn helper",
        "def:app:main.nz::record P",
        "def:app:lib.nz::fn far",
        "doc:spec",
        "doc:goals",
        "schema:x.one/1",
        "profile:app",
    ] {
        assert!(all.contains(&id), "{id} missing from {all:?}");
    }
    // A directory without a `main.rs` is not a target; `main` of the bin is not a second
    // one; no local, no path and no line is an id.
    assert!(!all.iter().any(|i| i.contains("helper/data")));
    assert!(!all.contains(&"target:beta:bin:beta"));
    assert!(!all.iter().any(|i| i.contains(r.path().to_str().unwrap())));
    assert!(!all.iter().any(|i| i.contains("fn n") || i.contains("::p")));
    // The recovered compilation's modules are mapped, its definitions are not.
    let broken = m["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "root:app:broken.nz")
        .unwrap();
    assert_eq!(broken["detail"], "incomplete");
    assert!(!all.contains(&"def:app:orphan.nz::fn lone"));
    // Relationships name their authority's facts.
    let alpha = m["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "package:alpha")
        .unwrap();
    let rel: Vec<String> = alpha["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            format!(
                "{} {}",
                r["kind"].as_str().unwrap(),
                r["target"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        rel,
        [
            "depends_on package:beta",
            "documented_by doc:caps:now",
            "documented_by doc:goals:g1",
            "documented_by doc:plan:next",
            "documented_by doc:spec:rules",
        ]
    );
    // No body travels in a map.
    let text = nazm_repo::map_of(r.path(), &LAYOUT).to_json();
    assert!(!text.contains("far() + p.x"));
    assert!(!text.contains("What exists today"));
}

#[test]
fn a_map_is_the_same_bytes_every_time_and_from_any_directory() {
    let a = repository();
    let b = repository();
    let first = nazm_repo::map_of(a.path(), &LAYOUT).to_json();
    assert_eq!(first, nazm_repo::map_of(a.path(), &LAYOUT).to_json());
    assert_eq!(first, nazm_repo::map_of(b.path(), &LAYOUT).to_json());
    let t = |root: &Path| {
        nazm_repo::task_of(
            root,
            &LAYOUT,
            &TaskRequest {
                intent: None,
                seeds: vec!["def:app:main.nz::fn helper".into(), "doc:spec:rules".into()],
                state: None,
                budget: NONE,
            },
        )
        .to_json()
    };
    assert_eq!(t(a.path()), t(a.path()));
    assert_eq!(t(a.path()), t(b.path()));
    assert!(!first.contains(a.path().to_str().unwrap()));
}

#[test]
fn the_state_moves_with_every_input_and_a_stale_one_is_refused() {
    let r = repository();
    let state = map(r.path())["state"].as_str().unwrap().to_owned();
    let fresh = |seed: &str, state: &str| -> Value {
        serde_json::from_str(
            &nazm_repo::task_of(
                r.path(),
                &LAYOUT,
                &TaskRequest {
                    intent: None,
                    seeds: vec![seed.to_owned()],
                    state: Some(state.to_owned()),
                    budget: NONE,
                },
            )
            .to_json(),
        )
        .unwrap()
    };
    assert_eq!(
        fresh("def:app:main.nz::fn helper", &state)["status"],
        "complete"
    );
    for (path, text) in [
        ("src/lib.nz", "pub fn far() -> Int { 2 }\n"),
        ("docs/caps.md", "# Caps\n\n## Now\n<!-- id: now -->\n\nWhat exists now.\n"),
        ("crates/beta/Cargo.toml", "[package]\nname = \"beta\"\n\n[[bin]]\nname = \"b\"\npath = \"src/main.rs\"\n# edited\n"),
        ("crates/beta/tests/new.rs", "#[test]\nfn t() {}\n"),
        ("mut/catalogue.toml", &format!("{CATALOGUE}\n# edited\n")),
        ("schema/one.json", "{\"properties\": {\"schema\": {\"const\": \"x.one/1\"}}, \"x\": 1}\n"),
    ] {
        let before = nazm_repo::state(r.path(), &LAYOUT).unwrap();
        let p = r.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        let after = nazm_repo::state(r.path(), &LAYOUT).unwrap();
        assert_ne!(before, after, "{path} did not move the state");
        let v = fresh("def:app:main.nz::fn helper", &before);
        assert_eq!(v["status"], "stale_state", "{path}");
        assert_eq!(v["state"], after.as_str());
        assert!(v.get("items").is_none());
    }
    // A file of a source root that no compilation loads is still in the state.
    let before = nazm_repo::state(r.path(), &LAYOUT).unwrap();
    std::fs::write(r.path().join("src/unused.nz"), "fn x() -> Int { 1 }\n").unwrap();
    assert_ne!(before, nazm_repo::state(r.path(), &LAYOUT).unwrap());
}

#[test]
fn an_understand_context_is_the_seed_its_direct_links_and_its_authority() {
    let r = repository();
    let v = ask(
        r.path(),
        Some(Intent::Understand),
        &["def:app:main.nz::fn helper"],
        NONE,
    );
    assert_eq!(v["status"], "complete", "{v}");
    let seed = item(&v, "def:app:main.nz::fn helper");
    assert_eq!(seed["reason"], "seed");
    assert_eq!(seed["content"]["signature"], "fn helper(p: P) -> Int");
    assert_eq!(
        seed["content"]["source"],
        "fn helper(p: P) -> Int { far() + p.x }"
    );
    assert_eq!(seed["source_bytes"], 38);
    assert_eq!(seed["retrieval"]["schema"], "nazm.context/1");
    let byte = seed["retrieval"]["args"]["byte"].as_u64().unwrap() as usize;
    assert_eq!(&MAIN[byte..byte + 6], "helper");
    // What its signature names, and what it uses — by shape, without their source.
    let p = item(&v, "def:app:main.nz::record P");
    assert_eq!(p["reason"], "signature_type");
    assert!(p["content"].get("source").is_none());
    assert_eq!(p["content"]["fields"][0]["name"], "x");
    let far = item(&v, "def:app:lib.nz::fn far");
    assert_eq!(far["reason"], "direct_dependency");
    assert_eq!(far["content"]["signature"], "fn far() -> Int");
    assert_eq!(item(&v, "doc:spec:rules")["reason"], "authoritative_doc");
    // Not what uses it, and not tests: counted, not selected.
    let all = ids(&v);
    assert!(!all.contains(&"def:app:main.nz::fn user".to_owned()));
    assert!(!all.iter().any(|i| i.starts_with("test:")));
    assert!(v["excluded"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["relation"] == "direct_caller" && e["count"] == 1));
    // Every item says why, from where, with what authority.
    for i in v["items"].as_array().unwrap() {
        assert!(i["reason"].is_string() && i["class"].is_string() && i["authority"].is_string());
        assert!(
            i["reason"] == "seed" || i["via"] == "def:app:main.nz::fn helper",
            "{i}"
        );
    }
}

#[test]
fn an_edit_context_adds_what_uses_it_in_every_compilation_and_its_catalogue_tests() {
    let r = repository();
    let v = ask(r.path(), None, &["def:app:lib.nz::fn far"], NONE);
    assert_eq!(v["intent"], "edit");
    let helper = item(&v, "def:app:main.nz::fn helper");
    assert_eq!(helper["reason"], "direct_caller");
    assert_eq!(helper["class"], "edit_safety");
    assert_eq!(helper["compilation"], "root:app:main.nz");
    let via = item(&v, "def:app:fan.nz::fn via");
    assert_eq!(via["reason"], "direct_caller");
    assert_eq!(via["compilation"], "root:app:fan.nz");
    let span = &via["content"]["uses"][0];
    let fan = fan();
    let (a, b) = (
        span["start"].as_u64().unwrap() as usize,
        span["end"].as_u64().unwrap() as usize,
    );
    assert_eq!(&fan[a..b], "far");

    // A type's users are the definitions it is used in.
    let v = ask(
        r.path(),
        Some(Intent::Edit),
        &["def:app:main.nz::record P"],
        NONE,
    );
    assert_eq!(
        item(&v, "def:app:main.nz::fn helper")["reason"],
        "direct_reference"
    );
    assert_eq!(
        item(&v, "def:app:main.nz::fn user")["reason"],
        "direct_reference"
    );

    // Tests: the killer of the mutation inside the seed, and the profile's target — not the
    // killer of the mutation in the next function, and not a test named like it.
    let v = ask(
        r.path(),
        Some(Intent::Test),
        &["def:app:main.nz::fn helper"],
        NONE,
    );
    let tests: Vec<String> = ids(&v)
        .into_iter()
        .filter(|i| !i.starts_with("def:"))
        .collect();
    assert_eq!(
        tests,
        [
            "target:alpha:test:flow",
            "test:alpha:test:flow::helper_is_checked"
        ]
    );
    let killer = item(&v, "test:alpha:test:flow::helper_is_checked");
    assert_eq!(killer["reason"], "killer_test");
    assert_eq!(killer["content"]["mutations"][0], "helper-drops-the-field");
    assert_eq!(
        item(&v, "target:alpha:test:flow")["reason"],
        "profile_target"
    );
}

#[test]
fn documentation_is_the_selected_section_never_the_whole_document() {
    let r = repository();
    let v = ask(
        r.path(),
        Some(Intent::Understand),
        &["doc:spec:rules"],
        NONE,
    );
    let seed = item(&v, "doc:spec:rules");
    let section = nazm_docs::section_of(r.path(), DOCS, "spec:rules", None);
    let nazm_docs::SectionResult::Available(s) = section else {
        panic!()
    };
    assert_eq!(seed["content"]["body"], s.body.as_str());
    assert!(!seed["content"]["body"]
        .as_str()
        .unwrap()
        .contains("Unrelated"));
    assert_eq!(seed["doc_bytes"], s.body.len());
    assert_eq!(seed["authority"], "normative");
    assert_eq!(item(&v, "doc:caps")["reason"], "linked_section");
    assert!(!ids(&v).contains(&"doc:spec:other".to_owned()));
    // An edit adds what links to it, by id and without its text.
    let v = ask(r.path(), Some(Intent::Edit), &["doc:spec:rules"], NONE);
    let linking = item(&v, "doc:plan:next");
    assert_eq!(linking["reason"], "linking_section");
    assert!(linking["content"].get("body").is_none());
}

#[test]
fn goals_and_plans_never_outrank_the_specification_or_the_evidence() {
    let r = repository();
    let v = ask(r.path(), Some(Intent::Understand), &["package:alpha"], NONE);
    assert_eq!(
        ids(&v),
        [
            "package:alpha",
            "package:beta",
            "doc:spec:rules",
            "doc:caps:now",
            "doc:plan:next",
            "doc:goals:g1",
        ]
    );
    let classes: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["class"].as_str().unwrap())
        .collect();
    assert_eq!(
        classes,
        [
            "seed",
            "correctness",
            "authority",
            "authority",
            "context",
            "context"
        ]
    );
    // Room for one section: the specification's, never the goal's.
    let rules = item(&v, "doc:spec:rules")["doc_bytes"].as_u64().unwrap() as usize;
    let b = Budget {
        max_doc_bytes: Some(rules),
        ..NONE
    };
    let v = ask(r.path(), Some(Intent::Understand), &["package:alpha"], b);
    assert_eq!(v["status"], "partial");
    assert_eq!(ids(&v), ["package:alpha", "package:beta", "doc:spec:rules"]);
}

#[test]
fn a_budget_is_filled_in_priority_order_and_what_it_cuts_is_named() {
    let r = repository();
    let seeds = ["package:alpha"];
    let whole = ask(r.path(), Some(Intent::Understand), &seeds, NONE);
    let rules = item(&whole, "doc:spec:rules")["doc_bytes"]
        .as_u64()
        .unwrap() as usize;
    let caps = item(&whole, "doc:caps:now")["doc_bytes"].as_u64().unwrap() as usize;
    let with = |b: Budget| ask(r.path(), Some(Intent::Understand), &seeds, b);

    // Exactly the first two sections: complete up to them, the rest named.
    let exact = with(Budget {
        max_doc_bytes: Some(rules + caps),
        ..NONE
    });
    assert_eq!(ids(&exact).len(), 4);
    let omitted: Vec<(&str, &str)> = exact["omitted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| (o["id"].as_str().unwrap(), o["dimension"].as_str().unwrap()))
        .collect();
    assert_eq!(
        omitted,
        [("doc:plan:next", "doc_bytes"), ("doc:goals:g1", "priority")]
    );
    assert_eq!(exact["used"]["doc_bytes"], rules + caps);
    // One byte less: the second no longer fits, and everything after it is cut too.
    let below = with(Budget {
        max_doc_bytes: Some(rules + caps - 1),
        ..NONE
    });
    assert_eq!(
        ids(&below),
        ["package:alpha", "package:beta", "doc:spec:rules"]
    );
    assert_eq!(below["omitted"][0]["id"], "doc:caps:now");
    assert_eq!(below["omitted"][0]["dimension"], "doc_bytes");
    // One more is the same as exact, byte for byte.
    let above = with(Budget {
        max_doc_bytes: Some(rules + caps + 1),
        ..NONE
    });
    assert_eq!(ids(&above), ids(&exact));
    // Entities.
    let two = with(Budget {
        max_entities: Some(2),
        ..NONE
    });
    assert_eq!(ids(&two), ["package:alpha", "package:beta"]);
    assert_eq!(two["status"], "partial");
    assert_eq!(two["omitted"][0]["dimension"], "entities");
    // Everything fits: complete, nothing omitted.
    let all = with(Budget {
        max_entities: Some(6),
        ..NONE
    });
    assert_eq!(all["status"], "complete");
    assert!(all["omitted"].as_array().unwrap().is_empty());

    // The seed is never cut: a budget it does not fit is refused, with what it needs.
    let seed = "def:app:main.nz::fn helper";
    for b in [
        Budget {
            max_entities: Some(0),
            ..NONE
        },
        Budget {
            max_source_bytes: Some(37),
            ..NONE
        },
        Budget {
            max_source_bytes: Some(0),
            max_entities: Some(0),
            max_doc_bytes: Some(0),
        },
    ] {
        let v = ask(r.path(), Some(Intent::Understand), &[seed], b);
        assert_eq!(v["status"], "insufficient_budget", "{b:?}");
        assert_eq!(v["required"]["entities"], 1);
        assert_eq!(v["required"]["source_bytes"], 38);
        assert!(v.get("items").is_none());
    }
    let just = ask(
        r.path(),
        Some(Intent::Understand),
        &[seed],
        Budget {
            max_source_bytes: Some(38),
            max_entities: Some(1),
            ..NONE
        },
    );
    assert_eq!(ids(&just), [seed]);
    assert_eq!(just["status"], "partial");
}

#[test]
fn a_cycle_is_one_visit_and_a_wide_relationship_is_cut_and_counted() {
    let r = repository();
    let v = ask(
        r.path(),
        Some(Intent::Understand),
        &["def:app:main.nz::fn ping", "def:app:main.nz::fn pong"],
        NONE,
    );
    assert_eq!(v["status"], "complete");
    let all = ids(&v);
    assert_eq!(
        all.iter()
            .filter(|i| *i == "def:app:main.nz::fn ping")
            .count(),
        1
    );
    assert_eq!(
        all.iter()
            .filter(|i| *i == "def:app:main.nz::fn pong")
            .count(),
        1
    );
    assert_eq!(item(&v, "def:app:main.nz::fn pong")["reason"], "seed");

    let v = ask(
        r.path(),
        Some(Intent::Edit),
        &["def:app:fan.nz::fn hub"],
        NONE,
    );
    assert_eq!(v["status"], "partial");
    let callers = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["reason"] == "direct_caller")
        .count();
    assert_eq!(callers, FAN_OUT);
    assert_eq!(v["bounded"][0]["known"], 20);
    assert_eq!(v["bounded"][0]["included"], FAN_OUT);
    assert_eq!(v["bounded"][0]["reason"], "direct_caller");
    assert!(ids(&v).contains(&"def:app:fan.nz::fn c00".to_owned()));
}

#[test]
fn a_seed_is_an_id_and_never_a_path() {
    let r = repository();
    std::fs::write(r.path().join("secret.nz"), "fn leak() -> Int { 1 }\n").unwrap();
    for seed in [
        "/etc/passwd",
        "../../etc/passwd",
        "module:app:../secret.nz",
        "def:app:../secret.nz::fn leak",
        "root:app:/etc/passwd",
        "src/main.nz",
    ] {
        let v = ask(r.path(), None, &[seed], NONE);
        assert_eq!(v["status"], "invalid_request", "{seed}: {v}");
        assert!(!v.to_string().contains("leak() -> Int"));
    }
    let v = ask(r.path(), None, &["glob:*"], NONE);
    assert_eq!(v["problems"][0]["kind"], "unsupported_kind");
    // Well-formed ids of nothing are not found — never the nearest thing.
    for seed in [
        "def:app:main.nz::fn helpr",
        "module:app:secret.nz",
        "package:gamma",
        "doc:spec:rule",
        "target:alpha:test:flow2",
        "sources:elsewhere",
        "diagnostic:app:typed.nz:N9999@x#0",
    ] {
        let v = ask(r.path(), None, &[seed], NONE);
        assert_eq!(v["status"], "not_found", "{seed}: {v}");
        assert_eq!(v["seeds"][0], seed);
    }
    // No seed, too many, and one twice are malformed requests.
    assert_eq!(ask(r.path(), None, &[], NONE)["status"], "invalid_request");
    let many: Vec<String> = (0..=nazm_repo::MAX_SEEDS)
        .map(|i| format!("package:p{i}"))
        .collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(
        ask(r.path(), None, &many, NONE)["status"],
        "invalid_request"
    );
    assert_eq!(
        ask(r.path(), None, &["package:alpha", "package:alpha"], NONE)["status"],
        "invalid_request"
    );
}

#[test]
fn a_compilation_that_needed_recovery_invents_nothing() {
    let r = repository();
    let v = ask(r.path(), None, &["def:app:orphan.nz::fn lone"], NONE);
    assert_eq!(v["status"], "incomplete_compilation", "{v}");
    assert_eq!(v["seeds"][0], "def:app:orphan.nz::fn lone");
    // Its root is still an entity, and its diagnostics are still the compiler's.
    let v = ask(
        r.path(),
        Some(Intent::Understand),
        &["root:app:broken.nz"],
        NONE,
    );
    assert_eq!(item(&v, "root:app:broken.nz")["content"]["complete"], false);
}

#[test]
fn a_diagnostic_is_found_by_its_id_with_its_owner_and_its_exact_source() {
    let r = repository();
    let v = ask(
        r.path(),
        Some(Intent::Diagnose),
        &["def:app:typed.nz::fn wrong"],
        NONE,
    );
    let d = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["reason"] == "diagnostic_in_seed")
        .unwrap_or_else(|| panic!("{v}"));
    let id = d["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("diagnostic:app:typed.nz:N0300@"), "{id}");
    let v = ask(r.path(), Some(Intent::Diagnose), &[&id], NONE);
    assert_eq!(v["status"], "complete", "{v}");
    let seed = item(&v, &id);
    assert_eq!(seed["content"]["code"], "N0300");
    assert!(seed["content"]["message"]
        .as_str()
        .unwrap()
        .contains("Bool"));
    assert_eq!(seed["retrieval"]["schema"], "nazm.diagnostic-detail/1");
    let owner = item(&v, "def:app:typed.nz::fn wrong");
    assert_eq!(owner["reason"], "diagnostic_owner");
    assert_eq!(owner["content"]["source"], "fn wrong() -> Int { true }");
}

#[test]
fn a_broken_repository_is_unavailable_never_partially_mapped() {
    // A documentation link to nothing.
    let r = repository();
    std::fs::write(
        r.path().join("crates/alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\n\n[package.metadata.nazm]\ndocs = [\"spec:nowhere\"]\n",
    )
    .unwrap();
    let m = map(r.path());
    assert_eq!(m["status"], "unavailable");
    assert_eq!(m["problems"][0]["kind"], "broken_documentation_link");
    let v = ask(r.path(), None, &["package:alpha"], NONE);
    assert_eq!(v["status"], "unavailable");

    // A killer naming a target the workspace does not have.
    let r = repository();
    std::fs::write(
        r.path().join("mut/catalogue.toml"),
        CATALOGUE.replace(
            "test = \"flow\", name = \"user",
            "test = \"gone\", name = \"user",
        ),
    )
    .unwrap();
    assert_eq!(map(r.path())["problems"][0]["kind"], "unresolved_killer");

    // A duplicate package, a malformed manifest, a missing root.
    let r = repository();
    std::fs::write(
        r.path().join("crates/beta/Cargo.toml"),
        "[package]\nname = \"alpha\"\n",
    )
    .unwrap();
    assert!(map(r.path())["problems"]
        .to_string()
        .contains("duplicate_package"));
    let r = repository();
    std::fs::write(r.path().join("crates/beta/Cargo.toml"), "[package\n").unwrap();
    assert!(map(r.path())["problems"]
        .to_string()
        .contains("malformed_manifest"));
    let r = repository();
    std::fs::remove_file(r.path().join("src/fan.nz")).unwrap();
    assert!(map(r.path())["problems"]
        .to_string()
        .contains("missing_root"));

    // A compilation that reads a file outside its source root, which the state does not
    // cover.
    let r = repository();
    std::fs::write(
        r.path().join("src/fan.nz"),
        "use \"../elsewhere.nz\";\nfn main() -> Int { away() }\n",
    )
    .unwrap();
    std::fs::write(
        r.path().join("elsewhere.nz"),
        "pub fn away() -> Int { 1 }\n",
    )
    .unwrap();
    assert!(map(r.path())["problems"]
        .to_string()
        .contains("source_outside_state"));
    let v = ask(r.path(), None, &["def:app:lib.nz::fn far"], NONE);
    assert_eq!(v["status"], "unavailable", "{v}");

    // Or one in a directory below it, which the state does not read either.
    let r = repository();
    std::fs::create_dir_all(r.path().join("src/sub")).unwrap();
    std::fs::write(
        r.path().join("src/sub/inner.nz"),
        "pub fn inner() -> Int { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        r.path().join("src/fan.nz"),
        "use \"sub/inner.nz\";\nfn main() -> Int { inner() }\n",
    )
    .unwrap();
    assert!(map(r.path())["problems"]
        .to_string()
        .contains("source_outside_state"));
}

#[test]
fn the_validator_refuses_every_broken_map() {
    let r = repository();
    let MapResult::Available(good) = nazm_repo::map_of(r.path(), &LAYOUT) else {
        panic!()
    };
    let sections = |id: &str| id == "doc:spec:rules" || id.starts_with("doc:");
    assert!(nazm_repo::validate(&good, &sections).is_empty());
    let broken = |f: &dyn Fn(&mut nazm_repo::RepositoryMap)| {
        let mut m = (*good).clone();
        f(&mut m);
        nazm_repo::validate(&m, &|_| false)
            .into_iter()
            .map(|p| p.kind)
            .collect::<Vec<_>>()
    };
    let duplicate = broken(&|m| {
        let e = m.entities[0].clone();
        m.entities.insert(1, e);
    });
    assert!(duplicate.contains(&"duplicate_entity"), "{duplicate:?}");
    let parent = broken(&|m| {
        let e = m.entities.iter_mut().find(|e| e.kind == "target").unwrap();
        e.parent = Some("package:gone".into());
    });
    assert!(parent.contains(&"broken_parent"), "{parent:?}");
    let relation = broken(&|m| {
        m.entities[0].relations.push(nazm_repo::Relation {
            kind: "depends_on",
            target: "package:zzz".into(),
        });
    });
    assert!(relation.contains(&"broken_relation"), "{relation:?}");
    let retrieval = broken(&|m| {
        let e = m
            .entities
            .iter_mut()
            .find(|e| !e.retrieval.is_empty())
            .unwrap();
        e.retrieval[0].schema = "nazm.source-dump/1";
    });
    assert!(retrieval.contains(&"unknown_retrieval"), "{retrieval:?}");
    let authority = broken(&|m| {
        let e = m
            .entities
            .iter_mut()
            .find(|e| e.kind == "document")
            .unwrap();
        e.authority = Some("rumour");
    });
    assert!(authority.contains(&"unknown_authority"), "{authority:?}");
    let order = broken(&|m| m.entities.reverse());
    assert!(order.contains(&"unordered_entities"), "{order:?}");
    let kind = broken(&|m| m.entities[0].kind = "file");
    assert!(kind.contains(&"unknown_kind"), "{kind:?}");
}

/// Cargo's own description of this workspace, which the adapter has to agree with.
#[test]
fn the_workspace_is_what_cargo_metadata_says() {
    let root = nazm_docs::default_root();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut expected: Vec<String> = Vec::new();
    for p in meta["packages"].as_array().unwrap() {
        let name = p["name"].as_str().unwrap();
        expected.push(format!("package:{name}"));
        for t in p["targets"].as_array().unwrap() {
            let kind = t["kind"][0].as_str().unwrap();
            if matches!(kind, "lib" | "bin" | "test" | "bench" | "example") {
                expected.push(format!(
                    "target:{name}:{kind}:{}",
                    t["name"].as_str().unwrap()
                ));
            }
        }
    }
    expected.sort();
    let MapResult::Available(m) = nazm_repo::map(&root) else {
        panic!("{}", nazm_repo::map(&root).to_json())
    };
    let mut actual: Vec<String> = m
        .entities
        .iter()
        .filter(|e| matches!(e.kind, "package" | "target"))
        .map(|e| e.id.clone())
        .collect();
    actual.sort();
    assert_eq!(actual, expected);
}
