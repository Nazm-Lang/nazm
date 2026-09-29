//! Semantic context packets through the language service. N27.
//!
//! Packets are compared as the JSON they serialise to, since that is the contract: every
//! link by its durable identity and where it is declared, never by a name alone, so two
//! modules' `helper`, a record and a function both called `Box`, or a local taken for a
//! dependency fails.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_service::{ContextResult, LanguageService};
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
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

/// The byte offset of the `n`th occurrence of `needle`.
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

fn span(text: &str, needle: &str, n: usize) -> (u32, u32) {
    let s = at(text, needle, n);
    (s, s + u32::try_from(needle.len()).unwrap())
}

/// The packet for byte `offset` of `file` in the compilation of `root`, read from disk by a
/// fresh service — exactly what `nazm context` and `nazm-mcp` do — as its canonical JSON.
fn packet_json(root: &Path, file: &Path, offset: u32) -> String {
    let (root, file) = (root.to_path_buf(), file.to_path_buf());
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        s.context(&root, 0, &file, offset).to_json()
    })
}

fn packet(root: &Path, file: &Path, offset: u32) -> Value {
    serde_json::from_str(&packet_json(root, file, offset)).unwrap()
}

fn names(links: &Value) -> Vec<(String, String)> {
    links
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["kind"].as_str().unwrap().to_owned(),
                l["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn occurrences(link: &Value) -> Vec<(u32, u32)> {
    link["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                u32::try_from(o["start"].as_u64().unwrap()).unwrap(),
                u32::try_from(o["end"].as_u64().unwrap()).unwrap(),
            )
        })
        .collect()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_owned(), b.to_owned())
}

// ---------------------------------------------------------------------------------------
// The target.
// ---------------------------------------------------------------------------------------

const MAIN: &str = "\
use \"lib.nz\";
// A record, a function of the same name, and an enum.
struct Box {
    value: Int,
}
enum State {
    Ready,
    Done(code: Int),
}
fn Box(value: Int) -> Int { value }
/// Uses what it can.
fn user(b: Box, s: State) -> Int {
    let local = b.value;
    let made = Box(value: local);
    let d = State.Done(code: 1);
    let n = ints_new();
    helper(local) + helper(Box(made.value)) + far()
}
fn caller() -> Int { user(Box(value: 1), State.Ready()) }
";
const LIB: &str = "pub fn far() -> Int { helper() }\nfn helper() -> Int { 7 }\n";

fn main_and_lib() -> Project {
    // `helper` in main is a second function of the name: defined here, private to it.
    let main = format!("{MAIN}fn helper(x: Int) -> Int {{ x }}\n");
    Project::new(&[("main.nz", &main), ("lib.nz", LIB)])
}

fn main_text() -> String {
    format!("{MAIN}fn helper(x: Int) -> Int {{ x }}\n")
}

#[test]
fn a_target_is_the_definition_a_declaration_or_a_use_names() {
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    // A function, from its declaration and from a call: the same packet, byte for byte.
    let from_decl = packet_json(&main, &main, at(&text, "user(b", 0));
    let from_call = packet_json(&main, &main, at(&text, "user(Box", 0));
    assert_eq!(from_decl, from_call);
    let v: Value = serde_json::from_str(&from_decl).unwrap();
    assert_eq!(v["status"], "available");
    assert_eq!(v["target"]["kind"], "function");
    assert_eq!(
        v["target"]["identity"],
        json!({"definition": "main.nz::fn user", "member": null})
    );
    // The record `Box`, from its declaration, a written type and a construction; the
    // function `Box`, from its declaration and a call. Never one for the other.
    let record = packet_json(&main, &main, at(&text, "Box {", 0));
    assert_eq!(record, packet_json(&main, &main, at(&text, "Box, s", 0)));
    assert_eq!(
        record,
        packet_json(&main, &main, at(&text, "Box(value: local", 0))
    );
    let r: Value = serde_json::from_str(&record).unwrap();
    assert_eq!(r["target"]["identity"]["definition"], "main.nz::record Box");
    assert_eq!(
        r["target"]["fields"],
        json!([{"name": "value", "type": "Int"}])
    );
    let function = packet_json(&main, &main, at(&text, "Box(value: Int)", 0));
    assert_eq!(
        function,
        packet_json(&main, &main, at(&text, "Box(made", 0))
    );
    let f: Value = serde_json::from_str(&function).unwrap();
    assert_eq!(f["target"]["identity"]["definition"], "main.nz::fn Box");
    assert_eq!(f["target"]["signature"], "fn Box(value: Int) -> Int");
    // The enum, from its declaration and from the qualifier of a variant.
    let e = packet(&main, &main, at(&text, "State {", 0));
    assert_eq!(e, packet(&main, &main, at(&text, "State.Done", 0)));
    assert_eq!(
        e["target"]["variants"],
        json!([
            {"name": "Done", "fields": [{"name": "code", "type": "Int"}]},
            {"name": "Ready", "fields": []}
        ])
    );
    // Two functions called `helper`, in two modules: each its own.
    let lib = p.path("lib.nz");
    let mine = packet(&main, &main, at(&text, "helper(x", 0));
    let theirs = packet(&main, &lib, at(LIB, "helper() -> Int", 0));
    assert_eq!(
        mine["target"]["identity"]["definition"],
        "main.nz::fn helper"
    );
    assert_eq!(
        theirs["target"]["identity"]["definition"],
        "lib.nz::fn helper"
    );
    assert_eq!(theirs["target"]["source"]["file"], "lib.nz");
}

#[test]
fn a_position_that_names_no_source_definition_has_no_packet() {
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    let reason = |offset: u32| packet(&main, &main, offset)["reason"].clone();
    // A local, a parameter, a field, a variant, a payload label, an intrinsic, a built-in
    // type: no packet of their own.
    for needle in [
        "local =",
        "b: Box",
        "value;",
        "Done(code: 1",
        "code: 1",
        "ints_new",
        "Int {",
    ] {
        assert_eq!(reason(at(&text, needle, 0)), "not_a_target", "{needle}");
    }
    // Whitespace, a keyword, a comment: not a name.
    for needle in ["\n", "fn user", "// A"] {
        assert_eq!(reason(at(&text, needle, 0)), "not_a_name", "{needle:?}");
    }
    assert_eq!(
        reason(u32::try_from(text.len()).unwrap() + 1),
        "out_of_range"
    );
    // A file the compilation did not load — even one sitting beside it — is never read.
    std::fs::write(p.path("other.nz"), "fn other() -> Int { 0 }\n").unwrap();
    let v = packet(&main, &p.path("other.nz"), 3);
    assert_eq!(
        v,
        json!({"schema": "nazm.context/2", "status": "unavailable", "reason": "not_in_compilation"})
    );
    assert_eq!(
        packet(&main, Path::new("/etc/passwd"), 0)["reason"],
        "not_in_compilation"
    );
}

#[test]
fn the_target_source_is_the_declaration_byte_for_byte() {
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    let v = packet(&main, &main, at(&text, "user(b", 0));
    let source = &v["target"]["source"];
    let (start, end) = (
        usize::try_from(source["start"].as_u64().unwrap()).unwrap(),
        usize::try_from(source["end"].as_u64().unwrap()).unwrap(),
    );
    assert_eq!(source["text"].as_str().unwrap(), &text[start..end]);
    assert!(text[start..end].starts_with("fn user(b: Box"));
    assert!(text[start..end].ends_with("far()\n}"));
    assert_eq!(
        (
            source["name_start"].as_u64().unwrap(),
            source["name_end"].as_u64().unwrap()
        ),
        (
            u64::from(span(&text, "user", 0).0),
            u64::from(span(&text, "user", 0).1)
        )
    );
    assert_eq!(source["file"], "main.nz");
    assert_eq!(v["root"], "main.nz");
}

// ---------------------------------------------------------------------------------------
// Links.
// ---------------------------------------------------------------------------------------

#[test]
fn dependencies_are_the_entities_the_declaration_uses_grouped_by_identity() {
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    let v = packet(&main, &main, at(&text, "user(b", 0));
    // Functions, then records, enums, fields, variants, payload fields — each once, by
    // where it is declared: file (`lib.nz` before `main.nz`), then position. No local, no parameter, no intrinsic, and not `user` itself.
    assert_eq!(
        names(&v["dependencies"]),
        [
            pair("function", "far"),
            pair("function", "Box"),
            pair("function", "helper"),
            pair("record", "Box"),
            pair("enum", "State"),
            pair("field", "value"),
            pair("variant", "Done"),
            pair("payload_field", "Done.code"),
        ]
    );
    let deps = v["dependencies"].as_array().unwrap();
    // `helper` twice, one entry with both calls; the one declared in main, not lib's.
    let helper = &deps[2];
    assert_eq!(helper["identity"]["definition"], "main.nz::fn helper");
    assert_eq!(
        occurrences(helper),
        [
            span(&text, "helper(local", 0).0,
            span(&text, "helper(Box", 0).0
        ]
        .map(|s| (s, s + 6))
    );
    assert_eq!(deps[0]["identity"]["definition"], "lib.nz::fn far");
    assert_eq!(deps[0]["declaration"]["file"], "lib.nz");
    // The record: named as a type once and constructed once.
    assert_eq!(
        occurrences(&deps[3]),
        [span(&text, "Box, s", 0), span(&text, "Box(value: local", 0)].map(|(s, _)| (s, s + 3))
    );
    // A field's identity is its record's key and its name.
    assert_eq!(
        deps[5]["identity"],
        json!({"definition": "main.nz::record Box", "member": "value"})
    );
    // Related types: the records and enums whose names the checker recorded as written
    // types here — a parameter's type, a construction's head, a variant's enum — with those
    // occurrences. The function `Box` called here is not one: it is no type.
    assert_eq!(
        names(&v["related_types"]),
        [pair("record", "Box"), pair("enum", "State")]
    );
    assert_eq!(
        occurrences(&v["related_types"][0]),
        [span(&text, "Box, s", 0), span(&text, "Box(value: local", 0)].map(|(s, _)| (s, s + 3))
    );
    assert_eq!(
        occurrences(&v["related_types"][1]),
        [span(&text, "State)", 0), span(&text, "State.Done", 0)].map(|(s, _)| (s, s + 5))
    );
}

#[test]
fn related_types_are_direct_and_a_source_less_definition_is_no_link() {
    const TEXT: &str = "\
struct Inner {
    n: Int,
}
struct Outer {
    inner: Inner,
}
fn wrap(o: Outer) -> Option[Int] { Option.Some(value: o.inner.n) }
fn check(r: Result[Int, Str]) -> Bool { match r { Result.Ok(value: v) => v > 0, Result.Err(error: e) => false, } }
";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    let v = packet(&main, &main, at(TEXT, "wrap", 0));
    // `Outer` is written; `Inner` is only reached through it, and is no related type.
    let related = names(&v["related_types"]);
    assert_eq!(related, [pair("record", "Outer")]);
    // The core prelude's `Option` and its `Some` shaped the checking, and have no file:
    // they are no link at all — not a link without a place. `Int`, a built-in, neither.
    // The project's own record and field are still there, each at its declaration.
    assert_eq!(
        names(&v["dependencies"]),
        [
            pair("record", "Outer"),
            pair("field", "n"),
            pair("field", "inner"),
        ]
    );
    let r = packet(&main, &main, at(TEXT, "check", 0));
    assert_eq!(r["dependencies"], json!([]), "{r}");
    assert_eq!(r["related_types"], json!([]));
    for packet in [&v, &r] {
        let text = packet.to_string();
        assert!(!text.contains("@core"), "{text}");
        assert!(!text.contains("\"declaration\":null"), "{text}");
    }
}

#[test]
fn every_link_has_a_place_in_the_compilation() {
    // Across a function's, a record's and an enum's packets: every dependency, related type,
    // caller and callee is declared in a file the compilation loaded, at a real range.
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    for needle in [
        "user(b",
        "caller()",
        "Box {",
        "State {",
        "helper(x",
        "Box(value: Int)",
    ] {
        let v = packet(&main, &main, at(&text, needle, 0));
        let mut links: Vec<Value> = Vec::new();
        for section in ["dependencies", "related_types"] {
            links.extend(v[section].as_array().unwrap().iter().cloned());
        }
        for calls in ["callers", "callees"] {
            if let Some(items) = v[calls]["items"].as_array() {
                links.extend(items.iter().cloned());
            }
        }
        for l in &links {
            let d = &l["declaration"];
            let file = d["file"]
                .as_str()
                .unwrap_or_else(|| panic!("{needle}: {l}"));
            assert!(["main.nz", "lib.nz"].contains(&file), "{needle}: {l}");
            let (start, end) = (d["start"].as_u64().unwrap(), d["end"].as_u64().unwrap());
            assert!(start < end, "{needle}: {l}");
            let source = std::fs::read_to_string(p.path(file)).unwrap();
            assert_eq!(
                &source[usize::try_from(start).unwrap()..usize::try_from(end).unwrap()],
                l["name"].as_str().unwrap().rsplit('.').next().unwrap(),
                "{needle}: {l}"
            );
        }
    }
}

#[test]
fn a_private_definition_is_a_dependency_like_any_other() {
    // `hidden` is private to `lib.nz`, and `lib.nz`'s own `open` uses it: the packet of
    // `open`, asked of `lib.nz` in `main.nz`'s compilation, links it with its place.
    const LIB: &str = "fn hidden() -> Int { 1 }\npub fn open() -> Int { hidden() }\n";
    const MAIN: &str = "use \"lib.nz\";\nfn m() -> Int { open() }\n";
    let p = Project::new(&[("main.nz", MAIN), ("lib.nz", LIB)]);
    let v = packet(&p.path("main.nz"), &p.path("lib.nz"), at(LIB, "open", 0));
    assert_eq!(
        v["dependencies"],
        json!([{
            "identity": {"definition": "lib.nz::fn hidden", "member": null},
            "kind": "function",
            "name": "hidden",
            "declaration": {"file": "lib.nz", "start": 3, "end": 9},
            "occurrences": [{"start": at(LIB, "hidden()", 1), "end": at(LIB, "hidden()", 1) + 6}]
        }])
    );
}

#[test]
fn references_are_every_use_and_not_the_declaration() {
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    let v = packet(&main, &main, at(&text, "Box {", 0));
    let refs: Vec<(String, u64)> = v["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["file"].as_str().unwrap().to_owned(),
                r["start"].as_u64().unwrap(),
            )
        })
        .collect();
    let expected: Vec<(String, u64)> = [
        span(&text, "Box, s", 0).0,
        span(&text, "Box(value: local", 0).0,
        span(&text, "Box(value: 1)", 0).0,
    ]
    .iter()
    .map(|s| ("main.nz".to_owned(), u64::from(*s)))
    .collect();
    assert_eq!(refs, expected);
    assert!(!refs.contains(&("main.nz".to_owned(), u64::from(at(&text, "Box {", 0)))));
}

#[test]
fn callers_and_callees_are_call_hierarchys_and_a_type_has_none() {
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    let v = packet(&main, &main, at(&text, "user(b", 0));
    assert_eq!(v["callers"]["status"], "available");
    let callers = v["callers"]["items"].as_array().unwrap();
    assert_eq!(callers.len(), 1);
    assert_eq!(callers[0]["identity"]["definition"], "main.nz::fn caller");
    assert_eq!(
        callers[0]["calls"],
        json!([{"file": "main.nz", "start": at(&text, "user(Box", 0), "end": at(&text, "user(Box", 0) + 4}])
    );
    let callees: Vec<String> = v["callees"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["identity"]["definition"].as_str().unwrap().to_owned())
        .collect();
    // `ints_new` is an intrinsic, and the constructions are not calls.
    assert_eq!(
        callees,
        ["lib.nz::fn far", "main.nz::fn Box", "main.nz::fn helper"]
    );
    // The same as call hierarchy answers, prepared at the same name.
    let (root, at_user) = (main.clone(), at(&text, "user(b", 0));
    let hierarchy: Vec<String> = on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        let item = s
            .prepare_call_hierarchy(&root, 0, at_user)
            .unwrap()
            .unwrap();
        s.outgoing_calls(&item.locator)
            .unwrap()
            .into_iter()
            .map(|c| c.to.name)
            .collect()
    });
    assert_eq!(hierarchy, ["far", "Box", "helper"]);
    // A record and an enum: not applicable, which is not an empty list.
    for needle in ["Box {", "State {"] {
        let t = packet(&main, &main, at(&text, needle, 0));
        assert_eq!(
            t["callers"],
            json!({"status": "not_applicable", "items": null})
        );
        assert_eq!(
            t["callees"],
            json!({"status": "not_applicable", "items": null})
        );
    }
}

#[test]
fn diagnostics_are_the_targets_own_and_what_is_not_known_is_unsupported() {
    const TEXT: &str =
        "fn good() -> Int { 1 }\nfn bad() -> Int { \"s\" }\nfn worse() -> Int { true }\n";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    // A type error does not stop a packet; it is in the packet of the function it is in.
    let bad = packet(&main, &main, at(TEXT, "bad", 0));
    let diags = bad["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0]["start"], at(TEXT, "\"s\"", 0));
    assert_eq!(diags[0]["severity"], "error");
    let good = packet(&main, &main, at(TEXT, "good", 0));
    assert_eq!(good["diagnostics"], json!([]));
    assert_eq!(
        good["availability"],
        json!({
            "capabilities": "unsupported",
            "tests": "unsupported",
            "semantic_changes": "unsupported",
            "semantic_delta": "unsupported"
        })
    );
    // A syntax error anywhere in the compilation: no packet at all.
    std::fs::write(
        &main,
        format!("{TEXT}fn broken() -> Int {{ let x = ; 1 }}\n"),
    )
    .unwrap();
    assert_eq!(
        packet(&main, &main, at(TEXT, "good", 0))["reason"],
        "incomplete_compilation"
    );
    // A module that cannot be loaded, likewise.
    std::fs::write(&main, format!("use \"gone.nz\";\n{TEXT}")).unwrap();
    assert_eq!(
        packet(&main, &main, at(TEXT, "good", 0) + 15)["reason"],
        "incomplete_compilation"
    );
}

// ---------------------------------------------------------------------------------------
// The compilation, and the bytes.
// ---------------------------------------------------------------------------------------

#[test]
fn a_packet_answers_from_its_roots_compilation_and_no_other() {
    const COMMON: &str = "pub fn shared() -> Int { 0 }\n";
    const A: &str = "use \"common.nz\";\nfn only_a() -> Int { shared() }\n";
    const B: &str = "use \"common.nz\";\nfn only_b() -> Int { shared() + shared() }\n";
    let p = Project::new(&[("common.nz", COMMON), ("a.nz", A), ("b.nz", B)]);
    let (a, b, common) = (p.path("a.nz"), p.path("b.nz"), p.path("common.nz"));
    let at_shared = at(COMMON, "shared", 0);
    let via_a = packet(&a, &common, at_shared);
    let via_b = packet(&b, &common, at_shared);
    let files = |v: &Value, key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["file"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(files(&via_a, "references"), ["a.nz"]);
    assert_eq!(files(&via_b, "references"), ["b.nz", "b.nz"]);
    assert_eq!(via_a["callers"]["items"][0]["name"], "only_a");
    assert_eq!(via_b["callers"]["items"][0]["name"], "only_b");
    assert_eq!(via_a["target"], via_b["target"]);
    assert_eq!(via_a["root"], "a.nz");
}

#[test]
fn the_same_sources_and_question_give_the_same_bytes() {
    let p = main_and_lib();
    let main = p.path("main.nz");
    let text = main_text();
    let offset = at(&text, "user(b", 0);
    let (root, first) = (main.clone(), packet_json(&main, &main, offset));
    // A hundred times from one service, and again from a fresh one: identical.
    let again: Vec<String> = on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open_from_disk(&root).unwrap();
        (0..100)
            .map(|_| s.context(&root, 0, &root, offset).to_json())
            .collect()
    });
    assert!(again.iter().all(|j| *j == first));
    // Nothing of where it ran: no absolute path, no temporary directory.
    let dir = p.dir.path().to_string_lossy().into_owned();
    assert!(!first.contains(&dir), "{first}");
    assert!(!first.contains("\"/"), "no absolute path: {first}");
    // The same project copied elsewhere: the same bytes.
    let q = main_and_lib();
    assert_eq!(
        packet_json(&q.path("main.nz"), &q.path("main.nz"), offset),
        first
    );
}

#[test]
fn each_packet_is_of_the_disk_as_it_is_then() {
    const ONE: &str = "fn g() -> Int { 1 }\nfn f() -> Int { g() }\n";
    const TWO: &str = "fn g() -> Int { 1 }\nfn h() -> Int { 2 }\nfn f() -> Int { h() + g() }\n";
    let p = Project::new(&[("main.nz", ONE)]);
    let main = p.path("main.nz");
    let callees = |v: &Value| -> Vec<String> {
        v["callees"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(callees(&packet(&main, &main, at(ONE, "f()", 0))), ["g"]);
    std::fs::write(&main, TWO).unwrap();
    assert_eq!(
        callees(&packet(&main, &main, at(TWO, "f()", 0))),
        ["g", "h"]
    );
    // `f` gone: the position names nothing now, and nothing from before is served.
    std::fs::write(&main, "fn g() -> Int { 1 }\n").unwrap();
    assert_eq!(
        packet(&main, &main, at(TWO, "f()", 0))["status"],
        "unavailable"
    );
}

#[test]
fn every_definition_of_the_repository_has_a_packet_every_layer_agrees_with() {
    // Over real source: for every function, record and enum of every clean document, the
    // packet's ranges and signature are its outline symbol's (N22); its identifier is
    // classified as that kind's declaration (N23); each dependency occurrence is a use the
    // reference index gives the linked entity (N17); its callers and callees are call
    // hierarchy's (N25); and it serialises to the same bytes twice.
    use nazm_core::sema::ReferenceTarget;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf();
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["compiler", "examples"] {
        let mut stack = vec![root.join(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "nz") {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    on_the_service_stack(move || {
        let (mut documents, mut packets, mut deps, mut calls) = (0, 0, 0, 0);
        let (mut largest, mut largest_source, mut largest_name) = (0usize, 0usize, String::new());
        for path in &files {
            let mut s = LanguageService::new();
            let published = s.open_from_disk(path).unwrap();
            if !published[0].diagnostics.is_empty() {
                continue;
            }
            let decided = nazm_service::root::decide(path, None).unwrap();
            let loaded = nazm_service::load::load(path, decided.path()).unwrap();
            let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
            if !a.diagnostics.is_empty() {
                continue;
            }
            let index = nazm_core::sema::ReferenceIndex::of(&a.resolution);
            documents += 1;
            let symbols = s.document_symbols(path, 0).unwrap().symbols;
            let identifiers = s.semantic_identifiers(path, 0).unwrap();
            for sym in &symbols {
                let result = s.context(path, 0, path, sym.name_start);
                let json = result.to_json();
                assert_eq!(json, result.to_json());
                let ContextResult::Available(p) = result else {
                    panic!("{}: {} has no packet", path.display(), sym.name);
                };
                packets += 1;
                // N22.
                assert_eq!(
                    (
                        p.target.source.start,
                        p.target.source.end,
                        p.target.source.name_start,
                        p.target.source.name_end
                    ),
                    (sym.start, sym.end, sym.name_start, sym.name_end)
                );
                assert_eq!(
                    p.target.signature,
                    if sym.kind == nazm_service::SymbolKind::Function {
                        sym.detail.clone()
                    } else {
                        None
                    }
                );
                // N23.
                let class = identifiers
                    .identifiers
                    .iter()
                    .find(|i| i.start == sym.name_start && i.end == sym.name_end)
                    .map(|i| (i.class, i.role));
                let want = match sym.kind {
                    nazm_service::SymbolKind::Function => nazm_service::SemanticClass::Function,
                    nazm_service::SymbolKind::Record => nazm_service::SemanticClass::Record,
                    _ => nazm_service::SemanticClass::Enum,
                };
                assert_eq!(class, Some((want, nazm_service::Role::Declaration)));
                // N17: every dependency occurrence is a use of the linked entity, found
                // there by the reference index — the entity declared where the link says.
                for d in &p.dependencies {
                    deps += 1;
                    for o in &d.occurrences {
                        let t = index
                            .target(nazm_span::Span::new(
                                nazm_span::FileId::ROOT,
                                o.start,
                                o.end,
                            ))
                            .unwrap();
                        assert!(!matches!(t, ReferenceTarget::Local(_)));
                        // Every link is declared in a file of the compilation, at the
                        // place the reference index declares the entity.
                        let span = index.declaration(t).unwrap();
                        assert!(loaded.graph.module_of(span.file) != loaded.graph.prelude());
                        assert_eq!(
                            (d.declaration.start, d.declaration.end),
                            (span.start, span.end),
                            "{}",
                            d.name
                        );
                        assert!(!d.declaration.file.is_empty());
                    }
                }
                // N25.
                if let (Some(callers), Some(callees)) = (&p.callers.items, &p.callees.items) {
                    let item = s
                        .prepare_call_hierarchy(path, 0, sym.name_start)
                        .unwrap()
                        .unwrap();
                    let inc = s.incoming_calls(&item.locator).unwrap();
                    let out = s.outgoing_calls(&item.locator).unwrap();
                    assert_eq!(
                        callers
                            .iter()
                            .map(|c| (c.name.clone(), c.calls.len()))
                            .collect::<Vec<_>>(),
                        inc.iter()
                            .map(|c| (c.from.name.clone(), c.calls.len()))
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        callees
                            .iter()
                            .map(|c| (c.name.clone(), c.calls.len()))
                            .collect::<Vec<_>>(),
                        out.iter()
                            .map(|c| (c.to.name.clone(), c.calls.len()))
                            .collect::<Vec<_>>()
                    );
                    calls += callers.len() + callees.len();
                }
                if json.len() > largest {
                    largest = json.len();
                    largest_source = p.target.source.text.len();
                    largest_name = format!("{}:{}", path.display(), sym.name);
                }
            }
        }
        eprintln!(
            "{documents} documents: {packets} packets, {deps} dependency links, {calls} call edges; largest packet {largest} bytes ({largest_source} bytes of target source) for {largest_name}"
        );
        assert!(documents > 0 && packets > 0 && deps > 0 && calls > 0);
    });
}

#[test]
fn two_dependencies_of_one_spelling_are_two_dependencies() {
    const TEXT: &str = "struct A {\n    value: Int,\n}\nstruct B {\n    value: Int,\n}\nfn f(a: A, b: B) -> Int { a.value + b.value + a.value }\n";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    let v = packet(&main, &main, at(TEXT, "f(a", 0));
    let fields: Vec<&Value> = v["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["kind"] == "field")
        .collect();
    assert_eq!(fields.len(), 2, "{v}");
    assert_eq!(
        fields[0]["identity"],
        json!({"definition": "main.nz::record A", "member": "value"})
    );
    assert_eq!(
        fields[1]["identity"],
        json!({"definition": "main.nz::record B", "member": "value"})
    );
    assert_eq!(occurrences(fields[0]).len(), 2);
    assert_eq!(occurrences(fields[1]).len(), 1);
}

// ---------------------------------------------------------------------------------------
// N36: a function's effects are the checker's.
// ---------------------------------------------------------------------------------------

#[test]
fn a_functions_packet_carries_its_effects_and_why_from_the_checker() {
    const FX: &str = "use \"lib.nz\";\n\
        fn leaf() -> Int { print(\"x\") }\n\
        fn mid() -> Int { leaf() }\n\
        fn pure() -> Int ! {} { 1 }\n\
        fn both(io: IoCap, t: SpawnCap) -> Int ! { io, spawn } { mid() }\n\
        fn far() -> Int { helper() }\n\
        struct P { x: Int, }\n\
        fn main(io: IoCap, t: SpawnCap) -> Int { both(io, t) + pure() + far() + P(x: 1).x }\n";
    let p = Project::new(&[
        ("main.nz", FX),
        ("lib.nz", "pub fn helper() -> Int { 2 }\n"),
    ]);
    let main = p.path("main.nz");
    let of =
        |name: &str| packet(&main, &main, at(FX, &format!("fn {name}("), 0) + 3)["effects"].clone();

    let leaf = of("leaf");
    assert_eq!(leaf["declared"], Value::Null);
    assert_eq!(leaf["required"], json!(["io"]));
    // Declaring nothing publishes nothing: another module must assume every effect.
    assert_eq!(leaf["published"], json!(["io", "spawn"]));
    assert_eq!(
        leaf["reasons"],
        json!([{"effect": "io", "at": {"file": "main.nz", "start": at(FX, "print", 0), "end": at(FX, "print", 0) + 5},
                "source": "builtin", "builtin": "print", "callee": null}])
    );
    let mid = of("mid");
    assert_eq!(mid["reasons"][0]["source"], "inferred");
    assert_eq!(
        mid["reasons"][0]["callee"]["definition"],
        "main.nz::fn leaf"
    );
    assert_eq!(mid["reasons"][0]["at"]["start"], at(FX, "leaf() }", 0));
    let pure = of("pure");
    assert_eq!(
        pure,
        json!({"declared": [], "required": [], "published": [], "reasons": []})
    );
    let both = of("both");
    assert_eq!(both["declared"], json!(["io", "spawn"]));
    assert_eq!(both["required"], json!(["io"]));
    assert_eq!(both["published"], json!(["io", "spawn"]));
    let far = of("far");
    assert_eq!(far["required"], json!(["io", "spawn"]));
    assert!(
        far["reasons"].as_array().unwrap().iter().all(
            |r| r["source"] == "undeclared" && r["callee"]["definition"] == "lib.nz::fn helper"
        ),
        "{far}"
    );
    // A record has no effects to have: null, not empty.
    let record = packet(&main, &main, at(FX, "struct P", 0) + 7);
    assert_eq!(record["effects"], Value::Null);
    assert_eq!(record["schema"], "nazm.context/2");
    // And the signature shows a declared set as the formatter writes it.
    let both_packet = packet(&main, &main, at(FX, "fn both(", 0) + 3);
    assert_eq!(
        both_packet["target"]["signature"],
        "fn both(io: IoCap, t: SpawnCap) -> Int ! { io, spawn }"
    );
}
