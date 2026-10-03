//! What a persisted interface must do, and must refuse to do.
//!
//! These are the unit-level half. The half that needs two processes, two checkouts and a
//! dependency whose source is not there is `crates/nazm-cli/tests/durable_identity.rs`,
//! because it needs the binary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nazm_iface::wire::{PersistentUnitInterface, ReadError};
use nazm_iface::{InterfaceHash, SCHEMA};
use nazm_sema::enums::{EnumDef, VariantDef};
use nazm_sema::interface::Visibility;
use nazm_sema::interface::{EnumExport, Export, UnitInterface};
use nazm_sema::key::ModuleKey;
use nazm_sema::module::ModuleGraph;
use nazm_sema::record::FieldDef;
use nazm_sema::resolve::FnId;
use nazm_sema::types::Type;
use nazm_sema::udt::UserTypes;
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
                    bounds: Vec::new(),
                    name: (*name).to_owned(),
                    params: params.to_vec(),
                    ret: *ret,
                    // Deliberately a real span, and deliberately different per export: if
                    // any of this reached the published form, these tests would see it.
                    name_span: Span::new(FileId::ROOT, at, at + 3),
                    def: FnId::from_index(i + 17),
                    type_params: Vec::new(),
                    effects: None,
                    foreign: None,
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
        r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":["lib/b.nz"],"records":[],"enums":[],"exports":[{"def":"a.nz::fn parse","name":"parse","kind":"fn","generics":0,"params":["Str"],"ret":"Int"}]}"#
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
    p.schema = "nazm.interface/9".to_owned();
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
    // `/8` is a version this build does not have. `/1` to `/6` would do as well and say
    // something different — the rule is that *unknown* means refused, in either
    // direction, and a newer file is the case that matters because it is the one whose
    // extra meaning this reader would otherwise drop.
    let e = refuse(
        r#"{"schema":"nazm.interface/9","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[]}"#,
    );
    assert_eq!(e, ReadError::UnknownSchema("nazm.interface/9".to_owned()));
    // The message names both, because a reader holding a newer file needs to know it is
    // newer rather than broken.
    assert!(e.to_string().contains("nazm.interface/9"));
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
    // And the `/3` an N10 build wrote: its types are strings, where `/4`'s are trees with
    // parameter positions, so reading one as the other would misread every type.
    let three = refuse(
        r#"{"schema":"nazm.interface/3","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","params":[],"ret":"Int"}]}"#,
    );
    assert_eq!(
        three,
        ReadError::UnknownSchema("nazm.interface/3".to_owned())
    );
    // And the `/4` an N13 build wrote: it has no requirements, so a bounded generic read
    // from one would accept every argument its requirement refuses.
    let four = refuse(
        r#"{"schema":"nazm.interface/4","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":1,"params":[{"param":0}],"ret":"Int"}]}"#,
    );
    assert_eq!(
        four,
        ReadError::UnknownSchema("nazm.interface/4".to_owned())
    );
    // And the `/5` an N35 build wrote: its silence about effects meant "not modelled",
    // where `/6` silence means "declares none" — the same bytes, two contracts.
    let five = refuse(
        r#"{"schema":"nazm.interface/5","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[],"ret":"Int"}]}"#,
    );
    assert_eq!(
        five,
        ReadError::UnknownSchema("nazm.interface/5".to_owned())
    );
    // And the `/6` an N41 build wrote: it has no foreign exports, so a `/7` reader taking it
    // would take a C function for a Nazm one with no body (N42).
    let six = refuse(
        r#"{"schema":"nazm.interface/6","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[],"ret":"Int"}]}"#,
    );
    assert_eq!(six, ReadError::UnknownSchema("nazm.interface/6".to_owned()));
}

#[test]
fn a_foreign_export_round_trips_with_its_symbol() {
    let text = r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":["Int"],"ret":"Bool","effects":["foreign"],"foreign":"c_f"}]}"#;
    let read = PersistentUnitInterface::from_json(text).expect("reads");
    assert_eq!(
        read.exports.first().and_then(|e| e.foreign.as_deref()),
        Some("c_f")
    );
    assert_eq!(read.to_json().unwrap(), text);
}

#[test]
fn a_requirement_is_written_by_position_and_read_back_canonically() {
    // N14. Positions, ascending, and absent when there are none.
    let ok = r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":2,"requires_equality":[0,1],"params":[{"param":0},{"param":1}],"ret":"Int"}]}"#;
    let read = PersistentUnitInterface::from_json(ok).expect("valid");
    assert_eq!(read.to_json().unwrap(), ok);
    for bad in ["[1,0]", "[0,0]", "[2]"] {
        let json = ok.replace("[0,1]", bad);
        assert!(
            matches!(refuse(&json), ReadError::NotCanonical(_)),
            "{bad} should be refused"
        );
    }
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
        refuse(r#"{"schema":"nazm.interface/8"}"#),
        ReadError::Malformed(_)
    ));
}

#[test]
fn a_key_that_is_not_canonical_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"./a.nz","dependencies":[],"records":[],"enums":[],"exports":[]}"#
        ),
        ReadError::BadKey(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":["../b.nz"],"records":[],"enums":[],"exports":[]}"#
        ),
        ReadError::BadKey(_)
    ));
}

#[test]
fn a_duplicate_export_is_refused() {
    let e = refuse(
        r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[],"ret":"Int"},{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[],"ret":"Str"}]}"#,
    );
    assert_eq!(e, ReadError::DuplicateExport("f".to_owned()));
}

#[test]
fn an_unknown_type_or_kind_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":["Widget"],"ret":"Int"}]}"#
        ),
        ReadError::UnknownType(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::record f","name":"f","kind":"record","generics":0,"params":[],"ret":"Int"}]}"#
        ),
        ReadError::UnknownKind(_)
    ));
}

#[test]
fn an_export_belonging_to_another_module_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"b.nz::fn f","name":"f","kind":"fn","generics":0,"params":[],"ret":"Int"}]}"#
        ),
        ReadError::ForeignExport(_)
    ));
}

#[test]
fn an_identity_disagreeing_with_the_name_beside_it_is_refused() {
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn g","name":"f","kind":"fn","generics":0,"params":[],"ret":"Int"}]}"#
        ),
        ReadError::NameMismatch(_)
    ));
}

#[test]
fn a_file_not_in_canonical_order_is_refused() {
    // Two spellings of one interface would be two fingerprints for one meaning.
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn g","name":"g","kind":"fn","generics":0,"params":[],"ret":"Int"},{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[],"ret":"Int"}]}"#
        ),
        ReadError::NotCanonical(_)
    ));
    assert!(matches!(
        refuse(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":["c.nz","b.nz"],"records":[],"enums":[],"exports":[]}"#
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
        r#"{{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[{{"def":"a.nz::enum State","name":"State","generics":0,"variants":[{variants}]}}],"exports":[]}}"#
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
        params: Vec::new(),
        instance: None,
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
        r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[{"def":"a.nz::enum State","name":"State","generics":0,"variants":[{"name":"Done","fields":[{"name":"code","ty":"Int"},{"name":"why","ty":"Str"}]},{"name":"Ready","fields":[]}]}],"exports":[]}"#
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
        enum_interface(
            r#"{"name":"Done","fields":[{"name":"p","ty":{"def":"a.nz::record Point","args":[]}}]}"#
        )
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
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[{"def":"a.nz::enum State","name":"State","generics":0,"fields":[]}],"enums":[],"exports":[]}"#
        )
        .expect_err("should be refused"),
        ReadError::BadKey(_)
    ));
    assert!(matches!(
        PersistentUnitInterface::from_json(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[{"def":"a.nz::record State","name":"State","generics":0,"variants":[]}],"exports":[]}"#
        )
        .expect_err("should be refused"),
        ReadError::BadKey(_)
    ));
}

#[test]
fn an_enums_name_must_agree_with_its_identity() {
    assert!(matches!(
        PersistentUnitInterface::from_json(
            r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[{"def":"a.nz::enum State","name":"Other","generics":0,"variants":[]}],"exports":[]}"#
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
    let json = r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[{"def":"a.nz::record Event","name":"Event","generics":0,"fields":[{"name":"state","ty":{"def":"a.nz::enum State","args":[]}}]}],"enums":[{"def":"a.nz::enum State","name":"State","generics":0,"variants":[{"name":"At","fields":[{"name":"e","ty":{"def":"a.nz::record Event","args":[]}}]}]}],"exports":[]}"#;
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
        hash(
            r#"{"name":"Done","fields":[]},{"name":"Failed","fields":[]},{"name":"Ready","fields":[]}"#
        ),
        "adding a variant"
    );
    assert_ne!(
        base,
        hash(r#"{"name":"Done","fields":[]},{"name":"Waiting","fields":[]}"#),
        "renaming a variant"
    );
    assert_ne!(
        base,
        hash(
            r#"{"name":"Done","fields":[{"name":"code","ty":"Int"}]},{"name":"Ready","fields":[]}"#
        ),
        "adding a payload field"
    );
}

// --- N11: what a `/4` interface may not say --------------------------------------------------
//
// Each of these deserialises — it is JSON of the right shape — and describes no interface
// a build could have written. A cache entry built on one is refused, never a false hit.

const GENERIC: &str = r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[{"def":"a.nz::record Box","name":"Box","generics":1,"fields":[{"name":"value","ty":{"param":0}}]}],"enums":[],"exports":[{"def":"a.nz::fn wrap","name":"wrap","kind":"fn","generics":1,"params":[{"param":0}],"ret":{"def":"a.nz::record Box","args":[{"param":0}]}}]}"#;

#[test]
fn a_well_formed_generic_interface_is_read() {
    let parsed = PersistentUnitInterface::from_json(GENERIC).expect("a valid interface");
    assert_eq!(parsed.to_json().unwrap(), GENERIC);
}

#[test]
fn a_parameter_its_owner_does_not_declare_is_refused() {
    // A field naming the second parameter of a one-parameter record.
    let bad = GENERIC.replace(r#""ty":{"param":0}"#, r#""ty":{"param":1}"#);
    assert!(matches!(refuse(&bad), ReadError::BadParameter(_)), "{bad}");
    // A parameter in a function that declares none: a parameter outside its owner.
    let outside = GENERIC.replace(r#""kind":"fn","generics":1"#, r#""kind":"fn","generics":0"#);
    assert!(
        matches!(refuse(&outside), ReadError::BadParameter(_)),
        "{outside}"
    );
}

#[test]
fn a_definition_applied_to_the_wrong_number_of_arguments_is_refused() {
    let too_many = GENERIC.replace(r#""args":[{"param":0}]"#, r#""args":[{"param":0},"Int"]"#);
    assert!(
        matches!(refuse(&too_many), ReadError::WrongArity(_)),
        "{too_many}"
    );
    // And a definition that takes none, applied.
    let applied = GENERIC
        .replace(r#""generics":1,"fields""#, r#""generics":0,"fields""#)
        .replace(r#""ty":{"param":0}"#, r#""ty":"Int""#);
    assert!(
        matches!(refuse(&applied), ReadError::WrongArity(_)),
        "{applied}"
    );
}

#[test]
fn a_malformed_type_tree_is_refused() {
    let nested = GENERIC.replace(r#""params":[{"param":0}]"#, r#""params":[{"vec":[]}]"#);
    assert!(
        matches!(refuse(&nested), ReadError::Malformed(_)),
        "{nested}"
    );
    let unknown = GENERIC.replace(
        r#""params":[{"param":0}]"#,
        r#""params":[{"def":"a.nz::record Gone","args":[]}]"#,
    );
    assert!(
        matches!(refuse(&unknown), ReadError::UnknownType(_)),
        "{unknown}"
    );
    let not_builtin = GENERIC.replace(r#""params":[{"param":0}]"#, r#""params":["Vec"]"#);
    assert!(
        matches!(refuse(&not_builtin), ReadError::UnknownType(_)),
        "{not_builtin}"
    );
}

#[test]
fn a_type_nested_past_any_checked_program_is_refused() {
    let mut deep = String::from(r#"{"param":0}"#);
    for _ in 0..70 {
        deep = format!(r#"{{"vec":{deep}}}"#);
    }
    let bad = GENERIC.replace(
        r#""params":[{"param":0}]"#,
        &format!(r#""params":[{deep}]"#),
    );
    assert!(
        matches!(refuse(&bad), ReadError::Malformed(_)),
        "{:?}",
        refuse(&bad)
    );
}

#[test]
fn bytes_that_are_not_the_canonical_encoding_are_refused() {
    // The same interface with a field this build does not write: every structural check
    // passes, and it is still a second file for one meaning.
    let extra = GENERIC.replace(r#"{"param":0}]"#, r#"{"param":0,"name":"T"}]"#);
    assert!(
        matches!(refuse(&extra), ReadError::NotCanonical(_)),
        "{extra}"
    );
    // And spaces where there are none.
    let spaced = GENERIC.replace(r#","module":"#, r#", "module":"#);
    assert!(
        matches!(refuse(&spaced), ReadError::NotCanonical(_)),
        "{spaced}"
    );
}

// --- N36: effect contracts ----------------------------------------------------------------

/// The root module's interface with one export `f` whose declared set is `effects`.
fn with_effects(g: &ModuleGraph, effects: Option<nazm_sema::EffectSet>) -> PersistentUnitInterface {
    let mut i = interface(g, &[("f", &[], Type::Int)]);
    let exports: Vec<Export> = i
        .exports()
        .iter()
        .cloned()
        .map(|mut e| {
            e.effects = effects;
            e
        })
        .collect();
    i = UnitInterface::new(
        g.root(),
        exports,
        Vec::new(),
        Vec::new(),
        g.imports(g.root()).to_vec(),
    );
    PersistentUnitInterface::published(&i, g, &UserTypes::default()).expect("publishable")
}

#[test]
fn a_declared_set_is_published_by_name_in_id_order_and_an_undeclared_one_is_absent() {
    use nazm_sema::{Effect, EffectSet};
    let g = graph();
    let both: EffectSet = [Effect::Spawn, Effect::Io].into_iter().collect();
    let json = |e| with_effects(&g, e).to_json().unwrap();
    assert!(
        json(Some(both)).contains(r#""ret":"Int","effects":["io","spawn"]}"#),
        "{}",
        json(Some(both))
    );
    assert!(json(Some(EffectSet::EMPTY)).contains(r#""effects":[]"#));
    // Absent, not empty: no declaration promises nothing, and `[]` promises purity.
    assert!(!json(None).contains("effects"), "{}", json(None));
    // Three contracts, three fingerprints; one set however it was built, one.
    let hash = |e| InterfaceHash::of(&with_effects(&g, e)).unwrap();
    let io = EffectSet::of(Effect::Io);
    assert_ne!(hash(None), hash(Some(EffectSet::EMPTY)));
    assert_ne!(hash(Some(EffectSet::EMPTY)), hash(Some(io)));
    assert_ne!(hash(Some(io)), hash(Some(both)));
    let other: EffectSet = [Effect::Io, Effect::Spawn, Effect::Io]
        .into_iter()
        .collect();
    assert_eq!(hash(Some(both)), hash(Some(other)));
}

#[test]
fn a_declared_set_is_read_back_exactly_and_a_malformed_one_is_refused() {
    use nazm_sema::{Effect, EffectSet};
    let g = graph();
    for effects in [
        None,
        Some(EffectSet::EMPTY),
        Some(EffectSet::of(Effect::Spawn)),
        Some(EffectSet::all()),
    ] {
        let text = with_effects(&g, effects).to_json().unwrap();
        let read = PersistentUnitInterface::from_json(&text).unwrap();
        assert_eq!(read.to_json().unwrap(), text);
        let mut types = UserTypes::default();
        let rehydrated = read.rehydrate(g.root(), FnId::from_index(0), &mut types);
        assert_eq!(
            rehydrated.exports().first().and_then(|e| e.effects),
            effects,
            "{text}"
        );
    }
    let file = |effects: &str| {
        format!(
            r#"{{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[],"ret":"Int","effects":{effects}}}]}}"#
        )
    };
    assert!(PersistentUnitInterface::from_json(&file(r#"["io","spawn"]"#)).is_ok());
    assert!(matches!(
        refuse(&file(r#"["spawn","io"]"#)),
        ReadError::NotCanonical(_)
    ));
    assert!(matches!(
        refuse(&file(r#"["io","io"]"#)),
        ReadError::NotCanonical(_)
    ));
    assert!(matches!(
        refuse(&file(r#"["network"]"#)),
        ReadError::UnknownEffect(_)
    ));
    assert!(matches!(
        refuse(&file(r#"["IO"]"#)),
        ReadError::UnknownEffect(_)
    ));
}

// --- function types (N50) ----------------------------------------------------------------

#[test]
fn a_function_type_is_published_with_its_effects_and_read_back_as_the_same_type() {
    let g = graph();
    let mut types = UserTypes::default();
    let io = nazm_sema::EffectSet::of(nazm_sema::Effect::Io);
    let f = types.fn_of(nazm_sema::FnSignature::new(
        vec![Type::Int, Type::Str],
        Type::Bool,
        io,
    ));
    let p =
        PersistentUnitInterface::published(&interface(&g, &[("run", &[f], Type::Int)]), &g, &types)
            .expect("publishable");
    let json = p.to_json().unwrap();
    assert!(
        json.contains(r#""params":[{"fn":["Int","Str"],"ret":"Bool","effects":["io"]}]"#),
        "{json}"
    );
    let back = PersistentUnitInterface::from_json(&json).unwrap();
    assert_eq!(back, p);

    // Read into a fresh table, the type is the one it was: parameters, result and effects.
    let mut fresh = UserTypes::default();
    let read = back.rehydrate(g.root(), FnId::from_index(0), &mut fresh);
    let t = read
        .exports()
        .first()
        .and_then(|e| e.params.first().copied())
        .unwrap();
    let Type::Fn(id) = t else {
        panic!("not a function type: {t:?}");
    };
    let sig = fresh.fn_signature(id).unwrap();
    assert_eq!(sig.params, [Type::Int, Type::Str]);
    assert_eq!(sig.ret, Type::Bool);
    assert_eq!(sig.effects, io);
}

#[test]
fn a_channel_type_is_published_and_read_back_as_the_same_type() {
    // N53: `Chan[T]` is written `{"chan":T}` and read back as a channel, not as anything
    // else with one type argument.
    let g = graph();
    let mut types = UserTypes::default();
    let c = types.chan_of(Type::Str);
    let p =
        PersistentUnitInterface::published(&interface(&g, &[("run", &[c], Type::Int)]), &g, &types)
            .expect("publishable");
    let json = p.to_json().unwrap();
    assert!(json.contains(r#""params":[{"chan":"Str"}]"#), "{json}");
    let back = PersistentUnitInterface::from_json(&json).unwrap();
    assert_eq!(back, p);
    let mut fresh = UserTypes::default();
    let read = back.rehydrate(g.root(), FnId::from_index(0), &mut fresh);
    let t = read
        .exports()
        .first()
        .and_then(|e| e.params.first().copied())
        .unwrap();
    let Type::ChanOf(id) = t else {
        panic!("not a channel type: {t:?}");
    };
    assert_eq!(fresh.chan_element(id), Some(Type::Str));
}

#[test]
fn a_function_type_with_an_unknown_or_unordered_effect_is_refused() {
    let with = |effects: &str| {
        format!(
            r#"{{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[{{"fn":["Int"],"ret":"Int","effects":{effects}}}],"ret":"Int"}}]}}"#
        )
    };
    assert!(PersistentUnitInterface::from_json(&with("[]")).is_ok());
    assert!(matches!(
        refuse(&with(r#"["fly"]"#)),
        ReadError::Malformed(_)
    ));
    assert!(matches!(
        refuse(&with(r#"["spawn","io"]"#)),
        ReadError::Malformed(_)
    ));
    assert!(matches!(
        refuse(&with(r#"["io","io"]"#)),
        ReadError::Malformed(_)
    ));
}

#[test]
fn an_effect_parameter_is_written_as_a_star_and_read_back_as_the_function_s_own() {
    let text = r#"{"schema":"nazm.interface/8","module":"a.nz","dependencies":[],"records":[],"enums":[],"exports":[{"def":"a.nz::fn f","name":"f","kind":"fn","generics":0,"params":[{"fn":["Int"],"ret":"Int","effects":["io","*"]}],"ret":"Int","effects":["io","*"]}]}"#;
    let read = PersistentUnitInterface::from_json(text).expect("reads");
    assert_eq!(read.to_json().unwrap(), text);
    let mut types = UserTypes::default();
    let g = graph();
    let iface = read.rehydrate(g.root(), FnId::from_index(3), &mut types);
    let e = iface.exports().first().unwrap();
    let effects = e.effects.unwrap();
    assert!(effects.has_param() && effects.contains(nazm_sema::Effect::Io));
    let Some(Type::Fn(id)) = e.params.first().copied() else {
        panic!("not a function type");
    };
    let sig = types.fn_signature(id).unwrap();
    assert_eq!(
        sig.row,
        Some(nazm_sema::types::ParamOwner::Function(FnId::from_index(3)))
    );
    // Out of order, the parameter is not first: refused like any other set.
    assert!(PersistentUnitInterface::from_json(
        &text.replace(r#"["io","*"]}],"#, r#"["*","io"]}],"#)
    )
    .is_err());
}
