//! N37 through the language service: a capability is an ordinary built-in type to every
//! tool, and nothing a tool offers constructs one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use nazm_service::{Context, LanguageService, SemanticClass, SemanticTarget};

mod common;
use common::on_the_service_stack;

fn at(text: &str, needle: &str, into: usize) -> u32 {
    u32::try_from(text.find(needle).unwrap_or_else(|| panic!("{needle:?}")) + into).unwrap()
}

const MAIN: &str = "fn save(io: IoCap, n: Int) -> Int ! { io } { print(int_to_str(n)) }\n\
fn keep(io: IoCap) -> IoCap ! {} { io }\n\
fn main(io: IoCap, tasks: SpawnCap) -> Int { save(keep(io), 1) }\n";

#[test]
fn a_capability_is_a_built_in_type_to_every_tool_and_never_a_constructor() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.nz");
    std::fs::write(&main, MAIN).unwrap();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&main, 1, MAIN.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");

        // Hover and signature show the authority a function requires as its parameters,
        // and the effects beside them: two facts, each where it is written.
        let h = s.hover(&main, 1, at(MAIN, "save(keep", 1)).unwrap();
        assert_eq!(h.text, "fn save(io: IoCap, n: Int) -> Int ! { io }");
        let h = s.hover(&main, 1, at(MAIN, "keep(io), 1", 1)).unwrap();
        assert_eq!(h.text, "fn keep(io: IoCap) -> IoCap ! {}");
        let h = s.hover(&main, 1, at(MAIN, "io), 1", 0)).unwrap();
        assert_eq!(h.text, "io: IoCap");

        // Semantic tokens class a capability type as the built-in type it is.
        let ids = s.semantic_identifiers(&main, 1).unwrap();
        let cap = at(MAIN, "IoCap, n", 0);
        let token = ids
            .identifiers
            .iter()
            .find(|i| i.start == cap)
            .expect("a token over `IoCap`");
        assert_eq!(token.class, SemanticClass::BuiltinType);
        assert!(matches!(token.target, SemanticTarget::BuiltinType(t) if t.is_capability()));

        // In a type position both capabilities are offered, as built-in types.
        let c = s.completion(&main, 1, at(MAIN, "IoCap, n", 3)).unwrap();
        assert_eq!(c.context, Context::Type);
        let names: Vec<(&str, &str)> = c.items.iter().map(|i| (i.name.as_str(), i.kind)).collect();
        assert_eq!(names, [("IoCap", "built-in type")]);
        let c = s.completion(&main, 1, at(MAIN, "SpawnCap)", 1)).unwrap();
        let names: Vec<&str> = c.items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"SpawnCap"), "{names:?}");

        // A call head offers what may be called, and a capability may not be built.
        let c = s.completion(&main, 1, at(MAIN, "save(keep", 0)).unwrap();
        assert_eq!(c.context, Context::Call);
        assert!(
            c.items
                .iter()
                .all(|i| i.name != "IoCap" && i.name != "SpawnCap"),
            "{:?}",
            c.items.iter().map(|i| &i.name).collect::<Vec<_>>()
        );
        // A value position offers the capability that is in scope, by its binding.
        let c = s.completion(&main, 1, at(MAIN, "io), 1", 0)).unwrap();
        assert_eq!(c.context, Context::Value);
        assert!(c.items.iter().any(|i| i.name == "io" && i.kind == "local"));
        assert!(c.items.iter().all(|i| i.name != "IoCap"));
    });
}

#[test]
fn authority_diagnostics_reach_the_editor_with_no_fix_to_apply() {
    let src = "fn f() -> Int ! { io } { print(\"x\") }\nfn main() -> Int { f() }\n";
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.nz");
    std::fs::write(&main, src).unwrap();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&main, 1, src.into());
        let d = &out[0].diagnostics;
        assert_eq!(d.len(), 1, "{d:#?}");
        assert_eq!(d[0].code.as_str(), "N0369");
        assert!(d[0].fixes.is_empty());
    });
}
