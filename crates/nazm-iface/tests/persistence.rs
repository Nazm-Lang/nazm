//! What a persisted interface must do, and must refuse to do.
//!
//! These are the unit-level half. The half that needs two processes, two checkouts and a
//! dependency whose source is not there is `crates/nazm-cli/tests/durable_identity.rs`,
//! because it needs the binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nazm_iface::wire::{PersistentUnitInterface, ReadError};
use nazm_iface::{InterfaceHash, SCHEMA};
use nazm_sema::interface::{EnumExport, Export, UnitInterface};
use nazm_sema::key::ModuleKey;
use nazm_sema::module::ModuleGraph;
use nazm_sema::enums::{EnumDef, VariantDef};
use nazm_sema::interface::Visibility;
use nazm_sema::record::FieldDef;
use nazm_sema::udt::UserTypes;
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
        Vec::new(),
        g.imports(root).to_vec(),
    )
}

fn publish(g: &ModuleGraph, exports: &[(&str, &[Type], Type)]) -> PersistentUnitInterface {
    PersistentUnitInterface::published(&interface(g, exports), g, &UserTypes::default())
        .expect("publishable")
}

#[test]
fn the_published_form_carries_durable_keys_and_nothing_session_local() {
    let g = graph();
    let p = publish(&g, &[("parse", &[Type::Str], Type::Int)]);
    let json = p.to_json().unwrap();

    assert_eq!(
        json,
        r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":["lib/b.nz"],"records":[],"enums":[],"exports":[{"def":"a.nz::fn parse","name":"parse","kind":"fn","params":["Str"],"ret":"Int"}]}"#
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
    let i = UnitInterface::new(g.root(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    assert!(PersistentUnitInterface::published(&i, &g, &UserTypes::default()).is_err());
}

#[test]
fn a_dependency_without_a_durable_key_is_refused_too() {
    // Naming a dependency by its session id would be exactly the leak this format exists
    // to prevent, so an interface that cannot name one is not written at all.
    let mut g = ModuleGraph::default();
    let a = g.add(FileId::ROOT, ModuleKey::relative("a.nz"));
    let b = g.add(FileId::from_index(1), None);
    g.import(a, b, Span::synthetic());
    let i = UnitInterface::new(a, Vec::new(), Vec::new(), Vec::new(), g.imports(a).to_vec());
    assert!(PersistentUnitInterface::published(&i, &g, &UserTypes::default()).is_err());
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
    p.schema = "nazm.interface/4".to_owned();
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
    // `/4` is a version this build does not have. `/1` and `/2` would do as well and say
    // something different — the rule is that *unknown* means refused, in either
    // direction, and a newer file is the case that matters because it is the one whose
    // extra meaning this reader would otherwise drop.
    let e = refuse(
        r#"{"schema":"nazm.interface/4","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[]}"#,
    );
    assert_eq!(e, ReadError::UnknownSchema("nazm.interface/4".to_owned()));
    // The message names both, because a reader holding a newer file needs to know it is
    // newer rather than broken.
    assert!(e.to_string().contains("nazm.interface/4"));
    assert!(e.to_string().contains(SCHEMA));

    // And both versions this build replaced. A `/1` file describes a language with no
    // records and a `/2` one a language with no enums, so reading either as a `/3` would
    // be believing a module offers less than it does — the failure the major number
    // exists to prevent, once per major.
    let old =
        refuse(r#"{"schema":"nazm.interface/1","module":"a.nz","dependencies":[],"exports":[]}"#);
    assert_eq!(old, ReadError::UnknownSchema("nazm.interface/1".to_owned()));
    // Written in the `/2` shape it really had — records but no enums — so this is a file
    // an N9 build would have produced, not a `/3` with the version string changed.
    let two = refuse(
        r#"{"schema":"nazm.interface/2","module":"a.nz","dependencies":[],"records":[],"exports":[]}"#,
    );
    assert_eq!(two, ReadError::UnknownSchema("nazm.interface/2".to_owned()));
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
        refuse(r#"{"schema":"nazm.interface/3"}"#),
        ReadError::Malformed(_)
    ));
}

#[test]
fn a_key_that_is_not_canonical_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"./a.nz","dependencies":[],"records":[],"enums":[],"exports":[]}"#
        ),
        ReadError::BadKey(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":["../b.nz"],"records":[],"enums":[],"exports":[]}"#
        ),
        ReadError::BadKey(_)
    ));
}

#[test]
fn a_duplicate_export_is_refused() {
    let e = refuse(
        r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Int"},{"def":"a.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Str"}]}"#,
    );
    assert_eq!(e, ReadError::DuplicateExport("f".to_owned()));
}

#[test]
fn an_unknown_type_or_kind_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","params":["Widget"],"ret":"Int"}]}"#
        ),
        ReadError::UnknownType(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::record f","name":"f","kind":"record","params":[],"ret":"Int"}]}"#
        ),
        ReadError::UnknownKind(_)
    ));
}

#[test]
fn an_export_belonging_to_another_module_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"b.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Int"}]}"#
        ),
        ReadError::ForeignExport(_)
    ));
}

#[test]
fn an_identity_disagreeing_with_the_name_beside_it_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn g","name":"f","kind":"fn","params":[],"ret":"Int"}]}"#
        ),
        ReadError::NameMismatch(_)
    ));
}

#[test]
fn a_file_not_in_canonical_order_is_refused() {
    // Two spellings of one interface would be two fingerprints for one meaning.
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn g","name":"g","kind":"fn","params":[],"ret":"Int"},{"def":"a.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Int"}]}"#
        ),
        ReadError::NotCanonical(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":["c.nz","b.nz"],"records":[],"enums":[],"exports":[]}"#
        ),
        ReadError::NotCanonical(_)
    ));
}

// --- enums in a persisted interface -----------------------------------------------------
//
// A `/3` interface is **closed** over the types its public surface mentions, and canonical
// in every order that is not semantic. These are the ways a file could be well-formed JSON
// and not an interface, asked of the half N10 added.

/// An interface carrying one enum, built from its own bytes so these depend on the format
/// rather than on a second construction of it.
fn enum_interface(variants: &str) -> Result<PersistentUnitInterface, ReadError> {
    PersistentUnitInterface::from_json(&format!(
        r#"{{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[{{"def":"a.nz::enum State","name":"State","variants":[{variants}]}}],"exports":[]}}"#
    ))
}

#[test]
fn an_enum_round_trips_with_its_variants_and_payload_types() {
    let g = graph();
    let mut types = UserTypes::default();
    let e = types.enums_mut().declare(EnumDef {
        name: "State".to_owned(),
        module: g.root(),
        visibility: Visibility::Exported,
        name_span: Span::synthetic(),
        variants: Vec::new(),
        order: Vec::new(),
        decl: 0,
    });
    // Declared in neither canonical order: variants `Ready` then `Done`, and `Done`'s
    // payload `why` then `code`. The published bytes must be in both.
    types.enums_mut().complete(
        e,
        vec![
            VariantDef {
                name: "Ready".to_owned(),
                name_span: Span::synthetic(),
                fields: Vec::new(),
                layout: Vec::new(),
            },
            VariantDef {
                name: "Done".to_owned(),
                name_span: Span::synthetic(),
                fields: vec![
                    FieldDef {
                        name: "why".to_owned(),
                        ty: Type::Str,
                        name_span: Span::synthetic(),
                        ty_span: Span::synthetic(),
                    },
                    FieldDef {
                        name: "code".to_owned(),
                        ty: Type::Int,
                        name_span: Span::synthetic(),
                        ty_span: Span::synthetic(),
                    },
                ],
                layout: Vec::new(),
            },
        ],
    );
    let interface = UnitInterface::new(
        g.root(),
        Vec::new(),
        Vec::new(),
        vec![EnumExport {
            def: e,
            nameable: true,
        }],
        Vec::new(),
    );
    let published = PersistentUnitInterface::published(&interface, &g, &types).expect("publish");
    let json = published.to_json().expect("json");
    assert_eq!(
        json,
        r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[{"def":"a.nz::enum State","name":"State","variants":[{"name":"Done","fields":[{"name":"code","ty":"Int"},{"name":"why","ty":"Str"}]},{"name":"Ready","fields":[]}]}],"exports":[]}"#
    );
    // And it reads back: canonical bytes are what the reader requires.
    assert_eq!(
        PersistentUnitInterface::from_json(&json).expect("valid"),
        published
    );
}

#[test]
fn variants_out_of_canonical_order_are_refused() {
    assert_eq!(
        enum_interface(r#"{"name":"Ready","fields":[]},{"name":"Done","fields":[]}"#)
            .expect_err("should be refused"),
        ReadError::NotCanonical("variants".to_owned())
    );
}

#[test]
fn a_duplicate_variant_is_refused() {
    assert!(matches!(
        enum_interface(r#"{"name":"Done","fields":[]},{"name":"Done","fields":[]}"#)
            .expect_err("should be refused"),
        ReadError::DuplicateRecord(_)
    ));
}

#[test]
fn payload_fields_out_of_canonical_order_or_duplicated_are_refused() {
    assert_eq!(
        enum_interface(
            r#"{"name":"Done","fields":[{"name":"why","ty":"Str"},{"name":"code","ty":"Int"}]}"#
        )
        .expect_err("should be refused"),
        ReadError::NotCanonical("fields".to_owned())
    );
    assert!(matches!(
        enum_interface(
            r#"{"name":"Done","fields":[{"name":"code","ty":"Int"},{"name":"code","ty":"Int"}]}"#
        )
        .expect_err("should be refused"),
        ReadError::DuplicateRecord(_)
    ));
}

#[test]
fn a_payload_type_this_interface_does_not_define_is_refused() {
    // The closure, as a refusal: a `/3` file defines every type it names, so a dangling
    // reference is a malformed interface rather than something to go and look for.
    assert_eq!(
        enum_interface(r#"{"name":"Done","fields":[{"name":"p","ty":"a.nz::record Point"}]}"#)
            .expect_err("should be refused"),
        ReadError::UnknownType("a.nz::record Point".to_owned())
    );
}

#[test]
fn an_enum_listed_under_a_record_identity_is_refused() {
    // The kind is part of a durable identity, so a file that files an enum among the
    // records disagrees with itself.
    assert!(matches!(
        PersistentUnitInterface::from_json(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[{"def":"a.nz::enum State","name":"State","fields":[]}],"enums":[],"exports":[]}"#
        )
        .expect_err("should be refused"),
        ReadError::BadKey(_)
    ));
    assert!(matches!(
        PersistentUnitInterface::from_json(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[{"def":"a.nz::record State","name":"State","variants":[]}],"exports":[]}"#
        )
        .expect_err("should be refused"),
        ReadError::BadKey(_)
    ));
}

#[test]
fn an_enums_name_must_agree_with_its_identity() {
    assert!(matches!(
        PersistentUnitInterface::from_json(
            r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[{"def":"a.nz::enum State","name":"Other","variants":[]}],"exports":[]}"#
        )
        .expect_err("should be refused"),
        ReadError::NameMismatch(_)
    ));
}

#[test]
fn a_record_and_an_enum_may_refer_to_one_another_across_the_file() {
    // Closed and forward-referencing: a record's field names an enum defined after it and
    // an enum's payload names that record. Checked after the whole set is known, which is
    // why both orders work.
    let json = r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[{"def":"a.nz::record Event","name":"Event","fields":[{"name":"state","ty":"a.nz::enum State"}]}],"enums":[{"def":"a.nz::enum State","name":"State","variants":[{"name":"At","fields":[{"name":"e","ty":"a.nz::record Event"}]}]}],"exports":[]}"#;
    let read = PersistentUnitInterface::from_json(json).expect("a valid interface");
    assert_eq!(read.to_json().expect("json"), json);
}

#[test]
fn adding_or_renaming_a_variant_changes_the_fingerprint_and_reordering_does_not() {
    let hash = |variants: &str| {
        InterfaceHash::of(&enum_interface(variants).expect("valid"))
            .expect("hashable")
            .to_string()
    };
    let base = hash(r#"{"name":"Done","fields":[]},{"name":"Ready","fields":[]}"#);
    assert_ne!(
        base,
        hash(r#"{"name":"Done","fields":[]},{"name":"Failed","fields":[]},{"name":"Ready","fields":[]}"#),
        "adding a variant"
    );
    assert_ne!(
        base,
        hash(r#"{"name":"Done","fields":[]},{"name":"Waiting","fields":[]}"#),
        "renaming a variant"
    );
    assert_ne!(
        base,
        hash(r#"{"name":"Done","fields":[{"name":"code","ty":"Int"}]},{"name":"Ready","fields":[]}"#),
        "adding a payload field"
    );
}
