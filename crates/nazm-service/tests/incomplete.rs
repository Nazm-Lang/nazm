//! Completion at a structured hole — right after a `.` or `(` with nothing written — through
//! the language service. N26.
//!
//! Every expected candidate list is written out, by name, detail and where its definition
//! is declared, so a field of another record, a variant of another enum, a label of another
//! variant or a construction guessed for a call fails. Every hole's range is checked to be
//! the empty range at the cursor.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use nazm_core::__syntax::PARSE_STACK_BYTES;
use nazm_service::{Completion, Context, LanguageService, NoCompletion};

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
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

/// The byte offset just after the `n`th occurrence of `needle`.
fn after(text: &str, needle: &str, n: usize) -> u32 {
    let mut from = 0;
    let mut end = 0;
    for _ in 0..=n {
        let i = text[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} #{n}"))
            + from;
        end = i + needle.len();
        from = end;
    }
    u32::try_from(end).unwrap()
}

/// A completion as the tests compare it: context, and each candidate's name and detail.
fn shown(c: &Completion) -> (Context, Vec<(String, String)>) {
    (
        c.context,
        c.items
            .iter()
            .map(|i| (i.name.clone(), i.detail.clone().unwrap_or_default()))
            .collect(),
    )
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

/// Completion at the hole just after the `n`th `needle` of `main.nz`, opened alone with
/// `files` on disk — asserting the range is empty at the cursor.
fn hole(
    files: &[(&str, &str)],
    needle: &str,
    n: usize,
) -> Result<(Context, Vec<(String, String)>), NoCompletion> {
    let p = Project::new(files);
    let main = p.path("main.nz");
    let text = files[0].1.to_owned();
    let at = after(&text, needle, n);
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, text);
        let c = s.completion(&main, 1, at)?;
        assert_eq!(
            (c.start, c.end),
            (at, at),
            "a hole's edit is empty, at the cursor"
        );
        Ok(shown(&c))
    })
}

const DEFS: &str = "\
struct Point {
    y: Int,
    x: Int,
}
struct Holder {
    inner: Point,
    n: Int,
}
struct Box[T] {
    value: T,
}
enum State {
    Ready,
    Done(code: Int, message: Str),
}
enum Maybe[T] {
    None,
    Some(value: T),
}
fn make_point() -> Point { Point(x: 1, y: 2) }
fn helper(a: Int) -> Int { a }
";

fn with(body: &str) -> String {
    format!("{DEFS}{body}")
}

fn point() -> Vec<(String, String)> {
    pairs(&[("x", "x: Int"), ("y", "y: Int")])
}

// ---------------------------------------------------------------------------------------
// After `.`.
// ---------------------------------------------------------------------------------------

#[test]
fn a_bare_dot_offers_the_receivers_fields_whatever_the_receiver_is() {
    for (body, needle) in [
        // A parameter, in the middle of a function, the statement unfinished.
        (
            "fn f(p: Point) -> Int {\n    let n = p.\n    let z = 1;\n    z\n}\n",
            "p.",
        ),
        // A local, as the body's value.
        (
            "fn f() -> Int {\n    let p = make_point();\n    p.\n}\n",
            "p.",
        ),
        // A temporary: no receiver name to go by.
        (
            "fn f() -> Int {\n    let q = make_point().\n    0\n}\n",
            "make_point().",
        ),
        // A projection chain: the inner record's fields, not the outer's.
        ("fn f(h: Holder) -> Int {\n    h.inner.\n}\n", "h.inner."),
        // Inside a call's arguments.
        ("fn f(p: Point) -> Int {\n    helper(p.\n}\n", "p."),
    ] {
        assert_eq!(
            hole(&[("main.nz", &with(body))], needle, 0),
            Ok((Context::Member, point())),
            "{body}"
        );
    }
    // The outer record, for contrast: its own fields.
    assert_eq!(
        hole(
            &[("main.nz", &with("fn f(h: Holder) -> Int {\n    h.\n}\n"))],
            "h.",
            0
        ),
        Ok((
            Context::Member,
            pairs(&[("inner", "inner: Point"), ("n", "n: Int")])
        ))
    );
    // An applied generic: its instance's fields, substituted.
    assert_eq!(
        hole(
            &[("main.nz", &with("fn f(b: Box[Int]) -> Int {\n    b.\n}\n"))],
            "b.",
            0
        ),
        Ok((Context::Member, pairs(&[("value", "value: Int")])))
    );
    assert_eq!(
        hole(
            &[("main.nz", &with("fn f(b: Box[Str]) -> Int {\n    b.\n}\n"))],
            "b.",
            0
        ),
        Ok((Context::Member, pairs(&[("value", "value: Str")])))
    );
}

#[test]
fn a_bare_dot_after_an_enum_offers_exactly_its_variants() {
    let variants = pairs(&[
        ("Done", "Done(code: Int, message: Str)"),
        ("Ready", "Ready()"),
    ]);
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let s = State.\n    0\n}\n")
            )],
            "State.",
            0
        ),
        Ok((Context::Variant, variants))
    );
    // An applied generic enum: its instance's variants, with the payload substituted.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let m = Maybe[Int].\n    0\n}\n")
            )],
            "Maybe[Int].",
            0
        ),
        Ok((
            Context::Variant,
            pairs(&[("None", "None()"), ("Some", "Some(value: Int)")])
        ))
    );
    // The generic enum with no type arguments: which instance is meant is not written.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let m = Maybe.\n    0\n}\n")
            )],
            "Maybe.",
            0
        ),
        Err(NoCompletion::Unanchored)
    );
}

#[test]
fn a_bare_dot_never_anchors_without_a_value_of_a_record_or_an_enum() {
    for (body, needle) in [
        // A record type is not a value, and has no static fields.
        ("fn f() -> Int {\n    let m = Point.\n    0\n}\n", "Point."),
        // Nothing of that name.
        (
            "fn f() -> Int {\n    let m = unknown.\n    0\n}\n",
            "unknown.",
        ),
        // An enum value: its payload depends on its variant, so it has no fields.
        ("fn f(s: State) -> Int {\n    let m = s.\n    0\n}\n", "s."),
        // An `Int`.
        ("fn f(i: Int) -> Int {\n    let m = i.\n    0\n}\n", "i."),
    ] {
        assert_eq!(
            hole(&[("main.nz", &with(body))], needle, 0),
            Err(NoCompletion::Unanchored),
            "{body}"
        );
    }
    // Two dots: the first is an error the probe does not forgive, so there is no hole.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f(p: Point) -> Int {\n    let n = p..\n    0\n}\n")
            )],
            "p..",
            0
        ),
        Err(NoCompletion::NotACompletionSite)
    );
}

#[test]
fn a_value_and_an_enum_of_one_spelling_resolve_as_the_checker_resolves_them() {
    // `State` is a local here, a `Holder`, and the checker reads a value first: its fields,
    // never the enum's variants, and never both.
    let text = with(
        "fn f() -> Int {\n    let State = Holder(inner: make_point(), n: 1);\n    let m = State.\n    0\n}\n",
    );
    assert_eq!(
        hole(&[("main.nz", &text)], "State.", 0),
        Ok((
            Context::Member,
            pairs(&[("inner", "inner: Point"), ("n", "n: Int")])
        ))
    );
}

#[test]
fn same_named_members_stay_with_their_own_definition() {
    const TWO: &str = "\
struct A {
    value: Int,
    only_a: Int,
}
struct B {
    value: Str,
    only_b: Str,
}
enum EA {
    Ready,
    OnlyA,
}
enum EB {
    Ready,
    OnlyB,
}
enum S {
    Done(value: Int),
    Failed(value: Str, reason: Str),
}
fn f(a: A, b: B) -> Int {
    let x = a.
    let y = b.
    let e = EA.
    let g = EB.
    let d = S.Done(
    0
}
";
    let one = |needle: &str| hole(&[("main.nz", TWO)], needle, 0);
    // Each hole on its own line, with the others left unfinished around it: each is judged
    // in a probe of its own, where only it is the hole.
    let _ = one;
    let only = |cut: &str, needle: &str| {
        let text: String = TWO
            .lines()
            .filter(|l| !l.trim_start().starts_with("let") || l.contains(cut))
            .map(|l| format!("{l}\n"))
            .collect();
        hole(&[("main.nz", &text)], needle, 0)
    };
    assert_eq!(
        only("a.", "a."),
        Ok((
            Context::Member,
            pairs(&[("only_a", "only_a: Int"), ("value", "value: Int")])
        ))
    );
    assert_eq!(
        only("b.", "b."),
        Ok((
            Context::Member,
            pairs(&[("only_b", "only_b: Str"), ("value", "value: Str")])
        ))
    );
    assert_eq!(
        only("EA.", "EA."),
        Ok((
            Context::Variant,
            pairs(&[("OnlyA", "OnlyA()"), ("Ready", "Ready()")])
        ))
    );
    assert_eq!(
        only("EB.", "EB."),
        Ok((
            Context::Variant,
            pairs(&[("OnlyB", "OnlyB()"), ("Ready", "Ready()")])
        ))
    );
    assert_eq!(
        only("S.Done(", "S.Done("),
        Ok((Context::PayloadLabel, pairs(&[("value", "value: Int")])))
    );
}

// ---------------------------------------------------------------------------------------
// After `(`.
// ---------------------------------------------------------------------------------------

#[test]
fn a_bare_paren_offers_labels_only_where_no_function_is_called_so() {
    // A record and no function: its labels, with or without the `)` an editor closes.
    for body in [
        "fn f() -> Int {\n    let p = Point(\n    0\n}\n",
        "fn f() -> Int {\n    let p = Point()\n    0\n}\n",
        "fn f() -> Int {\n    let p = Point();\n    0\n}\n",
    ] {
        assert_eq!(
            hole(&[("main.nz", &with(body))], "Point(", 1),
            Ok((Context::RecordLabel, point())),
            "{body}"
        );
    }
    // An applied generic record: substituted.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let b = Box[Int](\n    0\n}\n")
            )],
            "Box[Int](",
            0
        ),
        Ok((Context::RecordLabel, pairs(&[("value", "value: Int")])))
    );
    // A generic record with no type arguments: which instance is meant is not written.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let b = Box(\n    0\n}\n")
            )],
            "Box(",
            0
        ),
        Err(NoCompletion::Unanchored)
    );
    // A function: its call, which signature help answers.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let h = helper(\n    0\n}\n")
            )],
            "helper(",
            1
        ),
        Err(NoCompletion::NotACompletionSite)
    );
    // Nothing of that name.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let u = Nowhere(\n    0\n}\n")
            )],
            "Nowhere(",
            0
        ),
        Err(NoCompletion::Unanchored)
    );
}

#[test]
fn a_record_and_a_function_of_one_spelling_are_never_guessed_at_a_bare_paren() {
    const BOTH: &str = "\
struct Box {
    value: Int,
}
fn Box(value: Int) -> Int {
    value
}
fn f() -> Int {
    let b = Box(
    0
}
";
    assert_eq!(
        hole(&[("main.nz", BOTH)], "Box(", 1),
        Err(NoCompletion::NotACompletionSite)
    );
    // Once a label is written, it is a construction, and N21 answers as before.
    let labelled = BOTH.replace("let b = Box(\n", "let b = Box(va: 1);\n");
    let p = Project::new(&[("main.nz", &labelled)]);
    let main = p.path("main.nz");
    let at = after(&labelled, "Box(va", 1);
    let c = on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, labelled);
        s.completion(&main, 1, at).unwrap()
    });
    assert_eq!(
        shown(&c),
        (Context::RecordLabel, pairs(&[("value", "value: Int")]))
    );
    assert_eq!(
        c.end - c.start,
        2,
        "a label being written is replaced whole"
    );
    // A function alone.
    const FN_ONLY: &str =
        "fn Point(x: Int) -> Int { x }\nfn f() -> Int {\n    let b = Point(\n    0\n}\n";
    assert_eq!(
        hole(&[("main.nz", FN_ONLY)], "Point(", 1),
        Err(NoCompletion::NotACompletionSite)
    );
    // A generic function and a generic record of one spelling.
    const GENERIC_BOTH: &str = "struct Pair[T] {\n    first: T,\n}\nfn Pair[T](first: T) -> T { first }\nfn f() -> Int {\n    let b = Pair[Int](\n    0\n}\n";
    assert_eq!(
        hole(&[("main.nz", GENERIC_BOTH)], "Pair[Int](", 0),
        Err(NoCompletion::NotACompletionSite)
    );
}

#[test]
fn a_bare_paren_after_a_variant_offers_its_payload_labels() {
    let done = pairs(&[("code", "code: Int"), ("message", "message: Str")]);
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let s = State.Done(\n    0\n}\n")
            )],
            "State.Done(",
            0
        ),
        Ok((Context::PayloadLabel, done.clone()))
    );
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f() -> Int {\n    let m = Maybe[Int].Some(\n    0\n}\n")
            )],
            "Maybe[Int].Some(",
            0
        ),
        Ok((Context::PayloadLabel, pairs(&[("value", "value: Int")])))
    );
    // In a pattern: alone, before `}`, and before another arm.
    for body in [
        "fn f(s: State) -> Int {\n    match s {\n        State.Done(\n    }\n}\n",
        "fn f(s: State) -> Int {\n    match s {\n        State.Done(\n        State.Ready() => 0,\n    }\n}\n",
        "fn f(s: State) -> Int {\n    match s {\n        State.Ready() => 0,\n        State.Done(\n    }\n}\n",
    ] {
        assert_eq!(
            hole(&[("main.nz", &with(body))], "State.Done(", 0),
            Ok((Context::PayloadLabel, done.clone())),
            "{body}"
        );
    }
}

// ---------------------------------------------------------------------------------------
// The same candidates as a name being written, apart from the range.
// ---------------------------------------------------------------------------------------

#[test]
fn a_hole_and_a_name_being_written_give_the_same_candidates_and_differ_in_the_range() {
    let written = with("fn f(p: Point) -> Int {\n    p.x\n}\n");
    let bare = with("fn f(p: Point) -> Int {\n    p.\n}\n");
    let p = Project::new(&[("main.nz", &written)]);
    let main = p.path("main.nz");
    let (w_at, b_at) = (after(&written, "p.", 0), after(&bare, "p.", 0));
    let (named, holed) = on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, written);
        let named = s.completion(&main, 1, w_at).unwrap();
        s.change(&main, 2, bare);
        let holed = s.completion(&main, 2, b_at).unwrap();
        (named, holed)
    });
    // `p.|x`: the name is replaced whole, and the empty prefix at its start keeps every
    // field. `p.|`: nothing to replace.
    assert_eq!((named.start, named.end), (w_at, w_at + 1));
    assert_eq!((holed.start, holed.end), (b_at, b_at));
    assert_eq!(named.context, holed.context);
    let same = |c: &Completion| {
        c.items
            .iter()
            .map(|i| {
                let d = i.declaration.as_ref().unwrap();
                (
                    i.name.clone(),
                    i.kind,
                    i.candidate,
                    i.detail.clone(),
                    (d.path.clone(), d.start, d.end),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        same(&named),
        same(&holed),
        "identity, detail, kind and declaration alike"
    );
    // The partial name `p.x` completes to `x` alone, as before.
    let prefix = with("fn f(p: Point) -> Int {\n    p.x\n}\n");
    let p = Project::new(&[("main.nz", &prefix)]);
    let main = p.path("main.nz");
    let at = after(&prefix, "p.x", 0);
    let c = on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 1, prefix);
        s.completion(&main, 1, at).unwrap()
    });
    assert_eq!(shown(&c), (Context::Member, pairs(&[("x", "x: Int")])));
    assert_eq!((c.start, c.end), (at - 1, at));
}

#[test]
fn reordering_a_declaration_does_not_reorder_a_hole() {
    let a =
        "struct R {\n    b: Int,\n    a: Int,\n    c: Int,\n}\nfn f(r: R) -> Int {\n    r.\n}\n";
    let b =
        "struct R {\n    c: Int,\n    a: Int,\n    b: Int,\n}\nfn f(r: R) -> Int {\n    r.\n}\n";
    let expected = Ok((
        Context::Member,
        pairs(&[("a", "a: Int"), ("b", "b: Int"), ("c", "c: Int")]),
    ));
    assert_eq!(hole(&[("main.nz", a)], "r.", 0), expected);
    assert_eq!(hole(&[("main.nz", b)], "r.", 0), expected);
}

// ---------------------------------------------------------------------------------------
// What the source around the hole must be.
// ---------------------------------------------------------------------------------------

#[test]
fn only_the_hole_is_forgiven() {
    let answer = Ok((Context::Member, point()));
    // Only the hole.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("fn f(p: Point) -> Int {\n    let n = p.\n    0\n}\n")
            )],
            "p.",
            0
        ),
        answer
    );
    // Another error in the same function, before or after it: the function's reading is
    // no longer the checker's, so nothing.
    for body in [
        "fn f(p: Point) -> Int {\n    let bad = ;\n    let n = p.\n    0\n}\n",
        "fn f(p: Point) -> Int {\n    let n = p.\n    let bad = ;\n    0\n}\n",
    ] {
        assert_eq!(
            hole(&[("main.nz", &with(body))], "p.", 0),
            Err(NoCompletion::RecoveredSyntax),
            "{body}"
        );
    }
    // Another function's body: it cannot change this one's locals or the module's names.
    assert_eq!(
        hole(&[("main.nz", &with("fn g() -> Int { let bad = ; 0 }\nfn f(p: Point) -> Int {\n    let n = p.\n    0\n}\n"))], "p.", 0),
        answer
    );
    // At the top level: a declaration may have been lost.
    assert_eq!(
        hole(
            &[(
                "main.nz",
                &with("struct = 3;\nfn f(p: Point) -> Int {\n    let n = p.\n    0\n}\n")
            )],
            "p.",
            0
        ),
        Err(NoCompletion::RecoveredSyntax)
    );
    // In a dependency.
    let main = "use \"lib.nz\";\nfn f(p: Point) -> Int {\n    let n = p.\n    0\n}\n";
    assert_eq!(
        hole(
            &[
                ("main.nz", main),
                (
                    "lib.nz",
                    "pub struct Point {\n    x: Int,\n}\nfn broken() -> Int { let bad = ; 0 }\n"
                )
            ],
            "p.",
            0
        ),
        Err(NoCompletion::RecoveredSyntax)
    );
    assert_eq!(
        hole(
            &[
                ("main.nz", main),
                ("lib.nz", "pub struct Point {\n    x: Int,\n}\n")
            ],
            "p.",
            0
        ),
        Ok((Context::Member, pairs(&[("x", "x: Int")])))
    );
}

#[test]
fn a_dot_that_is_not_punctuation_is_no_hole() {
    for (body, needle) in [
        (
            "fn f(p: Point) -> Int {\n    let s = \"p.\";\n    0\n}\n",
            "\"p.",
        ),
        ("fn f(p: Point) -> Int {\n    // p.\n    0\n}\n", "// p."),
        (
            "fn f(p: Point) -> Int {\n    let s = \"p.x\";\n    0\n}\n",
            "\"p.",
        ),
    ] {
        assert_eq!(
            hole(&[("main.nz", &with(body))], needle, 0),
            Err(NoCompletion::NotAName),
            "{body}"
        );
    }
    // Whitespace, after a `.` elsewhere on the line: no name and no hole there either.
    assert_eq!(
        hole(
            &[("main.nz", &with("fn f(p: Point) -> Int {\n    p.x \n}\n"))],
            "p.x ",
            0
        ),
        Err(NoCompletion::NotAName)
    );
}

#[test]
fn private_and_ambiguous_imports_are_never_anchors() {
    // A private record of a dependency is not a name this module has.
    let main = "use \"lib.nz\";\nfn f() -> Int {\n    let p = Hidden(\n    0\n}\n";
    assert_eq!(
        hole(
            &[
                ("main.nz", main),
                ("lib.nz", "struct Hidden {\n    x: Int,\n}\n")
            ],
            "Hidden(",
            0
        ),
        Err(NoCompletion::Unanchored)
    );
    // Two imports each exporting `Point`: the checker establishes neither.
    let main = "use \"a.nz\";\nuse \"b.nz\";\nfn f() -> Int {\n    let p = Point(\n    0\n}\n";
    assert_eq!(
        hole(
            &[
                ("main.nz", main),
                ("a.nz", "pub struct Point {\n    x: Int,\n}\n"),
                ("b.nz", "pub struct Point {\n    y: Int,\n}\n"),
            ],
            "Point(",
            0
        ),
        Err(NoCompletion::Unanchored)
    );
    // An exported one is.
    let main = "use \"lib.nz\";\nfn f() -> Int {\n    let p = Shown(\n    0\n}\n";
    assert_eq!(
        hole(
            &[
                ("main.nz", main),
                ("lib.nz", "pub struct Shown {\n    x: Int,\n}\n")
            ],
            "Shown(",
            0
        ),
        Ok((Context::RecordLabel, pairs(&[("x", "x: Int")])))
    );
}

// ---------------------------------------------------------------------------------------
// The current source, and only the current source.
// ---------------------------------------------------------------------------------------

#[test]
fn a_hole_follows_the_buffers_and_nothing_is_brought_back() {
    const LIB: &str = "pub struct Point {\n    x: Int,\n}\n";
    const LIB_EDITED: &str = "pub struct Point {\n    x: Int,\n    z: Int,\n}\n";
    let named = "use \"lib.nz\";\nfn f(p: Point) -> Int {\n    p.x\n}\n";
    let bare = "use \"lib.nz\";\nfn f(p: Point) -> Int {\n    p.\n}\n";
    let unknown = "use \"lib.nz\";\nfn f(p: Point) -> Int {\n    unknown.\n}\n";
    let retyped =
        "use \"lib.nz\";\nstruct Other {\n    w: Int,\n}\nfn f(p: Other) -> Int {\n    p.\n}\n";
    let p = Project::new(&[("main.nz", named), ("lib.nz", LIB)]);
    let (main, lib) = (p.path("main.nz"), p.path("lib.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let names = |s: &LanguageService, v: i32, at: u32| {
            s.completion(&main, v, at)
                .map(|c| c.items.into_iter().map(|i| i.name).collect::<Vec<_>>())
        };
        s.open(&main, 1, named.to_owned());
        s.change(&main, 2, bare.to_owned());
        assert_eq!(names(&s, 2, after(bare, "p.", 0)), Ok(vec!["x".to_owned()]));
        // A receiver with no anchor: nothing from the version before.
        s.change(&main, 3, unknown.to_owned());
        assert_eq!(
            names(&s, 3, after(unknown, "unknown.", 0)),
            Err(NoCompletion::Unanchored)
        );
        // An old version is refused, however it is asked.
        assert_eq!(
            names(&s, 2, after(bare, "p.", 0)),
            Err(NoCompletion::NotCurrent)
        );
        // The receiver retyped, unsaved: the new record's fields at once.
        s.change(&main, 4, retyped.to_owned());
        assert_eq!(
            names(&s, 4, after(retyped, "p.", 0)),
            Ok(vec!["w".to_owned()])
        );
        // The dependency, unsaved: a field added.
        s.change(&main, 5, bare.to_owned());
        s.open(&lib, 1, LIB_EDITED.to_owned());
        assert_eq!(
            names(&s, 5, after(bare, "p.", 0)),
            Ok(vec!["x".to_owned(), "z".to_owned()])
        );
        // Closed: the disk again.
        s.close(&lib);
        assert_eq!(names(&s, 5, after(bare, "p.", 0)), Ok(vec!["x".to_owned()]));
    });
}

#[test]
fn another_open_program_lends_a_hole_nothing() {
    // `a.nz` and `b.nz` each declare their own `Point`; neither sees the other's.
    let a = "struct Point {\n    only_a: Int,\n}\nfn f(p: Point) -> Int {\n    p.\n}\n";
    let b = "struct Point {\n    only_b: Int,\n}\nfn f(p: Point) -> Int {\n    p.\n}\n";
    let p = Project::new(&[("a.nz", a), ("b.nz", b)]);
    let (pa, pb) = (p.path("a.nz"), p.path("b.nz"));
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&pa, 1, a.to_owned());
        s.open(&pb, 1, b.to_owned());
        let names = |path: &Path, text: &str| {
            s.completion(path, 1, after(text, "p.", 0))
                .unwrap()
                .items
                .into_iter()
                .map(|i| i.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&pa, a), ["only_a"]);
        assert_eq!(names(&pb, b), ["only_b"]);
    });
}

#[test]
fn a_hole_leaves_the_published_diagnostics_and_the_analysis_as_they_were() {
    // The probe's own diagnostics — an empty field name that does not exist, a construction
    // with no fields — are never published, and nothing about the document's analysis
    // changes by asking.
    let text = with("fn f(p: Point) -> Int {\n    let n = p.\n    0\n}\n");
    let p = Project::new(&[("main.nz", &text)]);
    let main = p.path("main.nz");
    let at = after(&text, "p.", 0);
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        let published = s.open(&main, 1, text.clone());
        let before = (s.retained(), s.indexed(), s.calls(), s.unresolved_members());
        for _ in 0..3 {
            assert!(s.completion(&main, 1, at).is_ok());
        }
        assert_eq!(
            (s.retained(), s.indexed(), s.calls(), s.unresolved_members()),
            before
        );
        assert_eq!(s.text(&main).as_deref(), Some(text.as_str()));
        // One syntax error — the unfinished statement — exactly as `nazm check` reports it.
        let codes: Vec<String> = published[0]
            .diagnostics
            .iter()
            .map(|d| d.code.to_string())
            .collect();
        assert_eq!(codes, ["N0100"], "{:?}", published[0].diagnostics);
    });
}

#[test]
fn a_thousand_edits_leave_only_the_current_hole() {
    const SHAPES: [(&str, &str, &[&str]); 10] = [
        ("fn f(p: Point) -> Int {\n    p.y\n}\n", "p.", &["x", "y"]),
        ("fn f(p: Point) -> Int {\n    p.\n}\n", "p.", &["x", "y"]),
        (
            "fn f(p: Point) -> Int {\n    let n = unknown.\n    0\n}\n",
            "unknown.",
            &[],
        ),
        (
            "fn f() -> Int {\n    let s = State.\n    0\n}\n",
            "State.",
            &["Done", "Ready"],
        ),
        (
            "fn f() -> Int {\n    let s = State.Re\n    0\n}\n",
            "State.",
            &[],
        ),
        (
            "fn f() -> Int {\n    let p = Point(\n    0\n}\n",
            "Point(",
            &["x", "y"],
        ),
        (
            "fn f() -> Int {\n    let p = Point(x: 1)\n    0\n}\n",
            "Point(x: 1)",
            &[],
        ),
        (
            "fn f() -> Int {\n    let s = State.Done(\n    0\n}\n",
            "State.Done(",
            &["code", "message"],
        ),
        (
            "fn f() -> Int {\n    let s = State.Done(code: 1)\n    0\n}\n",
            "State.Done(code: 1)",
            &[],
        ),
        (
            "fn f(h: Holder) -> Int {\n    h.inner.\n}\n",
            "h.inner.",
            &["x", "y"],
        ),
    ];
    let first = with(SHAPES[0].0);
    let p = Project::new(&[("main.nz", &first)]);
    let main = p.path("main.nz");
    on_the_service_stack(move || {
        let mut s = LanguageService::new();
        s.open(&main, 0, first.clone());
        let baseline = (s.retained(), s.indexed(), s.calls(), s.unresolved_members());
        for v in 1..=1000 {
            let k = usize::try_from(v).unwrap() % SHAPES.len();
            let (body, needle, expected) = SHAPES[k];
            let text = with(body);
            s.change(&main, v, text.clone());
            let n = usize::from(needle == "Point(");
            let at = after(&text, needle, n);
            let got: Vec<String> = s
                .completion(&main, v, at)
                .map(|c| c.items.into_iter().map(|i| i.name).collect())
                .unwrap_or_default();
            assert_eq!(got, expected, "version {v}: {needle}");
            assert_eq!(
                s.completion(&main, v - 1, at).map(|_| ()),
                Err(NoCompletion::NotCurrent)
            );
            if k == 0 {
                assert_eq!(
                    (s.retained(), s.indexed(), s.calls(), s.unresolved_members()),
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

/// The name declared at `decl`, read from the loaded text of its file.
fn res_name(res: &nazm_core::Resolution, decl: nazm_span::Span) -> String {
    for r in res.records().all() {
        if let Some(f) = r.fields.iter().find(|f| f.name_span == decl) {
            return f.name.clone();
        }
    }
    for e in res.enums().all() {
        if let Some(v) = e.variants.iter().find(|v| v.name_span == decl) {
            return v.name.clone();
        }
    }
    panic!("nothing declared at {decl:?}")
}

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
fn every_member_and_variant_of_the_repository_is_among_its_holes_candidates() {
    // Over real source: take a resolved `receiver.field`, delete the field's name so the
    // buffer reads `receiver.`, and complete at the hole — the field the checker resolved
    // there must be among the candidates, identified by where it is declared, and every
    // candidate must be a field of that same record. Likewise `E.V(…)` in a construction,
    // with the variant and its payload deleted so the buffer reads `E.`. Sites are sampled
    // evenly, up to a bound per document, since each is a fresh analysis and a probe.
    use nazm_core::__syntax::SyntaxKind;
    let root = workspace();
    let mut files = walk(&root.join("compiler"));
    files.extend(walk(&root.join("examples")));
    on_the_service_stack(move || {
        let (mut documents, mut members, mut variants, mut generic) = (0, 0, 0, 0);
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
            documents += 1;
            let res = &a.resolution;
            let text = std::fs::read_to_string(path).unwrap();
            let cst = nazm_core::__syntax::parse_cst(nazm_span::FileId::ROOT, &text).cst;
            // (delete from, delete to, the declaration the candidates must include, generic)
            let mut sites: Vec<(u32, u32, nazm_span::Span, bool, bool)> = Vec::new();
            for node in cst.root().descendants() {
                let leaves: Vec<_> = node
                    .child_leaves()
                    .into_iter()
                    .filter(|l| !l.kind.is_trivia())
                    .collect();
                match node.kind() {
                    SyntaxKind::FieldExpr => {
                        let Some(name) = leaves.last().filter(|l| l.kind == SyntaxKind::Ident)
                        else {
                            continue;
                        };
                        let Some(f) = res.field(name.span) else {
                            continue;
                        };
                        let def = res.records().get(f.record).unwrap();
                        let decl = def.fields[f.field as usize].name_span;
                        sites.push((
                            name.span.start,
                            name.span.end,
                            decl,
                            def.instance.is_some(),
                            true,
                        ));
                    }
                    SyntaxKind::VariantExpr => {
                        let dot = leaves.iter().position(|l| l.kind == SyntaxKind::Dot);
                        let Some(name) = dot.and_then(|d| leaves.get(d + 1)) else {
                            continue;
                        };
                        let Some(v) = res.variant(name.span) else {
                            continue;
                        };
                        let def = res.enums().get(v.enumeration).unwrap();
                        let decl = def.variants[v.variant as usize].name_span;
                        sites.push((
                            name.span.start,
                            node.span().end,
                            decl,
                            def.instance.is_some(),
                            false,
                        ));
                    }
                    _ => {}
                }
            }
            let take = |kind: bool, bound: usize| {
                let all: Vec<_> = sites.iter().filter(|s| s.4 == kind).copied().collect();
                let step = all.len().div_ceil(bound).max(1);
                all.into_iter()
                    .step_by(step)
                    .take(bound)
                    .collect::<Vec<_>>()
            };
            let mut chosen = take(true, 25);
            chosen.extend(take(false, 12));
            let mut s = LanguageService::new();
            s.open(path, 0, text.clone());
            for (v, (from, to, decl, is_generic, is_member)) in chosen.into_iter().enumerate() {
                let version = i32::try_from(v).unwrap() + 1;
                let cut = format!("{}{}", &text[..from as usize], &text[to as usize..]);
                s.change(path, version, cut);
                let c = s
                    .completion(path, version, from)
                    .unwrap_or_else(|e| panic!("{}:{from}: {e:?}", path.display()));
                assert_eq!((c.start, c.end), (from, from));
                let expected_context = if is_member {
                    Context::Member
                } else {
                    Context::Variant
                };
                assert_eq!(c.context, expected_context);
                let prelude = loaded.graph.module_of(decl.file) == loaded.graph.prelude();
                let name = res_name(&a.resolution, decl);
                if prelude {
                    // The core prelude has no file: its members are named, not located.
                    assert_eq!(
                        c.items.iter().filter(|i| i.name == name).count(),
                        1,
                        "{}:{from}",
                        path.display()
                    );
                    assert!(c.items.iter().all(|i| i.declaration.is_none()));
                } else {
                    let want = loaded.map.name(decl.file);
                    // A declaration after the cut, in the same file, moved back by its length.
                    let shift = |at: u32| {
                        if decl.file == nazm_span::FileId::ROOT && at >= to {
                            at - (to - from)
                        } else {
                            at
                        }
                    };
                    let (start, end) = (shift(decl.start), shift(decl.end));
                    let here = |l: &nazm_service::Location| {
                        l.path.canonicalize().ok() == Path::new(want).canonicalize().ok()
                    };
                    let found = c.items.iter().filter(|i| {
                        i.name == name
                            && i.declaration
                                .as_ref()
                                .is_some_and(|l| l.start == start && l.end == end && here(l))
                    });
                    assert_eq!(found.count(), 1, "{}:{from}", path.display());
                    // Every candidate is a member of the one definition: declared in its file.
                    assert!(c
                        .items
                        .iter()
                        .all(|i| i.declaration.as_ref().is_some_and(here)));
                }
                if is_member {
                    members += 1;
                } else {
                    variants += 1;
                }
                if is_generic {
                    generic += 1;
                }
            }
        }
        eprintln!(
            "{documents} documents: {members} member holes, {variants} variant holes, {generic} of them on generic instances"
        );
        assert!(documents > 0 && members > 0 && variants > 0);
    });
}
