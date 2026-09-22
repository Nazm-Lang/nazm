//! What a persisted interface must do, and must refuse to do.
//!
//! These are the unit-level half. The half that needs two processes, two checkouts and a
//! dependency whose source is not there is `crates/nazm-cli/tests/durable_identity.rs`,
//! because it needs the binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nazm_iface::wire::{PersistentUnitInterface, ReadError};
use nazm_iface::{InterfaceHash, SCHEMA};
use nazm_sema::interface::{Export, UnitInterface};
use nazm_sema::key::ModuleKey;
use nazm_sema::module::ModuleGraph;
use nazm_sema::record::Records;
use nazm_sema::resolve::FnId;
use nazm_sema::types::Type;
use nazm_span::{FileId, Span};

/// A graph of two modules, `a.nz` importing `lib/b.nz`, with durable keys.
fn graph() -> ModuleGraph {
    let mut g = ModuleGraph::default();
    let a = g.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = g.add(FileId::from_index(1), ModuleKey::relative("lib/b.nz"));
    g.import(a, b, Span::synthetic());
    g
}

/// An in-memory interface for the root module, with these exports.
fn interface(g: &ModuleGraph, exports: &[(&str, &[Type], Type)]) -> UnitInterface {
    let root = g.root();
    UnitInterface::new(
        root,
        exports
            .iter()
            .enumerate()
            .map(|(i, (name, params, ret))| {
                let at = u32::try_from(i).unwrap_or(0) * 10;
                Export {
                    name: (*name).to_owned(),
                    params: params.to_vec(),
                    ret: *ret,
                    // Deliberately a real span, and deliberately different per export: if
                    // any of this reached the published form, these tests would see it.
                    name_span: Span::new(FileId::ROOT, at, at + 3),
                    def: FnId::from_index(i + 17),
                }
            })
            .collect(),
        Vec::new(),
        g.imports(root).to_vec(),
    )
}

fn publish(g: &ModuleGraph, exports: &[(&str, &[Type], Type)]) -> PersistentUnitInterface {
    PersistentUnitInterface::published(&interface(g, exports), g, &Records::default())
        .expect("publishable")
}

#[test]
fn the_published_form_carries_durable_keys_and_nothing_session_local() {
    let g = graph();
    let p = publish(&g, &[("parse", &[Type::Str], Type::Int)]);
    let json = p.to_json().unwrap();

    assert_eq!(
        json,
        r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":["lib/b.nz"],"records":[],"exports":[{"def":"a.nz::fn parse","name":"parse","kind":"fn","params":["Str"],"ret":"Int"}]}"#
    );
    // The ways a session-local value could have leaked, asserted separately so a failure
    // says which. `17` is the FnId; the spans above would show as small integers.
    assert!(!json.contains("17"), "{json}");
    assert!(!json.contains("span"), "{json}");
    assert!(!json.contains("file"), "{json}");
}

#[test]
fn a_module_without_a_durable_key_is_refused_rather_than_guessed() {
    let mut g = ModuleGraph::default();
    g.add(FileId::ROOT, None);
    let i = UnitInterface::new(g.root(), Vec::new(), Vec::new(), Vec::new());
    assert!(PersistentUnitInterface::published(&i, &g, &Records::default()).is_err());
}

#[test]
fn a_dependency_without_a_durable_key_is_refused_too() {
    // Naming a dependency by its session id would be exactly the leak this format exists
    // to prevent, so an interface that cannot name one is not written at all.
    let mut g = ModuleGraph::default();
    let a = g.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = g.add(FileId::from_index(1), None);
    g.import(a, b, Span::synthetic());
    let i = UnitInterface::new(a, Vec::new(), Vec::new(), g.imports(a).to_vec());
    assert!(PersistentUnitInterface::published(&i, &g, &Records::default()).is_err());
}

#[test]
fn serialising_the_same_interface_twice_gives_the_same_bytes() {
    let g = graph();
    let one = publish(&g, &[("b", &[], Type::Int), ("a", &[Type::Int], Type::Str)]);
    let two = publish(&g, &[("a", &[Type::Int], Type::Str), ("b", &[], Type::Int)]);
    // Declaration order differs; what the module offers does not.
    assert_eq!(one.to_json().unwrap(), two.to_json().unwrap());
    assert_eq!(
        InterfaceHash::of(&one).unwrap(),
        InterfaceHash::of(&two).unwrap()
    );
}

#[test]
fn a_private_definition_is_absent_from_the_published_form() {
    // `UnitInterface` only ever holds exports — privacy is enforced where the interface is
    // built, not where it is written — so what this pins is that publishing adds nothing:
    // no "all definitions" list, no count, no hint that a private one exists.
    let g = graph();
    let json = publish(&g, &[("shown", &[], Type::Int)]).to_json().unwrap();
    assert!(json.contains("shown"));
    assert!(!json.contains("hidden"));
}

#[test]
fn an_interface_round_trips_through_its_own_bytes() {
    let g = graph();
    let p = publish(&g, &[("parse", &[Type::Str, Type::Int], Type::Bool)]);
    let json = p.to_json().unwrap();
    let back = PersistentUnitInterface::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.to_json().unwrap(), json);
}

// --- the fingerprint's job -------------------------------------------------------------

#[test]
fn an_exported_signature_change_changes_the_fingerprint() {
    let g = graph();
    let before = InterfaceHash::of(&publish(&g, &[("f", &[Type::Int], Type::Int)])).unwrap();
    let after = InterfaceHash::of(&publish(&g, &[("f", &[Type::Str], Type::Int)])).unwrap();
    assert_ne!(before, after);
}

#[test]
fn adding_and_removing_an_export_changes_the_fingerprint() {
    let g = graph();
    let one = InterfaceHash::of(&publish(&g, &[("f", &[], Type::Int)])).unwrap();
    let two = InterfaceHash::of(&publish(
        &g,
        &[("f", &[], Type::Int), ("g", &[], Type::Int)],
    ))
    .unwrap();
    assert_ne!(one, two);
}

#[test]
fn renaming_an_export_changes_both_the_key_and_the_fingerprint() {
    let g = graph();
    let before = publish(&g, &[("calculate", &[], Type::Int)]);
    let after = publish(&g, &[("compute", &[], Type::Int)]);
    assert_ne!(
        before.exports.first().map(|e| &e.def),
        after.exports.first().map(|e| &e.def)
    );
    assert_ne!(
        InterfaceHash::of(&before).unwrap(),
        InterfaceHash::of(&after).unwrap()
    );
}

#[test]
fn the_schema_is_an_input_to_the_fingerprint() {
    // So that the same exported signatures under a future schema fingerprint differently
    // rather than colliding with what this one wrote.
    let g = graph();
    let mut p = publish(&g, &[("f", &[], Type::Int)]);
    let before = InterfaceHash::of(&p).unwrap();
    p.schema = "nazm.interface/3".to_owned();
    assert_ne!(before, InterfaceHash::of(&p).unwrap());
}

#[test]
fn a_fingerprint_round_trips_through_its_text() {
    let g = graph();
    let h = InterfaceHash::of(&publish(&g, &[("f", &[], Type::Int)])).unwrap();
    assert_eq!(InterfaceHash::parse(&h.to_string()), Some(h));
    assert_eq!(h.to_string().len(), 64);
    assert_eq!(InterfaceHash::ALGORITHM, "blake3-256");
    // Anything that is not sixty-four lowercase hexadecimal digits is refused rather than
    // repaired: a fingerprint is compared, and a repaired one compares equal to nothing.
    assert!(InterfaceHash::parse("").is_none());
    assert!(InterfaceHash::parse(&h.to_string().to_uppercase()).is_none());
    let text = h.to_string();
    assert!(InterfaceHash::parse(text.get(..63).unwrap()).is_none());
    assert!(InterfaceHash::parse(&format!("{}z", text.get(..63).unwrap())).is_none());
}

// --- reading is not trusting ------------------------------------------------------------

fn refuse(json: &str) -> ReadError {
    PersistentUnitInterface::from_json(json).expect_err("should have been refused")
}

#[test]
fn a_future_schema_is_refused_and_never_read_as_this_one() {
    // `/3` is a version this build does not have. `/1` would do as well and says
    // something different — the rule is that *unknown* means refused, in either
    // direction, and a newer file is the case that matters because it is the one whose
    // extra meaning this reader would otherwise drop.
    let e = refuse(
        r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"exports":[]}"#,
    );
    assert_eq!(e, ReadError::UnknownSchema("nazm.interface/3".to_owned()));
    // The message names both, because a reader holding a newer file needs to know it is
    // newer rather than broken.
    assert!(e.to_string().contains("nazm.interface/3"));
    assert!(e.to_string().contains(SCHEMA));

    // And the version this build replaced: a `/1` file describes a language with no
    // records, so reading it as a `/2` would be believing a module offers less than it
    // does — the failure the major number exists to prevent.
    let old =
        refuse(r#"{"schema":"nazm.interface/1","module":"a.nz","dependencies":[],"exports":[]}"#);
    assert_eq!(old, ReadError::UnknownSchema("nazm.interface/1".to_owned()));
}

#[test]
fn malformed_and_truncated_bytes_are_refused() {
    assert!(matches!(refuse("not json at all"), ReadError::Malformed(_)));
    assert!(matches!(refuse(""), ReadError::Malformed(_)));
    let g = graph();
    let json = publish(&g, &[("f", &[], Type::Int)]).to_json().unwrap();
    let half = json.len().checked_div(2).unwrap_or(0);
    assert!(matches!(
        refuse(json.get(..half).unwrap()),
        ReadError::Malformed(_)
    ));
    // Right schema, missing the rest.
    assert!(matches!(
        refuse(r#"{"schema":"nazm.interface/2"}"#),
        ReadError::Malformed(_)
    ));
}

#[test]
fn a_key_that_is_not_canonical_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"./a.nz","dependencies":[],"records":[],"exports":[]}"#
        ),
        ReadError::BadKey(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":["../b.nz"],"records":[],"exports":[]}"#
        ),
        ReadError::BadKey(_)
    ));
}

#[test]
fn a_duplicate_export_is_refused() {
    let e = refuse(
        r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":[],"records":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Int"},{"def":"a.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Str"}]}"#,
    );
    assert_eq!(e, ReadError::DuplicateExport("f".to_owned()));
}

#[test]
fn an_unknown_type_or_kind_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":[],"records":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","params":["Widget"],"ret":"Int"}]}"#
        ),
        ReadError::UnknownType(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":[],"records":[],"exports":[{"def":"a.nz::record f","name":"f","kind":"record","params":[],"ret":"Int"}]}"#
        ),
        ReadError::UnknownKind(_)
    ));
}

#[test]
fn an_export_belonging_to_another_module_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":[],"records":[],"exports":[{"def":"b.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Int"}]}"#
        ),
        ReadError::ForeignExport(_)
    ));
}

#[test]
fn an_identity_disagreeing_with_the_name_beside_it_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":[],"records":[],"exports":[{"def":"a.nz::fn g","name":"f","kind":"fn","params":[],"ret":"Int"}]}"#
        ),
        ReadError::NameMismatch(_)
    ));
}

#[test]
fn a_file_not_in_canonical_order_is_refused() {
    // Two spellings of one interface would be two fingerprints for one meaning.
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":[],"records":[],"exports":[{"def":"a.nz::fn g","name":"g","kind":"fn","params":[],"ret":"Int"},{"def":"a.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Int"}]}"#
        ),
        ReadError::NotCanonical(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":["c.nz","b.nz"],"records":[],"exports":[]}"#
        ),
        ReadError::NotCanonical(_)
    ));
}
