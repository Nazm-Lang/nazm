//! Structure at a position — fields, variants and labels — and completion and constructor
//! signature help from it, through the language service. N21.
//!
//! Every candidate is identified by where it is declared, not by its name, so that two
//! records' `value`, two enums' `Ready` or two variants' payload `value` taken for one
//! another fails. Every candidate set is written out exactly.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_core::sema::{Candidate, ReferenceTarget};
use nazm_service::{
    Completion, Constructor, Context, Help, LanguageService, NoCall, NoCompletion, NoStructure,
    StructureSite,
};

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
            let p = dir.path().join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, text).unwrap();
        }
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

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

/// A candidate as the tests compare it: name, kind, detail, and the byte of `file` its
/// declaration is at.
type Id = (String, &'static str, String, Option<(String, u32)>);

fn ids(c: &Completion) -> Vec<Id> {
    c.items
        .iter()
        .map(|i| {
            (
                i.name.clone(),
                i.kind,
                i.detail.clone().unwrap_or_default(),
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

fn id(name: &str, kind: &'static str, detail: &str, file: &str, at: u32) -> Id {
    (
        name.to_owned(),
        kind,
        detail.to_owned(),
        Some((file.to_owned(), at)),
    )
}

fn complete(s: &LanguageService, path: &Path, version: i32, offset: u32) -> Completion {
    s.completion(path, version, offset)
        .unwrap_or_else(|e| panic!("no completion at {offset}: {e:?}"))
}

fn names(c: &Completion) -> Vec<&str> {
    c.items.iter().map(|i| i.name.as_str()).collect()
}

// ---------------------------------------------------------------------------------------
// One spelling, many structures.
// ---------------------------------------------------------------------------------------

const MAIN: &str = "struct A { value: Int, only_a: Int, }\n\
struct B { value: Int, only_b: Int, }\n\
enum EA { Ready, OnlyA, }\n\
enum EB { Ready, OnlyB, }\n\
enum State { Done(code: Int, value: Int), Failed(why: Str, value: Int), Idle, }\n\
enum Opt[T] { Nothing, Some(value: T), }\n\
struct Box { value: Int, }\n\
fn Box(x: Int) -> Int { x }\n\
struct Pair[F, S] { first: F, second: S, }\n\
struct Holder[T] { item: T, }\n\
fn make_a() -> A { A(value: 1, only_a: 2) }\n\
fn pick(e: EA, f: EB, o: Opt[Int]) -> Int { 0 }\n\
fn main() -> Int {\n    \
    let a = A(value: 1, only_a: 2);\n    \
    let b = B(value: 3, only_b: 4);\n    \
    let p = Pair[Int, Str](first: 1, second: \"s\");\n    \
    let h = Holder[A](item: a);\n    \
    let q = pick(EA.Ready(), EB.Ready(), Opt[Int].Some(value: 7));\n    \
    let s = State.Done(code: 1, value: 2);\n    \
    let n = Box(value: 5).value + Box(1);\n    \
    let m = match s { State.Done(code: c, value: v) => c + v, State.Failed(why: w, value: v) => v, State.Idle() => 0, };\n    \
    a.value + a.only_a + b.value + b.only_b + make_a().only_a + h.item.only_a + p.first + n + m + q\n\
}\n";

#[test]
fn structure_completion_follows_type_and_identity_not_spelling() {
    let p = Project::new(&[("main.nz", MAIN)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, MAIN.into());
        assert!(
            published[0].diagnostics.is_empty(),
            "{:?}",
            published[0].diagnostics
        );
        let field = |record: &str, name: &str| {
            at(MAIN, &format!("struct {record} "), 0, 0)
                + u32::try_from(
                    MAIN[at(MAIN, &format!("struct {record} "), 0, 0) as usize..]
                        .find(&format!("{name}:"))
                        .unwrap(),
                )
                .unwrap()
        };
        let a_fields = vec![
            id(
                "only_a",
                "field",
                "only_a: Int",
                "main.nz",
                field("A", "only_a"),
            ),
            id(
                "value",
                "field",
                "value: Int",
                "main.nz",
                field("A", "value"),
            ),
        ];
        let b_fields = vec![
            id(
                "only_b",
                "field",
                "only_b: Int",
                "main.nz",
                field("B", "only_b"),
            ),
            id(
                "value",
                "field",
                "value: Int",
                "main.nz",
                field("B", "value"),
            ),
        ];
        let tail = at(MAIN, "a.value + a.only_a", 0, 0);
        let here = |needle: &str| {
            tail + u32::try_from(MAIN[tail as usize..].find(needle).unwrap()).unwrap()
        };

        // A member is a field of the receiver's record — `A`'s for an `A`, `B`'s for a `B`,
        // though both have a `value`. Empty prefix: the cursor at the name's start.
        let c = complete(&s, &main, 1, here("value"));
        assert_eq!(c.context, Context::Member);
        assert_eq!(ids(&c), a_fields);
        assert_eq!(ids(&complete(&s, &main, 1, here("only_b"))), b_fields);
        // A temporary, and a chain through a generic record's substituted field.
        assert_eq!(
            ids(&complete(&s, &main, 1, here("make_a().only_a") + 9)),
            a_fields
        );
        assert_eq!(
            ids(&complete(&s, &main, 1, here("item.only_a") + 5)),
            a_fields
        );
        // An applied generic's fields have its substituted types.
        let pair = ids(&complete(&s, &main, 1, here("first")));
        assert_eq!(
            pair.iter()
                .map(|(n, _, d, _)| (n.as_str(), d.as_str()))
                .collect::<Vec<_>>(),
            [("first", "first: Int"), ("second", "second: Str")]
        );
        // The prefix filters, and the identifier is replaced whole.
        let c = complete(&s, &main, 1, here("only_b") + 2);
        assert_eq!(names(&c), ["only_b"]);
        assert_eq!(&MAIN[c.start as usize..c.end as usize], "only_b");

        // A variant is one of that enum's — `EA`'s `Ready` is not `EB`'s.
        let variants =
            |needle: &str, into: u32| ids(&complete(&s, &main, 1, at(MAIN, needle, 0, 0) + into));
        let ea = at(MAIN, "enum EA", 0, 0);
        let eb = at(MAIN, "enum EB", 0, 0);
        assert_eq!(
            variants("EA.Ready()", 3),
            [
                id("OnlyA", "variant", "OnlyA()", "main.nz", ea + 17),
                id("Ready", "variant", "Ready()", "main.nz", ea + 10),
            ]
        );
        assert_eq!(
            variants("EB.Ready()", 3),
            [
                id("OnlyB", "variant", "OnlyB()", "main.nz", eb + 17),
                id("Ready", "variant", "Ready()", "main.nz", eb + 10),
            ]
        );
        // A generic enum's payloads, substituted; a variant's payload shape as its detail.
        let opt = variants("Opt[Int].Some", 9);
        assert_eq!(
            opt.iter()
                .map(|(n, k, d, _)| (n.as_str(), *k, d.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Nothing", "variant", "Nothing()"),
                ("Some", "variant", "Some(value: Int)")
            ]
        );
        let state = variants("State.Done(code: 1", 6);
        assert_eq!(
            state
                .iter()
                .map(|(n, _, d, _)| (n.as_str(), d.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Done", "Done(code: Int, value: Int)"),
                ("Failed", "Failed(value: Int, why: Str)"),
                ("Idle", "Idle()")
            ]
        );

        // A payload label is one of that variant's, and one given elsewhere is not offered.
        let done = at(MAIN, "Done(code: Int", 0, 0);
        let c = complete(&s, &main, 1, at(MAIN, "code: 1", 0, 0));
        assert_eq!(c.context, Context::PayloadLabel);
        assert_eq!(
            ids(&c),
            [id(
                "code",
                "payload field",
                "code: Int",
                "main.nz",
                done + 5
            )]
        );
        let c = complete(&s, &main, 1, at(MAIN, "value: 2", 0, 0));
        assert_eq!(
            ids(&c),
            [id(
                "value",
                "payload field",
                "value: Int",
                "main.nz",
                done + 16
            )]
        );
        // In a pattern too: `Failed`'s `value`, not `Done`'s.
        let failed = at(MAIN, "Failed(why", 0, 0);
        let c = complete(&s, &main, 1, at(MAIN, "value: v) => v", 0, 0));
        assert_eq!(
            ids(&c),
            [id(
                "value",
                "payload field",
                "value: Int",
                "main.nz",
                failed + 17
            )]
        );
        // A pattern's binding is not a label: `v` and `c` are locals.
        assert_eq!(
            s.completion(&main, 1, at(MAIN, "c, value: v)", 0, 0))
                .unwrap_err(),
            NoCompletion::NotACompletionSite
        );

        // A record label is one of the constructed record's: `Box` the record, never the
        // function; `Box(1)` is a call and its argument no label.
        let c = complete(&s, &main, 1, at(MAIN, "value: 5", 0, 0));
        assert_eq!(c.context, Context::RecordLabel);
        assert_eq!(
            ids(&c),
            [id(
                "value",
                "field",
                "value: Int",
                "main.nz",
                at(MAIN, "struct Box { value", 0, 13)
            )]
        );
        assert_eq!(
            s.structure_at(&main, 1, at(MAIN, "Box(1)", 0, 1))
                .unwrap_err(),
            NoStructure::NotAStructureSite
        );
        // Every candidate keeps its identity.
        let c = complete(&s, &main, 1, at(MAIN, "value: 5", 0, 0));
        assert!(matches!(
            c.items[0].candidate,
            Candidate::Entity(ReferenceTarget::Field(_))
        ));
        let site = s.structure_at(&main, 1, at(MAIN, "code: 1", 0, 0)).unwrap();
        assert!(matches!(
            site.site,
            StructureSite::PayloadLabel { pattern: false, .. }
        ));
        let site = s.structure_at(&main, 1, at(MAIN, "code: c", 0, 0)).unwrap();
        assert!(matches!(
            site.site,
            StructureSite::PayloadLabel { pattern: true, .. }
        ));
    });
}

// ---------------------------------------------------------------------------------------
// Labels given elsewhere, the label being edited, and what does not resolve.
// ---------------------------------------------------------------------------------------

#[test]
fn a_label_given_elsewhere_is_not_offered_and_the_one_being_edited_is() {
    let t = "struct R { a: Int, b: Int, c: Int, }\n\
fn main() -> Int { let r = R(a: 1, bb: 2, c: 3); r.a }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, t.into());
        // On the unknown `bb`: `a` and `c` are given, so `b` — with `bb` itself replaced.
        let c = complete(&s, &m, 1, at(t, "bb:", 0, 0));
        assert_eq!(names(&c), ["b"]);
        assert_eq!(&t[c.start as usize..c.end as usize], "bb");
        let c = complete(&s, &m, 1, at(t, "bb:", 0, 1));
        assert_eq!(names(&c), ["b"]);
        // On `a`: `a` is the label being edited, so it stays; `c` is given elsewhere.
        assert_eq!(names(&complete(&s, &m, 1, at(t, "a: 1", 0, 0))), ["a", "b"]);
        assert_eq!(names(&complete(&s, &m, 1, at(t, "c: 3", 0, 0))), ["b", "c"]);
    });
}

#[test]
fn what_does_not_resolve_is_offered_what_its_anchor_has_or_nothing() {
    let t = "struct A { value: Int, only_a: Int, }\n\
enum EA { Ready, OnlyA, }\n\
fn main() -> Int {\n    \
    let a = A(value: 1, only_a: 2);\n    \
    let x = a.zz + EA.Rea + EA.Re().zz + A(xx: 1, value: 2, only_a: 3).value;\n    \
    let y = unknown.zz + Nope(xx: 1) + Nope.Va() + 3.zz;\n    \
    0\n\
}\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, t.into());
        let fields = ["only_a", "value"];
        // An unknown member of a known receiver: that receiver's fields.
        assert_eq!(names(&complete(&s, &m, 1, at(t, "zz +", 0, 0))), fields);
        // `EA.` with no `(` yet: `EA` is no value, and the checker found it names `EA`.
        let c = complete(&s, &m, 1, at(t, "Rea +", 0, 0));
        assert_eq!(c.context, Context::Variant);
        assert_eq!(names(&c), ["OnlyA", "Ready"]);
        // An unknown variant with its parentheses: the enum it was looked for in.
        assert_eq!(
            names(&complete(&s, &m, 1, at(t, "Re()", 0, 0))),
            ["OnlyA", "Ready"]
        );
        // An unknown label of a known construction: the record's fields not given.
        assert_eq!(
            names(&complete(&s, &m, 1, at(t, "xx: 1, value", 0, 0))),
            Vec::<&str>::new()
        );
        // Nothing the checker could anchor has an answer, whatever it is spelled.
        for needle in ["zz + Nope", "xx: 1)", "Va()", "zz;"] {
            assert_eq!(
                s.completion(&m, 1, at(t, needle, 0, 0)).unwrap_err(),
                NoCompletion::Unanchored,
                "{needle}"
            );
        }
    });
}

// ---------------------------------------------------------------------------------------
// Constructor signature help.
// ---------------------------------------------------------------------------------------

const CTOR: &str = "struct Point { x: Int, label: Str, }\n\
struct Inner { value: Int, }\n\
struct Outer { inner: Inner, count: Int, }\n\
struct Box[T] { value: T, }\n\
enum State { Done(code: Int), Idle, }\n\
enum Opt[T] { Nothing, Some(value: T), }\n\
fn foo(n: Int) -> Int { n }\n\
fn keep(p: Point) -> Int { 0 }\n\
fn main() -> Int {\n    \
    let p = Point(x: foo(1), label: \"a, (b\");\n    \
    let o = Outer(inner: Inner(value: 2), count: 3);\n    \
    let b = Box[Int](value: 4);\n    \
    let s = State.Done(code: 5);\n    \
    let q = Opt[Int].Some(value: 6);\n    \
    let r = Point(xx: 7, label: \"c\");\n    \
    keep(Point(x: 8, label: \"d\")) + b.value\n\
}\n";

fn shown(h: &Help) -> (String, Option<u32>, &'static str) {
    match h {
        Help::Call(c) => (c.rendered.label.clone(), c.active_parameter, "call"),
        Help::Constructor(c) => (c.rendered.label.clone(), c.active_parameter, "constructor"),
    }
}

#[test]
fn constructor_signature_help_follows_the_label_not_the_order() {
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 1, CTOR.into());
        let help = |offset: u32| shown(&s.help_at(&m, 1, offset).unwrap());
        let point = "Point(label: Str, x: Int)";
        // Canonical order: `label` before `x`, whatever the declaration or the call says.
        // `x` is written first and is parameter 1; `label` written second is parameter 0.
        assert_eq!(
            help(at(CTOR, "x: foo", 0, 1)),
            (point.into(), Some(1), "constructor")
        );
        assert_eq!(
            help(at(CTOR, "\"a, (b\"", 0, 3)),
            (point.into(), Some(0), "constructor")
        );
        assert_eq!(
            help(at(CTOR, "label: \"a", 0, 0)),
            (point.into(), Some(0), "constructor")
        );
        // A call inside an initialiser is the call; after its `)` the construction again.
        assert_eq!(
            help(at(CTOR, "foo(1)", 0, 4)),
            ("fn foo(n: Int) -> Int".into(), Some(0), "call")
        );
        assert_eq!(
            help(at(CTOR, "foo(1)", 0, 6)),
            (point.into(), Some(1), "constructor")
        );
        // Between initialisers: the construction, with no field active.
        assert_eq!(
            help(at(CTOR, ", label: \"a", 0, 1)),
            (point.into(), None, "constructor")
        );
        // An unknown label: the shape, and no field guessed.
        assert_eq!(
            help(at(CTOR, "xx: 7", 0, 4)),
            (point.into(), None, "constructor")
        );
        // Nested constructions: the innermost, then the outer at its own field.
        assert_eq!(
            help(at(CTOR, "value: 2", 0, 7)),
            ("Inner(value: Int)".into(), Some(0), "constructor")
        );
        assert_eq!(
            help(at(CTOR, "value: 2)", 0, 9)),
            (
                "Outer(count: Int, inner: Inner)".into(),
                Some(1),
                "constructor"
            )
        );
        // Instantiated generics, and variants.
        assert_eq!(
            help(at(CTOR, "value: 4", 0, 7)),
            ("Box[Int](value: Int)".into(), Some(0), "constructor")
        );
        assert_eq!(
            help(at(CTOR, "code: 5", 0, 6)),
            ("State.Done(code: Int)".into(), Some(0), "constructor")
        );
        assert_eq!(
            help(at(CTOR, "value: 6", 0, 7)),
            ("Opt[Int].Some(value: Int)".into(), Some(0), "constructor")
        );
        // A construction inside a call's argument is the construction.
        assert_eq!(
            help(at(CTOR, "x: 8", 0, 3)),
            (point.into(), Some(1), "constructor")
        );
        assert_eq!(
            help(at(CTOR, "keep(Point", 0, 5)),
            ("fn keep(p: Point) -> Int".into(), Some(0), "call")
        );
        // The structured answer: a record, not a call, with identities.
        let Help::Constructor(c) = s.help_at(&m, 1, at(CTOR, "code: 5", 0, 6)).unwrap() else {
            panic!("a construction");
        };
        assert!(matches!(c.constructor, Constructor::Variant(_)));
        assert!(matches!(
            c.active_field,
            Some(ReferenceTarget::PayloadField(_))
        ));
        // Outside every parenthesis: nothing.
        assert_eq!(
            s.help_at(&m, 1, at(CTOR, "let o", 0, 0)).unwrap_err(),
            NoCall::NotInACall
        );
    });
}

#[test]
fn reordering_declarations_changes_no_structure_answer() {
    let one = "struct P { a: Int, b: Str, }\nenum E { Y(q: Int, p: Str), X, }\n\
fn main() -> Int { let v = P(a: 1, b: \"s\"); let e = E.Y(q: 1, p: \"t\"); v.a }\n";
    let two = "struct P { b: Str, a: Int, }\nenum E { X, Y(p: Str, q: Int), }\n\
fn main() -> Int { let v = P(a: 1, b: \"s\"); let e = E.Y(q: 1, p: \"t\"); v.a }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let mut seen = Vec::new();
        for (v, t) in [(1, one), (2, two)] {
            s.change(&m, v, t.into());
            let shape = |c: Completion| -> Vec<(String, String)> {
                c.items
                    .into_iter()
                    .map(|i| (i.name, i.detail.unwrap_or_default()))
                    .collect()
            };
            let help = |offset: u32| shown(&s.help_at(&m, v, offset).unwrap());
            seen.push((
                shape(complete(&s, &m, v, at(t, "a }", 0, 0))),
                shape(complete(&s, &m, v, at(t, "E.Y(q", 0, 2))),
                shape(complete(&s, &m, v, at(t, "q: 1", 0, 0))),
                help(at(t, "a: 1", 0, 3)),
                help(at(t, "p: \"t\"", 0, 3)),
            ));
        }
        assert_eq!(seen[0], seen[1]);
        assert_eq!(
            seen[0].3,
            ("P(a: Int, b: Str)".into(), Some(0), "constructor")
        );
        assert_eq!(
            seen[0].4,
            ("E.Y(p: Str, q: Int)".into(), Some(0), "constructor")
        );
    });
}

// ---------------------------------------------------------------------------------------
// Completeness: current source only.
// ---------------------------------------------------------------------------------------

#[test]
fn a_recovery_is_answered_around_only_when_it_cannot_change_the_answer() {
    let clean = "struct A { value: Int, }\nfn f() -> Int { 1 }\nfn main() -> Int { let a = A(value: 1); a.value }\n";
    let elsewhere = "struct A { value: Int, }\nfn f() -> Int { let y = ; 1 }\nfn main() -> Int { let a = A(value: 1); a.value }\n";
    let same = "struct A { value: Int, }\nfn f() -> Int { 1 }\nfn main() -> Int { let y = ; let a = A(value: 1); a.value }\n";
    let top = "struct A { value: Int, }\nstruct Q { a: }\nfn main() -> Int { let a = A(value: 1); a.value }\n";
    let p = Project::new(&[("lib.nz", "pub fn g() -> Int { 1 }\n")]);
    let (m, lib) = (p.path("main.nz"), p.path("lib.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let member = |t: &str| at(t, "value }", 0, 0);
        s.open(&m, 1, clean.into());
        assert_eq!(names(&complete(&s, &m, 1, member(clean))), ["value"]);
        // A recovered statement in another function's body: answered.
        s.change(&m, 2, elsewhere.into());
        assert_eq!(names(&complete(&s, &m, 2, member(elsewhere))), ["value"]);
        assert!(s.help_at(&m, 2, at(elsewhere, "value: 1", 0, 7)).is_ok());
        // In the site's own function, or at the top level: refused, and not version 1's.
        s.change(&m, 3, same.into());
        assert_eq!(
            s.completion(&m, 3, member(same)).unwrap_err(),
            NoCompletion::RecoveredSyntax
        );
        assert_eq!(
            s.help_at(&m, 3, at(same, "value: 1", 0, 7)).unwrap_err(),
            NoCall::RecoveredSyntax
        );
        s.change(&m, 4, top.into());
        assert_eq!(
            s.completion(&m, 4, member(top)).unwrap_err(),
            NoCompletion::RecoveredSyntax
        );
        // In another file of the compilation: refused.
        let importing = format!("use \"lib.nz\";\n{clean}");
        s.change(&m, 5, importing.clone());
        assert_eq!(names(&complete(&s, &m, 5, member(&importing))), ["value"]);
        s.open(&lib, 1, "pub fn g( {\n".into());
        assert_eq!(
            s.completion(&m, 5, member(&importing)).unwrap_err(),
            NoCompletion::RecoveredSyntax
        );
        s.close(&lib);
        // Repaired: answered again. And an old version never is.
        assert_eq!(names(&complete(&s, &m, 5, member(&importing))), ["value"]);
        assert_eq!(
            s.completion(&m, 1, member(clean)).unwrap_err(),
            NoCompletion::NotCurrent
        );
        assert_eq!(
            s.structure_at(&m, 1, member(clean)).unwrap_err(),
            NoStructure::NotCurrent
        );
        assert_eq!(s.help_at(&m, 1, 0).unwrap_err(), NoCall::NotCurrent);
    });
}

#[test]
fn an_unsaved_imported_structure_is_what_the_importer_sees() {
    let lib = "pub struct T { a: Int, }\npub enum K { One, }\nstruct Hidden { h: Int, }\n";
    let main = "use \"lib.nz\";\nfn main() -> Int { let t = T(a: 1); let k = K.One(); let z = Hidden(h: 1); t.a }\n";
    let p = Project::new(&[("lib.nz", lib), ("main.nz", main)]);
    let (pl, pm) = (p.path("lib.nz"), p.path("main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&pm, 1, main.into());
        let member = at(main, "a }", 0, 0);
        let variant = at(main, "One()", 0, 0);
        let label = at(main, "a: 1", 0, 0);
        assert_eq!(names(&complete(&s, &pm, 1, member)), ["a"]);
        assert_eq!(names(&complete(&s, &pm, 1, variant)), ["One"]);
        // A private record of the dependency is not the importer's to construct.
        assert_eq!(
            s.completion(&pm, 1, at(main, "h: 1", 0, 0)).unwrap_err(),
            NoCompletion::Unanchored
        );
        s.open(
            &pl,
            1,
            "pub struct T { a: Int, extra: Str, }\npub enum K { One, Two, }\nstruct Hidden { h: Int, }\n"
                .into(),
        );
        assert_eq!(names(&complete(&s, &pm, 1, member)), ["a", "extra"]);
        assert_eq!(names(&complete(&s, &pm, 1, variant)), ["One", "Two"]);
        assert_eq!(names(&complete(&s, &pm, 1, label)), ["a", "extra"]);
        let Help::Constructor(c) = s.help_at(&pm, 1, label + 3).unwrap() else {
            panic!("a construction");
        };
        assert_eq!(c.rendered.label, "T(a: Int, extra: Str)");
        s.close(&pl);
        assert_eq!(names(&complete(&s, &pm, 1, member)), ["a"]);
        assert_eq!(names(&complete(&s, &pm, 1, variant)), ["One"]);
    });
}

#[test]
fn another_open_program_does_not_supply_a_structure() {
    let one = "struct S { mine: Int, }\nfn main() -> Int { let s = S(mine: 1); s.mine }\n";
    let two = "struct S { theirs: Int, }\nfn main() -> Int { let s = S(theirs: 1); s.theirs }\n";
    let p = Project::new(&[("one/main.nz", one), ("two/main.nz", two)]);
    let (a, b) = (p.path("one/main.nz"), p.path("two/main.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&a, 1, one.into());
        s.open(&b, 1, two.into());
        assert_eq!(
            names(&complete(&s, &a, 1, at(one, "mine }", 0, 0))),
            ["mine"]
        );
        assert_eq!(
            names(&complete(&s, &b, 1, at(two, "theirs }", 0, 0))),
            ["theirs"]
        );
        assert_eq!(
            names(&complete(&s, &a, 1, at(one, "mine: 1", 0, 0))),
            ["mine"]
        );
    });
}

#[test]
fn a_thousand_edits_leave_one_current_structure() {
    let with = "struct R { a: Int, b: Int, }\nenum E { X, Y(p: Int), }\n\
fn main() -> Int { let r = R(a: 1, b: 2); let e = E.Y(p: 1); r.a }\n";
    let without = "struct R { a: Int, }\nenum E { X, Y(q: Str), }\n\
fn main() -> Int { let r = R(a: 1); let e = E.Y(q: \"s\"); r.zz }\n";
    let p = Project::new(&[]);
    let m = p.path("m.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&m, 0, without.into());
        let mut after_warmup = ((0, 0, 0), 0, 0);
        for version in 1..=1000 {
            let (text, fields, payload): (&str, &[&str], &[&str]) = if version % 2 == 0 {
                (without, &["a"], &["q"])
            } else {
                (with, &["a", "b"], &["p"])
            };
            s.change(&m, version, text.into());
            let member = at(text, "r.", 0, 2);
            assert_eq!(
                names(&complete(&s, &m, version, member)),
                fields,
                "version {version}"
            );
            let label = at(text, "E.Y(", 0, 4);
            assert_eq!(
                names(&complete(&s, &m, version, label)),
                payload,
                "version {version}"
            );
            if version > 1 {
                assert_eq!(
                    s.completion(&m, version - 1, member).unwrap_err(),
                    NoCompletion::NotCurrent
                );
            }
            if version == 10 {
                after_warmup = (s.retained(), s.calls(), s.unresolved_members());
            }
        }
        assert_eq!(
            (s.retained(), s.calls(), s.unresolved_members()),
            after_warmup
        );
    });
}

// ---------------------------------------------------------------------------------------
// The repository's own source.
// ---------------------------------------------------------------------------------------

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for e in entries {
        if e.is_dir() {
            out.extend(walk(&e));
        } else if e.extension().is_some_and(|x| x == "nz") {
            out.push(e);
        }
    }
    out
}

#[test]
fn every_resolved_member_is_among_what_its_site_offers() {
    // Over real source: wherever the checker resolved a field after `.`, a variant after
    // `E.`, or a construction's or pattern's label, completion at that name offers exactly
    // that identity, once. Paired with the exact-set tests above, which catch an extra one.
    use nazm_core::__syntax::SyntaxKind;
    let root = workspace();
    // The compiler's files that hold its records and enums, each the root of its own
    // compilation, so each structure is asked about in the document it is written in.
    let mut roots: Vec<PathBuf> = ["analyse.nz", "emit.nz", "module.nz", "lex.nz"]
        .iter()
        .map(|f| root.join("compiler").join(f))
        .collect();
    for e in walk(&root.join("compiler/conformance/modules")) {
        if e.file_name().is_some_and(|n| n == "main.nz") {
            roots.push(e);
        }
    }
    roots.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let mut counts = [0usize; 4];
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
            let text = std::fs::read_to_string(path).unwrap();
            let mut service = LanguageService::new();
            service.open(path, 1, text.clone());
            let cst = nazm_core::__syntax::parse_cst(nazm_span::FileId::ROOT, &text).cst;
            for node in cst.nodes() {
                let leaves: Vec<_> = node
                    .child_leaves()
                    .into_iter()
                    .filter(|l| !l.kind.is_trivia())
                    .collect();
                for (i, l) in leaves.iter().enumerate() {
                    if l.kind != SyntaxKind::Ident {
                        continue;
                    }
                    let after_dot = i > 0 && leaves[i - 1].kind == SyntaxKind::Dot;
                    let labelled = leaves
                        .get(i + 1)
                        .is_some_and(|n| n.kind == SyntaxKind::Colon);
                    let (slot, expected) = match node.kind() {
                        SyntaxKind::FieldExpr | SyntaxKind::Place if after_dot => {
                            (0, res.field(l.span).map(ReferenceTarget::Field))
                        }
                        SyntaxKind::VariantExpr | SyntaxKind::Pattern if after_dot => {
                            (1, res.variant(l.span).map(ReferenceTarget::Variant))
                        }
                        SyntaxKind::FieldInit if i == 0 && labelled => (
                            2,
                            res.field(l.span)
                                .map(ReferenceTarget::Field)
                                .or(res.payload(l.span).map(ReferenceTarget::PayloadField)),
                        ),
                        SyntaxKind::PatternField if i == 0 && labelled => {
                            (3, res.payload(l.span).map(ReferenceTarget::PayloadField))
                        }
                        _ => continue,
                    };
                    let expected = expected.unwrap_or_else(|| {
                        panic!("unresolved at {:?} in {}", l.span, path.display())
                    });
                    let c = service
                        .completion(path, 1, l.span.start)
                        .unwrap_or_else(|e| {
                            panic!("no completion at {:?} in {}: {e:?}", l.span, path.display())
                        });
                    let found = c
                        .items
                        .iter()
                        .filter(|i| i.candidate == Candidate::Entity(expected))
                        .count();
                    assert_eq!(
                        found,
                        1,
                        "{expected:?} at {:?} in {}",
                        l.span,
                        path.display()
                    );
                    counts[slot] += 1;
                }
            }
        }
        eprintln!(
            "members {}, variants {}, construction labels {}, pattern labels {}",
            counts[0], counts[1], counts[2], counts[3]
        );
        assert!(counts.iter().all(|c| *c > 0), "{counts:?}");
    });
}
