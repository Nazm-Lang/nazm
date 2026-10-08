//! What the store does with an entry that is damaged, foreign, or someone else's.
//!
//! # The property being established
//!
//! **Nothing on disk is authority.** An entry is a file, a file is data, and the only
//! thing that decides what a program means is its source under this compiler. So every
//! test here takes a valid entry, breaks one thing about it, and requires the store to
//! decline — never to hit, and never to turn the damage into a compilation error.
//!
//! The corresponding positive is one test: an untouched entry is found. Without it the
//! rest would pass on a store that never hits at all.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    // Halving a length to truncate a file is arithmetic on bytes, not a measurement.
    clippy::integer_division
)]

use std::path::Path;

use nazm_cache::artifact::Invalid;
use nazm_cache::{
    CheckArtifact, CheckKey, CheckerIdentity, Dependency, Found, SourceFingerprint, Store,
};
use nazm_iface::{InterfaceHash, PersistentUnitInterface};
use nazm_sema::key::ModuleKey;

fn key(p: &str) -> ModuleKey {
    ModuleKey::relative(p).expect("a module key")
}

/// An interface as the compiler would have published it, built from its own bytes so
/// that these tests depend on the format rather than on a second construction of it.
fn interface(module: &str, exports: &str) -> PersistentUnitInterface {
    let text = format!(
        r#"{{"schema":"nazm.interface/11","module":"{module}","dependencies":[],"records":[],"enums":[],"exports":[{exports}]}}"#
    );
    PersistentUnitInterface::from_json(&text).expect("a valid interface")
}

fn one_export(module: &str, name: &str, ret: &str) -> PersistentUnitInterface {
    interface(
        module,
        &format!(
            r#"{{"def":"{module}::fn {name}","name":"{name}","kind":"fn","generics":0,"params":["Int"],"ret":"{ret}"}}"#
        ),
    )
}

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    key: CheckKey,
    artifact: CheckArtifact,
}

/// A store holding one valid entry for `a.nz`, which depends on `b.nz`.
/// A resolved unit of `module` over `source` (N77), with nothing in it.
fn unit(module: &str, source: SourceFingerprint) -> nazm_iface::ResolvedUnit {
    nazm_iface::ResolvedUnit {
        schema: nazm_iface::resolved::SCHEMA.to_owned(),
        module: module.to_owned(),
        source: source.to_string(),
        imports: Vec::new(),
        definitions: Vec::new(),
        occurrences: Vec::new(),
    }
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::at(dir.path().join("check"));
    let deps = vec![Dependency {
        module: key("b.nz"),
        interface: InterfaceHash::of_bytes(b"b"),
    }];
    let artifact = CheckArtifact::of(
        CheckerIdentity::current(),
        &key("a.nz"),
        SourceFingerprint::of(b"fn main() -> Int { 1 }\n"),
        &deps,
        one_export("a.nz", "twice", "Int"),
    )
    .expect("an artefact")
    .with_resolved(unit(
        "a.nz",
        SourceFingerprint::of(b"fn main() -> Int { 1 }\n"),
    ));
    let k = CheckKey::parse(&artifact.key).expect("its key");
    store.put(&artifact).expect("publish");
    Fixture {
        _dir: dir,
        store,
        key: k,
        artifact,
    }
}

/// Overwrite the stored entry with `text`, leaving it under the same name.
fn tamper(store: &Store, k: &CheckKey, text: &str) {
    let name = k.to_string();
    let path = Path::new(store.directory())
        .join(&name[..2])
        .join(format!("{name}.json"));
    assert!(path.exists(), "the fixture should have written {name}");
    std::fs::write(path, text).expect("overwrite");
}

fn rejected(store: &Store, k: &CheckKey) -> Invalid {
    match store.get(k) {
        Found::Rejected(e) => e,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

/// Q1-M-07: the provenance facts and resolved unit are sealed by the digest beside them, so an
/// edit to either, or to the digest, or an entry without one, is refused.
#[test]
fn an_entry_whose_facts_moved_is_refused() {
    let f = fixture();
    let text = f.artifact.to_json().expect("json");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert!(v["facts_digest"]
        .as_str()
        .is_some_and(|d| d.starts_with("blake3:")));
    v["facts_digest"] = serde_json::Value::String("blake3:00".to_owned());
    tamper(&f.store, &f.key, &v.to_string());
    assert_eq!(rejected(&f.store, &f.key), Invalid::FactsMoved);
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("json");
    v.as_object_mut().expect("object").remove("facts_digest");
    tamper(&f.store, &f.key, &v.to_string());
    assert_eq!(rejected(&f.store, &f.key), Invalid::FactsMoved);
}

#[test]
fn an_untouched_entry_is_found() {
    let f = fixture();
    match f.store.get(&f.key) {
        Found::Hit(r) => {
            assert_eq!(r.module, key("a.nz"));
            assert_eq!(r.interface, one_export("a.nz", "twice", "Int"));
            assert_eq!(
                r.interface_hash,
                InterfaceHash::of(&one_export("a.nz", "twice", "Int")).unwrap()
            );
        }
        other => panic!("expected a hit, got {other:?}"),
    }
}

#[test]
fn a_key_nothing_was_written_under_is_a_miss() {
    let f = fixture();
    let other = CheckKey::of(
        CheckerIdentity::current(),
        &key("z.nz"),
        SourceFingerprint::of(b"other"),
        &[],
    );
    assert_eq!(f.store.get(&other), Found::Miss);
}

#[test]
fn a_truncated_entry_is_not_an_entry() {
    // The shape an interrupted write would have had, if publication were not atomic.
    let f = fixture();
    let whole = f.artifact.to_json().expect("json");
    tamper(&f.store, &f.key, &whole[..whole.len() / 2]);
    assert!(matches!(rejected(&f.store, &f.key), Invalid::Malformed(_)));
}

#[test]
fn an_empty_entry_is_not_an_entry() {
    let f = fixture();
    tamper(&f.store, &f.key, "");
    assert!(matches!(rejected(&f.store, &f.key), Invalid::Malformed(_)));
}

#[test]
fn a_schema_this_build_does_not_implement_is_refused_rather_than_reinterpreted() {
    // The failure a version exists to prevent, and the one that fails silently: the bytes
    // parse, so a compiler that ignored the version would act on a meaning nobody wrote.
    let f = fixture();
    let text = f
        .artifact
        .to_json()
        .expect("json")
        .replace("nazm.check/5", "nazm.check/6");
    tamper(&f.store, &f.key, &text);
    assert_eq!(
        rejected(&f.store, &f.key),
        Invalid::UnknownSchema("nazm.check/6".to_owned())
    );
}

#[test]
fn an_entry_whose_module_was_edited_does_not_hit() {
    // The mutation that would serve one module's result for another. The module key is an
    // input, so editing it breaks the agreement between the entry and its own name.
    let f = fixture();
    // The entry's own `module` field only — `replacen` stops before the one inside the
    // interface it carries, which is the point: the two must agree.
    let text =
        f.artifact
            .to_json()
            .expect("json")
            .replacen(r#""module":"a.nz""#, r#""module":"c.nz""#, 1);
    tamper(&f.store, &f.key, &text);
    assert_eq!(rejected(&f.store, &f.key), Invalid::ForeignInterface);
}

#[test]
fn an_entry_whose_dependency_fingerprint_was_edited_does_not_hit() {
    let f = fixture();
    let moved = InterfaceHash::of_bytes(b"something else").to_string();
    let text = f
        .artifact
        .to_json()
        .expect("json")
        .replace(&InterfaceHash::of_bytes(b"b").to_string(), &moved);
    tamper(&f.store, &f.key, &text);
    assert_eq!(rejected(&f.store, &f.key), Invalid::KeyDisagrees);
}

#[test]
fn an_entry_whose_source_fingerprint_was_edited_does_not_hit() {
    let f = fixture();
    let text = f.artifact.to_json().expect("json").replace(
        &SourceFingerprint::of(b"fn main() -> Int { 1 }\n").to_string(),
        &SourceFingerprint::of(b"different").to_string(),
    );
    tamper(&f.store, &f.key, &text);
    assert_eq!(rejected(&f.store, &f.key), Invalid::KeyDisagrees);
}

#[test]
fn an_entry_whose_interface_was_edited_does_not_hit() {
    // The one that matters most: the interface is what a dependent would be checked
    // against, so a changed one with an unchanged fingerprint is a stale accept waiting
    // to happen.
    let f = fixture();
    let text = f
        .artifact
        .to_json()
        .expect("json")
        .replace(r#""ret":"Int""#, r#""ret":"Str""#);
    tamper(&f.store, &f.key, &text);
    assert_eq!(rejected(&f.store, &f.key), Invalid::InterfaceMoved);
}

#[test]
fn an_entry_carrying_an_invalid_interface_does_not_hit() {
    // Parsing is not trusting, at both layers. `nazm-iface` validates what an interface
    // must be; an entry does not get to smuggle one past it by embedding it.
    let f = fixture();
    let text = f
        .artifact
        .to_json()
        .expect("json")
        .replace(r#""ret":"Int""#, r#""ret":"Float""#);
    tamper(&f.store, &f.key, &text);
    assert!(matches!(
        rejected(&f.store, &f.key),
        Invalid::BadInterface(_)
    ));
}

#[test]
fn an_entry_moved_to_another_name_does_not_hit() {
    // A file-name match is evidence, not proof. The entry carries the key it was computed
    // for, and asking under a different one is a miss even though the file is perfectly
    // valid.
    let f = fixture();
    let elsewhere = CheckKey::of(
        CheckerIdentity::current(),
        &key("a.nz"),
        SourceFingerprint::of(b"a different module's source"),
        &[],
    );
    let name = elsewhere.to_string();
    let path = Path::new(f.store.directory())
        .join(&name[..2])
        .join(format!("{name}.json"));
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(&path, f.artifact.to_json().expect("json")).expect("write");
    assert_eq!(rejected(&f.store, &elsewhere), Invalid::WrongKey);
}

#[test]
fn an_entry_from_another_checker_is_unreachable() {
    // Not distrusted — unreachable. The checker's identity is an input to the key, so an
    // entry written by a compiler with different rules is filed under a name this one
    // never asks for.
    let f = fixture();
    let text = f
        .artifact
        .to_json()
        .expect("json")
        .replace(&CheckerIdentity::current().to_string(), &"0".repeat(64));
    tamper(&f.store, &f.key, &text);
    assert_eq!(rejected(&f.store, &f.key), Invalid::KeyDisagrees);
}

#[test]
fn a_half_written_file_is_never_an_entry() {
    // Publication is a rename, so the only thing an interrupted write can leave behind is
    // a `.part` file — and nothing looks for one.
    let f = fixture();
    let name = f.key.to_string();
    let dir = Path::new(f.store.directory()).join(&name[..2]);
    std::fs::write(dir.join(format!("{name}.1.0.part")), "{").expect("write a partial");
    assert!(matches!(f.store.get(&f.key), Found::Hit(_)));
}

#[test]
fn writing_the_same_entry_twice_is_the_same_entry() {
    // What two compilers racing to publish one result reduces to: entries are addressed
    // by everything they depend on, so both computed the same bytes and whichever rename
    // lands second lands the same file.
    let f = fixture();
    let again = CheckArtifact::of(
        CheckerIdentity::current(),
        &key("a.nz"),
        SourceFingerprint::of(b"fn main() -> Int { 1 }\n"),
        &[Dependency {
            module: key("b.nz"),
            interface: InterfaceHash::of_bytes(b"b"),
        }],
        one_export("a.nz", "twice", "Int"),
    )
    .expect("an artefact")
    .with_resolved(unit(
        "a.nz",
        SourceFingerprint::of(b"fn main() -> Int { 1 }\n"),
    ));
    assert_eq!(again.to_json().unwrap(), f.artifact.to_json().unwrap());
    f.store.put(&again).expect("publish again");
    assert!(matches!(f.store.get(&f.key), Found::Hit(_)));
}

#[test]
fn a_store_that_does_not_exist_is_a_miss_rather_than_a_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::at(dir.path().join("never/created"));
    let k = CheckKey::of(
        CheckerIdentity::current(),
        &key("a.nz"),
        SourceFingerprint::of(b"x"),
        &[],
    );
    assert_eq!(store.get(&k), Found::Miss);
}

#[test]
fn a_project_store_sits_under_its_source_root() {
    let store = Store::for_project(Path::new("/somewhere/project"));
    assert_eq!(
        store.directory(),
        Path::new("/somewhere/project/.nazm/check")
    );
}

#[test]
fn an_entry_without_its_resolved_unit_is_never_an_entry() {
    // N77: a `/3` entry's unit is part of what it means; one without is a miss, never a
    // module whose names resolved to nothing.
    let f = fixture();
    let mut v: serde_json::Value =
        serde_json::from_str(&f.artifact.to_json().expect("json")).expect("parse");
    v["resolved"] = serde_json::Value::Null;
    tamper(&f.store, &f.key, &v.to_string());
    assert_eq!(rejected(&f.store, &f.key), Invalid::ForeignResolution);
}

#[test]
fn a_unit_of_another_module_or_other_text_is_never_read() {
    for (field, value) in [("module", "b.nz"), ("source", "00")] {
        let f = fixture();
        let mut v: serde_json::Value =
            serde_json::from_str(&f.artifact.to_json().expect("json")).expect("parse");
        v["resolved"][field] = serde_json::Value::String(value.to_owned());
        tamper(&f.store, &f.key, &v.to_string());
        assert_eq!(
            rejected(&f.store, &f.key),
            Invalid::ForeignResolution,
            "{field}"
        );
    }
    let f = fixture();
    let mut v: serde_json::Value =
        serde_json::from_str(&f.artifact.to_json().expect("json")).expect("parse");
    v["resolved"]["schema"] = serde_json::Value::String("nazm.resolved/2".to_owned());
    tamper(&f.store, &f.key, &v.to_string());
    assert_eq!(rejected(&f.store, &f.key), Invalid::ForeignResolution);
}
