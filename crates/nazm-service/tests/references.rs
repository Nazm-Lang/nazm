//! References, driven through the language service directly: no protocol. N17.
//!
//! A reference is an occurrence the compiler's resolution resolved to an entity. Every test
//! here is shaped so that grouping occurrences by their spelling would fail it: the same
//! name is given to functions, parameters, shadowing locals, fields of two records,
//! variants of two enums and private functions of two modules, and each must come back as
//! its own set.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_service::{LanguageService, Location, References, Target};

mod common;
use common::{on_the_service_stack, walk, workspace, Project};

/// The byte offset of the `n`th occurrence of `needle` in `text`, plus `into` bytes.
fn at(text: &str, needle: &str, n: usize, into: usize) -> u32 {
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
    u32::try_from(found.unwrap() + into).unwrap()
}

/// A location as `(file name, start, the bytes it covers)`.
fn spot(l: &Location) -> (String, u32, String) {
    (
        l.path.file_name().unwrap().to_string_lossy().into_owned(),
        l.start,
        l.text[l.start as usize..l.end as usize].to_owned(),
    )
}

fn spots(ls: &[Location]) -> Vec<(String, u32, String)> {
    ls.iter().map(spot).collect()
}

/// What the test expects at `(file, offset)`: a name of `len` bytes.
fn want(file: &str, offset: u32, name: &str) -> (String, u32, String) {
    (file.to_owned(), offset, name.to_owned())
}

fn refs(s: &LanguageService, path: &Path, version: i32, offset: u32) -> References {
    s.references(path, version, offset)
        .unwrap_or_else(|| panic!("no references at {offset} of {}", path.display()))
}

// ---------------------------------------------------------------------------------------
// The adversarial program.
// ---------------------------------------------------------------------------------------

const LIB: &str = "pub fn helper(n: Int) -> Int { n + 1 }\n\
fn private() -> Int { 1 }\n\
pub fn twice(n: Int) -> Int { helper(helper(n)) + private() }\n";

const OTHER: &str = "pub fn other() -> Int { private() }\nfn private() -> Int { 2 }\n";

const MAIN: &str = "use \"lib.nz\";\n\
use \"other.nz\";\n\
struct A { value: Int, }\n\
struct B { value: Int, }\n\
enum E { Ready, Busy(value: Int), }\n\
enum F { Ready, }\n\
fn value() -> Int { 7 }\n\
fn f(value: Int) -> Int {\n    \
    let a = A(value: value);\n    \
    let mut b = B(value: a.value);\n    \
    b.value = b.value + 1;\n    \
    let value = b.value + value();\n    \
    let inner = { let value = value + 1; value };\n    \
    value + inner\n\
}\n\
fn g(e: E) -> Int { match e { E.Ready() => 0, E.Busy(value: v) => v, } }\n\
fn h(x: F) -> Int { match x { F.Ready() => 1, } }\n\
fn main() -> Int { f(helper(1)) + g(E.Busy(value: 3)) + g(E.Ready()) + h(F.Ready()) + twice(2) + other() }\n";

fn adversarial() -> Project {
    Project::new(&[("lib.nz", LIB), ("other.nz", OTHER), ("main.nz", MAIN)])
}

/// One entity's expected declaration and uses, and the positions to ask from: its
/// declaration and every one of its uses must give the same answer.
struct Case {
    what: &'static str,
    kind: &'static str,
    declaration: (String, u32, String),
    uses: Vec<(String, u32, String)>,
}

fn cases() -> Vec<Case> {
    let m = |needle: &str, n: usize, into: usize, name: &str| {
        want("main.nz", at(MAIN, needle, n, into), name)
    };
    let l = |needle: &str, n: usize, into: usize, name: &str| {
        want("lib.nz", at(LIB, needle, n, into), name)
    };
    let o = |needle: &str, n: usize, into: usize, name: &str| {
        want("other.nz", at(OTHER, needle, n, into), name)
    };
    vec![
        Case {
            what: "the function `value`",
            kind: "function",
            declaration: m("fn value", 0, 3, "value"),
            uses: vec![m("+ value()", 0, 2, "value")],
        },
        Case {
            what: "the parameter `value`",
            kind: "local",
            declaration: m("f(value", 0, 2, "value"),
            uses: vec![m("value: value)", 0, 7, "value")],
        },
        Case {
            what: "the `let value` that shadows the parameter",
            kind: "local",
            declaration: m("let value = b", 0, 4, "value"),
            uses: vec![
                m("= value + 1", 0, 2, "value"),
                m("value + inner", 0, 0, "value"),
            ],
        },
        Case {
            what: "the inner `let value` that shadows the outer one",
            kind: "local",
            declaration: m("let value = value", 0, 4, "value"),
            uses: vec![m("value }", 0, 0, "value")],
        },
        Case {
            what: "`A.value`",
            kind: "field",
            declaration: m("A { value", 0, 4, "value"),
            uses: vec![m("A(value", 0, 2, "value"), m("a.value", 0, 2, "value")],
        },
        Case {
            what: "`B.value`: a construction label, an assignment and two reads",
            kind: "field",
            declaration: m("B { value", 0, 4, "value"),
            uses: vec![
                m("B(value", 0, 2, "value"),
                m("b.value", 0, 2, "value"),
                m("b.value", 1, 2, "value"),
                m("b.value", 2, 2, "value"),
            ],
        },
        Case {
            what: "the payload field `E.Busy.value`",
            kind: "payload field",
            declaration: m("Busy(value", 0, 5, "value"),
            uses: vec![
                m("Busy(value: v", 0, 5, "value"),
                m("Busy(value: 3", 0, 5, "value"),
            ],
        },
        Case {
            what: "`E.Ready`",
            kind: "variant",
            declaration: m("Ready,", 0, 0, "Ready"),
            uses: vec![m("E.Ready()", 0, 2, "Ready"), m("E.Ready()", 1, 2, "Ready")],
        },
        Case {
            what: "`F.Ready`",
            kind: "variant",
            declaration: m("Ready,", 1, 0, "Ready"),
            uses: vec![m("F.Ready()", 0, 2, "Ready"), m("F.Ready()", 1, 2, "Ready")],
        },
        Case {
            what: "the record `A`",
            kind: "record",
            declaration: m("struct A", 0, 7, "A"),
            uses: vec![m("A(value", 0, 0, "A")],
        },
        Case {
            what: "`helper`, across files",
            kind: "function",
            declaration: l("fn helper", 0, 3, "helper"),
            uses: vec![
                l("helper(helper", 0, 0, "helper"),
                l("helper(n))", 0, 0, "helper"),
                m("helper(1)", 0, 0, "helper"),
            ],
        },
        Case {
            what: "`lib.nz`'s private `private`",
            kind: "function",
            declaration: l("fn private", 0, 3, "private"),
            uses: vec![l("+ private()", 0, 2, "private")],
        },
        Case {
            what: "`other.nz`'s private `private`",
            kind: "function",
            declaration: o("fn private", 0, 3, "private"),
            uses: vec![o("{ private()", 0, 2, "private")],
        },
    ]
}

fn text_of(file: &str) -> &'static str {
    match file {
        "main.nz" => MAIN,
        "lib.nz" => LIB,
        "other.nz" => OTHER,
        _ => panic!("{file}"),
    }
}

#[test]
fn references_follow_semantic_identity_not_spelling() {
    let p = adversarial();
    let paths: Vec<(String, PathBuf)> = ["main.nz", "lib.nz", "other.nz"]
        .iter()
        .map(|n| ((*n).to_owned(), p.path(n)))
        .collect();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        for (name, path) in &paths {
            let out = s.open(path, 1, text_of(name).into());
            for published in out {
                assert!(published.diagnostics.is_empty(), "{published:#?}");
            }
        }
        let path_of = |file: &str| paths.iter().find(|(n, _)| n == file).unwrap().1.clone();
        let mut seen = Vec::new();
        for case in cases() {
            // Ask from the declaration and from every use: one entity, one answer.
            let mut asked = vec![case.declaration.clone()];
            asked.extend(case.uses.iter().cloned());
            for (file, offset, _) in &asked {
                let r = refs(&s, &path_of(file), 1, *offset);
                assert_eq!(r.kind, case.kind, "{} from {file}:{offset}", case.what);
                assert_eq!(
                    r.declaration.as_ref().map(spot),
                    Some(case.declaration.clone()),
                    "{} from {file}:{offset}",
                    case.what
                );
                assert_eq!(
                    spots(&r.uses),
                    case.uses,
                    "{} from {file}:{offset}",
                    case.what
                );
            }
            seen.push(case.uses);
        }
        // No two entities share an occurrence, which a grouping by spelling would.
        let all: Vec<_> = seen.iter().flatten().collect();
        let mut unique = all.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(all.len(), unique.len(), "an occurrence in two sets");
    });
}

#[test]
fn definition_and_references_agree_by_construction() {
    // For every position of every open file: when definition has an answer, references
    // has one of the same kind, with that declaration; and asking from each use it lists
    // gives the same definition and the same set.
    let p = adversarial();
    let paths: Vec<(String, PathBuf)> = ["main.nz", "lib.nz", "other.nz"]
        .iter()
        .map(|n| ((*n).to_owned(), p.path(n)))
        .collect();
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        for (name, path) in &paths {
            s.open(path, 1, text_of(name).into());
        }
        let path_of = |file: &str| paths.iter().find(|(n, _)| n == file).unwrap().1.clone();
        let mut answered = 0;
        for (name, path) in &paths {
            let text = text_of(name);
            for offset in 0..=u32::try_from(text.len()).unwrap() {
                let d = s.definition(path, 1, offset);
                let r = s.references(path, 1, offset);
                assert_eq!(d.is_some(), r.is_some(), "{name}:{offset}");
                let (Some(d), Some(r)) = (d, r) else { continue };
                answered += 1;
                assert_eq!(d.kind, r.kind);
                let Target::File(declared) = &d.target else {
                    panic!("everything here is declared in a file")
                };
                assert_eq!(r.declaration.as_ref(), Some(declared), "{name}:{offset}");
                for u in &r.uses {
                    let file = u.path.file_name().unwrap().to_string_lossy().into_owned();
                    let again = s.definition(&path_of(&file), 1, u.start).unwrap();
                    assert_eq!(
                        again.target, d.target,
                        "{name}:{offset} → {file}:{}",
                        u.start
                    );
                    assert_eq!(s.references(&path_of(&file), 1, u.start).as_ref(), Some(&r));
                    assert!(
                        !r.declaration.iter().any(|d| d == u),
                        "a declaration is not a use of itself"
                    );
                }
            }
        }
        assert!(answered > 200, "{answered}");
    });
}

#[test]
fn a_shadowed_use_belongs_to_the_inner_binding_and_no_further() {
    let text = "fn main() -> Int {\n    let n = 1;\n    let a = { let n = n + 10; n * 2 };\n    let b = { n };\n    n + a + b\n}\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, text.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let outer = refs(&s, &m, 1, at(text, "let n = 1", 0, 4));
        assert_eq!(
            spots(&outer.uses),
            vec![
                want("m.nz", at(text, "n + 10", 0, 0), "n"),
                want("m.nz", at(text, "{ n }", 0, 2), "n"),
                want("m.nz", at(text, "n + a", 0, 0), "n"),
            ],
            "the outer `n`: before the shadow, beside it and after it — never inside it"
        );
        let inner = refs(&s, &m, 1, at(text, "n * 2", 0, 0));
        assert_eq!(
            inner.declaration.as_ref().map(spot),
            Some(want("m.nz", at(text, "let n = n", 0, 4), "n"))
        );
        assert_eq!(
            spots(&inner.uses),
            vec![want("m.nz", at(text, "n * 2", 0, 0), "n")]
        );
    });
}

#[test]
fn a_field_reached_through_two_instances_of_one_generic_record_is_one_field() {
    let text = "struct Box[T] { value: T, }\n\
fn main() -> Int {\n    let a = Box[Int](value: 1);\n    let b = Box[Str](value: \"x\");\n    a.value + str_len(b.value)\n}\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, text.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let r = refs(&s, &m, 1, at(text, "a.value", 0, 2));
        assert_eq!(r.kind, "field");
        assert_eq!(
            r.declaration.as_ref().map(spot),
            Some(want("m.nz", at(text, "value: T", 0, 0), "value"))
        );
        assert_eq!(
            spots(&r.uses),
            vec![
                want("m.nz", at(text, "value: 1", 0, 0), "value"),
                want("m.nz", at(text, "value: \"x\"", 0, 0), "value"),
                want("m.nz", at(text, "a.value", 0, 2), "value"),
                want("m.nz", at(text, "b.value", 0, 2), "value"),
            ]
        );
        let record = refs(&s, &m, 1, at(text, "Box[Str]", 0, 0));
        assert_eq!(record.uses.len(), 2, "both constructions are of `Box`");
    });
}

// ---------------------------------------------------------------------------------------
// includeDeclaration, order and duplicates.
// ---------------------------------------------------------------------------------------

#[test]
fn the_declaration_is_kept_apart_and_added_only_when_asked_for() {
    let p = adversarial();
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        let r = refs(&s, &main, 1, at(MAIN, "helper(1)", 0, 0));
        let without = r.locations(false);
        assert_eq!(without, r.uses, "without the declaration: the uses, only");
        let with = r.locations(true);
        assert_eq!(with.len(), r.uses.len() + 1);
        let d = r.declaration.clone().unwrap();
        assert!(with.contains(&d) && !without.contains(&d));
        // And in the same canonical order either way.
        let key = |l: &Location| (l.path.clone(), l.start, l.end);
        let mut sorted = with.clone();
        sorted.sort_by_key(key);
        assert_eq!(with, sorted);
    });
}

#[test]
fn two_identical_analyses_give_identical_ordered_answers_with_no_duplicates() {
    let p = adversarial();
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let answer = || {
            let mut s = LanguageService::new();
            s.open(&main, 1, MAIN.into());
            (0..u32::try_from(MAIN.len()).unwrap())
                .filter_map(|o| s.references(&main, 1, o))
                .collect::<Vec<_>>()
        };
        let (first, second) = (answer(), answer());
        assert_eq!(first, second);
        for r in &first {
            let key = |l: &Location| (l.path.clone(), l.start, l.end);
            let keys: Vec<_> = r.uses.iter().map(key).collect();
            let mut canonical = keys.clone();
            canonical.sort();
            canonical.dedup();
            assert_eq!(keys, canonical, "ordered, and each occurrence once");
        }
    });
}

// ---------------------------------------------------------------------------------------
// Where there is nothing to refer to.
// ---------------------------------------------------------------------------------------

#[test]
fn a_position_that_refers_to_nothing_has_no_references() {
    let p = Project::new(&[("lib.nz", LIB)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let text = "use \"lib.nz\";\nfn main() -> Int { let s = \"helper\"; helper( 41 ) + str_len(s) + missing() } // helper\nfn broken(p: Int) -> Int { let x = helper(p +; x }\n";
        s.open(&main, 1, text.into());
        let none = |o: u32, why: &str| {
            assert!(s.references(&main, 1, o).is_none(), "{why}");
        };
        none(at(text, "( 41", 0, 1), "whitespace");
        none(at(text, "( 41", 0, 0) + 1, "after punctuation");
        none(at(text, "// helper", 0, 5), "a comment");
        none(at(text, "\"helper\"", 0, 3), "the contents of a string");
        none(at(text, "missing", 0, 2), "a name that resolved to nothing");
        none(at(text, "str_len(s)", 0, 1), "a built-in");
        none(
            at(text, "Int {", 0, 1),
            "a type annotation: not resolved to an entity",
        );
        none(
            at(text, "helper(p", 0, 2),
            "a name inside a body that did not parse",
        );
        none(u32::try_from(text.len()).unwrap(), "the end of the file");
        none(9999, "past the end");
        // And the healthy name beside all that still answers.
        assert!(s
            .references(&main, 1, at(text, "helper( 41", 0, 0))
            .is_some());
    });
}

#[test]
fn a_prelude_entity_has_uses_in_files_and_no_declaration_to_go_to() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let text = "fn main() -> Int {\n    let r = Result[Int, Int].Ok(value: 1);\n    match r { Result.Ok(value: v) => v, Result.Err(error: e) => e, }\n}\n";
        let out = s.open(&m, 1, text.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let r = refs(&s, &m, 1, at(text, "Ok(value: 1", 0, 0));
        assert_eq!(r.kind, "variant");
        assert_eq!(r.declaration, None, "the prelude has no file to point at");
        assert_eq!(
            spots(&r.uses),
            vec![
                want("m.nz", at(text, "Ok(value: 1", 0, 0), "Ok"),
                want("m.nz", at(text, "Ok(value: v", 0, 0), "Ok"),
            ],
            "only uses in files; the prelude's own are not navigable"
        );
    });
}

// ---------------------------------------------------------------------------------------
// The current, unsaved program.
// ---------------------------------------------------------------------------------------

#[test]
fn references_are_for_the_current_unsaved_program() {
    let lib = "pub fn helper(n: Int) -> Int { n + 1 }\n";
    let main = "use \"lib.nz\";\nfn main() -> Int { helper(1) }\n";
    let p = Project::new(&[("lib.nz", lib), ("main.nz", main)]);
    let (lib_path, main_path) = (p.path("lib.nz"), p.path("main.nz"));
    let decl = at(lib, "helper", 0, 0);
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main_path, 1, main.into());
        s.open(&lib_path, 1, lib.into());
        let uses = |s: &LanguageService, v: i32| spots(&refs(s, &lib_path, v, decl).uses);
        assert_eq!(
            uses(&s, 1),
            vec![want("main.nz", at(main, "helper", 0, 0), "helper")]
        );

        // An unsaved edit adds a use...
        let added = "use \"lib.nz\";\nfn main() -> Int { helper(1) + helper(2) }\n";
        s.change(&main_path, 2, added.into());
        assert_eq!(
            uses(&s, 1),
            vec![
                want("main.nz", at(added, "helper", 0, 0), "helper"),
                want("main.nz", at(added, "helper", 1, 0), "helper"),
            ]
        );
        // ...and one in the dependency's own unsaved text, which moves its declaration.
        let lib2 = "// moved\npub fn helper(n: Int) -> Int { n + 1 }\npub fn more() -> Int { helper(3) }\n";
        s.change(&lib_path, 2, lib2.into());
        let r = refs(&s, &lib_path, 2, at(lib2, "helper", 0, 0));
        assert_eq!(
            r.declaration.as_ref().map(spot),
            Some(want("lib.nz", at(lib2, "helper", 0, 0), "helper"))
        );
        assert_eq!(r.uses.len(), 3, "{:#?}", spots(&r.uses));
        assert!(
            s.references(&lib_path, 1, decl).is_none(),
            "version 1 is gone"
        );

        // An unsaved edit removes one.
        let removed = "use \"lib.nz\";\nfn main() -> Int { 2 }\n";
        s.change(&main_path, 3, removed.into());
        assert_eq!(
            spots(&refs(&s, &lib_path, 2, at(lib2, "helper", 0, 0)).uses),
            vec![want("lib.nz", at(lib2, "helper(3)", 0, 0), "helper")]
        );

        // Closing the dependency gives it back to the disk: its declaration where the
        // file has it, and the importer's use from its buffer.
        s.change(&main_path, 4, main.into());
        s.close(&lib_path);
        let r = refs(&s, &main_path, 4, at(main, "helper", 0, 0));
        assert_eq!(
            r.declaration.as_ref().map(spot),
            Some(want("lib.nz", decl, "helper"))
        );
        assert_eq!(
            spots(&r.uses),
            vec![want("main.nz", at(main, "helper", 0, 0), "helper")]
        );
        // Nothing was written.
        assert_eq!(p.read("lib.nz"), lib);
        assert_eq!(p.read("main.nz"), main);
    });
}

#[test]
fn an_unsaved_edit_can_make_a_use_refer_to_another_entity_of_the_same_name() {
    let a = "pub fn pick() -> Int { 1 }\n";
    let b = "pub fn pick() -> Int { 2 }\n";
    let v1 = "use \"a.nz\";\nfn main() -> Int { pick() }\n";
    let v2 = "use \"b.nz\";\nfn main() -> Int { pick() }\n";
    let p = Project::new(&[("a.nz", a), ("b.nz", b), ("main.nz", v1)]);
    let (pa, pb, main) = (p.path("a.nz"), p.path("b.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, v1.into());
        s.open(&pa, 1, a.into());
        s.open(&pb, 1, b.into());
        let decl = at(a, "pick", 0, 0);
        let call = want("main.nz", at(v1, "pick", 0, 0), "pick");
        assert_eq!(spots(&refs(&s, &pa, 1, decl).uses), vec![call.clone()]);
        assert!(refs(&s, &pb, 1, decl).uses.is_empty());

        s.change(&main, 2, v2.into());
        assert!(
            refs(&s, &pa, 1, decl).uses.is_empty(),
            "the call no longer refers to `a.nz`'s `pick`"
        );
        assert_eq!(spots(&refs(&s, &pb, 1, decl).uses), vec![call]);
        let from_call = refs(&s, &main, 2, at(v2, "pick", 0, 0));
        assert_eq!(
            from_call.declaration.unwrap().path.file_name().unwrap(),
            "b.nz"
        );
    });
}

#[test]
fn a_broken_buffer_never_answers_from_the_clean_one_before_it() {
    let lib = "pub fn helper(n: Int) -> Int { n + 1 }\n";
    let good = "use \"lib.nz\";\nfn main() -> Int { helper(1) }\nfn other() -> Int { helper(2) }\n";
    let broken = "use \"lib.nz\";\nfn main() -> Int { let x = ; helper(1) }\nfn other() -> Int { helper(2) }\n";
    let p = Project::new(&[("lib.nz", lib)]);
    let (lib_path, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, good.into());
        s.open(&lib_path, 1, lib.into());
        assert_eq!(refs(&s, &lib_path, 1, 7).uses.len(), 2);
        s.change(&main, 2, broken.into());
        // The body that did not parse has no resolution, so its call is no reference;
        // the healthy function's still is. Nothing is served from version 1.
        assert_eq!(
            spots(&refs(&s, &lib_path, 1, 7).uses),
            vec![want("main.nz", at(broken, "helper(2)", 0, 0), "helper")]
        );
        assert!(s
            .references(&main, 2, at(broken, "helper(1)", 0, 0))
            .is_none());
        assert!(s
            .references(&main, 1, at(good, "helper(1)", 0, 0))
            .is_none());
        assert!(s
            .references(&main, 2, at(broken, "helper(2)", 0, 0))
            .is_some());
    });
}

#[test]
fn a_use_in_a_dependency_nobody_opened_is_reported_in_that_dependency() {
    // Only `main.nz` is open, so the one compilation there is has `main.nz` as its root and
    // `lib.nz` as a dependency: `twice`'s calls of `helper` are found there, and are in
    // `lib.nz`, not in the document the question came from.
    let p = adversarial();
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, MAIN.into());
        let r = refs(&s, &main, 1, at(MAIN, "helper(1)", 0, 0));
        assert_eq!(
            spots(&r.uses),
            vec![
                want("lib.nz", at(LIB, "helper(helper", 0, 0), "helper"),
                want("lib.nz", at(LIB, "helper(n))", 0, 0), "helper"),
                want("main.nz", at(MAIN, "helper(1)", 0, 0), "helper"),
            ]
        );
    });
}

#[test]
fn only_programs_being_analysed_are_searched() {
    // `stray.nz` calls `helper` and sits beside the others on disk, but no open
    // document's compilation loads it: it is in no program being analysed.
    let lib = "pub fn helper(n: Int) -> Int { n + 1 }\n";
    let main = "use \"lib.nz\";\nfn main() -> Int { helper(1) }\n";
    let stray = "use \"lib.nz\";\nfn main() -> Int { helper(9) }\n";
    let p = Project::new(&[("lib.nz", lib), ("main.nz", main), ("stray.nz", stray)]);
    let (lib_path, main_path, stray_path) =
        (p.path("lib.nz"), p.path("main.nz"), p.path("stray.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&lib_path, 1, lib.into());
        assert!(
            refs(&s, &lib_path, 1, 7).uses.is_empty(),
            "nothing open imports `lib.nz`"
        );
        s.open(&main_path, 1, main.into());
        assert_eq!(refs(&s, &lib_path, 1, 7).uses.len(), 1);
        s.open(&stray_path, 1, stray.into());
        assert_eq!(
            refs(&s, &lib_path, 1, 7).uses.len(),
            2,
            "once it is open, it is a program being analysed"
        );
        s.close(&stray_path);
        assert_eq!(refs(&s, &lib_path, 1, 7).uses.len(), 1);
    });
}

// ---------------------------------------------------------------------------------------
// Long-lived means bounded.
// ---------------------------------------------------------------------------------------

#[test]
fn a_thousand_edits_that_add_and_remove_uses_leave_one_current_index() {
    let lib = "pub fn helper(n: Int) -> Int { n + 1 }\n";
    let one = "use \"lib.nz\";\nfn main() -> Int { helper(1) }\n";
    let three = "use \"lib.nz\";\nfn main() -> Int { helper(1) + helper(2) + helper(3) }\n";
    let p = Project::new(&[("lib.nz", lib)]);
    let (lib_path, main) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&lib_path, 0, lib.into());
        s.open(&main, 0, one.into());
        let mut after_warmup = ((0, 0, 0), (0, 0));
        for version in 1..=1000 {
            let text = if version % 2 == 0 { one } else { three };
            s.change(&main, version, text.into());
            let n = refs(&s, &lib_path, 0, 7).uses.len();
            assert_eq!(n, if version % 2 == 0 { 1 } else { 3 }, "version {version}");
            if version == 10 {
                after_warmup = (s.retained(), s.indexed());
            }
        }
        // Two documents, two analyses, two indexes, over the current text only: the same
        // after a thousand edits as after ten.
        assert_eq!((s.retained(), s.indexed()), after_warmup);
        assert_eq!(s.retained().1, 2);
        assert!(s.references(&main, 999, at(one, "helper", 0, 0)).is_none());
        assert_eq!(refs(&s, &main, 1000, at(one, "helper", 0, 0)).uses.len(), 1);
    });
}

// ---------------------------------------------------------------------------------------
// A named function written as a value (N50) is a use of it (R1-C1).
// ---------------------------------------------------------------------------------------

const VALUE_LIB: &str = "pub fn triple(x: Int) -> Int ! {} { x * 3 }\n";
const VALUE_RE: &str = "pub use \"lib.nz\" { triple as thrice };\n";
const VALUE_MAIN: &str = "use \"lib.nz\";\n\
use \"re.nz\";\n\
struct Holder { f: fn(Int) -> Int, }\n\
fn double(x: Int) -> Int { x * 2 }\n\
fn pick() -> fn(Int) -> Int { double }\n\
fn apply[T](f: fn(T) -> T, x: T) -> T { f(x) }\n\
fn main() -> Int {\n    \
    let h = Holder(f: double);\n    \
    let hf = h.f;\n    \
    let g = pick();\n    \
    let d = double;\n    \
    apply(double, 1) + hf(2) + g(3) + d(4) + double(5) + apply(triple, 6) + apply(thrice, 7)\n\
}\n";

#[test]
fn a_function_written_as_a_value_is_a_use_of_it_in_every_position_the_language_allows() {
    use nazm_core::sema::{ReferenceIndex, ReferenceTarget};
    let p = Project::new(&[
        ("lib.nz", VALUE_LIB),
        ("re.nz", VALUE_RE),
        ("main.nz", VALUE_MAIN),
    ]);
    let (main, lib) = (p.path("main.nz"), p.path("lib.nz"));
    on_the_service_stack(move || {
        // The index, as every consumer reads it.
        let decided = nazm_service::root::decide(&main, None).unwrap();
        let loaded = nazm_service::load::load(&main, decided.path()).unwrap();
        let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
        assert!(a.diagnostics.is_empty(), "{:#?}", a.diagnostics);
        let index = ReferenceIndex::of(&a.resolution);
        let main_file = loaded
            .map
            .ids()
            .find(|f| loaded.map.name(*f).ends_with("main.nz"))
            .unwrap();
        let named = |name: &str| -> Vec<u32> {
            VALUE_MAIN
                .match_indices(name)
                .map(|(i, _)| u32::try_from(i).unwrap())
                .collect()
        };
        let double = named("double");
        let declared = nazm_span::Span::new(main_file, double[0], double[0] + 6);
        let Some(entity @ ReferenceTarget::Function(_)) = index.target(declared) else {
            panic!("`double` is not declared as a function");
        };
        let uses: Vec<u32> = index.uses(entity).iter().map(|u| u.start).collect();
        // Returned from `pick`, stored in a field, bound by `let`, passed to a generic
        // function, and called: every one, exactly at the name, each once.
        assert_eq!(uses, double[1..], "{uses:?}");
        for u in index.uses(entity) {
            assert_eq!(
                u.end - u.start,
                6,
                "the name's span, not the expression's: {u:?}"
            );
            assert_eq!(index.target(*u), Some(entity));
        }
        for w in index.uses(entity).windows(2) {
            assert!(w[0] < w[1], "ordered, and no occurrence twice");
        }

        // Through the service: from a value use, the same set and the declaration; and an
        // imported function written as a value, under its own name and under an alias a
        // re-export gave it, is a use of the definition in the module that declares it.
        let mut s = LanguageService::new();
        for (path, text) in [(&main, VALUE_MAIN), (&lib, VALUE_LIB)] {
            for published in s.open(path, 1, text.into()) {
                assert!(published.diagnostics.is_empty(), "{published:#?}");
            }
        }
        let r = refs(&s, &main, 1, double[2] + 1);
        assert_eq!(r.kind, "function");
        assert_eq!(
            r.declaration.as_ref().map(spot),
            Some(want("main.nz", double[0], "double"))
        );
        assert_eq!(
            spots(&r.uses),
            double[1..]
                .iter()
                .map(|at| want("main.nz", *at, "double"))
                .collect::<Vec<_>>()
        );
        let triple = named("triple")[0];
        let thrice = named("thrice")[0];
        let r = refs(&s, &main, 1, triple);
        assert_eq!(
            r.declaration.as_ref().map(spot),
            Some(want("lib.nz", 7, "triple"))
        );
        let got = spots(&r.uses);
        assert!(got.contains(&want("main.nz", triple, "triple")), "{got:?}");
        assert!(got.contains(&want("main.nz", thrice, "thrice")), "{got:?}");
    });
}

// ---------------------------------------------------------------------------------------
// The repository's own source: the index against the resolution it was read from.
// ---------------------------------------------------------------------------------------

/// Every compilation root the audit reads: the compiler written in Nazm (its emitter
/// imports every other module of it), each multi-module conformance program, and each
/// example. The core prelude is in every one of them.
fn corpus() -> Vec<PathBuf> {
    let root = workspace();
    let mut roots = vec![root.join("compiler/emit.nz")];
    for e in walk(&root.join("compiler/conformance/modules")) {
        if e.file_name().is_some_and(|n| n == "main.nz") {
            roots.push(e);
        }
    }
    roots.extend(walk(&root.join("examples")));
    roots
}

/// What the resolution itself answers at `span`, asked the way `nazm-service` asked it
/// before N17 — one accessor at a time — and reduced to the declaration it names.
fn resolved_declaration(
    res: &nazm_core::Resolution,
    span: nazm_span::Span,
) -> Option<(&'static str, nazm_span::Span)> {
    use nazm_core::sema::Callee;
    if let Some(Callee::Function(id)) = res.callee(span) {
        return Some(("function", res.function(id)?.name_span));
    }
    // A named function written as a value, `apply_twice(double, 5)` (N50): a use of it, as
    // a call is (R1-C1).
    if let Some(v) = res.fn_value(span) {
        return Some(("function", res.function(v.function)?.name_span));
    }
    if let Some(l) = res.local(span) {
        // A closure's capture is a slot of the closure that holds a copy at run time, not a
        // binding the source declares (N50): the name is the binding it copies.
        let l = res.binding_of(l);
        let f = res.function(l.function)?;
        return Some(("local", f.locals.get(l.slot as usize)?.span));
    }
    if let Some(r) = res.construction(span) {
        return Some(("record", res.records().get(r)?.name_span));
    }
    if let Some(v) = res.variant(span) {
        let e = res.enums().get(v.enumeration)?;
        return Some(("variant", e.variants.get(v.variant as usize)?.name_span));
    }
    if let Some(f) = res.field(span) {
        let r = res.records().get(f.record)?;
        return Some(("field", r.fields.get(f.field as usize)?.name_span));
    }
    if let Some(p) = res.payload(span) {
        let e = res.enums().get(p.enumeration)?;
        let v = e.variants.get(p.variant as usize)?;
        return Some(("payload field", v.fields.get(p.field as usize)?.name_span));
    }
    // N78: a trait named in a bound, an impl or a call, by the trait it names.
    if let Some((_, t)) = res.traits().mentions().find(|(s, _)| *s == span) {
        return Some(("trait", res.traits().get(t)?.name_span));
    }
    // N78: a method called through a bound, `shape.area()` with `shape: T` and `T: Area`, is a
    // use of the trait's method — not a field projection, and not any one impl's function.
    if let Some((_, c)) = res.traits().trait_calls().find(|(s, _)| *s == span) {
        let t = res.traits().get(c.trait_id)?;
        return Some(("trait method", t.methods.get(c.method as usize)?.name_span));
    }
    // N18: a written type name, by the definition the checker resolved it to.
    match res.type_name(span)? {
        nazm_core::sema::TypeNode::Record(id) => Some(("record", res.records().get(id)?.name_span)),
        nazm_core::sema::TypeNode::Enum(id) => Some(("enum", res.enums().get(id)?.name_span)),
    }
}

#[test]
fn every_resolved_occurrence_is_in_exactly_one_entitys_set() {
    use nazm_core::sema::ReferenceIndex;
    let roots = corpus();
    assert!(roots.len() >= 15, "{roots:?}");
    on_the_service_stack(move || {
        let (mut occurrences, mut targets, mut uses, mut cross) = (0, 0, 0, 0);
        for path in &roots {
            let decided = nazm_service::root::decide(path, None).unwrap();
            let Ok(loaded) = nazm_service::load::load(path, decided.path()) else {
                continue;
            };
            let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
            let res = &a.resolution;
            let index = ReferenceIndex::of(res);

            // The sets partition the occurrences: every use and declaration listed is of
            // the entity it is listed under, and together they are all of them, once.
            let mut listed = 0;
            for (t, us) in index.iter() {
                if let Some(d) = index.declaration(t) {
                    assert_eq!(index.target(d), Some(t), "{}", path.display());
                    listed += 1;
                }
                for w in us.windows(2) {
                    assert!(w[0] < w[1], "ordered, and no occurrence twice");
                }
                for u in us {
                    assert_eq!(index.target(*u), Some(t), "{}", path.display());
                    assert_ne!(index.declaration(t), Some(*u));
                    if index.declaration(t).is_some_and(|d| d.file != u.file) {
                        cross += 1;
                    }
                }
                listed += us.len();
            }
            assert_eq!(listed, index.occurrences(), "{}", path.display());

            // And each is where the resolution put it: for every name token of every file,
            // the resolution's own answer and the index's agree, and a token the
            // resolution answered nothing for is in no set.
            for f in loaded.map.ids() {
                let cst = nazm_core::__syntax::parse_cst(f, loaded.map.text(f)).cst;
                for leaf in cst.leaves() {
                    let span = leaf.span;
                    let from_index = index.target(span).map(|t| (t.kind(), index.declaration(t)));
                    match resolved_declaration(res, span) {
                        Some((kind, declared)) => {
                            assert_eq!(
                                from_index,
                                Some((kind, Some(declared))),
                                "{}: {span:?}",
                                path.display()
                            );
                        }
                        // Not a use the resolution recorded: in the index only as a
                        // declaration, and then as its own.
                        None => {
                            if let Some((_, d)) = from_index {
                                assert_eq!(d, Some(span), "{}: {span:?}", path.display());
                            }
                        }
                    }
                }
            }
            occurrences += index.occurrences();
            targets += index.targets();
            uses += index.use_count();
        }
        // Descriptive, not a proof: recorded in `docs/performance.md`.
        eprintln!(
            "corpus: {} compilations, {occurrences} occurrences, {targets} entities, {uses} uses, {cross} cross-file uses",
            roots.len()
        );
        assert!(cross > 0 && uses > targets);
    });
}

#[test]
fn the_services_references_are_the_command_lines_resolution_read_backwards() {
    // For every root of the corpus opened alone, each name token's references in the
    // service are exactly the index of the compilation the command-line loader and the
    // same `analyse` produce — the same entity, the same declaration, the same uses.
    use nazm_core::sema::ReferenceIndex;
    let roots = corpus();
    on_the_service_stack(move || {
        for path in roots {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let decided = nazm_service::root::decide(&path, None).unwrap();
            let Ok(loaded) = nazm_service::load::load(&path, decided.path()) else {
                continue;
            };
            let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
            let index = ReferenceIndex::of(&a.resolution);
            let place = |span: nazm_span::Span| {
                (
                    Path::new(loaded.map.name(span.file)).canonicalize().ok(),
                    span.start,
                    span.end,
                )
            };
            let mut s = LanguageService::new();
            s.open(&path, 1, text.clone());
            let cst = nazm_core::__syntax::parse_cst(nazm_span::FileId::ROOT, &text).cst;
            // Every name of every small root; one name in a hundred of the compiler's, whose
            // twenty-five thousand would each be a whole query in a debug build.
            let stride = if text.len() > 50_000 { 100 } else { 1 };
            let names = cst
                .leaves()
                .into_iter()
                .filter(|l| l.kind == nazm_core::__syntax::SyntaxKind::Ident)
                .step_by(stride);
            for leaf in names {
                let service = s.references(&path, 1, leaf.span.start);
                let direct = index.target(leaf.span);
                let Some(t) = direct else {
                    assert!(service.is_none(), "{}: {:?}", path.display(), leaf.span);
                    continue;
                };
                let service = service.unwrap();
                assert_eq!(service.kind, t.kind());
                let got: Vec<_> = service
                    .uses
                    .iter()
                    .map(|l| (l.path.canonicalize().ok(), l.start, l.end))
                    .collect();
                let prelude = loaded.graph.prelude();
                let want: Vec<_> = index
                    .uses(t)
                    .iter()
                    // The toolchain's modules — the prelude, and the standard library's
                    // (N45) — have no file to send a client to.
                    .filter(|u| {
                        let module = loaded.graph.module_of(u.file);
                        module != prelude
                            && !module
                                .and_then(|m| loaded.graph.key(m))
                                .is_some_and(nazm_core::sema::ModuleKey::is_std)
                    })
                    .map(|u| place(*u))
                    .collect();
                let (mut got_sorted, mut want_sorted) = (got.clone(), want.clone());
                got_sorted.sort();
                want_sorted.sort();
                assert_eq!(
                    got_sorted,
                    want_sorted,
                    "{}: {:?}",
                    path.display(),
                    leaf.span
                );
            }
        }
    });
}

#[test]
fn every_name_token_of_the_corpus_is_indexed_or_is_not_an_entity() {
    // The coverage gate (N18). A rename is complete only if every place a program's own
    // entity can be written is recorded. So for every clean compilation of the repository's
    // source, every identifier token must be in the index — or be one of a short list of
    // things that are not a program's entity, each recognised by its syntactic position
    // and not by guessing what it means. A new syntax position that names an entity and
    // that the checker does not record fails here, before any rename could miss it.
    use nazm_core::__syntax::SyntaxKind as K;
    use nazm_core::sema::{Callee, ReferenceIndex};
    let roots = corpus();
    on_the_service_stack(move || {
        let mut unexplained = Vec::new();
        let mut checked = 0;
        for path in &roots {
            let decided = nazm_service::root::decide(path, None).unwrap();
            let Ok(loaded) = nazm_service::load::load(path, decided.path()) else {
                continue;
            };
            let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
            if !a.diagnostics.is_empty() {
                continue; // `examples/bad`: a rename is never offered on an unclean program
            }
            let index = ReferenceIndex::of(&a.resolution);
            for f in loaded.map.ids() {
                let cst = nazm_core::__syntax::parse_cst(f, loaded.map.text(f)).cst;
                // Each item's own type parameters, by the item's span: a name written in a
                // type position inside it may be one of them.
                let mut params: Vec<(nazm_span::Span, Vec<String>)> = Vec::new();
                for item in cst.nodes() {
                    if matches!(item.kind(), K::FnItem | K::RecordItem | K::EnumItem) {
                        let names = item
                            .descendants()
                            .into_iter()
                            .filter(|n| n.kind() == K::TypeParam)
                            .filter_map(|n| {
                                // `effects E` (N51): the parameter is the name after the word.
                                let idents: Vec<_> = n
                                    .child_leaves()
                                    .into_iter()
                                    .filter(|l| l.kind == K::Ident)
                                    .collect();
                                match idents.as_slice() {
                                    [w, e, ..] if w.text == "effects" => Some(e.text.to_owned()),
                                    [first, ..] => Some(first.text.to_owned()),
                                    [] => None,
                                }
                            })
                            .collect();
                        params.push((item.span(), names));
                    }
                }
                // Each trait's span (N78): its methods are signatures with no body, so their
                // parameters name nothing a program can use, and `Self` is the trait's own type
                // parameter.
                let traits: Vec<nazm_span::Span> = cst
                    .nodes()
                    .into_iter()
                    .filter(|n| n.kind() == K::TraitItem)
                    .map(|n| n.span())
                    .collect();
                let in_a_trait = |span: nazm_span::Span| {
                    traits
                        .iter()
                        .any(|s| s.start <= span.start && span.end <= s.end)
                };
                let a_type_parameter = |span: nazm_span::Span, text: &str| {
                    params.iter().any(|(s, names)| {
                        s.start <= span.start
                            && span.end <= s.end
                            && names.iter().any(|n| n == text)
                    })
                };
                for node in cst.nodes() {
                    for leaf in node.child_leaves() {
                        if leaf.kind != K::Ident {
                            continue;
                        }
                        checked += 1;
                        let span = leaf.span;
                        let explained = index.target(span).is_some()
                            || node.kind() == K::TypeParam
                            || (node.kind() == K::Type
                                && (nazm_core::Type::from_name(leaf.text).is_some()
                                    || leaf.text == "Vec"
                                    || a_type_parameter(span, leaf.text)))
                            || (node.kind() == K::PatternField && leaf.text == "_")
                            // A contract's `requires` and `ensures` are its syntax (N87).
                            || (node.kind() == K::Contract
                                && (leaf.text == "requires" || leaf.text == "ensures"))
                            // An effect name is the compiler's, never a program's entity (N36);
                            // an effect parameter is its function's, as a type parameter is (N51).
                            || (node.kind() == K::EffectSet
                                && (nazm_core::sema::Effect::by_name(leaf.text).is_some()
                                    || a_type_parameter(span, leaf.text)))
                            || matches!(a.resolution.callee(span), Some(Callee::Builtin(_)))
                            // `impl Trait for Type`: `for` is its syntax (N78).
                            || (node.kind() == K::ImplItem && leaf.text == "for")
                            // A trait method's parameter, and `Self`, inside a trait (N78).
                            || (node.kind() == K::Param && in_a_trait(span))
                            || (node.kind() == K::Type && leaf.text == "Self" && in_a_trait(span));
                        if !explained {
                            unexplained.push(format!(
                                "{}:{} {:?} `{}`",
                                loaded.map.name(f),
                                span.start,
                                node.kind(),
                                leaf.text
                            ));
                        }
                    }
                }
            }
        }
        assert!(checked > 20_000, "{checked}");
        assert!(
            unexplained.is_empty(),
            "{} name tokens are neither indexed nor a non-entity: {:#?}",
            unexplained.len(),
            &unexplained[..unexplained.len().min(20)]
        );
    });
}
