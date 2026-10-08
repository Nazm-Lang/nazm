//! Document and workspace symbols through the language service. N22.
//!
//! Every symbol is compared by where it is declared — its file, its name's bytes and its
//! declaration's bytes — and by the identity the checker gave it, never by its name alone,
//! so that two records' `value`, two enums' `Ready`, a record and a function both called
//! `Box`, or two modules' `helper` taken for one another fails. Every expected set is
//! written out exactly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use nazm_core::sema::ReferenceTarget;
use nazm_service::{LanguageService, NoSymbols, Symbol, SymbolKind, Target, WorkspaceSymbol};

mod common;
use common::{on_the_service_stack, walk, workspace, Project};

/// The byte range of the `n`th occurrence of `needle` in `text`.
fn span(text: &str, needle: &str, n: usize) -> (u32, u32) {
    let mut from = 0;
    let mut found = None;
    for _ in 0..=n {
        let i = text[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} #{n}"))
            + from;
        found = Some(i);
        from = i + needle.len();
    }
    let start = found.unwrap();
    (
        u32::try_from(start).unwrap(),
        u32::try_from(start + needle.len()).unwrap(),
    )
}

/// The one-byte name at the start of the `n`th occurrence of `needle`.
fn first(text: &str, needle: &str, n: usize) -> (u32, u32) {
    let (start, _) = span(text, needle, n);
    (start, start + 1)
}

/// A top-level symbol with its children's names: kind, name, name bytes, children's name
/// bytes.
type Outline = (SymbolKind, String, (u32, u32), Vec<(u32, u32)>);

/// A symbol tree as the tests compare it: kind, name, declaration bytes, name bytes, and
/// children.
#[derive(Debug, PartialEq, Eq)]
struct Shape {
    kind: SymbolKind,
    name: String,
    range: (u32, u32),
    selection: (u32, u32),
    children: Vec<Shape>,
}

fn shape(s: &Symbol) -> Shape {
    Shape {
        kind: s.kind,
        name: s.name.clone(),
        range: (s.start, s.end),
        selection: (s.name_start, s.name_end),
        children: s.children.iter().map(shape).collect(),
    }
}

/// `kind name` for the declaration written as `decl` (its `n`th occurrence) whose name is
/// the first occurrence of `name` inside it.
fn node(
    text: &str,
    kind: SymbolKind,
    decl: &str,
    n: usize,
    name: &str,
    children: Vec<Shape>,
) -> Shape {
    let range = span(text, decl, n);
    let within = decl
        .find(name)
        .unwrap_or_else(|| panic!("{name:?} in {decl:?}"));
    let start = range.0 + u32::try_from(within).unwrap();
    Shape {
        kind,
        name: name.to_owned(),
        range,
        selection: (start, start + u32::try_from(name.len()).unwrap()),
        children,
    }
}

/// Every symbol of a tree with its parent's target, depth first.
fn flat(symbols: &[Symbol]) -> Vec<(&Symbol, Option<ReferenceTarget>)> {
    fn walk<'a>(
        s: &'a Symbol,
        parent: Option<ReferenceTarget>,
        out: &mut Vec<(&'a Symbol, Option<ReferenceTarget>)>,
    ) {
        out.push((s, parent));
        for c in &s.children {
            walk(c, Some(s.target), out);
        }
    }
    let mut out = Vec::new();
    for s in symbols {
        walk(s, None, &mut out);
    }
    out
}

/// The containment laws, over a whole tree: a symbol's range holds its name, and a
/// parent's range holds each child's.
fn contained(symbols: &[Symbol]) {
    fn check(s: &Symbol) {
        assert!(
            s.start <= s.name_start && s.name_end <= s.end && s.name_start < s.name_end,
            "{} {:?}: range {}..{} does not hold its name {}..{}",
            s.name,
            s.kind,
            s.start,
            s.end,
            s.name_start,
            s.name_end
        );
        assert!(
            (s.name_start, s.name_end) != (s.start, s.end) || s.children.is_empty(),
            "{}: a declaration with children is more than its name",
            s.name
        );
        for c in &s.children {
            assert!(
                s.start <= c.start && c.end <= s.end,
                "{} is not inside {}",
                c.name,
                s.name
            );
            check(c);
        }
    }
    for s in symbols {
        check(s);
    }
}

/// A workspace symbol as the tests compare it: name, kind, container, file name, and the
/// name's bytes.
type Found = (String, SymbolKind, Option<String>, String, (u32, u32));

fn found(w: &[WorkspaceSymbol]) -> Vec<Found> {
    w.iter()
        .map(|s| {
            (
                s.name.clone(),
                s.kind,
                s.container.clone(),
                s.selection
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                (s.selection.start, s.selection.end),
            )
        })
        .collect()
}

fn names(w: &[WorkspaceSymbol]) -> Vec<(String, String)> {
    w.iter()
        .map(|s| {
            (
                s.name.clone(),
                s.selection
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// One document.
// ---------------------------------------------------------------------------------------

const SHAPES: &str = "\
struct A {
    value: Int,
    only_a: Int,
}
pub struct B {
    value: Str,
}
struct Box[T] {
    value: T,
}
enum EA {
    Ready,
    Done(code: Int, value: Str),
}
pub enum EB {
    Ready,
}
fn Box(x: Int) -> Int { let y = x; y }
pub fn identity[T](x: T) -> T { x }
fn uses(a: A, b: B, e: EA) -> Int {
    let s = b.value;
    let k = EB.Ready();
    let q = Box[Int](value: 1);
    match e {
        EA.Ready() => a.value,
        EA.Done(code: c, value: v) => Box(c),
    }
}
";

#[test]
fn document_symbols_are_the_declarations_nested_in_source_order() {
    let p = Project::new(&[("main.nz", SHAPES)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, SHAPES.to_owned());
        assert_eq!(published[0].diagnostics, [], "the fixture checks");
        let d = s.document_symbols(&main, 1).unwrap();
        let t = SHAPES;
        use SymbolKind::{Enum, Field, Function, PayloadField, Record, Variant};
        let expected = [
            node(
                t,
                Record,
                "struct A {\n    value: Int,\n    only_a: Int,\n}",
                0,
                "A",
                vec![
                    node(t, Field, "value: Int", 0, "value", vec![]),
                    node(t, Field, "only_a: Int", 0, "only_a", vec![]),
                ],
            ),
            node(
                t,
                Record,
                "pub struct B {\n    value: Str,\n}",
                0,
                "B",
                vec![node(t, Field, "value: Str", 0, "value", vec![])],
            ),
            // No `T`: a type parameter is not a declaration of the file's structure.
            node(
                t,
                Record,
                "struct Box[T] {\n    value: T,\n}",
                0,
                "Box",
                vec![node(t, Field, "value: T", 0, "value", vec![])],
            ),
            node(
                t,
                Enum,
                "enum EA {\n    Ready,\n    Done(code: Int, value: Str),\n}",
                0,
                "EA",
                vec![
                    node(t, Variant, "Ready", 0, "Ready", vec![]),
                    node(
                        t,
                        Variant,
                        "Done(code: Int, value: Str)",
                        0,
                        "Done",
                        vec![
                            node(t, PayloadField, "code: Int", 0, "code", vec![]),
                            node(t, PayloadField, "value: Str", 1, "value", vec![]),
                        ],
                    ),
                ],
            ),
            node(
                t,
                Enum,
                "pub enum EB {\n    Ready,\n}",
                0,
                "EB",
                vec![node(t, Variant, "Ready", 1, "Ready", vec![])],
            ),
            // A function sharing a record's name is a second declaration, in the other
            // namespace. No parameter or local is a child.
            node(
                t,
                Function,
                "fn Box(x: Int) -> Int { let y = x; y }",
                0,
                "Box",
                vec![],
            ),
            node(
                t,
                Function,
                "pub fn identity[T](x: T) -> T { x }",
                0,
                "identity",
                vec![],
            ),
        ];
        let got: Vec<Shape> = d.symbols.iter().map(shape).collect();
        // `uses` is last, and checked apart: its range is long.
        assert_eq!(got.len(), expected.len() + 1, "{got:#?}");
        assert_eq!(got[..expected.len()], expected[..]);
        assert_eq!(got[expected.len()].name, "uses");
        assert!(got[expected.len()].children.is_empty());
        contained(&d.symbols);

        // Visibility is kept, not used to filter: private declarations are in their own
        // outline.
        let exported: Vec<(&str, Option<bool>)> = d
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.exported))
            .collect();
        assert_eq!(
            exported,
            [
                ("A", Some(false)),
                ("B", Some(true)),
                ("Box", Some(false)),
                ("EA", Some(false)),
                ("EB", Some(true)),
                ("Box", Some(false)),
                ("identity", Some(true)),
                ("uses", Some(false)),
            ]
        );
        // Details, from the renderers other answers use.
        let details: Vec<(&str, Option<&str>)> = flat(&d.symbols)
            .into_iter()
            .map(|(s, _)| (s.name.as_str(), s.detail.as_deref()))
            .collect();
        assert_eq!(
            details,
            [
                ("A", None),
                ("value", Some("Int")),
                ("only_a", Some("Int")),
                ("B", None),
                ("value", Some("Str")),
                ("Box", None),
                ("value", Some("T")),
                ("EA", None),
                ("Ready", None),
                ("Done", Some("Done(code: Int, value: Str)")),
                ("code", Some("Int")),
                ("value", Some("Str")),
                ("EB", None),
                ("Ready", None),
                ("Box", Some("fn Box(x: Int) -> Int")),
                ("identity", Some("fn identity[T](x: T) -> T")),
                ("uses", Some("fn uses(a: A, b: B, e: EA) -> Int")),
            ]
        );
    });
}

#[test]
fn a_symbol_is_the_entity_references_and_definition_name() {
    // Each symbol's identity is the entity a reference to its name resolves to, and its
    // selection is where definition goes from every use: one table, read twice.
    let p = Project::new(&[("main.nz", SHAPES)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, SHAPES.to_owned());
        let d = s.document_symbols(&main, 1).unwrap();
        let all = flat(&d.symbols);
        let mut targets = BTreeSet::new();
        let mut used = 0;
        for (symbol, parent) in &all {
            assert!(targets.insert(symbol.target), "{} twice", symbol.name);
            assert_eq!(symbol.target.kind(), symbol.kind.word(), "{}", symbol.name);
            // A child's identity is its parent's member.
            match (symbol.target, parent) {
                (ReferenceTarget::Field(f), Some(ReferenceTarget::Record(r))) => {
                    assert_eq!(f.record, *r);
                }
                (ReferenceTarget::Variant(v), Some(ReferenceTarget::Enum(e))) => {
                    assert_eq!(v.enumeration, *e);
                }
                (ReferenceTarget::PayloadField(f), Some(ReferenceTarget::Variant(v))) => {
                    assert_eq!((f.enumeration, f.variant), (v.enumeration, v.variant));
                }
                (
                    ReferenceTarget::Function(_)
                    | ReferenceTarget::Record(_)
                    | ReferenceTarget::Enum(_),
                    None,
                ) => {}
                other => panic!("{} attached as {other:?}", symbol.name),
            }
            let r = s.references(&main, 1, symbol.name_start).unwrap();
            assert_eq!(r.kind, symbol.kind.word());
            let decl = r.declaration.unwrap();
            assert_eq!((decl.start, decl.end), (symbol.name_start, symbol.name_end));
            for u in &r.uses {
                let Target::File(to) = s.definition(&main, 1, u.start).unwrap().target else {
                    panic!("a source declaration");
                };
                assert_eq!((to.start, to.end), (symbol.name_start, symbol.name_end));
                used += 1;
            }
        }
        // `A.value`, `B.value`, `Box[Int]`'s `value`, both `Ready`s, `Box` the record and
        // `Box` the function, `EA` and its `Done` payload each have uses of their own.
        assert!(used >= 12, "{used}");
        let value = |parent: &str| {
            all.iter()
                .find(|(s, p)| {
                    s.name == "value"
                        && p.is_some_and(|p| {
                            d.symbols.iter().any(|t| t.target == p && t.name == parent)
                        })
                })
                .map(|(s, _)| s.target)
        };
        assert_ne!(value("A"), value("B"), "same spelling, two fields");
        let readies: Vec<ReferenceTarget> = all
            .iter()
            .filter(|(s, _)| s.name == "Ready")
            .map(|(s, _)| s.target)
            .collect();
        assert_eq!(readies.len(), 2);
        assert_ne!(readies[0], readies[1], "same spelling, two variants");
    });
}

#[test]
fn reordering_fields_reorders_the_outline_and_nothing_else() {
    // An outline presents the file: moving two fields moves them in it, though the record
    // means the same — the order N21 shows candidates in is the canonical one instead.
    const ONE: &str =
        "struct P {\n    b: Int,\n    a: Int,\n}\nenum E {\n    Y(q: Int, p: Int),\n    X,\n}\n";
    const TWO: &str =
        "struct P {\n    a: Int,\n    b: Int,\n}\nenum E {\n    X,\n    Y(p: Int, q: Int),\n}\n";
    let p = Project::new(&[("main.nz", ONE)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let order = |s: &LanguageService, v: i32| -> Vec<String> {
            flat(&s.document_symbols(&main, v).unwrap().symbols)
                .into_iter()
                .map(|(s, _)| s.name.clone())
                .collect()
        };
        s.open(&main, 1, ONE.to_owned());
        assert_eq!(order(&s, 1), ["P", "b", "a", "E", "Y", "q", "p", "X"]);
        s.change(&main, 2, TWO.to_owned());
        assert_eq!(order(&s, 2), ["P", "a", "b", "E", "X", "Y", "p", "q"]);
    });
}

#[test]
fn an_imported_declaration_is_not_in_the_importers_outline() {
    const LIB: &str = "pub struct Shared {\n    v: Int,\n}\npub fn helper() -> Int { 1 }\n";
    // `helper` is also declared here, which collides with the import — a diagnostic — and
    // is still this module's declaration, in this file.
    const MAIN: &str =
        "use \"lib.nz\";\nfn helper() -> Int { 2 }\nfn main() -> Int { let s = Shared(v: 1); s.v }\n";
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.to_owned());
        let d = s.document_symbols(&main, 1).unwrap();
        let got: Vec<Shape> = d.symbols.iter().map(shape).collect();
        assert_eq!(
            got,
            vec![
                node(
                    MAIN,
                    SymbolKind::Function,
                    "fn helper() -> Int { 2 }",
                    0,
                    "helper",
                    vec![]
                ),
                node(
                    MAIN,
                    SymbolKind::Function,
                    "fn main() -> Int { let s = Shared(v: 1); s.v }",
                    0,
                    "main",
                    vec![],
                ),
            ]
        );
        // Both `helper`s exist in the analysed universe, each in its own file.
        let w = s.workspace_symbols("helper");
        assert_eq!(
            names(&w),
            [
                ("helper".to_owned(), "lib.nz".to_owned()),
                ("helper".to_owned(), "main.nz".to_owned())
            ]
        );
        // And the dependency's own outline, when it is opened, is its declarations.
        s.open(&lib, 1, LIB.to_owned());
        let names: Vec<String> = flat(&s.document_symbols(&lib, 1).unwrap().symbols)
            .into_iter()
            .map(|(s, _)| s.name.clone())
            .collect();
        assert_eq!(names, ["Shared", "v", "helper"]);
    });
}

#[test]
fn a_symbol_exists_exactly_when_the_checker_declared_it() {
    // A body that did not parse keeps its declaration; a body with a type error is still
    // declared; an item the parser could not read was never declared; a second declaration
    // the checker refused has no symbol, and the first keeps its own.
    const TEXT: &str = "\
fn broken() -> Int { let x = ; 1 }
fn wrong() -> Int { \"s\" }
struct = 3;
fn twice() -> Int { 1 }
fn twice() -> Int { 2 }
struct T {
    a: Int,
    a: Str,
}
enum T {
    X,
}
enum V {
    X,
    X,
}
fn after() -> Int { 0 }
";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, TEXT.to_owned());
        assert!(!published[0].diagnostics.is_empty());
        let d = s.document_symbols(&main, 1).unwrap();
        let got: Vec<Outline> = d
            .symbols
            .iter()
            .map(|s| {
                (
                    s.kind,
                    s.name.clone(),
                    (s.name_start, s.name_end),
                    s.children
                        .iter()
                        .map(|c| (c.name_start, c.name_end))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (
                    SymbolKind::Function,
                    "broken".to_owned(),
                    span(TEXT, "broken", 0),
                    vec![]
                ),
                (
                    SymbolKind::Function,
                    "wrong".to_owned(),
                    span(TEXT, "wrong", 0),
                    vec![]
                ),
                (
                    SymbolKind::Function,
                    "twice".to_owned(),
                    span(TEXT, "twice", 0),
                    vec![]
                ),
                (
                    SymbolKind::Record,
                    "T".to_owned(),
                    first(TEXT, "T {", 0),
                    vec![first(TEXT, "a: Int", 0)],
                ),
                (
                    SymbolKind::Enum,
                    "V".to_owned(),
                    first(TEXT, "V {", 0),
                    vec![span(TEXT, "X", 1)]
                ),
                (
                    SymbolKind::Function,
                    "after".to_owned(),
                    span(TEXT, "after", 0),
                    vec![]
                ),
            ]
        );
        contained(&d.symbols);
    });
}

// ---------------------------------------------------------------------------------------
// The analysed source universe.
// ---------------------------------------------------------------------------------------

const COMMON: &str = "pub struct Point {\n    x: Int,\n}\npub fn shared() -> Int { 1 }\n";
const ROOT_A: &str = "use \"common.nz\";\nfn helper() -> Int { shared() }\n";
const ROOT_B: &str = "use \"common.nz\";\nfn helper() -> Int { let p = Point(x: 2); p.x }\n";
const ORPHAN: &str = "pub fn orphan() -> Int { 3 }\npub struct Point {\n    x: Int,\n}\n";

#[test]
fn workspace_symbols_are_the_open_programs_declarations_each_once() {
    let p = Project::new(&[
        ("common.nz", COMMON),
        ("a.nz", ROOT_A),
        ("b.nz", ROOT_B),
        // In the same directory, imported by nothing that is open: no program being
        // analysed has it, so it is not searched.
        ("orphan.nz", ORPHAN),
    ]);
    let (a, b) = (p.path("a.nz"), p.path("b.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        assert_eq!(
            s.workspace_symbols(""),
            [],
            "nothing open, nothing analysed"
        );
        s.open(&a, 1, ROOT_A.to_owned());
        s.open(&b, 1, ROOT_B.to_owned());
        let (px, py) = span(COMMON, "Point", 0);
        let (xs, xe) = span(COMMON, "x", 0);
        let (ss, se) = span(COMMON, "shared", 0);
        let (ha, hae) = span(ROOT_A, "helper", 0);
        let (hb, hbe) = span(ROOT_B, "helper", 0);
        let all = s.workspace_symbols("");
        // `common.nz` is loaded by both compilations and listed once; the two `helper`s
        // are two declarations and both stay; `orphan.nz`'s `Point` is not among them.
        assert_eq!(
            found(&all),
            [
                (
                    "Point".into(),
                    SymbolKind::Record,
                    None,
                    "common.nz".into(),
                    (px, py)
                ),
                (
                    "helper".into(),
                    SymbolKind::Function,
                    None,
                    "a.nz".into(),
                    (ha, hae)
                ),
                (
                    "helper".into(),
                    SymbolKind::Function,
                    None,
                    "b.nz".into(),
                    (hb, hbe)
                ),
                (
                    "shared".into(),
                    SymbolKind::Function,
                    None,
                    "common.nz".into(),
                    (ss, se)
                ),
                (
                    "x".into(),
                    SymbolKind::Field,
                    Some("Point".into()),
                    "common.nz".into(),
                    (xs, xe)
                ),
            ]
        );
        // Each location is a real file with the text it was computed from, and its range
        // holds its name.
        for w in &all {
            assert!(w.selection.path.is_file(), "{}", w.selection.path.display());
            assert_eq!(w.declaration.path, w.selection.path);
            assert!(w.declaration.start <= w.selection.start);
            assert!(w.selection.end <= w.declaration.end);
            assert_eq!(
                &w.selection.text[w.selection.start as usize..w.selection.end as usize],
                w.name
            );
        }
        // A private declaration is listed: the question is what exists, not what may be
        // named from somewhere.
        assert!(all
            .iter()
            .any(|w| w.name == "helper" && w.exported == Some(false)));

        // Closing one root leaves `common.nz`, which the other still loads.
        s.close(&a);
        assert_eq!(
            names(&s.workspace_symbols("")),
            [
                ("Point".to_owned(), "common.nz".to_owned()),
                ("helper".to_owned(), "b.nz".to_owned()),
                ("shared".to_owned(), "common.nz".to_owned()),
                ("x".to_owned(), "common.nz".to_owned()),
            ]
        );
        // Closing the last one leaves nothing: no compilation loads it any more.
        s.close(&b);
        assert_eq!(s.workspace_symbols(""), []);
    });
}

#[test]
fn a_workspace_query_is_a_case_sensitive_substring_of_the_name() {
    const TEXT: &str = "struct Point {\n    v: Int,\n}\nstruct Pointer {\n    v: Int,\n}\n\
                        enum Checkpoint {\n    Here,\n}\nfn Other() -> Int { 0 }\n";
    let p = Project::new(&[("main.nz", TEXT)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, TEXT.to_owned());
        let query = |s: &LanguageService, q: &str| -> Vec<(String, Option<String>)> {
            s.workspace_symbols(q)
                .into_iter()
                .map(|w| (w.name, w.container))
                .collect()
        };
        let named = |n: &str, c: Option<&str>| (n.to_owned(), c.map(str::to_owned));
        let before = (s.retained(), s.indexed());
        assert_eq!(
            query(&s, ""),
            [
                named("Checkpoint", None),
                named("Here", Some("Checkpoint")),
                named("Other", None),
                named("Point", None),
                named("Pointer", None),
                named("v", Some("Point")),
                named("v", Some("Pointer")),
            ]
        );
        assert_eq!(
            query(&s, "Point"),
            [named("Point", None), named("Pointer", None)]
        );
        assert_eq!(query(&s, "point"), [named("Checkpoint", None)]);
        assert_eq!(
            query(&s, "int"),
            [
                named("Checkpoint", None),
                named("Point", None),
                named("Pointer", None)
            ]
        );
        assert_eq!(query(&s, "Pointers"), []);
        // Filtering chose among what was already analysed; it analysed nothing.
        assert_eq!((s.retained(), s.indexed()), before);
    });
}

#[test]
fn workspace_order_is_name_file_position_kind_every_time() {
    // Same-named declarations in two modules and in two namespaces of one module: all
    // listed, in one order, however the documents were opened.
    const M1: &str =
        "struct Thing {\n    v: Int,\n}\nfn Thing() -> Int { 1 }\nenum Kind {\n    Thing,\n}\n";
    const M2: &str = "struct Thing {\n    v: Int,\n}\n";
    let p = Project::new(&[("m1.nz", M1), ("m2.nz", M2)]);
    let (m1, m2) = (p.path("m1.nz"), p.path("m2.nz"));
    on_the_service_stack(move || {
        let expected: Vec<Found> = vec![
            (
                "Thing".into(),
                SymbolKind::Record,
                None,
                "m1.nz".into(),
                span(M1, "Thing", 0),
            ),
            (
                "Thing".into(),
                SymbolKind::Function,
                None,
                "m1.nz".into(),
                span(M1, "Thing", 1),
            ),
            (
                "Thing".into(),
                SymbolKind::Variant,
                Some("Kind".into()),
                "m1.nz".into(),
                span(M1, "Thing", 2),
            ),
            (
                "Thing".into(),
                SymbolKind::Record,
                None,
                "m2.nz".into(),
                span(M2, "Thing", 0),
            ),
        ];
        for round in 0..10 {
            let mut s = LanguageService::new();
            let (first, second) = if round % 2 == 0 {
                (&m1, &m2)
            } else {
                (&m2, &m1)
            };
            s.open(first, 1, std::fs::read_to_string(first).unwrap());
            s.open(second, 1, std::fs::read_to_string(second).unwrap());
            assert_eq!(
                found(&s.workspace_symbols("Thing")),
                expected,
                "round {round}"
            );
        }
    });
}

#[test]
fn what_has_no_source_is_not_a_symbol() {
    // The core prelude's `Option` and `Result` are compiled into the toolchain, and a
    // module that could not be read declares nothing: neither is given a location.
    const MAIN: &str = "use \"gone.nz\";\nfn pick(o: Option[Int]) -> Int { match o { Option.Some(value: v) => v, Option.None() => 0, } }\n";
    let p = Project::new(&[("main.nz", MAIN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, MAIN.to_owned());
        assert!(!published[0].diagnostics.is_empty(), "gone.nz is missing");
        assert_eq!(
            names(&s.workspace_symbols("")),
            [("pick".to_owned(), "main.nz".to_owned())]
        );
        let d = s.document_symbols(&main, 1).unwrap();
        assert_eq!(d.symbols.len(), 1);
    });
}

#[test]
fn symbols_are_the_current_buffers_and_close_restores_the_disk() {
    const LIB: &str = "pub struct Shape {\n    side: Int,\n}\npub enum Mode {\n    On,\n}\n";
    const LIB_EDITED: &str =
        "pub struct Figure {\n    edge: Int,\n}\npub fn added() -> Int { 1 }\n";
    const MAIN: &str = "use \"lib.nz\";\nfn main() -> Int { 0 }\n";
    const MAIN_EDITED: &str =
        "use \"lib.nz\";\nfn main() -> Int { 0 }\nenum Local {\n    A,\n    B(n: Int),\n}\n";
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let (lib, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let all = |s: &LanguageService| -> Vec<String> {
            s.workspace_symbols("")
                .into_iter()
                .map(|w| w.name)
                .collect()
        };
        s.open(&main, 1, MAIN.to_owned());
        assert_eq!(all(&s), ["Mode", "On", "Shape", "main", "side"]);
        // An unsaved edit to the dependency: a record renamed, a field renamed, an enum
        // removed, a function added — seen at once, from every compilation that loads it.
        s.open(&lib, 1, LIB_EDITED.to_owned());
        assert_eq!(all(&s), ["Figure", "added", "edge", "main"]);
        let names: Vec<String> = flat(&s.document_symbols(&lib, 1).unwrap().symbols)
            .into_iter()
            .map(|(s, _)| s.name.clone())
            .collect();
        assert_eq!(names, ["Figure", "edge", "added"]);
        // An unsaved edit to the document: an enum with a variant and a payload added.
        s.change(&main, 2, MAIN_EDITED.to_owned());
        assert_eq!(
            all(&s),
            ["A", "B", "Figure", "Local", "added", "edge", "main", "n"]
        );
        let n = s
            .workspace_symbols("n")
            .into_iter()
            .find(|w| w.name == "n")
            .unwrap();
        assert_eq!(n.container.as_deref(), Some("Local.B"));
        // A superseded version has no symbols; there is no previous answer to fall back
        // on.
        assert_eq!(s.document_symbols(&main, 1), Err(NoSymbols::NotCurrent));
        assert!(s.document_symbols(&main, 2).is_ok());
        // Closing the dependency makes the disk authoritative for it again.
        s.close(&lib);
        assert_eq!(s.document_symbols(&lib, 1), Err(NoSymbols::NotCurrent));
        assert_eq!(
            all(&s),
            ["A", "B", "Local", "Mode", "On", "Shape", "main", "n", "side"]
        );
    });
}

#[test]
fn a_thousand_edits_leave_one_current_outline() {
    const ONE: &str = "struct R {\n    a: Int,\n}\nfn f() -> Int { 1 }\n";
    const TWO: &str = "struct S {\n    b: Int,\n    c: Int,\n}\nenum E {\n    X,\n}\n";
    let p = Project::new(&[("main.nz", ONE)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 0, ONE.to_owned());
        let baseline = (s.retained(), s.indexed(), s.scoped(), s.calls());
        for v in 1..=1000 {
            let (text, expected): (&str, &[&str]) = if v % 2 == 0 {
                (ONE, &["R", "a", "f"])
            } else {
                (TWO, &["S", "b", "c", "E", "X"])
            };
            s.change(&main, v, text.to_owned());
            let names: Vec<String> = flat(&s.document_symbols(&main, v).unwrap().symbols)
                .into_iter()
                .map(|(s, _)| s.name.clone())
                .collect();
            assert_eq!(names, expected, "version {v}");
            assert_eq!(s.document_symbols(&main, v - 1), Err(NoSymbols::NotCurrent));
            assert_eq!(s.workspace_symbols("").len(), expected.len());
            if v % 2 == 0 {
                // What is held is a function of what is open, never of the history.
                assert_eq!(
                    (s.retained(), s.indexed(), s.scoped(), s.calls()),
                    baseline,
                    "version {v}"
                );
            }
        }
    });
}

// ---------------------------------------------------------------------------------------
// The repository's own source.
// ---------------------------------------------------------------------------------------

/// Every declaration the concrete tree of `text` holds, independently of the service:
/// `(kind, name span, declaration span, parent's name span)`, read from the item nodes and
/// their first and last significant tokens.
type Written = (SymbolKind, (u32, u32), (u32, u32), Option<(u32, u32)>);

fn written(text: &str) -> Vec<Written> {
    use nazm_core::__syntax::{CstNode, SyntaxKind};
    fn extent(n: CstNode<'_>) -> (u32, u32) {
        let leaves: Vec<_> = n
            .leaves()
            .into_iter()
            .filter(|l| !l.kind.is_trivia())
            .collect();
        (
            leaves.first().unwrap().span.start,
            leaves.last().unwrap().span.end,
        )
    }
    fn name(n: CstNode<'_>) -> (u32, u32) {
        let leaves = n.child_leaves();
        // A function's name follows `fn`: a foreign declaration's `extern` is an identifier
        // before it (N85).
        let from = if n.kind() == SyntaxKind::FnItem {
            leaves
                .iter()
                .position(|l| l.kind == SyntaxKind::FnKw)
                .map_or(0, |i| i + 1)
        } else {
            0
        };
        let l = leaves
            .iter()
            .skip(from)
            .find(|l| l.kind == SyntaxKind::Ident)
            .unwrap();
        (l.span.start, l.span.end)
    }
    fn visit(n: CstNode<'_>, parent: Option<(u32, u32)>, out: &mut Vec<Written>) {
        let kind = match n.kind() {
            SyntaxKind::FnItem => Some(SymbolKind::Function),
            SyntaxKind::RecordItem => Some(SymbolKind::Record),
            SyntaxKind::EnumItem => Some(SymbolKind::Enum),
            SyntaxKind::Variant => Some(SymbolKind::Variant),
            SyntaxKind::Field if parent.is_some() => Some(SymbolKind::Field),
            _ => None,
        };
        let mut inner = parent;
        if let Some(mut kind) = kind {
            if kind == SymbolKind::Field
                && out
                    .iter()
                    .any(|w| Some(w.1) == parent && w.0 == SymbolKind::Variant)
            {
                kind = SymbolKind::PayloadField;
            }
            let at = name(n);
            out.push((kind, at, extent(n), parent));
            inner = Some(at);
            if kind == SymbolKind::Function {
                // Nothing inside a function is a symbol.
                return;
            }
        }
        for c in n.children() {
            visit(c, inner, out);
        }
    }
    let cst = nazm_core::__syntax::parse_cst(nazm_span::FileId::ROOT, text).cst;
    let mut out = Vec::new();
    visit(cst.root(), None, &mut out);
    out
}

#[test]
fn every_written_declaration_of_the_repository_is_one_symbol() {
    // Over real source: in every file that checks cleanly, each declaration its concrete
    // tree holds is exactly one document symbol — the same kind, name, declaration range
    // and parent — whose identity is the entity the reference index records at that name.
    // And nothing else is a symbol.
    let root = workspace();
    let mut files = walk(&root.join("compiler"));
    files.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let mut audited = [0usize; 6];
        let mut documents = 0;
        for path in &files {
            let Ok(decided) = nazm_service::root::decide(path, None) else {
                continue;
            };
            let Ok(loaded) = nazm_service::load::load(path, decided.path()) else {
                continue;
            };
            let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
            if !a.diagnostics.is_empty() {
                continue;
            }
            let index = nazm_core::sema::ReferenceIndex::of(&a.resolution);
            let text = std::fs::read_to_string(path).unwrap();
            let mut s = LanguageService::new();
            s.open(path, 1, text.clone());
            let d = s.document_symbols(path, 1).unwrap();
            contained(&d.symbols);
            let all = flat(&d.symbols);
            let mut expected = written(&text);
            let mut got: Vec<Written> = all
                .iter()
                .map(|(s, parent)| {
                    let parent = parent.map(|p| {
                        let (ps, _) = all.iter().find(|(t, _)| t.target == p).unwrap();
                        (ps.name_start, ps.name_end)
                    });
                    (s.kind, (s.name_start, s.name_end), (s.start, s.end), parent)
                })
                .collect();
            expected.sort_unstable();
            got.sort_unstable();
            assert_eq!(got, expected, "{}", path.display());
            for (symbol, _) in &all {
                let at = nazm_span::Span::new(
                    nazm_span::FileId::ROOT,
                    symbol.name_start,
                    symbol.name_end,
                );
                assert_eq!(
                    index.target(at),
                    Some(symbol.target),
                    "{} in {}",
                    symbol.name,
                    path.display()
                );
                assert_eq!(index.declaration(symbol.target), Some(at));
                audited[symbol.kind as usize] += 1;
            }
            documents += 1;
        }
        eprintln!(
            "{documents} documents: functions {}, records {}, enums {}, variants {}, fields {}, payload fields {}",
            audited[0], audited[1], audited[2], audited[3], audited[4], audited[5]
        );
        assert!(audited.iter().all(|c| *c > 0), "{audited:?}");
    });
}

#[test]
fn the_compilers_workspace_is_every_declaration_of_its_files_once() {
    // The compiler opened as four programs whose imports overlap — `lex.nz` is loaded by
    // all four — lists every declaration of the eight files those programs load, each
    // exactly once, and nothing from anywhere else.
    let compiler = workspace().join("compiler");
    let roots: Vec<PathBuf> = ["check.nz", "emit.nz", "parser.nz", "lexer.nz"]
        .iter()
        .map(|f| compiler.join(f))
        .collect();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        for r in &roots {
            s.open(r, 1, std::fs::read_to_string(r).unwrap());
        }
        let all = s.workspace_symbols("");
        let mut got: Vec<(PathBuf, SymbolKind, (u32, u32))> = all
            .iter()
            .map(|w| {
                (
                    w.selection.path.canonicalize().unwrap(),
                    w.kind,
                    (w.selection.start, w.selection.end),
                )
            })
            .collect();
        let mut expected = Vec::new();
        let mut files = BTreeSet::new();
        for f in [
            "analyse.nz",
            "check.nz",
            "emit.nz",
            "lex.nz",
            "lexer.nz",
            "module.nz",
            "parse.nz",
            "parser.nz",
        ] {
            let path = compiler.join(f).canonicalize().unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            for (kind, name, _, _) in written(&text) {
                expected.push((path.clone(), kind, name));
            }
            files.insert(path);
        }
        let listed: BTreeSet<PathBuf> = got.iter().map(|g| g.0.clone()).collect();
        assert_eq!(listed, files, "exactly the files the open programs load");
        let before = got.len();
        got.sort_unstable();
        got.dedup();
        assert_eq!(got.len(), before, "a declaration listed twice");
        expected.sort_unstable();
        assert_eq!(got, expected);
        eprintln!("{} workspace symbols over {} files", got.len(), files.len());
    });
}
