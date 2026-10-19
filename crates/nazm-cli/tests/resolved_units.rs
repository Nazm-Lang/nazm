//! Resolved units (N77, `docs/internal/architecture.md` §7.78): a module's resolution, persisted with
//! its check, read back instead of checking a reused module's bodies again.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const NAZM: &str = env!("CARGO_BIN_EXE_nazm");

const LIB: &str = "pub fn helper(x: Int) -> Int { x + 1 }\n\
pub struct P { a: Int, }\n\
pub enum E { A, B(v: Int), }\n\
fn private_detail() -> Int { 7 }\n";

const MAIN: &str = "use \"lib.nz\";\n\
fn main() -> Int {\n\
    let p = P(a: helper(1));\n\
    match E.B(v: p.a) { E.A() => 0, E.B(v: x) => x + helper(2) }\n\
}\n";

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("nazm.root"), "").unwrap();
    for (n, t) in files {
        std::fs::write(d.path().join(n), t).unwrap();
    }
    d
}

fn nazm(dir: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let o = Command::new(NAZM)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run nazm");
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn resolve(dir: &Path) -> Vec<Value> {
    let (code, out, err) = nazm(dir, &["resolve", "main.nz"]);
    assert_eq!(code, Some(0), "{err}");
    out.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn references(dir: &Path, def: &str) -> Value {
    let (code, out, err) = nazm(dir, &["references", def, "main.nz"]);
    assert_eq!(code, Some(0), "{err}");
    serde_json::from_str(out.trim()).unwrap()
}

fn unit<'a>(units: &'a [Value], module: &str) -> &'a Value {
    units
        .iter()
        .find(|u| u["module"] == module)
        .unwrap_or_else(|| panic!("no unit for {module}"))
}

#[test]
fn a_unit_is_the_same_bytes_in_two_processes_and_two_checkouts() {
    let a = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let b = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let first = nazm(a.path(), &["resolve", "main.nz"]).1;
    let again = nazm(a.path(), &["resolve", "main.nz"]).1;
    let elsewhere = nazm(b.path(), &["resolve", "main.nz"]).1;
    assert!(!first.is_empty());
    assert_eq!(first, again, "computed, then read back from the store");
    assert_eq!(first, elsewhere, "another directory, another process");
    // Nothing a process chose: no path of either checkout appears.
    for d in [a.path(), b.path()] {
        assert!(!first.contains(&d.display().to_string()));
    }
    // One order, the source's: each unit's occurrences by position, as the index holds them.
    let mut ordered = 0;
    for u in resolve(a.path()) {
        let at: Vec<(u64, u64)> = u["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| (o["start"].as_u64().unwrap(), o["end"].as_u64().unwrap()))
            .collect();
        assert!(
            at.windows(2).all(|w| w[0] <= w[1]),
            "{}: {at:?}",
            u["module"]
        );
        ordered += usize::from(at.len() > 1);
    }
    assert!(ordered > 0, "no unit has two occurrences to order");
}

#[test]
fn a_unit_names_every_target_durably_and_no_local() {
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let units = resolve(d.path());
    let main = unit(&units, "main.nz");
    assert_eq!(main["schema"], "nazm.resolved/1");
    assert_eq!(main["imports"][0]["written"], "lib.nz");
    assert_eq!(main["imports"][0]["module"], "lib.nz");
    assert!(main["imports"][0]["interface"].is_string());
    let kinds: BTreeSet<(String, String)> = main["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["target"]["kind"].as_str().unwrap().to_owned(),
                format!(
                    "{}{}",
                    o["target"]["item"].as_str().unwrap(),
                    o["target"]["member"]
                        .as_str()
                        .map(|m| format!(".{m}"))
                        .unwrap_or_default()
                ),
            )
        })
        .collect();
    for want in [
        ("function", "lib.nz::fn helper"),
        ("record", "lib.nz::record P"),
        ("field", "lib.nz::record P.a"),
        ("enum", "lib.nz::enum E"),
        ("variant", "lib.nz::enum E.B"),
        ("payload field", "lib.nz::enum E.B.v"),
    ] {
        assert!(
            kinds.contains(&(want.0.to_owned(), want.1.to_owned())),
            "{want:?} in {kinds:?}"
        );
    }
    // `p` and `x` are locals: not durable, not here.
    let text = main.to_string();
    assert!(!text.contains("\"local\""));
    // Each range is exactly the name it records.
    for o in main["occurrences"].as_array().unwrap() {
        let (s, e) = (
            o["start"].as_u64().unwrap() as usize,
            o["end"].as_u64().unwrap() as usize,
        );
        let name = &MAIN[s..e];
        let item = o["target"]["item"].as_str().unwrap();
        let member = o["target"]["member"].as_str();
        let expected = member
            .map(|m| m.rsplit('.').next().unwrap().to_owned())
            .unwrap_or_else(|| item.rsplit(' ').next().unwrap().to_owned());
        assert_eq!(name, expected, "{o}");
    }
}

#[test]
fn references_read_a_reused_modules_unit_instead_of_checking_it() {
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let cold = references(d.path(), "lib.nz::fn helper");
    assert_eq!(cold["reused"], 0);
    let warm = references(d.path(), "lib.nz::fn helper");
    assert_eq!(warm["checked"], 0, "{warm}");
    assert_eq!(warm["uses"], cold["uses"]);
    assert_eq!(warm["declaration"], cold["declaration"]);
    assert_eq!(warm["uses"].as_array().unwrap().len(), 2);
}

#[test]
fn a_private_body_change_rechecks_its_module_and_keeps_its_importers_unit() {
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let before = resolve(d.path());
    std::fs::write(d.path().join("lib.nz"), LIB.replace("{ 7 }", "{ 8 }")).unwrap();
    let r = references(d.path(), "lib.nz::fn helper");
    assert_eq!(r["checked"], 1, "lib alone: {r}");
    let after = resolve(d.path());
    assert_eq!(unit(&before, "main.nz"), unit(&after, "main.nz"));
    assert_ne!(
        unit(&before, "lib.nz")["source"],
        unit(&after, "lib.nz")["source"]
    );
}

#[test]
fn an_exported_signature_change_rechecks_every_direct_importer() {
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let before = resolve(d.path());
    // A new exported function moves the interface; `main` is checked against the new one.
    std::fs::write(
        d.path().join("lib.nz"),
        format!("{LIB}pub fn more() -> Int {{ 1 }}\n"),
    )
    .unwrap();
    let r = references(d.path(), "lib.nz::fn helper");
    assert_eq!(r["checked"], 2, "lib and main: {r}");
    let after = resolve(d.path());
    assert_ne!(
        unit(&before, "main.nz")["imports"][0]["interface"],
        unit(&after, "main.nz")["imports"][0]["interface"]
    );
}

#[test]
fn an_alias_is_part_of_the_importers_source_and_its_unit() {
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    resolve(d.path());
    let aliased = MAIN
        .replace("use \"lib.nz\";", "use \"lib.nz\" as l;")
        .replace("helper(", "l::helper(")
        .replace("P(a:", "l::P(a:")
        .replace("E.", "l::E.");
    std::fs::write(d.path().join("main.nz"), &aliased).unwrap();
    let units = resolve(d.path());
    let main = unit(&units, "main.nz");
    assert_eq!(main["imports"][0]["alias"], "l");
    let r = references(d.path(), "lib.nz::fn helper");
    // The same entity, reached through the alias, at the item's own bytes.
    for u in r["uses"].as_array().unwrap() {
        let (s, e) = (
            u["start"].as_u64().unwrap() as usize,
            u["end"].as_u64().unwrap() as usize,
        );
        assert_eq!(&aliased[s..e], "helper");
    }
    assert_eq!(r["uses"].as_array().unwrap().len(), 2);
}

#[test]
fn a_compilation_that_does_not_check_has_no_units() {
    let d = project(&[("main.nz", "fn main() -> Int { missing() }\n")]);
    let (code, out, err) = nazm(d.path(), &["resolve", "main.nz"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(out.is_empty());
    assert!(err.contains("N0"), "{err}");
    let (code, _, err) = nazm(d.path(), &["references", "main.nz::fn main", "main.nz"]);
    assert_eq!(code, Some(1), "{err}");
}

#[test]
fn an_entity_nothing_declares_or_uses_is_refused() {
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (code, out, err) = nazm(d.path(), &["references", "lib.nz::fn nowhere", "main.nz"]);
    assert_eq!(code, Some(1));
    assert!(out.is_empty());
    assert!(err.contains("names nothing"), "{err}");
}

#[test]
fn references_agree_with_the_language_service_over_the_same_saved_compilation() {
    // The independent oracle: the service's index, computed in-process from a fresh check,
    // never from a stored unit.
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    references(d.path(), "lib.nz::fn helper");
    let warm = references(d.path(), "lib.nz::fn helper");
    assert_eq!(warm["checked"], 0);
    let main = d.path().join("main.nz");
    let at = u32::try_from(MAIN.find("helper").unwrap()).unwrap();
    let service = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(nazm_core::__syntax::PARSE_STACK_BYTES)
            .spawn_scoped(scope, || {
                let mut s = nazm_service::LanguageService::new();
                s.open(&main, 1, MAIN.to_owned());
                s.references(&main, 1, at).unwrap()
            })
            .unwrap()
            .join()
            .unwrap()
    });
    let from_service: BTreeSet<(u32, u32)> =
        service.uses.iter().map(|l| (l.start, l.end)).collect();
    let from_units: BTreeSet<(u32, u32)> = warm["uses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u32::try_from(u["start"].as_u64().unwrap()).unwrap(),
                u32::try_from(u["end"].as_u64().unwrap()).unwrap(),
            )
        })
        .collect();
    assert_eq!(from_units, from_service);
    let decl = service.declaration.unwrap();
    assert_eq!(warm["declaration"]["start"], decl.start);
    assert_eq!(warm["declaration"]["end"], decl.end);
}

#[test]
fn a_stored_unit_of_other_text_is_never_read() {
    // Forge the store: replace every entry's unit with one claiming other source text. The
    // next run must not read it, and must say so by checking every module again.
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    references(d.path(), "lib.nz::fn helper");
    let mut forged = 0;
    for e in walk(&d.path().join(".nazm")) {
        if e.extension().is_some_and(|x| x == "json") {
            let text = std::fs::read_to_string(&e).unwrap();
            let mut v: Value = serde_json::from_str(&text).unwrap();
            if v["schema"] == "nazm.check/5" {
                v["resolved"]["source"] = Value::String("0".repeat(64));
                std::fs::write(&e, v.to_string()).unwrap();
                forged += 1;
            }
        }
    }
    assert!(forged >= 2, "{forged}");
    let r = references(d.path(), "lib.nz::fn helper");
    assert_eq!(r["reused"], 0, "{r}");
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn a_member_is_its_own_entity_apart_from_its_item() {
    // A variant's occurrences are not its enum's, and a payload field's are not its variant's.
    let d = project(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let variant = references(d.path(), "lib.nz::enum E.B");
    let payload = references(d.path(), "lib.nz::enum E.B.v");
    let item = references(d.path(), "lib.nz::enum E");
    assert_eq!(variant["kind"], "variant");
    assert_eq!(payload["kind"], "payload field");
    assert_eq!(item["kind"], "enum");
    for (r, name) in [(&variant, "B"), (&payload, "v"), (&item, "E")] {
        for u in r["uses"].as_array().unwrap() {
            let (s, e) = (
                u["start"].as_u64().unwrap() as usize,
                u["end"].as_u64().unwrap() as usize,
            );
            assert_eq!(&MAIN[s..e], name, "{r}");
        }
    }
    assert_eq!(variant["uses"].as_array().unwrap().len(), 2);
}

/// R1-C1: a named function written as a value is a use of it, as a call is — in this module
/// and in an importer — and a reused module's unit holds it as the checked one did.
#[test]
fn a_function_written_as_a_value_is_a_use_of_it_cold_and_warm() {
    let lib = "pub fn triple(x: Int) -> Int ! {} { x * 3 }\n";
    let main = "use \"lib.nz\";\n\
fn double(x: Int) -> Int { x * 2 }\n\
fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int { f(f(x)) }\n\
fn main() -> Int { let d = double; apply_twice(double, 5) + double(1) + d(2) + apply_twice(triple, 1) + triple(0) }\n";
    let d = project(&[("lib.nz", lib), ("main.nz", main)]);
    let every = |text: &str, name: &str| -> Vec<(u64, u64)> {
        text.match_indices(name)
            .map(|(i, _)| (i as u64, (i + name.len()) as u64))
            .collect()
    };
    let uses = |r: &Value| -> Vec<(String, u64, u64)> {
        r["uses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| {
                (
                    u["module"].as_str().unwrap().to_owned(),
                    u["start"].as_u64().unwrap(),
                    u["end"].as_u64().unwrap(),
                )
            })
            .collect()
    };
    let in_main = |name: &str| -> Vec<(String, u64, u64)> {
        // Every occurrence of the name in `main.nz` but its declaration, in order.
        every(main, name)
            .into_iter()
            .filter(|(s, _)| !main[..*s as usize].ends_with("fn "))
            .map(|(s, e)| ("main.nz".to_owned(), s, e))
            .collect()
    };
    for pass in ["cold", "warm"] {
        let r = references(d.path(), "main.nz::fn double");
        assert_eq!(r["kind"], "function", "{pass}: {r}");
        // `let d = double`, `apply_twice(double, 5)` and `double(1)`: three, each once.
        assert_eq!(uses(&r), in_main("double"), "{pass}: {r}");
        assert_eq!(uses(&r).len(), 3, "{pass}: {r}");
        let r = references(d.path(), "lib.nz::fn triple");
        assert_eq!(uses(&r), in_main("triple"), "{pass}: {r}");
        assert_eq!(uses(&r).len(), 2, "{pass}: {r}");
        if pass == "warm" {
            assert_eq!(r["checked"], 0, "{r}");
        }
    }
}
