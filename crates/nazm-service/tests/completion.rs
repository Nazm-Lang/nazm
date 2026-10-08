//! Scope at a position, and completion from it, through the language service. N19.
//!
//! Every expected candidate set is written out, and every candidate is identified by where
//! it is declared — not only by its name — so that a completion grouping by spelling, or
//! offering a shadowed binding beside the one that hides it, fails.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use nazm_core::sema::{Candidate, Intrinsic, Type};
use nazm_service::{Completion, Context, LanguageService, NoCompletion};

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

/// A candidate as the tests compare it: its name, its kind, and the file and byte its
/// declaration is at — `None` for a built-in or a type parameter.
type Id = (String, &'static str, Option<(String, u32)>);

fn ids(c: &Completion) -> Vec<Id> {
    c.items
        .iter()
        .map(|i| {
            (
                i.name.clone(),
                i.kind,
                i.declaration.as_ref().map(|l| {
                    (
                        l.path.file_name().unwrap().to_string_lossy().into_owned(),
                        l.start,
                    )
                }),
            )
        })
        .collect()
}

fn id(name: &str, kind: &'static str, file: &str, at: u32) -> Id {
    (name.to_owned(), kind, Some((file.to_owned(), at)))
}

/// Only the program's own candidates: built-ins are a fixed inventory, tested apart.
fn own(c: &Completion) -> Vec<Id> {
    ids(c).into_iter().filter(|(_, _, d)| d.is_some()).collect()
}

fn complete(s: &LanguageService, path: &Path, version: i32, offset: u32) -> Completion {
    s.completion(path, version, offset)
        .unwrap_or_else(|e| panic!("no completion at {offset}: {e:?}"))
}

// ---------------------------------------------------------------------------------------
// One spelling, many meanings.
// ---------------------------------------------------------------------------------------

const LIB: &str =
    "pub fn libfn() -> Int { 1 }\nfn hidden() -> Int { 2 }\npub struct Thing { v: Int, }\n";
const MAIN: &str = "use \"lib.nz\";\n\
struct Box { v: Int, }\n\
fn Box() -> Int { 3 }\n\
fn f[T](t: T) -> T { t }\n\
fn h(x: Box) -> Box { x }\n\
fn g(Box: Int) -> Int {\n    \
    let a = Box;\n    \
    let b = { let Box = a + 1; Box };\n    \
    a + b + Box() + h(Box(v: 1)).v + libfn()\n\
}\n\
fn main() -> Int { g(1) + f[Int](2) }\n";

#[test]
fn completion_follows_semantic_scope_not_spelling() {
    // `Box` is a record, a function, a parameter and an inner `let`. Which one a
    // completion offers is decided by the context and the scope, never by the spelling.
    let p = Project::new(&[("lib.nz", LIB), ("main.nz", MAIN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&main, 1, MAIN.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let m = |needle: &str, n: usize, into: usize| at(MAIN, needle, n, into);
        let record = id("Box", "record", "main.nz", m("struct Box", 0, 7));
        let function = id("Box", "function", "main.nz", m("fn Box", 0, 3));
        let param = id("Box", "local", "main.nz", m("g(Box", 0, 2));
        let inner = id("Box", "local", "main.nz", m("let Box", 0, 4));

        // A value: the parameter, and only it — not the record, not the function.
        // The cursor at the end of the name: what is written of it is the whole name.
        let c = complete(&s, &main, 1, m("= Box;", 0, 5));
        assert_eq!(c.context, Context::Value);
        assert_eq!(ids(&c), std::slice::from_ref(&param));
        // Inside the block, after the inner `let`: the inner binding, which hides the
        // parameter — one `Box`, not two.
        let c = complete(&s, &main, 1, m("Box };", 0, 3));
        assert_eq!(ids(&c), [inner]);
        // After the block the parameter is visible again, and the inner binding is not.
        let everything: Vec<Id> = own(&complete(&s, &main, 1, m("a + b", 0, 0)));
        assert_eq!(
            everything,
            [
                id("Box", "local", "main.nz", m("g(Box", 0, 2)),
                id("a", "local", "main.nz", m("let a", 0, 4)),
                id("b", "local", "main.nz", m("let b", 0, 4)),
            ],
            "a value position after the block: the parameter, `a`, `b`; not the inner `Box`"
        );
        // A call head: the function `Box`, and imported and local functions — never a
        // local, never a type.
        let c = complete(&s, &main, 1, m("Box() +", 0, 3));
        assert_eq!(c.context, Context::Call);
        assert_eq!(ids(&c), [function]);
        let calls = own(&complete(&s, &main, 1, m("libfn()", 0, 0)));
        assert_eq!(
            calls,
            [
                id("Box", "function", "main.nz", m("fn Box", 0, 3)),
                id("f", "function", "main.nz", m("fn f", 0, 3)),
                id("g", "function", "main.nz", m("fn g", 0, 3)),
                id("h", "function", "main.nz", m("fn h", 0, 3)),
                id("libfn", "function", "lib.nz", at(LIB, "libfn", 0, 0)),
                id("main", "function", "main.nz", m("fn main", 0, 3)),
            ],
            "every function this module may call — not `lib.nz`'s private `hidden`"
        );
        // A type: the record, and the imported record — never the function.
        let c = complete(&s, &main, 1, m("x: Box", 0, 6));
        assert_eq!(c.context, Context::Type);
        assert_eq!(ids(&c), std::slice::from_ref(&record));
        let types = own(&complete(&s, &main, 1, m("-> Box {", 0, 3)));
        assert_eq!(
            types,
            [
                record,
                id("Thing", "record", "lib.nz", at(LIB, "struct Thing", 0, 7))
            ]
        );
        assert_eq!(everything[0], param);
    });
}

// ---------------------------------------------------------------------------------------
// When a binding is visible.
// ---------------------------------------------------------------------------------------

const LOCALS: &str = "fn f(x: Int) -> Int {\n    \
    let early = 1;\n    \
    let x = x + early;\n    \
    let late = x;\n    \
    while late < 0 { let inside = late; }\n    \
    let e = if true { let t = 1; t } else { let u = 2; u };\n    \
    scope { let scoped = e; }\n    \
    late + e\n\
}\nfn main() -> Int { f(1) }\n";

/// The names of the locals visible at `offset`, from the scope query alone.
fn locals(s: &LanguageService, path: &Path, offset: u32) -> Vec<String> {
    s.scope_at(path, 1, offset)
        .unwrap()
        .values
        .iter()
        .map(|n| n.name.clone())
        .collect()
}

#[test]
fn a_binding_is_visible_from_where_the_checker_introduces_it_and_no_earlier() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, LOCALS.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let l = |needle: &str, n: usize, into: usize| at(LOCALS, needle, n, into);
        let names = |o: u32| -> Vec<String> { locals(&s, &m, o) };
        // A later local is not visible early; the parameter is.
        assert_eq!(names(l("let early", 0, 0)), ["x"]);
        // In `let x = x + early`, the right-hand `x` is the parameter: the new `x` is not
        // visible in its own initializer.
        let rhs = complete(&s, &m, 1, l("= x +", 0, 3));
        assert_eq!(
            ids(&rhs),
            [id("x", "local", "m.nz", l("f(x", 0, 2))],
            "the parameter, not the `let` being written"
        );
        // After it, the new `x` hides the parameter: one `x`.
        let after = complete(&s, &m, 1, l("= x;", 0, 3));
        assert_eq!(ids(&after), [id("x", "local", "m.nz", l("let x", 0, 4))]);
        // A loop body's binding is visible in the body only.
        assert_eq!(names(l("late; }", 0, 0)), ["early", "late", "x"]);
        assert!(!names(l("let e", 0, 0)).contains(&"inside".to_owned()));
        // Sibling branches do not see each other's bindings.
        assert_eq!(names(l("t }", 0, 0)), ["early", "late", "t", "x"]);
        assert_eq!(names(l("u }", 0, 0)), ["early", "late", "u", "x"]);
        // A `scope { … }` binding stays inside it; after the statement, `e` is visible.
        assert_eq!(names(l("late + e", 0, 0)), ["e", "early", "late", "x"]);
        // Away from any identifier, the scope query still answers.
        let blank = l("    late + e", 0, 1);
        assert_eq!(names(blank), ["e", "early", "late", "x"]);
        // Outside every function: no locals at all.
        assert!(names(0).is_empty());
    });
}

#[test]
fn a_match_arms_bindings_stay_in_their_arm() {
    const T: &str = "enum E { A(v: Int), B(v: Int), }\n\
fn f(e: E) -> Int {\n    \
    let r = match e { E.A(v: v) => v, E.B(v: v) => v + 1, };\n    \
    r\n\
}\nfn main() -> Int { f(E.A(v: 1)) }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, T.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let first = complete(&s, &m, 1, at(T, "=> v,", 0, 4));
        assert_eq!(
            ids(&first),
            [id("v", "local", "m.nz", at(T, "(v: v)", 0, 4))]
        );
        let second = complete(&s, &m, 1, at(T, "=> v +", 0, 4));
        assert_eq!(
            ids(&second),
            [id("v", "local", "m.nz", at(T, "(v: v)", 1, 4))],
            "the second arm's own `v`, not the first's"
        );
        let after = complete(&s, &m, 1, at(T, "    r\n", 0, 4));
        assert_eq!(
            own(&after),
            [
                id("e", "local", "m.nz", at(T, "f(e", 0, 2)),
                id("r", "local", "m.nz", at(T, "let r", 0, 4)),
            ],
            "neither arm's `v` after the match"
        );
    });
}

#[test]
fn a_refused_duplicate_binding_names_nothing() {
    // Two `let x` in one scope is refused (`N0309`), and the checker's later lookups find
    // the second. Completion does not pick a winner: neither is offered.
    const T: &str = "fn main() -> Int { let w = 0; let x = 1; let x = 2; x + w }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, T.into());
        assert!(!out[0].diagnostics.is_empty(), "the duplicate is reported");
        let c = complete(&s, &m, 1, at(T, "x + w", 0, 0));
        assert_eq!(own(&c), [id("w", "local", "m.nz", at(T, "let w", 0, 4))]);
    });
}

#[test]
fn a_later_top_level_function_is_callable_and_a_later_local_is_not_visible() {
    const T: &str = "fn main() -> Int { let a = later(); a }\nfn later() -> Int { 1 }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, T.into());
        let c = complete(&s, &m, 1, at(T, "later()", 0, 0));
        assert_eq!(
            own(&c),
            [
                id("later", "function", "m.nz", at(T, "fn later", 0, 3)),
                id("main", "function", "m.nz", at(T, "fn main", 0, 3)),
            ],
            "top-level order does not matter: declarations are collected before bodies"
        );
        assert!(
            s.scope_at(&m, 1, at(T, "later()", 0, 0))
                .unwrap()
                .values
                .is_empty(),
            "`a` is not visible in its own initializer"
        );
    });
}

// ---------------------------------------------------------------------------------------
// Type parameters, built-ins.
// ---------------------------------------------------------------------------------------

#[test]
fn a_type_parameter_is_visible_in_its_own_definition_only() {
    const T: &str = "struct Box[T] { value: T, }\n\
enum Maybe[U] { Some(value: U), None, }\n\
fn wrap[T](t: T) -> Box[T] { Box[T](value: t) }\n\
fn plain(x: Int) -> Int { x }\n\
fn main() -> Int { plain(wrap[Int](1).value) }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&m, 1, T.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let params = |o: u32| -> Vec<String> {
            complete(&s, &m, 1, o)
                .items
                .iter()
                .filter(|i| i.kind == "type parameter")
                .map(|i| i.name.clone())
                .collect()
        };
        assert_eq!(
            params(at(T, "value: T", 0, 7)),
            ["T"],
            "the record's own `T`"
        );
        assert_eq!(
            params(at(T, "value: U", 0, 7)),
            ["U"],
            "the enum's `U`, not the record's `T`"
        );
        assert_eq!(params(at(T, "t: T", 0, 3)), ["T"], "the function's own `T`");
        assert!(
            params(at(T, "x: Int", 0, 3)).is_empty(),
            "no `T` leaks into `plain`"
        );
    });
}

#[test]
fn built_ins_come_from_the_compilers_own_inventories() {
    const T: &str = "fn main() -> Int { str_len(\"a\") }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, T.into());
        // Every built-in function, from `Intrinsic::ALL`, in a call head.
        let calls = complete(&s, &m, 1, at(T, "str_len", 0, 0));
        let mut expected: Vec<Candidate> = Intrinsic::ALL
            .iter()
            .map(|i| Candidate::Intrinsic(*i))
            .collect();
        expected.sort();
        let mut got: Vec<Candidate> = calls
            .items
            .iter()
            .map(|i| i.candidate)
            .filter(|c| matches!(c, Candidate::Intrinsic(_)))
            .collect();
        got.sort();
        assert_eq!(got, expected);
        // Every built-in type, from `Type::ALL`, and `Vec`, in a type position.
        let types = complete(&s, &m, 1, at(T, "Int", 0, 0));
        let mut got: Vec<String> = types
            .items
            .iter()
            .filter(|i| i.kind == "built-in type")
            .map(|i| i.name.clone())
            .collect();
        got.sort();
        let mut expected: Vec<String> = Type::ALL
            .iter()
            .filter_map(|t| t.builtin_name())
            .map(str::to_owned)
            .chain(["Vec".to_owned()])
            .collect();
        expected.sort();
        assert_eq!(got, expected);
        // Every capability type among them, read from the capability kinds themselves: until
        // Gate 1-C1 completion offered `IoCap` and `SpawnCap` and none of the five later ones.
        for k in nazm_core::sema::CapabilityKind::ALL {
            assert!(
                got.iter().any(|n| n == k.type_name()),
                "completion does not offer {}: {got:?}",
                k.type_name()
            );
        }
        // And a prefix filters: exactly the names that begin with what is written.
        let str_ = complete(&s, &m, 1, at(T, "str_len", 0, 4));
        assert!(!str_.items.is_empty());
        assert!(str_.items.iter().all(|i| i.name.starts_with("str_")));
        assert_eq!(
            (str_.start, str_.end),
            (at(T, "str_len", 0, 0), at(T, "str_len", 0, 7)),
            "the whole identifier is replaced, not the prefix"
        );
    });
}

#[test]
fn the_service_keeps_no_list_of_built_ins() {
    // Built-ins are read from `Intrinsic::ALL` and `Type::ALL` through the checker's
    // environments. A name spelled out here would be a second inventory.
    for source in [
        include_str!("../src/completion.rs"),
        include_str!("../src/service.rs"),
    ] {
        for name in ["\"str_len\"", "\"Int\"", "\"Bool\"", "\"print\"", "\"Vec\""] {
            assert!(!source.contains(name), "{name} is spelled in the service");
        }
    }
}

// ---------------------------------------------------------------------------------------
// Modules: direct imports only, exports only, one program only.
// ---------------------------------------------------------------------------------------

#[test]
fn only_direct_imports_exports_are_offered() {
    let c = "pub fn from_c() -> Int { 3 }\npub struct CType { v: Int, }\n";
    let b = "use \"c.nz\";\npub fn from_b() -> Int { from_c() }\nfn private_b() -> Int { 2 }\npub struct BType { v: Int, }\nstruct BHidden { v: Int, }\n";
    let a = "use \"b.nz\";\nfn main() -> Int { from_b() }\nfn own(x: Int) -> Int { x }\n";
    let p = Project::new(&[("c.nz", c), ("b.nz", b), ("a.nz", a)]);
    let pa = p.path("a.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&pa, 1, a.into());
        assert!(out[0].diagnostics.is_empty(), "{out:#?}");
        let calls = own(&complete(&s, &pa, 1, at(a, "from_b", 0, 0)));
        assert_eq!(
            calls,
            [
                id("from_b", "function", "b.nz", at(b, "fn from_b", 0, 3)),
                id("main", "function", "a.nz", at(a, "fn main", 0, 3)),
                id("own", "function", "a.nz", at(a, "fn own", 0, 3)),
            ],
            "`b.nz`'s export; not its private function, not `c.nz`'s export"
        );
        let types = own(&complete(&s, &pa, 1, at(a, "x: Int", 0, 3)));
        assert_eq!(
            types,
            [id("BType", "record", "b.nz", at(b, "struct BType", 0, 7))],
            "not `BHidden`, not `CType`"
        );
    });
}

#[test]
fn an_ambiguous_import_offers_neither_meaning() {
    let x = "pub fn pick() -> Int { 1 }\npub fn only_x() -> Int { 1 }\n";
    let y = "pub fn pick() -> Int { 2 }\n";
    let m = "use \"x.nz\";\nuse \"y.nz\";\nfn main() -> Int { only_x() }\n";
    let p = Project::new(&[("x.nz", x), ("y.nz", y), ("m.nz", m)]);
    let pm = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let out = s.open(&pm, 1, m.into());
        assert!(!out[0].diagnostics.is_empty(), "the ambiguity is reported");
        let names: Vec<String> = own(&complete(&s, &pm, 1, at(m, "only_x", 0, 0)))
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(
            names,
            ["main", "only_x"],
            "`pick` is ambiguous here, so it is not offered"
        );
    });
}

#[test]
fn another_open_program_does_not_pollute_a_completion() {
    let one = "fn helper_one() -> Int { 1 }\nfn main() -> Int { helper_one() }\n";
    let two = "fn helper_two() -> Int { 2 }\nfn main() -> Int { helper_two() }\n";
    let p = Project::new(&[("one/main.nz", one), ("two/main.nz", two)]);
    let (a, b) = (p.path("one/main.nz"), p.path("two/main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&a, 1, one.into());
        s.open(&b, 1, two.into());
        let names: Vec<String> = own(&complete(&s, &a, 1, at(one, "helper_one()", 1, 0)))
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(names, ["helper_one", "main"]);
    });
}

// ---------------------------------------------------------------------------------------
// The current, unsaved program — and only a complete scope.
// ---------------------------------------------------------------------------------------

#[test]
fn completion_is_of_the_current_unsaved_program() {
    let lib = "pub fn a() -> Int { 1 }\n";
    let v1 = "fn main() -> Int { let x = 1; x }\n";
    let p = Project::new(&[("lib.nz", lib), ("main.nz", v1)]);
    let (pl, pm) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&pm, 1, v1.into());
        let values = |s: &LanguageService, v: i32, text: &str, needle: &str| -> Vec<String> {
            s.completion(&pm, v, at(text, needle, 0, 0))
                .unwrap()
                .items
                .iter()
                .map(|i| i.name.clone())
                .collect()
        };
        assert_eq!(values(&s, 1, v1, "x }"), ["x"]);
        // An unsaved local appears.
        let v2 = "fn main() -> Int { let x = 1; let y = 2; x + y }\n";
        s.change(&pm, 2, v2.into());
        assert_eq!(values(&s, 2, v2, "x + y"), ["x", "y"]);
        assert_eq!(
            s.completion(&pm, 1, 0).unwrap_err(),
            NoCompletion::NotCurrent
        );
        // An unsaved import brings its exports in.
        let v3 = "use \"lib.nz\";\nfn main() -> Int { a() }\n";
        s.change(&pm, 3, v3.into());
        let calls: Vec<String> = own(&complete(&s, &pm, 3, at(v3, "a()", 0, 0)))
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(calls, ["a", "main"]);
        // An unsaved change to the dependency is what the importer sees; closing it gives
        // the disk back.
        s.open(
            &pl,
            1,
            "pub fn a() -> Int { 1 }\npub fn b() -> Int { 2 }\n".into(),
        );
        let calls: Vec<String> = own(&complete(&s, &pm, 3, at(v3, "a()", 0, 0)))
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(calls, ["a", "b", "main"]);
        s.close(&pl);
        let calls: Vec<String> = own(&complete(&s, &pm, 3, at(v3, "a()", 0, 0)))
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(calls, ["a", "main"]);
    });
}

#[test]
fn a_body_that_does_not_parse_has_no_completion_and_never_the_last_clean_one() {
    let clean = "fn main() -> Int { let x = 1; x }\nfn other() -> Int { let q = 2; q }\n";
    let broken =
        "fn main() -> Int { let x = 1; let y = ; x }\nfn other() -> Int { let q = 2; q }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, clean.into());
        assert_eq!(complete(&s, &m, 1, at(clean, "x }", 0, 0)).items.len(), 1);
        s.change(&m, 2, broken.into());
        assert_eq!(
            s.completion(&m, 2, at(broken, "x }", 0, 0)).unwrap_err(),
            NoCompletion::RecoveredSyntax,
            "the function is a recovery: no locals, and not version 1's"
        );
        assert_eq!(
            s.scope_at(&m, 2, at(broken, "x }", 0, 0)).unwrap_err(),
            NoCompletion::RecoveredSyntax
        );
        // The function beside it parsed, and its locals are complete...
        let q = complete(&s, &m, 2, at(broken, "q }", 0, 0));
        assert_eq!(
            q.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["q"]
        );
        // ...but the module's names are not: a recovered file may have lost an item.
        let head = "fn main() -> Int { let x = 1; let y = ; x }
fn other() -> Int { other() }
";
        s.change(&m, 20, head.into());
        assert_eq!(
            s.completion(&m, 20, at(head, "other() }", 0, 0))
                .unwrap_err(),
            NoCompletion::RecoveredSyntax
        );
        // A type error is not a parse error: the scope is still the checker's.
        let typed = "fn main() -> Int { let x = 1; let y = true + 1; x }\n";
        s.change(&m, 3, typed.into());
        let c = complete(&s, &m, 3, at(typed, "x }", 0, 0));
        assert_eq!(
            c.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["x", "y"]
        );
        s.change(&m, 4, clean.into());
        assert_eq!(complete(&s, &m, 4, at(clean, "x }", 0, 0)).items.len(), 1);
    });
}

#[test]
fn a_position_that_is_not_a_completion_site_has_no_completion() {
    const T: &str = "struct P { x: Int, }\nfn main() -> Int { let p = P(x: 1); p.x } // note\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, T.into());
        let none = |o: u32| s.completion(&m, 1, o).unwrap_err();
        // A field after `.` and a label are structure sites since N21 (`tests/structure.rs`);
        // a construction's head and a declaration's name are still not completion sites.
        assert_eq!(
            none(at(T, "P(x", 0, 0)),
            NoCompletion::NotACompletionSite,
            "a construction's head"
        );
        assert_eq!(
            none(at(T, "fn main", 0, 4)),
            NoCompletion::NotACompletionSite,
            "a declaration"
        );
        assert_eq!(
            none(at(T, "note", 0, 1)),
            NoCompletion::NotAName,
            "a comment"
        );
        assert_eq!(
            none(at(T, "= P", 0, 1)),
            NoCompletion::NotAName,
            "whitespace"
        );
    });
}

// ---------------------------------------------------------------------------------------
// Long-lived means bounded.
// ---------------------------------------------------------------------------------------

#[test]
fn a_thousand_edits_leave_one_current_scope() {
    let lib = "pub fn a() -> Int { 1 }\n";
    let with = "use \"lib.nz\";\nfn main() -> Int { let x = 1; let y = a(); x + y }\n";
    let without = "fn main() -> Int { let z = 1; z }\n";
    let p = Project::new(&[("lib.nz", lib)]);
    let m = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 0, without.into());
        let mut after_warmup = ((0, 0, 0), (0, 0));
        for version in 1..=1000 {
            let text = if version % 2 == 0 { without } else { with };
            s.change(&m, version, text.into());
            let names: Vec<String> = s
                .scope_at(
                    &m,
                    version,
                    u32::try_from(text.find("}\n").unwrap()).unwrap() - 1,
                )
                .unwrap()
                .values
                .into_iter()
                .map(|n| n.name)
                .collect();
            let expected: &[&str] = if version % 2 == 0 {
                &["z"]
            } else {
                &["x", "y"]
            };
            assert_eq!(names, expected, "version {version}");
            if version == 10 {
                after_warmup = (s.retained(), s.scoped());
            }
        }
        assert_eq!((s.retained(), s.scoped()), after_warmup);
    });
}

// ---------------------------------------------------------------------------------------
// The repository's own source.
// ---------------------------------------------------------------------------------------

#[test]
fn every_resolved_name_is_among_what_its_scope_offers() {
    // Forward consistency over real source: wherever the checker resolved a local, a call
    // or a written type, the scope at that position offers exactly that meaning under
    // that name. Paired with the exact-set tests above, which catch an extra candidate.
    use nazm_core::sema::{Callee, ReferenceTarget, TypeNode};
    let root = workspace();
    let mut roots = vec![root.join("compiler/emit.nz")];
    for e in walk(&root.join("compiler/conformance/modules")) {
        if e.file_name().is_some_and(|n| n == "main.nz") {
            roots.push(e);
        }
    }
    roots.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let mut checked = 0usize;
        for path in &roots {
            let decided = nazm_service::root::decide(path, None).unwrap();
            let Ok(loaded) = nazm_service::load::load(path, decided.path()) else {
                continue;
            };
            let a = nazm_core::analyse(&loaded.map, &loaded.graph, nazm_core::Entry::Absent);
            if !a.diagnostics.is_empty() {
                continue;
            }
            let res = &a.resolution;
            for f in loaded.map.ids() {
                let module = loaded.graph.module_of(f).unwrap();
                let cst = nazm_core::__syntax::parse_cst(f, loaded.map.text(f)).cst;
                // The last significant leaf before this one: a method's name follows a `.`,
                // and is found through its receiver's type, not the scope (N78, N102).
                let mut after_dot = false;
                for leaf in cst.leaves() {
                    if leaf.kind != nazm_core::__syntax::SyntaxKind::Ident {
                        if !leaf.text.trim().is_empty() && !leaf.text.starts_with("//") {
                            after_dot = leaf.text == ".";
                        }
                        continue;
                    }
                    let method = after_dot;
                    after_dot = false;
                    let span = leaf.span;
                    if method {
                        continue;
                    }
                    let expected = if let Some(l) = res.local(span) {
                        let declared =
                            res.function(l.function).unwrap().locals[l.slot as usize].span;
                        if declared == span {
                            continue; // a declaration, not a use
                        }
                        // A closure's copy of a captured binding means the binding it
                        // copies, as references and rename read it (N50,
                        // `Resolution::binding_of`); scope offers that binding.
                        (
                            0,
                            Candidate::Entity(ReferenceTarget::Local(res.binding_of(l))),
                        )
                    } else if let Some(c) = res.callee(span) {
                        let c = match c {
                            Callee::Function(id) => {
                                Candidate::Entity(ReferenceTarget::Function(id))
                            }
                            Callee::Builtin(i) => Candidate::Intrinsic(i),
                            // A call through a binding is the binding's use, met above (N50).
                            Callee::Indirect(_) => continue,
                        };
                        (1, c)
                    } else if let Some(node) = res.type_name(span) {
                        let c = match node {
                            TypeNode::Record(id) => ReferenceTarget::Record(id),
                            TypeNode::Enum(id) => ReferenceTarget::Enum(id),
                        };
                        (2, Candidate::Entity(c))
                    } else {
                        continue;
                    };
                    let v = res
                        .scopes()
                        .visible_at(module, f, span.start)
                        .unwrap_or_else(|e| panic!("{}:{}: {e:?}", loaded.map.name(f), span.start));
                    let pool = match expected.0 {
                        0 => &v.values,
                        1 => &v.callables,
                        _ => &v.types,
                    };
                    let offered: Vec<_> = pool.iter().filter(|n| n.name == leaf.text).collect();
                    assert!(
                        offered.len() == 1 && offered[0].candidate == expected.1,
                        "{}:{} `{}`: resolved to {:?}, offered {:?}",
                        loaded.map.name(f),
                        span.start,
                        leaf.text,
                        expected.1,
                        offered
                    );
                    checked += 1;
                }
            }
        }
        eprintln!(
            "scope audit: {checked} resolved names, each offered exactly once as what it means"
        );
        assert!(checked > 15_000, "{checked}");
    });
}
